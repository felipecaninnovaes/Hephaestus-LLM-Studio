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

        orig_vae_dev = getattr(vae, "device", None)
        orig_vae_dtype = getattr(vae, "dtype", None)
        target_dtype = getattr(transformer, "dtype", None)
        if hasattr(vae, "to") and orig_vae_dev is not None and str(orig_vae_dev) != str(device):
            try:
                vae.to(device)
            except Exception:
                pass
        if (
            target_dtype is not None
            and orig_vae_dtype is not None
            and orig_vae_dtype != target_dtype
        ):
            try:
                vae.to(dtype=target_dtype)
            except Exception:
                pass

        if hasattr(transformer, "config") and not hasattr(transformer.config, "guidance_embeds"):
            try:
                from diffusers.configuration_utils import FrozenDict
                cfg_dict = dict(transformer.config)
                cfg_dict["guidance_embeds"] = False
                transformer.config = FrozenDict(cfg_dict)
            except Exception:
                pass

        try:
            PipelineCls = getattr(
                diffusers,
                "QwenImage21Pipeline",
                getattr(diffusers, "QwenImagePipeline", None),
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
            if hasattr(pipe, "vae") and pipe.vae is not None:
                if hasattr(pipe.vae, "enable_tiling"):
                    try:
                        pipe.vae.enable_tiling()
                    except Exception:
                        pass
                if hasattr(pipe.vae, "enable_slicing"):
                    try:
                        pipe.vae.enable_slicing()
                    except Exception:
                        pass
            exec_dev = getattr(pipe, "_execution_device", None) or getattr(transformer, "device", None)
            if not isinstance(exec_dev, (str, torch.device)):
                exec_dev = "cuda" if torch.cuda.is_available() else "cpu"
            generator = torch.Generator(device=exec_dev).manual_seed(seed)
            sample_res = min(resolution, 512)
            sample_res = max(16, (sample_res // 16) * 16)
            with torch.inference_mode():
                pipe_kwargs: dict[str, Any] = {
                    "generator": generator,
                    "num_inference_steps": total_sample_steps,
                    "height": sample_res,
                    "width": sample_res,
                }
                try:
                    call_sig = inspect.signature(pipe.__call__)
                    call_params = call_sig.parameters
                except Exception:
                    call_params = {}

                if "true_cfg_scale" in call_params:
                    pipe_kwargs["true_cfg_scale"] = 3.5
                elif "guidance_scale" in call_params:
                    pipe_kwargs["guidance_scale"] = 3.5
                else:
                    pipe_kwargs["true_cfg_scale"] = 3.5
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
                    ipm = sample_embeds.get("image_pad_mask")
                    if ipm is not None:
                        pipe_kwargs["image_pad_mask"] = (
                            ipm.to(device) if hasattr(ipm, "to") else ipm
                        )
                else:
                    print(
                        f"[WARN] Amostra de validação cancelada: sample_embeds ausente para '{prompt[:40]}'.",
                        flush=True,
                    )
                    return
                def _invoke_pipe(kwargs: dict[str, Any]) -> Any:
                    try:
                        return pipe(**kwargs, callback_on_step_end=step_callback)
                    except TypeError:
                        return pipe(**kwargs)

                try:
                    out = _invoke_pipe(pipe_kwargs)
                except TypeError as err:
                    err_msg = str(err).lower()
                    if "true_cfg_scale" in pipe_kwargs and ("true_cfg_scale" in err_msg or "unexpected keyword" in err_msg):
                        alt_kwargs = dict(pipe_kwargs)
                        alt_kwargs.pop("true_cfg_scale", None)
                        alt_kwargs["guidance_scale"] = 3.5
                        out = _invoke_pipe(alt_kwargs)
                    elif "guidance_scale" in pipe_kwargs and ("guidance_scale" in err_msg or "unexpected keyword" in err_msg):
                        alt_kwargs = dict(pipe_kwargs)
                        alt_kwargs.pop("guidance_scale", None)
                        alt_kwargs["true_cfg_scale"] = 3.5
                        out = _invoke_pipe(alt_kwargs)
                    else:
                        raise
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
            if "pipe" in locals():
                try:
                    del pipe
                except Exception:
                    pass
            import gc
            import ctypes
            gc.collect()
            try:
                ctypes.CDLL("libc.so.6").malloc_trim(0)
            except Exception:
                pass
            if hasattr(vae, "to") and orig_vae_dev is not None and str(orig_vae_dev) != str(device):
                try:
                    vae.to(orig_vae_dev)
                except Exception:
                    pass
            if torch.cuda.is_available():
                torch.cuda.empty_cache()
                torch.cuda.ipc_collect()
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
