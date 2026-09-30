"""Adapter for FLUX.1 and FLUX.2-Klein training using unified TrainingLoopRunner."""

from __future__ import annotations

import os
import math
import shutil
from pathlib import Path
from typing import Any, Callable

from trainer_difusao.common import (
    ENABLE_TEXT_ENCODER_UNLOAD,
    TextEmbedsCache,
    _build_intx_torchao_config,
    _die,
    _emit_metric,
    _load_lora_weights,
    _offload_encoders_to_cpu,
    _precompute_sample_embeds_flux,
    _precompute_text_cache,
    _precompute_text_cache_with_cleanup,
    _validate_train_aux,
)
from trainer_difusao.models.loop import LoraTrainConfig, ModelComponents, parse_lora_train_config
from trainer_difusao.models.flux_pkg import (
    _custom_checkpoint_identity,
    _encode_qwen3_prompt,
    _generate_sample_flux,
    _is_cache_valid,
    _pack_latents,
    _pack_latents_flux2,
    _patchify_latents_flux2,
    _prepare_flux2_latent_ids,
    _prepare_flux2_text_ids,
    _prepare_latent_image_ids,
    _prepare_text_ids,
    _save_quant_metadata,
)
from trainer_difusao.loaders import (
    load_or_quantize_text_encoder,
    load_or_quantize_transformer,
    resolve_quant_base_dir,
)

__all__ = ["FluxAdapter"]


