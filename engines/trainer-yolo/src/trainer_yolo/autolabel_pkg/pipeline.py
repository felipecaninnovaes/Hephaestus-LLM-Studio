"""
Pipeline de execução de AutoLabel v2 (Florence-2, Qwen2-VL, OpenAI, Mock) e CLI.
"""
from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import os
import sys
import threading
import time
from pathlib import Path
from typing import Any

from engine_kit.runtime import die as _die
from engine_kit.telemetry import TelemetryEmitter
from trainer_yolo.autolabel_pkg.captions import (
    MOCK_DESCRIPTIONS,
    _generate_caption_florence,
    _generate_caption_mock,
    _generate_caption_qwen,
)
from trainer_yolo.autolabel_pkg.vision_api import _call_openai_vision_api
from trainer_yolo.config import load_and_validate_autolabel_config
from trainer_yolo.dataset import _discover_dataset_images


def _autolabel_pipeline(cfg: dict, output_dir: Path) -> None:
    """Executa o pipeline do AutoLabel v2 (Florence-2, Qwen2-VL, OpenAI, Mock)."""
    output_dir.mkdir(parents=True, exist_ok=True)
    dataset_path = Path(cfg["dataset_path"])
    seed = int(cfg.get("seed", 42))
    model = cfg.get("model", "mock")

    al_section = cfg.get("autolabel", {})
    prompt = None
    api_key = None
    api_base = "https://api.openai.com/v1"
    openai_model = "gpt-4o-mini"
    reasoning_effort = None

    if isinstance(al_section, dict):
        prompt = al_section.get("prompt")
        raw_key = al_section.get("api_key") or os.environ.get("OPENAI_API_KEY")
        if raw_key:
            api_key = str(raw_key).strip().strip("\"'")
        if al_section.get("api_base"):
            api_base = str(al_section["api_base"]).strip().strip("\"'").rstrip("/")
        if al_section.get("openai_model"):
            openai_model = str(al_section["openai_model"]).strip().strip("\"'")
        if al_section.get("reasoning_effort"):
            reasoning_effort = str(al_section["reasoning_effort"]).strip().strip("\"'")

    image_map = _discover_dataset_images(dataset_path)
    sorted_filenames = sorted(image_map.keys())
    total_imgs = len(sorted_filenames)

    captions_path = output_dir / "captions.jsonl"

    emitter = TelemetryEmitter(output_dir)
    emitter.emit(
        phase="preparing",
        message=f"Iniciando AutoLabel com modelo {model} ({total_imgs} imagens)...",
        progress=0.05,
    )

    step_time_ema: float | None = None

    if model == "openai":
        raw_concurrency = os.environ.get("AUTOLABEL_CONCURRENCY", "4")
        try:
            concurrency = max(1, int(raw_concurrency))
        except ValueError:
            concurrency = 4

        is_official = "api.openai.com" in api_base
        if is_official and not (api_key and api_key.strip()):
            if os.environ.get("AUTOLABEL_TEST_MOCK_FALLBACK") == "1":
                pass
            else:
                _die(
                    "O modelo 'openai' com endpoint oficial requer uma API Key válida. "
                    "Forneça a apiKey na requisição ou configure a variável OPENAI_API_KEY no nó."
                )

        def _label_worker(fname: str) -> tuple[str, str, float]:
            t_start = time.monotonic()
            img_path = image_map[fname]
            if is_official and not (api_key and api_key.strip()):
                caption = _generate_caption_mock(seed, fname, prompt)
                return fname, caption, max(0.0, time.monotonic() - t_start)

            try:
                caption = _call_openai_vision_api(
                    img_path,
                    prompt,
                    api_key,
                    api_base,
                    openai_model,
                    reasoning_effort=reasoning_effort,
                )
            except (RuntimeError, ValueError, OSError) as exc:
                if os.environ.get("AUTOLABEL_TEST_MOCK_FALLBACK") == "1":
                    print(
                        f"[autolabel-openai] Fallback ativado para teste: {exc}",
                        file=sys.stderr,
                    )
                    h = int(
                        hashlib.sha256(fname.encode("utf-8")).hexdigest(),
                        16,
                    )
                    fallback_desc = MOCK_DESCRIPTIONS[
                        h % len(MOCK_DESCRIPTIONS)
                    ]
                    caption = f"{prompt or 'Desc'} — [OpenAI fallback]: {fallback_desc}"
                else:
                    raise exc
            return fname, caption, max(0.0, time.monotonic() - t_start)
        executor = concurrent.futures.ThreadPoolExecutor(max_workers=concurrency)
        futures: dict[str, concurrent.futures.Future] = {}
        remaining_to_submit = list(sorted_filenames)

        completed_results: dict[str, str] = {}
        step_num = 0
        last_completion_time = time.monotonic()

        with open(captions_path, "w", encoding="utf-8") as f:
            try:
                # Processa a primeira imagem sequencialmente se ainda não submetida
                # para garantir validação inicial de credenciais/endpoint e progresso imediato
                if remaining_to_submit:
                    first_fname = remaining_to_submit.pop(0)
                    try:
                        fname, caption, _ = _label_worker(first_fname)
                    except Exception as exc:
                        _die(f"AutoLabel OpenAI falhou na imagem '{first_fname}': {exc}")

                    completed_results[fname] = caption
                    step_num += 1
                    now = time.monotonic()
                    dt_completion = max(0.0, now - last_completion_time)
                    last_completion_time = now
                    step_time_ema = dt_completion
                    remaining_imgs = total_imgs - step_num
                    eta_s = int(round(step_time_ema * remaining_imgs)) if remaining_imgs > 0 else 0
                    progress = 0.05 + 0.94 * (step_num / total_imgs)
                    emitter.emit(
                        phase="labeling",
                        message=f"Anotando imagem {step_num}/{total_imgs}: {fname}",
                        progress=progress,
                        step=step_num,
                        total_steps=total_imgs,
                        step_time_seconds=round(step_time_ema, 4),
                        eta_seconds=eta_s,
                    )

                # Para as imagens restantes, executa no pool com concorrência configurada
                for fname_expected in list(remaining_to_submit):
                    while len(futures) < concurrency and remaining_to_submit:
                        fname_next = remaining_to_submit.pop(0)
                        futures[fname_next] = executor.submit(_label_worker, fname_next)

                    fut = futures.pop(fname_expected)
                    try:
                        fname, caption, _ = fut.result()
                    except Exception as exc:
                        executor.shutdown(wait=False, cancel_futures=True)
                        _die(f"AutoLabel OpenAI falhou na imagem '{fname_expected}': {exc}")

                    completed_results[fname] = caption
                    step_num += 1
                    now = time.monotonic()
                    dt_completion = max(0.0, now - last_completion_time)
                    last_completion_time = now

                    if step_time_ema is None:
                        step_time_ema = dt_completion
                    else:
                        step_time_ema = 0.2 * dt_completion + 0.8 * step_time_ema

                    remaining_imgs = total_imgs - step_num
                    eta_s = int(round(step_time_ema * remaining_imgs)) if remaining_imgs > 0 else 0
                    progress = 0.05 + 0.94 * (step_num / total_imgs)

                    emitter.emit(
                        phase="labeling",
                        message=f"Anotando imagem {step_num}/{total_imgs}: {fname}",
                        progress=progress,
                        step=step_num,
                        total_steps=total_imgs,
                        step_time_seconds=round(step_time_ema, 4),
                        eta_seconds=eta_s,
                    )
            finally:
                executor.shutdown(wait=False, cancel_futures=True)

            for fname in sorted_filenames:
                if fname in completed_results:
                    line = json.dumps(
                        {"filename": fname, "caption": completed_results[fname]},
                        ensure_ascii=False,
                    )
                    f.write(line + "\n")
            f.flush()
    else:
        with open(captions_path, "w", encoding="utf-8") as f:
            for idx, fname in enumerate(sorted_filenames):
                t0 = time.monotonic()
                if model == "florence-2":
                    caption = _generate_caption_florence(seed, fname, prompt)
                elif model == "qwen2-vl":
                    caption = _generate_caption_qwen(seed, fname, prompt)
                else:
                    caption = _generate_caption_mock(seed, fname, prompt)

                line = json.dumps(
                    {"filename": fname, "caption": caption}, ensure_ascii=False
                )
                f.write(line + "\n")
                f.flush()

                elapsed_img = max(0.0, time.monotonic() - t0)
                if step_time_ema is None:
                    step_time_ema = elapsed_img
                else:
                    step_time_ema = 0.2 * elapsed_img + 0.8 * step_time_ema

                step_num = idx + 1
                remaining_imgs = total_imgs - step_num
                eta_s = int(round(step_time_ema * remaining_imgs)) if remaining_imgs > 0 else 0

                progress = 0.05 + 0.94 * (step_num / total_imgs)
                emitter.emit(
                    phase="labeling",
                    message=f"Anotando imagem {step_num}/{total_imgs}: {fname}",
                    progress=progress,
                    step=step_num,
                    total_steps=total_imgs,
                    step_time_seconds=round(step_time_ema, 4),
                    eta_seconds=eta_s,
                )
    emitter.emit(
        phase="completed",
        message=f"AutoLabel concluído: {total_imgs} imagens anotadas com sucesso.",
        progress=1.0,
    )


def cmd_autolabel(args: list[str]) -> None:
    """Subcomando autolabel."""
    parser = argparse.ArgumentParser(
        prog="trainer-yolo autolabel",
        description="AutoLabel v2 — Geração de legendas em lote com VLM e OpenAI API",
    )
    parser.add_argument("--config", required=True, help="Path to config.yaml")
    parser.add_argument("--output", required=True, help="Output directory")

    opts = parser.parse_args(args)
    cfg = load_and_validate_autolabel_config(opts.config)
    output = Path(opts.output)

    _autolabel_pipeline(cfg, output)
