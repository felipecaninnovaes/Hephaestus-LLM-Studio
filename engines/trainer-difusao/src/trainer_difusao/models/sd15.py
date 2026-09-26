"""Pipeline de treino LoRA para Stable Diffusion 1.5 na GPU (wrapper fino via TrainingLoopRunner)."""

from pathlib import Path
from typing import Any

from trainer_difusao.models.base import BaseModelTrainer
from trainer_difusao.models.loop import TrainingLoopRunner
from trainer_difusao.models.sd_family.adapter import SD15Adapter
from trainer_difusao.models.sd_pkg.sample import _generate_sample_sd15

__all__ = [
    "_generate_sample_sd15",
    "_real_train_sd15",
    "SD15Trainer",
]


def _real_train_sd15(cfg: dict[str, Any], output: Path) -> None:
    """Pipeline real de treino LoRA para Stable Diffusion 1.5 na GPU."""
    runner = TrainingLoopRunner(SD15Adapter())
    runner.run(cfg, output)


class SD15Trainer(BaseModelTrainer):
    """Trainer de difusão para Stable Diffusion 1.5."""

    def train(self, cfg: dict[str, Any], output: Path) -> None:
        _real_train_sd15(cfg, output)
