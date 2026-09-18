"""
Adapter isolando a integração e Lazy Import da biblioteca Ultralytics.
"""
from __future__ import annotations

import sys
from typing import Any

from engine_kit.runtime import die


def get_yolo_class() -> Any:
    """Carrega dinamicamente a classe YOLO com mensagem clara caso ausente."""
    try:
        from ultralytics import YOLO
        return YOLO
    except ImportError as exc:
        die(f"ultralytics package required for real YOLO operation (ENGINE_MOCK=0): {exc}")
