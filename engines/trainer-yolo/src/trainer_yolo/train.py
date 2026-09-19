"""
Modo treino simulado (ENGINE_MOCK=1) e real (ultralytics) do trainer-yolo.
"""
from __future__ import annotations

import json
import os
import sys
import time
from pathlib import Path
from typing import Any

from engine_kit.mock import is_mock
from engine_kit.runtime import die as _die
from engine_kit.telemetry import TelemetryEmitter
from trainer_yolo.config import (
    REQUIRED_AUGMENT_KEYS,
    REQUIRED_CONFIG_KEYS,
    REQUIRED_YOLO_KEYS,
    load_and_validate_config,
)
from trainer_yolo.dataset import _prepare_dataset_yaml
from trainer_yolo.deterministic import (
    MOCK_MAGIC,
    _make_fake_artifact,
    _seed_bytes,
    _synthetic_metrics,
)
from trainer_yolo.metrics_io import (
    METRIC_KEYS,
    _convert_ultralytics_metrics,
    _copy_flat_weights,
    _write_metrics_line,
)
from trainer_yolo.yolo_adapter import get_yolo_class


def _mock_train(cfg: dict, output: Path) -> None:
    """Simulate training: iterate epochs, write metrics.jsonl, fake artifacts."""
    seed = cfg["seed"]
    yolo = cfg["yolo"]
    epochs = yolo["epochs"]

    dataset_path = Path(cfg["dataset_path"])
    if not dataset_path.is_dir():
        _die(f"dataset_path does not exist or is not a directory: {dataset_path}")
    dataset_yaml = dataset_path / "dataset.yaml"
    if not dataset_yaml.is_file():
        _die(f"dataset.yaml not found inside dataset_path: {dataset_path}")

    output.mkdir(parents=True, exist_ok=True)
    metrics_path = output / "metrics.jsonl"

    emitter = TelemetryEmitter(output, legacy_filename=None)
    emitter.emit(
        phase="preparing",
        message=f"Iniciando treino YOLO ({yolo['model']}, {epochs} epochs)...",
        progress=0.05,
    )

    epoch_sleep_ms = int(os.environ.get("MOCK_EPOCH_SLEEP_MS", "200"))
    sleep_sec = epoch_sleep_ms / 1000.0

    metrics: list[dict] = []

    for epoch in range(1, epochs + 1):
        m = _synthetic_metrics(seed, epoch, epochs)
        metrics.append(m)

        print(
            f"epoch={epoch}/{epochs} "
            f"box_loss={m['box_loss']:.6f} "
            f"cls_loss={m['cls_loss']:.6f} "
            f"dfl_loss={m['dfl_loss']:.6f} "
            f"mAP50={m['mAP50']:.6f} "
            f"mAP50-95={m['mAP50-95']:.6f}"
        )

        progress = round(epoch / epochs, 4)
        emitter.emit(
            phase="training",
            message=f"Epoch {epoch}/{epochs} concluída",
            progress=progress,
            epoch=epoch,
            total_epochs=epochs,
            metrics=m,
        )

        if epoch < epochs and sleep_sec > 0:
            time.sleep(sleep_sec)

    with open(metrics_path, "w", encoding="utf-8") as f:
        for m in metrics:
            f.write(json.dumps(m) + "\n")

    config_summary = json.dumps(
        {"job_id": cfg["job_id"], "model": yolo["model"], "seed": seed},
        sort_keys=True,
    )
    fake_pt = _make_fake_artifact(config_summary)

    for name in ("best.pt", "last.pt"):
        (output / name).write_bytes(fake_pt)

    emitter.emit(
        phase="completed",
        message=f"Treino YOLO concluído com sucesso ({epochs} epochs)!",
        progress=1.0,
    )


def _real_train(cfg: dict, output: Path) -> None:
    """Real training via ultralytics (requires GPU + extras [train])."""
    try:
        from ultralytics import YOLO
    except ImportError:
        _die(
            "ultralytics not installed. Install with: pip install -e '.[train]'"
        )

    yolo_cfg = cfg["yolo"]
    dataset_path = Path(cfg["dataset_path"])
    dataset_yaml = dataset_path / "dataset.yaml"
    if not dataset_yaml.is_file():
        _die(f"dataset.yaml not found: {dataset_path}")

    _prepare_dataset_yaml(dataset_yaml, dataset_path)

    output.mkdir(parents=True, exist_ok=True)
    metrics_path = output / "metrics.jsonl"
    emitter = TelemetryEmitter(output, legacy_filename=None)
    emitter.emit(
        phase="preparing",
        message=f"Configurando pipeline de treino real YOLO ({yolo_cfg['model']})...",
        progress=0.05,
    )

    seed = cfg["seed"]
    weights_path = cfg.get("weights_path")
    model = YOLO(weights_path) if weights_path else YOLO(yolo_cfg["model"])

    def on_train_epoch_end(trainer) -> None:
        epoch = trainer.epoch + 1
        raw = trainer.metrics or {}
        converted = _convert_ultralytics_metrics(raw)
        if converted is not None:
            _write_metrics_line(metrics_path, epoch, converted)
            progress = round(epoch / yolo_cfg["epochs"], 4)
            emitter.emit(
                phase="training",
                message=f"Epoch {epoch}/{yolo_cfg['epochs']} concluída",
                progress=progress,
                epoch=epoch,
                total_epochs=yolo_cfg["epochs"],
                metrics=converted,
            )

    model.add_callback("on_train_epoch_end", on_train_epoch_end)

    model.train(
        data=str(dataset_yaml),
        epochs=yolo_cfg["epochs"],
        batch=yolo_cfg["batch"],
        imgsz=yolo_cfg["imgsz"],
        lr0=yolo_cfg["lr0"],
        optimizer=yolo_cfg["optimizer"],
        mosaic=float(yolo_cfg["augment"]["mosaic"]),
        mixup=0.5 if yolo_cfg["augment"]["mixup_flip"] else 0.0,
        project=str(output),
        name="train",
        exist_ok=True,
        device=0,
        seed=seed,
    )

    _copy_flat_weights(output)
    emitter.emit(
        phase="completed",
        message="Treino YOLO concluído com sucesso!",
        progress=1.0,
    )


def cmd_train(args: list[str]) -> None:
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

    mock = is_mock()
    if mock:
        _mock_train(cfg, output)
    else:
        _real_train(cfg, output)


def cmd_health() -> None:
    print(json.dumps({"status": "ok", "engine": "trainer-yolo", "mode": "mock"}))

def main(argv: list[str] | None = None) -> None:
    if argv is None:
        argv = sys.argv[1:]

    if not argv or argv[0] in ("-h", "--help"):
        if not argv:
            cmd_health()
            return

    subcmd = argv[0]
    sub_args = argv[1:]

    if subcmd == "train":
        cmd_train(sub_args)
    elif subcmd == "health":
        cmd_health()
    elif subcmd == "autotrack":
        from trainer_yolo.autotrack import cmd_autotrack
        cmd_autotrack(sub_args)
    elif subcmd == "predict":
        from trainer_yolo.predict import cmd_predict
        cmd_predict(sub_args)
    elif subcmd == "autolabel":
        from trainer_yolo.autolabel import cmd_autolabel
        cmd_autolabel(sub_args)
    else:
        print(f"Unknown subcommand: {subcmd}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
