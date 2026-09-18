"""
Geração sintética determinística de imagens para modo MOCK (CI e testes locais).
"""
from __future__ import annotations

import hashlib
import json
import os
import random
import shutil
import struct
from pathlib import Path
from typing import Any

from trainer_difusao.generation.artifacts import (
    _build_generation_meta,
    _is_cancelled,
    _png_info_for_generation,
    _write_thumb,
)
from trainer_difusao.generation.config import _resolve_loras_from_legacy
from trainer_difusao.generation.progress import _MOCK_TELEMETRY_SUBSTEPS, _sampler_progress


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
        if _is_cancelled(output_dir):
            print(f"[MOCK-GEN] Cancel detectado antes do item {i}. Saindo.", flush=True)
            break

        current_seed = seed_base + i
        h = hashlib.sha256(struct.pack("<q", current_seed)).digest()
        width = params["width"]
        height = params["height"]

        progressPreparing = 0.05 + (0.4 * i / batch_size)
        emitter.emit(
            phase="preparing",
            message=f"Preparando imagem {i + 1}/{batch_size}...",
            progress=progressPreparing,
        )

        r_base = 20 + (h[0] % 35)
        g_base = 15 + (h[1] % 30)
        b_base = 35 + (h[2] % 50)

        img = Image.new("RGB", (width, height), (r_base, g_base, b_base))
        if init_img_base is not None:
            try:
                img = Image.blend(img, init_img_base, 0.3)
            except Exception:
                pass
        draw = ImageDraw.Draw(img)

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

        emitter.emit(
            phase="saving",
            message=f"Gravando imagem {i + 1}/{batch_size} no disco...",
            progress=0.9,
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
        img.save(out_file, "PNG", pnginfo=_png_info_for_generation(meta_entry))
        print(
            f"[MOCK-GEN] Imagem {i + 1}/{batch_size} gerada ({width}x{height}, seed={current_seed}): {out_file}",
            flush=True,
        )

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
            with Image.open(out_file) as _up:
                _up.save(out_file, "PNG", pnginfo=_png_info_for_generation(meta_entry))
            print(
                f"[MOCK-GEN] Upscale mock x{upscale_scale}: {width}x{height} → "
                f"{up_w}x{up_h} ({out_file})",
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
                shutil.copy2(new_png, legacy_png)

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
