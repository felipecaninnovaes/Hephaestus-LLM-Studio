"""
Pacote de AutoLabel para geração de legendas via VLM e APIs de visão.
"""
from trainer_yolo.autolabel_pkg.captions import (
    MOCK_DESCRIPTIONS,
    _generate_caption,
    _generate_caption_florence,
    _generate_caption_mock,
    _generate_caption_qwen,
)
from trainer_yolo.autolabel_pkg.vision_api import (
    _call_openai_vision_api,
    _get_mime_type,
    _normalize_api_base,
    _prepare_image_for_vision,
)
from trainer_yolo.autolabel_pkg.pipeline import (
    _autolabel_pipeline,
    cmd_autolabel,
)

__all__ = [
    "MOCK_DESCRIPTIONS",
    "_generate_caption",
    "_generate_caption_florence",
    "_generate_caption_mock",
    "_generate_caption_qwen",
    "_call_openai_vision_api",
    "_get_mime_type",
    "_normalize_api_base",
    "_prepare_image_for_vision",
    "_autolabel_pipeline",
    "cmd_autolabel",
]
