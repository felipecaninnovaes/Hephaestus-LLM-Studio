"""Modo treino simulado (ENGINE_MOCK=1) do trainer-difusao (LoRA).

ADR-0018: leitura de config.yaml, iteração de epochs com métricas sintéticas,
gravação de metrics.jsonl e geração de adapter.safetensors determinístico.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
import sys
import time
from pathlib import Path
from typing import Any

import yaml


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


def _generate_mock_safetensors(output_file: Path, lora_params: dict[str, Any]) -> None:
    """Gera um arquivo .safetensors sintético em conformidade com a especificação HuggingFace."""
    header = {
        "__metadata__": {
            "format": "pt",
            "framework": "diffusers",
            "model_type": "lora",
            "lora_rank": str(lora_params.get("rank", 16)),
            "lora_alpha": str(lora_params.get("alpha", 16)),
            "base_model": str(lora_params.get("base_model", "sdxl")),
        },
        "lora_unet_up_blocks_0_attentions_0_proj_in.lora_down.weight": {
            "dtype": "F32",
            "shape": [16, 320],
            "data_offsets": [0, 16 * 320 * 4],
        },
    }
    header_json = json.dumps(header).encode("utf-8")
    header_len = len(header_json)
    # Alinhamento opcional a 8 bytes
    pad_len = (8 - (header_len % 8)) % 8
    header_json += b" " * pad_len
    header_len += pad_len

    data_bytes = b"\x00" * (16 * 320 * 4)

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
    base_model = cfg.get("model", "sdxl")

    # Limpa metrics.jsonl anterior se existir
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
        time.sleep(0.01)

    adapter_path = output / "adapter.safetensors"
    lora_info = dict(lora_cfg)
    lora_info["base_model"] = base_model
    _generate_mock_safetensors(adapter_path, lora_info)


def cmd_train(args: list[str]) -> None:
    parser = argparse.ArgumentParser(
        prog="trainer-difusao train",
        description="Diffusion LoRA trainer (mock/real)",
    )
    parser.add_argument("--config", required=True, help="Path to config.yaml")
    parser.add_argument("--output", required=True, help="Output directory")

    opts = parser.parse_args(args)
    with open(opts.config, "r", encoding="utf-8") as f:
        cfg = yaml.safe_load(f)

    output = Path(opts.output)
    _mock_train(cfg, output)


def cmd_health() -> None:
    print(json.dumps({"status": "ok", "engine": "trainer-difusao", "mode": "mock"}))


def main(argv: list[str] | None = None) -> None:
    if argv is None:
        argv = sys.argv[1:]

    if not argv:
        cmd_health()
        return

    if argv[0] in ("-h", "--help"):
        print("Usage: python -m trainer_difusao [health|train] [args...]")
        return

    if argv[0] == "train":
        cmd_train(argv[1:])
    elif argv[0] == "health":
        cmd_health()
    else:
        print(f"ERROR: Unknown subcommand {argv[0]}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
