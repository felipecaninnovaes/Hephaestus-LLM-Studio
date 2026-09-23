"""
Validação e normalização de configurações de geração Text-to-Image.
"""
from __future__ import annotations

import os
from typing import Any

from trainer_difusao.common_pkg.core import _canonical_model_name, _die


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

    # --- custom_checkpoint_path + arch (D4; feat/pesos-custom-flux2: +flux-2) ---
    custom_checkpoint_path = gen_cfg.get("custom_checkpoint_path")
    arch = gen_cfg.get("arch")
    if custom_checkpoint_path:
        if (
            not isinstance(custom_checkpoint_path, str)
            or not custom_checkpoint_path.strip()
        ):
            _die("custom_checkpoint_path deve ser uma string não vazia.")
        custom_checkpoint_path = custom_checkpoint_path.strip()
        arch = str(arch).strip().lower() if arch else None
        if arch in ("flux", "flux2", "flux-2", "flux2-klein-4b", "flux.2-klein-4b"):
            arch = "flux-2-klein-4b"
        elif arch in ("qwen", "qwen-image", "qwen-image-2.1", "qwen2.1", "qwen_image", "qwen-image-2-1"):
            arch = "qwen-image-2.1"
        if arch not in ("sdxl", "sd15", "flux-2-klein-4b", "qwen-image-2.1"):
            _die(
                "custom_checkpoint_path exige campo 'arch' válido "
                "('sdxl', 'sd15', 'flux-2-klein-4b' ou 'qwen-image-2.1')."
            )
    else:
        custom_checkpoint_path = None
        arch = str(arch).strip().lower() if arch else None

    # --- text_encoder_path (feat/pesos-custom-flux2): override opcional do Qwen3 ---
    raw_encoder = gen_cfg.get("text_encoder_path")
    if not raw_encoder:
        text_encoder_path = None
    else:
        if not isinstance(raw_encoder, str) or not raw_encoder.strip():
            _die("text_encoder_path deve ser uma string não vazia.")
        text_encoder_path = raw_encoder.strip()

    # --- base_model (XOR com custom_checkpoint_path) ---
    raw_base_model = gen_cfg.get("base_model") or cfg.get("model") or "flux-2-klein-4b"
    base_model = _canonical_model_name(str(raw_base_model))

    if custom_checkpoint_path and gen_cfg.get("base_model"):
        _die(
            "Campos 'custom_checkpoint_path' e 'base_model' são mutuamente exclusivos. "
            "Use apenas um deles."
        )

    if custom_checkpoint_path:
        if arch not in ("sdxl", "sd15", "flux-2-klein-4b", "qwen-image-2.1"):
            _die(
                "Arquitetura custom não suportada: "
                f"{arch}. Use 'sdxl', 'sd15', 'flux-2-klein-4b' ou 'qwen-image-2.1'."
            )
        base_model = arch
    elif base_model not in ("flux-2-klein-4b", "sdxl", "sd15", "qwen-image-2.1"):
        _die(f"Modelo base de difusão não suportado: {raw_base_model}")

    width = int(gen_cfg.get("width", 1024))
    height = int(gen_cfg.get("height", 1024))
    if width < 256 or width > 2048 or height < 256 or height > 2048:
        _die(f"Dimensões inválidas ({width}x{height}). Permitido entre 256 e 2048.")

    steps = int(gen_cfg.get("steps", 20))
    if steps < 1 or steps > 100:
        _die(f"Steps inválido: {steps}. Deve estar entre 1 e 100.")

    guidance_scale = float(
        gen_cfg.get("guidance_scale", 3.5 if ("flux" in base_model or "qwen" in base_model) else 7.0)
    )
    if guidance_scale < 1.0 or guidance_scale > 30.0:
        _die(f"Guidance scale inválido: {guidance_scale}. Deve estar entre 1.0 e 30.0.")

    quantization = str(gen_cfg.get("quantization", "4bit")).strip().lower()
    if quantization not in ("none", "2bit", "4bit", "6bit", "8bit"):
        _die(
            f"Nível de quantização inválido: {quantization}. "
            "Use 'none', '2bit', '4bit', '6bit' ou '8bit'."
        )

    # --- sampler (fatia flux2-motor-treino): scheduler fresh-instance por request ---
    from trainer_difusao.schedulers import FLUX_SAMPLER_CHOICES, SAMPLER_CHOICES

    sampler = str(gen_cfg.get("sampler", "default")).strip().lower()
    if sampler not in SAMPLER_CHOICES:
        _die(f"Sampler inválido: {sampler}. Use: {', '.join(SAMPLER_CHOICES)}.")
    if base_model == "flux-2-klein-4b" and sampler not in FLUX_SAMPLER_CHOICES:
        _die(
            f"Sampler '{sampler}' incompatível com FLUX.2 (flow-match). "
            f"Modelo flux-2-klein-4b aceita apenas: {', '.join(FLUX_SAMPLER_CHOICES)}."
        )

    # --- upscale (fatia flux2-motor-treino): pós-passo Real-ESRGAN, fora do cache ---
    from trainer_difusao.upscale import UPSCALE_MODELS

    raw_upscale = gen_cfg.get("upscale")
    if raw_upscale is None:
        upscale = None
    else:
        if not isinstance(raw_upscale, dict):
            _die("Campo 'upscale' deve ser um objeto {model, scale} ou null.")
        upscale_model = str(raw_upscale.get("model", "4x") or "4x").strip() or "4x"
        if upscale_model not in UPSCALE_MODELS:
            _die(
                f"Modelo de upscale inválido: {upscale_model}. "
                f"Use: {', '.join(UPSCALE_MODELS)}."
            )
        try:
            upscale_scale = int(raw_upscale.get("scale"))
        except (TypeError, ValueError):
            _die(
                f"Escala de upscale inválida: {raw_upscale.get('scale')}. Use 2 ou 4."
            )
        if upscale_scale not in (2, 4):
            _die(f"Escala de upscale inválida: {upscale_scale}. Use 2 ou 4.")
        upscale = {"model": upscale_model, "scale": upscale_scale}

    seed_raw = gen_cfg.get("seed")
    seed = int(seed_raw) if seed_raw is not None else None

    lora_scale = float(gen_cfg.get("lora_scale", 1.0))
    if lora_scale < 0.0 or lora_scale > 2.0:
        _die(f"lora_scale inválido: {lora_scale}. Deve estar entre 0.0 e 2.0.")

    weights_path = cfg.get("weights_path") or gen_cfg.get("weights_path")
    negative_prompt = gen_cfg.get("negative_prompt") or ""
    distilled = bool(gen_cfg.get("distilled", False))

    # --- img2img: init_image_path + init_strength (S2 feat/img2img) ---
    raw_init_path = gen_cfg.get("init_image_path")
    raw_init_strength = gen_cfg.get("init_strength")
    if raw_init_path is None:
        if raw_init_strength is not None:
            _die("Campo 'init_strength' exige 'init_image_path' (img2img).")
        init_image_path = None
        init_strength = None
    else:
        if not isinstance(raw_init_path, str) or not raw_init_path.strip():
            _die("Campo 'init_image_path' deve ser uma string não vazia.")
        init_image_path = raw_init_path.strip()
        if not os.path.isfile(init_image_path):
            _die(f"init_image_path não encontrado: {init_image_path}")
        if raw_init_strength is None:
            init_strength = 0.6
        else:
            try:
                init_strength = float(raw_init_strength)
            except (TypeError, ValueError):
                _die(
                    f"init_strength inválido: {raw_init_strength}. "
                    "Deve ser float entre 0.05 e 0.95."
                )
            if init_strength < 0.05 or init_strength > 0.95:
                _die(
                    f"init_strength inválido: {init_strength}. "
                    "Deve estar entre 0.05 e 0.95."
                )

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
        "text_encoder_path": text_encoder_path,
        "init_image_path": init_image_path,
        "init_strength": init_strength,
        "sampler": sampler,
        "upscale": upscale,
    }


def _resolve_loras_from_legacy(params: dict[str, Any]) -> list[dict[str, Any]]:
    """Converte weights_path+lora_scale legado para lista loras[] padrão."""
    loras = params.get("loras", [])
    if loras:
        valid: list[dict[str, Any]] = []
        for i, entry in enumerate(loras):
            path = entry.get("path", "")
            if path and os.path.exists(path):
                valid.append(entry)
            else:
                print(
                    f"[DIFFUSION-GEN] lora[{i}] path não existe: {path} — descartando",
                    flush=True,
                )
        return valid

    weights_path = params.get("weights_path")
    lora_scale = params.get("lora_scale", 1.0)
    if weights_path:
        if os.path.exists(weights_path):
            return [{"path": weights_path, "scale": lora_scale}]
        print(
            f"[DIFFUSION-GEN] weights_path informado ({weights_path}) "
            f"mas arquivo não encontrado — geração com base puro",
            flush=True,
        )
    return []
