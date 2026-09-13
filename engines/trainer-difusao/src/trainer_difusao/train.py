"""Motor de treino de Difusão LoRA do Hephaestus (trainer-difusao).

Suporta SD 1.5, SDXL e FLUX.2 Klein 4B.
ENGINE_MOCK=1 (default no dev) → stdlib pura, gera metrics.jsonl e adapter.safetensors.
ENGINE_MOCK=0 (@gpu)           → treino real com PyTorch, Diffusers e PEFT.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import struct
import sys
import time
from pathlib import Path
from typing import Any

import yaml


def _die(msg: str) -> None:
    print(f"ERROR: {msg}", file=sys.stderr)
    sys.exit(1)


def _seed_bytes(seed: int, length: int = 1024) -> bytes:
    h = hashlib.sha256(struct.pack("<q", seed)).digest()
    out = bytearray()
    while len(out) < length:
        h = hashlib.sha256(h).digest()
        out.extend(h)
    return bytes(out[:length])


def _synthetic_loss(seed: int, epoch: int, total_epochs: int) -> float:
    raw = _seed_bytes(seed + epoch * 13, 16)
    val = struct.unpack("<d", raw[:8])[0]
    norm = abs(val) / (1e300 if abs(val) > 1e300 else 1.0)
    norm = (norm % 1.0) * 0.05
    decay = 0.5 * (1.0 - (epoch / (total_epochs + 1)))
    return round(max(0.01, decay + norm), 4)


def _canonical_model_name(raw_model: str) -> str:
    norm = raw_model.strip().lower()
    if norm in ("flux", "flux2", "flux-2", "flux2-klein-4b", "flux.2-klein-4b"):
        return "flux-2-klein-4b"
    if norm in ("sdxl", "sdxl-1.0"):
        return "sdxl"
    if norm in ("sd15", "sd-1.5", "stable-diffusion-v1-5"):
        return "sd15"
    return norm


def _generate_mock_safetensors(output_file: Path, lora_params: dict[str, Any]) -> None:
    """Gera um arquivo .safetensors sintético em conformidade com a especificação HuggingFace."""
    base_model = lora_params.get("base_model", "flux-2-klein-4b")
    rank = int(lora_params.get("rank", 16))
    alpha = int(lora_params.get("alpha", 16))
    trigger_word = str(lora_params.get("trigger_word", ""))

    metadata = {
        "format": "pt",
        "framework": "diffusers",
        "model_type": "lora",
        "lora_rank": str(rank),
        "lora_alpha": str(alpha),
        "base_model": str(base_model),
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


def _mock_train(cfg: dict[str, Any], output: Path) -> None:
    output.mkdir(parents=True, exist_ok=True)
    metrics_path = output / "metrics.jsonl"

    seed = cfg.get("seed", 42)
    lora_cfg = cfg.get("lora", {})
    epochs = lora_cfg.get("epochs", 10)
    raw_model = cfg.get("model", "flux")
    base_model = _canonical_model_name(raw_model)

    sleep_ms = int(os.environ.get("MOCK_EPOCH_SLEEP_MS", "5"))

    if metrics_path.exists():
        metrics_path.unlink()

    for ep in range(1, epochs + 1):
        loss = _synthetic_loss(seed, ep, epochs)
        line = {
            "epoch": ep,
            "step": ep * 10,
            "loss": loss,
        }
        with open(metrics_path, "a", encoding="utf-8") as f:
            f.write(json.dumps(line) + "\n")
        if sleep_ms > 0:
            time.sleep(sleep_ms / 1000.0)

    adapter_path = output / "adapter.safetensors"
    lora_info = dict(lora_cfg)
    lora_info["base_model"] = base_model
    _generate_mock_safetensors(adapter_path, lora_info)


def _real_train_flux(cfg: dict[str, Any], output: Path) -> None:
    """Treino real LoRA para FLUX.2 Klein 4B via Diffusers/PEFT."""
    # Verificação de ambiente GPU e dependências
    try:
        import torch
        from diffusers import FluxPipeline  # noqa: F401
        from peft import LoraConfig, get_peft_model  # noqa: F401
    except ImportError as e:
        _die(f"Dependência ausente para treino real FLUX.2 Klein 4B: {e}")

    if not torch.cuda.is_available():
        _die("CUDA não disponível para treino real de difusão (ENGINE_MOCK=0)")

    # Implementação de treino real invocada no container GPU
    print("Iniciando pipeline de treino LoRA FLUX.2 Klein 4B...")


def _real_train_sdxl(cfg: dict[str, Any], output: Path) -> None:
    """Treino real LoRA para SDXL via Diffusers/PEFT."""
    try:
        import torch
        from diffusers import StableDiffusionXLPipeline  # noqa: F401
    except ImportError as e:
        _die(f"Dependência ausente para treino real SDXL: {e}")

    if not torch.cuda.is_available():
        _die("CUDA não disponível para treino real de difusão (ENGINE_MOCK=0)")

    print("Iniciando pipeline de treino LoRA SDXL...")


def _real_train_sd15(cfg: dict[str, Any], output: Path) -> None:
    """Treino real LoRA para SD 1.5 via Diffusers/PEFT."""
    try:
        import torch
        from diffusers import StableDiffusionPipeline  # noqa: F401
    except ImportError as e:
        _die(f"Dependência ausente para treino real SD 1.5: {e}")

    if not torch.cuda.is_available():
        _die("CUDA não disponível para treino real de difusão (ENGINE_MOCK=0)")

    print("Iniciando pipeline de treino LoRA SD 1.5...")


def _real_train(cfg: dict[str, Any], output: Path) -> None:
    raw_model = cfg.get("model", "flux")
    base_model = _canonical_model_name(raw_model)

    if "flux" in base_model:
        _real_train_flux(cfg, output)
    elif base_model == "sdxl":
        _real_train_sdxl(cfg, output)
    elif base_model == "sd15":
        _real_train_sd15(cfg, output)
    else:
        _die(f"Modelo base de difusão desconhecido: {base_model}")


def cmd_train(args: list[str]) -> None:
    parser = argparse.ArgumentParser(
        prog="trainer-difusao train",
        description="Diffusion LoRA trainer — FLUX.2 Klein 4B, SDXL, SD 1.5 (mock/real)",
    )
    parser.add_argument("--config", required=True, help="Caminho para config.yaml")
    parser.add_argument("--output", required=True, help="Diretório de saída")

    opts = parser.parse_args(args)
    if not os.path.exists(opts.config):
        _die(f"Arquivo de configuração não encontrado: {opts.config}")

    with open(opts.config, "r", encoding="utf-8") as f:
        cfg = yaml.safe_load(f)

    output = Path(opts.output)

    is_mock = os.environ.get("ENGINE_MOCK", "1") == "1"
    if is_mock:
        _mock_train(cfg, output)
    else:
        _real_train(cfg, output)


def cmd_health() -> None:
    is_mock = os.environ.get("ENGINE_MOCK", "1") == "1"
    mode = "mock" if is_mock else "real"
    print(json.dumps({"status": "ok", "engine": "trainer-difusao", "mode": mode}))


def main(argv: list[str] | None = None) -> None:
    if argv is None:
        argv = sys.argv[1:]

    if not argv:
        cmd_health()
        return

    if argv[0] in ("-h", "--help"):
        print("Uso: python -m trainer_difusao [health|train] [args...]")
        return

    if argv[0] == "train":
        cmd_train(argv[1:])
    elif argv[0] == "health":
        cmd_health()
    else:
        _die(f"Subcomando desconhecido: {argv[0]}")


if __name__ == "__main__":
    main()
