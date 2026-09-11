"""Modo predict mock determinístico (ENGINE_MOCK=1) e real (ultralytics) do trainer-yolo.

ADR-0013 D3: subcomando predict — inferência YOLO sobre imagens de um dataset,
gerando predictions.json.

Entrypoint: python -m trainer_yolo predict --config <config.yaml> --output <dir>

Saída:
  - predictions.json  (engine yolo, model, conf, images[].boxes[])
  - SEM metrics.jsonl (progresso binário honesto — ADR-0013 D3/D6)
"""

from __future__ import annotations

import json
import os
from pathlib import Path

import yaml

from trainer_yolo.autotrack import (
    _generate_boxes_for_image,
    _read_dataset,
)
from trainer_yolo.train import _die


# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

REQUIRED_PREDICT_KEYS = frozenset({
    "conf",
})


# ---------------------------------------------------------------------------
# Config validation
# ---------------------------------------------------------------------------

def load_and_validate_predict_config(config_path: str | Path) -> dict:
    """Load config.yaml and validate required fields for predict. Exit on failure."""
    config_path = Path(config_path)
    if not config_path.is_file():
        _die(f"Config file not found: {config_path}")

    with open(config_path, "r", encoding="utf-8") as f:
        cfg = yaml.safe_load(f)

    if not isinstance(cfg, dict):
        _die("Config is not a YAML mapping")

    # Top-level required keys — weights_path is mandatory for predict
    required_top = {"job_id", "engine", "model", "mode", "dataset_path", "output_path", "seed", "weights_path"}
    missing = required_top - cfg.keys()
    if missing:
        _die(f"Missing required config keys: {sorted(missing)}")

    if not isinstance(cfg.get("predict"), dict):
        _die("Missing 'predict' section in config")

    pr = cfg["predict"]
    missing_pr = REQUIRED_PREDICT_KEYS - pr.keys()
    if missing_pr:
        _die(f"Missing required predict keys: {sorted(missing_pr)}")

    conf = pr["conf"]
    if not isinstance(conf, (int, float)) or not (0.0 <= conf <= 1.0):
        _die(f"predict.conf must be a number in 0..1, got: {conf}")

    return cfg


# ---------------------------------------------------------------------------
# Mock predict (deterministic, reuses autotrack generators)
# ---------------------------------------------------------------------------

def _mock_predict(cfg: dict, output: Path) -> None:
    """Generate deterministic predictions.json reusing autotrack generators."""
    seed = cfg["seed"]
    pr = cfg["predict"]
    conf_threshold = pr["conf"]

    # Validate dataset_path
    dataset_path = Path(cfg["dataset_path"])
    if not dataset_path.is_dir():
        _die(f"dataset_path does not exist or is not a directory: {dataset_path}")

    class_names, image_filenames = _read_dataset(dataset_path)

    output.mkdir(parents=True, exist_ok=True)

    # Generate boxes for each image (same deterministic generators as autotrack)
    images_data = []
    for fname in image_filenames:
        boxes = _generate_boxes_for_image(seed, fname, class_names, conf_threshold)
        images_data.append({"filename": fname, "boxes": boxes})

    # Write predictions.json — engine "yolo", model from config
    predictions_obj = {
        "engine": "yolo",
        "model": cfg["model"],
        "conf": conf_threshold,
        "images": images_data,
    }
    predictions_path = output / "predictions.json"
    with open(predictions_path, "w", encoding="utf-8") as f:
        json.dump(predictions_obj, f, indent=2, ensure_ascii=False)


# ---------------------------------------------------------------------------
# Real predict (ultralytics, lazy import)
# ---------------------------------------------------------------------------

def _xywhn_to_topleft_clamped(cx: float, cy: float, w: float, h: float) -> tuple[float, float, float, float]:
    """Convert normalized center-based (cx, cy, w, h) to top-left (x, y, w, h) with clamp 0..1."""
    x = cx - w / 2
    y = cy - h / 2
    # Clamp to [0, 1]
    x = max(0.0, min(1.0, x))
    y = max(0.0, min(1.0, y))
    w = max(0.0, min(1.0, w))
    h = max(0.0, min(1.0, h))
    # A3: borda direita/inferior — evita box estourar além de 1.0
    x = min(x, 1.0 - w)
    y = min(y, 1.0 - h)
    return x, y, w, h


def _real_predict(cfg: dict, output: Path) -> None:
    """Real prediction via ultralytics (requires GPU + extras [train])."""
    try:
        from ultralytics import YOLO  # lazy — torch only on this path
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

    output.mkdir(parents=True, exist_ok=True)

    model = YOLO(weights_path)
    results = model.predict(source=str(dataset_path), conf=conf, imgsz=640, device=0)

    # Build predictions from results
    images_data = []
    for r in results:
        fname = Path(r.path).name
        boxes_out = []
        if r.boxes is not None and len(r.boxes) > 0:
            xywhn = r.boxes.xywhn  # shape (N, 4), normalized center-based
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

    # Write predictions.json
    predictions_obj = {
        "engine": "yolo",
        "model": cfg["model"],
        "conf": conf,
        "images": images_data,
    }
    predictions_path = output / "predictions.json"
    with open(predictions_path, "w", encoding="utf-8") as f:
        json.dump(predictions_obj, f, indent=2, ensure_ascii=False)


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def cmd_predict(args: list[str]) -> None:
    """Parse CLI args for 'predict' subcommand and run."""
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

    mock = os.environ.get("ENGINE_MOCK", "1") == "1"
    if mock:
        _mock_predict(cfg, output)
    else:
        _real_predict(cfg, output)
