"""Modo autolabel mock determinístico (ENGINE_MOCK=1) do trainer-yolo.

ADR-0016 D1: gera captions determinísticas para cada imagem do dataset,
com base no seed, filename e prompt opcional.

Entrypoint: python -m trainer_yolo autolabel --config <config.yaml> --output <dir>

Saída:
  - captions.jsonl (formato D1: {"filename": "...", "caption": "..."})
  - metrics.jsonl (opcional / tolerado — 1 linha)
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import sys

import yaml

from trainer_yolo.train import _die


MOCK_DESCRIPTIONS = [
    "Uma foto em close de uma placa de circuito impresso com solda fria e componentes SMD.",
    "Placa de circuito verde com conectores banhados a ouro e trilhas metálicas visíveis.",
    "Vista superior de placa de circuito eletrônico com capacitores e resistores alinhados.",
    "Foto detalhada de circuito impresso destacando pontos de solda e microcontrolador central.",
    "Componentes eletrônicos montados em superfície com solda de precisão sob luz de bancada.",
    "Macro de conexões eletrônicas em substrato cerâmico com filamentos condutores expostos.",
    "Detalhe de placa controladora com barramentos de cobre e conectores de alta densidade.",
    "Placa de circuito industrial exibindo pontos de teste e marcações serigráficas nítidas.",
]

IMAGE_EXTENSIONS = {".jpg", ".jpeg", ".png", ".webp", ".bmp"}


def load_and_validate_autolabel_config(config_path: str | Path) -> dict:
    """Carrega config.yaml e valida campos necessários para autolabel."""
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

    return cfg


def _read_dataset_images(dataset_path: Path) -> list[str]:
    """Descobre os arquivos de imagem dentro do pacote do dataset."""
    filenames: set[str] = set()

    # 1. Checa diretório images/
    images_dir = dataset_path / "images"
    if images_dir.is_dir():
        for p in images_dir.iterdir():
            if p.is_file() and p.suffix.lower() in IMAGE_EXTENSIONS:
                filenames.add(p.name)

    # 2. Checa dataset.yaml se existir
    dataset_yaml = dataset_path / "dataset.yaml"
    if dataset_yaml.is_file():
        with open(dataset_yaml, "r", encoding="utf-8") as f:
            ds = yaml.safe_load(f)
        if isinstance(ds, dict):
            for split_key in ("train", "val"):
                paths = ds.get(split_key)
                if isinstance(paths, list):
                    for p in paths:
                        fname = str(p).split("/")[-1]
                        if fname:
                            filenames.add(fname)

    # 3. Também checa a raiz do dataset_path caso as imagens estejam soltas
    for p in dataset_path.iterdir():
        if p.is_file() and p.suffix.lower() in IMAGE_EXTENSIONS:
            filenames.add(p.name)

    if not filenames:
        _die(f"No images found in dataset: {dataset_path}")

    return sorted(filenames)


def _generate_caption(seed: int, filename: str, prompt: str | None) -> str:
    """Gera legenda determinística a partir de (seed, filename, prompt)."""
    h_input = f"{seed}:{filename}"
    h = int(hashlib.sha256(h_input.encode("utf-8")).hexdigest(), 16)
    base = MOCK_DESCRIPTIONS[h % len(MOCK_DESCRIPTIONS)]

    if prompt and prompt.strip():
        return f"{prompt.strip()} — {base}"
    return base


def _mock_autolabel(cfg: dict, output_dir: Path) -> None:
    """Executa o pipeline de autolabel determinístico mock."""
    output_dir.mkdir(parents=True, exist_ok=True)
    dataset_path = Path(cfg["dataset_path"])
    seed = int(cfg.get("seed", 42))

    al_section = cfg.get("autolabel", {})
    prompt = None
    if isinstance(al_section, dict):
        prompt = al_section.get("prompt")

    image_filenames = _read_dataset_images(dataset_path)

    # Gera captions.jsonl (ADR-0016 D1)
    captions_path = output_dir / "captions.jsonl"
    with open(captions_path, "w", encoding="utf-8") as f:
        for fname in image_filenames:
            caption = _generate_caption(seed, fname, prompt)
            line = json.dumps({"filename": fname, "caption": caption}, ensure_ascii=False)
            f.write(line + "\n")

    # Gera metrics.jsonl (1 linha para compatibilidade com orquestrador)
    metrics_path = output_dir / "metrics.jsonl"
    with open(metrics_path, "w", encoding="utf-8") as f:
        metrics = {
            "epoch": 1,
            "loss": 0.0,
            "images": len(image_filenames),
        }
        f.write(json.dumps(metrics) + "\n")


def cmd_autolabel(args: list[str]) -> None:
    """Subcomando autolabel."""
    parser = argparse.ArgumentParser(
        prog="trainer-yolo autolabel",
        description="AutoLabel — mock determinístico (ENGINE_MOCK=1) de legendas em lote",
    )
    parser.add_argument("--config", required=True, help="Path to config.yaml")
    parser.add_argument("--output", required=True, help="Output directory")

    opts = parser.parse_args(args)
    cfg = load_and_validate_autolabel_config(opts.config)
    output = Path(opts.output)

    _mock_autolabel(cfg, output)
