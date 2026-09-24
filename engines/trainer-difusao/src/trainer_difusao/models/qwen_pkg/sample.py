"""
Geração de amostras de validação durante o treinamento do Qwen-Image-2.1.
"""
from __future__ import annotations

import os
from pathlib import Path
from typing import Any

from trainer_difusao.common import _ensure_qwen_diffusers_compat


def _generate_sample_qwen(
    transformer: Any,
    vae: Any,
    scheduler: Any,
    prompt: str,
    output_path: Path,
    seed: int = 42,
    resolution: int = 1024,
    metrics_path: Path | None = None,
    epoch: int = 0,
    sample_embeds: dict[str, Any] | None = None,
) -> None:
    """Gera uma imagem de teste para Qwen-Image-2.1 durante o treino com seed fixa determinística."""
    try:
        import torch

        _ensure_qwen_diffusers_compat()
        import diffusers

        output_path = Path(output_path)
        output_path.parent.mkdir(parents=True, exist_ok=True)
        tmp_path = output_path.with_name(f".tmp_{output_path.name}")

        was_training = getattr(transformer, "training", False)
        transformer.eval()

        total_sample_steps = 20

        def step_callback(
            pipe_obj: Any, step_idx: int, timestep: Any, callback_kwargs: dict[str, Any]
        ) -> dict[str, Any]:
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

        has_embeds = sample_embeds is not None and "prompt_embeds" in sample_embeds
        device = "cuda" if torch.cuda.is_available() else "cpu"

        orig_vae_dtype = getattr(vae, "dtype", None)
        target_dtype = getattr(transformer, "dtype", None)
        if (
            target_dtype is not None
            and orig_vae_dtype is not None
            and orig_vae_dtype != target_dtype
        ):
            try:
                vae.to(dtype=target_dtype)
            except Exception:
                pass

        try:
            PipelineCls = getattr(
                diffusers,
                "QwenImagePipeline",
                getattr(diffusers, "QwenImage21Pipeline", None),
            )
            if PipelineCls is None:
                print(
                    "[WARN] QwenImagePipeline não disponível no diffusers.",
                    flush=True,
                )
                return

            pipe_kwargs_init: dict[str, Any] = {
                "scheduler": scheduler,
                "vae": vae,
                "text_encoder": None,
                "transformer": transformer,
            }
            import inspect

            sig = inspect.signature(PipelineCls.__init__)
            if "tokenizer" in sig.parameters:
                pipe_kwargs_init["tokenizer"] = None
            if "processor" in sig.parameters:
                class _DummyProcessor:
                    def apply_chat_template(self, *args, **kwargs):
                        return [[0] * 34]
                    class tokenizer:
                        @staticmethod
                        def encode(*args, **kwargs):
                            return [151655]
                pipe_kwargs_init["processor"] = _DummyProcessor()

            pipe = PipelineCls(**pipe_kwargs_init)
            if hasattr(pipe, "set_progress_bar_config"):
                pipe.set_progress_bar_config(disable=True)

            exec_dev = getattr(pipe, "_execution_device", None) or getattr(transformer, "device", None)
            if not isinstance(exec_dev, (str, torch.device)):
                exec_dev = "cuda" if torch.cuda.is_available() else "cpu"
            generator = torch.Generator(device=exec_dev).manual_seed(seed)
            with torch.inference_mode():
                pipe_kwargs: dict[str, Any] = {
                    "generator": generator,
                    "num_inference_steps": total_sample_steps,
                    "guidance_scale": 3.5,
                    "height": resolution,
                    "width": resolution,
                }
                if has_embeds:
                    pe = sample_embeds["prompt_embeds"]
                    pipe_kwargs["prompt_embeds"] = (
                        pe.to(device) if hasattr(pe, "to") else pe
                    )
                    pem = sample_embeds.get("prompt_embeds_mask")
                    if pem is not None:
                        pipe_kwargs["prompt_embeds_mask"] = (
                            pem.to(device) if hasattr(pem, "to") else pem
                        )
                else:
                    print(
                        f"[WARN] Amostra de validação cancelada: sample_embeds ausente para '{prompt[:40]}'.",
                        flush=True,
                    )
                    return
                try:
                    out = pipe(**pipe_kwargs, callback_on_step_end=step_callback)
                except TypeError:
                    out = pipe(**pipe_kwargs)

                image = out.images[0]
                image.save(tmp_path)
                os.replace(tmp_path, output_path)
                print(
                    f"[QWEN-IMAGE] Amostra de validação salva (seed={seed}) em: {output_path}",
                    flush=True,
                )

                if metrics_path is not None:
                    try:
                        from trainer_difusao.common_pkg.metrics import _emit_metric

                        _emit_metric(
                            metrics_path,
                            epoch=epoch,
                            phase="sample_ready",
                            message=f"Amostra visual da Época {epoch} pronta.",
                            telemetry_only=True,
                        )
                    except Exception:
                        pass
        finally:
            if (
                orig_vae_dtype is not None
                and getattr(vae, "dtype", None) != orig_vae_dtype
            ):
                try:
                    vae.to(dtype=orig_vae_dtype)
                except Exception:
                    pass
            if was_training:
                transformer.train()
    except Exception as e:
        import traceback

        print(
            f"[WARN] Falha ao gerar amostra de validação Qwen-Image: {e}\n{traceback.format_exc()}",
            flush=True,
        )
