"""
I/O atômico de adaptadores e pesos LoRA (.safetensors).
"""
from __future__ import annotations

import os
import shutil
from pathlib import Path
from typing import Any

__all__ = [
    "_save_lora_safetensors",
    "_load_lora_weights",
    "save_adapter_checkpoint",
    "save_final_adapter",
]

_LORA_KEY_PREFIXES_TO_STRIP = (
    "base_model.model.",
    "unet.base_model.model.",
    "transformer.base_model.model.",
)


def _normalize_lora_keys(state_dict: dict[str, Any]) -> dict[str, Any]:
    """Remove prefixos PEFT ('base_model.model.', ...) das chaves do state_dict.

    O PEFT pode emitir chaves como 'base_model.model.transformer_blocks.0.attn...'.
    Consumidores (ComfyUI, loaders diffusers canônicos) esperam as chaves do
    módulo alvo direto. Renomeia e reporta colisões de forma honesta.
    """
    normalized: dict[str, Any] = {}
    for key, value in state_dict.items():
        clean = key
        for prefix in _LORA_KEY_PREFIXES_TO_STRIP:
            if clean.startswith(prefix):
                clean = clean[len(prefix):]
                break
        if clean in normalized and normalized[clean] is not value:
            print(
                f"[WARN] Colisão de chave LoRA ao normalizar: '{key}' -> '{clean}' "
                "(mantendo a primeira ocorrência)",
                flush=True,
            )
            continue
        normalized[clean] = value
    return normalized


def _save_lora_safetensors(
    model: Any, output_file: Path, metadata: dict[str, str]
) -> None:
    """Salva os pesos do adaptador LoRA em formato .safetensors canônico de forma atômica."""
    import safetensors.torch
    from peft import get_peft_model_state_dict

    output_file.parent.mkdir(parents=True, exist_ok=True)
    tmp_file = output_file.parent / f".tmp_{output_file.name}"
    lora_state_dict = _normalize_lora_keys(get_peft_model_state_dict(model))

    # Validação: garante que a injeção LoRA produziu tensores reais.
    # Falha silenciosa (ex: add_adapter em modelo 4-bit) gera arquivo vazio/corrompido.
    lora_a_keys = [k for k in lora_state_dict if k.endswith("lora_A.weight")]
    if not lora_a_keys:
        raise RuntimeError(
            "[LORA-SAVE] get_peft_model_state_dict() retornou 0 tensores lora_A — "
            "a LoRA não foi injetada corretamente no modelo. "
            "Verifique se get_peft_model() foi usado (não add_adapter) em modelos quantizados."
        )
    _actual_rank = lora_state_dict[lora_a_keys[0]].shape[0]
    print(
        f"[LORA-SAVE] {len(lora_a_keys)} módulos lora_A salvos | rank efetivo={_actual_rank} "
        f"| arquivo: {output_file.name}",
        flush=True,
    )
    safetensors.torch.save_file(lora_state_dict, str(tmp_file), metadata=metadata)
    os.replace(tmp_file, output_file)


def _load_lora_weights(model: Any, weights_path: Path | str) -> None:
    """Carrega pesos prévios do adaptador LoRA a partir de um arquivo .safetensors."""
    import safetensors.torch
    from peft import set_peft_model_state_dict

    path = Path(weights_path)
    if not path.exists():
        print(f"[WARN] Arquivo de pesos para continuação não encontrado: {path}", flush=True)
        return

    print(f"[INFO] Carregando pesos LoRA prévios de: {path}", flush=True)
    state_dict = safetensors.torch.load_file(str(path))
    set_peft_model_state_dict(model, state_dict)
    print("[INFO] Pesos LoRA injetados com sucesso no modelo para continuação de treino.", flush=True)


def save_adapter_checkpoint(
    model: Any,
    checkpoints_dir: Path,
    base_name: str,
    epoch: int,
    metadata: dict[str, str],
) -> Path:
    """Cria checkpoints_dir, grava {base_name}_epoch_{epoch:03d}.safetensors via _save_lora_safetensors e retorna o Path."""
    checkpoints_dir = Path(checkpoints_dir)
    checkpoints_dir.mkdir(parents=True, exist_ok=True)
    ckpt_file = checkpoints_dir / f"{base_name}_epoch_{epoch:03d}.safetensors"
    _save_lora_safetensors(model, ckpt_file, metadata)
    return ckpt_file


def save_final_adapter(
    model: Any,
    output_dir: Path,
    base_name: str,
    metadata: dict[str, str],
) -> Path:
    """Salva atomicamente {base_name}.safetensors via _save_lora_safetensors, e se base_name != 'adapter', copia para adapter.safetensors. Retorna o Path final."""
    output_dir = Path(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    final_adapter_file = output_dir / f"{base_name}.safetensors"
    _save_lora_safetensors(model, final_adapter_file, metadata)
    if base_name != "adapter":
        canonical_file = output_dir / "adapter.safetensors"
        tmp_canonical = output_dir / f".tmp_{canonical_file.name}"
        shutil.copy2(final_adapter_file, tmp_canonical)
        os.replace(tmp_canonical, canonical_file)
    return final_adapter_file
