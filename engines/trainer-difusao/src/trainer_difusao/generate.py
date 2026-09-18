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
        if arch not in ("sdxl", "sd15", "flux-2-klein-4b"):
            _die(
                "custom_checkpoint_path exige campo 'arch' válido "
                "('sdxl', 'sd15' ou 'flux-2-klein-4b')."
            )
    else:
        custom_checkpoint_path = None
        arch = str(arch).strip().lower() if arch else None

    # --- text_encoder_path (feat/pesos-custom-flux2): override opcional do Qwen3 ---
    # Ausente/falsy = sem override (retrocompatível). Presente = string não vazia.
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
        # XOR: quando custom está presente, base_model não deve ser especificado
        _die(
            "Campos 'custom_checkpoint_path' e 'base_model' são mutuamente exclusivos. "
            "Use apenas um deles."
        )

    if custom_checkpoint_path:
        # Para custom, aceita sdxl/sd15 + flux-2-klein-4b (feat/pesos-custom-flux2)
        if arch not in ("sdxl", "sd15", "flux-2-klein-4b"):
            _die(
                "Arquitetura custom não suportada: "
                f"{arch}. Use 'sdxl', 'sd15' ou 'flux-2-klein-4b'."
            )
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

    # Seed: ausente será resolvido no loop (random base)
    seed_raw = gen_cfg.get("seed")
    seed = int(seed_raw) if seed_raw is not None else None

    lora_scale = float(gen_cfg.get("lora_scale", 1.0))
    if lora_scale < 0.0 or lora_scale > 2.0:
        _die(f"lora_scale inválido: {lora_scale}. Deve estar entre 0.0 e 2.0.")

    weights_path = cfg.get("weights_path") or gen_cfg.get("weights_path")
    negative_prompt = gen_cfg.get("negative_prompt") or ""
    distilled = bool(gen_cfg.get("distilled", False))

    # --- img2img: init_image_path + init_strength (S2 feat/img2img) ---
    # Ambos ausentes = txt2img puro (retrocompatível).
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


# ---------------------------------------------------------------------------
# Legado: weights_path / lora_scale → loras[] (quando seção loras vazia)
# ---------------------------------------------------------------------------
def _resolve_loras_from_legacy(params: dict[str, Any]) -> list[dict[str, Any]]:
    """Converte weights_path+lora_scale legado para lista loras[] padrão.

    Quando a seção 'loras' já tem entradas, valida cada path no disco:
    paths inexistentes são descartados com warning (não derruba o job).
    Caso contrário, mapeia weights_path → [{path, scale}] somente se
    o arquivo existir no disco (restaurando o guard do engine antigo).
    """
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

def pipeline_cache_key(params: dict[str, Any]) -> tuple:
    """Extrai chave de cache do pipeline a partir dos params validados.

    Chave: (base_model|custom_checkpoint_path+arch, quantization, distilled,
    text_encoder_path). O encoder entra na chave: trocar o encoder troca os
    pesos de texto — reusar o pipeline cacheado seria fallback silencioso.
    """
    custom_cp = params.get("custom_checkpoint_path")
    return (
        custom_cp or params.get("base_model"),
        params.get("quantization"),
        params.get("distilled", False),
        params.get("text_encoder_path"),
    )

def ensure_pipeline(
    params: dict[str, Any], cache: dict[tuple, object]
) -> tuple[object | None, tuple]:
    """Verifica cache de pipeline e devolve (pipeline|None, key).

    Se cache hit → (pipeline_obj, key).
    Se cache miss → (None, key) — caller deve chamar _real_generate sem pipeline.
    """
    key = pipeline_cache_key(params)
    if key in cache:
        print(f"[DIFFUSION-GEN] Cache hit para spec {key}.", flush=True)
        return cache[key], key
    return None, key


HEPHAESTUS_GENERATION_PNG_KEY = "hephaestus.generation"

# Chaves de _build_generation_meta redundantes no PNG (nome do arquivo local).
_PNG_EXCLUDED_META_KEYS = frozenset({"filename", "thumb_filename", "batch_index"})


