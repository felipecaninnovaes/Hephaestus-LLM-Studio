"""Pipeline real de treino LoRA para Stable Diffusion XL (SDXL 1.0) na GPU."""

from __future__ import annotations

import math
import os
import shutil
from pathlib import Path
from typing import Any

from trainer_difusao.common import (
    _die,
    _emit_metric,
    _resolve_output_name,
    _save_lora_safetensors,
    _setup_cache_dir,
)
from trainer_difusao.dataset import DiffusionDataset
from trainer_difusao.models.base import BaseModelTrainer
from trainer_difusao.optimizers import _create_lr_scheduler, _create_optimizer


def _compute_sdxl_embeddings(
    prompts: list[str],
    tokenizer_one: Any,
    tokenizer_two: Any,
    text_encoder_one: Any,
    text_encoder_two: Any,
    device: Any,
) -> tuple[Any, Any]:
    """Codifica texto para SDXL combinando os dois encoders CLIP e extraindo pooled embeddings."""
    import torch

    with torch.no_grad():
        tokens_one = tokenizer_one(
            prompts,
            padding="max_length",
            max_length=tokenizer_one.model_max_length,
            truncation=True,
            return_tensors="pt",
        ).input_ids.to(device)
        enc_one = text_encoder_one(tokens_one, output_hidden_states=True)
        hidden_states_one = enc_one.hidden_states[-2]

        tokens_two = tokenizer_two(
            prompts,
            padding="max_length",
            max_length=tokenizer_two.model_max_length,
            truncation=True,
            return_tensors="pt",
        ).input_ids.to(device)
        enc_two = text_encoder_two(tokens_two, output_hidden_states=True)
        hidden_states_two = enc_two.hidden_states[-2]
        pooled_embeds = enc_two.text_embeds

        # Concatena canais de embedding (768 + 1280 = 2048)
        prompt_embeds = torch.concat([hidden_states_one, hidden_states_two], dim=-1)

    return prompt_embeds, pooled_embeds


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

        try:
            pipe = StableDiffusionXLPipeline(
                vae=vae,
                text_encoder=text_encoder_one,
                text_encoder_2=text_encoder_two,
                tokenizer=tokenizer_one,
                tokenizer_2=tokenizer_two,
                unet=unet,
                scheduler=noise_scheduler,
            )
            pipe.set_progress_bar_config(disable=True)
            generator = torch.Generator(
                device="cuda" if torch.cuda.is_available() else "cpu"
            ).manual_seed(seed)
            with torch.inference_mode():
                latents = pipe(
                    prompt,
                    generator=generator,
                    num_inference_steps=20,
                    guidance_scale=7.0,
                    output_type="latent",
                ).images
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


