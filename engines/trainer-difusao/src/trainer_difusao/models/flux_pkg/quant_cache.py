"""
Validação e integridade de cache de quantização persistida para FLUX.
Re-exporta de trainer_difusao.loaders.quant_cache para compatibilidade retroativa.
"""
from __future__ import annotations

from trainer_difusao.loaders.quant_cache import (
    _custom_checkpoint_identity,
    _is_cache_valid,
    _save_quant_metadata,
    get_quant_cache_root,
    resolve_quant_base_dir,
)

__all__ = [
    "_custom_checkpoint_identity",
    "_is_cache_valid",
    "_save_quant_metadata",
    "get_quant_cache_root",
    "resolve_quant_base_dir",
]
