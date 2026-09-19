"""
Geração de amostras de validação durante o treinamento do FLUX.
"""
from __future__ import annotations

import os
from pathlib import Path
from typing import Any


def _generate_sample_flux(
    transformer: Any,
    vae: Any,
    text_encoder_one: Any,
    text_encoder_two: Any,
    tokenizer_one: Any,
    tokenizer_two: Any,
    scheduler: Any,
    prompt: str,
    output_path: Path,
    seed: int = 42,
    is_flux2: bool = False,
    resolution: int = 512,
    metrics_path: Path | None = None,
    epoch: int = 0,
) -> None:
    """Gera uma imagem de teste para FLUX.2 Klein ou FLUX.1 com pesos LoRA ativos e seed fixa determinística."""
    try:
        import torch

        was_training = getattr(transformer, "training", False)
        transformer.eval()
        output_path.parent.mkdir(parents=True, exist_ok=True)
        tmp_path = output_path.with_name(f".tmp_{output_path.name}")

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

        try:
            if is_flux2:
                try:
                    from diffusers import Flux2KleinPipeline

                    pipe = Flux2KleinPipeline(
                        scheduler=scheduler,
                        text_encoder=text_encoder_one,
                        tokenizer=tokenizer_one,
                        vae=vae,
                        transformer=transformer,
                    )
                    pipe.set_progress_bar_config(disable=True)
                    generator = torch.Generator(device="cuda" if torch.cuda.is_available() else "cpu").manual_seed(seed)
                    with torch.inference_mode():
                        try:
                            image = pipe(
                                prompt=prompt,
                                generator=generator,
                                num_inference_steps=total_sample_steps,
                                guidance_scale=3.5,
                                height=resolution,
                                width=resolution,
                                callback_on_step_end=step_callback,
                            ).images[0]
                        except TypeError:
                            image = pipe(
                                prompt=prompt,
                                generator=generator,
                                num_inference_steps=total_sample_steps,
                                guidance_scale=3.5,
                                height=resolution,
                                width=resolution,
                            ).images[0]
                        image.save(tmp_path)
                        os.replace(tmp_path, output_path)
                        print(f"[FLUX-KLEIN] Amostra de validação salva (seed={seed}) em: {output_path}", flush=True)
                        return
                except Exception as e:
                    print(f"[WARN] Tentativa com Flux2KleinPipeline: {e}. Tentando fallback...", flush=True)

            from diffusers import FluxPipeline

            pipe = FluxPipeline(
                scheduler=scheduler,
                text_encoder=text_encoder_one,
                text_encoder_2=text_encoder_two,
                tokenizer=tokenizer_one,
                tokenizer_2=tokenizer_two,
                vae=vae,
                transformer=transformer,
            )
            pipe.set_progress_bar_config(disable=True)
            generator = torch.Generator(device="cuda" if torch.cuda.is_available() else "cpu").manual_seed(seed)
            with torch.inference_mode():
                try:
                    image = pipe(
                        prompt=prompt,
                        generator=generator,
                        num_inference_steps=total_sample_steps,
                        guidance_scale=3.5,
                        height=resolution,
                        width=resolution,
                        callback_on_step_end=step_callback,
                    ).images[0]
                except TypeError:
                    image = pipe(
                        prompt=prompt,
                        generator=generator,
                        num_inference_steps=total_sample_steps,
                        guidance_scale=3.5,
                        height=resolution,
                        width=resolution,
                    ).images[0]
                image.save(tmp_path)
                os.replace(tmp_path, output_path)
                print(f"[FLUX] Amostra de validação salva (seed={seed}, steps=20, cfg=3.5) em: {output_path}", flush=True)
        finally:
            if was_training:
                transformer.train()
    except Exception as e:
        print(f"[WARN] Falha ao gerar amostra de validação FLUX: {e}", flush=True)
