"""
Carregamento de text-encoders e transformers custom para FLUX.2 Klein.
"""
from __future__ import annotations

import os
from pathlib import Path
from typing import Any

from trainer_difusao.common_pkg.core import _die
from trainer_difusao.loaders.text_encoder_loader import (
    _apply_loose_encoder_state,
    _load_flux2_loose_encoder_merged,
)
from trainer_difusao.loaders.transformer_loader import (
    load_flux2_custom_transformer as _load_flux2_custom_transformer,
)


def _flux2_repo_id(*, distilled: bool) -> str:
    """Repo BFL do FLUX.2 Klein (overrides por env, defaults oficiais)."""
    if distilled:
        return (
            os.environ.get("FLUX_DISTILLED_MODEL_ID")
            or "black-forest-labs/FLUX.2-klein-4B"
        )
    return (
        os.environ.get("FLUX_MODEL_ID") or "black-forest-labs/FLUX.2-klein-base-4B"
    )


def _text_encoder_merge_dir(encoder_path: str) -> tuple[Path, str]:
    """Resolve (merged_dir, md5_16) do cache de merge p/ um encoder solto."""
    from trainer_difusao.common import _custom_text_encoder_merge_dir

    return _custom_text_encoder_merge_dir(encoder_path)


def _load_flux2_text_encoder_override(
    encoder_path: str,
    model_repo: str,
    pipe_dtype: Any,
    quantization_config: Any = None,
) -> tuple[Any, Any]:
    """Carrega encoder/tokenizer override p/ FLUX.2 Klein (Qwen3)."""
    from transformers import AutoModelForCausalLM, AutoTokenizer

    enc = Path(encoder_path)
    if enc.is_dir():
        try:
            enc_kwargs: dict[str, Any] = {"torch_dtype": pipe_dtype}
            if quantization_config is not None:
                enc_kwargs["quantization_config"] = quantization_config
            encoder = AutoModelForCausalLM.from_pretrained(
                str(enc), **enc_kwargs
            )
        except Exception as exc:
            _die(
                f"Falha ao carregar text_encoder custom de dir ({encoder_path}): {exc}"
            )
        try:
            tokenizer = AutoTokenizer.from_pretrained(str(enc))
        except Exception as exc:
            _die(
                f"Falha ao carregar tokenizer do text_encoder custom "
                f"({encoder_path}): {exc}"
            )
        print(
            f"[DIFFUSION-GEN] Text encoder custom (dir): {encoder_path}",
            flush=True,
        )
        return encoder, tokenizer
    if enc.is_file():
        if quantization_config is not None:
            return _load_flux2_loose_encoder_merged(
                encoder_path, model_repo, pipe_dtype, quantization_config
            )
        try:
            base_encoder = AutoModelForCausalLM.from_pretrained(
                model_repo, subfolder="text_encoder", torch_dtype=pipe_dtype
            )
        except Exception as exc:
            _die(
                f"Falha ao carregar text encoder base do repo ({model_repo}) "
                f"para aplicar override ({encoder_path}): {exc}"
            )
        from trainer_difusao.common import _load_loose_text_encoder_state

        state = _load_loose_text_encoder_state(encoder_path)
        _apply_loose_encoder_state(base_encoder, state, encoder_path, model_repo)
        try:
            tokenizer = AutoTokenizer.from_pretrained(
                model_repo, subfolder="tokenizer"
            )
        except Exception as exc:
            _die(
                f"Falha ao carregar tokenizer base do repo ({model_repo}): {exc}"
            )
        print(
            f"[DIFFUSION-GEN] Text encoder custom (.safetensors sobre repo): "
            f"{encoder_path}",
            flush=True,
        )
        return base_encoder, tokenizer
    _die(
        f"text_encoder_path não encontrado: {encoder_path}. "
        "Use um diretório HF ou arquivo .safetensors válido."
    )


__all__ = [
    "_flux2_repo_id",
    "_text_encoder_merge_dir",
    "_load_flux2_text_encoder_override",
    "_apply_loose_encoder_state",
    "_load_flux2_loose_encoder_merged",
    "_load_flux2_custom_transformer",
]