class FluxAdapter:
    """Adapter for FLUX.1 and FLUX.2-Klein training via TrainingLoopRunner."""

    arch_label = "FLUX"
    metadata_base_model = "flux"  # will be "flux-2-klein-4b" or "flux-1" in checkpoint_metadata

    def parse_lora_train_config(self, cfg: dict[str, Any]) -> LoraTrainConfig:
        """Parse training config for Flux with Flux-specific validation and quantization handling.
        
        Key differences from generic parse_lora_train_config:
        - Flux supports custom checkpoints and text encoder paths (both conditioned on is_flux2)
        - Flux supports 4/8/2/6-bit quantization with BitsAndBytes and TorchAO
        - hf_token handling from cfg/environment
        - Stores all Flux-specific state in extra={...} dict
        """
        # Resolve hf_token first (needed for hub_cache setup)
        hf_token = (
            cfg.get("hf_token")
            or os.environ.get("HF_TOKEN")
            or os.environ.get("HUGGING_FACE_HUB_TOKEN")
            or None
        )
        if hf_token:
            hf_token = hf_token.strip()

        # Resolve model_id and is_flux2 to make conditional validations work
        model_id = (
            cfg.get("model_id")
            or os.environ.get("FLUX_MODEL_ID")
            or "black-forest-labs/FLUX.2-klein-base-4B"
        )
        is_flux2 = any(k in model_id.lower() for k in ["klein", "flux.2", "flux-2"])

        # Validate custom_checkpoint_path (conditioned on raw_train_arch for clarity of error messages)
        raw_train_arch = str(cfg.get("model", "") or "").strip().lower()
        raw_custom_cp = cfg.get("custom_checkpoint_path")
        custom_checkpoint_path: str | None = None
        if raw_custom_cp:
            if not isinstance(raw_custom_cp, str) or not raw_custom_cp.strip():
                _die("custom_checkpoint_path deve ser uma string não vazia.")
            custom_checkpoint_path = raw_custom_cp.strip()
            if raw_train_arch not in ("flux-2-klein-4b", "flux", "flux2", "flux-2"):
                _die(
                    "treino custom não suportado para este arch "
                    f"({raw_train_arch or 'indefinido'}). "
                    "Checkpoints custom de treino usam arch 'flux-2-klein-4b' "
                    "(sdxl/sd15 custom seguem pelos loaders de sdxl.py/sd15.py)."
                )

        # Validate text_encoder_path (only allowed for flux2)
        raw_encoder_path = cfg.get("text_encoder_path")
        text_encoder_path: str | None = None
        if raw_encoder_path:
            if not isinstance(raw_encoder_path, str) or not raw_encoder_path.strip():
                _die("text_encoder_path deve ser uma string não vazia.")
            text_encoder_path = raw_encoder_path.strip()
            if not is_flux2:
                _die(
                    "text_encoder_path só é suportado com arch flux-2-klein-4b "
                    f"(modelo atual: {model_id})."
                )

        # Parse common config via generic helper, allowing custom_checkpoint and text_encoder_path
        tcfg = parse_lora_train_config(
            cfg,
            default_model_id=model_id,
            default_resolution=512,
            arch_name="flux",
            quant_default="4bit",  # Flux defaults to 4bit quantization
            allow_custom_checkpoint=True,
            allow_text_encoder_path=True,  # Flux supports custom text encoder
        )

        # Handle quantization configuration (verbatim from flux.py)
        aux = _validate_train_aux(cfg, quant_default=None)
        raw_quant = (
            cfg.get("lora", {}).get("quantization")
            or cfg.get("quantization")
            or os.environ.get("FLUX_QUANTIZATION")
            or aux["quantization"]
            or "4bit"
        )
        quantization = str(raw_quant).strip().lower()
        # Normaliza aliases legados e valida o enum canônico (none/2bit/4bit/6bit/8bit).
        aux_check = _validate_train_aux(
            {**cfg, "quantization": quantization},
            quant_default=None,
        )
        quantization = aux_check["quantization"] or "none"

        is_4bit = quantization == "4bit"
        is_8bit = quantization == "8bit"
        is_2bit = quantization == "2bit"
        is_6bit = quantization == "6bit"
        is_quantized = is_4bit or is_8bit or is_2bit or is_6bit

        # Build quantization configuration
        import torch
        from transformers import BitsAndBytesConfig

        target_dtype = (
            torch.bfloat16
            if torch.cuda.is_bf16_supported()
            else torch.float16
        )

        if is_4bit:
            quant_label = "4-bit NF4"
            quant_format = "4bit"
            bnb_config = BitsAndBytesConfig(
                load_in_4bit=True,
                bnb_4bit_quant_type="nf4",
                bnb_4bit_compute_dtype=target_dtype,
                bnb_4bit_use_double_quant=True,
            )
            torchao_quant_cfg = None
        elif is_8bit:
            quant_label = "8-bit BitsAndBytes"
            quant_format = "8bit"
            bnb_config = BitsAndBytesConfig(
                load_in_8bit=True,
            )
            torchao_quant_cfg = None
        elif is_2bit or is_6bit:
            quant_label = f"{'2' if is_2bit else '6'}-bit TorchAO intx weight-only"
            quant_format = "2bit" if is_2bit else "6bit"
            bnb_config = None
            torchao_quant_cfg = _build_intx_torchao_config(quant_format)
        else:
            quant_label = "Nenhum (FP16/BF16 pleno)"
            quant_format = "full"
            bnb_config = None
            torchao_quant_cfg = None

        force_requantize = bool(
            cfg.get("force_requantize", False)
            or os.environ.get("FLUX_FORCE_REQUANTIZE", "0").lower() in ("1", "true", "yes")
        )

        # Cache quantizado isolado por (model_id, quant, custom_checkpoint, encoder).
        custom_identity = _custom_checkpoint_identity(custom_checkpoint_path)
        if is_quantized:
            quant_base = resolve_quant_base_dir(
                model_id=model_id,
                quant_format=quant_format,
                custom_identity=custom_identity,
                text_encoder_path=text_encoder_path,
            )
            if force_requantize and quant_base.exists():
                print(
                    f"[INFO] Forçando re-quantização (force_requantize=True): expurgando cache existente em {quant_base}...",
                    flush=True,
                )
                shutil.rmtree(quant_base, ignore_errors=True)

            transformer_cache_dir = quant_base / "transformer"
            text_encoder_cache_dir = quant_base / ("text_encoder" if is_flux2 else "text_encoder_2")
            quant_base.mkdir(parents=True, exist_ok=True)
        else:
            quant_base = None
            transformer_cache_dir = None
            text_encoder_cache_dir = None

        # Store all Flux-specific state in extra dict (not generalizable to other archs)
        import dataclasses
        tcfg_with_extra = dataclasses.replace(
            tcfg,
            extra={
                "hf_token": hf_token,
                "is_flux2": is_flux2,
                "model_id": model_id,
                "quant_label": quant_label,
                "quant_format": quant_format,
                "bnb_config": bnb_config,
                "torchao_quant_cfg": torchao_quant_cfg,
                "quant_base": quant_base,
                "transformer_cache_dir": transformer_cache_dir,
                "text_encoder_cache_dir": text_encoder_cache_dir,
                "custom_identity": custom_identity,
                "force_requantize": force_requantize,
                "is_quantized": is_quantized,
                "target_dtype": target_dtype,
                # 'quantization' resolvido localmente (considera FLUX_QUANTIZATION env,
                # ausente do parse_lora_train_config genérico) — checkpoint_metadata DEVE
                # ler daqui, não de tcfg.quantization (que ignora esse fallback).
                "quantization": quantization,
            },
        )

        return tcfg_with_extra

    def load_and_inject_lora(self, tcfg: LoraTrainConfig, hub_cache: str, metrics_path: Path) -> ModelComponents:
        """Load Flux base model, inject LoRA, and return components.
        
        Preserves all quantization caching, loading callbacks (on_cached/on_loading/on_ready),
        and model setup logic verbatim from flux.py.
        """
        import torch
        from diffusers import (
            AutoencoderKL,
            FlowMatchEulerDiscreteScheduler,
            FluxTransformer2DModel,
        )
        from peft import LoraConfig, get_peft_model
        from transformers import (
            AutoModelForCausalLM,
            AutoTokenizer,
            CLIPTextModel,
            T5EncoderModel,
        )

        device = torch.device("cuda")

        # Extract Flux-specific state from extra dict
        hf_token = tcfg.extra["hf_token"]
        is_flux2 = tcfg.extra["is_flux2"]
        model_id = tcfg.extra["model_id"]
        quant_label = tcfg.extra["quant_label"]
        quant_format = tcfg.extra["quant_format"]
        bnb_config = tcfg.extra["bnb_config"]
        torchao_quant_cfg = tcfg.extra["torchao_quant_cfg"]
        quant_base = tcfg.extra["quant_base"]
        transformer_cache_dir = tcfg.extra["transformer_cache_dir"]
        text_encoder_cache_dir = tcfg.extra["text_encoder_cache_dir"]
        custom_identity = tcfg.extra["custom_identity"]
        force_requantize = tcfg.extra["force_requantize"]
        target_dtype = tcfg.extra["target_dtype"]

        _emit_metric(
            metrics_path,
            epoch=0,
            step=1,
            progress=0.01,
            phase="init",
            message=f"Inicializando motor FLUX: {model_id} ({quant_label})...",
        )

        # Classes condicionais para FLUX.2 / Klein se disponíveis no Diffusers instalado
        Flux2Transformer_cls = FluxTransformer2DModel
        AutoencoderKL_cls = AutoencoderKL
        if is_flux2:
            try:
                from diffusers import Flux2Transformer2DModel

                Flux2Transformer_cls = Flux2Transformer2DModel
            except ImportError:
                pass
            try:
                from diffusers import AutoencoderKLFlux2

                AutoencoderKL_cls = AutoencoderKLFlux2
            except ImportError:
                pass

        print(
            f"Carregando modelos base FLUX ({model_id}) [is_flux2={is_flux2}, quantização: {quant_label}, res: {tcfg.resolution}, dtype: {target_dtype}]...",
            flush=True,
        )

        # 1. Carregamento do Transformer (DiT): do cache quantizado se já existir e for válido, senão quantiza e salva
        transformer = load_or_quantize_transformer(
            model_id=model_id,
            transformer_cls=Flux2Transformer_cls,
            subfolder="transformer",
            target_dtype=target_dtype,
            quant_format=quant_format,
            quant_label=quant_label,
            bnb_config=bnb_config,
            torchao_quant_cfg=torchao_quant_cfg,
            custom_checkpoint_path=tcfg.custom_checkpoint_path,
            custom_identity=custom_identity,
            text_encoder_path=tcfg.text_encoder_path,
            force_requantize=force_requantize,
            quant_base=quant_base,
            transformer_cache_dir=transformer_cache_dir,
            hub_cache=hub_cache,
            hf_token=hf_token,
            is_flux2=is_flux2,
            on_cached=lambda p: _emit_metric(
                metrics_path,
                epoch=0,
                step=2,
                progress=0.03,
                phase="load_transformer",
                message=f"Carregando Transformer quantizado em {quant_label} do cache persistente...",
            ),
            on_loading=lambda is_quant: _emit_metric(
                metrics_path,
                epoch=0,
                step=2,
                progress=0.02,
                phase="quantizing_transformer" if is_quant else "load_transformer",
                message=(
                    f"Baixando e quantizando Transformer FLUX em {quant_label} ({model_id})..."
                    if is_quant
                    else f"Baixando e carregando Transformer FLUX em precisão plena ({model_id})..."
                ),
            ),
            on_ready=lambda _: _emit_metric(
                metrics_path,
                epoch=0,
                step=3,
                progress=0.04,
                phase="transformer_ready",
                message=f"Transformer FLUX ({quant_label}) carregado com sucesso.",
            ),
        )

        # 2. Carregamento do(s) Text Encoder(s)
        if is_flux2:
            tokenizer_two = None
            text_encoder_two = None

            text_encoder_one = load_or_quantize_text_encoder(
                model_id=model_id,
                encoder_type="qwen3",
                subfolder="text_encoder",
                encoder_cls=AutoModelForCausalLM,
                target_dtype=target_dtype,
                quant_format=quant_format,
                quant_label=quant_label,
                bnb_config=bnb_config,
                torchao_quant_cfg=torchao_quant_cfg,
                text_encoder_path=tcfg.text_encoder_path,
                custom_identity=custom_identity,
                force_requantize=force_requantize,
                quant_base=quant_base,
                text_encoder_cache_dir=text_encoder_cache_dir,
                hub_cache=hub_cache,
                hf_token=hf_token,
                is_flux2=True,
                on_cached=lambda p: _emit_metric(
                    metrics_path,
                    epoch=0,
                    step=4,
                    progress=0.05,
                    phase="load_text_encoder",
                    message=f"Carregando Text Encoder Qwen3 quantizado em {quant_label} do cache persistente...",
                ),
                on_loading=lambda is_quant: _emit_metric(
                    metrics_path,
                    epoch=0,
                    step=4,
                    progress=0.04,
                    phase="quantizing_text_encoder" if is_quant else "load_text_encoder",
                    message=(
                        f"Baixando e quantizando Text Encoder Qwen3 em {quant_label} ({model_id})..."
                        if is_quant
                        else f"Baixando e carregando Text Encoder Qwen3 em precisão plena ({model_id})..."
                    ),
                ),
            )

            if tcfg.text_encoder_path and Path(tcfg.text_encoder_path).is_dir():
                tokenizer_one = AutoTokenizer.from_pretrained(
                    str(tcfg.text_encoder_path), cache_dir=hub_cache, token=hf_token
                )
            else:
                tokenizer_one = AutoTokenizer.from_pretrained(
                    model_id, subfolder="tokenizer", cache_dir=hub_cache, token=hf_token
                )
        else:
            # FLUX.1: utiliza Text Encoder CLIP + T5-XXL
            text_encoder_two = load_or_quantize_text_encoder(
                model_id=model_id,
                encoder_type="t5",
                subfolder="text_encoder_2",
                encoder_cls=T5EncoderModel,
                target_dtype=target_dtype,
                quant_format=quant_format,
                quant_label=quant_label,
                bnb_config=bnb_config,
                torchao_quant_cfg=torchao_quant_cfg,
                custom_identity=custom_identity,
                force_requantize=force_requantize,
                quant_base=quant_base,
                text_encoder_cache_dir=text_encoder_cache_dir,
                hub_cache=hub_cache,
                hf_token=hf_token,
                is_flux2=False,
                on_cached=lambda p: _emit_metric(
                    metrics_path,
                    epoch=0,
                    step=4,
                    progress=0.05,
                    phase="load_text_encoder",
                    message=f"Carregando Text Encoder T5 quantizado em {quant_label} do cache persistente...",
                ),
                on_loading=lambda is_quant: print(
                    f"Baixando e quantizando Text Encoder T5 em {quant_label} ({model_id})..."
                    if is_quant
                    else f"Baixando e carregando Text Encoder T5 em precisão plena ({model_id})...",
                    flush=True,
                ),
            )

            tokenizer_one = AutoTokenizer.from_pretrained(
                model_id, subfolder="tokenizer", use_fast=False, cache_dir=hub_cache, token=hf_token
            )
            tokenizer_two = AutoTokenizer.from_pretrained(
                model_id, subfolder="tokenizer_2", use_fast=False, cache_dir=hub_cache, token=hf_token
            )
            text_encoder_one = CLIPTextModel.from_pretrained(
                model_id, subfolder="text_encoder", torch_dtype=target_dtype, cache_dir=hub_cache, token=hf_token
            ).to(device)

        _emit_metric(
            metrics_path,
            epoch=0,
            step=5,
            progress=0.06,
            phase="text_encoder_ready",
            message=f"Text Encoder ({quant_label}) pronto.",
        )

        # 3. Componentes auxiliares (VAE float32, Scheduler Flow Matching)
        vae = AutoencoderKL_cls.from_pretrained(
            model_id, subfolder="vae", torch_dtype=torch.float32, cache_dir=hub_cache, token=hf_token
        ).to(device)
        noise_scheduler = FlowMatchEulerDiscreteScheduler.from_pretrained(
            model_id, subfolder="scheduler", cache_dir=hub_cache, token=hf_token
        )

        vae.requires_grad_(False)
        text_encoder_one.requires_grad_(False)
        if text_encoder_two is not None:
            text_encoder_two.requires_grad_(False)
        transformer.requires_grad_(False)

        # 4. Injeção de adaptadores LoRA via PEFT nas camadas lineares do Transformer FLUX
        target_modules = [
            "to_k", "to_q", "to_v", "to_out.0",
            "add_k_proj", "add_v_proj", "add_q_proj", "to_add_out",
            "to_qkv_mlp_proj", "to_out_mlp_proj",
            "linear1", "linear2",
        ]
        lora_config = LoraConfig(
            r=tcfg.rank,
            lora_alpha=tcfg.alpha,
            init_lora_weights="gaussian",
            target_modules=target_modules,
        )
        # Gradient checkpointing economiza ~50% VRAM (diffusers usa use_reentrant=False nativo)
        transformer.enable_gradient_checkpointing()
        transformer = get_peft_model(transformer, lora_config)
        if tcfg.weights_path:
            _load_lora_weights(transformer, tcfg.weights_path)
        transformer.train()

        # Confirma congelamento dos pesos base e isolamento estrito da LoRA
        trainable_params_count = sum(p.numel() for p in transformer.parameters() if p.requires_grad)
        frozen_params_count = sum(p.numel() for p in transformer.parameters() if not p.requires_grad)
        print(
            f"[FLUX] Parâmetros treináveis LoRA: {trainable_params_count:,} | "
            f"Pesos base congelados: {frozen_params_count:,}",
            flush=True,
        )

        _emit_metric(
            metrics_path,
            epoch=0,
            step=6,
            progress=0.07,
            phase="setup_lora",
            message=f"Adaptadores LoRA injetados no Transformer (rank={tcfg.rank}, alpha={tcfg.alpha}, treináveis: {trainable_params_count:,}).",
        )

        # Compute VAE normalization factors
        if is_flux2:
            # FLUX.2 Klein: VAE com 32 canais e espaço latente retreinado (AutoencoderKLFlux2).
            # Jamás herdar los valores legados del Flux.1 (shift=0.1159 / scaling=0.3611).
            shift_factor = getattr(vae.config, "shift_factor", 0.0) or 0.0
            scaling_factor = getattr(vae.config, "scaling_factor", 1.0) or 1.0
            latents_mean = getattr(vae.config, "latents_mean", None)
            latents_std = getattr(vae.config, "latents_std", None)
        else:
            # FLUX.1: VAE clássica com 16 canais
            shift_factor = getattr(vae.config, "shift_factor", None)
            if shift_factor is None or shift_factor == 0.0:
                shift_factor = 0.1159
            scaling_factor = getattr(vae.config, "scaling_factor", None)
            if scaling_factor is None or scaling_factor == 0.0:
                scaling_factor = 0.3611
            latents_mean = None
            latents_std = None

        print(
            f"[FLUX] Carregamento completo: {tcfg.epochs} épocas (offset={tcfg.epoch_offset}), "
            f"is_flux2={is_flux2}, shift_factor={shift_factor}, scaling_factor={scaling_factor}, "
            f"tem_bn={hasattr(vae, 'bn') and getattr(vae.bn, 'running_mean', None) is not None}, "
            f"tem_stats_config={latents_mean is not None}",
            flush=True,
        )

        # Store everything needed in extra dict
        return ModelComponents(
            trainable_module=transformer,
            device=device,
            dtype=target_dtype,
            extra={
                "vae": vae,
                "noise_scheduler": noise_scheduler,
                "text_encoder_one": text_encoder_one,
                "text_encoder_two": text_encoder_two,
                "tokenizer_one": tokenizer_one,
                "tokenizer_two": tokenizer_two,
                "is_flux2": is_flux2,
                "shift_factor": shift_factor,
                "scaling_factor": scaling_factor,
                "latents_mean": latents_mean,
                "latents_std": latents_std,
                "hf_token": hf_token,
                "hub_cache": hub_cache,
            },
        )

    def build_text_cache_encode_fn(self, comp: ModelComponents) -> Callable[[list[str]], dict[str, Any]]:
        """Build text encoding function for cache (handles both Flux1 and Flux2)."""
        is_flux2 = comp["extra"]["is_flux2"]
        text_encoder_one = comp["extra"]["text_encoder_one"]
        text_encoder_two = comp["extra"]["text_encoder_two"]
        tokenizer_one = comp["extra"]["tokenizer_one"]
        tokenizer_two = comp["extra"]["tokenizer_two"]
        device = comp["device"]
        dtype = comp["dtype"]

        if is_flux2:
            def _encode_flux_all(caps: list[str]) -> dict[str, Any]:
                import torch
                with torch.no_grad():
                    hidden = _encode_qwen3_prompt(
                        text_encoder_one, tokenizer_one, caps, device, dtype
                    )
                    return {"hidden": hidden}
            return _encode_flux_all
        else:
            def _encode_flux_all(caps: list[str]) -> dict[str, Any]:
                import torch
                with torch.no_grad():
                    clip_inputs = tokenizer_one(
                        caps, padding="max_length", max_length=77,
                        truncation=True, return_tensors="pt",
                    ).to(device)
                    pooled = text_encoder_one(clip_inputs.input_ids).pooler_output
                    t5_inputs = tokenizer_two(
                        caps, padding="max_length", max_length=512,
                        truncation=True, return_tensors="pt",
                    ).to(device)
                    hidden = text_encoder_two(t5_inputs.input_ids)[0]
                    return {"hidden": hidden, "pooled": pooled}
            return _encode_flux_all

    def text_cache_encoders(self, comp: ModelComponents) -> list[Any]:
        """Return list of text encoders for unload/offload."""
        text_encoder_one = comp["extra"]["text_encoder_one"]
        text_encoder_two = comp["extra"]["text_encoder_two"]
        encoders = [text_encoder_one]
        if text_encoder_two is not None:
            encoders.append(text_encoder_two)
        return encoders

    def precompute_sample_embeds(self, comp: ModelComponents, tcfg: LoraTrainConfig) -> Any | None:
        """Precompute sample embeds if sample_prompt provided."""
        if not tcfg.sample_prompt:
            return None

        text_encoder_one = comp["extra"]["text_encoder_one"]
        text_encoder_two = comp["extra"]["text_encoder_two"]
        tokenizer_one = comp["extra"]["tokenizer_one"]
        tokenizer_two = comp["extra"]["tokenizer_two"]
        is_flux2 = comp["extra"]["is_flux2"]
        device = comp["device"]
        dtype = comp["dtype"]

        try:
            sample_embeds = _precompute_sample_embeds_flux(
                tokenizer_one=tokenizer_one,
                tokenizer_two=tokenizer_two,
                text_encoder_one=text_encoder_one,
                text_encoder_two=text_encoder_two,
                prompt=tcfg.sample_prompt,
                device=device,
                is_flux2=is_flux2,
                dtype=dtype,
            )
            return sample_embeds
        except Exception as e:
            print(f"[WARN] Falha ao pré-computar sample embeds FLUX: {e}", flush=True)
            return None

    def forward_and_loss(self, comp: ModelComponents, batch: dict, tcfg: LoraTrainConfig, cached_encode: dict[str, Any]) -> Any:
        """Forward pass and loss calculation with Flux1/Flux2 branching.
        
        CRITICAL: Preserves verbatim the is_flux2 branching for:
        - Latent packing (_pack_latents vs _pack_latents_flux2)
        - Image/text ID computation
        - VAE normalization (BN + stats vs shift/scaling fallback)
        - Noise/timestep sampling (flow-matching shifted logit-normal)
        - Transformer forward kwargs (guidance only for Flux1)
        """
        import torch
        import torch.nn.functional as F

        device = comp["device"]
        dtype = comp["dtype"]
        transformer = comp["trainable_module"]
        vae = comp["extra"]["vae"]
        noise_scheduler = comp["extra"]["noise_scheduler"]
        text_encoder_one = comp["extra"]["text_encoder_one"]
        text_encoder_two = comp["extra"]["text_encoder_two"]
        tokenizer_one = comp["extra"]["tokenizer_one"]
        tokenizer_two = comp["extra"]["tokenizer_two"]
        is_flux2 = comp["extra"]["is_flux2"]
        shift_factor = comp["extra"]["shift_factor"]
        scaling_factor = comp["extra"]["scaling_factor"]
        latents_mean = comp["extra"]["latents_mean"]
        latents_std = comp["extra"]["latents_std"]

        pixel_values = batch["pixel_values"].to(device)
        captions = batch["prompt"]
        bsz = pixel_values.shape[0]

        # Codifica imagens com VAE (em float32 para evitar instabilidade numérica)
        with torch.no_grad():
            latents = vae.encode(pixel_values.float()).latent_dist.sample()

            if is_flux2:
                latents = _patchify_latents_flux2(latents)
                if hasattr(vae, "bn") and getattr(vae.bn, "running_mean", None) is not None:
                    latents_bn_mean = vae.bn.running_mean.view(1, -1, 1, 1).to(latents.device, latents.dtype)
                    latents_bn_std = torch.sqrt(
                        vae.bn.running_var.view(1, -1, 1, 1) + getattr(vae.config, "batch_norm_eps", 1e-5)
                    ).to(latents.device, latents.dtype)
                    latents = (latents - latents_bn_mean) / latents_bn_std
                elif latents_mean is not None and latents_std is not None:
                    t_mean = torch.tensor(latents_mean, device=latents.device, dtype=latents.dtype).view(1, -1, 1, 1)
                    t_std = torch.tensor(latents_std, device=latents.device, dtype=latents.dtype).view(1, -1, 1, 1)
                    latents = (latents - t_mean) / t_std
                else:
                    latents = (latents - shift_factor) * scaling_factor
                latents = latents.to(dtype=dtype)
                img_ids = _prepare_flux2_latent_ids(latents)
                packed_latents = _pack_latents_flux2(latents)

                prompt_embeds = cached_encode["hidden"].to(device, dtype=dtype)
                txt_ids = _prepare_flux2_text_ids(prompt_embeds)
                pooled_prompt_embeds = None
            else:
                latents = (latents - shift_factor) * scaling_factor
                latents = latents.to(dtype=dtype)
                packed_latents = _pack_latents(latents)
                img_ids = _prepare_latent_image_ids(
                    bsz,
                    pixel_values.shape[2],
                    pixel_values.shape[3],
                    device,
                    dtype,
                )

                prompt_embeds = cached_encode["hidden"].to(device, dtype=dtype)
                pooled_prompt_embeds = cached_encode["pooled"].to(device, dtype=dtype)
                txt_ids = _prepare_text_ids(prompt_embeds.shape[1], device, prompt_embeds.dtype, batch_size=bsz)

        # Ruído gaussiano e timesteps amostrados com shifted logit-normal para Flow Matching
        noise = torch.randn_like(packed_latents)
        u = torch.normal(mean=0.0, std=1.0, size=(bsz,), device=device)
        t_sigmoid = torch.sigmoid(u)
        # Deslocamento de fluxo (time-shift schedule do Flux)
        flow_shift = float(getattr(noise_scheduler.config, "shift", 3.0) or 3.0)
        timesteps = (flow_shift * t_sigmoid) / (1.0 + (flow_shift - 1.0) * t_sigmoid)

        # Interpolação do fluxo retificado: x_t = (1 - t) * x_0 + t * noise
        t_expanded = timesteps.view(-1, 1, 1).to(dtype=dtype)
        noisy_latents = (1.0 - t_expanded) * packed_latents + t_expanded * noise
        target = noise - packed_latents

        # Forward no Transformer FLUX com adaptadores LoRA ativos
        if is_flux2:
            model_pred = transformer(
                hidden_states=noisy_latents,
                timestep=timesteps,
                encoder_hidden_states=prompt_embeds,
                txt_ids=txt_ids,
                img_ids=img_ids,
                return_dict=False,
            )[0]
        else:
            # Treino de LoRA em FLUX.1-dev exige guidance=1.0 (não usar 3.5 da inferência)
            guidance = torch.full((bsz,), 1.0, device=device, dtype=dtype)
            model_pred = transformer(
                hidden_states=noisy_latents,
                timestep=timesteps,
                guidance=guidance,
                pooled_projections=pooled_prompt_embeds,
                encoder_hidden_states=prompt_embeds,
                txt_ids=txt_ids,
                img_ids=img_ids,
                return_dict=False,
            )[0]

        loss = F.mse_loss(model_pred.float(), target.float(), reduction="mean")
        return loss

    def generate_sample(
        self, comp: ModelComponents, sample_file: Path, tcfg: LoraTrainConfig, *, epoch: int, metrics_path: Path, sample_embeds: Any | None
    ) -> None:
        """Generate sample visual and save to file."""
        transformer = comp["trainable_module"]
        vae = comp["extra"]["vae"]
        text_encoder_one = comp["extra"]["text_encoder_one"]
        text_encoder_two = comp["extra"]["text_encoder_two"]
        tokenizer_one = comp["extra"]["tokenizer_one"]
        tokenizer_two = comp["extra"]["tokenizer_two"]
        noise_scheduler = comp["extra"]["noise_scheduler"]
        is_flux2 = comp["extra"]["is_flux2"]

        _generate_sample_flux(
            transformer=transformer,
            vae=vae,
            text_encoder_one=text_encoder_one,
            text_encoder_two=text_encoder_two,
            tokenizer_one=tokenizer_one,
            tokenizer_two=tokenizer_two,
            scheduler=noise_scheduler,
            prompt=tcfg.sample_prompt,
            output_path=sample_file,
            seed=tcfg.sample_seed,
            is_flux2=is_flux2,
            resolution=tcfg.resolution,
            metrics_path=metrics_path,
            epoch=epoch,
            sample_embeds=sample_embeds,
        )

    def checkpoint_metadata(self, tcfg: LoraTrainConfig, *, epoch: int | None = None) -> dict[str, str]:
        """Return metadata for checkpoint (with epoch only if not None - same fix as Phase A)."""
        is_flux2 = tcfg.extra["is_flux2"]
        base_model = "flux-2-klein-4b" if is_flux2 else "flux-1"

        metadata = {
            "format": "pt",
            "model_type": "lora",
            "base_model": base_model,
            "lora_rank": str(tcfg.rank),
            "lora_alpha": str(tcfg.alpha),
            "trigger_word": tcfg.trigger_word,
            "quantization": tcfg.extra["quantization"],
        }
        if epoch is not None:
            metadata["epoch"] = str(epoch)
        return metadata