def _png_payload_for_generation(meta: dict[str, Any]) -> dict[str, Any]:
    """Deriva o payload JSON embarcado no PNG a partir do dict do JSONL.

    Mesma origem (`meta`) usada em `generation_meta.json`; apenas remove as
    chaves redundantes de arquivo local. Sem limite artificial de tamanho —
    prompts (incl. negativo) são gravados fiéis, UTF-8.
    """
    return {k: v for k, v in meta.items() if k not in _PNG_EXCLUDED_META_KEYS}


def _png_info_for_generation(meta: dict[str, Any]):
    """Serializa os campos de geração em chunk iTXt do PNG.

    Chave única `hephaestus.generation` com JSON compacto (sort_keys para
    determinismo). Usa `add_itxt` para suportar prompts UTF-8/com acentos
    (tEXt é latin-1 por spec).
    """
    from PIL.PngImagePlugin import PngInfo

    payload = _png_payload_for_generation(meta)
    text = json.dumps(payload, ensure_ascii=False, separators=(",", ":"), sort_keys=True)
    info = PngInfo()
    info.add_itxt(
        HEPHAESTUS_GENERATION_PNG_KEY, text, lang="en", tkey=HEPHAESTUS_GENERATION_PNG_KEY
    )
    return info

def _build_generation_meta(
    params: dict[str, Any],
    filename: str,
    thumb_filename: str,
    seed: int,
    batch_index: int,
    batch_size: int,
    loras_effective: list[dict[str, Any]],
    upscale_info: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Constrói o dict de metadados para uma imagem do batch (JSONL)."""
    meta: dict[str, Any] = {
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
        "text_encoder_path": params.get("text_encoder_path"),
        "base_model": params["base_model"],
        "batch_index": batch_index,
        "batch_size": batch_size,
        "job_id": params.get("job_id"),
        "sampler": params.get("sampler", "default"),
    }
    # upscale (fatia flux2-motor-treino): só após o pós-passo aplicar —
    # txt2img sem upscale nunca carrega a chave (round-trip intacto).
    if upscale_info is not None:
        meta["upscale"] = upscale_info
    # img2img (S2 feat/img2img): campos aditivos, só quando init presente —
    # txt2img puro nunca carrega essas chaves (round-trip existente intacto).
    if params.get("init_image_path"):
        meta["init_image"] = os.path.basename(params["init_image_path"])
        meta["init_strength"] = params.get("init_strength")
    return meta


# ---------------------------------------------------------------------------
# Telemetria fina de geração (bug 004 — barra "congelada" entre imagens).
#
# O intervalo de progresso da fase `generating` é [0.55, 0.90]: cada imagem
# `i` do batch ocupa a fatia [0.55+0.35*i/B, 0.55+0.35*(i+1)/B]. Os helpers
# abaixo interpolam os sampler steps dentro da fatia da imagem atual.
# `step`/`total_steps` da telemetria continuam sendo o contador de IMAGENS
# (i/B) — os steps do sampler vão apenas na mensagem.
# ---------------------------------------------------------------------------
_GENERATE_PROGRESS_BASE = 0.55
_GENERATE_PROGRESS_SPAN = 0.35
# Throttle: no máximo ~50 eventos por imagem → delta mínimo de 0.01
# (floor(progress*1000) avança ≥10 permilagem) entre emissões.
_SAMPLER_EMIT_MIN_DELTA_PERMILLE = 10
# Subdivisões do mock (sem sleep adicional): 3-5 incrementos por imagem.
_MOCK_TELEMETRY_SUBSTEPS = 4


def _sampler_progress(
    image_index: int, batch_size: int, sampler_step: int, num_sampler_steps: int
) -> float:
    """Progresso interpolado do sampler step dentro da fatia da imagem atual.

    `sampler_step` é 0-indexed (0..num_sampler_steps-1). Fórmula:
    `0.55 + 0.35*(i + (s+1)/num_steps)/batch_size`.
    """
    batch = max(1, int(batch_size))
    total = max(1, int(num_sampler_steps))
    frac = (int(sampler_step) + 1) / total
    frac = max(0.0, min(1.0, frac))
    return _GENERATE_PROGRESS_BASE + _GENERATE_PROGRESS_SPAN * (int(image_index) + frac) / batch


def _should_emit(
    prev_progress: float | None,
    cur_progress: float,
    *,
    is_first: bool = False,
    is_last: bool = False,
) -> bool:
    """Throttle do callback do sampler: no máximo ~50 eventos por imagem.

    Emite sempre no primeiro e no último step; nos demais, só quando
    floor(progress*1000) avança ≥10 (delta ≥ 0.01). Aceita chamada com 2
    args (`_should_emit(prev, cur)`); `prev=None` (nada emitido ainda)
    sempre emite.
    """
    if is_first or is_last:
        return True
    if prev_progress is None:
        return True
    try:
        return (
            int(float(cur_progress) * 1000) - int(float(prev_progress) * 1000)
        ) >= _SAMPLER_EMIT_MIN_DELTA_PERMILLE
    except (TypeError, ValueError):
        return True


def _make_sampler_callback(
    emitter: object, image_index: int, batch_size: int, num_sampler_steps: int
):
    """Constrói `callback_on_step_end` do diffusers p/ a imagem atual.

    Assinatura diffusers: `cb(pipe, step, timestep, callback_kwargs) -> dict`.
    NEVER quebra a geração: qualquer falha de telemetria é engolida e o
    `callback_kwargs` original é devolvido intacto.
    """
    total = max(1, int(num_sampler_steps))
    state: dict[str, float | None] = {"prev": None}

    def _callback(pipe: object, step: int, timestep: object, callback_kwargs: dict | None = None):
        try:
            s = int(step)
            progress = _sampler_progress(image_index, batch_size, s, total)
            if _should_emit(
                state["prev"], progress, is_first=(s <= 0), is_last=(s >= total - 1)
            ):
                emitter.emit(  # type: ignore[attr-defined]
                    phase="generating",
                    message=f"Imagem {int(image_index) + 1}/{int(batch_size)} · step {s + 1}/{total}",
                    progress=progress,
                    step=int(image_index),
                    total_steps=int(batch_size),
                )
                state["prev"] = progress
        except Exception:
            pass
        return callback_kwargs if callback_kwargs is not None else {}

    return _callback


def _pipe_call_kwargs_with_callback(
    emitter: object, image_index: int, batch_size: int, num_sampler_steps: int
) -> dict[str, object]:
    """Kwargs de callback p/ `pipe(...)`; `{}` (no-op) se construção falhar."""
    try:
        return {
            "callback_on_step_end": _make_sampler_callback(
                emitter, image_index, batch_size, num_sampler_steps
            ),
            "callback_on_step_end_tensor_inputs": ["latents"],
        }
    except Exception:
        return {}


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

    # img2img (S2 feat/img2img): tenta abrir a init como base do desenho
    # sintético. Falha de decode NÃO derruba o job — cai para mock puro.
    init_img_base = None
    init_image_path = params.get("init_image_path")
    if init_image_path:
        try:
            with Image.open(init_image_path) as _init:
                init_img_base = (
                    _init.convert("RGB").resize(
                        (params["width"], params["height"]), Image.LANCZOS
                    )
                )
        except Exception as exc:
            print(
                f"[MOCK-GEN] [AVISO] Falha ao abrir init_image ({init_image_path}): "
                f"{exc}. Seguindo com mock puro.",
                flush=True,
            )
            init_img_base = None

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
        if init_img_base is not None:
            # Mock honesto: blend da init (stretch exato, LANCZOS) com a
            # textura sintética (alpha 0.3) — visualmente distinto do txt2img.
            try:
                img = Image.blend(img, init_img_base, 0.3)
            except Exception:
                pass
        draw = ImageDraw.Draw(img)

        progressGen = 0.5
        # Mock simula o formato do path real: N incrementos interpolados
        # dentro da fatia da imagem atual (sem sleep adicional).
        for k in range(_MOCK_TELEMETRY_SUBSTEPS):
            emitter.emit(
                phase="generating",
                message=(
                    f"Imagem {i + 1}/{batch_size} · step {k + 1}/{_MOCK_TELEMETRY_SUBSTEPS} "
                    f"(mock, {params['steps']} sampler steps, item {i + 1}/{batch_size})..."
                ),
                progress=_sampler_progress(i, batch_size, k, _MOCK_TELEMETRY_SUBSTEPS),
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
        elif params.get("weights_path") and os.path.exists(params["weights_path"]):
            lora_label = f"LoRA: {Path(params['weights_path']).name}"
        else:
            lora_label = "LoRA: Nenhum (Base Puro)"
        if custom_cp:
            lora_label += f" | Custom: {Path(custom_cp).name} ({arch})"
        if params.get("text_encoder_path"):
            lora_label += f" | Encoder: {Path(str(params['text_encoder_path'])).name}"
        meta_line2 = f"{lora_label} | Res: {width}x{height} | Mode: MOCK DETERMINÍSTICO"
        if init_image_path:
            meta_line2 += f" | IMG2IMG: {Path(init_image_path).name}@{params.get('init_strength')}"
        sampler = params.get("sampler", "default")
        upscale_cfg = params.get("upscale")
        if sampler and sampler != "default":
            meta_line2 += f" | Sampler: {sampler}"
        if upscale_cfg:
            meta_line2 += f" | UPSCALE: {upscale_cfg['model']} x{upscale_cfg['scale']}"
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
        thumb_filename = f"thumb_{i + 1:04d}.jpg"
        # Meta entry (mesma origem do JSONL) — PNG embarca o mesmo dict.
        meta_entry = _build_generation_meta(
            params=params,
            filename=filename,
            thumb_filename=thumb_filename,
            seed=current_seed,
            batch_index=i,
            batch_size=batch_size,
            loras_effective=loras_effective,
        )
        img.save(out_file, "PNG", pnginfo=_png_info_for_generation(meta_entry))
        print(
            f"[MOCK-GEN] Imagem {i + 1}/{batch_size} gerada ({width}x{height}, seed={current_seed}): {out_file}",
            flush=True,
        )

        # Upscale mock (fatia flux2-motor-treino): PIL LANCZOS no fator
        # pedido + mesmos campos de meta do path real. Thumb reflete o final.
        if upscale_cfg:
            upscale_scale = int(upscale_cfg["scale"])
            upscale_model = str(upscale_cfg["model"])
            up_w, up_h = width * upscale_scale, height * upscale_scale
            with Image.open(out_file) as _saved:
                _saved.convert("RGB").resize(
                    (up_w, up_h), Image.LANCZOS
                ).save(out_file, "PNG")
            meta_entry["upscale"] = {
                "model": upscale_model,
                "scale": upscale_scale,
                "original_width": width,
                "original_height": height,
                "final_width": up_w,
                "final_height": up_h,
            }
            # Re-salva o PNG para embarcar o meta final (com upscale) no iTXt.
            with Image.open(out_file) as _up:
                _up.save(out_file, "PNG", pnginfo=_png_info_for_generation(meta_entry))
            print(
                f"[MOCK-GEN] Upscale mock x{upscale_scale}: {width}x{height} → "
                f"{up_w}x{up_h} ({out_file})",
                flush=True,
            )

        # Thumbnail
        thumb_path = output_dir / thumb_filename
        _write_thumb(out_file, thumb_path)

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
    """Resolve (merged_dir, md5_16) do cache de merge p/ um encoder solto.

    Delega p/ o helper compartilhado em trainer_difusao.common (mesmo padrão
    usado no treino flux.py). Re-exportado aqui p/ testes via generate.
    """
    from trainer_difusao.common import _custom_text_encoder_merge_dir

    return _custom_text_encoder_merge_dir(encoder_path)


def _load_flux2_text_encoder_override(
    encoder_path: str,
    model_repo: str,
    pipe_dtype: Any,
    quantization_config: Any = None,
) -> tuple[Any, Any]:
    """Carrega encoder/tokenizer override p/ FLUX.2 Klein (Qwen3).

    Dir com config.json → encoder+tokenizer do próprio path (modelo HF
    completo). Arquivo .safetensors solto → config/tokenizer do repo BFL e
    state_dict do arquivo aplicado sobre o encoder do repo (falha honesta se
    o layout não for reconhecido — nunca fallback silencioso p/ o oficial).

    *quantization_config* (transformers.BitsAndBytesConfig/TorchAoConfig) é
    aplicado via from_pretrained no caso dir. No caso arquivo-solto COM
    quantização, os pesos bf16 são mesclados UMA vez sobre o encoder do repo
    e persistidos no cache de merge
    ($TEXT_ENCODER_CUSTOM_CACHE/<md5>-<slug>/merged); a quantização é então
    aplicada por carga sobre o merged (mesma semântica do encoder default).
    Sem quantization_config, o comportamento atual é preservado (bf16 direto,
    sem cache).
    """
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
    """Aplica o state_dict solto sobre o encoder do repo (bf16, sem quant).

    Erro honesto citando as primeiras chaves divergentes — nunca fallback
    silencioso p/ o encoder oficial.
    """
    try:
        missing, unexpected = base_encoder.load_state_dict(state, strict=False)
    except Exception as exc:
        _die(
            f"Falha ao aplicar text_encoder custom ({encoder_path}) sobre o "
            f"encoder do repo ({model_repo}): layout não reconhecido ({exc})"
        )
    if missing or unexpected:
        missing = list(missing or [])
        unexpected = list(unexpected or [])
        _die(
            f"text_encoder custom ({encoder_path}) com layout não reconhecido: "
            f"{len(missing)} chave(s) ausente(s) {missing[:5]}, "
            f"{len(unexpected)} inesperada(s) {unexpected[:5]}. "
            "Envie o encoder como diretório HF completo ou um .safetensors "
            "compatível com o Qwen3 do FLUX.2 Klein."
        )


def _load_flux2_loose_encoder_merged(
    encoder_path: str,
    model_repo: str,
    pipe_dtype: Any,
    quantization_config: Any,
) -> tuple[Any, Any]:
    """Arquivo solto + quantização via cache de merge (bf16 mesclado em disco).

    O custo do merge é pago 1x por encoder; a quantização é aplicada por
    carga sobre o merged (mesma semântica do encoder default). O
    ``pipeline_cache_key`` NÃO muda — o merged é derivado do path, que já
    está na chave.
    """
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
    """Carrega transformer custom flux-2 do arquivo (falha honesta se layout inválido).

    *quantization_config* é repassado a `Flux2Transformer2DModel.from_single_file`
    (suportado em diffusers >= 0.40 via kwargs → DiffusersAutoQuantizer).
    """
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


# ---------------------------------------------------------------------------
# REAL — geração via Diffusers com aceleração CUDA
# ---------------------------------------------------------------------------
def _real_generate(
    params: dict[str, Any], output_dir: Path, pipeline: object | None = None
) -> object | None:
    """Executa a geração Text-to-Image real via Diffusers com aceleração CUDA.

    Suporta batch (loop sequencial), multi-LoRA (com fallback para Flux2 via peft),
    checkpoints custom (SDXL/SD15 via from_single_file) e img2img opcional via
    init_image_path/init_strength (SD: variante Img2Img leve dos componentes do
    pipe cacheado; Flux2Klein: image= nativo, sem strength na assinatura).

    Se *pipeline* for fornecido (cache hit), pula a fase de carregamento e usa o
    pipeline diretamente — caso contrário, carrega como antes (cache miss).
    Retorna o pipeline carregado (para caching pelo caller).
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
    text_encoder_path = params.get("text_encoder_path")
    # N2: override de text_encoder só é suportado no fluxo flux-2. Para
    # sdxl/sd15, falha honesta em vez de ignorar silenciosamente (BFF/manager
    # já bloqueiam; defesa em profundidade no engine).
    if text_encoder_path and arch in ("sdxl", "sd15"):
        _die(
            f"text_encoder_path ({text_encoder_path}) não é suportado com "
            f"checkpoint custom sdxl/sd15: o override de encoder é exclusivo "
            f"do fluxo flux-2-klein-4b. Remova text_encoder_path ou use "
            f"arch=flux-2-klein-4b."
        )

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
    # 4bit/8bit: BitsAndBytes (comportamento legado); 2bit/6bit: TorchAO
    # (honesto: ImportError/erro de build → _die, nunca silencioso).
    # Toda quantização exige cuda — em cpu o job falha com mensagem clara.
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
        # device já garantido cuda acima.
        from trainer_difusao.quantization import build_torchao_config

        emitter.emit(
            phase="quantizing",
            message=f"Configurando quantização {quant} (TorchAO)...",
            progress=0.15,
        )
        quantization_config = build_torchao_config(quant)

    # --- Carregar pipeline (ou usar cache) ---
    if pipeline is not None:
        # Cache hit: pipeline fornecido pelo caller (serve.py)
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
            # D4 — Modelo custom via from_single_file
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
            # Encoder override: text_encoder=/tokenizer= no from_pretrained.
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
            # Checkpoint flux-2 custom: transformer do arquivo + resto do repo BFL.
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
            # Fallback documentado: se transformer.set_adapters falhar, aplica só 1 LoRA.
            try:
                pipe.transformer.set_adapters(adapter_names, adapter_scales)
                print(
                    f"[DIFFUSION-GEN] Multi-LoRA aplicado via transformer.set_adapters: "
                    f"{adapter_names}",
                    flush=True,
                )
            except (AttributeError, RuntimeError, OSError) as exc:
                # Fallback: aplica só o primeiro LoRA via transformer (peft)
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
                    # Último recurso: decaimento honesto — sem set_adapters
                    print(
                        f"[DIFFUSION-GEN] [ERRO] transformer.set_adapters(1 LoRA) "
                        f"também falhou ({exc2}). LoRA não aplicada.",
                        flush=True,
                    )
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

    # --- img2img (S2 feat/img2img): variante leve + init pré-carregada ---
    # A variante é construída DEPOIS do LoRA (herda unet/transformer com os
    # adaptadores) a partir de `pipe.components` — compartilha os módulos,
    # sem recarregar pesos e sem poluir o pipeline cacheado. O cache key da
    # spec (pipeline_cache_key) NÃO muda.
    init_image_path = params.get("init_image_path")
    init_strength = params.get("init_strength")
    is_img2img = bool(init_image_path)
    init_image = None
    call_pipe = pipe
    if is_img2img:
        if base_model == "flux-2-klein-4b":
            # Flux2KleinPipeline.__call__ (diffusers 0.40.0, verificado via
            # inspect) já aceita `image=` nativo (condicionamento estilo
            # Kontext) e NÃO possui `strength` nem classe Img2Img dedicada —
            # usa o próprio pipe cacheado; strength fica só no meta.
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
                # Resize exato (width,height), LANCZOS — stretch documentado.
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

    # --- sampler (fatia flux2-motor-treino): fresh-instance por request + restore ---
    # O pipeline vem do cache do daemon e é reusado entre requests: trocar o
    # scheduler SEM restore contaminaria o próximo request. `swapped_scheduler`
    # restaura no finally (inclusive em exceção/cancel). Scheduler NUNCA entra
    # no pipeline_cache_key nem na spec do daemon (não altera pesos).
    # A variante img2img (`pipe.components`) compartilha o MESMO objeto
    # scheduler — restaurar o `call_pipe` restaura o cacheado também.
    from trainer_difusao.schedulers import build_scheduler, swapped_scheduler

    sampler_name = params.get("sampler", "default")
    upscale_cfg = params.get("upscale")
    sched_arch = (
        "flux" if base_model == "flux-2-klein-4b" else "sd"
    )
    if base_model not in ("flux-2-klein-4b", "sdxl", "sd15"):
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

    # --- LOOP DE BATCH ---
    seed_base = (
        params["seed"] if params["seed"] is not None else random.randint(0, 2**31 - 1)
    )
    meta_lines: list[dict[str, Any]] = []

    with swapped_scheduler(call_pipe, fresh_scheduler):
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

            # Inference (com callback de progresso do sampler; fallback sem
            # callback se a pipeline — ex. flux/distilled — usar API diferente).
            # Telemetria NEVER quebra a geração: TypeError → retry sem callback.
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
                    # Flux2Klein: image= nativo; sem strength na assinatura.
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
                        # Img2Img aceita negative_prompt; kwargs demais idênticos.
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
            thumb_filename = f"thumb_{i + 1:04d}.jpg"
            # Meta entry (mesma origem do JSONL) — PNG embarca o mesmo dict.
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

            # Upscale Real-ESRGAN (fatia flux2-motor-treino): pós-passo sobre o
            # PNG salvo (re-salva o mesmo arquivo). Falha → _die honesto.
            # Thumb e meta refletem a imagem final.
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
                # Re-salva o PNG para embarcar o meta final (com upscale) no iTXt.
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

            # Thumbnail
            thumb_path = output_dir / thumb_filename
            _write_thumb(out_file, thumb_path)

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

    return pipe


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
