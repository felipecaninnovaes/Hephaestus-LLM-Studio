"""
Utilitários de manipulação de dataset, resolução de imagens e preparação de dataset.yaml.
"""
from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

import yaml
from engine_kit.runtime import die as _die

IMAGE_EXTENSIONS = frozenset({".jpg", ".jpeg", ".png", ".webp", ".bmp"})


def _read_dataset(dataset_path: Path) -> tuple[list[str], list[str]]:
    """Read dataset.yaml and return (class_names, image_filenames)."""
    dataset_yaml = dataset_path / "dataset.yaml"
    if not dataset_yaml.is_file():
        _die(f"dataset.yaml not found in {dataset_path}")

    with open(dataset_yaml, "r", encoding="utf-8") as f:
        data = yaml.safe_load(f)

    if not isinstance(data, dict):
        _die(f"dataset.yaml is not a valid YAML mapping: {dataset_yaml}")

    names_entry = data.get("names")
    if names_entry is None:
        _die("dataset.yaml missing 'names' field")

    if isinstance(names_entry, list):
        class_names = [str(n) for n in names_entry]
    elif isinstance(names_entry, dict):
        try:
            sorted_keys = sorted(names_entry.keys(), key=int)
        except (ValueError, TypeError):
            sorted_keys = sorted(names_entry.keys())
        class_names = [str(names_entry[k]) for k in sorted_keys]
    else:
        _die(f"'names' field in dataset.yaml must be list or dict, got {type(names_entry).__name__}")

    if not class_names:
        _die("dataset.yaml has empty 'names' field")

    image_filenames: list[str] = []
    for split in ("train", "val"):
        entry = data.get(split)
        if entry is None:
            continue
        if isinstance(entry, list):
            for item in entry:
                image_filenames.append(Path(str(item)).name)
        elif isinstance(entry, str):
            split_path = Path(entry)
            if not split_path.is_absolute():
                split_path = dataset_path / split_path
            if split_path.is_dir():
                for f in split_path.iterdir():
                    if f.is_file() and f.suffix.lower() in IMAGE_EXTENSIONS:
                        image_filenames.append(f.name)
            elif split_path.is_file():
                try:
                    for line in split_path.read_text(encoding="utf-8").splitlines():
                        line = line.strip()
                        if line:
                            image_filenames.append(Path(line).name)
                except Exception:
                    pass

    if not image_filenames:
        images_dir = dataset_path / "images"
        if images_dir.is_dir():
            image_filenames = [
                f.name for f in images_dir.iterdir()
                if f.is_file() and f.suffix.lower() in IMAGE_EXTENSIONS
            ]

    if not image_filenames:
        _die("dataset.yaml has no images (empty train and val lists)")

    image_filenames = sorted(set(image_filenames))
    return class_names, image_filenames


def _prepare_dataset_yaml(dataset_yaml_path: Path, dataset_dir: Path) -> None:
    """Prepare dataset.yaml for ultralytics compatibility."""
    with open(dataset_yaml_path, "r", encoding="utf-8") as f:
        data = yaml.safe_load(f) or {}

    current = data.get("path")
    if current is not None and not Path(current).is_absolute():
        data["path"] = str(dataset_dir)

    for key in ("train", "val"):
        entries = data.get(key)
        if isinstance(entries, list) and entries:
            txt_path = dataset_dir / f"{key}.txt"
            lines = "\n".join(str(dataset_dir / entry) for entry in entries)
            txt_path.write_text(lines, encoding="utf-8")
            data[key] = f"{key}.txt"

    if not data.get("val"):
        data["val"] = "train.txt"

    with open(dataset_yaml_path, "w", encoding="utf-8") as f:
        yaml.safe_dump(data, f, sort_keys=False)

def _discover_dataset_images(dataset_path: Path) -> dict[str, Path]:
    """Descobre os arquivos de imagem dentro do pacote do dataset mapeando {filename: Path}."""
    if not dataset_path.is_dir():
        _die(f"Dataset path is not a directory: {dataset_path}")

    images: dict[str, Path] = {}

    images_dir = dataset_path / "images"
    if images_dir.is_dir():
        for p in images_dir.iterdir():
            if p.is_file() and p.suffix.lower() in IMAGE_EXTENSIONS:
                images[p.name] = p

    dataset_yaml = dataset_path / "dataset.yaml"
    if dataset_yaml.is_file():
        with open(dataset_yaml, "r", encoding="utf-8") as f:
            ds = yaml.safe_load(f)
        if isinstance(ds, dict):
            for split_key in ("train", "val"):
                paths = ds.get(split_key)
                if isinstance(paths, list):
                    for p_str in paths:
                        p = Path(p_str)
                        if not p.is_absolute():
                            p = dataset_path / p
                        if p.is_file() and p.suffix.lower() in IMAGE_EXTENSIONS:
                            images[p.name] = p

    for p in dataset_path.iterdir():
        if p.is_file() and p.suffix.lower() in IMAGE_EXTENSIONS and p.name not in images:
            images[p.name] = p

    if not images:
        _die(f"No image files found in dataset path: {dataset_path}")

    return images


def _read_dataset_images(dataset_path: Path) -> list[str]:
    """Alias retrocompatível que devolve lista ordenada de filenames de imagens."""
    return sorted(_discover_dataset_images(dataset_path).keys())
