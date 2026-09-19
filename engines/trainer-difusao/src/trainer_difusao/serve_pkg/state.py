"""
Estado compartilhado do daemon de inferência e identificação de specs de pipeline.
"""
from __future__ import annotations

import os
import threading
import time
from typing import Any

from engine_kit.mock import is_mock

_START_TIME = time.time()
_PID = os.getpid()
_MOCK = is_mock()

_gen_lock = threading.Lock()
_busy = False
_loaded_spec: dict[str, Any] | None = None
_pipeline_cache: dict[tuple, object] = {}
_server: Any = None


def _spec_key(spec: dict[str, Any] | None) -> tuple | None:
    """Extrai chave de comparação da spec (base, custom, arch, encoder, quant, distilled)."""
    if spec is None:
        return None
    return (
        spec.get("base_model"),
        spec.get("custom_checkpoint_path"),
        spec.get("arch"),
        spec.get("text_encoder_path"),
        spec.get("quantization"),
        spec.get("distilled"),
    )


def _spec_matches(a: dict[str, Any] | None, b: dict[str, Any] | None) -> bool:
    """Compara duas specs por campos relevantes para reload do pipeline."""
    return _spec_key(a) == _spec_key(b)


def _make_spec(params: dict[str, Any]) -> dict[str, Any]:
    """Extrai dict de spec mínima a partir dos params validados."""
    return {
        "base_model": params.get("base_model"),
        "custom_checkpoint_path": params.get("custom_checkpoint_path"),
        "arch": params.get("arch"),
        "text_encoder_path": params.get("text_encoder_path"),
        "quantization": params.get("quantization"),
        "distilled": params.get("distilled", False),
    }
