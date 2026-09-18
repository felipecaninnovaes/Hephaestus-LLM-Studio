"""
Facade para AutoLabel v2 (re-exporta de autolabel_pkg, dataset e config).
Preserva 100% da superfície de importação para testes e comandos.
"""
from __future__ import annotations

import sys
from trainer_yolo.autolabel_pkg import (
    MOCK_DESCRIPTIONS,
    _call_openai_vision_api,
    _generate_caption,
    _generate_caption_florence,
    _generate_caption_mock,
    _generate_caption_qwen,
    _get_mime_type,
    _normalize_api_base,
    _prepare_image_for_vision,
    _autolabel_pipeline,
    cmd_autolabel,
)
from trainer_yolo.config import load_and_validate_autolabel_config
from trainer_yolo.dataset import IMAGE_EXTENSIONS, _discover_dataset_images, _read_dataset_images

_mock_autolabel = _autolabel_pipeline

__all__ = [
    "MOCK_DESCRIPTIONS",
    "IMAGE_EXTENSIONS",
    "load_and_validate_autolabel_config",
    "_discover_dataset_images",
    "_read_dataset_images",
    "_mock_autolabel",
    "_generate_caption",
    "_generate_caption_mock",
    "_generate_caption_florence",
    "_generate_caption_qwen",
    "_get_mime_type",
    "_normalize_api_base",
    "_prepare_image_for_vision",
    "_call_openai_vision_api",
    "_autolabel_pipeline",
    "cmd_autolabel",
]

if __name__ == "__main__":
    cmd_autolabel(sys.argv[1:])
