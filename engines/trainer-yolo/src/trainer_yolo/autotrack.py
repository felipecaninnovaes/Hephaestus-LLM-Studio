"""Modo autotrack mock determinístico (ENGINE_MOCK=1) do trainer-yolo.

ADR-0008 D2: gera bounding boxes fake para cada imagem do dataset, com
coordenadas determinísticas a partir de (seed, filename, class).

Entrypoint: python -m trainer_yolo autotrack --config <config.yaml> --output <dir>

Saída:
  - boxes.json  (formato D1: engine/model/seed/conf + images[].boxes[])
  - metrics.jsonl (1 linha, 6 keys — compatível com parse_metrics_line do orquestrador)
"""

from __future__ import annotations

import hashlib
import json
import struct
import sys
from pathlib import Path

import yaml

from trainer_yolo.train import METRIC_KEYS, _seed_bytes, _die


# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

REQUIRED_AUTOTRACK_KEYS = frozenset({
    "model", "conf",
})

AUTOTRACK_BOX_COUNT_RANGE = (1, 3)  # min, max boxes per image


# ---------------------------------------------------------------------------
# Config validation
# ---------------------------------------------------------------------------

def load_and_validate_autotrack_config(config_path: str | Path) -> dict:
    """Load config.yaml and validate required fields for autotrack. Exit on failure."""
    config_path = Path(config_path)
    if not config_path.is_file():
        _die(f"Config file not found: {config_path}")

    with open(config_path, "r", encoding="utf-8") as f:
        cfg = yaml.safe_load(f)

    if not isinstance(cfg, dict):
        _die("Config is not a YAML mapping")

    missing = {"job_id", "engine", "model", "mode", "dataset_path", "output_path", "seed"} - cfg.keys()
    if missing:
        _die(f"Missing required config keys: {sorted(missing)}")

    if not isinstance(cfg.get("autotrack"), dict):
        _die("Missing 'autotrack' section in config")

    at = cfg["autotrack"]
    missing_at = REQUIRED_AUTOTRACK_KEYS - at.keys()
    if missing_at:
        _die(f"Missing required autotrack keys: {sorted(missing_at)}")

    conf = at["conf"]
    if not isinstance(conf, (int, float)) or not (0.0 <= conf <= 1.0):
        _die(f"autotrack.conf must be a number in 0..1, got: {conf}")

    return cfg


# ---------------------------------------------------------------------------
# Dataset reading (reuse pattern from train.py)
# ---------------------------------------------------------------------------

def _read_dataset(dataset_path: Path) -> tuple[list[str], list[str]]:
    """Read dataset.yaml and return (class_names, image_filenames).

    dataset.yaml format (from render_dataset_yaml):
        path: .
        train: [images/img1.jpg, ...]
        val: [images/img3.jpg, ...]
        names:
          0: class_name_0
          1: class_name_1
    """
    dataset_yaml = dataset_path / "dataset.yaml"
    if not dataset_yaml.is_file():
        _die(f"dataset.yaml not found inside dataset_path: {dataset_path}")

    with open(dataset_yaml, "r", encoding="utf-8") as f:
        ds = yaml.safe_load(f)

    if not isinstance(ds, dict):
        _die("dataset.yaml is not a YAML mapping")

    # Extract class names from 'names' dict (idx -> name)
    names = ds.get("names")
    if not isinstance(names, dict) or len(names) == 0:
        _die("dataset.yaml has no 'names' mapping")

    # Sort by index to get deterministic order
    class_names = [names[k] for k in sorted(names.keys())]

    # Extract image filenames from train + val lists
    image_filenames: list[str] = []
    for split_key in ("train", "val"):
        paths = ds.get(split_key)
        if isinstance(paths, list):
            for p in paths:
                # paths are like "images/img1.jpg" — extract just the filename
                fname = str(p).split("/")[-1]
                if fname:
                    image_filenames.append(fname)

    if not image_filenames:
        _die("dataset.yaml has no images (empty train and val lists)")

    # Deduplicate and sort for determinism
    image_filenames = sorted(set(image_filenames))

    return class_names, image_filenames


