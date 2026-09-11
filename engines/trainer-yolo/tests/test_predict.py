"""Tests for trainer_yolo.predict (ADR-0013 D3, passo J.1).

Covers:
  (a) Config parsing — valid shape, missing keys, missing predict section, conf out of range,
      missing weights_path.
  (b) Mock run in tempdir — predictions.json shape (engine yolo, model, conf, images[].boxes[]),
      domains 0..1, no metrics.jsonl.
  (c) Determinism — same seed/filenames/classes → byte-identical predictions.json.
  (d) dataset_path nonexistent / empty → error.
  (e) CLI via python -m trainer_yolo predict --config ... --output ...
  (f) Real predict — YOLO monkeypatched, xywhn→top-left clamp, class from model.names,
      empty detection → boxes [].
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import types
from pathlib import Path
from unittest.mock import MagicMock

import pytest
import yaml

# Ensure mock mode
os.environ.setdefault("ENGINE_MOCK", "1")
os.environ["MOCK_EPOCH_SLEEP_MS"] = "0"

from trainer_yolo.predict import (
    load_and_validate_predict_config,
    _mock_predict,
    _real_predict,
    _xywhn_to_topleft_clamped,
)


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

def _make_predict_config(
    tmp_path: Path,
    *,
    seed: int = 42,
    conf: float = 0.65,
    dataset_path: str | None = None,
    weights_path: str | None = "/outputs/job/weights/best.pt",
    missing_keys: set[str] | None = None,
    missing_predict: bool = False,
    bad_conf: bool = False,
) -> Path:
    """Write a valid predict config.yaml to tmp_path and return its path."""
    if dataset_path is None:
        ds = tmp_path / "dataset"
        ds.mkdir(exist_ok=True)
        (ds / "dataset.yaml").write_text(
            "path: .\n"
            "train: [images/img_0001.jpg, images/img_0002.jpg]\n"
            "val: [images/img_0003.jpg]\n"
            "names:\n"
            "  0: solda_fria\n"
            "  1: solda_quente\n"
        )
        dataset_path = str(ds)

    cfg: dict = {
        "job_id": "test-job-predict-001",
        "engine": "yolo",
        "model": "yolo11m",
        "mode": "predict",
        "dataset_path": dataset_path,
        "output_path": str(tmp_path / "output"),
        "seed": seed,
        "predict": {
            "conf": 999.0 if bad_conf else conf,
        },
    }

    # Only add weights_path if explicitly provided
    if weights_path is not None:
        cfg["weights_path"] = weights_path

    if missing_keys:
        for k in missing_keys:
            cfg.pop(k, None)
    if missing_predict:
        del cfg["predict"]

    cfg_path = tmp_path / "config.yaml"
    cfg_path.write_text(yaml.dump(cfg, default_flow_style=False))
    return cfg_path


def _make_dataset(
    tmp_path: Path,
    *,
    classes: dict[int, str] | None = None,
    train_images: list[str] | None = None,
    val_images: list[str] | None = None,
    empty: bool = False,
) -> Path:
    """Create a minimal dataset directory with dataset.yaml."""
    ds = tmp_path / "dataset"
    ds.mkdir(exist_ok=True)

    if empty:
        (ds / "dataset.yaml").write_text("path: .\ntrain: []\nval: []\nnames:\n")
        return ds

    if classes is None:
        classes = {0: "solda_fria", 1: "solda_quente"}
    if train_images is None:
        train_images = ["images/img_0001.jpg", "images/img_0002.jpg"]
    if val_images is None:
        val_images = ["images/img_0003.jpg"]

    names_yaml = "\n".join(f"  {k}: {v}" for k, v in sorted(classes.items()))
    train_str = ", ".join(train_images) if train_images else ""
    val_str = ", ".join(val_images) if val_images else ""

    (ds / "dataset.yaml").write_text(
        f"path: .\ntrain: [{train_str}]\nval: [{val_str}]\nnames:\n{names_yaml}\n"
    )
    return ds


# ---------------------------------------------------------------------------
# (a) Config parsing
# ---------------------------------------------------------------------------

class TestPredictConfigParsing:
    def test_valid_config(self, tmp_path: Path) -> None:
        cfg_path = _make_predict_config(tmp_path)
        cfg = load_and_validate_predict_config(cfg_path)
        assert cfg["engine"] == "yolo"
        assert cfg["predict"]["conf"] == 0.65
        assert cfg["weights_path"] == "/outputs/job/weights/best.pt"

    def test_missing_required_keys(self, tmp_path: Path) -> None:
        cfg_path = _make_predict_config(tmp_path, missing_keys={"seed"})
        with pytest.raises(SystemExit):
            load_and_validate_predict_config(cfg_path)

    def test_missing_weights_path(self, tmp_path: Path) -> None:
        cfg_path = _make_predict_config(tmp_path, weights_path=None)
        with pytest.raises(SystemExit):
            load_and_validate_predict_config(cfg_path)

    def test_missing_predict_section(self, tmp_path: Path) -> None:
        cfg_path = _make_predict_config(tmp_path, missing_predict=True)
        with pytest.raises(SystemExit):
            load_and_validate_predict_config(cfg_path)

    def test_conf_out_of_range(self, tmp_path: Path) -> None:
        cfg_path = _make_predict_config(tmp_path, bad_conf=True)
        with pytest.raises(SystemExit):
            load_and_validate_predict_config(cfg_path)

    def test_nonexistent_config_file(self, tmp_path: Path) -> None:
        with pytest.raises(SystemExit):
            load_and_validate_predict_config(tmp_path / "nonexistent.yaml")


# ---------------------------------------------------------------------------
# (b) Mock run — shape, domains, no metrics
# ---------------------------------------------------------------------------

class TestPredictMockRun:
    def test_predictions_json_shape(self, tmp_path: Path) -> None:
        """predictions.json has correct top-level keys and per-image structure."""
        ds = _make_dataset(tmp_path)
        cfg_path = _make_predict_config(tmp_path, dataset_path=str(ds))
        cfg = load_and_validate_predict_config(cfg_path)
        output = tmp_path / "output"

        _mock_predict(cfg, output)

        predictions_path = output / "predictions.json"
        assert predictions_path.is_file()

        data = json.loads(predictions_path.read_text())
        assert set(data.keys()) == {"engine", "model", "conf", "images"}
        assert data["engine"] == "yolo"
        assert data["model"] == "yolo11m"
        assert isinstance(data["conf"], (int, float))
        assert len(data["images"]) == 3  # img_0001, img_0002, img_0003

        for img in data["images"]:
            assert "filename" in img
            assert "boxes" in img
            assert isinstance(img["boxes"], list)
            assert len(img["boxes"]) >= 1
            for box in img["boxes"]:
                assert set(box.keys()) == {"class", "x", "y", "w", "h", "conf"}
                assert 0.0 <= box["x"] <= 1.0
                assert 0.0 <= box["y"] <= 1.0
                assert 0.0 <= box["w"] <= 1.0
                assert 0.0 <= box["h"] <= 1.0
                assert 0.0 <= box["conf"] <= 1.0

    def test_conf_threshold_honored(self, tmp_path: Path) -> None:
        """All box conf values are >= the configured conf threshold."""
        conf_threshold = 0.65
        ds = _make_dataset(tmp_path)
        cfg_path = _make_predict_config(tmp_path, conf=conf_threshold, dataset_path=str(ds))
        cfg = load_and_validate_predict_config(cfg_path)
        output = tmp_path / "output"

        _mock_predict(cfg, output)

        data = json.loads((output / "predictions.json").read_text())
        for img in data["images"]:
            for box in img["boxes"]:
                assert box["conf"] >= conf_threshold, f"conf {box['conf']} < threshold {conf_threshold}"

    def test_no_metrics_jsonl(self, tmp_path: Path) -> None:
        """predict does NOT produce metrics.jsonl (progresso binário honesto — D3)."""
        ds = _make_dataset(tmp_path)
        cfg_path = _make_predict_config(tmp_path, dataset_path=str(ds))
        cfg = load_and_validate_predict_config(cfg_path)
        output = tmp_path / "output"

        _mock_predict(cfg, output)

        assert not (output / "metrics.jsonl").exists()

    def test_engine_yolo_not_autotracker(self, tmp_path: Path) -> None:
        """predictions.json has engine 'yolo', not 'autotracker'."""
        ds = _make_dataset(tmp_path)
        cfg_path = _make_predict_config(tmp_path, dataset_path=str(ds))
        cfg = load_and_validate_predict_config(cfg_path)
        output = tmp_path / "output"

        _mock_predict(cfg, output)

        data = json.loads((output / "predictions.json").read_text())
        assert data["engine"] == "yolo"

    def test_no_boxes_or_labels_dir(self, tmp_path: Path) -> None:
        """Should NOT produce labels/ or samples/ directories."""
        ds = _make_dataset(tmp_path)
        cfg_path = _make_predict_config(tmp_path, dataset_path=str(ds))
        cfg = load_and_validate_predict_config(cfg_path)
        output = tmp_path / "output"

        _mock_predict(cfg, output)

        assert not (output / "labels").exists()
        assert not (output / "samples").exists()


# ---------------------------------------------------------------------------
# (c) Determinism
# ---------------------------------------------------------------------------

class TestPredictDeterminism:
    def test_same_seed_same_predictions(self, tmp_path: Path) -> None:
        """Two runs with same seed/filenames/classes → byte-identical predictions.json."""
        ds = _make_dataset(tmp_path)

        def _run(label: str) -> Path:
            cfg_path = _make_predict_config(tmp_path, seed=42, dataset_path=str(ds))
            cfg = load_and_validate_predict_config(cfg_path)
            out = tmp_path / f"out-{label}"
            _mock_predict(cfg, out)
            return out

        out1 = _run("a")
        out2 = _run("b")

        assert (out1 / "predictions.json").read_bytes() == (out2 / "predictions.json").read_bytes()

    def test_different_seed_different_predictions(self, tmp_path: Path) -> None:
        """Different seeds → different predictions.json content."""
        ds = _make_dataset(tmp_path)

        def _run(seed: int) -> Path:
            cfg_path = _make_predict_config(tmp_path, seed=seed, dataset_path=str(ds))
            cfg = load_and_validate_predict_config(cfg_path)
            out = tmp_path / f"out-{seed}"
            _mock_predict(cfg, out)
            return out

        out1 = _run(42)
        out2 = _run(99)

        assert (out1 / "predictions.json").read_bytes() != (out2 / "predictions.json").read_bytes()

    def test_mock_ignores_weights_path(self, tmp_path: Path) -> None:
        """Mock predict ignores weights_path — same output regardless of path."""
        ds = _make_dataset(tmp_path)

        cfg_with = {
            "job_id": "test", "engine": "yolo", "model": "yolo11m",
            "mode": "predict", "dataset_path": str(ds),
            "output_path": str(tmp_path / "output"), "seed": 42,
            "weights_path": "/path/to/weights.pt",
            "predict": {"conf": 0.65},
        }
        cfg_without = {k: v for k, v in cfg_with.items() if k != "weights_path"}

        out_with = tmp_path / "out-with"
        out_without = tmp_path / "out-without"

        _mock_predict(cfg_with, out_with)
        _mock_predict(cfg_without, out_without)

        assert (out_with / "predictions.json").read_bytes() == (out_without / "predictions.json").read_bytes()


# ---------------------------------------------------------------------------
# (d) Dataset validation
# ---------------------------------------------------------------------------

class TestPredictDatasetValidation:
    def test_nonexistent_dataset_path(self, tmp_path: Path) -> None:
        cfg_path = _make_predict_config(
            tmp_path, dataset_path="/nonexistent/path/that/does/not/exist"
        )
        cfg = load_and_validate_predict_config(cfg_path)
        with pytest.raises(SystemExit):
            _mock_predict(cfg, tmp_path / "output")

    def test_empty_dataset(self, tmp_path: Path) -> None:
        ds = _make_dataset(tmp_path, empty=True)
        cfg_path = _make_predict_config(tmp_path, dataset_path=str(ds))
        cfg = load_and_validate_predict_config(cfg_path)
        with pytest.raises(SystemExit):
            _mock_predict(cfg, tmp_path / "output")


# ---------------------------------------------------------------------------
# (e) CLI via python -m trainer_yolo predict
# ---------------------------------------------------------------------------

class TestPredictCLI:
    def test_cli_predict(self, tmp_path: Path) -> None:
        """Run full CLI via subprocess (mirrors orchestrator invocation)."""
        ds = _make_dataset(tmp_path)
        cfg_path = _make_predict_config(tmp_path, seed=42, dataset_path=str(ds))
        output = tmp_path / "output"

        env = os.environ.copy()
        env["ENGINE_MOCK"] = "1"

        result = subprocess.run(
            [
                sys.executable, "-m", "trainer_yolo", "predict",
                "--config", str(cfg_path),
                "--output", str(output),
            ],
            env=env,
            capture_output=True,
            text=True,
            timeout=30,
        )

        assert result.returncode == 0, f"CLI failed: {result.stderr}"

        predictions_path = output / "predictions.json"
        assert predictions_path.is_file()

        data = json.loads(predictions_path.read_text())
        assert data["engine"] == "yolo"
        assert data["model"] == "yolo11m"
        assert len(data["images"]) == 3

    def test_cli_predict_invalid_config(self, tmp_path: Path) -> None:
        """Missing predict section → exit != 0."""
        cfg_path = _make_predict_config(tmp_path, missing_predict=True)
        output = tmp_path / "output"

        result = subprocess.run(
            [
                sys.executable, "-m", "trainer_yolo", "predict",
                "--config", str(cfg_path),
                "--output", str(output),
            ],
            capture_output=True,
            text=True,
            timeout=10,
        )

        assert result.returncode != 0

    def test_cli_train_still_works(self, tmp_path: Path) -> None:
        """Ensure existing 'train' subcommand is not broken."""
        ds = tmp_path / "dataset"
        ds.mkdir(exist_ok=True)
        (ds / "dataset.yaml").write_text("train: ./train\nval: ./val\nnc: 1\n")

        cfg = {
            "job_id": "test-job-001",
            "engine": "yolo",
            "model": "yolo11m",
            "mode": "train",
            "dataset_path": str(ds),
            "output_path": str(tmp_path / "output"),
            "seed": 42,
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
        cfg_path = tmp_path / "config.yaml"
        cfg_path.write_text(yaml.dump(cfg))
        output = tmp_path / "output-train"
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
        assert (output / "metrics.jsonl").is_file()
        assert (output / "best.pt").is_file()

    def test_cli_autotrack_still_works(self, tmp_path: Path) -> None:
        """Ensure existing 'autotrack' subcommand is not broken."""
        ds = _make_dataset(tmp_path)
        cfg = {
            "job_id": "test-job-auto",
            "engine": "autotracker",
            "model": "mock",
            "mode": "autotrack",
            "dataset_path": str(ds),
            "output_path": str(tmp_path / "output"),
            "seed": 42,
            "autotrack": {"model": "mock", "conf": 0.65},
        }
        cfg_path = tmp_path / "config.yaml"
        cfg_path.write_text(yaml.dump(cfg))
        output = tmp_path / "output-auto"
        output.mkdir()

        env = os.environ.copy()
        env["ENGINE_MOCK"] = "1"

        result = subprocess.run(
            [
                sys.executable, "-m", "trainer_yolo", "autotrack",
                "--config", str(cfg_path),
                "--output", str(output),
            ],
            env=env,
            capture_output=True,
            text=True,
            timeout=30,
        )

        assert result.returncode == 0
        assert (output / "boxes.json").is_file()
        assert (output / "metrics.jsonl").is_file()


# ---------------------------------------------------------------------------
# (f) Real predict — ultralytics monkeypatched
# ---------------------------------------------------------------------------

def _make_fake_result(path: str, boxes_xywhn: list, clss: list, confs: list) -> MagicMock:
    """Create a fake ultralytics Results object for testing.

    Returns mock objects with .tolist() and len() support, avoiding torch dependency.
    """
    result = MagicMock()
    result.path = path

    # Create a list-like mock for xywhn that supports iteration and indexing
    class MockBoxes:
        def __init__(self, xywhn, clss, confs):
            self.xywhn = xywhn
            self.cls = clss
            self.conf = confs
            self._len = len(xywhn)

        def __len__(self):
            return self._len

    # Each xywhn entry needs .tolist()
    class MockCoord:
        def __init__(self, values):
            self._values = values
        def tolist(self):
            return self._values

    mock_xywhn = [MockCoord(row) for row in boxes_xywhn]
    # cls and conf: need indexing with [i]
    result.boxes = MockBoxes(mock_xywhn, clss, confs)

    return result


class TestRealPredict:
    """Tests for _real_predict — real ultralytics path monkeypatched."""

    def test_real_predict_calls_yolo_with_weights(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        """YOLO called with weights_path, predict called with correct args."""
        ds = _make_dataset(tmp_path)

        # Fake result: 1 box for img_0001.jpg
        fake_result = _make_fake_result(
            str(ds / "images" / "img_0001.jpg"),
            boxes_xywhn=[[0.5, 0.5, 0.3, 0.4]],
            clss=[0],
            confs=[0.92],
        )
        fake_result_empty = MagicMock()
        fake_result_empty.path = str(ds / "images" / "img_0002.jpg")
        fake_result_empty.boxes = MagicMock()
        fake_result_empty.boxes.xywhn = []
        fake_result_empty.boxes.cls = []
        fake_result_empty.boxes.conf = []
        fake_result_empty.boxes.__len__ = lambda self: 0

        mock_yolo_cls = MagicMock()
        mock_model = MagicMock()
        mock_model.predict.return_value = [fake_result, fake_result_empty]
        mock_model.names = {0: "solda_fria", 1: "solda_quente"}
        mock_yolo_cls.return_value = mock_model

        # Patch ultralytics module
        mock_ultralytics = types.ModuleType("ultralytics")
        mock_ultralytics.YOLO = mock_yolo_cls
        monkeypatch.setitem(sys.modules, "ultralytics", mock_ultralytics)

        cfg = {
            "job_id": "test-real", "engine": "yolo", "model": "yolo11m",
            "mode": "predict", "dataset_path": str(ds),
            "output_path": str(tmp_path / "output"), "seed": 42,
            "weights_path": str(tmp_path / "weights.pt"),
            "predict": {"conf": 0.5},
        }
        output = tmp_path / "output"
        output.mkdir(exist_ok=True)

        _real_predict(cfg, output)

        # YOLO called with weights_path
        mock_yolo_cls.assert_called_once_with(str(tmp_path / "weights.pt"))

        # predict called with correct args
        mock_model.predict.assert_called_once_with(
            source=str(ds), conf=0.5, imgsz=640, device=0,
        )

        # Verify predictions.json
        predictions = json.loads((output / "predictions.json").read_text())
        assert predictions["engine"] == "yolo"
        assert predictions["model"] == "yolo11m"
        assert predictions["conf"] == 0.5

        # First image has a box, second is empty
        assert len(predictions["images"]) == 2
        assert predictions["images"][0]["filename"] == "img_0001.jpg"
        assert len(predictions["images"][0]["boxes"]) == 1
        assert predictions["images"][0]["boxes"][0]["class"] == "solda_fria"

        assert predictions["images"][1]["filename"] == "img_0002.jpg"
        assert predictions["images"][1]["boxes"] == []

    def test_xywhn_to_topleft_clamped(self) -> None:
        """xywhn center-based → top-left with clamp."""
        # Normal case: box fits
        x, y, w, h = _xywhn_to_topleft_clamped(0.5, 0.5, 0.2, 0.3)
        assert x == pytest.approx(0.4)
        assert y == pytest.approx(0.35)
        assert w == pytest.approx(0.2)
        assert h == pytest.approx(0.3)

        # Clamp: box goes beyond 1.0
        # cx=0.95, w=0.2 → x = 0.95 - 0.1 = 0.85, but right-edge clamp → min(0.85, 1-0.2) = 0.8
        x, y, w, h = _xywhn_to_topleft_clamped(0.95, 0.95, 0.2, 0.2)
        assert x == pytest.approx(0.8)
        assert y == pytest.approx(0.8)
        assert w == pytest.approx(0.2)
        assert h == pytest.approx(0.2)

        # Clamp: box goes below 0.0
        x, y, w, h = _xywhn_to_topleft_clamped(0.05, 0.05, 0.2, 0.2)
        assert x == pytest.approx(0.0)
        assert y == pytest.approx(0.0)
        assert w == pytest.approx(0.2)
        assert h == pytest.approx(0.2)

    def test_real_predict_empty_detection(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        """Image with no detection → boxes [] in predictions.json."""
        ds = _make_dataset(tmp_path)

        fake_result = MagicMock()
        fake_result.path = str(ds / "images" / "img_0001.jpg")
        fake_result.boxes = MagicMock()
        fake_result.boxes.xywhn = []
        fake_result.boxes.cls = []
        fake_result.boxes.conf = []

        mock_yolo_cls = MagicMock()
        mock_model = MagicMock()
        mock_model.predict.return_value = [fake_result]
        mock_model.names = {0: "solda_fria"}
        mock_yolo_cls.return_value = mock_model

        mock_ultralytics = types.ModuleType("ultralytics")
        mock_ultralytics.YOLO = mock_yolo_cls
        monkeypatch.setitem(sys.modules, "ultralytics", mock_ultralytics)

        cfg = {
            "job_id": "test-empty", "engine": "yolo", "model": "yolo11m",
            "mode": "predict", "dataset_path": str(ds),
            "output_path": str(tmp_path / "output"), "seed": 42,
            "weights_path": str(tmp_path / "weights.pt"),
            "predict": {"conf": 0.65},
        }
        output = tmp_path / "output"
        output.mkdir(exist_ok=True)

        _real_predict(cfg, output)

        predictions = json.loads((output / "predictions.json").read_text())
        assert len(predictions["images"]) == 1
        assert predictions["images"][0]["boxes"] == []

    def test_real_predict_nonexistent_dataset(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        """Nonexistent dataset_path → _die."""
        mock_ultralytics = types.ModuleType("ultralytics")
        mock_ultralytics.YOLO = MagicMock()
        monkeypatch.setitem(sys.modules, "ultralytics", mock_ultralytics)

        cfg = {
            "job_id": "test-nodir", "engine": "yolo", "model": "yolo11m",
            "mode": "predict", "dataset_path": "/nonexistent/path",
            "output_path": str(tmp_path / "output"), "seed": 42,
            "weights_path": str(tmp_path / "weights.pt"),
            "predict": {"conf": 0.65},
        }
        with pytest.raises(SystemExit):
            _real_predict(cfg, tmp_path / "output")

    def test_real_predict_ultralytics_not_installed(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        """ultralytics not importable → _die with install hint."""
        # Ensure ultralytics is not in sys.modules
        monkeypatch.delitem(sys.modules, "ultralytics", raising=False)

        # Make import fail
        import builtins
        real_import = builtins.__import__

        def _no_ultralytics(name: str, *args, **kwargs):
            if name == "ultralytics":
                raise ImportError("No module named 'ultralytics'")
            return real_import(name, *args, **kwargs)

        monkeypatch.setattr(builtins, "__import__", _no_ultralytics)

        cfg = {
            "job_id": "test-no-ultra", "engine": "yolo", "model": "yolo11m",
            "mode": "predict", "dataset_path": str(tmp_path),
            "output_path": str(tmp_path / "output"), "seed": 42,
            "weights_path": str(tmp_path / "weights.pt"),
            "predict": {"conf": 0.65},
        }
        with pytest.raises(SystemExit):
            _real_predict(cfg, tmp_path / "output")
