"""Pipeline de treino LoRA para Qwen-Image-2.1 (nativo, sem fallback diffusers)."""

from __future__ import annotations

import copy
import math
import os
import shutil
import time
from pathlib import Path
from typing import Any

import torch
import torch.nn.functional as F
from huggingface_hub import hf_hub_download
from peft import LoraConfig
from safetensors.torch import save_file
from transformers import AutoTokenizer

from engine_kit.mock import is_mock
from engine_kit.vram import cleanup_cuda, release_memory, vram_allocated_gb, vram_reserved_gb

from trainer_difusao.common import (
    TextEmbedsCache,
    _die,
    _emit_metric,
    _format_eta,
    _load_lora_weights,
    _load_optimizer_state,
    _override_optimizer_lr,
    _normalize_train_quantization,
    _resolve_output_name,
    _setup_cache_dir,
    _validate_train_aux,
    save_adapter_checkpoint,
    save_final_adapter,
)
from trainer_difusao.dataset import DiffusionDataset, build_dataloader
from trainer_difusao.models.base import BaseModelTrainer
from trainer_difusao.models.mock import _mock_train
from trainer_difusao.common_pkg.diagnostics import (
    DiagnosticsTracker,
    compute_grad_norm_l2,
)
from trainer_difusao.models.qwen_pkg.qwen_image_2 import (
    AutoencoderKLQwenImage21,
    QwenImage21Pipeline,
    QwenImage21PromptEncoder,
    QwenImage21TextEncoder,
    QwenImage21Transformer2DModel,
    calculate_shift,
    pack_latents,
    pad_prompt_batch,
    run_transformer,
)

_release_system_memory = release_memory


# Configuration matching ai-toolkit reference
COMFY_REPO = "Comfy-Org/Qwen-Image-2.1"
BASE_REPO = "Qwen/Qwen-Image-2.1"
TILE_DECODE_ABOVE_PIXELS = 1024 * 1024


def _unload_text_pipeline(pipe: Any | None, text_enc: Any | None = None) -> None:
    """Descarrega text pipeline e encoder da memória de forma uniforme."""
    if pipe is not None:
        for attr in ("text_encoder", "tokenizer", "processor"):
            try:
                setattr(pipe, attr, None)
            except Exception:
                pass
        try:
            del pipe
        except Exception:
            pass
    if text_enc is not None:
        del text_enc


