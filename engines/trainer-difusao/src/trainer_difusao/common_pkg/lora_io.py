"""
I/O atômico de adaptadores e pesos LoRA (.safetensors).
"""
from __future__ import annotations

import os
from pathlib import Path
from typing import Any


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
