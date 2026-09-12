"""Modo treino simulado (ENGINE_MOCK=1) e real (ultralytics) do trainer-yolo.

ADR-0007 D5: leitura de config.yaml, iteração de epochs com métricas sintéticas,
gravação de metrics.jsonl e artefatos fake (best.pt/last.pt).

ENGINE_MOCK=1 (default) → stdlib puro, sem torch/ultralytics.
Sem ENGINE_MOCK + GPU  → modo real @gpu manual (fora do compose).
"""

from __future__ import annotations

import hashlib
import json
import os
import struct
import sys
import time
from pathlib import Path
from typing import Any

import yaml


# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

REQUIRED_CONFIG_KEYS = frozenset({
    "job_id", "engine", "model", "mode", "dataset_path", "output_path", "seed",
})
REQUIRED_YOLO_KEYS = frozenset({
    "model", "epochs", "batch", "imgsz", "lr0", "optimizer", "augment",
})
REQUIRED_AUGMENT_KEYS = frozenset({"mosaic", "mixup_flip"})
MOCK_MAGIC = b"HEPHMOCK"
METRIC_KEYS = ("epoch", "box_loss", "cls_loss", "dfl_loss", "mAP50", "mAP50-95")


# ---------------------------------------------------------------------------
# Deterministic metric generation (no RNG)
# ---------------------------------------------------------------------------

def _seed_bytes(seed: int, length: int = 1024) -> bytes:
    """Expand a single seed int into deterministic bytes via SHA-256 chain."""
    h = hashlib.sha256(struct.pack("<q", seed)).digest()
    out = bytearray()
    while len(out) < length:
        h = hashlib.sha256(h).digest()
        out.extend(h)
    return bytes(out[:length])


def _synthetic_metrics(seed: int, epoch: int, total_epochs: int) -> dict:
    """Generate one line of synthetic metrics for *epoch* (1-indexed).

    Deterministic: same (seed, epoch, total_epochs) → same values.
    Box/cls/dfl loss decay; mAP50/mAP50-95 grow — classic learning curve.
    """
    raw = _seed_bytes(seed + epoch * 7, 64)
    # 6 floats from first 48 bytes
    floats = [struct.unpack_from("<d", raw, i * 8)[0] for i in range(6)]
    # Normalise each to [0, 1)
    normed = [(v % 1.0 + 1.0) % 1.0 for v in floats]

    t = epoch / max(total_epochs, 1)  # 0 → 1 over training

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
    """Create a small deterministic fake .pt file (64–128 bytes).

    Structure: magic(8) + digest(16) + len(4) + payload + padding to ≥64.
    """
    payload = config_summary.encode("utf-8")
    digest = hashlib.sha256(payload).digest()[:16]
    body = magic + digest + struct.pack("<I", len(payload)) + payload
    # Pad to at least 64 bytes with zero bytes
    if len(body) < 64:
        body = body + b"\x00" * (64 - len(body))
    return body


# ---------------------------------------------------------------------------
# Config validation
# ---------------------------------------------------------------------------

def load_and_validate_config(config_path: str | Path) -> dict:
    """Load config.yaml and validate required fields. Exit on failure."""
    config_path = Path(config_path)
    if not config_path.is_file():
        _die(f"Config file not found: {config_path}")

    with open(config_path, "r", encoding="utf-8") as f:
        cfg = yaml.safe_load(f)

    if not isinstance(cfg, dict):
        _die("Config is not a YAML mapping")

    missing = REQUIRED_CONFIG_KEYS - cfg.keys()
    if missing:
        _die(f"Missing required config keys: {sorted(missing)}")

    if not isinstance(cfg.get("yolo"), dict):
        _die("Missing 'yolo' section in config")

    yolo = cfg["yolo"]
    missing_yolo = REQUIRED_YOLO_KEYS - yolo.keys()
    if missing_yolo:
        _die(f"Missing required yolo keys: {sorted(missing_yolo)}")

    augment = yolo.get("augment")
    if not isinstance(augment, dict):
        _die("Missing 'augment' subsection in yolo config")
    missing_aug = REQUIRED_AUGMENT_KEYS - augment.keys()
    if missing_aug:
        _die(f"Missing required augment keys: {sorted(missing_aug)}")

    return cfg


# ---------------------------------------------------------------------------
# Mock training
# ---------------------------------------------------------------------------

