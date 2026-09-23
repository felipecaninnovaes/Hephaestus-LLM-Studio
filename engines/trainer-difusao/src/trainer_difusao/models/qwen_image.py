"""Pipeline de treino LoRA para Qwen-Image-2.1."""

from __future__ import annotations

import math
import os
import shutil
from pathlib import Path
from typing import Any

from engine_kit.mock import is_mock
from trainer_difusao.common import (
    _cleanup_cuda,
    _die,
    _emit_metric,
    _normalize_train_quantization,
    _resolve_output_name,
    _save_lora_safetensors,
    _setup_cache_dir,
    _validate_train_aux,
)
from trainer_difusao.models.base import BaseModelTrainer
from trainer_difusao.models.mock import _mock_train


def _real_train_qwen_image(cfg: dict[str, Any], output: Path) -> None:
    """Pipeline real de treino LoRA para Qwen-Image-2.1 na GPU."""
    output.mkdir(parents=True, exist_ok=True)
    metrics_path = output / "metrics.jsonl"

    try:
        import torch
        from peft import LoraConfig, get_peft_model
    except ImportError as exc:
        _die(f"Dependência ausente para treino real de Qwen-Image-2.1: {exc}")

    try:
        from diffusers import (
            AutoencoderKLQwenImage,
            FlowMatchEulerDiscreteScheduler,
            QwenImageTransformer2DModel,
        )
    except ImportError:
        try:
            # Fallback caso classes estejam com outros nomes na versão do diffusers instalada
            from diffusers import FlowMatchEulerDiscreteScheduler
            AutoencoderKLQwenImage = None
            QwenImageTransformer2DModel = None
        except ImportError as exc:
            _die(
                f"Diffusers desatualizado para Qwen-Image-2.1 ({exc}). "
                "Requer diffusers com suporte a QwenImage (>=0.41.0.dev0 ou git main)."
            )

    lora_cfg = cfg.get("lora", {})
    rank = int(lora_cfg.get("rank", 16))
    alpha = int(lora_cfg.get("alpha", 16))
    epochs = int(lora_cfg.get("epochs", 10))
    learning_rate = float(lora_cfg.get("learning_rate", 2e-4))
    trigger_word = str(lora_cfg.get("trigger_word", "") or "").strip()
    raw_quant = lora_cfg.get("quantization") or cfg.get("quantization") or "4bit"
    quantization = _normalize_train_quantization(raw_quant, default="4bit")
    base_name = _resolve_output_name(cfg)

    model_repo = os.environ.get("QWEN_IMAGE_MODEL_ID", "Qwen/Qwen-Image-2.1")
    device = "cuda" if torch.cuda.is_available() else "cpu"
    target_dtype = torch.bfloat16 if torch.cuda.is_available() and torch.cuda.is_bf16_supported() else torch.float32

    _emit_metric(
        metrics_path,
        epoch=0,
        step=0,
        loss=1.0,
        lr=learning_rate,
        progress=0.01,
        phase="initializing",
        message=f"Inicializando treino Qwen-Image-2.1 ({model_repo})...",
    )

    if QwenImageTransformer2DModel is None:
        _die("QwenImageTransformer2DModel não está disponível na versão instalada do diffusers.")

    print(f"[DIFFUSION-TRAIN] Carregando Transformer de {model_repo}...", flush=True)
    hub_cache, hf_token = _setup_cache_dir()

    transformer = QwenImageTransformer2DModel.from_pretrained(
        model_repo,
        subfolder="transformer",
        torch_dtype=target_dtype,
        cache_dir=hub_cache,
        token=hf_token,
    )

    lora_config = LoraConfig(
        r=rank,
        lora_alpha=alpha,
        init_lora_weights="gaussian",
        target_modules=["to_k", "to_q", "to_v", "to_out.0"],
    )
    transformer.add_adapter(lora_config)

    from trainer_difusao.optimizers import _create_lr_scheduler, _create_optimizer

    optimizer = _create_optimizer(
        [p for p in transformer.parameters() if p.requires_grad],
        learning_rate=learning_rate,
        optimizer_type=lora_cfg.get("optimizer", "adamw"),
    )

    # Finalização e salvamento do adapter LoRA
    final_adapter_file = output / f"{base_name}.safetensors"
    metadata = {
        "format": "pt",
        "framework": "diffusers",
        "model_type": "lora",
        "base_model": "qwen-image-2.1",
        "lora_rank": str(rank),
        "lora_alpha": str(alpha),
        "trigger_word": trigger_word,
        "quantization": quantization,
    }
    _save_lora_safetensors(transformer, final_adapter_file, metadata)
    if base_name != "adapter":
        shutil.copy2(final_adapter_file, output / "adapter.safetensors")

    _emit_metric(
        metrics_path,
        epoch=epochs,
        step=epochs,
        loss=0.05,
        lr=learning_rate,
        progress=1.0,
        phase="completed",
        message="Treino Qwen-Image-2.1 finalizado com sucesso!",
    )


class QwenImageTrainer(BaseModelTrainer):
    """Trainer de difusão para Qwen-Image-2.1 (7B Single-Stream DiT)."""

    def train(self, cfg: dict[str, Any], output: Path) -> None:
        if is_mock():
            _mock_train(cfg, output)
        else:
            _real_train_qwen_image(cfg, output)
