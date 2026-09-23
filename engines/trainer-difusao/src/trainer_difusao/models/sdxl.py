"""Pipeline real de treino LoRA para Stable Diffusion XL (SDXL 1.0) na GPU."""

import math
import os
import random
import shutil
from pathlib import Path
from typing import Any

from trainer_difusao.common import (
    ENABLE_TEXT_ENCODER_UNLOAD,
    TextEmbedsCache,
    _cached_encode,
    _cleanup_cuda,
    _cycling_batches,
    _die,
    _emit_metric,
    _load_lora_weights,
    _offload_encoders_to_cpu,
    _precompute_sample_embeds_sdxl,
    _precompute_text_cache,
    _precompute_text_cache_with_cleanup,
    _prune_checkpoints,
    _resolve_output_name,
    _save_lora_safetensors,
    _setup_cache_dir,
    _temporary_device_encoders,
    _validate_train_aux,
)
from trainer_difusao.dataset import DiffusionDataset, build_dataloader
from trainer_difusao.models.base import BaseModelTrainer
from trainer_difusao.optimizers import _create_lr_scheduler, _create_optimizer


def _compute_sdxl_embeddings(
    prompts: list[str],
    tokenizer_one: Any,
    tokenizer_two: Any,
    text_encoder_one: Any,
    text_encoder_two: Any,
    device: Any,
    target_dtype: Any = None,
) -> tuple[Any, Any]:
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
        if target_dtype is not None:
            prompt_embeds = prompt_embeds.to(dtype=target_dtype)
            pooled_embeds = pooled_embeds.to(dtype=target_dtype)

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
        from transformers import (
            AutoTokenizer,
            BitsAndBytesConfig,
            CLIPTextModel,
            CLIPTextModelWithProjection,
        )
    except ImportError as e:
        _die(f"Dependência ausente para treino real SDXL: {e}")

    if not torch.cuda.is_available():
        _die("CUDA não disponível para treino real de difusão (ENGINE_MOCK=0)")

    device = torch.device("cuda")

    aux = _validate_train_aux(cfg, quant_default="none")
    control_dataset_path = aux["control_dataset_path"]
    control_ratio = aux["control_ratio"]
    cache_text_embeddings = aux["cache_text_embeddings"]

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
    # feat/pesos-custom-flux2: treino custom sdxl via from_single_file do UNet —
    # mecânico (mesmo padrão da geração): demais componentes do repo oficial.
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
            "(treino sdxl não usa encoder custom)."
        )
    mixed_precision = str(lora_cfg.get("mixed_precision", "fp16")).lower().strip()
    target_dtype = (
        torch.bfloat16
        if (mixed_precision == "bf16" and torch.cuda.is_bf16_supported())
        else torch.float16
    )
    quantization = aux["quantization"] or "none"
    is_4bit = quantization == "4bit"
    is_8bit = quantization == "8bit"
    is_quantized = is_4bit or is_8bit

    if is_4bit:
        bnb_config = BitsAndBytesConfig(
            load_in_4bit=True,
            bnb_4bit_quant_type="nf4",
            bnb_4bit_compute_dtype=target_dtype,
            bnb_4bit_use_double_quant=True,
        )
    elif is_8bit:
        bnb_config = BitsAndBytesConfig(load_in_8bit=True)
    else:
        bnb_config = None
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
    if custom_checkpoint_path:
        try:
            if is_quantized:
                unet = UNet2DConditionModel.from_single_file(
                    custom_checkpoint_path,
                    quantization_config=bnb_config,
                    torch_dtype=target_dtype,
                )
            else:
                unet = UNet2DConditionModel.from_single_file(
                    custom_checkpoint_path, torch_dtype=target_dtype
                ).to(device)
        except Exception as exc:
            _die(
                f"Falha ao carregar checkpoint sdxl custom "
                f"({custom_checkpoint_path}): layout não reconhecido ({exc})"
            )
        print(
            f"[SDXL] Checkpoint custom aplicado ao UNet: {custom_checkpoint_path}",
            flush=True,
        )
    else:
        if is_quantized:
            unet = UNet2DConditionModel.from_pretrained(
                model_id,
                subfolder="unet",
                quantization_config=bnb_config,
                torch_dtype=target_dtype,
                cache_dir=hub_cache,
            )
        else:
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

    # Gradient checkpointing economiza ~50% VRAM (diffusers usa use_reentrant=False nativo)
    # Nota: prepare_model_for_kbit_training do PEFT é exclusivo de modelos NLP/transformers.
    unet.enable_gradient_checkpointing()

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
        message=f"Adaptadores LoRA injetados no UNet SDXL (rank={rank}, alpha={alpha}).",
    )

    optimizer = _create_optimizer(unet, optimizer_name, learning_rate)

    dataset = DiffusionDataset(
        dataset_path,
        resolution=resolution,
        trigger_word=trigger_word,
        enable_bucket=enable_bucket,
        metrics_path=metrics_path,
    )

    # Dataset de controle (prior-preservation): mesma resolução/bucketing,
    # caption VAZIA (sem trigger word). Intercalação por step (mesma pipeline
    # de ruído/loss) com probabilidade control_ratio.
    control_dataset = None
    control_iter = None
    control_n = 0
    if control_dataset_path is not None:
        control_dataset = DiffusionDataset(
            control_dataset_path,
            resolution=resolution,
            trigger_word="",
            enable_bucket=enable_bucket,
            empty_captions=True,
            metrics_path=metrics_path,
        )
        control_iter = _cycling_batches(
            build_dataloader(control_dataset, batch_size, seed=seed)
        )
        control_n = len(control_dataset)

    steps_per_epoch = math.ceil(len(dataloader) / grad_accum)
    total_train_steps = max(1, steps_per_epoch * epochs)
    lr_scheduler = _create_lr_scheduler(
        optimizer, lr_scheduler_name, total_train_steps, lr_warmup_steps
    )
    # Pré-computa embeddings da amostra se sample_prompt fornecido (antes de offload dos encoders)
    sample_embeds = None
    if sample_prompt:
        try:
            sample_embeds = _precompute_sample_embeds_sdxl(
                tokenizer_one=tokenizer_one,
                tokenizer_two=tokenizer_two,
                text_encoder_one=text_encoder_one,
                text_encoder_two=text_encoder_two,
                prompt=sample_prompt,
                device=device,
                dtype=target_dtype,
            )
        except Exception as e:
            print(f"[WARN] Falha ao pré-computar sample embeds SDXL: {e}", flush=True)


    # Cache de text embeddings (SDXL: saída combinada dos dois CLIP + pooled),
    # pré-computado UMA vez no início; miss → on-the-fly + warm; falha → sem cache.
    text_cache = TextEmbedsCache(output, cache_text_embeddings)
    if cache_text_embeddings:
        def _encode_sdxl_all(caps: list[str]) -> dict[str, Any]:
            hidden, pooled = _compute_sdxl_embeddings(
                caps,
                tokenizer_one,
                tokenizer_two,
                text_encoder_one,
                text_encoder_two,
                device,
            )
            return {"hidden": hidden, "pooled": pooled}

        should_unload = ENABLE_TEXT_ENCODER_UNLOAD and epoch_offset == 0
        if should_unload:
            _precompute_text_cache_with_cleanup(
                text_cache,
                [c for _, c in dataset.samples]
                + ([c for _, c in control_dataset.samples] if control_dataset else []),
                _encode_sdxl_all,
                metrics_path=metrics_path,
                unload_encoders=True,
                encoders=[text_encoder_one, text_encoder_two],
            )
        else:
            _precompute_text_cache(
                text_cache,
                [c for _, c in dataset.samples]
                + ([c for _, c in control_dataset.samples] if control_dataset else []),
                _encode_sdxl_all,
                metrics_path=metrics_path,
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
        f"[SDXL] Treino: dataset={len(dataset)} imagens, "
        f"control_dataset_images={control_n}, control_ratio={control_ratio}, "
        f"cache_text_embeddings={cache_text_embeddings}, quantization={quantization}",
        flush=True,
    )

    # Time IDs padrão para SDXL dimensionados pela resolução configurada
    add_time_ids = torch.tensor(
        [[resolution, resolution, 0, 0, resolution, resolution]],
        dtype=target_dtype,
        device=device,
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
            metrics_path=metrics_path,
            epoch=0,
            sample_embeds=sample_embeds,
        )
        if sample_baseline_file.exists():
            _emit_metric(
                metrics_path,
                epoch=0,
                step=6,
                progress=0.10,
                phase="baseline_ready",
                message="Amostra baseline SDXL gerada com sucesso (Época 0).",
            )
        else:
            _emit_metric(
                metrics_path,
                epoch=0,
                step=6,
                progress=0.10,
                phase="baseline_failed",
                message="Falha ao gerar amostra baseline SDXL pré-treino.",
            )

    _emit_metric(
        metrics_path,
        epoch=epoch_offset,
        step=7,
        progress=0.10,
        phase="training_started",
        message=f"Iniciando loop de treino SDXL: {epochs} épocas (offset={epoch_offset}), {total_train_steps} passos totais.",
    )

    print(
        f"Iniciando treino LoRA SDXL: {epochs} épocas (offset={epoch_offset}), {len(dataset)} imagens, res={resolution}, "
        f"rank={rank}, alpha={alpha}, lr={learning_rate}, grad_accum={grad_accum}, opt={optimizer_name}, "
        f"scheduler={lr_scheduler_name}",
        flush=True,
    )
    global_step = 0
    safe_avg_loss = None

    def _encode_sdxl_batch(caps: list[str]) -> dict[str, Any]:
        hidden, pooled = _compute_sdxl_embeddings(
            caps,
            tokenizer_one,
            tokenizer_two,
            text_encoder_one,
            text_encoder_two,
            device,
            target_dtype,
        )
        return {"hidden": hidden, "pooled": pooled}

    for epoch_idx in range(1, epochs + 1):
        epoch = epoch_idx + epoch_offset
        unet.train()
        epoch_loss = 0.0
        steps_in_epoch = 0

        for batch in dataloader:
            # Prior-preservation: com prob. control_ratio usa o batch de controle
            # (regularização, captions vazias) no loss do mesmo step.
            if control_iter is not None and random.random() < control_ratio:
                batch = next(control_iter)
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

            cached = _cached_encode(
                prompts,
                _encode_sdxl_batch,
                text_cache,
                encoders=[text_encoder_one, text_encoder_two],
                device=device,
            )
            prompt_embeds = cached["hidden"].to(device, dtype=target_dtype)
            pooled_prompt_embeds = cached["pooled"].to(device, dtype=target_dtype)

            # Micro-conditioning de tamanho original, target e crop
            # Com bucketing ativo as dims reais do batch (bucket) substituem a resolução configurada.
            if enable_bucket:
                bh, bw = pixel_values.shape[2], pixel_values.shape[3]
                batch_time_ids = torch.tensor(
                    [[bh, bw, 0, 0, bh, bw]],
                    dtype=target_dtype,
                    device=device,
                ).repeat(cur_bs, 1)
            else:
                batch_time_ids = add_time_ids.repeat(cur_bs, 1)

            # Predição de ruído pelo UNet com adaptadores LoRA ativos
            model_pred = unet(
                noisy_latents,
                timesteps,
                prompt_embeds,
                added_cond_kwargs={
                    "text_embeds": pooled_prompt_embeds,
                    "time_ids": batch_time_ids,
                },
                return_dict=False,
            )[0]

            # Loss MSE simples contra o ruído gaussiano adicionado
            loss = F.mse_loss(model_pred.float(), noise.float(), reduction="mean")
            loss = loss / grad_accum
            loss.backward()

            cur_loss_raw = float(loss.item()) * grad_accum
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

            # Emite métricas intermediárias
            if steps_in_epoch % grad_accum == 0 and (
                global_step % 5 == 0 or steps_in_epoch == len(dataloader)
            ):
                safe_loss = (
                    None
                    if (math.isnan(cur_loss_raw) or math.isinf(cur_loss_raw))
                    else round(cur_loss_raw, 4)
                )
                current_progress = round(
                    min(0.99, max(0.10, 0.10 + 0.89 * (global_step / max(1, total_train_steps)))), 4
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

        avg_loss = epoch_loss / max(1, steps_in_epoch)
        safe_avg_loss = (
            None
            if (math.isnan(avg_loss) or math.isinf(avg_loss))
            else round(avg_loss, 4)
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
            f"[SDXL] Época {epoch}/{epochs + epoch_offset} concluída - Step {global_step} - Loss Médio: {avg_loss}",
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
                    "base_model": "sdxl",
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
            _emit_metric(
                metrics_path,
                epoch=epoch,
                phase="generating_sample",
                message=f"Iniciando geração de amostra visual (Época {epoch})...",
                telemetry_only=True,
            )
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
