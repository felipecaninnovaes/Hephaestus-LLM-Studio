"""
Conversão, gravação de métricas do contrato e cópia de pesos treinados para o YOLO.
"""
from __future__ import annotations

import json
import shutil
import sys
from pathlib import Path
from typing import Any

METRIC_KEYS = ("epoch", "box_loss", "cls_loss", "dfl_loss", "mAP50", "mAP50-95")


def _convert_ultralytics_metrics(raw: dict[str, Any]) -> dict[str, Any]:
    """Convert ultralytics trainer.metrics keys to contract keys."""
    mapping = {
        "train/box_loss": "box_loss",
        "train/cls_loss": "cls_loss",
        "train/dfl_loss": "dfl_loss",
        "metrics/mAP50(B)": "mAP50",
        "metrics/mAP50-95(B)": "mAP50-95",
    }
    result: dict[str, Any] = {}
    for ul_key, contract_key in mapping.items():
        val = raw.get(ul_key)
        if val is None:
            result[contract_key] = None
        else:
            result[contract_key] = float(val) if contract_key != "epoch" else int(val)
    return result


def _write_metrics_line(metrics_path: Path, epoch: int, metrics: dict[str, Any]) -> None:
    """Append one JSON line to metrics.jsonl (contract format)."""
    value_keys = ("box_loss", "cls_loss", "dfl_loss", "mAP50", "mAP50-95")
    if all(metrics.get(k) is None for k in value_keys):
        return
    line = {"epoch": epoch, **{k: metrics.get(k) for k in METRIC_KEYS if k != "epoch"}}
    with open(metrics_path, "a", encoding="utf-8") as f:
        f.write(json.dumps(line) + "\n")


def _copy_flat_weights(output: Path) -> None:
    """Copy best.pt and last.pt from ultralytics output to flat output dir."""
    src_dir = output / "train" / "weights"
    for name in ("best.pt", "last.pt"):
        src = src_dir / name
        dst = output / name
        if src.is_file():
            shutil.copy2(str(src), str(dst))
        else:
            print(f"WARNING: {src} not found, skipping copy", file=sys.stderr)
