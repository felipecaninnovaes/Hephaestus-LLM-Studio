"""
Helpers básicos de execução, naming e geração sintética de perda.
"""
from __future__ import annotations

import struct
from typing import Any

from engine_kit.mock import seed_bytes as _seed_bytes_impl, synthetic_loss as _synthetic_loss_impl
from engine_kit.runtime import die as _die_impl


def _die(msg: str) -> None:
    _die_impl(msg)


def _seed_bytes(seed: int, length: int = 1024) -> bytes:
    return _seed_bytes_impl(seed, length)


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


def _resolve_output_name(cfg: dict[str, Any]) -> str:
    """Resolve o nome base para os arquivos de pesos (.safetensors)."""
    raw_name = cfg.get("output_name")
    if raw_name and isinstance(raw_name, str) and raw_name.strip():
        name = raw_name.strip()
        if name.endswith(".safetensors"):
            name = name[:-12]
        clean = "".join(c if (c.isalnum() or c in ("-", "_", ".")) else "_" for c in name)
        clean = clean.strip("._-")
        if clean:
            return clean

    return "adapter"
