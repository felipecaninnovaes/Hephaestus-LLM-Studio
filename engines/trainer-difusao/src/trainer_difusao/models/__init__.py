"""Fábrica de instâncias de trainers por modelo de difusão."""

from __future__ import annotations

from trainer_difusao.common import _canonical_model_name, _die
from trainer_difusao.models.base import BaseModelTrainer
from trainer_difusao.models.flux import FluxTrainer
from trainer_difusao.models.mock import MockTrainer
from trainer_difusao.models.sd15 import SD15Trainer
from trainer_difusao.models.sdxl import SDXLTrainer


def get_trainer(model_name: str, is_mock: bool = False) -> BaseModelTrainer:
    """Retorna o trainer apropriado para o modelo solicitado ou MockTrainer."""
    if is_mock:
        return MockTrainer()

    canonical = _canonical_model_name(model_name)
    if "flux" in canonical:
        return FluxTrainer()
    if canonical == "sdxl":
        return SDXLTrainer()
    if canonical == "sd15":
        return SD15Trainer()

    _die(f"Modelo base de difusão desconhecido ou não suportado: '{model_name}'")
    raise ValueError(f"Modelo não suportado: {model_name}")


__all__ = [
    "BaseModelTrainer",
    "FluxTrainer",
    "MockTrainer",
    "SD15Trainer",
    "SDXLTrainer",
    "get_trainer",
]
