"""
Geração de amostras de validação para Stable Diffusion (SD 1.5 e SDXL).
"""
from __future__ import annotations

import os
from pathlib import Path
from typing import Any

__all__ = ["_generate_sample_sd15", "_generate_sample_sdxl"]


def _generate_sample_sd15(
    unet: Any,
    vae: Any,
    text_encoder: Any,
    tokenizer: Any,
    noise_scheduler: Any,
    prompt: str,
    output_path: Path,
    seed: int = 42,
    metrics_path: Path | None = None,
    epoch: int = 0,
    sample_embeds: dict[str, Any] | None = None,
) -> None:
    """Gera uma imagem de teste para SD 1.5 com os pesos LoRA ativos e seed fixa determinística.
    
    Chama unet.eval() durante a inferência e grava atomicamente via arquivo temporário (.tmp_*).
    """
    try:
        import torch
        from diffusers import StableDiffusionPipeline

        was_training = getattr(unet, "training", False)
        unet.eval()
        output_path.parent.mkdir(parents=True, exist_ok=True)
        tmp_path = output_path.with_name(f".tmp_{output_path.name}")

        has_embeds = sample_embeds is not None and "prompt_embeds" in sample_embeds
        device = "cuda" if torch.cuda.is_available() else "cpu"
        try:
            pipe = StableDiffusionPipeline(
                vae=vae,
                text_encoder=None if has_embeds else text_encoder,
                tokenizer=None if has_embeds else tokenizer,
                unet=unet,
                scheduler=noise_scheduler,
                safety_checker=None,
                feature_extractor=None,
                requires_safety_checker=False,
            )
            pipe.set_progress_bar_config(disable=True)
            generator = torch.Generator(device=device).manual_seed(seed)
            total_sample_steps = 20

            def step_callback(pipe_obj: Any, step_idx: int, timestep: Any, callback_kwargs: dict[str, Any]) -> dict[str, Any]:
                if metrics_path is not None:
                    try:
                        from trainer_difusao.common_pkg.metrics import _emit_metric

                        step_num = step_idx + 1
                        _emit_metric(
                            metrics_path,
                            epoch=epoch,
                            phase="generating_sample",
                            message=f"Gerando amostra de validação (passo {step_num}/{total_sample_steps})...",
                            telemetry_only=True,
                        )
                    except Exception:
                        pass
                return callback_kwargs

            with torch.inference_mode():
                pipe_kwargs = {
                    "generator": generator,
                    "num_inference_steps": total_sample_steps,
                    "guidance_scale": 7.5,
                    "output_type": "latent",
                }
                if has_embeds:
                    pipe_kwargs["prompt_embeds"] = sample_embeds["prompt_embeds"].to(device)
                    if sample_embeds.get("negative_prompt_embeds") is not None:
                        pipe_kwargs["negative_prompt_embeds"] = sample_embeds["negative_prompt_embeds"].to(device)
                else:
                    pipe_kwargs["prompt"] = prompt

                try:
                    latents = pipe(**pipe_kwargs, callback_on_step_end=step_callback).images
                except TypeError:
                    latents = pipe(**pipe_kwargs).images
                latents = latents.to(dtype=torch.float32) / 0.18215
                decoded = vae.decode(latents).sample
                image = (decoded / 2 + 0.5).clamp(0, 1)
                image = image.cpu().permute(0, 2, 3, 1).float().numpy()
                img = pipe.numpy_to_pil(image)[0]
                img.save(tmp_path)
                os.replace(tmp_path, output_path)
                print(
                    f"[SD 1.5] Amostra de validação salva (seed={seed}) em: {output_path}",
                    flush=True,
                )
        finally:
            if was_training:
                unet.train()
    except Exception as e:
        print(f"[WARN] Falha ao gerar amostra de validação SD 1.5: {e}", flush=True)


