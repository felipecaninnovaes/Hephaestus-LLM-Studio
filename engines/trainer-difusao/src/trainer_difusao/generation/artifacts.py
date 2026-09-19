"""
Construção de metadados, PNG iTXt e salvamento de thumbnails.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any


def _write_thumb(
    src_path: Path, dst_path: Path, max_side: int = 512, quality: int = 80
) -> None:
    """Salva thumbnail JPEG com max-side preservando proporção (PIL.Image.thumbnail)."""
    from PIL import Image

    with Image.open(src_path) as img:
        img.thumbnail((max_side, max_side), Image.LANCZOS)
        img.convert("RGB").save(dst_path, "JPEG", quality=quality)


def _is_cancelled(output_dir: Path) -> bool:
    """Checa se existe sentinela de cancelamento no output_dir."""
    return (output_dir / "cancel").exists()


HEPHAESTUS_GENERATION_PNG_KEY = "hephaestus.generation"

# Chaves de _build_generation_meta redundantes no PNG (nome do arquivo local).
_PNG_EXCLUDED_META_KEYS = frozenset({"filename", "thumb_filename", "batch_index"})


def _png_payload_for_generation(meta: dict[str, Any]) -> dict[str, Any]:
    """Deriva o payload JSON embarcado no PNG a partir do dict do JSONL."""
    return {k: v for k, v in meta.items() if k not in _PNG_EXCLUDED_META_KEYS}


def _png_info_for_generation(meta: dict[str, Any]):
    """Serializa os campos de geração em chunk iTXt do PNG."""
    from PIL.PngImagePlugin import PngInfo

    payload = _png_payload_for_generation(meta)
    text = json.dumps(payload, ensure_ascii=False, separators=(",", ":"), sort_keys=True)
    info = PngInfo()
    info.add_itxt(
        HEPHAESTUS_GENERATION_PNG_KEY, text, lang="en", tkey=HEPHAESTUS_GENERATION_PNG_KEY
    )
    return info


def _build_generation_meta(
    params: dict[str, Any],
    filename: str,
    thumb_filename: str,
    seed: int,
    batch_index: int,
    batch_size: int,
    loras_effective: list[dict[str, Any]],
    upscale_info: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Constrói o dict de metadados para uma imagem do batch (JSONL)."""
    meta: dict[str, Any] = {
        "filename": filename,
        "thumb_filename": thumb_filename,
        "seed": seed,
        "prompt": params["prompt"],
        "negative_prompt": params["negative_prompt"],
        "width": params["width"],
        "height": params["height"],
        "steps": params["steps"],
        "guidance_scale": params["guidance_scale"],
        "quantization": params["quantization"],
        "distilled": params["distilled"],
        "loras": loras_effective,
        "custom_model_path": params.get("custom_checkpoint_path"),
        "custom_model_id": params.get("custom_model_id"),
        "arch": params.get("arch"),
        "text_encoder_path": params.get("text_encoder_path"),
        "text_encoder_model_id": params.get("text_encoder_model_id"),
        "base_model": params["base_model"],
        "batch_index": batch_index,
        "batch_size": batch_size,
        "job_id": params.get("job_id"),
        "sampler": params.get("sampler", "default"),
    }
    if upscale_info is not None:
        meta["upscale"] = upscale_info
    if params.get("init_image_path"):
        meta["init_image"] = os.path.basename(params["init_image_path"])
        meta["init_strength"] = params.get("init_strength")
    return meta
