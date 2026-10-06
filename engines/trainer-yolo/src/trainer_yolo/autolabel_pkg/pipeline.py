"""
Pipeline de execução de AutoLabel v2 (Florence-2, Qwen2-VL, OpenAI, Mock) e CLI.
"""
from __future__ import annotations

import argparse
import collections
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
from trainer_yolo.autolabel_pkg.vision_api import (
    VisionApiFatalError,
    _call_openai_vision_api,
    _positive_int_env,
)
from trainer_yolo.config import load_and_validate_autolabel_config
from trainer_yolo.dataset import _discover_dataset_images

# Concorrência padrão do caminho OpenAI: 1 (servidores llama.cpp com speculative
# decoding retornam HTTP 500 sob requisições concorrentes). Override: AUTOLABEL_CONCURRENCY.
DEFAULT_AUTOLABEL_CONCURRENCY = 1


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
    skipped_count = 0

    if model == "openai":
        raw_concurrency = os.environ.get("AUTOLABEL_CONCURRENCY", str(DEFAULT_AUTOLABEL_CONCURRENCY))
        try:
            concurrency = max(1, int(raw_concurrency))
        except ValueError:
            concurrency = DEFAULT_AUTOLABEL_CONCURRENCY

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
        futures: dict[concurrent.futures.Future, str] = {}
        remaining_to_submit = collections.deque(sorted_filenames)

        # Preenche a janela inicial com até 'concurrency' tarefas em voo
        while len(futures) < concurrency and remaining_to_submit:
            fname_next = remaining_to_submit.popleft()
            f_init = executor.submit(_label_worker, fname_next)
            futures[f_init] = fname_next
            if remaining_to_submit:
                time.sleep(0.005)
        buffer_completed: dict[str, str] = {}
        failures: dict[str, str] = {}
        consecutive_failures = 0
        last_error = ""
        max_consecutive_failures = _positive_int_env("AUTOLABEL_MAX_CONSECUTIVE_FAILURES") or 10
        next_flush_idx = 0
        step_num = 0
        last_completion_time = time.monotonic()

        def _flush_ready() -> None:
            """Escreve o prefixo contíguo resolvido; imagens puladas contam como resolvidas."""
            nonlocal next_flush_idx
            while next_flush_idx < total_imgs:
                expected_f = sorted_filenames[next_flush_idx]
                if expected_f in buffer_completed:
                    line = json.dumps(
                        {"filename": expected_f, "caption": buffer_completed[expected_f]},
                        ensure_ascii=False,
                    )
                    f.write(line + "\n")
                    f.flush()
                elif expected_f not in failures:
                    break
                next_flush_idx += 1

        with open(captions_path, "w", encoding="utf-8") as f:
            try:
                while futures:
                    done, _ = concurrent.futures.wait(
                        futures.keys(), return_when=concurrent.futures.FIRST_COMPLETED
                    )
                    # Processa as tarefas concluídas priorizando a ordem de sorted_filenames se múltiplos concluírem no mesmo ciclo
                    sorted_done = sorted(done, key=lambda f: sorted_filenames.index(futures[f]))
                    for fut in sorted_done:
                        fname_orig = futures.pop(fut)
                        try:
                            fname, caption, _ = fut.result()
                        except Exception as exc:
                            if isinstance(exc, VisionApiFatalError):
                                executor.shutdown(wait=False, cancel_futures=True)
                                _die(f"AutoLabel OpenAI falhou na imagem '{fname_orig}': {exc}")
                            print(
                                f"[autolabel-openai] Pulando '{fname_orig}': {exc}",
                                file=sys.stderr,
                            )
                            failures[fname_orig] = str(exc)
                            last_error = str(exc)
                            consecutive_failures += 1
                            fname = fname_orig
                        else:
                            consecutive_failures = 0
                            buffer_completed[fname] = caption
                        step_num += 1

                        # Escreve em captions.jsonl o prefixo contíguo na ordem de sorted_filenames
                        _flush_ready()

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
                            message=(
                                f"Pulando imagem {step_num}/{total_imgs}: {fname}"
                                if fname in failures
                                else f"Anotando imagem {step_num}/{total_imgs}: {fname}"
                            ),
                            progress=progress,
                            step=step_num,
                            total_steps=total_imgs,
                            step_time_seconds=round(step_time_ema, 4),
                            eta_seconds=eta_s,
                        )

                        if consecutive_failures >= max_consecutive_failures:
                            executor.shutdown(wait=False, cancel_futures=True)
                            _die(
                                f"AutoLabel OpenAI abortado: {consecutive_failures} falhas "
                                f"consecutivas (limite {max_consecutive_failures}, "
                                f"AUTOLABEL_MAX_CONSECUTIVE_FAILURES). Último erro: {last_error}"
                            )

                        # Submete novas tarefas para manter a janela de até 'concurrency' em voo
                        while len(futures) < concurrency and remaining_to_submit:
                            fname_new = remaining_to_submit.popleft()
                            f_sub = executor.submit(_label_worker, fname_new)
                            futures[f_sub] = fname_new
            finally:
                executor.shutdown(wait=False, cancel_futures=True)

            # Flush final caso reste algo no buffer
            _flush_ready()
            f.flush()

        skipped_count = len(failures)
        if total_imgs and skipped_count == total_imgs:
            _die(
                f"AutoLabel OpenAI falhou em todas as {total_imgs} imagens. "
                f"Último erro: {last_error}"
            )
        if failures:
            failures_path = output_dir / "autolabel_failures.jsonl"
            with open(failures_path, "w", encoding="utf-8") as ff:
                for fail_name, fail_err in sorted(failures.items()):
                    ff.write(
                        json.dumps(
                            {"filename": fail_name, "error": fail_err}, ensure_ascii=False
                        )
                        + "\n"
                    )
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
    if skipped_count:
        completed_message = (
            f"AutoLabel concluído: {total_imgs - skipped_count}/{total_imgs} imagens anotadas; "
            f"{skipped_count} puladas por erro."
        )
    else:
        completed_message = f"AutoLabel concluído: {total_imgs} imagens anotadas com sucesso."
    emitter.emit(
        phase="completed",
        message=completed_message,
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
