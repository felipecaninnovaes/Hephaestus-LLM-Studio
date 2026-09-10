"""Tests for trainer_yolo (ADR-0007 D5).

Covers:
  (a) Config parsing — valid shape, missing keys, missing yolo section.
  (b) Mock run in tempdir — metrics.jsonl with N lines, exact keys, artifacts exist, no samples/.
  (c) Determinism — same config+seed → same .pt bytes and same jsonl values.
  (d) dataset_path nonexistent → exit ≠ 0.
  (e) CLI via python -m trainer_yolo train --config ... --output ...
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import textwrap
from pathlib import Path
from unittest.mock import MagicMock

import pytest
import yaml

# Ensure mock mode
os.environ.setdefault("ENGINE_MOCK", "1")
os.environ["MOCK_EPOCH_SLEEP_MS"] = "0"

from trainer_yolo.train import (
    METRIC_KEYS,
    MOCK_MAGIC,
    REQUIRED_AUGMENT_KEYS,
    REQUIRED_CONFIG_KEYS,
    REQUIRED_YOLO_KEYS,
    _convert_ultralytics_metrics,
    _copy_flat_weights,
    _make_fake_artifact,
    _prepare_dataset_yaml,
    _seed_bytes,
    _synthetic_metrics,
    _write_metrics_line,
    load_and_validate_config,
)


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

def _make_config(
    tmp_path: Path,
    *,
    epochs: int = 3,
    seed: int = 42,
    dataset_path: str | None = None,
    missing_keys: set[str] | None = None,
    missing_yolo: bool = False,
    missing_augment: bool = False,
) -> Path:
    """Write a valid config.yaml to tmp_path and return its path."""
    if dataset_path is None:
        ds = tmp_path / "dataset"
        ds.mkdir(exist_ok=True)
        (ds / "dataset.yaml").write_text("train: ./train\nval: ./val\nnc: 1\n")
        dataset_path = str(ds)

    cfg: dict = {
        "job_id": "test-job-001",
        "engine": "yolo",
        "model": "yolo11m",
        "mode": "train",
        "dataset_path": dataset_path,
        "output_path": str(tmp_path / "output"),
        "seed": seed,
        "yolo": {
            "model": "yolo11m",
            "epochs": epochs,
            "batch": 16,
            "imgsz": 640,
            "lr0": 0.01,
            "optimizer": "AdamW",
            "augment": {
                "mosaic": True,
                "mixup_flip": False,
            },
        },
    }

    if missing_keys:
        for k in missing_keys:
            cfg.pop(k, None)
    if missing_yolo:
        del cfg["yolo"]
    if missing_augment:
        cfg["yolo"].pop("augment", None)

    cfg_path = tmp_path / "config.yaml"
    cfg_path.write_text(yaml.dump(cfg, default_flow_style=False))
    return cfg_path


# ---------------------------------------------------------------------------
# (a) Config parsing
# ---------------------------------------------------------------------------

class TestConfigParsing:
    def test_valid_config(self, tmp_path: Path) -> None:
        cfg_path = _make_config(tmp_path)
        cfg = load_and_validate_config(cfg_path)
        assert cfg["job_id"] == "test-job-001"
        assert cfg["yolo"]["epochs"] == 3

    def test_missing_required_keys(self, tmp_path: Path) -> None:
        cfg_path = _make_config(tmp_path, missing_keys={"seed"})
        with pytest.raises(SystemExit):
            load_and_validate_config(cfg_path)

    def test_missing_yolo_section(self, tmp_path: Path) -> None:
        cfg_path = _make_config(tmp_path, missing_yolo=True)
        with pytest.raises(SystemExit):
            load_and_validate_config(cfg_path)

    def test_missing_augment_section(self, tmp_path: Path) -> None:
        cfg_path = _make_config(tmp_path, missing_augment=True)
        with pytest.raises(SystemExit):
            load_and_validate_config(cfg_path)

    def test_nonexistent_config_file(self, tmp_path: Path) -> None:
        with pytest.raises(SystemExit):
            load_and_validate_config(tmp_path / "nonexistent.yaml")


# ---------------------------------------------------------------------------
# (b) Mock run in tempdir
# ---------------------------------------------------------------------------

class TestMockRun:
    def test_mock_run_produces_metrics_and_artifacts(self, tmp_path: Path) -> None:
        epochs = 3
        cfg_path = _make_config(tmp_path, epochs=epochs)
        output = tmp_path / "output"
        output.mkdir()

        os.environ["ENGINE_MOCK"] = "1"
        os.environ["MOCK_EPOCH_SLEEP_MS"] = "0"

        from trainer_yolo.train import _mock_train

        cfg = load_and_validate_config(cfg_path)
        _mock_train(cfg, output)

        # metrics.jsonl exists and has exactly N lines
        metrics_path = output / "metrics.jsonl"
        assert metrics_path.is_file()
        lines = metrics_path.read_text().strip().split("\n")
        assert len(lines) == epochs

        # Each line is valid JSON with exact keys
        for line in lines:
            m = json.loads(line)
            assert set(m.keys()) == set(METRIC_KEYS), f"Unexpected keys: {m.keys()}"
            assert isinstance(m["epoch"], int)
            assert isinstance(m["box_loss"], float)
            assert isinstance(m["mAP50"], float)

        # best.pt and last.pt exist
        assert (output / "best.pt").is_file()
        assert (output / "last.pt").is_file()

        # No samples/ directory
        assert not (output / "samples").exists()

    def test_artifact_content_has_magic(self, tmp_path: Path) -> None:
        from trainer_yolo.train import MOCK_MAGIC

        fake = _make_fake_artifact("test-config")
        assert fake.startswith(MOCK_MAGIC)
        assert len(fake) >= 64
        assert len(fake) <= 256


# ---------------------------------------------------------------------------
# (c) Determinism
# ---------------------------------------------------------------------------

class TestDeterminism:
    def test_same_seed_same_metrics(self, tmp_path: Path) -> None:
        metrics_a = [_synthetic_metrics(42, e, 10) for e in range(1, 11)]
        metrics_b = [_synthetic_metrics(42, e, 10) for e in range(1, 11)]
        assert metrics_a == metrics_b

    def test_different_seed_different_metrics(self, tmp_path: Path) -> None:
        metrics_a = [_synthetic_metrics(42, e, 10) for e in range(1, 11)]
        metrics_b = [_synthetic_metrics(99, e, 10) for e in range(1, 11)]
        assert metrics_a != metrics_b

    def test_same_config_same_artifacts(self, tmp_path: Path) -> None:
        from trainer_yolo.train import MOCK_MAGIC

        summary = json.dumps({"job_id": "x", "model": "y", "seed": 42}, sort_keys=True)
        a = _make_fake_artifact(summary)
        b = _make_fake_artifact(summary)
        assert a == b

    def test_full_deterministic_run(self, tmp_path: Path) -> None:
        """Two runs with same config produce identical metrics.jsonl and .pt files."""
        os.environ["ENGINE_MOCK"] = "1"
        os.environ["MOCK_EPOCH_SLEEP_MS"] = "0"

        from trainer_yolo.train import _mock_train

        def _run(label: str) -> Path:
            cfg_path = _make_config(tmp_path, seed=42, epochs=3)
            out = tmp_path / f"out-{label}"
            out.mkdir()
            cfg = load_and_validate_config(cfg_path)
            _mock_train(cfg, out)
            return out

        out1 = _run("a")
        out2 = _run("b")

        assert (out1 / "metrics.jsonl").read_text() == (out2 / "metrics.jsonl").read_text()
        assert (out1 / "best.pt").read_bytes() == (out2 / "best.pt").read_bytes()
        assert (out1 / "last.pt").read_bytes() == (out2 / "last.pt").read_bytes()


# ---------------------------------------------------------------------------
# (d) dataset_path nonexistent
# ---------------------------------------------------------------------------

class TestDatasetValidation:
    def test_nonexistent_dataset_path(self, tmp_path: Path) -> None:
        cfg_path = _make_config(tmp_path, dataset_path="/nonexistent/path/that/does/not/exist")

        os.environ["ENGINE_MOCK"] = "1"
        os.environ["MOCK_EPOCH_SLEEP_MS"] = "0"

        from trainer_yolo.train import _mock_train, load_and_validate_config

        cfg = load_and_validate_config(cfg_path)
        with pytest.raises(SystemExit):
            _mock_train(cfg, tmp_path / "output")

    def test_dataset_path_without_dataset_yaml(self, tmp_path: Path) -> None:
        ds = tmp_path / "dataset-empty"
        ds.mkdir()
        # No dataset.yaml inside

        cfg_path = _make_config(tmp_path, dataset_path=str(ds))

        os.environ["ENGINE_MOCK"] = "1"
        os.environ["MOCK_EPOCH_SLEEP_MS"] = "0"

        from trainer_yolo.train import _mock_train, load_and_validate_config

        cfg = load_and_validate_config(cfg_path)
        with pytest.raises(SystemExit):
            _mock_train(cfg, tmp_path / "output")


# ---------------------------------------------------------------------------
# (e) CLI via python -m trainer_yolo train
# ---------------------------------------------------------------------------

class TestCLI:
    def test_cli_train(self, tmp_path: Path) -> None:
        """Run full CLI via subprocess (mirrors orchestrator invocation)."""
        cfg_path = _make_config(tmp_path, epochs=3, seed=42)
        output = tmp_path / "output"
        output.mkdir()

        env = os.environ.copy()
        env["ENGINE_MOCK"] = "1"
        env["MOCK_EPOCH_SLEEP_MS"] = "0"

        result = subprocess.run(
            [
                sys.executable, "-m", "trainer_yolo", "train",
                "--config", str(cfg_path),
                "--output", str(output),
            ],
            env=env,
            capture_output=True,
            text=True,
            timeout=30,
        )

        assert result.returncode == 0, f"CLI failed: {result.stderr}"

        metrics_path = output / "metrics.jsonl"
        assert metrics_path.is_file()

        lines = metrics_path.read_text().strip().split("\n")
        assert len(lines) == 3

        for line in lines:
            m = json.loads(line)
            assert set(m.keys()) == set(METRIC_KEYS)

        assert (output / "best.pt").is_file()
        assert (output / "last.pt").is_file()

    def test_cli_health(self) -> None:
        """Health check (no subcommand) should print JSON and exit 0."""
        result = subprocess.run(
            [sys.executable, "-m", "trainer_yolo"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        assert result.returncode == 0
        data = json.loads(result.stdout)
        assert data["status"] == "ok"
        assert data["engine"] == "trainer-yolo"

    def test_cli_invalid_subcommand(self) -> None:
        result = subprocess.run(
            [sys.executable, "-m", "trainer_yolo", "bogus"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        assert result.returncode != 0


# ---------------------------------------------------------------------------
# (e-extra) Structured stdout output
# ---------------------------------------------------------------------------

class TestStructuredOutput:
    def test_stdout_has_epoch_lines(self, tmp_path: Path) -> None:
        """Each epoch prints a parseable line to stdout."""
        cfg_path = _make_config(tmp_path, epochs=2, seed=1)
        output = tmp_path / "output"
        output.mkdir()

        env = os.environ.copy()
        env["ENGINE_MOCK"] = "1"
        env["MOCK_EPOCH_SLEEP_MS"] = "0"

        result = subprocess.run(
            [
                sys.executable, "-m", "trainer_yolo", "train",
                "--config", str(cfg_path),
                "--output", str(output),
            ],
            env=env,
            capture_output=True,
            text=True,
            timeout=30,
        )

        assert result.returncode == 0
        # Stdout should contain epoch=1/2 and epoch=2/2
        assert "epoch=1/2" in result.stdout
        assert "epoch=2/2" in result.stdout


# ---------------------------------------------------------------------------
# (f) Real training — pure functions (ultralytics mocked)
# ---------------------------------------------------------------------------

def _fake_ultralytics_metrics(
    *,
    box_loss: float = 0.5,
    cls_loss: float = 0.3,
    dfl_loss: float = 0.2,
    map50: float = 0.7,
    map50_95: float = 0.5,
) -> dict:
    """Simulate trainer.metrics dict from ultralytics."""
    return {
        "train/box_loss": box_loss,
        "train/cls_loss": cls_loss,
        "train/dfl_loss": dfl_loss,
        "metrics/mAP50(B)": map50,
        "metrics/mAP50-95(B)": map50_95,
    }


class TestConvertUltralyticsMetrics:
    """Tests for _convert_ultralytics_metrics (pure function)."""

    def test_convert_full_metrics(self) -> None:
        """Full metrics dict → 5 contract keys (excl. epoch) with correct mapping."""
        raw = _fake_ultralytics_metrics(
            box_loss=1.23, cls_loss=0.45, dfl_loss=0.67, map50=0.81, map50_95=0.55
        )
        result = _convert_ultralytics_metrics(raw)
        assert result is not None
        # _convert_ultralytics_metrics returns keys without 'epoch' (epoch is added by _write_metrics_line)
        expected_keys = set(METRIC_KEYS) - {"epoch"}
        assert set(result.keys()) == expected_keys
        assert result["box_loss"] == 1.23
        assert result["cls_loss"] == 0.45
        assert result["dfl_loss"] == 0.67
        assert result["mAP50"] == 0.81
        assert result["mAP50-95"] == 0.55

    def test_convert_map50_key_mapping(self) -> None:
        """mAP50(B) → mAP50 (not mAP50(B))."""
        raw = {"metrics/mAP50(B)": 0.9}
        result = _convert_ultralytics_metrics(raw)
        assert result is not None
        assert "mAP50" in result
        assert result["mAP50"] == 0.9
        assert "metrics/mAP50(B)" not in result

    def test_convert_map50_95_key_mapping(self) -> None:
        """mAP50-95(B) → mAP50-95 (not mAP50-95(B))."""
        raw = {"metrics/mAP50-95(B)": 0.65}
        result = _convert_ultralytics_metrics(raw)
        assert result is not None
        assert "mAP50-95" in result
        assert result["mAP50-95"] == 0.65
        assert "metrics/mAP50-95(B)" not in result

    def test_convert_missing_metrics_honest_skip(self) -> None:
        """Empty metrics dict → all None (honest skip, no crash)."""
        result = _convert_ultralytics_metrics({})
        assert result is not None
        expected_keys = set(METRIC_KEYS) - {"epoch"}
        for key in expected_keys:
            assert result[key] is None, f"Expected None for {key}"

    def test_convert_partial_metrics(self) -> None:
        """Partial metrics → present keys converted, missing keys None."""
        raw = {"train/box_loss": 0.5, "metrics/mAP50(B)": 0.7}
        result = _convert_ultralytics_metrics(raw)
        assert result is not None
        assert result["box_loss"] == 0.5
        assert result["mAP50"] == 0.7
        assert result["cls_loss"] is None
        assert result["dfl_loss"] is None
        assert result["mAP50-95"] is None

    def test_convert_none_values_honest(self) -> None:
        """Explicit None values in raw → None in output (honest)."""
        raw = {k: None for k in [
            "train/box_loss", "train/cls_loss", "train/dfl_loss",
            "metrics/mAP50(B)", "metrics/mAP50-95(B)",
        ]}
        result = _convert_ultralytics_metrics(raw)
        assert result is not None
        expected_keys = set(METRIC_KEYS) - {"epoch"}
        for key in expected_keys:
            assert result[key] is None


class TestWriteMetricsLine:
    """Tests for _write_metrics_line (pure function)."""

    def test_write_single_line(self, tmp_path: Path) -> None:
        """Write one line → file has exactly that line with correct keys."""
        metrics_path = tmp_path / "metrics.jsonl"
        metrics = {
            "box_loss": 0.5, "cls_loss": 0.3, "dfl_loss": 0.2,
            "mAP50": 0.7, "mAP50-95": 0.5,
        }
        _write_metrics_line(metrics_path, epoch=1, metrics=metrics)

        assert metrics_path.is_file()
        line = metrics_path.read_text().strip()
        data = json.loads(line)
        assert set(data.keys()) == set(METRIC_KEYS)
        assert data["epoch"] == 1
        assert data["box_loss"] == 0.5

    def test_write_append_multiple_lines(self, tmp_path: Path) -> None:
        """Append multiple lines → file has N lines, one per epoch."""
        metrics_path = tmp_path / "metrics.jsonl"
        for epoch in range(1, 4):
            metrics = {
                "box_loss": 0.5 / epoch, "cls_loss": 0.3, "dfl_loss": 0.2,
                "mAP50": 0.5 + epoch * 0.1, "mAP50-95": 0.3 + epoch * 0.1,
            }
            _write_metrics_line(metrics_path, epoch=epoch, metrics=metrics)

        lines = metrics_path.read_text().strip().split("\n")
        assert len(lines) == 3
        for i, line in enumerate(lines, start=1):
            data = json.loads(line)
            assert data["epoch"] == i

    def test_write_none_values_skips_line(self, tmp_path: Path) -> None:
        """All None values → no line written (skip when all 5 value metrics are None)."""
        metrics_path = tmp_path / "metrics.jsonl"
        metrics = {
            "box_loss": None, "cls_loss": None, "dfl_loss": None,
            "mAP50": None, "mAP50-95": None,
        }
        _write_metrics_line(metrics_path, epoch=1, metrics=metrics)

        # File should not exist (no line written when all 5 value metrics are None)
        assert not metrics_path.exists()


class TestCopyFlatWeights:
    """Tests for _copy_flat_weights (pure function)."""

    def test_copy_best_and_last(self, tmp_path: Path) -> None:
        """Copy from train/weights/ → flat output dir."""
        output = tmp_path / "output"
        output.mkdir()
        weights_dir = output / "train" / "weights"
        weights_dir.mkdir(parents=True)
        (weights_dir / "best.pt").write_bytes(b"BEST_WEIGHT")
        (weights_dir / "last.pt").write_bytes(b"LAST_WEIGHT")

        _copy_flat_weights(output)

        assert (output / "best.pt").read_bytes() == b"BEST_WEIGHT"
        assert (output / "last.pt").read_bytes() == b"LAST_WEIGHT"

    def test_copy_missing_best_no_crash(self, tmp_path: Path) -> None:
        """Missing best.pt → warning, no crash, last.pt still copied."""
        output = tmp_path / "output"
        output.mkdir()
        weights_dir = output / "train" / "weights"
        weights_dir.mkdir(parents=True)
        (weights_dir / "last.pt").write_bytes(b"LAST_WEIGHT")

        _copy_flat_weights(output)

        assert not (output / "best.pt").exists()
        assert (output / "last.pt").read_bytes() == b"LAST_WEIGHT"

    def test_copy_missing_weights_dir_no_crash(self, tmp_path: Path) -> None:
        """Missing train/weights/ dir → no crash (graceful no-op)."""
        output = tmp_path / "output"
        output.mkdir()

        # Should not raise
        _copy_flat_weights(output)

        assert not (output / "best.pt").exists()
        assert not (output / "last.pt").exists()


class TestRealTrainWeightsPath:
    """Tests for _real_train weights_path support (ADR-0012 I.3b)."""

    def _run_real_train(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch, cfg: dict) -> MagicMock:
        """Helper: mock ultralytics.YOLO and run _real_train, return the mock."""
        from trainer_yolo.train import _real_train

        output = tmp_path / "output"
        output.mkdir(exist_ok=True)

        mock_yolo_cls = MagicMock()
        mock_model = MagicMock()
        mock_yolo_cls.return_value = mock_model

        # Patch the lazy import: ultralytics.YOLO
        import types
        mock_ultralytics = types.ModuleType("ultralytics")
        mock_ultralytics.YOLO = mock_yolo_cls
        monkeypatch.setitem(sys.modules, "ultralytics", mock_ultralytics)

        # Patch _prepare_dataset_yaml to skip filesystem side effects
        monkeypatch.setattr("trainer_yolo.train._prepare_dataset_yaml", lambda *a, **kw: None)

        _real_train(cfg, output)
        return mock_yolo_cls

    def test_real_train_with_weights_path(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        """weights_path present → YOLO called with the path, not the variant."""
        ds = tmp_path / "dataset"
        ds.mkdir()
        (ds / "dataset.yaml").write_text("train: ./train\nval: ./val\nnc: 1\n")

        cfg = {
            "job_id": "test-weights", "engine": "yolo", "model": "yolo11m",
            "mode": "train", "dataset_path": str(ds),
            "output_path": str(tmp_path / "output"), "seed": 42,
            "weights_path": "/outputs/test-weights/weights/best.pt",
            "yolo": {
                "model": "yolo11n", "epochs": 1, "batch": 16, "imgsz": 640,
                "lr0": 0.01, "optimizer": "AdamW",
                "augment": {"mosaic": True, "mixup_flip": False},
            },
        }
        mock_yolo = self._run_real_train(tmp_path, monkeypatch, cfg)
        mock_yolo.assert_called_once_with("/outputs/test-weights/weights/best.pt")

    def test_real_train_without_weights_path(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        """No weights_path → YOLO called with the variant (current behavior)."""
        ds = tmp_path / "dataset"
        ds.mkdir()
        (ds / "dataset.yaml").write_text("train: ./train\nval: ./val\nnc: 1\n")

        cfg = {
            "job_id": "test-no-weights", "engine": "yolo", "model": "yolo11m",
            "mode": "train", "dataset_path": str(ds),
            "output_path": str(tmp_path / "output"), "seed": 42,
            "yolo": {
                "model": "yolo11n", "epochs": 1, "batch": 16, "imgsz": 640,
                "lr0": 0.01, "optimizer": "AdamW",
                "augment": {"mosaic": True, "mixup_flip": False},
            },
        }
        mock_yolo = self._run_real_train(tmp_path, monkeypatch, cfg)
        mock_yolo.assert_called_once_with("yolo11n")

    def test_mock_with_weights_path_deterministic(self, tmp_path: Path) -> None:
        """Mock mode + weights_path present → same deterministic output (key ignored)."""
        from trainer_yolo.train import _mock_train

        ds = tmp_path / "dataset"
        ds.mkdir()
        (ds / "dataset.yaml").write_text("train: ./train\nval: ./val\nnc: 1\n")

        cfg_with = {
            "job_id": "test-mock-weights",
            "engine": "yolo",
            "model": "yolo11m",
            "mode": "train",
            "dataset_path": str(ds),
            "output_path": str(tmp_path / "output"),
            "seed": 42,
            "weights_path": "/outputs/test-mock-weights/weights/best.pt",
            "yolo": {
                "model": "yolo11m",
                "epochs": 2,
                "batch": 16,
                "imgsz": 640,
                "lr0": 0.01,
                "optimizer": "AdamW",
                "augment": {"mosaic": True, "mixup_flip": False},
            },
        }
        cfg_without = {k: v for k, v in cfg_with.items() if k != "weights_path"}

        out_with = tmp_path / "out-with"
        out_with.mkdir()
        out_without = tmp_path / "out-without"
        out_without.mkdir()

        _mock_train(cfg_with, out_with)
        _mock_train(cfg_without, out_without)

        assert (out_with / "metrics.jsonl").read_text() == (out_without / "metrics.jsonl").read_text()
        assert (out_with / "best.pt").read_bytes() == (out_without / "best.pt").read_bytes()
        assert (out_with / "last.pt").read_bytes() == (out_without / "last.pt").read_bytes()


class TestRealTrainTolerantParsing:
    """Tolerant parsing: epoch without metrics does not break the pipeline."""

    def test_epoch_without_metrics_skips_line(self, tmp_path: Path) -> None:
        """Simulate callback with empty trainer.metrics → no line written (all None = skip)."""
        metrics_path = tmp_path / "metrics.jsonl"
        raw = {}
        converted = _convert_ultralytics_metrics(raw)
        assert converted is not None
        _write_metrics_line(metrics_path, epoch=1, metrics=converted)

        # File should not exist (no line written when all 5 value metrics are None)
        assert not metrics_path.exists()

    def test_epoch_with_only_box_loss_writes_line(self, tmp_path: Path) -> None:
        """Epoch with only box_loss numeric + rest None → line written with nulls."""
        metrics_path = tmp_path / "metrics.jsonl"
        raw = {"train/box_loss": 0.5}
        converted = _convert_ultralytics_metrics(raw)
        assert converted is not None
        _write_metrics_line(metrics_path, epoch=1, metrics=converted)

        assert metrics_path.is_file()
        data = json.loads(metrics_path.read_text().strip())
        assert data["epoch"] == 1
        assert data["box_loss"] == 0.5
        assert data["cls_loss"] is None
        assert data["dfl_loss"] is None
        assert data["mAP50"] is None
        assert data["mAP50-95"] is None

    def test_mixed_epochs_some_with_metrics(self, tmp_path: Path) -> None:
        """Some epochs with metrics, some without → only epochs with metrics written."""
        metrics_path = tmp_path / "metrics.jsonl"
        for epoch in range(1, 4):
            if epoch == 2:
                raw = {}  # all None → skip
            else:
                raw = _fake_ultralytics_metrics(box_loss=epoch * 0.1)
            converted = _convert_ultralytics_metrics(raw)
            _write_metrics_line(metrics_path, epoch=epoch, metrics=converted)

        lines = metrics_path.read_text().strip().split("\n")
        # Epoch 2 skipped (all None), so only 2 lines
        assert len(lines) == 2

        # Epoch 1: has metrics
        data1 = json.loads(lines[0])
        assert data1["box_loss"] == pytest.approx(0.1)

        # Epoch 3: has metrics (Epoch 2 was skipped)
        data2 = json.loads(lines[1])
        assert data2["box_loss"] == pytest.approx(0.3)


# ---------------------------------------------------------------------------
# (g) _prepare_dataset_yaml — path rewriting + list→txt conversion
# ---------------------------------------------------------------------------

class TestPrepareDatasetYaml:
    """Tests for _prepare_dataset_yaml (pure, no ultralytics)."""

    def test_relative_path_rewritten_to_absolute(self, tmp_path: Path) -> None:
        """YAML with path: . + absolute dataset_path → path rewritten, train/val intact."""
        ds = tmp_path / "dataset"
        ds.mkdir()
        yaml_content = {"path": ".", "train": "images/a.png", "val": "images/b.png", "nc": 1}
        yaml_path = ds / "dataset.yaml"
        with open(yaml_path, "w") as f:
            yaml.safe_dump(yaml_content, f, sort_keys=False)

        _prepare_dataset_yaml(yaml_path, ds)

        with open(yaml_path, "r") as f:
            result = yaml.safe_load(f)

        assert result["path"] == str(ds)
        assert result["train"] == "images/a.png"
        assert result["val"] == "images/b.png"
        assert result["nc"] == 1

    def test_already_absolute_path_untouched(self, tmp_path: Path) -> None:
        """YAML with path: /abs/já → no rewrite."""
        ds = tmp_path / "dataset"
        ds.mkdir()
        yaml_content = {"path": "/abs/já", "train": "images/a.png", "nc": 1}
        yaml_path = ds / "dataset.yaml"
        with open(yaml_path, "w") as f:
            yaml.safe_dump(yaml_content, f, sort_keys=False)

        _prepare_dataset_yaml(yaml_path, ds)

        with open(yaml_path, "r") as f:
            result = yaml.safe_load(f)

        assert result["path"] == "/abs/já"

    def test_list_train_val_generates_txt_files(self, tmp_path: Path) -> None:
        """List-style train+val → train.txt/val.txt with absolute paths, YAML points to txt."""
        ds = tmp_path / "dataset"
        ds.mkdir()
        images = ds / "images"
        images.mkdir()
        (images / "a.png").write_bytes(b"\x89PNG")
        (images / "b.png").write_bytes(b"\x89PNG")
        (images / "c.png").write_bytes(b"\x89PNG")

        yaml_content = {
            "path": ".",
            "train": ["images/a.png", "images/b.png"],
            "val": ["images/c.png"],
            "nc": 1,
        }
        yaml_path = ds / "dataset.yaml"
        with open(yaml_path, "w") as f:
            yaml.safe_dump(yaml_content, f, sort_keys=False)

        _prepare_dataset_yaml(yaml_path, ds)

        # YAML now points to txt files
        with open(yaml_path, "r") as f:
            result = yaml.safe_load(f)
        assert result["train"] == "train.txt"
        assert result["val"] == "val.txt"

        # train.txt has absolute paths, one per line
        train_txt = (ds / "train.txt").read_text().strip().split("\n")
        assert train_txt == [str(ds / "images/a.png"), str(ds / "images/b.png")]

        # val.txt has absolute path
        val_txt = (ds / "val.txt").read_text().strip().split("\n")
        assert val_txt == [str(ds / "images/c.png")]

    def test_empty_val_points_to_train_txt(self, tmp_path: Path) -> None:
        """Empty val list → val points to train.txt (small dataset fallback)."""
        ds = tmp_path / "dataset"
        ds.mkdir()
        images = ds / "images"
        images.mkdir()
        (images / "a.png").write_bytes(b"\x89PNG")

        yaml_content = {
            "path": ".",
            "train": ["images/a.png"],
            "val": [],
            "nc": 1,
        }
        yaml_path = ds / "dataset.yaml"
        with open(yaml_path, "w") as f:
            yaml.safe_dump(yaml_content, f, sort_keys=False)

        _prepare_dataset_yaml(yaml_path, ds)

        with open(yaml_path, "r") as f:
            result = yaml.safe_load(f)
        assert result["train"] == "train.txt"
        assert result["val"] == "train.txt"

    def test_missing_val_points_to_train_txt(self, tmp_path: Path) -> None:
        """No val key at all → val points to train.txt (small dataset fallback)."""
        ds = tmp_path / "dataset"
        ds.mkdir()
        images = ds / "images"
        images.mkdir()
        (images / "a.png").write_bytes(b"\x89PNG")

        yaml_content = {
            "path": ".",
            "train": ["images/a.png"],
            "nc": 1,
        }
        yaml_path = ds / "dataset.yaml"
        with open(yaml_path, "w") as f:
            yaml.safe_dump(yaml_content, f, sort_keys=False)

        _prepare_dataset_yaml(yaml_path, ds)

        with open(yaml_path, "r") as f:
            result = yaml.safe_load(f)
        assert result["train"] == "train.txt"
        assert result["val"] == "train.txt"
