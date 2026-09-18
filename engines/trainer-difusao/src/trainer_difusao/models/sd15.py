"""Pipeline real de treino LoRA para Stable Diffusion 1.5 na GPU."""

import math
import os
import random
import shutil
from pathlib import Path
from typing import Any

from trainer_difusao.common import (
    _cached_encode,
    _cycling_batches,
    _die,
    _emit_metric,
    _load_lora_weights,
    _precompute_text_cache,
    _resolve_output_name,
    _save_lora_safetensors,
    _setup_cache_dir,
    _validate_train_aux,
    TextEmbedsCache,
    _prune_checkpoints,
    _cleanup_cuda,
)
from trainer_difusao.dataset import DiffusionDataset, build_dataloader
from trainer_difusao.models.base import BaseModelTrainer
from trainer_difusao.optimizers import _create_lr_scheduler, _create_optimizer


def _generate_sample_sd15(
    unet: Any,
    vae: Any,
    text_encoder: Any,
    tokenizer: Any,
    noise_scheduler: Any,
    prompt: str,
    output_path: Path,
    seed: int = 42,
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

        try:
            pipe = StableDiffusionPipeline(
                vae=vae,
                text_encoder=text_encoder,
                tokenizer=tokenizer,
                unet=unet,
                scheduler=noise_scheduler,
                safety_checker=None,
                feature_extractor=None,
                requires_safety_checker=False,
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
                    guidance_scale=7.5,
                    output_type="latent",
                ).images
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


def _real_train_sd15(cfg: dict[str, Any], output: Path) -> None:
    """Pipeline real de treino LoRA para Stable Diffusion 1.5 na GPU."""
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
        from transformers import CLIPTextModel, CLIPTokenizer
    except ImportError as e:
        _die(f"Dependência ausente para treino real SD 1.5: {e}")

    if not torch.cuda.is_available():
        _die("CUDA não disponível para treino real de difusão (ENGINE_MOCK=0)")

    device = torch.device("cuda")

    aux = _validate_train_aux(cfg, quant_default="none")
    control_dataset_path = aux["control_dataset_path"]
    control_ratio = aux["control_ratio"]
    cache_text_embeddings = aux["cache_text_embeddings"]

    seed = int(cfg.get("seed", 42))
    model_id = cfg.get("model_id") or "runwayml/stable-diffusion-v1-5"
    dataset_path = Path(cfg.get("dataset_path", "/datasets"))
    lora_cfg = cfg.get("lora", {})
    epochs = int(lora_cfg.get("epochs", 10))
    batch_size = int(lora_cfg.get("batch_size", 1))
    learning_rate = float(lora_cfg.get("learning_rate", 1e-4))
    rank = int(lora_cfg.get("rank", 16))
    alpha = int(lora_cfg.get("alpha", 16))
    trigger_word = str(lora_cfg.get("trigger_word", ""))
    base_name = _resolve_output_name(cfg)
    # feat/pesos-custom-flux2: treino custom sd15 via from_single_file do UNet —
    # mecânico (mesmo padrão da geração); demais componentes do repo oficial.
    raw_custom_cp = cfg.get("custom_checkpoint_path")
    custom_checkpoint_path: str | None = None
    if raw_custom_cp:
        if not isinstance(raw_custom_cp, str) or not raw_custom_cp.strip():
            _die("custom_checkpoint_path deve ser uma string não vazia.")
        custom_checkpoint_path = raw_custom_cp.strip()
    raw_enc = cfg.get("text_encoder_path")
    if raw_enc:
        _die(
            "text_encoder_path só é suportado com arch flux-2-klein-4b "
            "(treino sd15 não usa encoder custom)."
        )

    samples_cfg = cfg.get("samples", {})
    sample_prompt = str(samples_cfg.get("prompt", "") or "").strip()
    sample_interval = int(samples_cfg.get("interval", 1))
    sample_seed = int(samples_cfg.get("seed", seed))

    resolution = int(lora_cfg.get("resolution", 512))
    enable_bucket = bool(lora_cfg.get("enable_bucket", True))
    grad_accum = max(1, int(lora_cfg.get("gradient_accumulation_steps", 1)))
    optimizer_name = str(lora_cfg.get("optimizer", "adamw8bit"))
    lr_scheduler_name = str(lora_cfg.get("lr_scheduler", "cosine"))
    lr_warmup_steps = int(lora_cfg.get("lr_warmup_steps", 0))

    checkpoint_interval = max(
        1, int(cfg.get("checkpoint_interval") or lora_cfg.get("checkpoint_interval") or 1)
    )
    epoch_offset = max(
        0, int(cfg.get("epoch_offset") or lora_cfg.get("epoch_offset") or 0)
    )
    weights_path = cfg.get("weights_path")

    mixed_precision = str(lora_cfg.get("mixed_precision", "fp16")).lower().strip()
    target_dtype = (
        torch.bfloat16
        if (mixed_precision == "bf16" and torch.cuda.is_bf16_supported())
        else torch.float16
    )
    # Hoje o treino SD 1.5/SDXL carrega o modelo base em precisão plena (sem
    # BitsAndBytesConfig): o nível é validado/normalizado e registrado na
    # telemetry + metadados do safetensors. 2bit/6bit (torchao intx) exigem CUDA
    # e seguem o mesmo caminho de aplicação do Flux quando o ponto de aplicação
    # existir — nunca degradação silenciosa.
    quantization = aux["quantization"] or "none"

    _emit_metric(
        metrics_path,
        epoch=0,
        step=1,
        progress=0.01,
        phase="init",
        message=f"Inicializando treino SD 1.5: {model_id}...",
    )

    print(
        f"Carregando modelos base SD 1.5 ({model_id}) [cache: {hub_cache}, res: {resolution}, dtype: {target_dtype}]...",
        flush=True,
    )
    _emit_metric(
        metrics_path,
        epoch=0,
        step=2,
        progress=0.03,
        phase="loading_models",
        message=f"Baixando e carregando componentes SD 1.5 ({model_id})...",
    )
    tokenizer = CLIPTokenizer.from_pretrained(
        model_id, subfolder="tokenizer", cache_dir=hub_cache
    )
    text_encoder = CLIPTextModel.from_pretrained(
        model_id,
        subfolder="text_encoder",
        torch_dtype=target_dtype,
        cache_dir=hub_cache,
    ).to(device)
    # VAE em float32 para prevenir underflow/overflow numérico (NaN)
    vae = AutoencoderKL.from_pretrained(
        model_id, subfolder="vae", torch_dtype=torch.float32, cache_dir=hub_cache
    ).to(device)
    if custom_checkpoint_path:
        try:
            unet = UNet2DConditionModel.from_single_file(
                custom_checkpoint_path, torch_dtype=target_dtype
            ).to(device)
        except Exception as exc:
            _die(
                f"Falha ao carregar checkpoint sd15 custom "
                f"({custom_checkpoint_path}): layout não reconhecido ({exc})"
            )
        print(
            f"[SD15] Checkpoint custom aplicado ao UNet: {custom_checkpoint_path}",
            flush=True,
        )
    else:
        unet = UNet2DConditionModel.from_pretrained(
            model_id, subfolder="unet", torch_dtype=target_dtype, cache_dir=hub_cache
        ).to(device)
    noise_scheduler = DDPMScheduler.from_pretrained(
        model_id, subfolder="scheduler", cache_dir=hub_cache
    )

    # Congela VAE e Text Encoder
    vae.requires_grad_(False)
    text_encoder.requires_grad_(False)
    unet.requires_grad_(False)

    # Gradient checkpointing economiza ~50% VRAM
    unet.enable_gradient_checkpointing()

    # Injeta LoRA no UNet
    lora_config = LoraConfig(
        r=rank,
        lora_alpha=alpha,
        init_lora_weights="gaussian",
        target_modules=["to_k", "to_q", "to_v", "to_out.0"],
    )
    unet = get_peft_model(unet, lora_config)
    if weights_path:
        _load_lora_weights(unet, weights_path)

    _emit_metric(
        metrics_path,
        epoch=0,
        step=3,
        progress=0.06,
        phase="setup_lora",
        message=f"Adaptadores LoRA injetados no UNet (rank={rank}, alpha={alpha}).",
    )

    optimizer = _create_optimizer(unet, optimizer_name, learning_rate)

    dataset = DiffusionDataset(
        dataset_path,
        resolution=resolution,
        trigger_word=trigger_word,
        enable_bucket=enable_bucket,
    )
    dataloader = build_dataloader(dataset, batch_size, seed=seed)

    # Dataset de controle (prior-preservation): mesma resolução/bucketing do
    # principal, caption VAZIA (sem trigger word). Intercalação por step via
    # _cycling_batches: com prob. control_ratio usa-se o batch de controle no
    # loss do mesmo step (mesma pipeline de ruído/loss) em vez do principal.
    control_dataset = None
    control_loader = None
    control_iter = None
    control_n = 0
    if control_dataset_path is not None:
        control_dataset = DiffusionDataset(
            control_dataset_path,
            resolution=resolution,
            trigger_word="",
            enable_bucket=enable_bucket,
            empty_captions=True,
        )
        control_loader = build_dataloader(control_dataset, batch_size, seed=seed)
        control_iter = _cycling_batches(control_loader)
        control_n = len(control_dataset)

    steps_per_epoch = math.ceil(len(dataloader) / grad_accum)
    total_train_steps = max(1, steps_per_epoch * epochs)
    lr_scheduler = _create_lr_scheduler(
        optimizer, lr_scheduler_name, total_train_steps, lr_warmup_steps
    )

    # Cache de text embeddings (SD: saída do CLIP text encoder), pré-computado
    # UMA vez no início; miss → on-the-fly + warm; falha → segue sem cache.
    text_cache = TextEmbedsCache(output, cache_text_embeddings)
    if cache_text_embeddings:
        with torch.no_grad():
            _precompute_text_cache(
                text_cache,
                [c for _, c in dataset.samples]
                + ([c for _, c in control_dataset.samples] if control_dataset else []),
                lambda caps: {
                    "hidden": text_encoder(
                        tokenizer(
                            caps,
                            padding="max_length",
                            max_length=tokenizer.model_max_length,
                            truncation=True,
                            return_tensors="pt",
                        ).input_ids.to(device)
                    )[0].to(dtype=target_dtype)
                },
            )

    _emit_metric(
        metrics_path,
        epoch=0,
        step=4,
        progress=0.08,
        phase="dataset_ready",
        message=f"Dataset pronto: {len(dataset)} imagens.",
    )
    print(
        f"[SD 1.5] Treino: dataset={len(dataset)} imagens, "
        f"control_dataset_images={control_n}, control_ratio={control_ratio}, "
        f"cache_text_embeddings={cache_text_embeddings}, quantization={quantization}",
        flush=True,
    )

    # Amostra baseline (Época 0) pré-treino (apenas se não estiver retomando)
    if sample_prompt and epoch_offset == 0:
        _emit_metric(
            metrics_path,
            epoch=0,
            step=5,
            progress=0.09,
            phase="generating_baseline_sample",
            message=f"Gerando amostra baseline pré-treino (Época 0): '{sample_prompt[:40]}...'",
        )
        sample_baseline_file = output / "samples" / "sample_epoch_000.png"
        _generate_sample_sd15(
            unet,
            vae,
            text_encoder,
            tokenizer,
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
            message="Amostra baseline gerada com sucesso (Época 0).",
        )

    _emit_metric(
        metrics_path,
        epoch=epoch_offset,
        step=7,
        progress=0.10,
        phase="training_started",
        message=f"Iniciando loop de treino SD 1.5: {epochs} épocas (offset={epoch_offset}), {total_train_steps} passos totais.",
    )

    print(
        f"Iniciando treino LoRA SD 1.5: {epochs} épocas (offset={epoch_offset}), {len(dataset)} imagens, res={resolution}, "
        f"rank={rank}, alpha={alpha}, lr={learning_rate}, grad_accum={grad_accum}, opt={optimizer_name}, "
        f"scheduler={lr_scheduler_name}",
        flush=True,
    )
    global_step = 0
    safe_avg_loss = None

    def _encode_sd15(caps: list[str]) -> dict[str, Any]:
        inputs = tokenizer(
            caps,
            padding="max_length",
            max_length=tokenizer.model_max_length,
            truncation=True,
            return_tensors="pt",
        ).input_ids.to(device)
        with torch.no_grad():
            hidden = text_encoder(inputs)[0].to(dtype=target_dtype)
        return {"hidden": hidden}

    for epoch_idx in range(1, epochs + 1):
        epoch = epoch_idx + epoch_offset
        unet.train()
        epoch_loss = 0.0
        steps_in_epoch = 0

        for batch in dataloader:
            # Prior-preservation: com prob. control_ratio troca-se o batch pelo
            # de controle (regularização, captions vazias) no mesmo step.
            if control_iter is not None and random.random() < control_ratio:
                batch = next(control_iter)
            pixel_values = batch["pixel_values"].to(device, dtype=torch.float32)
            captions = batch["prompt"]
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

            encoder_hidden_states = _cached_encode(captions, _encode_sd15, text_cache)["hidden"].to(
                device, dtype=target_dtype
            )

            model_pred = unet(
                noisy_latents,
                timesteps,
                encoder_hidden_states,
                return_dict=False,
            )[0]

            loss = F.mse_loss(model_pred.float(), noise.float(), reduction="mean")

            cur_loss_raw = loss.item()
            loss = loss / grad_accum
            loss.backward()

            steps_in_epoch += 1
            is_accum_step = (steps_in_epoch % grad_accum == 0) or (steps_in_epoch == len(dataloader))
            if is_accum_step:
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
            if is_accum_step and (global_step % 5 == 0 or steps_in_epoch == len(dataloader)):
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
                    message=f"Época {epoch}/{epochs + epoch_offset} · Step {global_step}/{total_train_steps} · Loss: {safe_loss}",
                )
                print(
                    f"[SD 1.5] Época {epoch}/{epochs + epoch_offset} · Step {global_step}/{total_train_steps} · Loss: {cur_loss_raw:.4f} · LR: {effective_lr:.2e}",
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
            min(0.99, max(0.10, 0.10 + 0.89 * (epoch_idx / epochs))), 4
        )
        _emit_metric(
            metrics_path,
            epoch=epoch,
            step=global_step,
            loss=safe_avg_loss,
            lr=effective_lr,
            progress=epoch_progress,
            phase="epoch_complete",
            message=f"Época {epoch}/{epochs + epoch_offset} concluída · Loss Médio: {safe_avg_loss}",
        )
        print(
            f"[SD 1.5] Época {epoch}/{epochs + epoch_offset} concluída - Step {global_step} - Loss Médio: {avg_loss}",
            flush=True,
        )

        # Salva checkpoint da época respeitando checkpoint_interval
        if epoch_idx % checkpoint_interval == 0 or epoch_idx == epochs:
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
                    "base_model": "sd15",
                    "lora_rank": str(rank),
                    "lora_alpha": str(alpha),
                    "trigger_word": trigger_word,
                    "quantization": quantization,
                    "epoch": str(epoch),
                },
            )
            _prune_checkpoints(checkpoints_dir, keep_last_n=2)

        _cleanup_cuda()

        if (
            sample_prompt
            and sample_interval > 0
            and (epoch_idx % sample_interval == 0 or epoch_idx == epochs)
        ):
            sample_file = output / "samples" / f"sample_epoch_{epoch:03d}.png"
            _generate_sample_sd15(
                unet,
                vae,
                text_encoder,
                tokenizer,
                noise_scheduler,
                sample_prompt,
                sample_file,
                seed=sample_seed,
            )

    # Salva adapter final com nome semântico configurado
    final_adapter_file = output / f"{base_name}.safetensors"
    metadata = {
        "format": "pt",
        "framework": "diffusers",
        "model_type": "lora",
        "base_model": "sd15",
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
        message="Treino SD 1.5 finalizado com sucesso!",
    )
    print(f"Treino SD 1.5 finalizado com sucesso! Checkpoint salvo em: {final_adapter_file}")


class SD15Trainer(BaseModelTrainer):
    """Trainer de difusão para Stable Diffusion 1.5."""

    def train(self, cfg: dict[str, Any], output: Path) -> None:
        _real_train_sd15(cfg, output)
