"""
Geração determinística de embeddings mock para o trainer-clip.
"""
from __future__ import annotations

from engine_kit.mock import mock_vector as _engine_mock_vector


def mock_vector(payload: bytes, dim: int = 512) -> list[float]:
    """Gera vetor determinístico normalizado L2 para embeddings mock."""
    return _engine_mock_vector(payload, dim=dim)
