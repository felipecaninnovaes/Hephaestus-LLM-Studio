"""
I/O atômico de adaptadores e pesos LoRA (.safetensors).
"""
from __future__ import annotations

import os
import shutil
from pathlib import Path
from typing import Any

from trainer_difusao.common_pkg.core import _die

__all__ = [
    "_save_lora_safetensors",
    "_load_lora_weights",
    "_save_optimizer_state",
    "_load_optimizer_state",
    "_override_optimizer_lr",
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
    """Carrega pesos prévios do adaptador LoRA a partir de um arquivo .safetensors.

    O arquivo é gravado por _save_lora_safetensors em formato canônico (chaves
    sem o prefixo PEFT 'base_model.model.' e sem o segmento '.default.'), que é
    o contrato de arquivo com ComfyUI/diffusers e NÃO pode mudar. Já
    set_peft_model_state_dict() exige esse mesmo formato canônico (com prefixo,
    sem '.default.') — não as chaves cruas de named_parameters() do PeftModel.
    Esta função remapeia as chaves normalizadas do arquivo para as chaves
    canônicas do modelo atual (get_peft_model_state_dict), aceitando também
    arquivos que já venham com o prefixo.

    Falha alto (_die) se: o arquivo não existir; sobrar qualquer chave do
    arquivo que não corresponda a nenhum tensor LoRA do modelo atual; faltar
    qualquer tensor LoRA treinável do modelo atual no arquivo; ou houver
    divergência de shape (ex.: rank do LoRA salvo diferente do configurado).
    Nunca treina do zero em silêncio.
    """
    import safetensors.torch
    from peft import get_peft_model_state_dict, set_peft_model_state_dict

    path = Path(weights_path)
    if not path.exists():
        _die(f"weights_path informado para continuação de treino não existe: {path}")

    print(f"[INFO] Carregando pesos LoRA prévios de: {path}", flush=True)
    file_normalized = _normalize_lora_keys(safetensors.torch.load_file(str(path)))

    target_state_dict = get_peft_model_state_dict(model)
    # Mapa chave-normalizada -> chave-canônica exigida por set_peft_model_state_dict.
    normalized_to_canonical = _normalize_lora_keys({k: k for k in target_state_dict})

    remapped: dict[str, Any] = {}
    unexpected_keys: list[str] = []
    for norm_key, tensor in file_normalized.items():
        canonical_key = normalized_to_canonical.get(norm_key)
        if canonical_key is None:
            unexpected_keys.append(norm_key)
            continue
        remapped[canonical_key] = tensor

    if unexpected_keys:
        _die(
            f"Pesos LoRA em {path} têm {len(unexpected_keys)} chave(s) que não "
            f"correspondem a nenhum tensor LoRA do modelo atual: {unexpected_keys[:5]}"
        )

    missing_keys = [k for k in target_state_dict if k not in remapped]
    if missing_keys:
        _die(
            f"Pesos LoRA em {path} não cobrem {len(missing_keys)} tensor(es) LoRA "
            f"treinável(is) do modelo atual: {missing_keys[:5]}"
        )

    for canonical_key, tensor in remapped.items():
        expected_shape = tuple(target_state_dict[canonical_key].shape)
        actual_shape = tuple(tensor.shape)
        if expected_shape != actual_shape:
            _die(
                f"Divergência de shape ao carregar '{canonical_key}' de {path}: "
                f"esperado {expected_shape}, recebido {actual_shape} "
                "(rank do LoRA salvo é diferente do rank configurado?)"
            )

    load_result = set_peft_model_state_dict(model, remapped)
    unexpected_after = [k for k in load_result.unexpected_keys if "lora_" in k]
    if unexpected_after:
        _die(
            f"set_peft_model_state_dict reportou {len(unexpected_after)} chave(s) "
            f"LoRA inesperada(s) ao aplicar {path}: {unexpected_after[:5]}"
        )

    print(
        "[INFO] Pesos LoRA injetados com sucesso no modelo para continuação de "
        f"treino ({len(remapped)} tensores carregados de {path.name}).",
        flush=True,
    )


def _save_optimizer_state(optimizer: Any, path: Path) -> None:
    """Salva o state_dict do optimizer (momentum/variância Adam) de forma atômica via torch.save."""
    import torch

    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp_file = path.parent / f".tmp_{path.name}"
    torch.save(optimizer.state_dict(), str(tmp_file))
    os.replace(tmp_file, path)
    print(f"[INFO] Estado do optimizer salvo em: {path}", flush=True)


# Chaves de estado de optimizer cujo shape espelha o shape do parâmetro (e
# portanto detectam divergência de rank). NUNCA inclui absmax1/absmax2/qmap1/
# qmap2 (bitsandbytes 8-bit): são codebooks/blocos de quantização com shape
# próprio, não o shape do parâmetro, e comparar causaria falso-positivo.
_PARAM_SHAPED_OPTIMIZER_STATE_KEYS = (
    "exp_avg",      # Adam/AdamW
    "exp_avg_sq",   # Adam/AdamW
    "state1",       # bitsandbytes 8-bit (AdamW8bit/paged): momentum
    "state2",       # bitsandbytes 8-bit (AdamW8bit/paged): variância
    "s",            # Prodigy
    "p0",           # Prodigy
)


def _load_optimizer_state(optimizer: Any, path: Path | str) -> bool:
    """Carrega o state_dict de um optimizer prévio. Nunca levanta exceção: retorna
    True/False conforme sucesso e loga warning em caso de arquivo ausente ou
    incompatibilidade (ex.: param_groups diferentes entre retomada e treino original,
    ou shape de estado divergente por mudança de rank de LoRA)."""
    import torch

    path = Path(path)
    if not path.exists():
        print(f"[WARN] Arquivo de estado do optimizer não encontrado: {path}", flush=True)
        return False

    try:
        state_dict = torch.load(str(path), map_location="cpu")

        # Valida compatibilidade ANTES de aplicar, para não estourar no primeiro
        # optimizer.step() com estado de shape divergente (ex.: retomada com rank
        # de LoRA diferente do treino original).
        current_params = [p for group in optimizer.param_groups for p in group["params"]]
        saved_state = state_dict.get("state", {})
        saved_param_groups = state_dict.get("param_groups", [])
        saved_total_params = sum(len(g.get("params", [])) for g in saved_param_groups)

        if saved_param_groups and saved_total_params != len(current_params):
            print(
                f"[WARN] Estado do optimizer em {path} incompatível: {saved_total_params} "
                f"parâmetro(s) salvos != {len(current_params)} parâmetro(s) atuais "
                "(arquitetura ou configuração de LoRA diferente?). Prosseguindo com "
                "optimizer novo.",
                flush=True,
            )
            return False

        for idx, saved_param_state in saved_state.items():
            if idx >= len(current_params):
                continue
            current_shape = tuple(current_params[idx].shape)
            for key in _PARAM_SHAPED_OPTIMIZER_STATE_KEYS:
                saved_tensor = saved_param_state.get(key)
                if saved_tensor is not None and tuple(saved_tensor.shape) != current_shape:
                    print(
                        f"[WARN] Estado do optimizer em {path} incompatível: parâmetro "
                        f"#{idx} '{key}' shape {tuple(saved_tensor.shape)} != atual "
                        f"{current_shape} (rank de LoRA diferente?). Prosseguindo com "
                        "optimizer novo.",
                        flush=True,
                    )
                    return False

        optimizer.load_state_dict(state_dict)
    except Exception as exc:
        print(
            f"[WARN] Falha ao carregar estado do optimizer de {path}: {exc} "
            "(prosseguindo com optimizer novo)",
            flush=True,
        )
        return False

    print(f"[INFO] Estado do optimizer restaurado com sucesso de: {path}", flush=True)
    return True


def _override_optimizer_lr(optimizer: Any, lr: float) -> None:
    """Sobrescreve `lr` e `initial_lr` em todos os param_groups do optimizer.

    Usado logo após restaurar o optimizer de um checkpoint de retomada: o LR
    da nova requisição representa o LR de pico da curva original e sempre
    prevalece sobre o persistido no state_dict restaurado. Quando a retomada
    continua o scheduler, o LR efetivo nos steps seguintes é determinado
    pela posição correspondente na curva original."""
    for group in optimizer.param_groups:
        group["lr"] = lr
        group["initial_lr"] = lr


def save_adapter_checkpoint(
    model: Any,
    checkpoints_dir: Path,
    base_name: str,
    epoch: int,
    metadata: dict[str, str],
    optimizer: Any = None,
) -> Path:
    """Cria checkpoints_dir, grava {base_name}_epoch_{epoch:03d}.safetensors via _save_lora_safetensors e retorna o Path.

    Se `optimizer` for fornecido, salva também o state_dict do optimizer em
    {base_name}_epoch_{epoch:03d}_optimizer.pt no mesmo diretório (continuidade real
    de momentum/variância Adam em retomadas de treino)."""
    checkpoints_dir = Path(checkpoints_dir)
    checkpoints_dir.mkdir(parents=True, exist_ok=True)
    ckpt_file = checkpoints_dir / f"{base_name}_epoch_{epoch:03d}.safetensors"
    _save_lora_safetensors(model, ckpt_file, metadata)
    if optimizer is not None:
        optimizer_file = checkpoints_dir / f"{base_name}_epoch_{epoch:03d}_optimizer.pt"
        _save_optimizer_state(optimizer, optimizer_file)
    return ckpt_file


def save_final_adapter(
    model: Any,
    output_dir: Path,
    base_name: str,
    metadata: dict[str, str],
    optimizer: Any = None,
) -> Path:
    """Salva atomicamente {base_name}.safetensors via _save_lora_safetensors, e se base_name != 'adapter', copia para adapter.safetensors. Retorna o Path final.

    Se `optimizer` for fornecido, salva também {base_name}_optimizer.pt em output_dir,
    e se base_name != 'adapter', copia (atomicamente) para adapter_optimizer.pt,
    espelhando a cópia canônica do safetensors."""
    output_dir = Path(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    final_adapter_file = output_dir / f"{base_name}.safetensors"
    _save_lora_safetensors(model, final_adapter_file, metadata)
    if base_name != "adapter":
        canonical_file = output_dir / "adapter.safetensors"
        tmp_canonical = output_dir / f".tmp_{canonical_file.name}"
        shutil.copy2(final_adapter_file, tmp_canonical)
        os.replace(tmp_canonical, canonical_file)
    if optimizer is not None:
        optimizer_file = output_dir / f"{base_name}_optimizer.pt"
        _save_optimizer_state(optimizer, optimizer_file)
        if base_name != "adapter":
            canonical_optimizer_file = output_dir / "adapter_optimizer.pt"
            tmp_canonical_optimizer = output_dir / f".tmp_{canonical_optimizer_file.name}"
            shutil.copy2(optimizer_file, tmp_canonical_optimizer)
            os.replace(tmp_canonical_optimizer, canonical_optimizer_file)
    return final_adapter_file
