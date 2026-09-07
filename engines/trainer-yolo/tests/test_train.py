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
    _make_fake_artifact,
    _seed_bytes,
    _synthetic_metrics,
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
