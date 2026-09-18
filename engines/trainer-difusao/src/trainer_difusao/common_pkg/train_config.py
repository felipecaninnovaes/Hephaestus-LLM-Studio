"""
Validação e normalização de parâmetros auxiliares de treino e quantização.
"""
from __future__ import annotations

import hashlib
from pathlib import Path
from typing import Any

from trainer_difusao.common_pkg.core import _die

# Níveis canônicos de quantização aceitos no treino (wire api-principal → engines).
_TRAIN_QUANT_LEVELS = ("none", "2bit", "4bit", "6bit", "8bit")
# Aliases legados que continuam aceitos na validação (compat com configs antigas).
_TRAIN_QUANT_ALIASES = {
    "4bit-nf4": "4bit",
    "nf4": "4bit",
    "8bit-bnb": "8bit",
    "int8": "8bit",
}

# Fração default/máxima de steps com prior-preservation (estilo DreamBooth, 10%).
_CONTROL_RATIO_DEFAULT = 0.1
_CONTROL_RATIO_MAX = 0.5


def _normalize_train_quantization(raw: Any, default: str | None = "none") -> str | None:
    """Normaliza o nível de quantização do treino para o enum canônico."""
    if raw is None or (isinstance(raw, str) and not raw.strip()):
        if default is None:
            return None
        raw = default
    norm = str(raw).strip().lower()
    norm = _TRAIN_QUANT_ALIASES.get(norm, norm)
    if norm not in _TRAIN_QUANT_LEVELS:
        _die(
            f"Quantização de treino inválida: '{raw}'. "
            "Valores aceitos: none, 2bit, 4bit, 6bit, 8bit "
            "(aliases legados 4bit-nf4/8bit-bnb continuam aceitos)."
        )
    return norm


def _validate_train_aux(cfg: dict[str, Any], quant_default: str | None = None) -> dict[str, Any]:
    """Valida as chaves auxiliares do treino vindas do YAML gerado pelo api-principal."""
    if not isinstance(cfg, dict):
        _die("Configuração de treino inválida: raiz deve ser um dicionário.")
    lora_cfg = cfg.get("lora", {}) or {}

    raw_control = cfg.get("control_dataset_path", None)
    if isinstance(raw_control, str) and not raw_control.strip():
        raw_control = None
    control_dataset_path: Path | None = None
    if raw_control is not None:
        control_dataset_path = Path(str(raw_control))
        if not control_dataset_path.exists() or not control_dataset_path.is_dir():
            _die(f"control_dataset_path inválido ou inexistente: {raw_control}")

    raw_ratio = cfg.get("control_ratio", None)
    if raw_ratio is None:
        raw_ratio = lora_cfg.get("control_ratio", _CONTROL_RATIO_DEFAULT)
    try:
        control_ratio = float(raw_ratio)
    except (TypeError, ValueError):
        _die(
            f"control_ratio inválido: {raw_ratio!r}. "
            f"Deve ser float entre 0 e {_CONTROL_RATIO_MAX}."
        )
    if not (0.0 <= control_ratio <= _CONTROL_RATIO_MAX):
        _die(
            f"control_ratio fora do intervalo permitido: {control_ratio}. "
            f"Deve estar entre 0 e {_CONTROL_RATIO_MAX}."
        )

    raw_cache = cfg.get("cache_text_embeddings", None)
    if raw_cache is None:
        raw_cache = lora_cfg.get("cache_text_embeddings", False)
    if isinstance(raw_cache, str):
        cache_text_embeddings = raw_cache.strip().lower() in ("1", "true", "yes")
    else:
        cache_text_embeddings = bool(raw_cache)

    raw_quant = lora_cfg.get("quantization", None)
    if raw_quant is None:
        raw_quant = cfg.get("quantization", None)
    quantization = _normalize_train_quantization(raw_quant, default=quant_default)

    return {
        "control_dataset_path": control_dataset_path,
        "control_ratio": control_ratio,
        "cache_text_embeddings": cache_text_embeddings,
        "quantization": quantization,
    }


def _count_control_images(control_path: Path) -> int:
    """Conta imagens de regularização (mesmas extensões do DiffusionDataset, sem abrir arquivos)."""
    target = control_path / "images"
    if not target.exists():
        target = control_path
    valid_exts = {".webp", ".png", ".jpg", ".jpeg"}
    try:
        return sum(
            1 for p in target.iterdir() if p.is_file() and p.suffix.lower() in valid_exts
        )
    except OSError:
        return 0


def _cycling_batches(loader: Any) -> Any:
    """Iterador infinito sobre um DataLoader (amostragem do control dataset com reposição)."""
    while True:
        for batch in loader:
            yield batch


def _caption_cache_key(caption: str) -> str:
    """Chave de invalidação natural do cache: sha256(caption)[:16]."""
    return hashlib.sha256(caption.encode("utf-8")).hexdigest()[:16]


def _build_intx_torchao_config(quantization: str) -> Any:
    """Constrói o torchao IntxWeightOnlyConfig para quantização 2bit/6bit weight-only."""
    norm = str(quantization).strip().lower()
    if norm not in ("2bit", "6bit"):
        _die(f"Quantização torchao intx inválida: '{quantization}'. Use '2bit' ou '6bit'.")
    try:
        import torch
    except ImportError:
        _die(f"Quantização '{norm}' exige torch instalado (extra 'train').")
    try:
        from torchao.quantization import IntxWeightOnlyConfig
    except ImportError as e:
        _die(f"Quantização '{norm}' exige torchao instalado (extra 'train'): {e}")
    if not torch.cuda.is_available():
        _die(
            f"Quantização '{norm}' (torchao int2/int6 weight-only) exige CUDA "
            "— sem fallback em CPU."
        )
    weight_dtype = torch.int2 if norm == "2bit" else torch.int6
    try:
        return IntxWeightOnlyConfig(weight_dtype=weight_dtype)
    except Exception as e:
        _die(f"Falha ao construir config torchao {norm}: {e}")