def _mock_train(cfg: dict, output: Path) -> None:
    """Simulate training: iterate epochs, write metrics.jsonl, fake artifacts."""
    seed = cfg["seed"]
    yolo = cfg["yolo"]
    epochs = yolo["epochs"]

    # Validate dataset_path exists and contains dataset.yaml
    dataset_path = Path(cfg["dataset_path"])
    if not dataset_path.is_dir():
        _die(f"dataset_path does not exist or is not a directory: {dataset_path}")
    dataset_yaml = dataset_path / "dataset.yaml"
    if not dataset_yaml.is_file():
        _die(f"dataset.yaml not found inside dataset_path: {dataset_path}")

    output.mkdir(parents=True, exist_ok=True)
    metrics_path = output / "metrics.jsonl"

    epoch_sleep_ms = int(os.environ.get("MOCK_EPOCH_SLEEP_MS", "200"))
    sleep_sec = epoch_sleep_ms / 1000.0

    metrics: list[dict] = []

    for epoch in range(1, epochs + 1):
        m = _synthetic_metrics(seed, epoch, epochs)
        metrics.append(m)

        # Structured progress line (one per epoch, parseable by orchestrator)
        print(
            f"epoch={epoch}/{epochs} "
            f"box_loss={m['box_loss']:.6f} "
            f"cls_loss={m['cls_loss']:.6f} "
            f"dfl_loss={m['dfl_loss']:.6f} "
            f"mAP50={m['mAP50']:.6f} "
            f"mAP50-95={m['mAP50-95']:.6f}"
        )

        if epoch < epochs and sleep_sec > 0:
            time.sleep(sleep_sec)

    # Write metrics.jsonl — one JSON object per line
    with open(metrics_path, "w", encoding="utf-8") as f:
        for m in metrics:
            f.write(json.dumps(m) + "\n")

    # Write fake artifacts: best.pt and last.pt
    config_summary = json.dumps(
        {"job_id": cfg["job_id"], "model": yolo["model"], "seed": seed},
        sort_keys=True,
    )
    fake_pt = _make_fake_artifact(config_summary)

    for name in ("best.pt", "last.pt"):
        (output / name).write_bytes(fake_pt)

    # Do NOT generate samples/ (D0 — ADR-0007)


# ---------------------------------------------------------------------------
# Real training (ultralytics, lazy import)
# ---------------------------------------------------------------------------

def _convert_ultralytics_metrics(raw: dict[str, Any]) -> dict[str, Any]:
    """Convert ultralytics trainer.metrics keys to contract keys.

    Contract keys (METRIC_KEYS): epoch, box_loss, cls_loss, dfl_loss, mAP50, mAP50-95.

    Always returns a dict with all keys; missing metrics are set to None (honest skip).
    """
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
            # Epoch without this metric — still include with None (honest skip)
            result[contract_key] = None
        else:
            result[contract_key] = float(val) if contract_key != "epoch" else int(val)
    return result


def _write_metrics_line(metrics_path: Path, epoch: int, metrics: dict[str, Any]) -> None:
    """Append one JSON line to metrics.jsonl (contract format).

    Skips writing when all 5 value metrics (box_loss, cls_loss, dfl_loss, mAP50,
    mAP50-95) are None — no useful data for the Rust parser (as_f64()? discards).
    """
    value_keys = ("box_loss", "cls_loss", "dfl_loss", "mAP50", "mAP50-95")
    if all(metrics.get(k) is None for k in value_keys):
        return  # no usable metrics for this epoch
    line = {"epoch": epoch, **{k: metrics.get(k) for k in METRIC_KEYS if k != "epoch"}}
    with open(metrics_path, "a", encoding="utf-8") as f:
        f.write(json.dumps(line) + "\n")


def _copy_flat_weights(output: Path) -> None:
    """Copy best.pt and last.pt from ultralytics output to flat output dir."""
    import shutil

    src_dir = output / "train" / "weights"
    for name in ("best.pt", "last.pt"):
        src = src_dir / name
        dst = output / name
        if src.is_file():
            shutil.copy2(str(src), str(dst))
        else:
            # Weight file missing — not fatal, but log a warning
            print(f"WARNING: {src} not found, skipping copy", file=sys.stderr)


def _prepare_dataset_yaml(dataset_yaml_path: Path, dataset_dir: Path) -> None:
    """Prepare dataset.yaml for ultralytics compatibility.

    1. Rewrite relative ``path:`` to absolute (avoids CWD-dependent resolution).
    2. Convert list-style ``train``/``val`` entries (e.g. ``[images/a.png, ...]``)
       to .txt files with one absolute image path per line — ultralytics opens
       .txt files as text (not .png bytes), so passing image paths directly
       causes ``UnicodeDecodeError``.
    3. If ``val`` is empty/missing, point to ``train.txt`` (small datasets where
       train and val share the same images; avoids val-is-empty error).
    """
    with open(dataset_yaml_path, "r", encoding="utf-8") as f:
        data = yaml.safe_load(f) or {}

    # Make path absolute
    current = data.get("path")
    if current is not None and not Path(current).is_absolute():
        data["path"] = str(dataset_dir)

    # Convert list entries to .txt files with absolute paths
    for key in ("train", "val"):
        entries = data.get(key)
        if isinstance(entries, list) and entries:
            txt_path = dataset_dir / f"{key}.txt"
            lines = "\n".join(str(dataset_dir / entry) for entry in entries)
            txt_path.write_text(lines, encoding="utf-8")
            data[key] = f"{key}.txt"

    # Small datasets: val shares train images when val is empty/missing
    if not data.get("val"):
        data["val"] = "train.txt"

    with open(dataset_yaml_path, "w", encoding="utf-8") as f:
        yaml.safe_dump(data, f, sort_keys=False)


