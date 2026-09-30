"""Pipeline de treino LoRA para FLUX.1 e FLUX.2 Klein na GPU (wrapper fino via TrainingLoopRunner)."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from trainer_difusao.models.base import BaseModelTrainer
from trainer_difusao.models.loop import TrainingLoopRunner
from trainer_difusao.models.flux_adapter import FluxAdapter
from trainer_difusao.models.flux_pkg import (
    _custom_checkpoint_identity,
    _encode_qwen3_prompt,
    _generate_sample_flux,
    _is_cache_valid,
    _pack_latents,
    _pack_latents_flux2,
    _patchify_latents_flux2,
    _prepare_flux2_latent_ids,
    _prepare_flux2_text_ids,
    _prepare_latent_image_ids,
    _prepare_text_ids,
    _save_quant_metadata,
)

__all__ = [
    "FluxTrainer",
    "_real_train_flux",
    "_custom_checkpoint_identity",
    "_encode_qwen3_prompt",
    "_generate_sample_flux",
    "_is_cache_valid",
    "_pack_latents",
    "_pack_latents_flux2",
    "_patchify_latents_flux2",
    "_prepare_flux2_latent_ids",
    "_prepare_flux2_text_ids",
    "_prepare_latent_image_ids",
    "_prepare_text_ids",
    "_save_quant_metadata",
]


def _real_train_flux(cfg: dict[str, Any], output: Path) -> None:
    """Pipeline real de treino LoRA para FLUX.1 e FLUX.2 Klein na GPU."""
    runner = TrainingLoopRunner(FluxAdapter())
    runner.run(cfg, output)


class FluxTrainer(BaseModelTrainer):
    """Trainer de difusão para FLUX.2 Klein 4B e FLUX.1."""

    def train(self, cfg: dict[str, Any], output: Path) -> None:
        _real_train_flux(cfg, output)
