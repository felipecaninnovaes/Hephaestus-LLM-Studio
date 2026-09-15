"""Módulo de inferência/geração Text-to-Image para o Hephaestus Studio.

Suporta FLUX.2 Klein 4B, SDXL e Stable Diffusion 1.5, com aplicação opcional de
adaptadores LoRA (multi-LoRA), checkpoints custom e geração em lote (batch).

ENGINE_MOCK=1 (default no dev) → geração sintética determinística com Pillow.
ENGINE_MOCK=0 (@gpu)           → pipeline real Diffusers com aceleração CUDA e quantização.

ADR-0023 — Fatia G.2: batch + multi-LoRA + custom + meta + thumbs.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import random
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


def _write_thumb(
    src_path: Path, dst_path: Path, max_side: int = 512, quality: int = 80
) -> None:
    """Salva thumbnail JPEG com max-side preservando proporção (PIL.Image.thumbnail)."""
    from PIL import Image

    with Image.open(src_path) as img:
        img.thumbnail((max_side, max_side), Image.LANCZOS)
        img.convert("RGB").save(dst_path, "JPEG", quality=quality)


def _is_cancelled(output_dir: Path) -> bool:
    """Checa se existe sentinela de cancelamento no output_dir."""
    return (output_dir / "cancel").exists()


def load_and_validate_generate_config(cfg: dict[str, Any]) -> dict[str, Any]:
    """Valida o dicionário de configuração de geração Text-to-Image.

    Retrocompatível: chaves novas (batch_size, loras, custom_checkpoint_path, arch)
    são opcionais e possuem defaults que reproduzem o comportamento legado.
    """
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

    # --- batch_size (1..8, default 1) ---
    batch_size = int(gen_cfg.get("batch_size", 1))
    if batch_size < 1 or batch_size > 8:
        _die(f"batch_size inválido: {batch_size}. Deve estar entre 1 e 8.")

    # --- loras (lista de {path, scale}, max 4) ---
    raw_loras = gen_cfg.get("loras", [])
    if not isinstance(raw_loras, list):
        _die("Campo 'loras' deve ser uma lista.")
    if len(raw_loras) > 4:
        _die(f"Máximo de 4 LoRAs permitido. Recebido: {len(raw_loras)}.")
    loras: list[dict[str, Any]] = []
    for i, entry in enumerate(raw_loras):
        if not isinstance(entry, dict):
            _die(f"LoRA [{i}] deve ser um dicionário {{path, scale}}.")
        lora_path = entry.get("path")
        if not lora_path or not str(lora_path).strip():
            _die(f"LoRA [{i}] requer campo 'path'.")
        lora_scale = float(entry.get("scale", 1.0))
        if lora_scale < 0.0 or lora_scale > 2.0:
            _die(
                f"LoRA [{i}] scale inválido: {lora_scale}. Deve estar entre 0.0 e 2.0."
            )
        loras.append({"path": str(lora_path).strip(), "scale": lora_scale})

    # --- custom_checkpoint_path + arch (D4) ---
    custom_checkpoint_path = gen_cfg.get("custom_checkpoint_path")
    arch = gen_cfg.get("arch")
    if custom_checkpoint_path:
        if (
            not isinstance(custom_checkpoint_path, str)
            or not custom_checkpoint_path.strip()
        ):
            _die("custom_checkpoint_path deve ser uma string não vazia.")
        custom_checkpoint_path = custom_checkpoint_path.strip()
        if not arch or str(arch).strip().lower() not in ("sdxl", "sd15"):
            _die("custom_checkpoint_path exige campo 'arch' válido ('sdxl' ou 'sd15').")
        arch = str(arch).strip().lower()
    else:
        custom_checkpoint_path = None
        arch = str(arch).strip().lower() if arch else None

    # --- base_model (XOR com custom_checkpoint_path) ---
    raw_base_model = gen_cfg.get("base_model") or cfg.get("model") or "flux-2-klein-4b"
    base_model = _canonical_model_name(str(raw_base_model))

    if custom_checkpoint_path and gen_cfg.get("base_model"):
        # XOR: quando custom está presente, base_model não deve ser especificado
        _die(
            "Campos 'custom_checkpoint_path' e 'base_model' são mutuamente exclusivos. "
            "Use apenas um deles."
        )

    if custom_checkpoint_path:
        # Para custom, aceita apenas sdxl/sd15
        if arch not in ("sdxl", "sd15"):
            _die(f"Arquitetura custom não suportada: {arch}. Use 'sdxl' ou 'sd15'.")
        base_model = arch  # custom força base_model = arch
    elif base_model not in ("flux-2-klein-4b", "sdxl", "sd15"):
        _die(f"Modelo base de difusão não suportado: {raw_base_model}")

    width = int(gen_cfg.get("width", 1024))
    height = int(gen_cfg.get("height", 1024))
    if width < 256 or width > 2048 or height < 256 or height > 2048:
        _die(f"Dimensões inválidas ({width}x{height}). Permitido entre 256 e 2048.")

    steps = int(gen_cfg.get("steps", 20))
    if steps < 1 or steps > 100:
        _die(f"Steps inválido: {steps}. Deve estar entre 1 e 100.")

    guidance_scale = float(
        gen_cfg.get("guidance_scale", 3.5 if "flux" in base_model else 7.0)
    )
    if guidance_scale < 1.0 or guidance_scale > 30.0:
        _die(f"Guidance scale inválido: {guidance_scale}. Deve estar entre 1.0 e 30.0.")

    quantization = str(gen_cfg.get("quantization", "4bit")).strip().lower()
    if quantization not in ("none", "4bit", "8bit"):
        _die(
            f"Nível de quantização inválido: {quantization}. Use 'none', '4bit' ou '8bit'."
        )

    # Seed: ausente será resolvido no loop (random base)
    seed_raw = gen_cfg.get("seed")
    seed = int(seed_raw) if seed_raw is not None else None

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
        "batch_size": batch_size,
        "loras": loras,
        "custom_checkpoint_path": custom_checkpoint_path,
        "arch": arch,
    }


# ---------------------------------------------------------------------------
# Legado: weights_path / lora_scale → loras[] (quando seção loras vazia)
# ---------------------------------------------------------------------------
def _resolve_loras_from_legacy(params: dict[str, Any]) -> list[dict[str, Any]]:
    """Converte weights_path+lora_scale legado para lista loras[] padrão.

    Quando a seção 'loras' já tem entradas, retorna como está.
    Caso contrário, mapeia weights_path → [{path, scale}].
    """
    loras = params.get("loras", [])
    if loras:
        return loras
    weights_path = params.get("weights_path")
    lora_scale = params.get("lora_scale", 1.0)
    if weights_path:
        return [{"path": weights_path, "scale": lora_scale}]
    return []


def _build_generation_meta(
    params: dict[str, Any],
    filename: str,
    thumb_filename: str,
    seed: int,
    batch_index: int,
    batch_size: int,
    loras_effective: list[dict[str, Any]],
) -> dict[str, Any]:
    """Constrói o dict de metadados para uma imagem do batch (JSONL)."""
    return {
        "filename": filename,
        "thumb_filename": thumb_filename,
        "seed": seed,
        "prompt": params["prompt"],
        "negative_prompt": params["negative_prompt"],
        "width": params["width"],
        "height": params["height"],
        "steps": params["steps"],
        "guidance_scale": params["guidance_scale"],
        "quantization": params["quantization"],
        "distilled": params["distilled"],
        "loras": loras_effective,
        "custom_model_path": params.get("custom_checkpoint_path"),
        "arch": params.get("arch"),
        "base_model": params["base_model"],
        "batch_index": batch_index,
        "batch_size": batch_size,
    }


# ---------------------------------------------------------------------------
# MOCK — geração sintética determinística
# ---------------------------------------------------------------------------
def _mock_generate(params: dict[str, Any], output_dir: Path, emitter=None) -> None:
    """Gera imagens de mock determinísticas com visual representativo, thumbs e meta JSONL."""
    from PIL import Image, ImageDraw

    if emitter is None:
        try:
            from trainer_difusao.telemetry import TelemetryEmitter
        except ImportError:
            from telemetry import TelemetryEmitter

        emitter = TelemetryEmitter(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    batch_size = params["batch_size"]
    seed_base = (
        params["seed"] if params["seed"] is not None else random.randint(0, 2**31 - 1)
    )
    loras_effective = _resolve_loras_from_legacy(params)
    custom_cp = params.get("custom_checkpoint_path")
    arch = params.get("arch")
    base_model = params["base_model"]

    emitter.emit(
        phase="preparing",
        message=f"Configurando pipeline Text-to-Image ({base_model})...",
        progress=0.05,
    )

    meta_lines: list[dict[str, Any]] = []

    for i in range(batch_size):
        # --- abort check ---
        if _is_cancelled(output_dir):
            print(f"[MOCK-GEN] Cancel detectado antes do item {i}. Saindo.", flush=True)
            break

        current_seed = seed_base + i
        h = hashlib.sha256(struct.pack("<q", current_seed)).digest()
        width = params["width"]
        height = params["height"]

        # Fases de telemetria por item
        progressPreparing = 0.05 + (0.4 * i / batch_size)
        emitter.emit(
            phase="preparing",
            message=f"Preparando imagem {i + 1}/{batch_size}...",
            progress=progressPreparing,
        )

        # Fundo com degradê escuro óptico determinístico baseado na seed
        r_base = 20 + (h[0] % 35)
        g_base = 15 + (h[1] % 30)
        b_base = 35 + (h[2] % 50)

        img = Image.new("RGB", (width, height), (r_base, g_base, b_base))
        draw = ImageDraw.Draw(img)

        progressGen = 0.5
        emitter.emit(
            phase="generating",
            message=f"Sintetizando imagem determinística ({params['steps']} passos, item {i + 1}/{batch_size})...",
            progress=progressGen,
            step=i,
            total_steps=batch_size,
        )

        # Desenho de círculos concêntricos e linhas geométricas simulando geração
        for ci in range(12):
            radius = int(min(width, height) * (0.08 * (ci + 1)))
            cx = int(width / 2 + ((h[ci % len(h)] - 128) / 256.0) * (width * 0.15))
            cy = int(
                height / 2 + ((h[(ci + 4) % len(h)] - 128) / 256.0) * (height * 0.15)
            )
            alpha_color = (
                min(255, r_base + ci * 15 + (h[ci] % 40)),
                min(255, g_base + ci * 10 + (h[(ci + 1) % len(h)] % 40)),
                min(255, b_base + ci * 18 + (h[(ci + 2) % len(h)] % 50)),
            )
            draw.ellipse(
                [cx - radius, cy - radius, cx + radius, cy + radius],
                outline=alpha_color,
                width=2,
            )

        # Card de informações e metadados no rodapé
        pad = 24
        card_h = 160
        card_box = [pad, height - card_h - pad, width - pad, height - pad]
        draw.rectangle(card_box, fill=(18, 18, 22), outline=(131, 80, 242), width=2)

        distilled = params.get("distilled", False)
        variant_label = "DESTILADO (4-8 steps)" if distilled else "BASE (20+ steps)"
        title_text = (
            f"HEPHAESTUS STUDIO · GERAÇÃO [{base_model.upper()} · {variant_label}]"
        )
        prompt_line = f"Prompt: {params['prompt'][:70]}{'...' if len(params['prompt']) > 70 else ''}"
        if params["negative_prompt"]:
            prompt_line += f" | Neg: {params['negative_prompt'][:30]}"
        meta_line1 = f"Seed: {current_seed} | Steps: {params['steps']} | CFG: {params['guidance_scale']} | Quant: {params['quantization']}"
        if loras_effective:
            lora_names = [Path(l["path"]).name for l in loras_effective]
            lora_label = f"LoRA(s): {', '.join(lora_names)}"
        elif params.get("weights_path"):
            lora_label = f"LoRA: {Path(params['weights_path']).name}"
        else:
            lora_label = "LoRA: Nenhum (Base Puro)"
        if custom_cp:
            lora_label += f" | Custom: {Path(custom_cp).name} ({arch})"
        meta_line2 = f"{lora_label} | Res: {width}x{height} | Mode: MOCK DETERMINÍSTICO"
        batch_line = f"Batch: {i + 1}/{batch_size} (index={i})"

        draw.text(
            (pad + 16, height - card_h - pad + 16), title_text, fill=(200, 180, 255)
        )
        draw.text(
            (pad + 16, height - card_h - pad + 48), prompt_line, fill=(255, 255, 255)
        )
        draw.text(
            (pad + 16, height - card_h - pad + 80), meta_line1, fill=(180, 180, 195)
        )
        draw.text(
            (pad + 16, height - card_h - pad + 108), meta_line2, fill=(140, 220, 160)
        )
        draw.text(
            (pad + 16, height - card_h - pad + 136), batch_line, fill=(160, 160, 180)
        )

        # Salvar imagem
        emitter.emit(
            phase="saving",
            message=f"Gravando imagem {i + 1}/{batch_size} no disco...",
            progress=0.9,
        )

        filename = f"generated_{i + 1:04d}.png"
        out_file = output_dir / filename
        img.save(out_file, "PNG")
        print(
            f"[MOCK-GEN] Imagem {i + 1}/{batch_size} gerada ({width}x{height}, seed={current_seed}): {out_file}",
            flush=True,
        )

        # Thumbnail
        thumb_filename = f"thumb_{i + 1:04d}.jpg"
        thumb_path = output_dir / thumb_filename
        _write_thumb(out_file, thumb_path)

        # Meta entry
        meta_entry = _build_generation_meta(
            params=params,
            filename=filename,
            thumb_filename=thumb_filename,
            seed=current_seed,
            batch_index=i,
            batch_size=batch_size,
            loras_effective=loras_effective,
        )
        meta_lines.append(meta_entry)

    # Retrocompat: symlink generated.png → generated_0001.png (batch=1)
    if batch_size == 1:
        legacy_png = output_dir / "generated.png"
        new_png = output_dir / "generated_0001.png"
        if not legacy_png.exists():
            try:
                legacy_png.symlink_to(new_png.name)
            except OSError:
                # Fallback: copiar arquivo
                import shutil

                shutil.copy2(new_png, legacy_png)

    # Salvar generation_meta.json (JSONL)
    meta_path = output_dir / "generation_meta.json"
    with open(meta_path, "w", encoding="utf-8") as f:
        f.writelines(
            json.dumps(entry, ensure_ascii=False) + "\n" for entry in meta_lines
        )
    print(
        f"[MOCK-GEN] Metadados salvos: {meta_path} ({len(meta_lines)} entradas)",
        flush=True,
    )

    emitter.emit(
        phase="completed",
        message=f"Geração concluída com sucesso! {len(meta_lines)}/{batch_size} imagens.",
        progress=1.0,
    )


# ---------------------------------------------------------------------------
# REAL — geração via Diffusers com aceleração CUDA
# ---------------------------------------------------------------------------
def _real_generate(params: dict[str, Any], output_dir: Path) -> None:
    """Executa a geração Text-to-Image real via Diffusers com aceleração CUDA.

    Suporta batch (loop sequencial), multi-LoRA (com fallback para Flux2 via peft),
    e checkpoints custom (SDXL/SD15 via from_single_file).
    """
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

    device = "cuda" if torch.cuda.is_available() else "cpu"

    # --- FASE: preparar pipeline ---
    emitter.emit(
        phase="preparing",
        message=f"Inicializando pipeline Text-to-Image ({base_model})...",
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

    # --- Configuração de quantização ---
    bnb_config = None
    if quant in ("4bit", "8bit") and device == "cuda":
        try:
            from transformers import BitsAndBytesConfig

            emitter.emit(
                phase="quantizing",
                message=f"Configurando quantização {quant} (BitsAndBytes)...",
                progress=0.15,
            )
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
            print(
                f"[WARN] Falha ao configurar BitsAndBytes: {e}. Usando precisão padrão.",
                flush=True,
            )

    # --- Carregar pipeline ---
    emitter.emit(
        phase="loading_model",
        message=f"Carregando pesos do modelo {base_model}...",
        progress=0.25,
    )

    pipe = None

    if custom_cp and arch in ("sdxl", "sd15"):
        # D4 — Modelo custom via from_single_file
        print(
            f"[DIFFUSION-GEN] Carregando checkpoint custom: {custom_cp} (arch={arch})",
            flush=True,
        )
        if arch == "sdxl":
            from diffusers import StableDiffusionXLPipeline

            load_kwargs: dict[str, Any] = {
                "torch_dtype": torch.float16 if device == "cuda" else torch.float32,
            }
            if bnb_config and device == "cuda":
                load_kwargs["quantization_config"] = bnb_config
            pipe = StableDiffusionXLPipeline.from_single_file(custom_cp, **load_kwargs)
        elif arch == "sd15":
            from diffusers import StableDiffusionPipeline

            load_kwargs_sd15: dict[str, Any] = {
                "torch_dtype": torch.float16 if device == "cuda" else torch.float32,
            }
            if bnb_config and device == "cuda":
                load_kwargs_sd15["quantization_config"] = bnb_config
            pipe = StableDiffusionPipeline.from_single_file(
                custom_cp, **load_kwargs_sd15
            )

        if pipe and device == "cuda" and not bnb_config:
            pipe.to(device)

    elif base_model == "flux-2-klein-4b":
        from diffusers import Flux2KleinPipeline

        model_repo = (
            (
                os.environ.get("FLUX_DISTILLED_MODEL_ID")
                or "black-forest-labs/FLUX.2-klein-4B"
            )
            if distilled
            else (
                os.environ.get("FLUX_MODEL_ID")
                or "black-forest-labs/FLUX.2-klein-base-4B"
            )
        )
        print(
            f"[DIFFUSION-GEN] Carregando FLUX.2 Klein 4B ({'Destilado' if distilled else 'Base'}): {model_repo}",
            flush=True,
        )
        pipe = Flux2KleinPipeline.from_pretrained(
            model_repo,
            torch_dtype=torch.bfloat16 if device == "cuda" else torch.float32,
        )
        if bnb_config is None and device == "cuda":
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

    else:
        _die(f"Modelo não suportado para geração real: {base_model}")

    # --- Multi-LoRA (D3) ---
    if loras_effective:
        emitter.emit(
            phase="injecting_lora",
            message=f"Carregando {len(loras_effective)} adaptador(es) LoRA...",
            progress=0.40,
        )
        # Flux2: load_lora_weights por adapter_name + peft set_adapters no transformer
        # SDXL/SD15: load_lora_weights + set_adapters canônico
        if base_model == "flux-2-klein-4b":
            # Flux2KleinPipeline não tem set_adapters — usar peft no transformer
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

            # Aplicar escala multi via peft no transformer
            # Ref: diffusers 0.40.0 loaders/peft.py:437 — PeftAdapterMixin
            # O mixin Flux2LoraLoaderMixin NÃO tem set_adapters (verificado no spike S1).
            # Fallback documentado: se transformer.set_adapters falhar, aplica só o primeiro LoRA.
            try:
                pipe.transformer.set_adapters(adapter_names, adapter_scales)
                print(
                    f"[DIFFUSION-GEN] Multi-LoRA aplicado via transformer.set_adapters: {adapter_names}",
                    flush=True,
                )
            except (AttributeError, RuntimeError, OSError) as exc:
                # Fallback: aplica só o primeiro LoRA (documentado no spike S1 do ADR-0023)
                print(
                    f"[DIFFUSION-GEN] [AVISO] transformer.set_adapters falhou ({exc}). "
                    f"Fallback: aplicando apenas o primeiro LoRA ({adapter_names[0]}). "
                    f"Ref: ADR-0023 spike S1.",
                    flush=True,
                )
                pipe.set_adapters([adapter_names[0]], [adapter_scales[0]])
        else:
            # SDXL / SD15 — set_adapters canônico do diffusers
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

    # --- LOOP DE BATCH ---
    seed_base = (
        params["seed"] if params["seed"] is not None else random.randint(0, 2**31 - 1)
    )
    meta_lines: list[dict[str, Any]] = []

    for i in range(batch_size):
        # --- abort check ---
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
            message=f"Executando amostragem de difusão (item {i + 1}/{batch_size}, seed={current_seed})...",
            progress=0.55 + (0.35 * i / batch_size),
            step=i,
            total_steps=batch_size,
        )
        print(
            f"[DIFFUSION-GEN] Gerando imagem {i + 1}/{batch_size} (seed={current_seed})...",
            flush=True,
        )

        # Inference
        if base_model == "flux-2-klein-4b":
            with torch.inference_mode():
                image = pipe(
                    prompt=prompt,
                    generator=generator,
                    num_inference_steps=steps,
                    guidance_scale=guidance,
                    width=width,
                    height=height,
                ).images[0]
        elif base_model in ("sdxl", "sd15"):
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
            _die(f"Modelo não suportado para inferência: {base_model}")

        # Salvar imagem
        emitter.emit(
            phase="saving",
            message=f"Salvando artefato de imagem {i + 1}/{batch_size}...",
            progress=0.92,
        )

        filename = f"generated_{i + 1:04d}.png"
        out_file = output_dir / filename
        image.save(out_file, "PNG")
        print(
            f"[DIFFUSION-GEN] Imagem {i + 1}/{batch_size} salva: {out_file}", flush=True
        )

        # Thumbnail
        thumb_filename = f"thumb_{i + 1:04d}.jpg"
        thumb_path = output_dir / thumb_filename
        _write_thumb(out_file, thumb_path)

        # Meta entry
        meta_entry = _build_generation_meta(
            params=params,
            filename=filename,
            thumb_filename=thumb_filename,
            seed=current_seed,
            batch_index=i,
            batch_size=batch_size,
            loras_effective=loras_effective,
        )
        meta_lines.append(meta_entry)

    # Retrocompat: symlink generated.png → generated_0001.png (batch=1)
    if batch_size == 1:
        legacy_png = output_dir / "generated.png"
        new_png = output_dir / "generated_0001.png"
        if not legacy_png.exists():
            try:
                legacy_png.symlink_to(new_png.name)
            except OSError:
                import shutil

                shutil.copy2(new_png, legacy_png)

    # Salvar generation_meta.json (JSONL)
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
        message=f"Geração finalizada com sucesso! {len(meta_lines)}/{batch_size} imagens.",
        progress=1.0,
    )


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------
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

    is_mock = os.environ.get("ENGINE_MOCK", "1") == "1"
    if is_mock:
        _mock_generate(params, output_dir)
    else:
        _real_generate(params, output_dir)
