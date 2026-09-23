"""
Execução de inferência real via Diffusers com aceleração CUDA e entrypoint de CLI.
"""
from __future__ import annotations

import argparse
import json
import os
import random
import sys
from pathlib import Path
from typing import Any

import yaml
from engine_kit.mock import is_mock
from trainer_difusao.common_pkg.core import _die
from trainer_difusao.common import _ensure_qwen_diffusers_compat, _setup_cache_dir
from trainer_difusao.generation.artifacts import (
    _build_generation_meta,
    _is_cancelled,
    _png_info_for_generation,
    _write_thumb,
)
from trainer_difusao.generation.config import (
    _resolve_loras_from_legacy,
    load_and_validate_generate_config,
)
from trainer_difusao.generation.mock import _mock_generate
from trainer_difusao.generation.progress import _pipe_call_kwargs_with_callback
from trainer_difusao.generation.text_encoder import (
    _flux2_repo_id,
    _load_flux2_custom_transformer,
    _load_flux2_text_encoder_override,
)


def _real_generate(
    params: dict[str, Any], output_dir: Path, pipeline: object | None = None
) -> object | None:
    """Executa a geração Text-to-Image real via Diffusers com aceleração CUDA."""
    import torch

    try:
        from trainer_difusao.telemetry import TelemetryEmitter
    except ImportError:
        from telemetry import TelemetryEmitter

    emitter = TelemetryEmitter(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    base_model = params["base_model"]
    prompt = params["prompt"]
    neg_prompt = params["negative_prompt"] or None
    width = params["width"]
    height = params["height"]
    steps = params["steps"]
    guidance = params["guidance_scale"]
    quant = params["quantization"]
    distilled = params.get("distilled", False)
    batch_size = params["batch_size"]
    loras_effective = _resolve_loras_from_legacy(params)
    custom_cp = params.get("custom_checkpoint_path")
    arch = params.get("arch")
    text_encoder_path = params.get("text_encoder_path")

    if text_encoder_path and arch in ("sdxl", "sd15"):
        _die(
            f"text_encoder_path ({text_encoder_path}) não é suportado com "
            f"checkpoint custom sdxl/sd15: o override de encoder é exclusivo "
            f"do fluxo flux-2-klein-4b. Remova text_encoder_path ou use "
            f"arch=flux-2-klein-4b."
        )

    device = "cuda" if torch.cuda.is_available() else "cpu"
    pipe_dtype = (
        torch.bfloat16
        if device == "cuda" and torch.cuda.is_bf16_supported()
        else (torch.float16 if device == "cuda" else torch.float32)
    )
    hub_cache = _setup_cache_dir()

    emitter.emit(
        phase="preparing",
        message=f"Inicializando motor de geração de imagens ({base_model})...",
        progress=0.05,
    )
    print(
        f"[DIFFUSION-GEN] Iniciando geração: model={base_model}, quant={quant}, batch={batch_size}, "
        f"loras={len(loras_effective)}, custom={'sim' if custom_cp else 'não'}...",
        flush=True,
    )

    if distilled and guidance > 2.0:
        print(
            f"[DIFFUSION-GEN] [AVISO] Modelo destilado com CFG={guidance}. Recomenda-se CFG 1.0.",
            flush=True,
        )

    if quant in ("2bit", "4bit", "6bit", "8bit") and device != "cuda":
        _die(
            f"Quantização {quant} exige GPU CUDA (device atual: {device}). "
            "Use quantization 'none' em CPU."
        )
    quantization_config = None
    if quant in ("4bit", "8bit") and device == "cuda":
        try:
            from transformers import BitsAndBytesConfig

            emitter.emit(
                phase="quantizing",
                message=f"Configurando quantização {quant} (BitsAndBytes)...",
                progress=0.15,
            )
            if quant == "4bit":
                quantization_config = BitsAndBytesConfig(
                    load_in_4bit=True,
                    bnb_4bit_quant_type="nf4",
                    bnb_4bit_use_double_quant=True,
                    bnb_4bit_compute_dtype=torch.bfloat16,
                )
            else:
                quantization_config = BitsAndBytesConfig(load_in_8bit=True)
        except (ImportError, RuntimeError, ValueError) as e:
            print(
                f"[WARN] Falha ao configurar BitsAndBytes: {e}. Usando precisão padrão.",
                flush=True,
            )
    elif quant in ("2bit", "6bit"):
        from trainer_difusao.quantization import build_torchao_config

        emitter.emit(
            phase="quantizing",
            message=f"Configurando quantização {quant} (TorchAO)...",
            progress=0.15,
        )
        quantization_config = build_torchao_config(quant)

    if pipeline is not None:
        pipe = pipeline
        print("[DIFFUSION-GEN] Usando pipeline do cache (hot path).", flush=True)
    else:
        emitter.emit(
            phase="loading_model",
            message=f"Carregando pesos do modelo {base_model}...",
            progress=0.25,
        )

        pipe = None

        if custom_cp and arch in ("sdxl", "sd15"):
            print(
                f"[DIFFUSION-GEN] Carregando checkpoint custom: {custom_cp} (arch={arch})",
                flush=True,
            )
            if arch == "sdxl":
                from diffusers import StableDiffusionXLPipeline

                load_kwargs: dict[str, Any] = {
                    "torch_dtype": (
                        torch.float16 if device == "cuda" else torch.float32
                    ),
                }
                if quantization_config and device == "cuda":
                    load_kwargs["quantization_config"] = quantization_config
                pipe = StableDiffusionXLPipeline.from_single_file(
                    custom_cp, **load_kwargs
                )
            elif arch == "sd15":
                from diffusers import StableDiffusionPipeline

                load_kwargs_sd15: dict[str, Any] = {
                    "torch_dtype": (
                        torch.float16 if device == "cuda" else torch.float32
                    ),
                }
                if quantization_config and device == "cuda":
                    load_kwargs_sd15["quantization_config"] = quantization_config
                pipe = StableDiffusionPipeline.from_single_file(
                    custom_cp, **load_kwargs_sd15
                )

            if pipe and device == "cuda" and not quantization_config:
                pipe.to(device)

        elif base_model == "flux-2-klein-4b":
            from diffusers import Flux2KleinPipeline

            model_repo = _flux2_repo_id(distilled=distilled)
            pipe_dtype = torch.bfloat16 if device == "cuda" else torch.float32
            flux_kwargs: dict[str, Any] = {"torch_dtype": pipe_dtype}
            encoder_override = params.get("text_encoder_path")
            if encoder_override:
                try:
                    enc_model, enc_tok = _load_flux2_text_encoder_override(
                        encoder_override, model_repo, pipe_dtype,
                        quantization_config=quantization_config,
                    )
                except SystemExit:
                    raise
                except Exception as exc:
                    _die(
                        f"Falha ao carregar text_encoder custom "
                        f"({encoder_override}): {exc}"
                    )
                flux_kwargs["text_encoder"] = enc_model
                flux_kwargs["tokenizer"] = enc_tok
            if custom_cp and arch == "flux-2-klein-4b":
                flux_kwargs["transformer"] = _load_flux2_custom_transformer(
                    custom_cp, pipe_dtype, quantization_config=quantization_config
                )
                print(
                    f"[DIFFUSION-GEN] Carregando FLUX.2 Klein 4B custom: "
                    f"{custom_cp} (componentes base: {model_repo})",
                    flush=True,
                )
            else:
                print(
                    f"[DIFFUSION-GEN] Carregando FLUX.2 Klein 4B "
                    f"({'Destilado' if distilled else 'Base'}): {model_repo}",
                    flush=True,
                )
            try:
                pipe = Flux2KleinPipeline.from_pretrained(model_repo, **flux_kwargs)
            except SystemExit:
                raise
            except Exception as exc:
                _die(
                    f"Falha ao carregar pipeline FLUX.2 Klein ({model_repo}): {exc}"
                )
            if quantization_config is None and device == "cuda":
                pipe.to(device)
            else:
                pipe.enable_model_cpu_offload()

        elif base_model == "sdxl":
            from diffusers import AutoencoderKL, StableDiffusionXLPipeline

            vae = AutoencoderKL.from_pretrained(
                "madebyollin/sdxl-vae-fp16-fix",
                torch_dtype=torch.float32,
            )
            pipe = StableDiffusionXLPipeline.from_pretrained(
                "stabilityai/stable-diffusion-xl-base-1.0",
                vae=vae,
                torch_dtype=torch.float16 if device == "cuda" else torch.float32,
                use_safetensors=True,
            )
            if device == "cuda":
                pipe.to(device)

        elif base_model == "sd15":
            from diffusers import StableDiffusionPipeline

            pipe = StableDiffusionPipeline.from_pretrained(
                "runwayml/stable-diffusion-v1-5",
                torch_dtype=torch.float16 if device == "cuda" else torch.float32,
                use_safetensors=True,
            )
            if device == "cuda":
                pipe.to(device)

        elif base_model == "qwen-image-2.1":
            _ensure_qwen_diffusers_compat()
            import diffusers

            QwenPipelineCls = getattr(
                diffusers, "QwenImage21Pipeline", getattr(diffusers, "QwenImagePipeline", None)
            )
            if QwenPipelineCls is None:
                _die(
                    "QwenImage21Pipeline não disponível na versão instalada do diffusers. "
                    "Instale diffusers>=0.41.0.dev0 ou git+https://github.com/huggingface/diffusers.git"
                )
            model_repo = os.environ.get("QWEN_IMAGE_MODEL_ID", "Qwen/Qwen-Image-2.1")
            print(f"[DIFFUSION-GEN] Carregando Qwen-Image-2.1: {model_repo}", flush=True)
            pipe_kwargs: dict[str, Any] = {
                "torch_dtype": pipe_dtype,
                "cache_dir": hub_cache,
            }
            if quantization_config is not None and device == "cuda":
                from trainer_difusao.models.flux_pkg.quant_cache import _is_cache_valid, _save_quant_metadata
                base_dir = (
                    Path("/data/outputs")
                    if Path("/data/outputs").exists()
                    else (Path("/outputs") if Path("/outputs").exists() else Path.home() / ".cache" / "hephaestus")
                )
                quant_base = base_dir / ".cache" / "quantized" / f"qwen_image_2_1_{quant}"
                trans_cache_dir = quant_base / "transformer"
                text_cache_dir = quant_base / "text_encoder"

                TransformerCls = getattr(
                    diffusers, "QwenImage21Transformer2DModel", getattr(diffusers, "QwenImageTransformer2DModel", None)
                )

                # 1. Carrega do cache ou quantiza e persiste o Transformer
                if _is_cache_valid(trans_cache_dir, model_repo, quant) and TransformerCls is not None:
                    print(f"[DIFFUSION-GEN] Carregando Transformer {quant} do cache persistente: {trans_cache_dir}", flush=True)
                    pipe_kwargs["transformer"] = TransformerCls.from_pretrained(
                        trans_cache_dir,
                        torch_dtype=pipe_dtype,
                    )
                elif TransformerCls is not None:
                    try:
                        print(f"[DIFFUSION-GEN] Quantizando Transformer em {quant} (BitsAndBytes)...", flush=True)
                        t_mod = TransformerCls.from_pretrained(
                            model_repo,
                            subfolder="transformer",
                            quantization_config=quantization_config,
                            torch_dtype=pipe_dtype,
                            cache_dir=hub_cache,
                        )
                        trans_cache_dir.mkdir(parents=True, exist_ok=True)
                        t_mod.save_pretrained(trans_cache_dir)
                        _save_quant_metadata(quant_base, model_repo, quant, quant, pipe_dtype, False)
                        print(f"[DIFFUSION-GEN] Transformer {quant} salvo no cache persistente: {trans_cache_dir}", flush=True)
                        pipe_kwargs["transformer"] = t_mod
                    except Exception as e:
                        print(f"[WARN] Falha ao quantizar/salvar transformer ({e}).", flush=True)

                # 2. Carrega do cache ou quantiza e persiste o Text Encoder
                try:
                    from transformers import Qwen3VLForConditionalGeneration
                    if _is_cache_valid(text_cache_dir, model_repo, quant):
                        print(f"[DIFFUSION-GEN] Carregando Text Encoder {quant} do cache persistente: {text_cache_dir}", flush=True)
                        pipe_kwargs["text_encoder"] = Qwen3VLForConditionalGeneration.from_pretrained(
                            text_cache_dir,
                            torch_dtype=pipe_dtype,
                        )
                    else:
                        print(f"[DIFFUSION-GEN] Quantizando Text Encoder em {quant} (BitsAndBytes)...", flush=True)
                        te_mod = Qwen3VLForConditionalGeneration.from_pretrained(
                            model_repo,
                            subfolder="text_encoder",
                            quantization_config=quantization_config,
                            torch_dtype=pipe_dtype,
                            cache_dir=hub_cache,
                        )
                        text_cache_dir.mkdir(parents=True, exist_ok=True)
                        te_mod.save_pretrained(text_cache_dir)
                        _save_quant_metadata(quant_base, model_repo, quant, quant, pipe_dtype, False)
                        print(f"[DIFFUSION-GEN] Text Encoder {quant} salvo no cache persistente: {text_cache_dir}", flush=True)
                        pipe_kwargs["text_encoder"] = te_mod
                except Exception as e:
                    print(f"[WARN] Falha ao quantizar/salvar text_encoder ({e}).", flush=True)
            pipe = QwenPipelineCls.from_pretrained(
                model_repo,
                **pipe_kwargs,
            )
            if device == "cuda":
                if "text_encoder" in pipe_kwargs:
                    pipe.enable_model_cpu_offload()
                else:
                    pipe.enable_sequential_cpu_offload()
        else:
            _die(f"Modelo não suportado para geração real: {base_model}")

    if loras_effective:
        emitter.emit(
            phase="injecting_lora",
            message=f"Carregando {len(loras_effective)} adaptador(es) LoRA...",
            progress=0.40,
        )
        if base_model == "flux-2-klein-4b":
            adapter_names = []
            adapter_scales = []
            for idx, lora in enumerate(loras_effective):
                name = f"lora_{idx}"
                print(
                    f"[DIFFUSION-GEN] Carregando LoRA {idx}: {lora['path']} (scale={lora['scale']})",
                    flush=True,
                )
                pipe.load_lora_weights(lora["path"], adapter_name=name)
                adapter_names.append(name)
                adapter_scales.append(lora["scale"])

            try:
                pipe.transformer.set_adapters(adapter_names, adapter_scales)
                print(
                    f"[DIFFUSION-GEN] Multi-LoRA aplicado via transformer.set_adapters: "
                    f"{adapter_names}",
                    flush=True,
                )
            except (AttributeError, RuntimeError, OSError) as exc:
                print(
                    f"[DIFFUSION-GEN] [AVISO] transformer.set_adapters falhou ({exc}). "
                    f"Fallback: aplicando apenas o primeiro LoRA ({adapter_names[0]}). "
                    f"Ref: ADR-0023 spike S1.",
                    flush=True,
                )
                try:
                    pipe.transformer.set_adapters(
                        [adapter_names[0]], [adapter_scales[0]]
                    )
                except (AttributeError, RuntimeError, OSError) as exc2:
                    print(
                        f"[DIFFUSION-GEN] [ERRO] transformer.set_adapters(1 LoRA) "
                        f"também falhou ({exc2}). LoRA não aplicada.",
                        flush=True,
                    )
        else:
            adapter_names = []
            adapter_scales = []
            for idx, lora in enumerate(loras_effective):
                name = f"lora_{idx}"
                print(
                    f"[DIFFUSION-GEN] Carregando LoRA {idx}: {lora['path']} (scale={lora['scale']})",
                    flush=True,
                )
                pipe.load_lora_weights(lora["path"], adapter_name=name)
                adapter_names.append(name)
                adapter_scales.append(lora["scale"])

            pipe.set_adapters(adapter_names, adapter_scales)
            print(f"[DIFFUSION-GEN] Multi-LoRA aplicado: {adapter_names}", flush=True)

    init_image_path = params.get("init_image_path")
    init_strength = params.get("init_strength")
    is_img2img = bool(init_image_path)
    init_image = None
    call_pipe = pipe
    if is_img2img:
        if base_model in ("flux-2-klein-4b", "qwen-image-2.1"):
            call_pipe = pipe
        elif base_model == "sdxl":
            from diffusers import StableDiffusionXLImg2ImgPipeline as _I2I

            call_pipe = _I2I(**pipe.components)
        elif base_model == "sd15":
            from diffusers import StableDiffusionImg2ImgPipeline as _I2I

            call_pipe = _I2I(**pipe.components)
        else:
            _die(f"img2img não suportado para o modelo: {base_model}")
        print(
            f"[DIFFUSION-GEN] img2img: variante={type(call_pipe).__name__} "
            f"(cache preservado), strength={init_strength}",
            flush=True,
        )
        try:
            from PIL import Image as _PILImage

            with _PILImage.open(init_image_path) as _f:
                init_image = _f.convert("RGB").resize(
                    (width, height), _PILImage.LANCZOS
                )
            print(
                f"[DIFFUSION-GEN] img2img: init={init_image_path} "
                f"(stretch exato {width}x{height}, LANCZOS)",
                flush=True,
            )
        except Exception as exc:
            _die(f"Falha ao abrir init_image ({init_image_path}): {exc}")

    from trainer_difusao.schedulers import build_scheduler, swapped_scheduler

    sampler_name = params.get("sampler", "default")
    upscale_cfg = params.get("upscale")
    sched_arch = (
        "flux" if base_model in ("flux-2-klein-4b", "qwen-image-2.1") else "sd"
    )
    if base_model not in ("flux-2-klein-4b", "sdxl", "sd15", "qwen-image-2.1"):
        _die(f"Modelo não suportado para inferência: {base_model}")
    fresh_scheduler = None
    if sampler_name and sampler_name != "default":
        try:
            base_sched_config = dict(call_pipe.scheduler.config)
        except (AttributeError, TypeError) as e:
            _die(
                f"Sampler '{sampler_name}': pipeline sem scheduler configurável ({e})."
            )
        try:
            fresh_scheduler = build_scheduler(
                sampler_name, sched_arch, base_sched_config
            )
        except (ValueError, ImportError) as e:
            _die(f"Sampler '{sampler_name}': {e}")
        print(
            f"[DIFFUSION-GEN] Sampler '{sampler_name}' → "
            f"{type(fresh_scheduler).__name__} (fresh-instance, restore no finally).",
            flush=True,
        )

    seed_base = (
        params["seed"] if params["seed"] is not None else random.randint(0, 2**31 - 1)
    )
    meta_lines: list[dict[str, Any]] = []

    with swapped_scheduler(call_pipe, fresh_scheduler):
        for i in range(batch_size):
            if _is_cancelled(output_dir):
                print(
                    f"[DIFFUSION-GEN] Cancel detectado antes do item {i}. Saindo.",
                    flush=True,
                )
                break

            current_seed = seed_base + i
            generator = torch.Generator(device=device).manual_seed(current_seed)

            emitter.emit(
                phase="generating",
                message=f"Iniciando amostragem da imagem {i + 1}/{batch_size} (seed={current_seed})...",
                progress=0.55 + (0.35 * i / batch_size),
                step=i,
                total_steps=batch_size,
            )
            print(
                f"[DIFFUSION-GEN] Gerando imagem {i + 1}/{batch_size} (seed={current_seed})...",
                flush=True,
            )

            sampler_cb_kwargs = _pipe_call_kwargs_with_callback(emitter, i, batch_size, steps)
            if base_model == "flux-2-klein-4b":
                flux_call_kwargs: dict[str, Any] = {
                    "prompt": prompt,
                    "generator": generator,
                    "num_inference_steps": steps,
                    "guidance_scale": guidance,
                    "width": width,
                    "height": height,
                }
                if is_img2img:
                    flux_call_kwargs["image"] = init_image
                with torch.inference_mode():
                    try:
                        image = call_pipe(**flux_call_kwargs, **sampler_cb_kwargs).images[0]
                    except TypeError as exc:
                        if "callback_on_step_end" not in str(exc):
                            raise
                        print(
                            f"[DIFFUSION-GEN] [AVISO] pipeline não suporta callback "
                            f"de progresso ({exc}). Seguindo sem telemetria fina.",
                            flush=True,
                        )
                        image = call_pipe(**flux_call_kwargs).images[0]
            elif base_model in ("sdxl", "sd15"):
                with torch.inference_mode():
                    sd_kwargs: dict[str, Any] = {
                        "prompt": prompt,
                        "negative_prompt": neg_prompt,
                        "generator": generator,
                        "num_inference_steps": steps,
                        "guidance_scale": guidance,
                        "width": width,
                        "height": height,
                    }
                    if is_img2img:
                        sd_kwargs["image"] = init_image
                        sd_kwargs["strength"] = init_strength
                    try:
                        image = call_pipe(**sd_kwargs, **sampler_cb_kwargs).images[0]
                    except TypeError as exc:
                        if "callback_on_step_end" not in str(exc):
                            raise
                        print(
                            f"[DIFFUSION-GEN] [AVISO] pipeline não suporta callback "
                            f"de progresso ({exc}). Seguindo sem telemetria fina.",
                            flush=True,
                        )
                        image = call_pipe(**sd_kwargs).images[0]
            elif base_model == "qwen-image-2.1":
                qwen_call_kwargs: dict[str, Any] = {
                    "prompt": prompt,
                    "generator": generator,
                    "num_inference_steps": steps,
                    "true_cfg_scale": guidance,
                    "width": width,
                    "height": height,
                }
                if neg_prompt:
                    qwen_call_kwargs["negative_prompt"] = neg_prompt
                if is_img2img:
                    qwen_call_kwargs["image"] = init_image
                with torch.inference_mode():
                    try:
                        image = call_pipe(**qwen_call_kwargs, **sampler_cb_kwargs).images[0]
                    except TypeError as exc:
                        if "callback_on_step_end" not in str(exc):
                            raise
                        print(
                            f"[DIFFUSION-GEN] [AVISO] pipeline não suporta callback "
                            f"de progresso ({exc}). Seguindo sem telemetria fina.",
                            flush=True,
                        )
                        image = call_pipe(**qwen_call_kwargs).images[0]
            else:
                _die(f"Modelo não suportado para inferência: {base_model}")

            emitter.emit(
                phase="saving",
                message=f"Salvando imagem {i + 1}/{batch_size}...",
                progress=0.92,
            )

            filename = f"generated_{i + 1:04d}.png"
            out_file = output_dir / filename
            thumb_filename = f"thumb_{i + 1:04d}.jpg"
            meta_entry = _build_generation_meta(
                params=params,
                filename=filename,
                thumb_filename=thumb_filename,
                seed=current_seed,
                batch_index=i,
                batch_size=batch_size,
                loras_effective=loras_effective,
            )
            image.save(out_file, "PNG", pnginfo=_png_info_for_generation(meta_entry))
            print(
                f"[DIFFUSION-GEN] Imagem {i + 1}/{batch_size} salva: {out_file}", flush=True
            )

            if upscale_cfg:
                from trainer_difusao.upscale import upscale_image

                dims = upscale_image(
                    out_file,
                    out_file,
                    model=str(upscale_cfg["model"]),
                    scale=int(upscale_cfg["scale"]),
                )
                meta_entry["upscale"] = {
                    "model": str(upscale_cfg["model"]),
                    "scale": int(upscale_cfg["scale"]),
                    **dims,
                }
                from PIL import Image as _UpImage

                with _UpImage.open(out_file) as _up:
                    _up.save(
                        out_file, "PNG", pnginfo=_png_info_for_generation(meta_entry)
                    )
                print(
                    f"[DIFFUSION-GEN] Upscale {upscale_cfg['model']} x{upscale_cfg['scale']}: "
                    f"{dims['original_width']}x{dims['original_height']} → "
                    f"{dims['final_width']}x{dims['final_height']} ({out_file})",
                    flush=True,
                )

            thumb_path = output_dir / thumb_filename
            _write_thumb(out_file, thumb_path)
            meta_lines.append(meta_entry)

    if batch_size == 1:
        legacy_png = output_dir / "generated.png"
        new_png = output_dir / "generated_0001.png"
        if not legacy_png.exists():
            try:
                legacy_png.symlink_to(new_png.name)
            except OSError:
                import shutil

                shutil.copy2(new_png, legacy_png)

    meta_path = output_dir / "generation_meta.json"
    with open(meta_path, "w", encoding="utf-8") as f:
        f.writelines(
            json.dumps(entry, ensure_ascii=False) + "\n" for entry in meta_lines
        )
    print(
        f"[DIFFUSION-GEN] Metadados salvos: {meta_path} ({len(meta_lines)} entradas)",
        flush=True,
    )

    emitter.emit(
        phase="completed",
        message=f"Geração finalizada com sucesso! {len(meta_lines)}/{batch_size} imagem(ns).",
        progress=1.0,
    )

    return pipe


def cmd_generate(args: list[str]) -> None:
    parser = argparse.ArgumentParser(
        prog="trainer-difusao generate",
        description="Geração Text-to-Image (FLUX.2 Klein, SDXL, SD 1.5, batch, multi-LoRA, custom)",
    )
    parser.add_argument("--config", required=True, help="Caminho para config.yaml")
    parser.add_argument("--output", required=True, help="Diretório de saída")

    opts = parser.parse_args(args)
    if not os.path.exists(opts.config):
        _die(f"Arquivo de configuração não encontrado: {opts.config}")

    with open(opts.config, "r", encoding="utf-8") as f:
        cfg = yaml.safe_load(f)

    params = load_and_validate_generate_config(cfg)
    output_dir = Path(opts.output)

    is_mock_mode = is_mock()
    if is_mock_mode:
        _mock_generate(params, output_dir)
    else:
        _real_generate(params, output_dir)
