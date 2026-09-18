"""Motor de treino de Difusão LoRA do Hephaestus (trainer-difusao).

Ponto de entrada do CLI e despachante modular para:
- FLUX.2 Klein 4B e FLUX.1 (Flow Matching, AutoencoderKLFlux2)
- SDXL 1.0 (Dual CLIP, micro-conditioning, UNet)
- Stable Diffusion 1.5 (DDPM, CLIPText, UNet)
- Mock Trainer (ENGINE_MOCK=1 para dev e CI)
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path
from typing import Any

import yaml

from trainer_difusao.common import (
    _canonical_model_name,
    _die,
    _emit_metric,
    _resolve_output_name,
    _save_lora_safetensors,
    _setup_cache_dir,
)
from trainer_difusao.dataset import DiffusionDataset
from trainer_difusao.models import get_trainer
from trainer_difusao.models.flux import (
    _encode_qwen3_prompt,
    _generate_sample_flux,
    _pack_latents,
    _pack_latents_flux2,
    _patchify_latents_flux2,
    _prepare_flux2_latent_ids,
    _prepare_flux2_text_ids,
    _prepare_latent_image_ids,
    _prepare_text_ids,
    _real_train_flux,
)
from trainer_difusao.models.mock import (
    _generate_mock_safetensors,
    _generate_mock_sample,
    _mock_train,
)
from trainer_difusao.models.sd15 import (
    _generate_sample_sd15,
    _real_train_sd15,
)
from trainer_difusao.models.sdxl import (
    _compute_sdxl_embeddings,
    _generate_sample_sdxl,
    _real_train_sdxl,
)
from trainer_difusao.optimizers import (
    _create_lr_scheduler,
    _create_optimizer,
)


def _real_train(cfg: dict[str, Any], output: Path) -> None:
    """Executa o pipeline real despachando para o trainer do modelo configurado."""
    raw_model = cfg.get("model", "sdxl")
    trainer = get_trainer(raw_model, is_mock=False)
    trainer.train(cfg, output)


def cmd_train(args: list[str]) -> None:
    """Subcomando train: valida flags e despacha para o trainer correspondente."""
    parser = argparse.ArgumentParser(
        prog="trainer-difusao train",
        description="Diffusion LoRA trainer — FLUX.2 Klein 4B, SDXL, SD 1.5 (mock/real)",
    )
    parser.add_argument("--config", required=True, help="Caminho para config.yaml")
    parser.add_argument("--output", required=True, help="Diretório de saída")

    opts = parser.parse_args(args)
    if not os.path.exists(opts.config):
        _die(f"Arquivo de configuração não encontrado: {opts.config}")

    with open(opts.config, "r", encoding="utf-8") as f:
        cfg = yaml.safe_load(f)

    output = Path(opts.output)
    is_mock = os.environ.get("ENGINE_MOCK", "1").strip().lower() in ("1", "true", "yes")

    raw_model = cfg.get("model", "flux")
    trainer = get_trainer(raw_model, is_mock=is_mock)
    try:
        trainer.train(cfg, output)
    except Exception as e:
        is_oom = "OutOfMemoryError" in type(e).__name__ or "CUDA out of memory" in str(e)
        code = "cuda_oom" if is_oom else "crash"
        msg = f"CUDA OOM: {e}" if is_oom else f"Erro no treino: {e}"
        _emit_metric(output / "metrics.jsonl", epoch=0, step=0, phase="error", message=msg)
        print(f"ERROR: [{code}] {msg}", file=sys.stderr, flush=True)
        sys.exit(1)

def cmd_health() -> None:
    """Subcomando health: emite status JSON para verificação de liveness."""
    is_mock = os.environ.get("ENGINE_MOCK", "1").strip().lower() in ("1", "true", "yes")
    mode = "mock" if is_mock else "real"
    print(json.dumps({"status": "ok", "engine": "trainer-difusao", "mode": mode}))


def main(argv: list[str] | None = None) -> None:
    """Ponto de entrada universal do CLI do trainer de difusão."""
    if argv is None:
        argv = sys.argv[1:]

    if not argv:
        cmd_health()
        return

    if argv[0] in ("-h", "--help"):
        print("Uso: python -m trainer_difusao [health|train|generate|serve] [args...]")
        return

    if argv[0] == "train":
        cmd_train(argv[1:])
    elif argv[0] == "generate":
        from trainer_difusao.generate import cmd_generate

        cmd_generate(argv[1:])
    elif argv[0] == "serve":
        from trainer_difusao.serve import cmd_serve

        cmd_serve(argv[1:])
    elif argv[0] == "health":
        cmd_health()
    else:
        _die(f"Subcomando desconhecido: {argv[0]}")


if __name__ == "__main__":
    main()


__all__ = [
    "DiffusionDataset",
    "_canonical_model_name",
    "_compute_sdxl_embeddings",
    "_create_lr_scheduler",
    "_create_optimizer",
    "_die",
    "_emit_metric",
    "_encode_qwen3_prompt",
    "_generate_mock_safetensors",
    "_generate_mock_sample",
    "_generate_sample_flux",
    "_generate_sample_sd15",
    "_generate_sample_sdxl",
    "_mock_train",
    "_pack_latents",
    "_pack_latents_flux2",
    "_patchify_latents_flux2",
    "_prepare_flux2_latent_ids",
    "_prepare_flux2_text_ids",
    "_prepare_latent_image_ids",
    "_prepare_text_ids",
    "_real_train",
    "_real_train_flux",
    "_real_train_sd15",
    "_real_train_sdxl",
    "_resolve_output_name",
    "_save_lora_safetensors",
    "_setup_cache_dir",
    "cmd_health",
    "cmd_train",
    "main",
]
