"""
Loaders modulares para modelos de difusão, quantização e caching persistente.
"""
from __future__ import annotations

from trainer_difusao.loaders.quant_cache import (
    _custom_checkpoint_identity,
    _is_cache_valid,
    _save_quant_metadata,
    get_quant_cache_root,
    resolve_quant_base_dir,
)
from trainer_difusao.loaders.text_encoder_loader import (
    _apply_loose_encoder_state,
    _load_flux2_loose_encoder_merged,
    load_or_quantize_text_encoder,
)
from trainer_difusao.loaders.transformer_loader import (
    load_flux2_custom_transformer,
    load_or_quantize_transformer,
)

__all__ = [
    "_custom_checkpoint_identity",
    "_is_cache_valid",
    "_save_quant_metadata",
    "get_quant_cache_root",
    "resolve_quant_base_dir",
    "load_or_quantize_transformer",
    "load_flux2_custom_transformer",
    "_apply_loose_encoder_state",
    "_load_flux2_loose_encoder_merged",
    "load_or_quantize_text_encoder",
]
