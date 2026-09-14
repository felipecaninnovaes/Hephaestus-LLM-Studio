"""Pipeline sintético de treino (ENGINE_MOCK=1) para desenvolvimento local e CI."""

from __future__ import annotations

import json
import os
import struct
import time
from pathlib import Path
from typing import Any

from trainer_difusao.common import (
    _canonical_model_name,
    _emit_metric,
    _synthetic_loss,
)
from trainer_difusao.models.base import BaseModelTrainer


def _generate_mock_safetensors(output_file: Path, lora_params: dict[str, Any]) -> None:
    """Gera um arquivo .safetensors sintético em conformidade com a especificação HuggingFace."""
    base_model = lora_params.get("base_model", "flux-2-klein-4b")
    rank = int(lora_params.get("rank", 16))
    alpha = int(lora_params.get("alpha", 16))
    trigger_word = str(lora_params.get("trigger_word", ""))

    quantization = str(lora_params.get("quantization", "4bit"))
    metadata = {
        "format": "pt",
        "framework": "diffusers",
        "model_type": "lora",
        "lora_rank": str(rank),
        "lora_alpha": str(alpha),
        "base_model": str(base_model),
        "quantization": str(quantization),
    }
    if trigger_word:
        metadata["trigger_word"] = trigger_word

    if "flux" in base_model:
        tensor_name = "transformer.single_transformer_blocks.0.linear1.lora_A.weight"
    else:
        tensor_name = "lora_unet_up_blocks_0_attentions_0_proj_in.lora_down.weight"

    tensor_size = rank * 320 * 4
    header = {
        "__metadata__": metadata,
        tensor_name: {
            "dtype": "F32",
            "shape": [rank, 320],
            "data_offsets": [0, tensor_size],
        },
    }
    header_json = json.dumps(header).encode("utf-8")
    header_len = len(header_json)
    pad_len = (8 - (header_len % 8)) % 8
    header_json += b" " * pad_len
    header_len += pad_len

    data_bytes = b"\x00" * tensor_size

    with open(output_file, "wb") as f:
        f.write(struct.pack("<Q", header_len))
        f.write(header_json)
        f.write(data_bytes)


def _generate_mock_sample(
    output_dir: Path, epoch: int, prompt: str, seed: int = 42
) -> None:
    """Gera uma imagem de teste sintética para validação do fluxo de artefatos de sample."""
    samples_dir = output_dir / "samples"
    samples_dir.mkdir(parents=True, exist_ok=True)
    sample_file = samples_dir / f"sample_epoch_{epoch:03d}.png"
    try:
        from PIL import Image, ImageDraw

        img = Image.new("RGB", (512, 512), color=(24, 24, 37))
        draw = ImageDraw.Draw(img)
        draw.rectangle([16, 16, 496, 496], outline=(129, 140, 248), width=3)
        draw.text(
            (32, 210),
            f"Hephaestus Diffusion Sample\nEpoch: {epoch} | Seed: {seed}\nPrompt: {prompt[:50]}",
            fill=(240, 240, 250),
        )
        img.save(sample_file, format="PNG")
    except Exception:
        import base64

        tiny_png = base64.b64decode(
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkWPjfDwAEeQHzG4L5eAAAAABJRU5ErkJggg=="
        )
        sample_file.write_bytes(tiny_png)


def _mock_train(cfg: dict[str, Any], output: Path) -> None:
    """Loop sintético de treino que emite métricas e gera adapter.safetensors com metadata."""
    output.mkdir(parents=True, exist_ok=True)
    metrics_path = output / "metrics.jsonl"

    seed = int(cfg.get("seed", 42))
    lora_cfg = cfg.get("lora", {})
    epochs = int(lora_cfg.get("epochs", 10))
    learning_rate = float(lora_cfg.get("learning_rate", 0.0001))
    raw_model = cfg.get("model", "flux")
    base_model = _canonical_model_name(raw_model)

    samples_cfg = cfg.get("samples", {})
    sample_prompt = str(samples_cfg.get("prompt", "") or "").strip()
    sample_interval = int(samples_cfg.get("interval", 1))
    sample_seed = int(samples_cfg.get("seed", seed))

    sleep_ms = int(os.environ.get("MOCK_EPOCH_SLEEP_MS", "5"))

    if metrics_path.exists():
        metrics_path.unlink()

    # Amostra baseline Época 0 (se configurada)
    if sample_prompt:
        _generate_mock_sample(output, 0, sample_prompt, seed=sample_seed)
        _emit_metric(
            metrics_path,
            epoch=0,
            step=1,
            progress=0.05,
            phase="baseline_ready",
            message="Amostra baseline gerada com sucesso (Época 0).",
        )

    for ep in range(1, epochs + 1):
        loss = _synthetic_loss(seed, ep, epochs)
        progress = round(ep / epochs, 4)
        _emit_metric(
            metrics_path,
            epoch=ep,
            step=ep * 10,
            loss=loss,
            lr=learning_rate,
            progress=progress,
            phase="training",
            message=f"Época {ep}/{epochs} concluída · Loss: {loss}",
        )

        if sample_prompt and sample_interval > 0 and (ep % sample_interval == 0 or ep == epochs):
            _generate_mock_sample(output, ep, sample_prompt, seed=sample_seed)

        if sleep_ms > 0:
            time.sleep(sleep_ms / 1000.0)

    adapter_path = output / "adapter.safetensors"
    lora_info = dict(lora_cfg)
    lora_info["base_model"] = base_model
    lora_info["quantization"] = str(
        lora_cfg.get("quantization") or cfg.get("quantization") or "4bit"
    )
    _generate_mock_safetensors(adapter_path, lora_info)


class MockTrainer(BaseModelTrainer):
    """Trainer sintético para ambiente sem GPU / CI."""

    def train(self, cfg: dict[str, Any], output: Path) -> None:
        _mock_train(cfg, output)
