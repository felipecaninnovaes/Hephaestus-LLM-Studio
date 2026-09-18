"""
Modo autotrack mock determinístico (ENGINE_MOCK=1) e real (ultralytics world) do trainer-yolo.
"""
from __future__ import annotations

import json
import os
import struct
import sys
from pathlib import Path
from typing import Any

from engine_kit.mock import is_mock
from engine_kit.runtime import die as _die
from engine_kit.telemetry import TelemetryEmitter
from trainer_yolo.config import REQUIRED_AUTOTRACK_KEYS, load_and_validate_autotrack_config
from trainer_yolo.dataset import _read_dataset
from trainer_yolo.deterministic import (
    AUTOTRACK_BOX_COUNT_RANGE,
    _box_for_image,
    _generate_boxes_for_image,
    _seed_bytes,
    _xywhn_to_topleft_clamped,
)
from trainer_yolo.metrics_io import METRIC_KEYS


def _mock_autotrack(cfg: dict, output: Path) -> None:
    """Generate deterministic boxes.json and metrics.jsonl for the dataset."""
    seed = cfg["seed"]
    at = cfg["autotrack"]
    conf_threshold = at["conf"]

    dataset_path = Path(cfg["dataset_path"])
    if not dataset_path.is_dir():
        _die(f"dataset_path does not exist or is not a directory: {dataset_path}")

    class_names, image_filenames = _read_dataset(dataset_path)
    output.mkdir(parents=True, exist_ok=True)

    emitter = TelemetryEmitter(output, legacy_filename=None)
    emitter.emit(
        phase="preparing",
        message=f"Executando autotrack determinístico ({len(image_filenames)} imagens)...",
        progress=0.1,
    )

    images_data = []
    for fname in image_filenames:
        boxes = _generate_boxes_for_image(seed, fname, class_names, conf_threshold)
        images_data.append({"filename": fname, "boxes": boxes})

    boxes_obj = {
        "engine": "autotracker",
        "model": at["model"],
        "seed": seed,
        "conf": conf_threshold,
        "images": images_data,
    }
    boxes_path = output / "boxes.json"
    with open(boxes_path, "w", encoding="utf-8") as f:
        json.dump(boxes_obj, f, indent=2, ensure_ascii=False)

    raw = _seed_bytes(seed + 7, 64)
    floats = [struct.unpack_from("<d", raw, i * 8)[0] for i in range(6)]
    normed = [(v % 1.0 + 1.0) % 1.0 for v in floats]

    metrics_line = {
        "epoch": 1,
        "box_loss": round(2.0 * 0.3 * normed[0], 6),
        "cls_loss": round(1.5 * 0.2 * normed[1], 6),
        "dfl_loss": round(1.0 * 0.15 * normed[2], 6),
        "mAP50": round(0.1 + 0.85 * 0.05 * normed[3], 6),
        "mAP50-95": round(0.05 + 0.80 * 0.04 * normed[4], 6),
    }

    metrics_path = output / "metrics.jsonl"
    with open(metrics_path, "w", encoding="utf-8") as f:
        f.write(json.dumps(metrics_line) + "\n")

    emitter.emit(
        phase="completed",
        message=f"Autotrack concluído com sucesso ({len(image_filenames)} imagens)!",
        progress=1.0,
        metrics=metrics_line,
    )


def _real_autotrack(cfg: dict, output: Path) -> None:
    """Real autotrack via ultralytics open-vocab world model (requires GPU + extras [train])."""
    at = cfg["autotrack"]
    conf = at["conf"]
    weights_path = cfg.get("weights_path")
    if not weights_path:
        _die("weights_path required for real autotrack (ENGINE_MOCK=0)")

    dataset_path = Path(cfg["dataset_path"])
    if not dataset_path.is_dir():
        _die(f"dataset_path does not exist or is not a directory: {dataset_path}")

    class_names, _ = _read_dataset(dataset_path)

    try:
        from ultralytics import YOLO
    except ImportError:
        _die(
            "ultralytics not installed. Install with: pip install -e '.[train]'"
        )

    output.mkdir(parents=True, exist_ok=True)
    emitter = TelemetryEmitter(output, legacy_filename=None)
    emitter.emit(
        phase="preparing",
        message="Inicializando modelo YOLO para autotrack real...",
        progress=0.1,
    )

    model = YOLO(weights_path)
    if hasattr(model, "set_classes") and callable(getattr(model, "set_classes")):
        try:
            model.set_classes(class_names)
        except Exception as exc:
            print(
                f"INFO: model does not support set_classes ({exc}), using trained classes",
                file=sys.stderr,
            )

    images_dir = dataset_path / "images"
    if images_dir.is_dir() and any(images_dir.iterdir()):
        source = str(images_dir)
    else:
        source = str(dataset_path)

    emitter.emit(
        phase="generating",
        message=f"Executando predição de autotrack em {source}...",
        progress=0.5,
    )

    results = model.predict(source=source, conf=conf, imgsz=640, device=0)

    images_data = []
    for r in results:
        fname = Path(r.path).name
        boxes_out = []
        if r.boxes is not None and len(r.boxes) > 0:
            xywhn = r.boxes.xywhn
            clss = r.boxes.cls
            confs = r.boxes.conf
            for i in range(len(xywhn)):
                cx, cy, bw, bh = xywhn[i].tolist()
                x, y, w, h = _xywhn_to_topleft_clamped(cx, cy, bw, bh)
                class_name = model.names[int(clss[i])]
                box_conf = float(confs[i])
                boxes_out.append({
                    "class": class_name,
                    "x": round(x, 4),
                    "y": round(y, 4),
                    "w": round(w, 4),
                    "h": round(h, 4),
                    "conf": round(box_conf, 4),
                })
        images_data.append({"filename": fname, "boxes": boxes_out})

    boxes_obj = {
        "engine": "autotracker",
        "model": at["model"],
        "seed": 0,
        "conf": conf,
        "images": images_data,
    }
    boxes_path = output / "boxes.json"
    with open(boxes_path, "w", encoding="utf-8") as f:
        json.dump(boxes_obj, f, indent=2, ensure_ascii=False)

    emitter.emit(
        phase="completed",
        message=f"Autotrack finalizado: {len(images_data)} imagens processadas.",
        progress=1.0,
    )


def cmd_autotrack(args: list[str]) -> None:
    import argparse

    parser = argparse.ArgumentParser(
        prog="trainer-yolo autotrack",
        description="AutoTracker — mock determinístico (ENGINE_MOCK=1) ou real (ultralytics world)",
    )
    parser.add_argument("--config", required=True, help="Path to config.yaml")
    parser.add_argument("--output", required=True, help="Output directory")

    opts = parser.parse_args(args)
    cfg = load_and_validate_autotrack_config(opts.config)
    output = Path(opts.output)

    mock = is_mock()
    if mock:
        _mock_autotrack(cfg, output)
    else:
        _real_autotrack(cfg, output)


__all__ = [
    "REQUIRED_AUTOTRACK_KEYS",
    "AUTOTRACK_BOX_COUNT_RANGE",
    "METRIC_KEYS",
    "_seed_bytes",
    "_die",
    "_read_dataset",
    "_box_for_image",
    "_generate_boxes_for_image",
    "_xywhn_to_topleft_clamped",
    "_mock_autotrack",
    "_real_autotrack",
    "load_and_validate_autotrack_config",
    "cmd_autotrack",
]