def _qwen_sample_native(
    transformer: Any,
    vae: Any,
    prompt_encoder: Any,
    prompt: str,
    height: int,
    width: int,
    num_inference_steps: int,
    seed: int,
    device: Any,
    dtype: Any,
    output_path: Path | None = None,
) -> Any:
    """Procedural Qwen-Image-2.1 sampling using flow matching without OOP pipeline interface.
    
    Returns a PIL Image or None if output_path is None.
    """
    import numpy as np
    from diffusers import FlowMatchEulerDiscreteScheduler
    from PIL import Image
    
    try:
        import torch
        
        # Encode prompt
        with torch.no_grad():
            embeds_list, masks_list, slot_masks_list = prompt_encoder.encode([prompt])
            prompt_embeds, prompt_mask, slot_mask = pad_prompt_batch(
                embeds_list, masks_list, slot_masks_list, device, dtype
            )
        
        # Setup scheduler with flow matching
        scheduler_config = {
            "base_image_seq_len": 256,
            "max_image_seq_len": 8192,
            "base_shift": 0.5,
            "max_shift": 0.9,
            "num_train_timesteps": 1000,
            "shift": 1.0,
        }
        try:
            scheduler = FlowMatchEulerDiscreteScheduler(**scheduler_config)
        except TypeError:
            # Fallback if some params are rejected
            scheduler = FlowMatchEulerDiscreteScheduler(
                num_train_timesteps=1000,
            )
        
        # Compute timestep schedule
        latent_h = height // 16
        latent_w = width // 16
        image_seq_len = latent_h * latent_w
        shift = calculate_shift(
            image_seq_len,
            base_seq_len=256,
            max_seq_len=8192,
            base_shift=0.5,
            max_shift=0.9,
        )
        
        # Sample initial latents with seed
        # Get channel count from transformer config (64 for Qwen-Image-2.1)
        channels = transformer.config.in_channels
        generator = torch.Generator(device=device).manual_seed(seed)
        latents = torch.randn(
            (1, channels, latent_h, latent_w),
            generator=generator,
            device=device,
            dtype=dtype,
        )
        
        # Denoising loop
        with torch.no_grad():
            transformer.eval()
            vae.eval()
            
            # Compute sigma schedule
            sigmas = np.linspace(1.0, 1.0/num_inference_steps, num_inference_steps)
            scheduler.set_timesteps(sigmas=sigmas, device=device)
            
            for t in scheduler.timesteps:
                # Compute timestep in [0, 1]
                t_float = t.float() / 1000.0 if t.is_floating_point() else t.float() / 1000.0
                
                
                # Run transformer (it handles packing internally)
                pred = run_transformer(
                    transformer,
                    latents,
                    t_float.expand(1).to(device, dtype),
                    prompt_embeds,
                    prompt_mask,
                    slot_mask,
                    condition_latents=None,
                    condition_shapes=[],
                )
                
                # Scheduler step
                latents = scheduler.step(pred.float(), t, latents.float())[0].to(dtype)
        
        # Decode to image
        with torch.no_grad():
            vae.to(device)
            # Denormalize latents using VAE config
            mean = torch.tensor(vae.config.latents_mean).view(1, -1, 1, 1).to(device, dtype=dtype)
            std = torch.tensor(vae.config.latents_std).view(1, -1, 1, 1).to(device, dtype=dtype)
            latents_denorm = latents * std + mean
            
            # VAE expects 5D input for decode as well
            latents_5d = latents_denorm.unsqueeze(2)  # (B, 64, H', W') -> (B, 64, 1, H', W')
            
            # Enable tiling for large images
            if height > 1024 or width > 1024:
                vae.enable_tiling()
            
            # Decode
            image = vae.decode(latents_5d).sample  # (B, C, 1, H, W) in [0, 1]
            image = image.squeeze(2)  # Remove frame dim: (B, C, 1, H, W) -> (B, C, H, W)
            # Convert to PIL (RGB, stripping alpha if present)
            if image.shape[1] == 4:  # RGBA
                image = image[:, :3]  # Strip alpha
            
            # Denormalize to [0, 255]
            image = ((image + 1.0) / 2.0).clamp(0, 1)
            image = (image * 255).to(torch.uint8).cpu().numpy()
            image = image.transpose(0, 2, 3, 1)[0]  # (H, W, C)
            pil_image = Image.fromarray(image, mode="RGB")
        
        # Save if output path provided
        if output_path is not None:
            output_path = Path(output_path)
            output_path.parent.mkdir(parents=True, exist_ok=True)
            pil_image.save(str(output_path))
            print(f"[SAMPLE] Saved to {output_path}", flush=True)
        
        return pil_image
    
    except Exception as e:
        print(f"[ERROR] Sampling failed: {e}", flush=True)
        import traceback
        traceback.print_exc()
        return None

def _resume_epoch_range(epoch_offset: int, epochs: int) -> list[tuple[int, int]]:
    """Gera os pares (local_epoch_idx, epoch_absoluto) do treino.

    `epochs` é sempre o número de épocas ADICIONAIS desta execução, numeradas
    de `epoch_offset + 1` até `epoch_offset + epochs` (epoch_offset=0 reproduz
    a numeração original 1..epochs). `local_epoch_idx` é 1-based relativo a
    esta execução (usado para checkpoint_interval/sample_interval/progress);
    `epoch` é o número absoluto de época (usado em métricas/nomes de arquivo).
    """
    return list(
        enumerate(range(epoch_offset + 1, epoch_offset + epochs + 1), start=1)
    )


