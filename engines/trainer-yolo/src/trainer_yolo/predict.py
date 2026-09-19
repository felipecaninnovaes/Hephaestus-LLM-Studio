"""
Modo predict mock determinístico (ENGINE_MOCK=1) e real (ultralytics) do trainer-yolo.
"""
from __future__ import annotations

import json
import os
import sys
from pathlib import Path
from typing import Any

from engine_kit.mock import is_mock
from engine_kit.runtime import die as _die
from engine_kit.telemetry import TelemetryEmitter
from trainer_yolo.config import REQUIRED_PREDICT_KEYS, load_and_validate_predict_config
from trainer_yolo.dataset import _read_dataset
from trainer_yolo.deterministic import _generate_boxes_for_image, _xywhn_to_topleft_clamped


def _mock_predict(cfg: dict, output: Path) -> None:
    """Generate deterministic predictions.json reusing autotrack generators."""
    seed = cfg["seed"]
    pr = cfg["predict"]
    conf_threshold = pr["conf"]

    dataset_path = Path(cfg["dataset_path"])
    if not dataset_path.is_dir():
        _die(f"dataset_path does not exist or is not a directory: {dataset_path}")

    class_names, image_filenames = _read_dataset(dataset_path)
    output.mkdir(parents=True, exist_ok=True)

    emitter = TelemetryEmitter(output, legacy_filename=None)
    emitter.emit(
        phase="preparing",
        message=f"Iniciando predição YOLO mock ({len(image_filenames)} imagens)...",
        progress=0.1,
    )

    images_data = []
    for fname in image_filenames:
        boxes = _generate_boxes_for_image(seed, fname, class_names, conf_threshold)
        images_data.append({"filename": fname, "boxes": boxes})

    predictions_obj = {
        "engine": "yolo",
        "model": cfg["model"],
        "conf": conf_threshold,
        "images": images_data,
    }
    predictions_path = output / "predictions.json"
    with open(predictions_path, "w", encoding="utf-8") as f:
        json.dump(predictions_obj, f, indent=2, ensure_ascii=False)

    emitter.emit(
        phase="completed",
        message=f"Predição YOLO concluída: {len(images_data)} imagens processadas.",
        progress=1.0,
    )


def _real_predict(cfg: dict, output: Path) -> None:
    """Real prediction via ultralytics (requires GPU + extras [train])."""
    try:
        from ultralytics import YOLO
    except ImportError:
        _die(
            "ultralytics not installed. Install with: pip install -e '.[train]'"
        )

    pr = cfg["predict"]
    conf = pr["conf"]
    weights_path = cfg["weights_path"]
    dataset_path = Path(cfg["dataset_path"])

    if not dataset_path.is_dir():
        _die(f"dataset_path does not exist or is not a directory: {dataset_path}")

    images_dir = dataset_path / "images"
    if images_dir.is_dir() and any(images_dir.iterdir()):
        source = str(images_dir)
    else:
        source = str(dataset_path)

    output.mkdir(parents=True, exist_ok=True)
    emitter = TelemetryEmitter(output, legacy_filename=None)
    emitter.emit(
        phase="generating",
        message=f"Executando inferência YOLO em {source}...",
        progress=0.5,
    )

    model = YOLO(weights_path)
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

    predictions_obj = {
        "engine": "yolo",
        "model": cfg["model"],
        "conf": conf,
        "images": images_data,
    }
    predictions_path = output / "predictions.json"
    with open(predictions_path, "w", encoding="utf-8") as f:
        json.dump(predictions_obj, f, indent=2, ensure_ascii=False)

    emitter.emit(
        phase="completed",
        message=f"Predição finalizada: {len(images_data)} imagens processadas.",
        progress=1.0,
    )


def cmd_predict(args: list[str]) -> None:
    import argparse

    parser = argparse.ArgumentParser(
        prog="trainer-yolo predict",
        description="YOLO predict — mock determinístico (ENGINE_MOCK=1) ou real (ultralytics)",
    )
    parser.add_argument("--config", required=True, help="Path to config.yaml")
    parser.add_argument("--output", required=True, help="Output directory")

    opts = parser.parse_args(args)
    cfg = load_and_validate_predict_config(opts.config)
    output = Path(opts.output)

    mock = is_mock()
    if mock:
        _mock_predict(cfg, output)
    else:
        _real_predict(cfg, output)


__all__ = [
    "REQUIRED_PREDICT_KEYS",
    "load_and_validate_predict_config",
    "_mock_predict",
    "_real_predict",
    "cmd_predict",
]
