"""Tests for trainer_yolo.autotrack (ADR-0008 D2, passo A.1).

Covers:
  (a) Config parsing — valid shape, missing keys, missing autotrack section, conf out of range.
  (b) Mock run in tempdir — boxes.json shape D1, metrics.jsonl 1 line 6 keys, domains 0..1.
  (c) Determinism — same seed/filenames/classes → byte-identical boxes.json.
  (d) dataset_path nonexistent / empty → error.
  (e) CLI via python -m trainer_yolo autotrack --config ... --output ...
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

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
