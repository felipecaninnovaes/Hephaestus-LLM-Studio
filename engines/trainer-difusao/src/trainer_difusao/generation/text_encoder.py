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

_QUANT_LEVELS_ACTIVE = ("2bit", "4bit", "6bit", "8bit")


def _flux2_quant_configs(quant: str, compute_dtype: Any) -> tuple[Any, Any]:
    """(bnb_config, torchao_cfg) da quantização, mesmo mapeamento do treino (flux_adapter)."""
    if quant == "4bit":
        from transformers import BitsAndBytesConfig

        return (
            BitsAndBytesConfig(
                load_in_4bit=True,
                bnb_4bit_quant_type="nf4",
                bnb_4bit_compute_dtype=compute_dtype,
                bnb_4bit_use_double_quant=True,
            ),
            None,
        )
    if quant == "8bit":
        from transformers import BitsAndBytesConfig

        return BitsAndBytesConfig(load_in_8bit=True), None
    if quant in ("2bit", "6bit"):
        from trainer_difusao.common_pkg.train_config import _build_intx_torchao_config

        return None, _build_intx_torchao_config(quant)
    _die(f"Quantização inválida para Klein: {quant}.")


def _assert_quantized(model: Any, quant: str, name: str) -> int:
    """Falha alto se o componente não saiu quantizado; retorna nº de módulos bnb (log de prova)."""
    quantized = (
        getattr(model, "hf_quantizer", None) is not None
        or bool(getattr(model, "is_quantized", False))
        or hasattr(model, "quantization_config")
    )
    if not quantized:
        _die(
            f"Quantização {quant} solicitada, mas o {name} foi carregado sem "
            "quantização (no-op). Abortando em vez de gerar em bf16."
        )
    if quant not in ("4bit", "8bit"):
        return 0
    wanted = "Linear4bit" if quant == "4bit" else "Linear8bitLt"
    try:
        return sum(1 for m in model.modules() if type(m).__name__ == wanted)
    except Exception:  # noqa: BLE001 — só log; a checagem dura é a de cima
        return 0


def load_flux2_quantized_components(
    model_repo: str,
    quant: str,
    pipe_dtype: Any,
    *,
    custom_cp: str | None = None,
    text_encoder_path: str | None = None,
    hub_cache: Any = None,
    hf_token: str | None = None,
) -> dict[str, Any]:
    """Transformer + Qwen3 text encoder quantizados p/ o Klein (kwargs do pipeline).

    Reusa os loaders do treino e o MESMO cache quantizado persistido
    (`resolve_quant_base_dir(model_repo, quant, custom_identity, text_encoder_path)`):
    hit → carrega do disco sem re-quantizar; miss → quantiza e persiste.
    Custom checkpoint sem cache de treino válido cai em `from_single_file`
    quantizado (sem persistir). Qualquer componente que não saia quantizado
    aborta (nunca bf16 silencioso).
    """
    if quant not in _QUANT_LEVELS_ACTIVE:
        _die(f"load_flux2_quantized_components exige quantização ativa (recebido: {quant}).")
    from transformers import AutoModelForCausalLM

    from trainer_difusao.loaders.quant_cache import (
        _custom_checkpoint_identity,
        _is_cache_valid,
        resolve_quant_base_dir,
    )
    from trainer_difusao.loaders.text_encoder_loader import load_or_quantize_text_encoder
    from trainer_difusao.loaders.transformer_loader import load_or_quantize_transformer

    bnb_config, torchao_cfg = _flux2_quant_configs(quant, pipe_dtype)
    custom_identity = _custom_checkpoint_identity(custom_cp)
    quant_base = resolve_quant_base_dir(
        model_id=model_repo,
        quant_format=quant,
        custom_identity=custom_identity,
        text_encoder_path=text_encoder_path,
    )
    print(
        f"[DIFFUSION-GEN] Klein quantização {quant}: cache quantizado em {quant_base}",
        flush=True,
    )

    from diffusers import Flux2Transformer2DModel

    transformer_dir = quant_base / "transformer"
    custom_uncached = bool(custom_cp) and not _is_cache_valid(
        transformer_dir,
        expected_model_id=model_repo,
        expected_quant=quant,
        expected_custom=custom_identity,
    )
    if custom_uncached:
        if torchao_cfg is not None:
            from diffusers import TorchAoConfig as _DiffTorchAoConfig

            single_cfg: Any = _DiffTorchAoConfig(quant_type=torchao_cfg)
        else:
            single_cfg = bnb_config
        transformer = _load_flux2_custom_transformer(
            custom_cp, pipe_dtype, quantization_config=single_cfg
        )
    else:
        transformer = load_or_quantize_transformer(
            model_id=model_repo,
            transformer_cls=Flux2Transformer2DModel,
            subfolder="transformer",
            target_dtype=pipe_dtype,
            quant_format=quant,
            quant_label=quant,
            bnb_config=bnb_config,
            torchao_quant_cfg=torchao_cfg,
            custom_checkpoint_path=custom_cp,
            custom_identity=custom_identity,
            text_encoder_path=text_encoder_path,
            quant_base=quant_base,
            transformer_cache_dir=transformer_dir,
            hub_cache=hub_cache,
            hf_token=hf_token,
            is_flux2=True,
        )
    n_tf = _assert_quantized(transformer, quant, "transformer")

    text_encoder, tokenizer = load_or_quantize_text_encoder(
        model_id=model_repo,
        encoder_type="qwen3",
        subfolder="text_encoder",
        encoder_cls=AutoModelForCausalLM,
        target_dtype=pipe_dtype,
        quant_format=quant,
        quant_label=quant,
        bnb_config=bnb_config,
        torchao_quant_cfg=torchao_cfg,
        text_encoder_path=text_encoder_path,
        custom_identity=custom_identity,
        quant_base=quant_base,
        text_encoder_cache_dir=quant_base / "text_encoder",
        hub_cache=hub_cache,
        hf_token=hf_token,
        is_flux2=True,
        return_tokenizer=True,
    )
    n_te = _assert_quantized(text_encoder, quant, "text encoder")
    print(
        f"[DIFFUSION-GEN] Klein quantizado ({quant}) OK: "
        f"transformer={type(transformer).__name__} (módulos quant={n_tf}), "
        f"text_encoder={type(text_encoder).__name__} (módulos quant={n_te}).",
        flush=True,
    )
    return {
        "transformer": transformer,
        "text_encoder": text_encoder,
        "tokenizer": tokenizer,
    }


__all__ = [
    "_flux2_repo_id",
    "_text_encoder_merge_dir",
    "_load_flux2_text_encoder_override",
    "_apply_loose_encoder_state",
    "_load_flux2_loose_encoder_merged",
    "_load_flux2_custom_transformer",
    "load_flux2_quantized_components",
]
