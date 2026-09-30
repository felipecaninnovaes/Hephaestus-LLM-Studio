"""Pipeline de treino LoRA para Stable Diffusion XL (SDXL 1.0) na GPU (wrapper fino via TrainingLoopRunner)."""

from pathlib import Path
from typing import Any

from trainer_difusao.models.base import BaseModelTrainer
from trainer_difusao.models.loop import TrainingLoopRunner
from trainer_difusao.models.sd_family.adapter import SDXLAdapter
from trainer_difusao.models.sd_pkg import (
    _compute_sdxl_embeddings,
    _generate_sample_sdxl,
)

__all__ = [
    "_compute_sdxl_embeddings",
    "_generate_sample_sdxl",
    "_real_train_sdxl",
    "SDXLTrainer",
]


def _real_train_sdxl(cfg: dict[str, Any], output: Path) -> None:
    """Pipeline real de treino LoRA para Stable Diffusion XL (SDXL 1.0) na GPU."""
    runner = TrainingLoopRunner(SDXLAdapter())
    runner.run(cfg, output)


class SDXLTrainer(BaseModelTrainer):
    """Trainer de difusão para Stable Diffusion XL (SDXL 1.0)."""

    def train(self, cfg: dict[str, Any], output: Path) -> None:
        _real_train_sdxl(cfg, output)
