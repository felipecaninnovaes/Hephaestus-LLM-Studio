"""
Gerenciamento de cache e identificação de pipelines de difusão residentes em VRAM.
"""
from __future__ import annotations

from typing import Any


def pipeline_cache_key(params: dict[str, Any]) -> tuple:
    """Extrai chave de cache do pipeline a partir dos params validados.

    Chave: (base_model|custom_checkpoint_path+arch, quantization, distilled,
    text_encoder_path).
    """
    custom_cp = params.get("custom_checkpoint_path")
    return (
        custom_cp or params.get("base_model"),
        params.get("quantization"),
        params.get("distilled", False),
        params.get("text_encoder_path"),
    )


def ensure_pipeline(
    params: dict[str, Any], cache: dict[tuple, object]
) -> tuple[object | None, tuple]:
    """Verifica cache de pipeline e devolve (pipeline|None, key).

    Se cache hit → (pipeline_obj, key).
    Se cache miss → (None, key) — caller deve chamar _real_generate sem pipeline.
    """
    key = pipeline_cache_key(params)
    if key in cache:
        print(f"[DIFFUSION-GEN] Cache hit para spec {key}.", flush=True)
        return cache[key], key
    return None, key
