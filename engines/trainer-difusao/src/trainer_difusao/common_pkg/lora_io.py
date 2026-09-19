"""
I/O atômico de adaptadores e pesos LoRA (.safetensors).
"""
from __future__ import annotations

import os
from pathlib import Path
from typing import Any


def _save_lora_safetensors(
    model: Any, output_file: Path, metadata: dict[str, str]
) -> None:
    """Salva os pesos do adaptador LoRA em formato .safetensors canônico de forma atômica."""
    import safetensors.torch
    from peft import get_peft_model_state_dict

    output_file.parent.mkdir(parents=True, exist_ok=True)
    tmp_file = output_file.parent / f".tmp_{output_file.name}"
    lora_state_dict = get_peft_model_state_dict(model)
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