def _real_train_sdxl(cfg: dict[str, Any], output: Path) -> None:
    """Pipeline real de treino LoRA para Stable Diffusion XL (SDXL 1.0) na GPU."""
    output.mkdir(parents=True, exist_ok=True)
    metrics_path = output / "metrics.jsonl"
    if metrics_path.exists():
        metrics_path.unlink()

    hub_cache = _setup_cache_dir()

    try:
        import torch
        import torch.nn.functional as F
        from diffusers import AutoencoderKL, DDPMScheduler, UNet2DConditionModel
        from peft import LoraConfig, get_peft_model
        from torch.utils.data import DataLoader
        from transformers import (
            AutoTokenizer,
            CLIPTextModel,
            CLIPTextModelWithProjection,
        )
    except ImportError as e:
        _die(f"Dependência ausente para treino real SDXL: {e}")

    if not torch.cuda.is_available():
        _die("CUDA não disponível para treino real de difusão (ENGINE_MOCK=0)")

    device = torch.device("cuda")

    seed = int(cfg.get("seed", 42))
    model_id = cfg.get("model_id") or "stabilityai/stable-diffusion-xl-base-1.0"
    dataset_path = Path(cfg.get("dataset_path", "/datasets"))
    lora_cfg = cfg.get("lora", {})
    epochs = int(lora_cfg.get("epochs", 10))
    batch_size = int(lora_cfg.get("batch_size", 1))
    learning_rate = float(lora_cfg.get("learning_rate", 1e-4))
    rank = int(lora_cfg.get("rank", 16))
    alpha = int(lora_cfg.get("alpha", 16))
    trigger_word = str(lora_cfg.get("trigger_word", ""))
    base_name = _resolve_output_name(cfg)

    samples_cfg = cfg.get("samples", {})
    sample_prompt = str(samples_cfg.get("prompt", "") or "").strip()
    sample_interval = int(samples_cfg.get("interval", 1))
    sample_seed = int(samples_cfg.get("seed", seed))

    resolution = int(lora_cfg.get("resolution", 1024))
    grad_accum = max(1, int(lora_cfg.get("gradient_accumulation_steps", 1)))
    optimizer_name = str(lora_cfg.get("optimizer", "adamw8bit"))
    lr_scheduler_name = str(lora_cfg.get("lr_scheduler", "cosine"))
    lr_warmup_steps = int(lora_cfg.get("lr_warmup_steps", 0))
    mixed_precision = str(lora_cfg.get("mixed_precision", "fp16")).lower().strip()
    target_dtype = (
        torch.bfloat16
        if (mixed_precision == "bf16" and torch.cuda.is_bf16_supported())
        else torch.float16
    )
    quantization = str(
        lora_cfg.get("quantization") or cfg.get("quantization") or "none"
    ).lower().strip()

    _emit_metric(
        metrics_path,
        epoch=0,
        step=1,
        progress=0.01,
        phase="init",
        message=f"Inicializando treino SDXL: {model_id}...",
    )

    print(
        f"Carregando modelos base SDXL ({model_id}) [cache: {hub_cache}, res: {resolution}, dtype: {target_dtype}]...",
        flush=True,
    )
    _emit_metric(
        metrics_path,
        epoch=0,
        step=2,
        progress=0.03,
        phase="loading_models",
        message=f"Baixando e carregando componentes SDXL ({model_id})...",
    )
    tokenizer_one = AutoTokenizer.from_pretrained(
        model_id, subfolder="tokenizer", use_fast=False, cache_dir=hub_cache
    )
    tokenizer_two = AutoTokenizer.from_pretrained(
        model_id, subfolder="tokenizer_2", use_fast=False, cache_dir=hub_cache
    )
    text_encoder_one = CLIPTextModel.from_pretrained(
        model_id,
        subfolder="text_encoder",
        torch_dtype=target_dtype,
        cache_dir=hub_cache,
    ).to(device)
    text_encoder_two = CLIPTextModelWithProjection.from_pretrained(
        model_id,
        subfolder="text_encoder_2",
        torch_dtype=target_dtype,
        cache_dir=hub_cache,
    ).to(device)
    # VAE em float32 para prevenir underflow/overflow numérico (NaN) conhecido no SDXL em fp16
    vae = AutoencoderKL.from_pretrained(
        model_id, subfolder="vae", torch_dtype=torch.float32, cache_dir=hub_cache
    ).to(device)
    unet = UNet2DConditionModel.from_pretrained(
        model_id, subfolder="unet", torch_dtype=target_dtype, cache_dir=hub_cache
    ).to(device)
    noise_scheduler = DDPMScheduler.from_pretrained(
        model_id, subfolder="scheduler", cache_dir=hub_cache
    )

    vae.requires_grad_(False)
    text_encoder_one.requires_grad_(False)
    text_encoder_two.requires_grad_(False)
    unet.requires_grad_(False)

    unet.enable_gradient_checkpointing()

    lora_config = LoraConfig(
        r=rank,
        lora_alpha=alpha,
        init_lora_weights="gaussian",
        target_modules=["to_k", "to_q", "to_v", "to_out.0"],
    )
    unet = get_peft_model(unet, lora_config)

    _emit_metric(
        metrics_path,
        epoch=0,
        step=3,
        progress=0.06,
        phase="setup_lora",
        message=f"Adaptadores LoRA injetados no UNet SDXL (rank={rank}, alpha={alpha}).",
    )

    optimizer = _create_optimizer(unet, optimizer_name, learning_rate)

    dataset = DiffusionDataset(
        dataset_path, resolution=resolution, trigger_word=trigger_word
    )
    dataloader = DataLoader(
        dataset, batch_size=batch_size, shuffle=True, drop_last=False
    )

    total_train_steps = max(1, (len(dataloader) * epochs) // grad_accum)
    lr_scheduler = _create_lr_scheduler(
        optimizer, lr_scheduler_name, total_train_steps, lr_warmup_steps
    )

    _emit_metric(
        metrics_path,
        epoch=0,
        step=4,
        progress=0.08,
        phase="dataset_ready",
        message=f"Dataset pronto: {len(dataset)} imagens.",
    )

    # Time IDs padrão para SDXL dimensionados pela resolução configurada
    add_time_ids = torch.tensor(
        [[resolution, resolution, 0, 0, resolution, resolution]],
        dtype=target_dtype,
        device=device,
    )

    # Amostra baseline (Época 0) pré-treino
    if sample_prompt:
        _emit_metric(
            metrics_path,
            epoch=0,
            step=5,
            progress=0.09,
            phase="generating_baseline_sample",
            message=f"Gerando amostra baseline pré-treino (Época 0): '{sample_prompt[:40]}...'",
        )
        sample_baseline_file = output / "samples" / "sample_epoch_000.png"
        _generate_sample_sdxl(
            unet,
            vae,
            text_encoder_one,
            text_encoder_two,
            tokenizer_one,
            tokenizer_two,
            noise_scheduler,
            sample_prompt,
            sample_baseline_file,
            seed=sample_seed,
        )
        _emit_metric(
            metrics_path,
            epoch=0,
            step=6,
            progress=0.10,
            phase="baseline_ready",
            message="Amostra baseline SDXL gerada com sucesso (Época 0).",
        )

    _emit_metric(
        metrics_path,
        epoch=0,
        step=7,
        progress=0.10,
        phase="training_started",
        message=f"Iniciando loop de treino SDXL: {epochs} épocas, {total_train_steps} passos totais.",
    )

    print(
        f"Iniciando treino LoRA SDXL: {epochs} épocas, {len(dataset)} imagens, res={resolution}, "
        f"rank={rank}, alpha={alpha}, lr={learning_rate}, grad_accum={grad_accum}, opt={optimizer_name}, "
        f"scheduler={lr_scheduler_name}",
        flush=True,
    )
    global_step = 0
    safe_avg_loss = None

    for epoch in range(1, epochs + 1):
        unet.train()
        epoch_loss = 0.0
        steps_in_epoch = 0

        for batch in dataloader:
            pixel_values = batch["pixel_values"].to(device, dtype=torch.float32)
            prompts = batch["prompt"]
            cur_bs = pixel_values.shape[0]

            with torch.no_grad():
                latents = (
                    vae.encode(pixel_values).latent_dist.sample()
                    * vae.config.scaling_factor
                ).to(dtype=target_dtype)

            noise = torch.randn_like(latents)
            timesteps = torch.randint(
                0, noise_scheduler.config.num_train_timesteps, (cur_bs,), device=device
            ).long()
            noisy_latents = noise_scheduler.add_noise(latents, noise, timesteps)

            prompt_embeds, pooled_prompt_embeds = _compute_sdxl_embeddings(
                prompts,
                tokenizer_one,
                tokenizer_two,
                text_encoder_one,
                text_encoder_two,
                device,
            )

            added_cond_kwargs = {
                "text_embeds": pooled_prompt_embeds,
                "time_ids": add_time_ids.repeat(cur_bs, 1),
            }

            model_pred = unet(
                noisy_latents,
                timesteps,
                prompt_embeds,
                added_cond_kwargs=added_cond_kwargs,
            ).sample
            loss = F.mse_loss(model_pred.float(), noise.float(), reduction="mean")

            cur_loss_raw = loss.item()
            loss = loss / grad_accum
            loss.backward()

            steps_in_epoch += 1
            if steps_in_epoch % grad_accum == 0 or steps_in_epoch == len(dataloader):
                torch.nn.utils.clip_grad_norm_(unet.parameters(), 1.0)
                optimizer.step()
                if lr_scheduler is not None:
                    lr_scheduler.step()
                optimizer.zero_grad()

            global_step += 1
            if not math.isnan(cur_loss_raw) and not math.isinf(cur_loss_raw):
                epoch_loss += cur_loss_raw

            effective_lr = (
                lr_scheduler.get_last_lr()[0] if lr_scheduler else learning_rate
            )

            # Emite métricas intermediárias por step para streaming em tempo real
            if global_step % 5 == 0 or steps_in_epoch == len(dataloader):
                safe_loss = (
                    None
                    if (math.isnan(cur_loss_raw) or math.isinf(cur_loss_raw))
                    else round(cur_loss_raw, 4)
                )
                current_progress = round(
                    min(
                        0.99,
                        max(
                            0.10,
                            0.10 + 0.89 * (global_step / max(1, total_train_steps)),
                        ),
                    ),
                    4,
                )
                _emit_metric(
                    metrics_path,
                    epoch=epoch,
                    step=global_step,
                    loss=safe_loss,
                    lr=effective_lr,
                    progress=current_progress,
                    phase="training",
                    message=f"Época {epoch}/{epochs} · Step {global_step}/{total_train_steps} · Loss: {safe_loss}",
                )
                print(
                    f"[SDXL] Época {epoch}/{epochs} · Step {global_step} · Loss: {cur_loss_raw:.4f} · LR: {effective_lr:.2e}",
                    flush=True,
                )

        avg_loss = (
            round(epoch_loss / max(1, steps_in_epoch), 4)
            if steps_in_epoch > 0
            else 0.0
        )
        safe_avg_loss = (
            None if (math.isnan(avg_loss) or math.isinf(avg_loss)) else avg_loss
        )
        effective_lr = (
            lr_scheduler.get_last_lr()[0] if lr_scheduler else learning_rate
        )
        epoch_progress = round(
            min(0.99, max(0.10, 0.10 + 0.89 * (epoch / epochs))), 4
        )
        _emit_metric(
            metrics_path,
            epoch=epoch,
            step=global_step,
            loss=safe_avg_loss,
            lr=effective_lr,
            progress=epoch_progress,
            phase="epoch_complete",
            message=f"Época {epoch}/{epochs} concluída · Loss Médio: {safe_avg_loss}",
        )
        print(
            f"[SDXL] Época {epoch}/{epochs} concluída - Step {global_step} - Loss Médio: {avg_loss}",
            flush=True,
        )

        # Salva checkpoint da época
        checkpoints_dir = output / "checkpoints"
        checkpoints_dir.mkdir(parents=True, exist_ok=True)
        ckpt_file = checkpoints_dir / f"{base_name}_epoch_{epoch:03d}.safetensors"
        _save_lora_safetensors(
            unet,
            ckpt_file,
            metadata={
                "format": "pt",
                "framework": "diffusers",
                "model_type": "lora",
                "base_model": "sdxl",
                "lora_rank": str(rank),
                "lora_alpha": str(alpha),
                "trigger_word": trigger_word,
                "quantization": quantization,
                "epoch": str(epoch),
            },
        )

        if (
            sample_prompt
            and sample_interval > 0
            and (epoch % sample_interval == 0 or epoch == epochs)
        ):
            sample_file = output / "samples" / f"sample_epoch_{epoch:03d}.png"
            _generate_sample_sdxl(
                unet,
                vae,
                text_encoder_one,
                text_encoder_two,
                tokenizer_one,
                tokenizer_two,
                noise_scheduler,
                sample_prompt,
                sample_file,
                seed=sample_seed,
            )

    final_adapter_file = output / f"{base_name}.safetensors"
    metadata = {
        "format": "pt",
        "framework": "diffusers",
        "model_type": "lora",
        "base_model": "sdxl",
        "lora_rank": str(rank),
        "lora_alpha": str(alpha),
        "trigger_word": trigger_word,
        "quantization": quantization,
    }
    _save_lora_safetensors(unet, final_adapter_file, metadata)
    if base_name != "adapter":
        shutil.copy2(final_adapter_file, output / "adapter.safetensors")

    _emit_metric(
        metrics_path,
        epoch=epochs,
        step=global_step,
        loss=safe_avg_loss,
        lr=effective_lr,
        progress=1.0,
        phase="completed",
        message="Treino SDXL finalizado com sucesso!",
    )
    print(f"Treino SDXL finalizado com sucesso! Checkpoint salvo em: {final_adapter_file}")


class SDXLTrainer(BaseModelTrainer):
    """Trainer de difusão para Stable Diffusion XL (SDXL 1.0)."""

    def train(self, cfg: dict[str, Any], output: Path) -> None:
        _real_train_sdxl(cfg, output)
