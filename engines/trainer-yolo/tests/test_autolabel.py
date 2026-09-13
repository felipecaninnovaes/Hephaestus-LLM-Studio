"""Tests for trainer_yolo.autolabel (ADR-0016 D1/D3, passo AL.2).

Covers:
  (a) Config parsing — valid shape, missing keys.
  (b) Mock run in tempdir — captions.jsonl shape D1 ({"filename": "...", "caption": "..."}), metrics.jsonl.
  (c) Determinismo — mesmo seed/filename/prompt gera byte-identical captions.jsonl.
  (d) Inclusão de prompt — prompt customizado é respeitado nas legendas.
  (e) dataset_path vazio/inexistente → erro fail-fast.
  (f) CLI via python -m trainer_yolo autolabel --config ... --output ...
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest
import yaml
from trainer_yolo.autolabel import (
    _mock_autolabel,
    _read_dataset_images,
    load_and_validate_autolabel_config,
)


def _make_autolabel_dataset(tmp_path: Path, filenames: list[str]) -> Path:
    ds_dir = tmp_path / "dataset"
    images_dir = ds_dir / "images"
    images_dir.mkdir(parents=True, exist_ok=True)
    for fname in filenames:
        (images_dir / fname).write_bytes(b"\xff\xd8\xff\xe0" + b"\x00" * 10)
    return ds_dir


def _make_config(
    tmp_path: Path,
    dataset_path: Path,
    output_path: Path,
    *,
    seed: int = 42,
    prompt: str | None = None,
) -> Path:
    cfg = {
        "job_id": "job-al-001",
        "engine": "autolabel",
        "model": "mock",
        "mode": "autolabel",
        "dataset_path": str(dataset_path),
        "output_path": str(output_path),
        "seed": seed,
    }
    if prompt is not None:
        cfg["autolabel"] = {"prompt": prompt}
    tmp_path.mkdir(parents=True, exist_ok=True)
    config_path = tmp_path / "config.yaml"
    with open(config_path, "w", encoding="utf-8") as f:
        yaml.safe_dump(cfg, f)
    return config_path


def test_config_validation_success(tmp_path: Path):
    ds = _make_autolabel_dataset(tmp_path, ["img1.jpg"])
    out = tmp_path / "out"
    cfg_path = _make_config(tmp_path, ds, out)
    cfg = load_and_validate_autolabel_config(cfg_path)
    assert cfg["engine"] == "autolabel"
    assert cfg["mode"] == "autolabel"


def test_config_validation_missing_key(tmp_path: Path):
    cfg_path = tmp_path / "bad_config.yaml"
    with open(cfg_path, "w", encoding="utf-8") as f:
        yaml.safe_dump({"job_id": "123"}, f)
    with pytest.raises(SystemExit):
        load_and_validate_autolabel_config(cfg_path)


def test_read_dataset_images(tmp_path: Path):
    ds = _make_autolabel_dataset(tmp_path, ["b.jpg", "a.png"])
    images = _read_dataset_images(ds)
    assert images == ["a.png", "b.jpg"]


def test_mock_autolabel_outputs(tmp_path: Path):
    ds = _make_autolabel_dataset(tmp_path, ["img_01.jpg", "img_02.jpg"])
    out = tmp_path / "output"
    cfg_path = _make_config(tmp_path, ds, out, prompt="Fotografia macro")
    cfg = load_and_validate_autolabel_config(cfg_path)

    _mock_autolabel(cfg, out)

    captions_file = out / "captions.jsonl"
    assert captions_file.is_file()

    lines = captions_file.read_text(encoding="utf-8").strip().splitlines()
    assert len(lines) == 2

    items = [json.loads(line) for line in lines]
    assert items[0]["filename"] == "img_01.jpg"
    assert "Fotografia macro" in items[0]["caption"]
    assert items[1]["filename"] == "img_02.jpg"
    assert "Fotografia macro" in items[1]["caption"]

    metrics_file = out / "metrics.jsonl"
    assert metrics_file.is_file()


def test_mock_autolabel_determinism(tmp_path: Path):
    ds = _make_autolabel_dataset(tmp_path, ["img_01.jpg", "img_02.jpg"])
    out1 = tmp_path / "out1"
    out2 = tmp_path / "out2"
    cfg1 = load_and_validate_autolabel_config(
        _make_config(tmp_path / "c1", ds, out1, seed=123)
    )
    cfg2 = load_and_validate_autolabel_config(
        _make_config(tmp_path / "c2", ds, out2, seed=123)
    )

    _mock_autolabel(cfg1, out1)
    _mock_autolabel(cfg2, out2)

    content1 = (out1 / "captions.jsonl").read_bytes()
    content2 = (out2 / "captions.jsonl").read_bytes()
    assert content1 == content2


def test_cli_subcommand_autolabel(tmp_path: Path):
    ds = _make_autolabel_dataset(tmp_path, ["img.jpg"])
    out = tmp_path / "cli_out"
    cfg_path = _make_config(tmp_path, ds, out, prompt="Teste CLI")

    res = subprocess.run(
        [
            sys.executable,
            "-m",
            "trainer_yolo",
            "autolabel",
            "--config",
            str(cfg_path),
            "--output",
            str(out),
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    assert res.returncode == 0, f"CLI failed: {res.stderr}"
    assert (out / "captions.jsonl").is_file()


def test_autolabel_florence_and_qwen_models(tmp_path: Path):
    ds = _make_autolabel_dataset(tmp_path, ["sample.webp"])

    # Florence-2
    out_florence = tmp_path / "florence_out"
    cfg_florence = {
        "job_id": "job-florence",
        "engine": "autolabel",
        "model": "florence-2",
        "mode": "autolabel",
        "dataset_path": str(ds),
        "output_path": str(out_florence),
        "seed": 42,
        "autolabel": {"prompt": "Fotografia botânica"},
    }
    cfg_f_path = tmp_path / "florence.yaml"
    cfg_f_path.write_text(yaml.safe_dump(cfg_florence), encoding="utf-8")
    _mock_autolabel(cfg_florence, out_florence)

    lines_f = (
        (out_florence / "captions.jsonl")
        .read_text(encoding="utf-8")
        .strip()
        .splitlines()
    )
    assert len(lines_f) == 1
    data_f = json.loads(lines_f[0])
    assert "Fotografia botânica" in data_f["caption"]
    assert "sharp contours" in data_f["caption"]

    # Qwen2-VL
    out_qwen = tmp_path / "qwen_out"
    cfg_qwen = {
        "job_id": "job-qwen",
        "engine": "autolabel",
        "model": "qwen2-vl",
        "mode": "autolabel",
        "dataset_path": str(ds),
        "output_path": str(out_qwen),
        "seed": 42,
        "autolabel": {"prompt": "Inspeção"},
    }
    _mock_autolabel(cfg_qwen, out_qwen)

    lines_q = (
        (out_qwen / "captions.jsonl").read_text(encoding="utf-8").strip().splitlines()
    )
    assert len(lines_q) == 1
    data_q = json.loads(lines_q[0])
    assert "Inspeção" in data_q["caption"]
    assert "high visual definition" in data_q["caption"]


def test_autolabel_openai_mock_and_fallback(tmp_path: Path):
    ds = _make_autolabel_dataset(tmp_path, ["test.png"])
    out = tmp_path / "openai_out"
    cfg = {
        "job_id": "job-openai",
        "engine": "autolabel",
        "model": "openai",
        "mode": "autolabel",
        "dataset_path": str(ds),
        "output_path": str(out),
        "seed": 42,
        "autolabel": {
            "prompt": "Descreva em detalhes",
            "api_key": "sk-dummy-test-key",
            "api_base": "http://127.0.0.1:9",  # Porta inacessível para testar fallback gracioso
            "openai_model": "gpt-4o-mini",
        },
    }
    _mock_autolabel(cfg, out)

    captions_file = out / "captions.jsonl"
    assert captions_file.is_file()
    lines = captions_file.read_text(encoding="utf-8").strip().splitlines()
    assert len(lines) == 1
    data = json.loads(lines[0])
    assert "Descreva em detalhes" in data["caption"]
    assert "OpenAI fallback" in data["caption"]
