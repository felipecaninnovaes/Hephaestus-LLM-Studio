"""
Primitivas determinísticas para modo MOCK (CI, desenvolvimento local e simulação).
"""
import hashlib
import math
import os
import struct
from typing import Optional

MOCK_MAGIC: bytes = b"HEPHMOCK"

def is_mock(env_val: Optional[str] = None) -> bool:
    """Verifica se o modo MOCK está ativo de forma tolerante (1, true, yes, default 1)."""
    val = env_val if env_val is not None else os.environ.get("ENGINE_MOCK", "1")
    return val.strip().lower() in ("1", "true", "yes")

def seed_bytes(seed: int, length: int) -> bytes:
    """Expansão determinística pseudo-aleatória a partir de uma seed inteira (SHA-256 chain)."""
    h = hashlib.sha256(struct.pack("<q", seed)).digest()
    out = bytearray()
    while len(out) < length:
        out.extend(h)
        h = hashlib.sha256(h).digest()
    return bytes(out[:length])


def mock_vector(payload: bytes, dim: int = 512) -> list[float]:
    """Gera vetor determinístico normalizado L2 para embeddings mock.

    Contrato idêntico ao MockEmbedder em Rust (services/api-principal/src/search/embed.rs).
    """
    h = hashlib.sha256(payload).digest()
    vals = []
    while len(vals) < dim:
        h = hashlib.sha256(h).digest()
        for i in range(4):
            q = struct.unpack_from("<Q", h, i * 8)[0] & ((1 << 53) - 1)
            vals.append((q / float(1 << 53)) * 2.0 - 1.0)
    vals = vals[:dim]
    norm = math.sqrt(sum(x * x for x in vals))
    if norm == 0.0:
        return [0.0] * dim
    return [x / norm for x in vals]


def synthetic_loss(seed: int, epoch: int, total_epochs: int, base_loss: float = 1.0) -> float:
    """Gera curva de perda sintética decrescente suave com pequeno jitter determinístico."""
    b = seed_bytes(seed + epoch * 1000, 4)
    jitter = (struct.unpack("<I", b)[0] / float(0xFFFFFFFF)) * 0.1 - 0.05
    progress = max(0.0, min(1.0, (epoch - 1) / max(1, total_epochs - 1)))
    decay = math.exp(-2.5 * progress)
    loss = base_loss * decay + jitter
    return max(0.01, round(loss, 4))


def synthetic_yolo_metrics(seed: int, epoch: int, total_epochs: int) -> dict[str, float]:
    """Gera métricas sintéticas de treino YOLO (box_loss, cls_loss, dfl_loss, mAP50, mAP50-95)."""
    progress = max(0.0, min(1.0, epoch / max(1, total_epochs)))
    loss = synthetic_loss(seed, epoch, total_epochs, base_loss=0.8)
    map50 = min(0.95, round(0.40 + 0.50 * (1.0 - math.exp(-3.0 * progress)), 4))
    map50_95 = min(0.85, round(map50 * 0.70, 4))
    return {
        "box_loss": loss,
        "cls_loss": round(loss * 0.6, 4),
        "dfl_loss": round(loss * 0.8, 4),
        "mAP50": map50,
        "mAP50-95": map50_95,
    }
