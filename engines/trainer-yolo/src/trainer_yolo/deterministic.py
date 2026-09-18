"""
Operações determinísticas, métricas sintéticas e bounding boxes simuladas para modo mock.
"""
from __future__ import annotations

import hashlib
import struct
from typing import Any

from engine_kit.mock import seed_bytes as _engine_seed_bytes

MOCK_MAGIC = b"HEPHMOCK"
AUTOTRACK_BOX_COUNT_RANGE = (1, 3)


def _seed_bytes(seed: int, length: int = 1024) -> bytes:
    """Expand a single seed int into deterministic bytes via SHA-256 chain."""
    return _engine_seed_bytes(seed, length)


def _synthetic_metrics(seed: int, epoch: int, total_epochs: int) -> dict:
    """Generate one line of synthetic metrics for *epoch* (1-indexed)."""
    raw = _seed_bytes(seed + epoch * 7, 64)
    floats = [struct.unpack_from("<d", raw, i * 8)[0] for i in range(6)]
    normed = [(v % 1.0 + 1.0) % 1.0 for v in floats]
    t = epoch / max(total_epochs, 1)

    return {
        "epoch": epoch,
        "box_loss": round(2.0 * (1.0 - t) + 0.3 * normed[0] * (1.0 - t), 6),
        "cls_loss": round(1.5 * (1.0 - t) + 0.2 * normed[1] * (1.0 - t), 6),
        "dfl_loss": round(1.0 * (1.0 - t) + 0.15 * normed[2] * (1.0 - t), 6),
        "mAP50": round(0.1 + 0.85 * t + 0.05 * normed[3] * t, 6),
        "mAP50-95": round(0.05 + 0.80 * t + 0.04 * normed[4] * t, 6),
    }


def _make_fake_artifact(
    config_summary: str, magic: bytes = MOCK_MAGIC
) -> bytes:
    """Create a small deterministic fake .pt file (64–128 bytes)."""
    payload = config_summary.encode("utf-8")
    digest = hashlib.sha256(payload).digest()[:16]
    body = magic + digest + struct.pack("<I", len(payload)) + payload
    if len(body) < 64:
        body = body + b"\x00" * (64 - len(body))
    return body


def _xywhn_to_topleft_clamped(cx: float, cy: float, w: float, h: float) -> tuple[float, float, float, float]:
    """Convert normalized center-based (cx, cy, w, h) to top-left (x, y, w, h) with clamp 0..1."""
    x = cx - w / 2
    y = cy - h / 2
    x = max(0.0, min(1.0, x))
    y = max(0.0, min(1.0, y))
    w = max(0.0, min(1.0, w))
    h = max(0.0, min(1.0, h))
    x = min(x, 1.0 - w)
    y = min(y, 1.0 - h)
    return x, y, w, h


def _box_for_image(
    seed: int, filename: str, class_name: str, conf_threshold: float,
) -> dict[str, Any]:
    """Generate a single deterministic bounding box for a given (seed, filename, class)."""
    key = f"{seed}:{filename}:{class_name}".encode("utf-8")
    raw = hashlib.sha256(key).digest()

    coords = [struct.unpack_from("<d", raw, i * 8)[0] for i in range(4)]
    normed = [(v % 1.0 + 1.0) % 1.0 for v in coords]

    x, y = normed[0], normed[1]
    w = 0.05 + 0.35 * normed[2]
    h = 0.05 + 0.35 * normed[3]

    if x + w > 1.0:
        x = max(0.0, 1.0 - w)
    if y + h > 1.0:
        y = max(0.0, 1.0 - h)

    conf_min = max(conf_threshold, 0.70)
    conf_key = f"{seed}:{filename}:{class_name}:conf".encode("utf-8")
    conf_raw = hashlib.sha256(conf_key).digest()[0]
    conf = conf_min + (0.99 - conf_min) * (conf_raw / 255.0)
    conf = min(conf, 0.99)

    return {
        "class": class_name,
        "x": round(x, 4),
        "y": round(y, 4),
        "w": round(w, 4),
        "h": round(h, 4),
        "conf": round(conf, 4),
    }


def _generate_boxes_for_image(
    seed: int, filename: str, class_names: list[str], conf_threshold: float,
) -> list[dict[str, Any]]:
    """Generate 1–3 boxes per image deterministically."""
    fname_hash = hashlib.sha256(filename.encode("utf-8")).digest()
    n_classes = len(class_names)

    target = AUTOTRACK_BOX_COUNT_RANGE[1]
    if n_classes < target:
        target = n_classes
    raw_count = (fname_hash[0] % target) + 1
    selected_classes = []
    for i in range(raw_count):
        idx = fname_hash[i + 1] % n_classes
        cname = class_names[idx]
        if cname not in selected_classes:
            selected_classes.append(cname)

    boxes = []
    for cname in selected_classes:
        boxes.append(_box_for_image(seed, filename, cname, conf_threshold))

    return boxes
