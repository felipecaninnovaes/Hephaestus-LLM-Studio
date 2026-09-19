"""
Submódulos de tensor, RoPE, quantização e amostragem para FLUX.
"""
from trainer_difusao.models.flux_pkg.rope import (
    _pack_latents,
    _patchify_latents_flux2,
    _pack_latents_flux2,
    _prepare_latent_image_ids,
    _prepare_text_ids,
    _prepare_flux2_latent_ids,
    _prepare_flux2_text_ids,
)
from trainer_difusao.models.flux_pkg.quant_cache import (
    _custom_checkpoint_identity,
    _is_cache_valid,
    _save_quant_metadata,
)
from trainer_difusao.models.flux_pkg.encoding import _encode_qwen3_prompt
from trainer_difusao.models.flux_pkg.sample import _generate_sample_flux

__all__ = [
    "_pack_latents",
    "_patchify_latents_flux2",
    "_pack_latents_flux2",
    "_prepare_latent_image_ids",
    "_prepare_text_ids",
    "_prepare_flux2_latent_ids",
    "_prepare_flux2_text_ids",
    "_custom_checkpoint_identity",
    "_is_cache_valid",
    "_save_quant_metadata",
    "_encode_qwen3_prompt",
    "_generate_sample_flux",
]
