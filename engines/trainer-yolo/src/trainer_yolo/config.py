"""
Parsers e validação de configurações YAML para todas as ferramentas do trainer-yolo.
"""
from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

import yaml
from engine_kit.runtime import die as _die

REQUIRED_CONFIG_KEYS = frozenset({
    "job_id", "engine", "model", "mode", "dataset_path", "output_path", "seed",
})
REQUIRED_YOLO_KEYS = frozenset({
    "model", "epochs", "batch", "imgsz", "lr0", "optimizer", "augment",
})
REQUIRED_AUGMENT_KEYS = frozenset({"mosaic", "mixup_flip"})
REQUIRED_AUTOTRACK_KEYS = frozenset({"model", "conf"})
REQUIRED_PREDICT_KEYS = frozenset({"conf"})


def _load_yaml_file(config_path: str | Path) -> dict[str, Any]:
    p = Path(config_path)
    if not p.is_file():
        _die(f"Config file not found: {p}")
    with open(p, "r", encoding="utf-8") as f:
        cfg = yaml.safe_load(f)
    if not isinstance(cfg, dict):
        _die("Config is not a YAML mapping")
    return cfg


def load_and_validate_config(config_path: str | Path) -> dict:
    """Load config.yaml and validate required fields for YOLO training."""
    cfg = _load_yaml_file(config_path)
    missing = REQUIRED_CONFIG_KEYS - cfg.keys()
    if missing:
        _die(f"Missing required config keys: {sorted(missing)}")

    yolo = cfg.get("yolo")
    if not isinstance(yolo, dict):
        _die("Missing 'yolo' section in config")

    missing_yolo = REQUIRED_YOLO_KEYS - yolo.keys()
    if missing_yolo:
        _die(f"Missing required yolo keys: {sorted(missing_yolo)}")

    augment = yolo.get("augment")
    if not isinstance(augment, dict):
        _die("yolo.augment must be a mapping")

    missing_aug = REQUIRED_AUGMENT_KEYS - augment.keys()
    if missing_aug:
        _die(f"Missing required augment keys: {sorted(missing_aug)}")

    return cfg


def load_and_validate_autotrack_config(config_path: str | Path) -> dict:
    """Load config.yaml and validate required fields for autotrack."""
    cfg = _load_yaml_file(config_path)
    missing = REQUIRED_CONFIG_KEYS - cfg.keys()
    if missing:
        _die(f"Missing required config keys: {sorted(missing)}")

    at = cfg.get("autotrack")
    if not isinstance(at, dict):
        _die("Missing 'autotrack' section in config")

    missing_at = REQUIRED_AUTOTRACK_KEYS - at.keys()
    if missing_at:
        _die(f"Missing required autotrack keys: {sorted(missing_at)}")

    conf = at.get("conf")
    if not isinstance(conf, (int, float)) or not (0.0 <= conf <= 1.0):
        _die(f"autotrack.conf must be a number in 0..1, got: {conf}")

    return cfg


def load_and_validate_predict_config(config_path: str | Path) -> dict:
    """Load config.yaml and validate required fields for predict."""
    cfg = _load_yaml_file(config_path)
    required_top = REQUIRED_CONFIG_KEYS | {"weights_path"}
    missing = required_top - cfg.keys()
    if missing:
        _die(f"Missing required config keys: {sorted(missing)}")

    pred = cfg.get("predict")
    if not isinstance(pred, dict):
        _die("Missing 'predict' section in config")

    missing_pred = REQUIRED_PREDICT_KEYS - pred.keys()
    if missing_pred:
        _die(f"Missing required predict keys: {sorted(missing_pred)}")

    conf = pred.get("conf")
    if not isinstance(conf, (int, float)) or not (0.0 <= conf <= 1.0):
        _die(f"predict.conf must be a number in 0..1, got: {conf}")

    return cfg


def load_and_validate_autolabel_config(config_path: str | Path) -> dict:
    """Load config.yaml and validate required fields for autolabel."""
    cfg = _load_yaml_file(config_path)
    missing = {"engine", "model", "mode", "dataset_path", "output_path", "seed"} - cfg.keys()
    if missing:
        _die(f"Missing required config keys: {sorted(missing)}")
    return cfg
