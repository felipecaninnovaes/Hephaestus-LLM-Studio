"""Módulo de inferência/geração Text-to-Image para o Playground de Difusão.

Suporta FLUX.2 Klein 4B, SDXL e Stable Diffusion 1.5, com aplicação opcional de adaptadores LoRA.
ENGINE_MOCK=1 (default no dev) → geração sintética determinística com Pillow.
ENGINE_MOCK=0 (@gpu)           → pipeline real Diffusers com aceleração CUDA e quantização.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import struct
import sys
from pathlib import Path
from typing import Any

import yaml


def _die(msg: str) -> None:
    print(f"ERROR: {msg}", file=sys.stderr)
    sys.exit(1)


def _canonical_model_name(raw_model: str) -> str:
    norm = raw_model.strip().lower()
    if norm in ("flux", "flux2", "flux-2", "flux2-klein-4b", "flux.2-klein-4b"):
        return "flux-2-klein-4b"
    if norm in ("sdxl", "sdxl-1.0"):
        return "sdxl"
    if norm in ("sd15", "sd-1.5", "stable-diffusion-v1-5"):
        return "sd15"
    return norm


def load_and_validate_generate_config(cfg: dict[str, Any]) -> dict[str, Any]:
    """Valida o dicionário de configuração de geração Text-to-Image."""
    if not isinstance(cfg, dict):
        _die("Configuração raiz deve ser um dicionário YAML.")

    job_id = cfg.get("job_id")
    if not job_id:
        _die("Campo obrigatório ausente: job_id")

    gen_cfg = cfg.get("generate")
    if not isinstance(gen_cfg, dict):
        _die("Seção obrigatória ausente: generate")

    prompt = gen_cfg.get("prompt")
    if not prompt or not str(prompt).strip():
        _die("Campo 'prompt' é obrigatório e não pode ser vazio.")

    raw_base_model = gen_cfg.get("base_model") or cfg.get("model") or "flux-2-klein-4b"
    base_model = _canonical_model_name(str(raw_base_model))
    if base_model not in ("flux-2-klein-4b", "sdxl", "sd15"):
        _die(f"Modelo base de difusão não suportado: {raw_base_model}")

    width = int(gen_cfg.get("width", 1024))
    height = int(gen_cfg.get("height", 1024))
    if width < 256 or width > 2048 or height < 256 or height > 2048:
        _die(f"Dimensões inválidas ({width}x{height}). Permitido entre 256 e 2048.")

    steps = int(gen_cfg.get("steps", 20))
    if steps < 1 or steps > 100:
        _die(f"Steps inválido: {steps}. Deve estar entre 1 e 100.")

    guidance_scale = float(gen_cfg.get("guidance_scale", 3.5 if "flux" in base_model else 7.0))
    if guidance_scale < 1.0 or guidance_scale > 30.0:
        _die(f"Guidance scale inválido: {guidance_scale}. Deve estar entre 1.0 e 30.0.")

    quantization = str(gen_cfg.get("quantization", "4bit")).strip().lower()
    if quantization not in ("none", "4bit", "8bit"):
        _die(f"Nível de quantização inválido: {quantization}. Use 'none', '4bit' ou '8bit'.")

    seed = int(gen_cfg.get("seed", 42))
    lora_scale = float(gen_cfg.get("lora_scale", 1.0))
    if lora_scale < 0.0 or lora_scale > 2.0:
        _die(f"lora_scale inválido: {lora_scale}. Deve estar entre 0.0 e 2.0.")

    weights_path = cfg.get("weights_path") or gen_cfg.get("weights_path")
    negative_prompt = gen_cfg.get("negative_prompt") or ""
    distilled = bool(gen_cfg.get("distilled", False))

    return {
        "job_id": str(job_id),
        "base_model": base_model,
        "prompt": str(prompt).strip(),
        "negative_prompt": str(negative_prompt).strip(),
        "width": width,
        "height": height,
        "steps": steps,
        "guidance_scale": guidance_scale,
        "seed": seed,
        "quantization": quantization,
        "distilled": distilled,
        "weights_path": str(weights_path) if weights_path else None,
        "lora_scale": lora_scale,
    }


def _mock_generate(params: dict[str, Any], output_dir: Path) -> Path:
    """Gera uma imagem de mock determinística com visual representativo e metadados visuais."""
    from PIL import Image, ImageDraw

    width = params["width"]
    height = params["height"]
    seed = params["seed"]
    base_model = params["base_model"]
    prompt = params["prompt"]
    neg = params["negative_prompt"]
    steps = params["steps"]
    cfg = params["guidance_scale"]
    quant = params["quantization"]
    distilled = params.get("distilled", False)
    weights_path = params["weights_path"]

    # Fundo com degradê escuro óptico determinístico baseado na seed
    h = hashlib.sha256(struct.pack("<q", seed)).digest()
    r_base = 20 + (h[0] % 35)
    g_base = 15 + (h[1] % 30)
    b_base = 35 + (h[2] % 50)

    img = Image.new("RGB", (width, height), (r_base, g_base, b_base))
    draw = ImageDraw.Draw(img)

    # Desenho de círculos concêntricos e linhas geométricas simulando geração de imagem
    for i in range(12):
        radius = int(min(width, height) * (0.08 * (i + 1)))
        cx = int(width / 2 + ((h[i % len(h)] - 128) / 256.0) * (width * 0.15))
        cy = int(height / 2 + ((h[(i + 4) % len(h)] - 128) / 256.0) * (height * 0.15))
        alpha_color = (
            min(255, r_base + i * 15 + (h[i] % 40)),
            min(255, g_base + i * 10 + (h[(i + 1) % len(h)] % 40)),
            min(255, b_base + i * 18 + (h[(i + 2) % len(h)] % 50)),
        )
        draw.ellipse([cx - radius, cy - radius, cx + radius, cy + radius], outline=alpha_color, width=2)

    # Card de informações e metadados no rodapé
    pad = 24
    card_h = 160
    card_box = [pad, height - card_h - pad, width - pad, height - pad]
    draw.rectangle(card_box, fill=(18, 18, 22), outline=(131, 80, 242), width=2)

    # Textos informativos
    # Textos informativos
    variant_label = "DESTILADO (4-8 steps)" if distilled else "BASE (20+ steps)"
    title_text = f"HEPHAESTUS STUDIO · PLAYGROUND DE DIFUSÃO [{base_model.upper()} · {variant_label}]"
    prompt_line = f"Prompt: {prompt[:70]}{'...' if len(prompt) > 70 else ''}"
    if neg:
        prompt_line += f" | Neg: {neg[:30]}"
    meta_line1 = f"Seed: {seed} | Steps: {steps} | CFG: {cfg} | Quant: {quant}"
    lora_label = f"LoRA: {Path(weights_path).name}" if weights_path else "LoRA: Nenhum (Base Puro)"
    meta_line2 = f"{lora_label} | Res: {width}x{height} | Mode: MOCK DETERMINÍSTICO"

    draw.text((pad + 16, height - card_h - pad + 16), title_text, fill=(200, 180, 255))
    draw.text((pad + 16, height - card_h - pad + 48), prompt_line, fill=(255, 255, 255))
    draw.text((pad + 16, height - card_h - pad + 80), meta_line1, fill=(180, 180, 195))
    draw.text((pad + 16, height - card_h - pad + 108), meta_line2, fill=(140, 220, 160))

    output_dir.mkdir(parents=True, exist_ok=True)
    out_file = output_dir / "generated.png"
    img.save(out_file, "PNG")
    print(f"[MOCK-GEN] Imagem gerada com sucesso ({width}x{height}, seed={seed}, {variant_label}): {out_file}", flush=True)
    return out_file


def _real_generate(params: dict[str, Any], output_dir: Path) -> Path:
    """Executa a geração Text-to-Image real via Diffusers com aceleração CUDA."""
    import torch

    base_model = params["base_model"]
    prompt = params["prompt"]
    neg_prompt = params["negative_prompt"] or None
    width = params["width"]
    height = params["height"]
    steps = params["steps"]
    guidance = params["guidance_scale"]
    seed = params["seed"]
    quant = params["quantization"]
    distilled = params.get("distilled", False)
    weights_path = params["weights_path"]
    lora_scale = params["lora_scale"]

    device = "cuda" if torch.cuda.is_available() else "cpu"
    generator = torch.Generator(device=device).manual_seed(seed)

    variant_str = "Destilado (4-8 steps)" if distilled else "Base (20+ steps)"
    print(f"[DIFFUSION-GEN] Iniciando geração real: model={base_model} [{variant_str}], quant={quant}, seed={seed}, steps={steps}, CFG={guidance}...", flush=True)
    if distilled and guidance > 2.0:
        print(f"[DIFFUSION-GEN] [AVISO] Modelo destilado em execução com CFG={guidance}. Recomenda-se CFG 1.0 para evitar saturação/queima.", flush=True)

    # Configuração de quantização
    bnb_config = None
    if quant in ("4bit", "8bit") and device == "cuda":
        try:
            from transformers import BitsAndBytesConfig

            if quant == "4bit":
                bnb_config = BitsAndBytesConfig(
                    load_in_4bit=True,
                    bnb_4bit_quant_type="nf4",
                    bnb_4bit_use_double_quant=True,
                    bnb_4bit_compute_dtype=torch.bfloat16,
                )
            else:
                bnb_config = BitsAndBytesConfig(load_in_8bit=True)
        except (ImportError, RuntimeError, ValueError) as e:
            print(f"[WARN] Falha ao configurar BitsAndBytes: {e}. Usando precisão padrão.", flush=True)

    if base_model == "flux-2-klein-4b":
        from diffusers import Flux2KleinPipeline

        pipe_kwargs: dict[str, Any] = {
            "torch_dtype": torch.bfloat16 if device == "cuda" else torch.float32,
        }
        model_repo = (
            (os.environ.get("FLUX_DISTILLED_MODEL_ID") or "unsloth/FLUX.2-klein-4B")
            if distilled
            else (os.environ.get("FLUX_MODEL_ID") or "unsloth/FLUX.2-klein-4B")
        )
        print(f"[DIFFUSION-GEN] Carregando FLUX.2 Klein 4B ({'Destilado' if distilled else 'Base'}): {model_repo}", flush=True)
        pipe = Flux2KleinPipeline.from_pretrained(model_repo, **pipe_kwargs)
        if bnb_config is None and device == "cuda":
            pipe.to(device)
        else:
            pipe.enable_model_cpu_offload()

        if weights_path and os.path.exists(weights_path):
            print(f"[DIFFUSION-GEN] Injetando pesos LoRA: {weights_path} (scale={lora_scale})", flush=True)
            pipe.load_lora_weights(weights_path)

        with torch.inference_mode():
            image = pipe(
                prompt=prompt,
                generator=generator,
                num_inference_steps=steps,
                guidance_scale=guidance,
                width=width,
                height=height,
            ).images[0]

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

        if weights_path and os.path.exists(weights_path):
            print(f"[DIFFUSION-GEN] Injetando pesos LoRA: {weights_path} (scale={lora_scale})", flush=True)
            pipe.load_lora_weights(weights_path)

        with torch.inference_mode():
            image = pipe(
                prompt=prompt,
                negative_prompt=neg_prompt,
                generator=generator,
                num_inference_steps=steps,
                guidance_scale=guidance,
                width=width,
                height=height,
            ).images[0]

    elif base_model == "sd15":
        from diffusers import StableDiffusionPipeline

        pipe = StableDiffusionPipeline.from_pretrained(
            "runwayml/stable-diffusion-v1-5",
            torch_dtype=torch.float16 if device == "cuda" else torch.float32,
            use_safetensors=True,
        )
        if device == "cuda":
            pipe.to(device)

        if weights_path and os.path.exists(weights_path):
            print(f"[DIFFUSION-GEN] Injetando pesos LoRA: {weights_path} (scale={lora_scale})", flush=True)
            pipe.load_lora_weights(weights_path)

        with torch.inference_mode():
            image = pipe(
                prompt=prompt,
                negative_prompt=neg_prompt,
                generator=generator,
                num_inference_steps=steps,
                guidance_scale=guidance,
                width=width,
                height=height,
            ).images[0]
    else:
        _die(f"Modelo não suportado para geração real: {base_model}")

    output_dir.mkdir(parents=True, exist_ok=True)
    out_file = output_dir / "generated.png"
    image.save(out_file, "PNG")
    print(f"[DIFFUSION-GEN] Geração concluída com sucesso: {out_file}", flush=True)
    return out_file


def cmd_generate(args: list[str]) -> None:
    parser = argparse.ArgumentParser(
        prog="trainer-difusao generate",
        description="Geração Text-to-Image para Playground de Difusão (FLUX.2 Klein, SDXL, SD 1.5)",
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

    is_mock = os.environ.get("ENGINE_MOCK", "1") == "1"
    if is_mock:
        _mock_generate(params, output_dir)
    else:
        _real_generate(params, output_dir)
