"""
Carregamento modular, quantização e fusão de Text Encoders (Qwen3-VL, T5-XXL, CLIP).
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


def _apply_loose_encoder_state(
    base_encoder: Any,
    state: dict[str, Any],
    encoder_path: str,
    model_repo: str,
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
    hub_cache: Path | str | None = None,
    hf_token: str | None = None,
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
                model_repo, subfolder="tokenizer", cache_dir=hub_cache, token=hf_token
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
            model_repo,
            subfolder="text_encoder",
            torch_dtype=pipe_dtype,
            cache_dir=hub_cache,
            token=hf_token,
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
            tmp_dir,
            md5=md5,
            basename=Path(encoder_path).name,
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
            model_repo, subfolder="tokenizer", cache_dir=hub_cache, token=hf_token
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


def load_or_quantize_text_encoder(
    model_id: str,
    *,
    encoder_type: str = "qwen3",
    subfolder: str | None = None,
    encoder_cls: Any = None,
    target_dtype: Any = None,
    quant_format: str = "none",
    quant_label: str | None = None,
    bnb_config: Any = None,
    torchao_quant_cfg: Any = None,
    quantization_config: Any = None,
    text_encoder_path: str | None = None,
    custom_identity: str | None = None,
    force_requantize: bool = False,
    quant_base: Path | None = None,
    text_encoder_cache_dir: Path | None = None,
    hub_cache: Path | str | None = None,
    hf_token: str | None = None,
    is_flux2: bool = False,
    device: str | None = None,
    return_tokenizer: bool = False,
    tokenizer_subfolder: str = "tokenizer",
    on_cached: Callable[[Path], None] | None = None,
    on_loading: Callable[[bool], None] | None = None,
    on_ready: Callable[[Any], None] | None = None,
) -> Any:
    """
    Carrega Text Encoder (Qwen3, Qwen3-VL, T5-XXL, CLIP), quantizando e salvando em cache se aplicável.
    Suporta overrides de pesos custom (.safetensors ou diretório HF) com merge em disco.
    """
    is_quantized = quant_format not in ("none", "", None) and quant_format in (
        "2bit",
        "4bit",
        "6bit",
        "8bit",
    )
    label = quant_label or quant_format

    enc_type_lower = encoder_type.lower()
    default_subfolder = "text_encoder"
    if encoder_cls is None:
        if enc_type_lower in ("qwen3-vl", "qwen-vl"):
            from transformers import Qwen3VLForConditionalGeneration

            encoder_cls = Qwen3VLForConditionalGeneration
            default_subfolder = "text_encoder"
        elif enc_type_lower in ("t5", "t5-xxl", "t5xxl"):
            from transformers import T5EncoderModel

            encoder_cls = T5EncoderModel
            default_subfolder = "text_encoder_2"
        elif enc_type_lower in ("clip", "clip-l"):
            from transformers import CLIPTextModel

            encoder_cls = CLIPTextModel
            default_subfolder = "text_encoder"
        else:
            from transformers import AutoModelForCausalLM

            encoder_cls = AutoModelForCausalLM
            default_subfolder = "text_encoder"

    effective_subfolder = subfolder if subfolder is not None else default_subfolder

    if is_quantized:
        if quant_base is None:
            quant_base = resolve_quant_base_dir(
                model_id=model_id,
                quant_format=quant_format,
                custom_identity=custom_identity,
                text_encoder_path=text_encoder_path,
            )
        if text_encoder_cache_dir is None:
            text_encoder_cache_dir = quant_base / effective_subfolder

        if force_requantize and text_encoder_cache_dir.exists():
            print(
                f"[INFO] Forçando re-quantização (force_requantize=True): expurgando cache em {text_encoder_cache_dir}...",
                flush=True,
            )
            shutil.rmtree(text_encoder_cache_dir, ignore_errors=True)

    text_enc_is_cached = (
        is_quantized
        and not force_requantize
        and text_encoder_cache_dir is not None
        and _is_cache_valid(
            text_encoder_cache_dir,
            expected_model_id=model_id,
            expected_quant=quant_format,
            expected_custom=custom_identity,
        )
    )

    if text_enc_is_cached and text_encoder_cache_dir is not None:
        if on_cached is not None:
            on_cached(text_encoder_cache_dir)
        print(
            f"Carregando Text Encoder ({encoder_type}) quantizado em {label} do cache persistente validado: {text_encoder_cache_dir}",
            flush=True,
        )
        text_encoder = encoder_cls.from_pretrained(
            text_encoder_cache_dir,
            torch_dtype=target_dtype,
        )
    else:
        if text_encoder_cache_dir and text_encoder_cache_dir.exists():
            print(
                f"[INFO] Cache do text encoder em {text_encoder_cache_dir} é inválido ou pertence a outro modelo. Refazendo quantização...",
                flush=True,
            )
            shutil.rmtree(text_encoder_cache_dir, ignore_errors=True)

        if on_loading is not None:
            on_loading(is_quantized)
        else:
            step_msg = (
                f"Baixando e quantizando Text Encoder ({encoder_type}) em {label} ({model_id})..."
                if is_quantized
                else f"Baixando e carregando Text Encoder ({encoder_type}) em precisão plena ({model_id})..."
            )
            print(step_msg, flush=True)

        effective_quant_cfg: Any = None
        if is_quantized:
            if torchao_quant_cfg is not None:
                from transformers import TorchAoConfig as _HfTorchAoConfig

                effective_quant_cfg = _HfTorchAoConfig(quant_type=torchao_quant_cfg)
            elif bnb_config is not None:
                effective_quant_cfg = bnb_config
            elif quantization_config is not None:
                effective_quant_cfg = quantization_config

        if text_encoder_path:
            enc_p = Path(text_encoder_path)
            if enc_p.is_dir():
                try:
                    enc_kwargs: dict[str, Any] = {"torch_dtype": target_dtype}
                    if effective_quant_cfg is not None:
                        enc_kwargs["quantization_config"] = effective_quant_cfg
                    text_encoder = encoder_cls.from_pretrained(str(enc_p), **enc_kwargs)
                except Exception as exc:
                    _die(
                        f"Falha ao carregar text_encoder custom de dir ({text_encoder_path}): {exc}"
                    )
                print(
                    f"[LOADER] Text encoder custom (dir): {text_encoder_path}",
                    flush=True,
                )
            elif enc_p.is_file() and effective_quant_cfg is not None:
                text_encoder, _ = _load_flux2_loose_encoder_merged(
                    text_encoder_path,
                    model_id,
                    target_dtype,
                    effective_quant_cfg,
                    hub_cache=hub_cache,
                    hf_token=hf_token,
                )
            elif enc_p.is_file():
                try:
                    base_encoder = encoder_cls.from_pretrained(
                        model_id,
                        subfolder=effective_subfolder,
                        torch_dtype=target_dtype,
                        cache_dir=hub_cache,
                        token=hf_token,
                    )
                except Exception as exc:
                    _die(
                        f"Falha ao carregar text encoder base do repo ({model_id}) "
                        f"para aplicar override ({text_encoder_path}): {exc}"
                    )
                from trainer_difusao.common import _load_loose_text_encoder_state

                enc_state = _load_loose_text_encoder_state(text_encoder_path)
                _apply_loose_encoder_state(base_encoder, enc_state, text_encoder_path, model_id)
                text_encoder = base_encoder
                print(
                    f"[LOADER] Text encoder custom (.safetensors sobre repo): {text_encoder_path}",
                    flush=True,
                )
            else:
                _die(
                    f"text_encoder_path não encontrado: {text_encoder_path}. "
                    "Use um diretório HF ou arquivo .safetensors válido."
                )
        else:
            load_kwargs: dict[str, Any] = {}
            if effective_subfolder:
                load_kwargs["subfolder"] = effective_subfolder
            if effective_quant_cfg is not None:
                load_kwargs["quantization_config"] = effective_quant_cfg
            if target_dtype is not None:
                load_kwargs["torch_dtype"] = target_dtype
            if hub_cache is not None:
                load_kwargs["cache_dir"] = hub_cache
            if hf_token is not None:
                load_kwargs["token"] = hf_token

            text_encoder = encoder_cls.from_pretrained(model_id, **load_kwargs)

        if is_quantized and text_encoder_cache_dir and quant_base:
            try:
                text_encoder_cache_dir.mkdir(parents=True, exist_ok=True)
                text_encoder.save_pretrained(text_encoder_cache_dir)
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
                    f"Text Encoder {label} persistido em cache para execuções futuras: {text_encoder_cache_dir}",
                    flush=True,
                )
            except Exception as e:
                print(
                    f"[WARN] Não foi possível persistir text encoder quantizado em disco: {e}",
                    flush=True,
                )

    if device is not None and not is_quantized:
        text_encoder = text_encoder.to(device)

    if on_ready is not None:
        on_ready(text_encoder)

    if return_tokenizer:
        from transformers import AutoTokenizer

        if text_encoder_path and Path(text_encoder_path).is_dir():
            tok = AutoTokenizer.from_pretrained(
                str(text_encoder_path), cache_dir=hub_cache, token=hf_token
            )
        else:
            tok = AutoTokenizer.from_pretrained(
                model_id,
                subfolder=tokenizer_subfolder,
                cache_dir=hub_cache,
                token=hf_token,
            )
        return text_encoder, tok

    return text_encoder


__all__ = [
    "_apply_loose_encoder_state",
    "_load_flux2_loose_encoder_merged",
    "load_or_quantize_text_encoder",
]
