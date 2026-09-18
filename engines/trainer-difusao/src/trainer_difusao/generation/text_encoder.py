"""
Carregamento de text-encoders e transformers custom para FLUX.2 Klein.
"""
from __future__ import annotations

import os
from pathlib import Path
from typing import Any

from trainer_difusao.common_pkg.core import _die


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


def _apply_loose_encoder_state(
    base_encoder: Any, state: dict[str, Any], encoder_path: str, model_repo: str
) -> None:
    """Aplica o state_dict solto sobre o encoder do repo (bf16, sem quant)."""
    try:
        missing, unexpected = base_encoder.load_state_dict(state, strict=False)
    except Exception as exc:
        _die(
            f"Falha ao aplicar text_encoder custom ({encoder_path}) sobre o "
            f"encoder do repo ({model_repo}): layout não reconhecido ({exc})"
        )
    ignored_keys = {"lm_head.weight", "model.lm_head.weight"}
    missing_filtered = [k for k in (missing or []) if k not in ignored_keys]
    unexpected_filtered = [k for k in (unexpected or []) if k not in ignored_keys]
    if missing_filtered or unexpected_filtered:
        _die(
            f"text_encoder custom ({encoder_path}) com layout não reconhecido: "
            f"{len(missing_filtered)} chave(s) ausente(s) {missing_filtered[:5]}, "
            f"{len(unexpected_filtered)} inesperada(s) {unexpected_filtered[:5]}. "
            "Envie o encoder como diretório HF completo ou um .safetensors "
            "compatível com o Qwen3 do FLUX.2 Klein."
        )
    if hasattr(base_encoder, "tie_weights"):
        try:
            base_encoder.tie_weights()
        except Exception:
            pass


def _load_flux2_loose_encoder_merged(
    encoder_path: str,
    model_repo: str,
    pipe_dtype: Any,
    quantization_config: Any,
) -> tuple[Any, Any]:
    """Arquivo solto + quantização via cache de merge (bf16 mesclado em disco)."""
    from transformers import AutoModelForCausalLM, AutoTokenizer

    from trainer_difusao.common import (
        _cleanup_merge_tmp_dir,
        _custom_text_encoder_merge_dir,
        _load_loose_text_encoder_state,
        _merged_text_encoder_tmp_dir,
        _merged_text_encoder_valid,
        _publish_merged_text_encoder,
        _sweep_text_encoder_merge_cache,
        _write_merged_text_encoder_metadata,
    )

    merged_dir, md5 = _custom_text_encoder_merge_dir(encoder_path)
    if _merged_text_encoder_valid(merged_dir, md5):
        try:
            encoder = AutoModelForCausalLM.from_pretrained(
                str(merged_dir),
                torch_dtype=pipe_dtype,
                quantization_config=quantization_config,
            )
        except Exception as exc:
            _die(
                f"Falha ao carregar text_encoder custom do cache de merge "
                f"({merged_dir}): {exc}"
            )
        try:
            tokenizer = AutoTokenizer.from_pretrained(
                model_repo, subfolder="tokenizer"
            )
        except Exception as exc:
            _die(
                f"Falha ao carregar tokenizer base do repo ({model_repo}): {exc}"
            )
        print(
            f"[DIFFUSION-GEN] Text encoder custom (merge em cache: "
            f"{merged_dir}): {encoder_path}",
            flush=True,
        )
        return encoder, tokenizer
    try:
        base_encoder = AutoModelForCausalLM.from_pretrained(
            model_repo, subfolder="text_encoder", torch_dtype=pipe_dtype
        )
    except Exception as exc:
        _die(
            f"Falha ao carregar text encoder base do repo ({model_repo}) "
            f"para aplicar override ({encoder_path}): {exc}"
        )
    state = _load_loose_text_encoder_state(encoder_path)
    _apply_loose_encoder_state(base_encoder, state, encoder_path, model_repo)
    parent = merged_dir.parent
    tmp_dir = _merged_text_encoder_tmp_dir(merged_dir)
    try:
        parent.mkdir(parents=True, exist_ok=True)
        _cleanup_merge_tmp_dir(tmp_dir)
        base_encoder.save_pretrained(str(tmp_dir))
        _write_merged_text_encoder_metadata(
            tmp_dir, md5=md5, basename=Path(encoder_path).name,
            model_id=model_repo,
        )
        _publish_merged_text_encoder(tmp_dir, merged_dir)
    except SystemExit:
        raise
    except Exception as exc:
        if _merged_text_encoder_valid(merged_dir, md5):
            _cleanup_merge_tmp_dir(tmp_dir)
            print(
                f"[DIFFUSION-GEN] Text encoder custom (merge concorrente "
                f"detectado em {merged_dir}): {encoder_path}",
                flush=True,
            )
        else:
            _cleanup_merge_tmp_dir(tmp_dir)
            _die(
                f"Falha ao persistir cache de merge do text_encoder custom "
                f"({merged_dir}): {exc}"
            )
    try:
        encoder = AutoModelForCausalLM.from_pretrained(
            str(merged_dir),
            torch_dtype=pipe_dtype,
            quantization_config=quantization_config,
        )
    except Exception as exc:
        _die(
            f"Falha ao carregar text_encoder custom do cache de merge "
            f"({merged_dir}): {exc}"
        )
    try:
        tokenizer = AutoTokenizer.from_pretrained(
            model_repo, subfolder="tokenizer"
        )
    except Exception as exc:
        _die(
            f"Falha ao carregar tokenizer base do repo ({model_repo}): {exc}"
        )
    print(
        f"[DIFFUSION-GEN] Text encoder custom (merge novo executado: "
        f"{merged_dir}): {encoder_path}",
        flush=True,
    )
    _sweep_text_encoder_merge_cache(merged_dir)
    return encoder, tokenizer


def _load_flux2_custom_transformer(
    custom_cp: str, pipe_dtype: Any, quantization_config: Any = None
) -> Any:
    """Carrega transformer custom flux-2 do arquivo (falha honesta se layout inválido)."""
    try:
        from diffusers import Flux2Transformer2DModel
    except ImportError as exc:
        _die(
            f"Checkpoint flux-2 custom exige diffusers com Flux2Transformer2DModel "
            f"({custom_cp}): {exc}"
        )
    try:
        tf_kwargs: dict[str, Any] = {"torch_dtype": pipe_dtype}
        if quantization_config is not None:
            tf_kwargs["quantization_config"] = quantization_config
        transformer = Flux2Transformer2DModel.from_single_file(
            custom_cp, **tf_kwargs
        )
    except Exception as exc:
        _die(
            f"Falha ao carregar checkpoint flux-2 custom ({custom_cp}): "
            f"layout de arquivo não reconhecido ({exc})"
        )
    print(
        f"[DIFFUSION-GEN] Transformer flux-2 custom: {custom_cp}",
        flush=True,
    )
    return transformer
