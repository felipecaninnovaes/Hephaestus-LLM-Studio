"""Qwen3-VL-8B conditioning stack for Qwen-Image 2.1.

Simplified version without toolkit dependency.
The Comfy-Org repack keeps the transformers key layout except that the language
tower sits directly under `model.` instead of `model.language_model.`, so the
single-file load only needs that prefix rewrite.
"""

import torch
import torch.nn as nn
from transformers import AutoModel, AutoProcessor


# comfy prefix -> transformers prefix for the language tower
_COMFY_PREFIXES = (
    ("model.layers.", "model.language_model.layers."),
    ("model.embed_tokens.", "model.language_model.embed_tokens."),
    ("model.norm.", "model.language_model.norm."),
)


class QwenImage21TextEncoder(nn.Module):
    """Qwen3-VL text encoder wrapper for Qwen-Image 2.1 prompt/reference encoding."""

    def __init__(self, model):
        super().__init__()
        self.model = model

    def forward(self, *args, **kwargs):
        return self.model(*args, **kwargs)

    @staticmethod
    def convert_state_dict_on_load(state_dict):
        """Convert ComfyUI state dict layout to transformers layout if needed."""
        if not any(k.startswith("model.layers.") for k in state_dict):
            return state_dict
        converted = {}
        for key, value in state_dict.items():
            for comfy_prefix, hf_prefix in _COMFY_PREFIXES:
                if key.startswith(comfy_prefix):
                    key = hf_prefix + key[len(comfy_prefix) :]
                    break
            converted[key] = value
        return converted

    @staticmethod
    def convert_state_dict_on_save(state_dict):
        """Convert transformers layout back to ComfyUI layout."""
        converted = {}
        for key, value in state_dict.items():
            for comfy_prefix, hf_prefix in _COMFY_PREFIXES:
                if key.startswith(hf_prefix):
                    key = comfy_prefix + key[len(hf_prefix) :]
                    break
            converted[key] = value
        return converted

    @staticmethod
    def load_processor(model_path: str):
        """Load the processor from model path."""
        return AutoProcessor.from_pretrained(model_path, subfolder="processor", trust_remote_code=True)

    @classmethod
    def load_model(cls, model_path: str, dtype=None, subfolder=None, **kwargs):
        """Load the model from path and wrap it, forwarding all kwargs to from_pretrained."""
        # Load the underlying Qwen3-VL model
        model = AutoModel.from_pretrained(
            model_path,
            subfolder=subfolder,
            trust_remote_code=True,
            torch_dtype=dtype if dtype else torch.bfloat16,
            **kwargs,
        )
        return cls(model)

    def patch_vision_patch_embed(self):
        """Patch vision patch embedding for bf16 efficiency if needed."""
        pass  # simplified - can add implementation if needed

    def __getattr__(self, name):
        """Forward unknown attributes to wrapped model."""
        if name.startswith('_'):
            return super().__getattribute__(name)
        try:
            return super().__getattribute__(name)
        except AttributeError:
            return getattr(self.model, name)