# ---------------------------------------------------------------------------
# Deterministic box generation
# ---------------------------------------------------------------------------

def _box_for_image(
    seed: int, filename: str, class_name: str, conf_threshold: float,
) -> dict:
    """Generate a single deterministic bounding box for a given (seed, filename, class).

    Returns dict with class, x, y, w, h (0..1), conf (0..1).
    """
    # Deterministic hash from (seed, filename, class)
    key = f"{seed}:{filename}:{class_name}".encode("utf-8")
    raw = hashlib.sha256(key).digest()

    # x, y, w, h from first 32 bytes (4 floats, each normalised to 0..1)
    coords = [struct.unpack_from("<d", raw, i * 8)[0] for i in range(4)]
    normed = [(v % 1.0 + 1.0) % 1.0 for v in coords]

    x, y = normed[0], normed[1]
    # w, h in [0.05, 0.4] to avoid degenerate boxes
    w = 0.05 + 0.35 * normed[2]
    h = 0.05 + 0.35 * normed[3]

    # Ensure box stays within [0, 1]
    if x + w > 1.0:
        x = max(0.0, 1.0 - w)
    if y + h > 1.0:
        y = max(0.0, 1.0 - h)

    # conf in [max(conf_threshold, 0.70), 0.99]
    conf_min = max(conf_threshold, 0.70)
    # Use second hash for conf (SHA-256 is 32 bytes; we need byte 32+)
    conf_key = f"{seed}:{filename}:{class_name}:conf".encode("utf-8")
    conf_raw = hashlib.sha256(conf_key).digest()[0]
    conf = conf_min + (0.99 - conf_min) * (conf_raw / 255.0)

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
) -> list[dict]:
    """Generate 1–3 boxes per image deterministically.

    Uses the filename hash to decide how many boxes (1 or len(class_names),
    capped to AUTOTRACK_BOX_COUNT_RANGE).
    """
    # Deterministic box count from filename hash
    fname_hash = hashlib.sha256(filename.encode("utf-8")).digest()
    n_classes = len(class_names)

    # Select 1 to min(3, n_classes) boxes
    target = AUTOTRACK_BOX_COUNT_RANGE[1]
    if n_classes < target:
        target = n_classes
    # Use hash to pick subset of classes
    raw_count = (fname_hash[0] % target) + 1  # 1..target
    # Pick which classes (deterministic from hash)
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


# ---------------------------------------------------------------------------
# Mock autotrack
# ---------------------------------------------------------------------------

def _mock_autotrack(cfg: dict, output: Path) -> None:
    """Generate deterministic boxes.json and metrics.jsonl for the dataset."""
    seed = cfg["seed"]
    at = cfg["autotrack"]
    conf_threshold = at["conf"]

    # Validate dataset_path
    dataset_path = Path(cfg["dataset_path"])
    if not dataset_path.is_dir():
        _die(f"dataset_path does not exist or is not a directory: {dataset_path}")

    class_names, image_filenames = _read_dataset(dataset_path)

    output.mkdir(parents=True, exist_ok=True)

    # Generate boxes for each image
    images_data = []
    for fname in image_filenames:
        boxes = _generate_boxes_for_image(seed, fname, class_names, conf_threshold)
        images_data.append({"filename": fname, "boxes": boxes})

    # Write boxes.json
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

    # Write metrics.jsonl — 1 line, epoch=1, same 6 keys as train mock
    # Deterministic synthetic metrics for epoch=1
    raw = _seed_bytes(seed + 7, 64)  # same pattern as train but epoch=1
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


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def cmd_autotrack(args: list[str]) -> None:
    """Parse CLI args for 'autotrack' subcommand and run."""
    import argparse

    parser = argparse.ArgumentParser(
        prog="trainer-yolo autotrack",
        description="AutoTracker mock (ENGINE_MOCK=1) — generates deterministic bounding boxes",
    )
    parser.add_argument("--config", required=True, help="Path to config.yaml")
    parser.add_argument("--output", required=True, help="Output directory")

    opts = parser.parse_args(args)

    cfg = load_and_validate_autotrack_config(opts.config)
    output = Path(opts.output)

    _mock_autotrack(cfg, output)
