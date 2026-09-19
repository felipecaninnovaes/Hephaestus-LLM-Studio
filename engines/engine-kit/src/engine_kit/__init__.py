"""
Hephaestus Engine Kit — Primitivas compartilhadas e biblioteca de infraestrutura das engines.
"""
from engine_kit.telemetry import TelemetryEmitter
from engine_kit.mock import (
    is_mock,
    seed_bytes,
    mock_vector,
    synthetic_loss,
    synthetic_yolo_metrics,
    MOCK_MAGIC,
)
from engine_kit.runtime import die, atomic_write, is_cancelled
from engine_kit.vram import vram_allocated_gb, vram_reserved_gb, cleanup_cuda, require_cuda
from engine_kit.httpd import JSONHandlerMixin, run_daemon
from engine_kit.artifacts import prune_checkpoints, make_fake_safetensors, make_fake_artifact

__all__ = [
    "TelemetryEmitter",
    "is_mock",
    "seed_bytes",
    "mock_vector",
    "MOCK_MAGIC",
    "synthetic_loss",
    "synthetic_yolo_metrics",
    "die",
    "atomic_write",
    "is_cancelled",
    "vram_allocated_gb",
    "vram_reserved_gb",
    "cleanup_cuda",
    "require_cuda",
    "JSONHandlerMixin",
    "run_daemon",
    "prune_checkpoints",
    "make_fake_safetensors",
    "make_fake_artifact",
]
