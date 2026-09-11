"""Tests for trainer_yolo.autotrack (ADR-0008 D2, ADR-0014 D3, passos A.1/K.1).

Covers:
  (a) Config parsing — valid shape, missing keys, missing autotrack section, conf out of range.
  (b) Mock run in tempdir — boxes.json shape D1, metrics.jsonl 1 line 6 keys, domains 0..1.
  (c) Determinism — same seed/filenames/classes → byte-identical boxes.json.
  (d) dataset_path nonexistent / empty → error.
  (e) CLI via python -m trainer_yolo autotrack --config ... --output ...
  (f) Real autotrack — ultralytics monkeypatched, set_classes, predict args, seed 0, no metrics.
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

from trainer_yolo.autotrack import (
    load_and_validate_autotrack_config,
    _mock_autotrack,
    _read_dataset,
    _box_for_image,
    _generate_boxes_for_image,
    _xywhn_to_topleft_clamped,
)
from trainer_yolo.train import METRIC_KEYS


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

def _make_autotrack_config(
    tmp_path: Path,
    *,
    seed: int = 42,
    conf: float = 0.65,
    dataset_path: str | None = None,
    missing_keys: set[str] | None = None,
    missing_autotrack: bool = False,
    bad_conf: bool = False,
) -> Path:
    """Write a valid autotrack config.yaml to tmp_path and return its path."""
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
        "job_id": "test-job-autotrack-001",
        "engine": "autotracker",
        "model": "mock",
        "mode": "autotrack",
        "dataset_path": dataset_path,
        "output_path": str(tmp_path / "output"),
        "seed": seed,
        "autotrack": {
            "model": "mock",
            "conf": 999.0 if bad_conf else conf,
        },
    }

    if missing_keys:
        for k in missing_keys:
            cfg.pop(k, None)
    if missing_autotrack:
        del cfg["autotrack"]

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

class TestAutotrackConfigParsing:
    def test_valid_config(self, tmp_path: Path) -> None:
        cfg_path = _make_autotrack_config(tmp_path)
        cfg = load_and_validate_autotrack_config(cfg_path)
        assert cfg["engine"] == "autotracker"
        assert cfg["autotrack"]["conf"] == 0.65

    def test_missing_required_keys(self, tmp_path: Path) -> None:
        cfg_path = _make_autotrack_config(tmp_path, missing_keys={"seed"})
        with pytest.raises(SystemExit):
            load_and_validate_autotrack_config(cfg_path)

    def test_missing_autotrack_section(self, tmp_path: Path) -> None:
        cfg_path = _make_autotrack_config(tmp_path, missing_autotrack=True)
        with pytest.raises(SystemExit):
            load_and_validate_autotrack_config(cfg_path)

    def test_conf_out_of_range(self, tmp_path: Path) -> None:
        cfg_path = _make_autotrack_config(tmp_path, bad_conf=True)
        with pytest.raises(SystemExit):
            load_and_validate_autotrack_config(cfg_path)

    def test_nonexistent_config_file(self, tmp_path: Path) -> None:
        with pytest.raises(SystemExit):
            load_and_validate_autotrack_config(tmp_path / "nonexistent.yaml")


# ---------------------------------------------------------------------------
# (b) Mock run — shape, domains, metrics
# ---------------------------------------------------------------------------

class TestAutotrackMockRun:
    def test_boxes_json_shape(self, tmp_path: Path) -> None:
        """boxes.json has correct top-level keys and per-image structure."""
        ds = _make_dataset(tmp_path)
        cfg_path = _make_autotrack_config(tmp_path, dataset_path=str(ds))
        cfg = load_and_validate_autotrack_config(cfg_path)
        output = tmp_path / "output"

        _mock_autotrack(cfg, output)

        boxes_path = output / "boxes.json"
        assert boxes_path.is_file()

        data = json.loads(boxes_path.read_text())
        assert set(data.keys()) == {"engine", "model", "seed", "conf", "images"}
        assert data["engine"] == "autotracker"
        assert data["model"] == "mock"
        assert isinstance(data["seed"], int)
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
        cfg_path = _make_autotrack_config(tmp_path, conf=conf_threshold, dataset_path=str(ds))
        cfg = load_and_validate_autotrack_config(cfg_path)
        output = tmp_path / "output"

        _mock_autotrack(cfg, output)

        data = json.loads((output / "boxes.json").read_text())
        for img in data["images"]:
            for box in img["boxes"]:
                assert box["conf"] >= conf_threshold, f"conf {box['conf']} < threshold {conf_threshold}"

    def test_metrics_jsonl_one_line_six_keys(self, tmp_path: Path) -> None:
        """metrics.jsonl has exactly 1 line with the 6 required keys."""
        ds = _make_dataset(tmp_path)
        cfg_path = _make_autotrack_config(tmp_path, dataset_path=str(ds))
        cfg = load_and_validate_autotrack_config(cfg_path)
        output = tmp_path / "output"

        _mock_autotrack(cfg, output)

        metrics_path = output / "metrics.jsonl"
        assert metrics_path.is_file()

        lines = metrics_path.read_text().strip().split("\n")
        assert len(lines) == 1, f"Expected 1 line, got {len(lines)}"

        m = json.loads(lines[0])
        assert set(m.keys()) == set(METRIC_KEYS), f"Unexpected keys: {m.keys()}"
        assert isinstance(m["epoch"], int)
        assert m["epoch"] == 1
        assert isinstance(m["box_loss"], float)
        assert isinstance(m["cls_loss"], float)
        assert isinstance(m["dfl_loss"], float)
        assert isinstance(m["mAP50"], float)
        assert isinstance(m["mAP50-95"], float)

    def test_no_boxes_or_labels_dir(self, tmp_path: Path) -> None:
        """Should NOT produce labels/ or samples/ directories."""
        ds = _make_dataset(tmp_path)
        cfg_path = _make_autotrack_config(tmp_path, dataset_path=str(ds))
        cfg = load_and_validate_autotrack_config(cfg_path)
        output = tmp_path / "output"

        _mock_autotrack(cfg, output)

        assert not (output / "labels").exists()
        assert not (output / "samples").exists()


# ---------------------------------------------------------------------------
# (c) Determinism
# ---------------------------------------------------------------------------

class TestAutotrackDeterminism:
    def test_same_seed_same_boxes(self, tmp_path: Path) -> None:
        """Two runs with same seed/filenames/classes → byte-identical boxes.json."""
        ds = _make_dataset(tmp_path)

        def _run(label: str) -> Path:
            cfg_path = _make_autotrack_config(tmp_path, seed=42, dataset_path=str(ds))
            cfg = load_and_validate_autotrack_config(cfg_path)
            out = tmp_path / f"out-{label}"
            _mock_autotrack(cfg, out)
            return out

        out1 = _run("a")
        out2 = _run("b")

        assert (out1 / "boxes.json").read_bytes() == (out2 / "boxes.json").read_bytes()
        assert (out1 / "metrics.jsonl").read_bytes() == (out2 / "metrics.jsonl").read_bytes()

    def test_different_seed_different_boxes(self, tmp_path: Path) -> None:
        """Different seeds → different boxes.json content."""
        ds = _make_dataset(tmp_path)

        def _run(seed: int) -> Path:
            cfg_path = _make_autotrack_config(tmp_path, seed=seed, dataset_path=str(ds))
            cfg = load_and_validate_autotrack_config(cfg_path)
            out = tmp_path / f"out-{seed}"
            _mock_autotrack(cfg, out)
            return out

        out1 = _run(42)
        out2 = _run(99)

        assert (out1 / "boxes.json").read_bytes() != (out2 / "boxes.json").read_bytes()


# ---------------------------------------------------------------------------
# (d) Dataset validation
# ---------------------------------------------------------------------------

class TestAutotrackDatasetValidation:
    def test_nonexistent_dataset_path(self, tmp_path: Path) -> None:
        cfg_path = _make_autotrack_config(
            tmp_path, dataset_path="/nonexistent/path/that/does/not/exist"
        )
        cfg = load_and_validate_autotrack_config(cfg_path)
        with pytest.raises(SystemExit):
            _mock_autotrack(cfg, tmp_path / "output")

    def test_empty_dataset(self, tmp_path: Path) -> None:
        ds = _make_dataset(tmp_path, empty=True)
        cfg_path = _make_autotrack_config(tmp_path, dataset_path=str(ds))
        cfg = load_and_validate_autotrack_config(cfg_path)
        with pytest.raises(SystemExit):
            _mock_autotrack(cfg, tmp_path / "output")


# ---------------------------------------------------------------------------
# (e) CLI via python -m trainer_yolo autotrack
# ---------------------------------------------------------------------------

class TestAutotrackCLI:
    def test_cli_autotrack(self, tmp_path: Path) -> None:
        """Run full CLI via subprocess (mirrors orchestrator invocation)."""
        ds = _make_dataset(tmp_path)
        cfg_path = _make_autotrack_config(tmp_path, seed=42, dataset_path=str(ds))
        output = tmp_path / "output"

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

        assert result.returncode == 0, f"CLI failed: {result.stderr}"

        boxes_path = output / "boxes.json"
        assert boxes_path.is_file()

        metrics_path = output / "metrics.jsonl"
        assert metrics_path.is_file()

        data = json.loads(boxes_path.read_text())
        assert data["engine"] == "autotracker"
        assert len(data["images"]) == 3

        lines = metrics_path.read_text().strip().split("\n")
        assert len(lines) == 1
        m = json.loads(lines[0])
        assert set(m.keys()) == set(METRIC_KEYS)
        assert m["epoch"] == 1

    def test_cli_autotrack_invalid_config(self, tmp_path: Path) -> None:
        """Missing autotrack section → exit != 0."""
        cfg_path = _make_autotrack_config(tmp_path, missing_autotrack=True)
        output = tmp_path / "output"

        result = subprocess.run(
            [
                sys.executable, "-m", "trainer_yolo", "autotrack",
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


# ---------------------------------------------------------------------------
# (f) Edge cases
# ---------------------------------------------------------------------------

class TestAutotrackEdgeCases:
    def test_single_class_single_image(self, tmp_path: Path) -> None:
        """Works with 1 class and 1 image."""
        ds = _make_dataset(
            tmp_path,
            classes={0: "defect"},
            train_images=["images/img.jpg"],
            val_images=[],
        )
        cfg_path = _make_autotrack_config(tmp_path, dataset_path=str(ds))
        cfg = load_and_validate_autotrack_config(cfg_path)
        output = tmp_path / "output"

        _mock_autotrack(cfg, output)

        data = json.loads((output / "boxes.json").read_text())
        assert len(data["images"]) == 1
        assert data["images"][0]["filename"] == "img.jpg"
        assert len(data["images"][0]["boxes"]) >= 1

    def test_many_classes(self, tmp_path: Path) -> None:
        """Works with many classes (up to 3 boxes per image)."""
        classes = {i: f"class_{i}" for i in range(10)}
        ds = _make_dataset(
            tmp_path,
            classes=classes,
            train_images=["images/a.jpg"],
            val_images=[],
        )
        cfg_path = _make_autotrack_config(tmp_path, dataset_path=str(ds))
        cfg = load_and_validate_autotrack_config(cfg_path)
        output = tmp_path / "output"

        _mock_autotrack(cfg, output)

        data = json.loads((output / "boxes.json").read_text())
        for img in data["images"]:
            assert len(img["boxes"]) <= 3  # capped at 3

    def test_read_dataset_extracts_filenames(self, tmp_path: Path) -> None:
        """_read_dataset extracts just filenames from paths like 'images/img.jpg'."""
        ds = _make_dataset(tmp_path)
        class_names, filenames = _read_dataset(ds)
        assert class_names == ["solda_fria", "solda_quente"]
        assert filenames == ["img_0001.jpg", "img_0002.jpg", "img_0003.jpg"]

    def test_box_for_image_deterministic(self) -> None:
        """Same inputs → same box dict."""
        b1 = _box_for_image(42, "img.jpg", "solda_fria", 0.65)
        b2 = _box_for_image(42, "img.jpg", "solda_fria", 0.65)
        assert b1 == b2


# ---------------------------------------------------------------------------
# (f) Real autotrack — ultralytics monkeypatched (ADR-0014 D3)
# ---------------------------------------------------------------------------

def _make_fake_result(path: str, boxes_xywhn: list, clss: list, confs: list) -> MagicMock:
    """Create a fake ultralytics Results object for testing.

    Returns mock objects with .tolist() and len() support, avoiding torch dependency.
    """
    result = MagicMock()
    result.path = path

    class MockBoxes:
        def __init__(self, xywhn, clss, confs):
            self.xywhn = xywhn
            self.cls = clss
            self.conf = confs
            self._len = len(xywhn)

        def __len__(self):
            return self._len

    class MockCoord:
        def __init__(self, values):
            self._values = values
        def tolist(self):
            return self._values

    mock_xywhn = [MockCoord(row) for row in boxes_xywhn]
    result.boxes = MockBoxes(mock_xywhn, clss, confs)

    return result


class TestRealAutotrack:
    """Tests for _real_autotrack — real ultralytics world path monkeypatched (ADR-0014 D3)."""

    def _make_real_config(
        self,
        tmp_path: Path,
        *,
        weights_path: str = "/outputs/job/weights/world.pt",
        model: str = "world",
        conf: float = 0.5,
        dataset_path: str | None = None,
    ) -> tuple[dict, Path]:
        """Create a config dict for real autotrack (with weights_path)."""
        if dataset_path is None:
            ds = _make_dataset(tmp_path)
            dataset_path = str(ds)

        cfg = {
            "job_id": "test-real-autotrack",
            "engine": "autotracker",
            "model": model,
            "mode": "autotrack",
            "dataset_path": dataset_path,
            "output_path": str(tmp_path / "output"),
            "seed": 42,
            "autotrack": {"model": model, "conf": conf},
            "weights_path": weights_path,
        }
        return cfg, Path(dataset_path)

    def test_real_autotrack_calls_yolo_with_weights(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        """YOLO called with weights_path, set_classes with dataset classes, predict with correct args."""
        ds = _make_dataset(tmp_path)
        # Create images/ directory with actual image files (YOLO package structure)
        images_dir = ds / "images"
        images_dir.mkdir(exist_ok=True)
        (images_dir / "img_0001.jpg").write_bytes(b"\x89PNG\r\n")
        (images_dir / "img_0002.jpg").write_bytes(b"\x89PNG\r\n")

        # Fake results: 1 box for img_0001, empty for img_0002
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

        cfg, _ = self._make_real_config(tmp_path)
        output = tmp_path / "output"
        output.mkdir(exist_ok=True)

        from trainer_yolo.autotrack import _real_autotrack
        _real_autotrack(cfg, output)

        # YOLO called with weights_path
        mock_yolo_cls.assert_called_once_with(str(tmp_path / "weights.pt") if "weights.pt" in cfg["weights_path"] else cfg["weights_path"])

        # set_classes called with dataset class names
        mock_model.set_classes.assert_called_once_with(["solda_fria", "solda_quente"])

        # predict called with images/ subdirectory (YOLO package structure)
        mock_model.predict.assert_called_once_with(
            source=str(ds / "images"), conf=0.5, imgsz=640, device=0,
        )

    def test_real_autotrack_boxes_json_shape_seed_zero(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        """boxes.json has same shape as mock, with seed: 0 (sentinel)."""
        ds = _make_dataset(tmp_path)

        fake_result = _make_fake_result(
            str(ds / "images" / "img_0001.jpg"),
            boxes_xywhn=[[0.5, 0.5, 0.3, 0.4]],
            clss=[0],
            confs=[0.92],
        )
        fake_result2 = _make_fake_result(
            str(ds / "images" / "img_0002.jpg"),
            boxes_xywhn=[[0.3, 0.3, 0.2, 0.2]],
            clss=[1],
            confs=[0.85],
        )
        fake_result3 = MagicMock()
        fake_result3.path = str(ds / "images" / "img_0003.jpg")
        fake_result3.boxes = MagicMock()
        fake_result3.boxes.xywhn = []
        fake_result3.boxes.cls = []
        fake_result3.boxes.conf = []
        fake_result3.boxes.__len__ = lambda self: 0

        mock_yolo_cls = MagicMock()
        mock_model = MagicMock()
        mock_model.predict.return_value = [fake_result, fake_result2, fake_result3]
        mock_model.names = {0: "solda_fria", 1: "solda_quente"}
        mock_yolo_cls.return_value = mock_model

        mock_ultralytics = types.ModuleType("ultralytics")
        mock_ultralytics.YOLO = mock_yolo_cls
        monkeypatch.setitem(sys.modules, "ultralytics", mock_ultralytics)

        cfg, _ = self._make_real_config(tmp_path, conf=0.65)
        output = tmp_path / "output"
        output.mkdir(exist_ok=True)

        from trainer_yolo.autotrack import _real_autotrack
        _real_autotrack(cfg, output)

        boxes_path = output / "boxes.json"
        assert boxes_path.is_file()

        data = json.loads(boxes_path.read_text())
        # Same shape as mock
        assert set(data.keys()) == {"engine", "model", "seed", "conf", "images"}
        assert data["engine"] == "autotracker"
        assert data["model"] == "world"
        assert data["seed"] == 0  # sentinel for real (non-deterministic)
        assert data["conf"] == 0.65
        assert len(data["images"]) == 3

        # Per-image structure
        for img in data["images"]:
            assert "filename" in img
            assert "boxes" in img
            assert isinstance(img["boxes"], list)
            for box in img["boxes"]:
                assert set(box.keys()) == {"class", "x", "y", "w", "h", "conf"}
                assert 0.0 <= box["x"] <= 1.0
                assert 0.0 <= box["y"] <= 1.0
                assert 0.0 <= box["w"] <= 1.0
                assert 0.0 <= box["h"] <= 1.0
                assert 0.0 <= box["conf"] <= 1.0

        # First image has 1 box, second has 1, third is empty
        assert len(data["images"][0]["boxes"]) == 1
        assert data["images"][0]["boxes"][0]["class"] == "solda_fria"
        assert len(data["images"][1]["boxes"]) == 1
        assert data["images"][1]["boxes"][0]["class"] == "solda_quente"
        assert data["images"][2]["boxes"] == []

    def test_real_autotrack_no_metrics_jsonl(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        """Real autotrack does NOT produce metrics.jsonl (binary progress — ADR-0014 D3)."""
        ds = _make_dataset(tmp_path)

        fake_result = _make_fake_result(
            str(ds / "images" / "img_0001.jpg"),
            boxes_xywhn=[[0.5, 0.5, 0.3, 0.4]],
            clss=[0],
            confs=[0.92],
        )

        mock_yolo_cls = MagicMock()
        mock_model = MagicMock()
        mock_model.predict.return_value = [fake_result]
        mock_model.names = {0: "solda_fria", 1: "solda_quente"}
        mock_yolo_cls.return_value = mock_model

        mock_ultralytics = types.ModuleType("ultralytics")
        mock_ultralytics.YOLO = mock_yolo_cls
        monkeypatch.setitem(sys.modules, "ultralytics", mock_ultralytics)

        cfg, _ = self._make_real_config(tmp_path)
        output = tmp_path / "output"
        output.mkdir(exist_ok=True)

        from trainer_yolo.autotrack import _real_autotrack
        _real_autotrack(cfg, output)

        assert not (output / "metrics.jsonl").exists()

    def test_real_autotrack_empty_detection(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        """Image with no detection → boxes [] in boxes.json."""
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
        mock_model.names = {0: "solda_fria", 1: "solda_quente"}
        mock_yolo_cls.return_value = mock_model

        mock_ultralytics = types.ModuleType("ultralytics")
        mock_ultralytics.YOLO = mock_yolo_cls
        monkeypatch.setitem(sys.modules, "ultralytics", mock_ultralytics)

        cfg, _ = self._make_real_config(tmp_path)
        output = tmp_path / "output"
        output.mkdir(exist_ok=True)

        from trainer_yolo.autotrack import _real_autotrack
        _real_autotrack(cfg, output)

        data = json.loads((output / "boxes.json").read_text())
        assert len(data["images"]) == 1
        assert data["images"][0]["boxes"] == []

    def test_real_autotrack_fallback_to_flat_dataset(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        """When images/ dir doesn't exist, predict uses dataset_path directly."""
        ds = _make_dataset(tmp_path)
        # Create flat image files (no images/ subdir)
        (ds / "img_0001.jpg").write_bytes(b"\x89PNG\r\n")

        fake_result = _make_fake_result(
            str(ds / "img_0001.jpg"),
            boxes_xywhn=[[0.5, 0.5, 0.3, 0.4]],
            clss=[0],
            confs=[0.92],
        )

        mock_yolo_cls = MagicMock()
        mock_model = MagicMock()
        mock_model.predict.return_value = [fake_result]
        mock_model.names = {0: "solda_fria", 1: "solda_quente"}
        mock_yolo_cls.return_value = mock_model

        mock_ultralytics = types.ModuleType("ultralytics")
        mock_ultralytics.YOLO = mock_yolo_cls
        monkeypatch.setitem(sys.modules, "ultralytics", mock_ultralytics)

        cfg, _ = self._make_real_config(tmp_path)
        output = tmp_path / "output"
        output.mkdir(exist_ok=True)

        from trainer_yolo.autotrack import _real_autotrack
        _real_autotrack(cfg, output)

        # No images/ dir → fallback to dataset_path
        mock_model.predict.assert_called_once_with(
            source=str(ds), conf=0.5, imgsz=640, device=0,
        )

    def test_real_autotrack_weights_path_absent_dies(self, tmp_path: Path) -> None:
        """ENGINE_MOCK=0 without weights_path → die honesto."""
        from trainer_yolo.autotrack import _real_autotrack

        ds = _make_dataset(tmp_path)
        cfg = {
            "job_id": "test-no-weights",
            "engine": "autotracker",
            "model": "world",
            "mode": "autotrack",
            "dataset_path": str(ds),
            "output_path": str(tmp_path / "output"),
            "seed": 42,
            "autotrack": {"model": "world", "conf": 0.5},
            # No weights_path
        }
        with pytest.raises(SystemExit):
            _real_autotrack(cfg, tmp_path / "output")

    def test_real_autotrack_ultralytics_not_installed(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        """ultralytics not importable → _die with install hint."""
        monkeypatch.delitem(sys.modules, "ultralytics", raising=False)

        import builtins
        real_import = builtins.__import__

        def _no_ultralytics(name: str, *args, **kwargs):
            if name == "ultralytics":
                raise ImportError("No module named 'ultralytics'")
            return real_import(name, *args, **kwargs)

        monkeypatch.setattr(builtins, "__import__", _no_ultralytics)

        from trainer_yolo.autotrack import _real_autotrack
        cfg, _ = self._make_real_config(tmp_path)
        with pytest.raises(SystemExit):
            _real_autotrack(cfg, tmp_path / "output")


# ---------------------------------------------------------------------------
# (g) cmd_autotrack routing — ENGINE_MOCK check (ADR-0014 D3)
# ---------------------------------------------------------------------------

class TestCmdAutotrackRouting:
    """Tests for cmd_autotrack ENGINE_MOCK routing (ADR-0014 D3)."""

    def test_engine_mock_1_routes_to_mock(self, tmp_path: Path) -> None:
        """ENGINE_MOCK=1 → _mock_autotrack (regression, no weights loaded)."""
        ds = _make_dataset(tmp_path)
        cfg = {
            "job_id": "test-mock-route",
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
        output = tmp_path / "output"

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

        assert result.returncode == 0, f"CLI failed: {result.stderr}"
        assert (output / "boxes.json").is_file()
        assert (output / "metrics.jsonl").is_file()  # mock produces metrics

    def test_engine_mock_0_without_weights_dies(self, tmp_path: Path) -> None:
        """ENGINE_MOCK=0 without weights_path → die honesto (exit != 0)."""
        ds = _make_dataset(tmp_path)
        cfg = {
            "job_id": "test-real-no-weights",
            "engine": "autotracker",
            "model": "world",
            "mode": "autotrack",
            "dataset_path": str(ds),
            "output_path": str(tmp_path / "output"),
            "seed": 42,
            "autotrack": {"model": "world", "conf": 0.65},
            # No weights_path
        }
        cfg_path = tmp_path / "config.yaml"
        cfg_path.write_text(yaml.dump(cfg))
        output = tmp_path / "output"

        env = os.environ.copy()
        env["ENGINE_MOCK"] = "0"

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

        assert result.returncode != 0, "Should fail without weights_path in real mode"
        assert "weights_path" in result.stderr.lower() or "weights_path" in result.stdout.lower()
