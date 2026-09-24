"""Pipeline de treino LoRA para Qwen-Image-2.1."""

from __future__ import annotations

import copy
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
from transformers import AutoTokenizer
from trainer_difusao.models.qwen_pkg import _generate_sample_qwen


def _real_train_qwen_image(cfg: dict[str, Any], output: Path | str) -> None:
    """Pipeline real de treino LoRA para Qwen-Image-2.1 na GPU."""
    output_path = Path(output)
    output_path.mkdir(parents=True, exist_ok=True)
    metrics_path = output_path / "metrics.jsonl"
    checkpoints_dir = output_path / "checkpoints"
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

    samples_cfg = cfg.get("samples", {})
    sample_prompt = str(samples_cfg.get("prompt", "") or "").strip()
    sample_interval = int(samples_cfg.get("interval", 1))
    sample_seed = int(samples_cfg.get("seed", seed))
    sample_embeds: dict[str, Any] | None = None

    checkpoint_interval = max(1, int(cfg.get("checkpoint_interval") or lora_cfg.get("checkpoint_interval") or 1))
    epoch_offset = max(0, int(cfg.get("epoch_offset") or lora_cfg.get("epoch_offset") or 0))
    grad_accum = max(1, int(lora_cfg.get("gradient_accumulation_steps", 1)))
    batch_size = max(1, int(lora_cfg.get("batch_size", 1)))
    resolution = int(cfg.get("resolution", 1024))

    raw_dataset_path = cfg.get("dataset_path")
    if not raw_dataset_path:
        _die("dataset_path não configurado no payload.")
    dataset_path = Path(raw_dataset_path)
    if not dataset_path.exists():
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

    # 2. Resolução de classes Diffusers para Qwen-Image
    PipelineCls = getattr(diffusers, "QwenImage21Pipeline", getattr(diffusers, "QwenImagePipeline", None))
    VaeCls = getattr(diffusers, "AutoencoderKLQwenImage21", getattr(diffusers, "AutoencoderKLQwenImage", None))
    TransformerCls = getattr(
        diffusers, "QwenImage21Transformer2DModel", getattr(diffusers, "QwenImageTransformer2DModel", None)
    )

    if VaeCls is None:
        _die("AutoencoderKLQwenImage21 não disponível no diffusers.")
    if TransformerCls is None:
        _die("QwenImage21Transformer2DModel não está disponível no diffusers.")

    # 3. Pré-computação de Text Embeddings (Text Encoder descarregado em seguida para poupar VRAM)
    prompt_cache: dict[str, tuple[torch.Tensor, torch.Tensor | None]] = {}
    unique_prompts = list({cap for _, cap in dataset.samples})

    if PipelineCls is not None:
        print(f"[DIFFUSION-TRAIN] Pré-computando embeddings de texto ({len(unique_prompts)} prompts) na CPU...", flush=True)
        try:
            tokenizer = AutoTokenizer.from_pretrained(
                model_repo,
                subfolder="processor",
                cache_dir=hub_cache,
                token=hf_token,
            )
            text_pipeline = PipelineCls.from_pretrained(
                model_repo,
                tokenizer=tokenizer,
                vae=None,
                transformer=None,
                torch_dtype=torch.float32,
                cache_dir=hub_cache,
                token=hf_token,
            )
            # Mantém estritamente na CPU do host para preservar 100% da VRAM da GPU
            with torch.no_grad():
                for p_text in unique_prompts:
                    encoded = text_pipeline.encode_prompt(p_text)
                    if len(encoded) == 3:
                        pe, pe_mask, ipm = encoded
                    else:
                        pe, pe_mask = encoded
                        ipm = None
                    prompt_cache[p_text] = (
                        pe.cpu(),
                        pe_mask.cpu() if pe_mask is not None else None,
                        ipm.cpu() if ipm is not None else None,
                    )
                if sample_prompt:
                    encoded_sample = text_pipeline.encode_prompt(sample_prompt)
                    if len(encoded_sample) == 3:
                        sample_pe, sample_pe_mask, sample_ipm = encoded_sample
                    else:
                        sample_pe, sample_pe_mask = encoded_sample
                        sample_ipm = None
                    sample_embeds = {
                        "prompt_embeds": sample_pe.cpu(),
                        "prompt_embeds_mask": sample_pe_mask.cpu() if sample_pe_mask is not None else None,
                        "image_pad_mask": sample_ipm.cpu() if sample_ipm is not None else None,
                    }
            gc.collect()
            _cleanup_cuda()
        except Exception as exc:
            print(f"[DIFFUSION-TRAIN] Aviso: falha na pré-computação com pipeline na CPU: {exc}. Criando fallbacks sintéticos.", flush=True)

    # 4. Carrega VAE
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

    latents_mean = None
    latents_std = None
    if hasattr(vae.config, "latents_mean") and vae.config.latents_mean is not None:
        latents_mean = torch.tensor(vae.config.latents_mean).view(1, vae.config.z_dim, 1, 1, 1).to(device, dtype=target_dtype)
    if hasattr(vae.config, "latents_std") and vae.config.latents_std is not None:
        latents_std = (1.0 / torch.tensor(vae.config.latents_std)).view(1, vae.config.z_dim, 1, 1, 1).to(device, dtype=target_dtype)

    vae_scale_factor = 8
    if hasattr(vae, "temperal_downsample"):
        vae_scale_factor = 2 ** len(vae.temperal_downsample)

    # 5. Carrega Transformer com quantização 4-bit (se selecionada) e LoRA
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
        target_modules=["to_k", "to_q", "to_v", "to_out.0", "add_k_proj", "add_q_proj", "add_v_proj"],
    )
    transformer.add_adapter(lora_config)
    try:
        transformer.enable_gradient_checkpointing()
    except Exception:
        pass
    transformer.train()

    # 6. Otimizador e Scheduler
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

    # Scheduler para amostragem determinística de validação
    try:
        from diffusers import FlowMatchEulerDiscreteScheduler
        noise_scheduler = FlowMatchEulerDiscreteScheduler.from_pretrained(
            model_repo,
            subfolder="scheduler",
            cache_dir=hub_cache,
            token=hf_token,
        )
    except Exception:
        try:
            from diffusers import FlowMatchEulerDiscreteScheduler
            noise_scheduler = FlowMatchEulerDiscreteScheduler()
        except Exception:
            noise_scheduler = None

    # Amostra baseline Época 0 (se configurada e sem epoch_offset)
    if sample_prompt and epoch_offset == 0:
        _emit_metric(
            metrics_path,
            epoch=0,
            step=0,
            progress=0.04,
            phase="generating_baseline_sample",
            message=f"Gerando amostra baseline pré-treino (Época 0): '{sample_prompt[:40]}...'",
        )
        sample_baseline_file = output_path / "samples" / "sample_epoch_000.png"
        _generate_sample_qwen(
            transformer=transformer,
            vae=vae,
            scheduler=noise_scheduler,
            prompt=sample_prompt,
            output_path=sample_baseline_file,
            seed=sample_seed,
            resolution=resolution,
            metrics_path=metrics_path,
            epoch=0,
            sample_embeds=sample_embeds,
        )
        if sample_baseline_file.exists():
            _emit_metric(
                metrics_path,
                epoch=0,
                step=0,
                progress=0.05,
                phase="baseline_ready",
                message="Amostra baseline gerada com sucesso (Época 0).",
            )
        else:
            _emit_metric(
                metrics_path,
                epoch=0,
                step=0,
                progress=0.05,
                phase="baseline_failed",
                message="Falha ao gerar amostra baseline pré-treino.",
            )

    # 7. Loop de Treino Real
    for epoch_idx in range(1, epochs + 1):
        epoch = epoch_idx + epoch_offset
        transformer.train()
        epoch_loss = 0.0
        steps_in_epoch = 0
        optimizer.zero_grad()

        for batch in dataloader:
            pixel_values = batch["pixel_values"].to(device)
            captions = batch["prompt"]
            bsz = pixel_values.shape[0]

            # Qwen-Image VAE exige formato 5D: [B, C, F, H, W] com F=1
            if pixel_values.ndim == 4:
                pixel_values = pixel_values.unsqueeze(2)

            # Qwen-Image VAE possui in_channels=4 (suporte a RGBA). Se a entrada for RGB (3 canais),
            # concatena canal Alpha opaco (1.0) para suprir os 4 canais requeridos pelo encoder.
            if pixel_values.shape[1] == 3:
                alpha = torch.ones(
                    (pixel_values.shape[0], 1, *pixel_values.shape[2:]),
                    device=device,
                    dtype=pixel_values.dtype,
                )
                pixel_values = torch.cat([pixel_values, alpha], dim=1)
            # Codifica imagens com VAE em latents
            with torch.no_grad():
                latents = vae.encode(pixel_values.float()).latent_dist.sample()
                latents = latents.to(dtype=target_dtype)
                if latents_mean is not None and latents_std is not None:
                    latents = (latents - latents_mean) * latents_std

            # Flow matching noise scheduling
            noise = torch.randn_like(latents)
            u = torch.sigmoid(torch.randn(bsz, device=device))
            timesteps = u * 1000.0
            sigmas = (timesteps / 1000.0).view(-1, 1, 1, 1, 1).to(device, dtype=target_dtype)
            noisy_latents = (1.0 - sigmas) * latents + sigmas * noise
            target = noise - latents

            latent_h = latents.shape[3]
            latent_w = latents.shape[4]
            img_shapes = [(1, latent_h // 2, latent_w // 2)] * bsz

            # Empacota latents se helper de packing estiver disponível
            if PipelineCls is not None and hasattr(PipelineCls, "_pack_latents"):
                noisy_in = noisy_latents.permute(0, 2, 1, 3, 4)
                packed_noisy = PipelineCls._pack_latents(
                    noisy_in,
                    batch_size=bsz,
                    num_channels_latents=latents.shape[1],
                    height=latent_h,
                    width=latent_w,
                )
            else:
                packed_noisy = noisy_latents.flatten(2).transpose(1, 2)

            # Recupera prompt embeds do cache
            embed_list = []
            mask_list = []
            pad_mask_list = []
            for cap in captions:
                if cap in prompt_cache:
                    cached_val = prompt_cache[cap]
                    pe = cached_val[0]
                    pm = cached_val[1]
                    ipm = cached_val[2] if len(cached_val) > 2 else None
                    embed_list.append(pe.to(device, dtype=target_dtype))
                    if pm is not None:
                        mask_list.append(pm.to(device))
                    if ipm is not None:
                        pad_mask_list.append(ipm.to(device))
                else:
                    # Dummy embed se prompt_cache não cobriu
                    dummy_e = torch.zeros((1, 64, transformer.config.in_channels), device=device, dtype=target_dtype)
                    embed_list.append(dummy_e)

            batch_embeds = torch.cat(embed_list, dim=0) if embed_list else None
            batch_mask = torch.cat(mask_list, dim=0) if len(mask_list) == len(embed_list) else None

            # Monta kwargs do transformer dinamicamente de acordo com a assinatura do diffusers
            import inspect
            trans_sig = inspect.signature(transformer.forward)
            trans_kwargs: dict[str, Any] = {
                "hidden_states": packed_noisy,
                "encoder_hidden_states": batch_embeds,
                "timestep": timesteps / 1000.0,
                "return_dict": False,
            }
            if "encoder_hidden_states_mask" in trans_sig.parameters:
                trans_kwargs["encoder_hidden_states_mask"] = batch_mask

            if "img_mask" in trans_sig.parameters:
                # QwenImage21Transformer2DModel consome unpatched latents e img_mask para sequência conjunta
                trans_kwargs["img_shapes"] = [[(1, latent_h, latent_w)] for _ in range(bsz)]
                num_target_slots = (latent_h * latent_w) // 4
                if pad_mask_list and len(pad_mask_list) == len(embed_list):
                    base_pad_mask = torch.cat(pad_mask_list, dim=0)
                else:
                    base_pad_mask = torch.zeros((bsz, batch_embeds.shape[1]), device=device, dtype=torch.bool)
                target_slots = base_pad_mask.new_ones((bsz, num_target_slots), dtype=torch.bool)
                trans_kwargs["img_mask"] = torch.cat([base_pad_mask, target_slots], dim=1)
            else:
                trans_kwargs["img_shapes"] = [(1, latent_h // 2, latent_w // 2)] * bsz

            # Forward pass no Transformer
            pred = transformer(**trans_kwargs)[0]

            # O transformer opera sobre a sequência conjunta (texto + imagem).
            # Isola exclusivamente os tokens da imagem do target no final da sequência se saída for conjunta.
            if pred.shape[1] > packed_noisy.shape[1]:
                pred_img = pred[:, -packed_noisy.shape[1] :]
            else:
                pred_img = pred
            if PipelineCls is not None and hasattr(PipelineCls, "_unpack_latents"):
                pred = PipelineCls._unpack_latents(
                    pred_img,
                    latent_h * vae_scale_factor,
                    latent_w * vae_scale_factor,
                    vae_scale_factor,
                )
                pred_target = target
            else:
                pred = pred_img
                pred_target = target.flatten(2).transpose(1, 2)

            loss = F.mse_loss(pred.float(), pred_target.float(), reduction="mean")
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
            message=f"Época {epoch}/{epochs} concluída - Loss: {avg_loss}",
        )

        if sample_prompt and sample_interval > 0 and (epoch_idx % sample_interval == 0 or epoch_idx == epochs):
            _emit_metric(
                metrics_path,
                epoch=epoch,
                phase="generating_sample",
                message=f"Iniciando geração de amostra visual (Época {epoch})...",
                telemetry_only=True,
            )
            sample_file = output_path / "samples" / f"sample_epoch_{epoch:03d}.png"
            _generate_sample_qwen(
                transformer=transformer,
                vae=vae,
                scheduler=noise_scheduler,
                prompt=sample_prompt,
                output_path=sample_file,
                seed=sample_seed,
                resolution=resolution,
                metrics_path=metrics_path,
                epoch=epoch,
                sample_embeds=sample_embeds,
            )
            if sample_file.exists():
                _emit_metric(
                    metrics_path,
                    epoch=epoch,
                    phase="sample_ready",
                    message=f"Amostra visual da Época {epoch} pronta.",
                    telemetry_only=True,
                )
            else:
                _emit_metric(
                    metrics_path,
                    epoch=epoch,
                    phase="sample_failed",
                    message=f"Falha ao gerar amostra visual da Época {epoch}.",
                    telemetry_only=True,
                )

        if epoch_idx % checkpoint_interval == 0 or epoch_idx == epochs:
            ckpt_file = checkpoints_dir / f"{base_name}_epoch_{epoch:03d}.safetensors"
            _save_lora_safetensors(transformer, ckpt_file, {**metadata, "epoch": str(epoch)})

    # 8. Salva adaptador final
    final_adapter_file = output_path / f"{base_name}.safetensors"
    _save_lora_safetensors(transformer, final_adapter_file, metadata)
    if base_name != "adapter":
        shutil.copy2(final_adapter_file, output_path / "adapter.safetensors")

    _emit_metric(
        metrics_path,
        epoch=epochs,
        step=global_step,
        loss=avg_loss if "avg_loss" in locals() else 0.05,
        lr=learning_rate,
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