def _real_train(cfg: dict, output: Path) -> None:
    """Real training via ultralytics (requires GPU + extras [train])."""
    try:
        from ultralytics import YOLO  # lazy — torch only on this path
    except ImportError:
        _die(
            "ultralytics not installed. Install with: pip install -e '.[train]'"
        )

    yolo_cfg = cfg["yolo"]
    dataset_path = Path(cfg["dataset_path"])
    dataset_yaml = dataset_path / "dataset.yaml"
    if not dataset_yaml.is_file():
        _die(f"dataset.yaml not found: {dataset_path}")

    # Resolve relative `path:` and list-style train/val in dataset.yaml.
    # Builder uses `path: .` (relative to export CWD) and `train: [images/...]`
    # which ultralytics opens as text — causing UnicodeDecodeError for images.
    # Rewrites path to absolute and converts lists to .txt files.
    _prepare_dataset_yaml(dataset_yaml, dataset_path)

    output.mkdir(parents=True, exist_ok=True)
    metrics_path = output / "metrics.jsonl"

    seed = cfg["seed"]
    weights_path = cfg.get("weights_path")
    model = YOLO(weights_path) if weights_path else YOLO(yolo_cfg["model"])

    # Callback: append one JSON line per epoch to metrics.jsonl
    def on_train_epoch_end(trainer) -> None:  # noqa: ANN001 — ultralytics callback
        epoch = trainer.epoch + 1  # ultralytics is 0-indexed
        raw = trainer.metrics or {}
        converted = _convert_ultralytics_metrics(raw)
        if converted is not None:
            _write_metrics_line(metrics_path, epoch, converted)

    model.add_callback("on_train_epoch_end", on_train_epoch_end)

    model.train(
        data=str(dataset_yaml),
        epochs=yolo_cfg["epochs"],
        batch=yolo_cfg["batch"],
        imgsz=yolo_cfg["imgsz"],
        lr0=yolo_cfg["lr0"],
        optimizer=yolo_cfg["optimizer"],
        mosaic=float(yolo_cfg["augment"]["mosaic"]),
        # mixup_flip is True/False but ultralytics mixup expects float 0.0–1.0
        mixup=0.5 if yolo_cfg["augment"]["mixup_flip"] else 0.0,
        project=str(output),
        name="train",
        exist_ok=True,
        device=0,
        seed=seed,
    )

    # Post-train: copy flat weights + ensure metrics.jsonl is consistent
    _copy_flat_weights(output)


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def _die(msg: str) -> None:
    """Print error and exit with code 1."""
    print(f"ERROR: {msg}", file=sys.stderr)
    sys.exit(1)


def cmd_train(args: list[str]) -> None:
    """Parse CLI args for 'train' subcommand and run."""
    import argparse

    parser = argparse.ArgumentParser(
        prog="trainer-yolo train",
        description="YOLO mock trainer (ENGINE_MOCK=1) or real (ultralytics)",
    )
    parser.add_argument("--config", required=True, help="Path to config.yaml")
    parser.add_argument("--output", required=True, help="Output directory")

    opts = parser.parse_args(args)

    cfg = load_and_validate_config(opts.config)
    output = Path(opts.output)

    mock = os.environ.get("ENGINE_MOCK", "1") == "1"
    if mock:
        _mock_train(cfg, output)
    else:
        _real_train(cfg, output)


def cmd_health() -> None:
    """Print health JSON (backward-compatible with existing stub)."""
    print(json.dumps({"status": "ok", "engine": "trainer-yolo", "mode": "mock"}))


def main(argv: list[str] | None = None) -> None:
    """Entry point: route to subcommand or print health."""
    if argv is None:
        argv = sys.argv[1:]

    if not argv or argv[0] in ("-h", "--help"):
        # Health check (backward-compatible) or help
        if not argv:
            cmd_health()
            return
        # --help falls through to argparse below

    if argv[0] == "train":
        cmd_train(argv[1:])
    elif argv[0] == "health":
        cmd_health()
    elif argv[0] == "autotrack":
        from trainer_yolo.autotrack import cmd_autotrack
        cmd_autotrack(argv[1:])
    elif argv[0] == "predict":
        from trainer_yolo.predict import cmd_predict
        cmd_predict(argv[1:])
    elif argv[0] == "autolabel":
        from trainer_yolo.autolabel import cmd_autolabel
        cmd_autolabel(argv[1:])
    else:
        _die(f"Unknown subcommand: {argv[0]}. Use 'train', 'autotrack', 'predict', 'autolabel', or 'health'.")


if __name__ == "__main__":
    main()
