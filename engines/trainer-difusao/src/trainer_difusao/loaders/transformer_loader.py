"""
Carregamento modular e cache quantizado de Transformers (DiT e UNet).
"""
from __future__ import annotations

import os
from pathlib import Path
import shutil
from typing import Any, Callable

from trainer_difusao.common_pkg.core import _die
from trainer_difusao.loaders.quant_cache import (
    _is_cache_valid,
    _save_quant_metadata,
    resolve_quant_base_dir,
)


def _check_hf_repo_error(e: Exception, model_id: str) -> None:
    """Verifica erros comuns de acesso a modelos restritos no Hugging Face."""
    msg = str(e).lower()
    if (
        "gated" in msg
        or "401" in str(e)
        or "403" in str(e)
        or "not a valid model identifier" in msg
    ):
        _die(
            f"Falha ao baixar modelo ({model_id}). Este repositório é restrito no Hugging Face.\n"
            f"1. Aceite a licença do modelo em https://huggingface.co/{model_id}\n"
            f"2. Defina a variável HF_TOKEN no env.gpu com o seu token de acesso: https://huggingface.co/settings/tokens\n"
            f"Erro original: {e}"
        )


def load_or_quantize_transformer(
    model_id: str,
    *,
    subfolder: str = "transformer",
    transformer_cls: Any = None,
    target_dtype: Any = None,
    quant_format: str = "none",
    quant_label: str | None = None,
    bnb_config: Any = None,
    torchao_quant_cfg: Any = None,
    quantization_config: Any = None,
    custom_checkpoint_path: str | None = None,
    custom_identity: str | None = None,
    text_encoder_path: str | None = None,
    force_requantize: bool = False,
    quant_base: Path | None = None,
    transformer_cache_dir: Path | None = None,
    hub_cache: Path | str | None = None,
    hf_token: str | None = None,
    is_flux2: bool = False,
    on_cached: Callable[[Path], None] | None = None,
    on_loading: Callable[[bool], None] | None = None,
    on_ready: Callable[[Any], None] | None = None,
) -> Any:
    """
    Carrega Transformer (DiT ou UNet), quantizando e persistindo em cache em disco se aplicável.
    Suporta BitsAndBytes e TorchAO intx, injeção de checkpoint custom (.safetensors).
    """
    is_quantized = quant_format not in ("none", "", None) and quant_format in (
        "2bit",
        "4bit",
        "6bit",
        "8bit",
    )
    label = quant_label or quant_format

    if transformer_cls is None:
        if subfolder == "unet":
            from diffusers import UNet2DConditionModel

            transformer_cls = UNet2DConditionModel
        elif is_flux2:
            from diffusers import Flux2Transformer2DModel

            transformer_cls = Flux2Transformer2DModel
        else:
            try:
                from diffusers import FluxTransformer2DModel

                transformer_cls = FluxTransformer2DModel
            except ImportError:
                _die(
                    f"Classe de transformer não pôde ser resolvida para subfolder='{subfolder}'. "
                    "Forneça transformer_cls explicitamente."
                )

    if is_quantized:
        if quant_base is None:
            quant_base = resolve_quant_base_dir(
                model_id=model_id,
                quant_format=quant_format,
                custom_identity=custom_identity,
                text_encoder_path=text_encoder_path,
            )
        if transformer_cache_dir is None:
            transformer_cache_dir = quant_base / subfolder

        if force_requantize and transformer_cache_dir.exists():
            print(
                f"[INFO] Forçando re-quantização (force_requantize=True): expurgando cache em {transformer_cache_dir}...",
                flush=True,
            )
            shutil.rmtree(transformer_cache_dir, ignore_errors=True)

    transformer_is_cached = (
        is_quantized
        and not force_requantize
        and transformer_cache_dir is not None
        and _is_cache_valid(
            transformer_cache_dir,
            expected_model_id=model_id,
            expected_quant=quant_format,
            expected_custom=custom_identity,
        )
    )

    if transformer_is_cached and transformer_cache_dir is not None:
        if on_cached is not None:
            on_cached(transformer_cache_dir)
        print(
            f"Carregando Transformer quantizado em {label} do cache persistente validado: {transformer_cache_dir}",
            flush=True,
        )
        transformer = transformer_cls.from_pretrained(
            transformer_cache_dir,
            torch_dtype=target_dtype,
        )
    else:
        if transformer_cache_dir and transformer_cache_dir.exists():
            print(
                f"[INFO] Cache do transformer em {transformer_cache_dir} é inválido ou pertence a outro modelo. Refazendo quantização...",
                flush=True,
            )
            shutil.rmtree(transformer_cache_dir, ignore_errors=True)

        if on_loading is not None:
            on_loading(is_quantized)
        else:
            step_msg = (
                f"Baixando e quantizando Transformer em {label} ({model_id})..."
                if is_quantized
                else f"Baixando e carregando Transformer em precisão plena ({model_id})..."
            )
            print(step_msg, flush=True)

        effective_quant_cfg: Any = None
        if is_quantized:
            if torchao_quant_cfg is not None:
                from diffusers import TorchAoConfig as _DiffTorchAoConfig

                effective_quant_cfg = _DiffTorchAoConfig(quant_type=torchao_quant_cfg)
            elif bnb_config is not None:
                effective_quant_cfg = bnb_config
            elif quantization_config is not None:
                effective_quant_cfg = quantization_config

        load_kwargs: dict[str, Any] = {}
        if subfolder:
            load_kwargs["subfolder"] = subfolder
        if effective_quant_cfg is not None:
            load_kwargs["quantization_config"] = effective_quant_cfg
        if target_dtype is not None:
            load_kwargs["torch_dtype"] = target_dtype
        if hub_cache is not None:
            load_kwargs["cache_dir"] = hub_cache
        if hf_token is not None:
            load_kwargs["token"] = hf_token

        try:
            transformer = transformer_cls.from_pretrained(model_id, **load_kwargs)
        except Exception as e:
            _check_hf_repo_error(e, model_id)
            raise

        if custom_checkpoint_path:
            try:
                from safetensors.torch import load_file as _st_load

                custom_state = _st_load(custom_checkpoint_path)
            except Exception as exc:
                _die(
                    f"Falha ao ler checkpoint custom ({custom_checkpoint_path}): {exc}"
                )
            try:
                missing, unexpected = transformer.load_state_dict(
                    custom_state, strict=False
                )
            except Exception as exc:
                _die(
                    f"Falha ao aplicar checkpoint custom ({custom_checkpoint_path}): layout não reconhecido ({exc})"
                )
            if missing or unexpected:
                _die(
                    f"Checkpoint custom ({custom_checkpoint_path}) com layout não reconhecido: "
                    f"{len(missing)} chave(s) ausente(s), {len(unexpected)} inesperada(s)."
                )
            print(
                f"[LOADER] Checkpoint custom aplicado ao transformer: {custom_checkpoint_path}",
                flush=True,
            )

        if is_quantized and transformer_cache_dir and quant_base:
            try:
                transformer_cache_dir.mkdir(parents=True, exist_ok=True)
                transformer.save_pretrained(transformer_cache_dir)
                _save_quant_metadata(
                    quant_base,
                    model_id=model_id,
                    quant_label=label,
                    quant_format=quant_format,
                    target_dtype=target_dtype,
                    is_flux2=is_flux2,
                    custom_checkpoint=custom_identity,
                )
                print(
                    f"Transformer {label} persistido em cache para execuções futuras: {transformer_cache_dir}",
                    flush=True,
                )
            except Exception as e:
                print(
                    f"[WARN] Não foi possível persistir transformer quantizado em disco: {e}",
                    flush=True,
                )

    if on_ready is not None:
        on_ready(transformer)

    return transformer


def load_flux2_custom_transformer(
    custom_cp: str,
    pipe_dtype: Any,
    quantization_config: Any = None,
) -> Any:
    """Carrega transformer custom flux-2 de arquivo único (.safetensors)."""
    try:
        from diffusers import Flux2Transformer2DModel
    except ImportError as exc:
        _die(
            f"Checkpoint flux-2 custom exige diffusers com Flux2Transformer2DModel ({custom_cp}): {exc}"
        )
    try:
        tf_kwargs: dict[str, Any] = {"torch_dtype": pipe_dtype}
        if quantization_config is not None:
            tf_kwargs["quantization_config"] = quantization_config
        transformer = Flux2Transformer2DModel.from_single_file(custom_cp, **tf_kwargs)
    except Exception as exc:
        _die(
            f"Falha ao carregar checkpoint flux-2 custom ({custom_cp}): "
            f"layout de arquivo não reconhecido ({exc})"
        )
    print(f"[LOADER] Transformer flux-2 custom: {custom_cp}", flush=True)
    return transformer


__all__ = [
    "load_or_quantize_transformer",
    "load_flux2_custom_transformer",
]