def _real_train_qwen_image(cfg: dict[str, Any], output: Path | str) -> None:
    """Pipeline real de treino LoRA para Qwen-Image-2.1 na GPU (nativo)."""
    # Elimina OOM por fragmentação de heap CUDA
    import os as _os
    _os.environ.setdefault("PYTORCH_CUDA_ALLOC_CONF", "expandable_segments:True")

    output_path = Path(output)
    output_path.mkdir(parents=True, exist_ok=True)
    metrics_path = output_path / "metrics.jsonl"
    checkpoints_dir = output_path / "checkpoints"
    checkpoints_dir.mkdir(parents=True, exist_ok=True)

    try:
        from peft import get_peft_model
        from bitsandbytes.optim import AdamW8bit
    except ImportError as exc:
        _die(f"Dependência ausente para treino real de Qwen-Image-2.1: {exc}")

    lora_cfg = cfg.get("lora", {})
    rank = int(lora_cfg.get("rank", 16))
    lora_alpha = int(lora_cfg.get("alpha", 16))
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

    checkpoint_interval = max(1, int(cfg.get("checkpoint_interval") or lora_cfg.get("checkpoint_interval") or 1))
    epoch_offset = max(0, int(cfg.get("epoch_offset") or lora_cfg.get("epoch_offset") or 0))
    weights_path = cfg.get("weights_path")
    optimizer_state_path = cfg.get("optimizer_state_path")
    grad_accum = max(1, int(lora_cfg.get("gradient_accumulation_steps", 1)))
    batch_size = max(1, int(lora_cfg.get("batch_size", 1)))
    resolution = int(cfg.get("resolution") or lora_cfg.get("resolution") or 768)

    raw_dataset_path = cfg.get("dataset_path")
    if not raw_dataset_path:
        _die("dataset_path não configurado no payload.")
    dataset_path = Path(raw_dataset_path)
    if not dataset_path.exists():
        _die(f"dataset_path inválido ou inexistente: '{dataset_path}'")

    model_repo = os.environ.get("QWEN_IMAGE_MODEL_ID", BASE_REPO)
    device = "cuda" if torch.cuda.is_available() else "cpu"
    torch_dtype = torch.bfloat16 if torch.cuda.is_available() and torch.cuda.is_bf16_supported() else torch.float32

    _emit_metric(
        metrics_path,
        epoch=epoch_offset,
        step=0,
        loss=1.0,
        lr=learning_rate,
        progress=0.01,
        phase="initializing",
        message=f"Inicializando treino Qwen-Image-2.1 nativo ({model_repo})...",
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
    total_steps = max(1, math.ceil(len(dataloader) * epochs / grad_accum))

    _emit_metric(
        metrics_path,
        epoch=epoch_offset,
        step=0,
        loss=1.0,
        lr=learning_rate,
        progress=0.01,
        phase="loading_models",
        message=f"Carregando modelos nativo Qwen-Image-2.1...",
    )

    # Resolve model source: prefer local, fallback to Comfy repack (weights only), then base repo (configs)
    model_path = model_repo
    base_model_path = os.environ.get("QWEN_IMAGE_BASE_ID", model_repo)
    if base_model_path == model_path and not os.path.isdir(base_model_path):
        base_model_path = BASE_REPO
    elif os.path.isdir(model_path) and os.path.isdir(os.path.join(model_path, "text_encoder")):
        base_model_path = model_path

    # 2. Carrega os componentes do modelo nativo (sem getattr diffusers!)
    try:
        # Load transformer
        _emit_metric(
            metrics_path,
            epoch=epoch_offset,
            step=0,
            loss=1.0,
            lr=learning_rate,
            progress=0.05,
            phase="loading_models",
            message="Carregando transformer...",
        )
        # Download and setup transformer with Comfy-Org weights
        from trainer_difusao.loaders.quant_cache import (
            _is_cache_valid,
            _save_quant_metadata,
            get_bitsandbytes_version,
            resolve_quant_base_dir,
            save_atomic_dir,
        )

        cfg_path = hf_hub_download(BASE_REPO, "transformer/config.json", cache_dir=hub_cache, token=hf_token)
        comfy_file = hf_hub_download(COMFY_REPO, "diffusion_models/qwen_image_2.1_bf16.safetensors", cache_dir=hub_cache, token=hf_token)
        
        local_dir = Path(hub_cache) / "qwen_transformer_local"
        local_dir.mkdir(parents=True, exist_ok=True)
        shutil.copy(cfg_path, local_dir / "config.json")
        target_weight = local_dir / "diffusion_pytorch_model.safetensors"
        if not target_weight.exists():
            os.symlink(comfy_file, target_weight)
        
        bnb_ver = get_bitsandbytes_version()
        quant_model_id = f"{COMFY_REPO}/qwen_image_2.1"
        quant_base = resolve_quant_base_dir(quant_model_id, "4bit")
        transformer_cache_dir = quant_base / "transformer"
        
        transformer = None
        if _is_cache_valid(
            transformer_cache_dir,
            expected_model_id=quant_model_id,
            expected_quant="4bit",
            expected_bnb_version=bnb_ver,
        ):
            try:
                print(
                    f"[QWEN-IMAGE] Carregando transformer 4-bit do cache quantizado validado: {transformer_cache_dir}",
                    flush=True,
                )
                transformer = QwenImage21Transformer2DModel.from_pretrained(
                    str(transformer_cache_dir),
                    torch_dtype=torch_dtype,
                )
            except Exception as e:
                print(
                    f"[WARN] Falha ao carregar cache do transformer quantizado ({transformer_cache_dir}): {e}. "
                    "Descartando cache corrompido e refazendo quantização...",
                    flush=True,
                )
                shutil.rmtree(transformer_cache_dir, ignore_errors=True)
                transformer = None
        elif transformer_cache_dir.exists():
            print(
                f"[INFO] Cache do transformer em {transformer_cache_dir} é inválido/incompatível. "
                "Expurgando e refazendo quantização...",
                flush=True,
            )
            shutil.rmtree(transformer_cache_dir, ignore_errors=True)
        
        if transformer is None:
            from transformers import BitsAndBytesConfig
            bnb_config = BitsAndBytesConfig(load_in_4bit=True, bnb_4bit_compute_dtype=torch_dtype)
            transformer = QwenImage21Transformer2DModel.from_pretrained(
                str(local_dir), torch_dtype=torch_dtype, quantization_config=bnb_config,
            )
            try:
                save_atomic_dir(transformer_cache_dir, lambda p: transformer.save_pretrained(p))
                _save_quant_metadata(
                    quant_base,
                    model_id=quant_model_id,
                    quant_label="4-bit BitsAndBytes",
                    quant_format="4bit",
                    target_dtype=torch_dtype,
                    bnb_version=bnb_ver,
                )
                print(
                    f"[QWEN-IMAGE] Transformer quantizado 4-bit persistido em cache para execuções futuras: {transformer_cache_dir}",
                    flush=True,
                )
            except Exception as e:
                print(
                    f"[WARN] Não foi possível persistir transformer quantizado em disco: {e}",
                    flush=True,
                )
        
        transformer = transformer.to(device)
        transformer.train()

        # Load text encoder (4-bit on GPU, CPU fallback)
        _emit_metric(
            metrics_path,
            epoch=epoch_offset,
            step=0,
            loss=1.0,
            lr=learning_rate,
            progress=0.10,
            phase="loading_models",
            message="Carregando text encoder...",
        )
        
        text_encoder = None
        if device == "cuda":
            try:
                from transformers import BitsAndBytesConfig
                bnb_config = BitsAndBytesConfig(
                    load_in_4bit=True,
                    bnb_4bit_compute_dtype=torch_dtype,
                )
                text_encoder = QwenImage21TextEncoder.load_model(
                    base_model_path,
                    dtype=torch_dtype,
                    subfolder="text_encoder",
                    quantization_config=bnb_config,
                    device_map="cuda:0",
                    cache_dir=hub_cache,
                    token=hf_token,
                )
            except Exception as e:
                print(f"[WARN] Falha ao carregar text encoder 4-bit: {e}. Usando CPU.", flush=True)
                device = "cpu"
                text_encoder = None

        if text_encoder is None:
            text_encoder = QwenImage21TextEncoder.load_model(
                base_model_path,
                dtype=torch_dtype,
                subfolder="text_encoder",
                device_map="cpu",
                cache_dir=hub_cache,
                token=hf_token,
            )

        text_encoder.requires_grad_(False)
        text_encoder.eval()
        processor = QwenImage21TextEncoder.load_processor(base_model_path)
        prompt_encoder = QwenImage21PromptEncoder(text_encoder, processor)
        cleanup_cuda()

        # Load VAE
        _emit_metric(
            metrics_path,
            epoch=epoch_offset,
            step=0,
            loss=1.0,
            lr=learning_rate,
            progress=0.15,
            phase="loading_models",
            message="Carregando VAE...",
        )
        vae = AutoencoderKLQwenImage21.from_pretrained(
            base_model_path,
            subfolder="vae",
            torch_dtype=torch_dtype,
            cache_dir=hub_cache,
            token=hf_token,
        )
        vae = vae.to(device)
        vae.requires_grad_(False)
        vae.eval()
        cleanup_cuda()

        # Setup schedulers
        scheduler_config = {
            "num_train_timesteps": 1000,
            "beta_schedule": "linear",
            "beta_start": 0.0001,
            "beta_end": 0.02,
        }
        noise_scheduler = type('SimpleScheduler', (), {'config': scheduler_config})()

    except Exception as e:
        _die(f"Erro ao carregar modelos: {e}")

    # 3. Pré-computação de Text Embeddings via cache
    text_cache = TextEmbedsCache(
        output_path,
        enabled=True,
        namespace_fields={
            "arch": "qwen_image_2_1",
            "model_id": base_model_path,
            "text_encoder_quant": "4bit-nf4-bnb" if device == "cuda" else "full",
            "dtype": str(torch_dtype),
        },
    )
    unique_prompts = list({cap for _, cap in dataset.samples})

    _emit_metric(
        metrics_path,
        epoch=epoch_offset,
        step=0,
        loss=1.0,
        lr=learning_rate,
        progress=0.20,
        phase="loading_models",
        message=f"Pré-computando {len(unique_prompts)} embeddings de texto...",
    )

    
    # Generate baseline sample after models are loaded
    if sample_prompt and sample_interval > 0:
        _emit_metric(
            metrics_path,
            epoch=epoch_offset,
            step=0,
            loss=1.0,
            lr=learning_rate,
            progress=0.22,
            phase="generating_baseline_sample",
            message=f"Gerando amostra baseline com prompt: '{sample_prompt}'",
        )
        try:
            _qwen_sample_native(
                transformer,
                vae,
                prompt_encoder,
                prompt=sample_prompt,
                height=resolution,
                width=resolution,
                num_inference_steps=20,
                seed=sample_seed,
                device=device,
                dtype=torch_dtype,
                output_path=output_path / "baseline.png",
            )
            _emit_metric(
                metrics_path,
                epoch=epoch_offset,
                step=0,
                loss=1.0,
                lr=learning_rate,
                progress=0.23,
                phase="baseline_ready",
                message="Amostra baseline concluída",
            )
        except Exception as e:
            print(f"[WARN] Erro ao gerar amostra baseline: {e}", flush=True)

    
    # Pre-compute all unique prompts (with proper VRAM cleanup)
    cached_count = 0
    for i, prompt in enumerate(unique_prompts):
        if text_cache.get(prompt) is not None:
            cached_count += 1
            continue
        try:
            # Encode prompt without reference images (T2I only)
            embeds_list, masks_list, slot_masks_list = prompt_encoder.encode([prompt])
            # Store all three components as a dict payload
            payload = {
                "embeds": embeds_list[0],
                "mask": masks_list[0],
                "slot_mask": slot_masks_list[0],
            }
            text_cache.put(prompt, payload)
        except Exception as e:
            print(f"[WARN] Erro ao pré-computar prompt '{prompt}': {e}", flush=True)
            continue
        if (i + 1) % 10 == 0:
            cleanup_cuda()

    cleanup_cuda()
    _unload_text_pipeline(None, text_encoder)

    # 4. Setup LoRA
    _emit_metric(
        metrics_path,
        epoch=epoch_offset,
        step=0,
        loss=1.0,
        lr=learning_rate,
        progress=0.25,
        phase="setup_lora",
        message=f"Configurando LoRA (rank={rank}, alpha={lora_alpha})...",
    )

    # Enable gradient checkpointing to reduce VRAM
    transformer.enable_gradient_checkpointing()

    lora_config = LoraConfig(
        r=rank,
        lora_alpha=lora_alpha,
        init_lora_weights="gaussian",
        target_modules=["to_q", "to_k", "to_v", "to_out.0"],
        bias="none",
    )

    try:
        model_with_lora = get_peft_model(transformer, lora_config)
    except Exception as e:
        _die(f"Erro ao configurar LoRA: {e}")
    if weights_path:
        _load_lora_weights(model_with_lora, weights_path)
    # 5. Setup optimizer
    optimizer = AdamW8bit(
        model_with_lora.parameters(),
        lr=learning_rate,
        betas=(0.9, 0.999),
    )
    if optimizer_state_path:
        _load_optimizer_state(optimizer, optimizer_state_path)
        # O LR da nova requisição (learning_rate) sempre prevalece sobre o
        # persistido no optimizer state restaurado.
        _override_optimizer_lr(optimizer, learning_rate)

    # 6. Training loop
    num_train_samples = len(dataset)
    steps_per_epoch = max(1, math.ceil(len(dataloader) / grad_accum))
    total_progress_steps = steps_per_epoch * epochs

    _emit_metric(
        metrics_path,
        epoch=epoch_offset,
        step=0,
        loss=1.0,
        lr=learning_rate,
        progress=0.30,
        phase="dataset_ready",
        message=f"Dataset pronto: {num_train_samples} amostras, {steps_per_epoch} steps/epoch",
    )

    # Baseline sample will be generated after models are loaded

    start_time = time.time()
    global_step = 0
    running_loss = 0.0
    diag_tracker = DiagnosticsTracker(lora_interval_steps=max(1, total_progress_steps // max(1, epochs)))
    last_grad_norm: float = 0.0
    _emit_metric(
        metrics_path,
        epoch=epoch_offset,
        step=0,
        loss=0.0,
        lr=learning_rate,
        progress=0.45,
        phase="training_started",
        message=(
            f"Iniciando treino por {epochs} épocas adicionais (offset={epoch_offset}), "
            f"numeradas de {epoch_offset + 1} a {epoch_offset + epochs}"
        ),
    )

    try:
        # `epochs` épocas ADICIONAIS, numeradas de epoch_offset+1 até
        # epoch_offset+epochs (epoch_offset=0 reproduz a numeração original 1..epochs).
        for local_epoch_idx, epoch in _resume_epoch_range(epoch_offset, epochs):
            epoch_loss = 0.0
            num_batches = 0

            for batch_idx, batch in enumerate(dataloader):
                # Prepare batch
                images = batch["pixel_values"].to(device, dtype=torch_dtype)
                captions = batch["prompt"]

                # Get cached text embeddings (embeds, mask, slot_mask)
                embeds_list = []
                masks_list = []
                slot_masks_list = []
                for cap in captions:
                    cached = text_cache.get(cap)
                    if cached is not None:
                        embeds_list.append(cached["embeds"])
                        masks_list.append(cached["mask"])
                        slot_masks_list.append(cached["slot_mask"])
                    else:
                        print(f"[WARN] Missing cache for caption: {cap}", flush=True)

                if not embeds_list:
                    continue

                batch_size_actual = len(embeds_list)

                # Encode images to latents
                with torch.no_grad():
                    vae.to(device)
                    # VAE nativo (RGBA) espera in_channels=4; dataset comum é RGB (3 canais)
                    if images.shape[1] == 3:
                        alpha_channel = torch.ones_like(images[:, :1])
                        images = torch.cat([images, alpha_channel], dim=1)
                    # VAE expects 5D input (B, C, T, H, W) with single-frame dim at dim=2
                    images_5d = images.unsqueeze(2)  # (B, C, H, W) -> (B, C, 1, H, W)
                    latents = vae.encode(images_5d).latent_dist.sample()  # (B, 64, 1, H', W')
                    # Normalize latents (ai-toolkit reference pattern)
                    mean = torch.tensor(vae.config.latents_mean).view(1, -1, 1, 1, 1).to(device, dtype=torch_dtype)
                    std = torch.tensor(vae.config.latents_std).view(1, -1, 1, 1, 1).to(device, dtype=torch_dtype)
                    latents = (latents - mean) / std
                    # Remove frame dimension for downstream processing
                    latents = latents.squeeze(2)  # (B, 64, 1, H', W') -> (B, 64, H', W')
                    vae.to("cpu")

                # Pad prompt embeddings and masks into batch tensors
                prompt_embeds, prompt_mask, slot_mask = pad_prompt_batch(
                    embeds_list, masks_list, slot_masks_list, device, torch_dtype
                )

                # Flow-matching sampling
                noise = torch.randn_like(latents[:batch_size_actual])
                # Sample u from Normal(0,1) and compute sigmoid(u)
                u = torch.normal(mean=0.0, std=1.0, size=(batch_size_actual,), device=device)
                t_sigmoid = torch.sigmoid(u)
                
                # Compute shift based on image resolution
                latent_h = latents.shape[2]
                latent_w = latents.shape[3]
                image_seq_len = latent_h * latent_w
                shift = calculate_shift(
                    image_seq_len,
                    base_seq_len=256,
                    max_seq_len=8192,
                    base_shift=0.5,
                    max_shift=0.9,
                )
                
                # Flow-matching timesteps in [0, 1]
                timesteps_flow = (shift * t_sigmoid) / (1.0 + (shift - 1.0) * t_sigmoid)
                
                # Interpolate: x_t = (1 - t) * x_0 + t * noise (rectified flow)
                t_expanded = timesteps_flow.view(-1, 1, 1, 1).to(dtype=torch_dtype)
                noisy_latents = (1.0 - t_expanded) * latents[:batch_size_actual] + t_expanded * noise
                target = noise - latents[:batch_size_actual]

                # Get noise prediction from model
                with torch.set_grad_enabled(True):
                    model_with_lora.train()
                    transformer.to(device)


                    # Run transformer (it handles packing internally)
                    noise_pred = run_transformer(
                        transformer,
                        noisy_latents,
                        timesteps_flow,
                        prompt_embeds,
                        prompt_mask,
                        slot_mask,
                    )

                    # Loss target: velocity = noise - latents (flow matching)
                    # Note: target is already (noise - latents), run_transformer returns unpacked (B, C, H, W)

                    # Compute loss using unpacked target (run_transformer returns unpacked)
                    loss = F.mse_loss(noise_pred.float(), target.float())

                    # Backward pass
                    optimizer.zero_grad()
                    loss.backward()
                    grad_norm_raw = torch.nn.utils.clip_grad_norm_(model_with_lora.parameters(), 1.0)
                    if hasattr(grad_norm_raw, "item"):
                        last_grad_norm = float(grad_norm_raw.item())
                    elif grad_norm_raw is not None:
                        last_grad_norm = float(grad_norm_raw)
                    else:
                        last_grad_norm = compute_grad_norm_l2(model_with_lora.parameters())
                    cur_loss_item = float(loss.item())
                    diag_tracker.observe_step(cur_loss_item, last_grad_norm)
                    optimizer.step()

                    running_loss += cur_loss_item
                    epoch_loss += cur_loss_item
                    num_batches += 1
                    global_step += 1
                    # Metrics
                    if global_step % 10 == 0:
                        avg_loss = running_loss / 10
                        elapsed = time.time() - start_time
                        time_per_step = elapsed / global_step
                        remaining_steps = total_progress_steps - global_step
                        eta_seconds = time_per_step * remaining_steps

                        progress = min(0.99, 0.45 + (global_step / total_progress_steps) * 0.50)
                        vram_used = vram_allocated_gb()
                        diag_payload = diag_tracker.build_diagnostics(
                            grad_norm_l2=last_grad_norm,
                            optimizer=optimizer,
                            default_lr=learning_rate,
                            model=model_with_lora,
                            step=global_step,
                        )
                        _emit_metric(
                            metrics_path,
                            epoch=epoch,
                            step=global_step,
                            loss=avg_loss,
                            lr=learning_rate,
                            grad_norm=last_grad_norm,
                            diagnostics=diag_payload,
                            progress=progress,
                            phase="training",
                            message=f"Epoch {epoch}/{epoch_offset + epochs}, step {global_step}, loss={(f'{avg_loss:.4f}' if math.isfinite(avg_loss) else 'null')}",
                            eta_s=int(eta_seconds) if eta_seconds is not None else None,
                            step_time_s=time_per_step,
                            vram_reserved_gb=vram_used,
                        )
                        running_loss = 0.0

                # Cleanup
                torch.cuda.empty_cache()

                if batch_idx % checkpoint_interval == 0 and batch_idx > 0:
                    try:
                        metadata = {
                            "format": "pt",
                            "model_type": "lora",
                            "base_model": "qwen-image-2.1",
                            "epoch": str(epoch),
                            "step": str(global_step),
                        }
                        ckpt_path = save_adapter_checkpoint(
                            model_with_lora,
                            checkpoints_dir,
                            base_name,
                            epoch,
                            metadata,
                            optimizer=optimizer,
                        )
                        print(f"[CHECKPOINT] Salvo em {ckpt_path}", flush=True)
                    except Exception as e:
                        print(f"[WARN] Erro ao salvar checkpoint: {e}", flush=True)

            # End of epoch
            avg_epoch_loss = epoch_loss / max(1, num_batches)
            epoch_diag = diag_tracker.build_diagnostics(
                grad_norm_l2=last_grad_norm,
                optimizer=optimizer,
                default_lr=learning_rate,
                model=model_with_lora,
                step=global_step,
                force_lora=True,
            )
            _emit_metric(
                metrics_path,
                epoch=epoch,
                step=global_step,
                loss=avg_epoch_loss,
                lr=learning_rate,
                grad_norm=last_grad_norm,
                diagnostics=epoch_diag,
                progress=0.45 + local_epoch_idx / epochs * 0.50,
                phase="epoch_complete",
                message=f"Epoch {epoch} completo, loss médio={(f'{avg_epoch_loss:.4f}' if math.isfinite(avg_epoch_loss) else 'null')}",
            )
            # Sample during training (reduced resolution)
            if sample_prompt and sample_interval > 0 and local_epoch_idx % sample_interval == 0:
                try:
                    sample_res = min(resolution, 512)
                    _qwen_sample_native(
                        transformer,
                        vae,
                        prompt_encoder,
                        prompt=sample_prompt,
                        height=sample_res,
                        width=sample_res,
                        num_inference_steps=20,
                        seed=sample_seed,
                        device=device,
                        dtype=torch_dtype,
                        output_path=output_path / f"epoch{epoch}.png",
                    )
                except Exception as e:
                    print(f"[WARN] Erro ao gerar amostra epoch {epoch}: {e}", flush=True)

    except Exception as e:
        print(f"[ERROR] Erro durante treino: {e}", flush=True)
        raise
    finally:
        cleanup_cuda()
        _release_system_memory()

    # 7. Salva adaptador final
    _emit_metric(
        metrics_path,
        epoch=epoch_offset + epochs,
        step=global_step,
        loss=0.0,
        lr=learning_rate,
        progress=0.95,
        phase="saving",
        message="Salvando adaptador final...",
    )

    metadata = {
        "format": "pt",
        "model_type": "lora",
        "base_model": "qwen-image-2.1",
    }
    final_adapter_file = save_final_adapter(
        model_with_lora,
        output_path,
        base_name,
        metadata,
        optimizer=optimizer,
    )

    _emit_metric(
        metrics_path,
        epoch=epoch_offset + epochs,
        step=global_step,
        loss=0.0,
        lr=learning_rate,
        progress=1.0,
        phase="completed",
        message=f"Treino concluído! Adaptador salvo em: {final_adapter_file}",
    )

    print(f"Treino Qwen-Image-2.1 finalizado com sucesso! Checkpoint salvo em: {final_adapter_file}", flush=True)


class QwenImageTrainer(BaseModelTrainer):
    """Trainer de difusão para Qwen-Image-2.1 (nativo, sem fallback)."""

    def train(self, cfg: dict[str, Any], output: Path) -> None:
        if is_mock():
            _mock_train(cfg, output)
        else:
            _real_train_qwen_image(cfg, output)
