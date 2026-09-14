"""Interface base para treinadores de modelos de difusão."""

from __future__ import annotations

from abc import ABC, abstractmethod
from pathlib import Path
from typing import Any


class BaseModelTrainer(ABC):
    """Protocolo base para execução de treino dos modelos de difusão."""

    @abstractmethod
    def train(self, cfg: dict[str, Any], output: Path) -> None:
        """Executa o loop de treino LoRA e geração de artefatos."""
        pass
