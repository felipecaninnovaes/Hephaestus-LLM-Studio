"""Pipeline de treino LoRA para Qwen-Image-2.1."""

from __future__ import annotations

import math
import os
import shutil
from pathlib import Path
from typing import Any

from engine_kit.mock import is_mock
from trainer_difusao.common import (
    _cleanup_cuda,
    _die,
    _ensure_qwen_diffusers_compat,
    _emit_metric,
    _normalize_train_quantization,
    _resolve_output_name,
    _save_lora_safetensors,
    _setup_cache_dir,
    _validate_train_aux,
)
from trainer_difusao.dataset import DiffusionDataset, build_dataloader
from trainer_difusao.models.base import BaseModelTrainer
from trainer_difusao.models.mock import _mock_train


def _real_train_qwen_image(cfg: dict[str, Any], output: Path) -> None:
    """Pipeline real de treino LoRA para Qwen-Image-2.1 na GPU."""
    output.mkdir(parents=True, exist_ok=True)
    metrics_path = output / "metrics.jsonl"
    checkpoints_dir = output / "checkpoints"
    checkpoints_dir.mkdir(parents=True, exist_ok=True)

    try:
        import torch
        import torch.nn.functional as F
        from peft import LoraConfig
    except ImportError as exc:
        _die(f"Dependência ausente para treino real de Qwen-Image-2.1: {exc}")
    _ensure_qwen_diffusers_compat()

    import diffusers

    lora_cfg = cfg.get("lora", {})
    rank = int(lora_cfg.get("rank", 16))
    alpha = int(lora_cfg.get("alpha", 16))
    epochs = int(lora_cfg.get("epochs", 10))
    learning_rate = float(lora_cfg.get("learning_rate", 2e-4))
    trigger_word = str(lora_cfg.get("trigger_word", "") or "").strip()
    raw_quant = lora_cfg.get("quantization") or cfg.get("quantization") or "4bit"
    quantization = _normalize_train_quantization(raw_quant, default="4bit")
    base_name = _resolve_output_name(cfg)
    seed = int(cfg.get("seed", 42))

    checkpoint_interval = max(1, int(cfg.get("checkpoint_interval") or lora_cfg.get("checkpoint_interval") or 1))
    epoch_offset = max(0, int(cfg.get("epoch_offset") or lora_cfg.get("epoch_offset") or 0))
    grad_accum = max(1, int(lora_cfg.get("gradient_accumulation_steps", 1)))
    batch_size = max(1, int(lora_cfg.get("batch_size", 1)))
    resolution = int(cfg.get("resolution", 1024))
    dataset_path = cfg.get("dataset_path")
    if not dataset_path or not os.path.exists(dataset_path):
        _die(f"dataset_path inválido ou inexistente: '{dataset_path}'")

    model_repo = os.environ.get("QWEN_IMAGE_MODEL_ID", "Qwen/Qwen-Image-2.1")
    device = "cuda" if torch.cuda.is_available() else "cpu"
    target_dtype = torch.bfloat16 if torch.cuda.is_available() and torch.cuda.is_bf16_supported() else torch.float32

    _emit_metric(
        metrics_path,
        epoch=epoch_offset,
        step=0,
        loss=1.0,
        lr=learning_rate,
        progress=0.01,
        phase="initializing",
        message=f"Inicializando treino Qwen-Image-2.1 ({model_repo})...",
    )

    hf_token = os.environ.get("HF_TOKEN")
    hub_cache = _setup_cache_dir(hf_token=hf_token)

    # 1. Carrega Dataset e DataLoader
    dataset = DiffusionDataset(
        dataset_path=dataset_path,
        resolution=resolution,
        trigger_word=trigger_word,
        enable_bucket=bool(cfg.get("enable_bucket", True)),
    )
    dataloader = build_dataloader(dataset, batch_size=batch_size, seed=seed)
    total_steps = len(dataloader) * epochs // grad_accum

    # 2. Carrega VAE
    VaeCls = getattr(diffusers, "AutoencoderKLQwenImage21", getattr(diffusers, "AutoencoderKLQwenImage", None))
    if VaeCls is None:
        _die("AutoencoderKLQwenImage21 não disponível no diffusers.")

    print(f"[DIFFUSION-TRAIN] Carregando VAE de {model_repo}...", flush=True)
    vae = VaeCls.from_pretrained(
        model_repo,
        subfolder="vae",
        torch_dtype=torch.float32,
        cache_dir=hub_cache,
        token=hf_token,
    ).to(device)
    vae.eval()
    vae.requires_grad_(False)

    # 3. Carrega Transformer com quantização 4-bit (se selecionada) e LoRA
    TransformerCls = getattr(
        diffusers, "QwenImage21Transformer2DModel", getattr(diffusers, "QwenImageTransformer2DModel", None)
    )
    if TransformerCls is None:
        _die("QwenImage21Transformer2DModel não está disponível na versão instalada do diffusers.")

    transformer_kwargs = {
        "torch_dtype": target_dtype,
        "cache_dir": hub_cache,
        "token": hf_token,
    }
    if quantization in ("4bit", "4bit-nf4") and device == "cuda":
        from transformers import BitsAndBytesConfig
        transformer_kwargs["quantization_config"] = BitsAndBytesConfig(
            load_in_4bit=True,
            bnb_4bit_quant_type="nf4",
            bnb_4bit_use_double_quant=True,
            bnb_4bit_compute_dtype=torch.bfloat16,
        )

    print(f"[DIFFUSION-TRAIN] Carregando Transformer de {model_repo} (quant={quantization})...", flush=True)
    transformer = TransformerCls.from_pretrained(
        model_repo,
        subfolder="transformer",
        **transformer_kwargs,
    )

    lora_config = LoraConfig(
        r=rank,
        lora_alpha=alpha,
        init_lora_weights="gaussian",
        target_modules=["to_k", "to_q", "to_v", "to_out.0"],
    )
    transformer.add_adapter(lora_config)
    try:
        transformer.enable_gradient_checkpointing()
    except Exception:
        pass
    transformer.train()

    # 4. Otimizador e Scheduler
    from trainer_difusao.optimizers import _create_lr_scheduler, _create_optimizer

    optimizer = _create_optimizer(
        transformer,
        lora_cfg.get("optimizer", "adamw8bit"),
        learning_rate,
    )
    lr_scheduler = _create_lr_scheduler(
        optimizer,
        lora_cfg.get("lr_scheduler", "cosine"),
        total_steps=total_steps,
        warmup_steps=int(lora_cfg.get("lr_warmup_steps", 10)),
    )

    metadata = {
        "format": "pt",
        "framework": "diffusers",
        "model_type": "lora",
        "base_model": "qwen-image-2.1",
        "lora_rank": str(rank),
        "lora_alpha": str(alpha),
        "trigger_word": trigger_word,
        "quantization": quantization,
    }

    global_step = 0
    effective_lr = learning_rate

    _emit_metric(
        metrics_path,
        epoch=epoch_offset,
        step=0,
        loss=1.0,
        lr=learning_rate,
        progress=0.05,
        phase="training",
        message=f"Iniciando loop de treino LoRA: {epochs} épocas, {len(dataset)} imagens.",
    )

    # 5. Loop de Treino Real
    for epoch_idx in range(1, epochs + 1):
        epoch = epoch_idx + epoch_offset
        transformer.train()
        epoch_loss = 0.0
        steps_in_epoch = 0
        optimizer.zero_grad()

        for batch in dataloader:
            pixel_values = batch["pixel_values"].to(device)
            bsz = pixel_values.shape[0]

            # Codifica imagens com VAE em latents
            with torch.no_grad():
                latents = vae.encode(pixel_values.float()).latent_dist.sample()
                latents = latents.to(dtype=target_dtype)

            # Ruído e timesteps Flow Matching
            noise = torch.randn_like(latents)
            u = torch.sigmoid(torch.randn(bsz, device=device))
            timesteps = u * 1000.0
            sigmas = (timesteps / 1000.0).view(-1, 1, 1, 1).to(device, dtype=target_dtype)
            noisy_latents = (1.0 - sigmas) * latents + sigmas * noise
            target = noise - latents

            # Forward pass
            pred = transformer(
                hidden_states=noisy_latents,
                timestep=timesteps,
                return_dict=False,
            )[0]

            loss = F.mse_loss(pred.float(), target.float(), reduction="mean")
            cur_loss_raw = loss.item()
            loss = loss / grad_accum
            loss.backward()

            steps_in_epoch += 1
            if steps_in_epoch % grad_accum == 0 or steps_in_epoch == len(dataloader):
                torch.nn.utils.clip_grad_norm_(transformer.parameters(), 1.0)
                optimizer.step()
                if lr_scheduler is not None:
                    lr_scheduler.step()
                optimizer.zero_grad()
                global_step += 1

            if not math.isnan(cur_loss_raw) and not math.isinf(cur_loss_raw):
                epoch_loss += cur_loss_raw

            effective_lr = (
                lr_scheduler.get_last_lr()[0] if lr_scheduler and hasattr(lr_scheduler, "get_last_lr") else learning_rate
            )

        # Métrica da época
        avg_loss = round(epoch_loss / max(1, steps_in_epoch), 4)
        progress = round(min(0.99, max(0.05, epoch_idx / epochs)), 4)
        _emit_metric(
            metrics_path,
            epoch=epoch,
            step=global_step,
            loss=avg_loss,
            lr=effective_lr,
            progress=progress,
            phase="training",
            message=f"Época {epoch}/{epochs + epoch_offset} concluída · Loss: {avg_loss}",
        )
        print(f"[DIFFUSION-TRAIN] Época {epoch}/{epochs + epoch_offset} concluída · Loss: {avg_loss}", flush=True)

        # Salva checkpoint da época
        if epoch_idx % checkpoint_interval == 0 or epoch_idx == epochs:
            ckpt_file = checkpoints_dir / f"{base_name}_epoch_{epoch:03d}.safetensors"
            _save_lora_safetensors(transformer, ckpt_file, {**metadata, "epoch": str(epoch)})

    # 6. Salva adaptador final
    final_adapter_file = output / f"{base_name}.safetensors"
    _save_lora_safetensors(transformer, final_adapter_file, metadata)
    if base_name != "adapter":
        shutil.copy2(final_adapter_file, output / "adapter.safetensors")

    _emit_metric(
        metrics_path,
        epoch=epochs + epoch_offset,
        step=global_step,
        loss=avg_loss,
        lr=effective_lr,
        progress=1.0,
        phase="completed",
        message="Treino Qwen-Image-2.1 finalizado com sucesso!",
    )
    print(f"Treino Qwen-Image-2.1 finalizado com sucesso! Checkpoint salvo em: {final_adapter_file}", flush=True)


class QwenImageTrainer(BaseModelTrainer):
    """Trainer de difusão para Qwen-Image-2.1 (7B Single-Stream DiT)."""

    def train(self, cfg: dict[str, Any], output: Path) -> None:
        if is_mock():
            _mock_train(cfg, output)
        else:
            _real_train_qwen_image(cfg, output)