def _generate_sample_sdxl(
    unet: Any,
    vae: Any,
    text_encoder_one: Any,
    text_encoder_two: Any,
    tokenizer_one: Any,
    tokenizer_two: Any,
    noise_scheduler: Any,
    prompt: str,
    output_path: Path,
    seed: int = 42,
    metrics_path: Path | None = None,
    epoch: int = 0,
    sample_embeds: dict[str, Any] | None = None,
) -> None:
    """Gera uma imagem de teste para SDXL com os pesos LoRA ativos e seed fixa determinística.
    
    Chama unet.eval() durante a inferência e grava atomicamente via arquivo temporário (.tmp_*).
    """
    try:
        import torch
        from diffusers import StableDiffusionXLPipeline

        was_training = getattr(unet, "training", False)
        unet.eval()
        output_path.parent.mkdir(parents=True, exist_ok=True)
        tmp_path = output_path.with_name(f".tmp_{output_path.name}")

        has_embeds = sample_embeds is not None and "prompt_embeds" in sample_embeds
        device = "cuda" if torch.cuda.is_available() else "cpu"
        try:
            pipe = StableDiffusionXLPipeline(
                vae=vae,
                text_encoder=None if has_embeds else text_encoder_one,
                text_encoder_2=None if has_embeds else text_encoder_two,
                tokenizer=None if has_embeds else tokenizer_one,
                tokenizer_2=None if has_embeds else tokenizer_two,
                unet=unet,
                scheduler=noise_scheduler,
            )
            pipe.set_progress_bar_config(disable=True)
            generator = torch.Generator(device=device).manual_seed(seed)
            total_sample_steps = 20

            def step_callback(pipe_obj: Any, step_idx: int, timestep: Any, callback_kwargs: dict[str, Any]) -> dict[str, Any]:
                if metrics_path is not None:
                    try:
                        from trainer_difusao.common_pkg.metrics import _emit_metric

                        step_num = step_idx + 1
                        _emit_metric(
                            metrics_path,
                            epoch=epoch,
                            step=step_num,
                            phase="generating_sample",
                            message=f"Gerando amostra de validação (passo {step_num}/{total_sample_steps})...",
                            telemetry_only=True,
                        )
                    except Exception:
                        pass
                return callback_kwargs

            with torch.inference_mode():
                pipe_kwargs = {
                    "generator": generator,
                    "num_inference_steps": total_sample_steps,
                    "guidance_scale": 7.0,
                    "output_type": "latent",
                }
                if has_embeds:
                    pipe_kwargs["prompt_embeds"] = sample_embeds["prompt_embeds"].to(device)
                    if sample_embeds.get("pooled_prompt_embeds") is not None:
                        pipe_kwargs["pooled_prompt_embeds"] = sample_embeds["pooled_prompt_embeds"].to(device)
                    if sample_embeds.get("negative_prompt_embeds") is not None:
                        pipe_kwargs["negative_prompt_embeds"] = sample_embeds["negative_prompt_embeds"].to(device)
                    if sample_embeds.get("negative_pooled_prompt_embeds") is not None:
                        pipe_kwargs["negative_pooled_prompt_embeds"] = sample_embeds["negative_pooled_prompt_embeds"].to(device)
                else:
                    pipe_kwargs["prompt"] = prompt

                try:
                    latents = pipe(**pipe_kwargs, callback_on_step_end=step_callback).images
                except TypeError:
                    latents = pipe(**pipe_kwargs).images
                latents = latents.to(dtype=torch.float32) / vae.config.scaling_factor
                decoded = vae.decode(latents).sample
                image = (decoded / 2 + 0.5).clamp(0, 1)
                image = image.cpu().permute(0, 2, 3, 1).float().numpy()
                img = pipe.numpy_to_pil(image)[0]
                img.save(tmp_path)
                os.replace(tmp_path, output_path)
                print(
                    f"[SDXL] Amostra de validação salva (seed={seed}) em: {output_path}",
                    flush=True,
                )
        finally:
            if was_training:
                unet.train()
    except Exception as e:
        print(f"[WARN] Falha ao gerar amostra de validação SDXL: {e}", flush=True)
