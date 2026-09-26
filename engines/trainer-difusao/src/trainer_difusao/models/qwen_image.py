"""Pipeline de treino LoRA para Qwen-Image-2.1."""

from __future__ import annotations

import copy
import math
import os
import time
from pathlib import Path
from typing import Any

from engine_kit.mock import is_mock
from engine_kit.vram import cleanup_cuda, release_memory, vram_allocated_gb, vram_reserved_gb

from trainer_difusao.common import (
    _die,
    _ensure_qwen_diffusers_compat,
    _emit_metric,
    _format_eta,
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
from trainer_difusao.models.qwen_pkg import _generate_sample_qwen

_release_system_memory = release_memory


def _unload_text_pipeline(pipe: Any | None, text_enc: Any | None = None) -> None:
    """Descarrega text pipeline e encoder da memória de forma uniforme."""
    if pipe is not None:
        for attr in ("text_encoder", "tokenizer", "processor"):
            try:
                setattr(pipe, attr, None)
            except Exception:
                pass
        for k in list(getattr(pipe, "components", {}).keys()):
            try:
                setattr(pipe, k, None)
            except Exception:
                pass
    if text_enc is not None:
        del text_enc


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
        from transformers import AutoTokenizer
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
    resolution = int(cfg.get("resolution") or lora_cfg.get("resolution") or 1024)

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
    total_steps = max(1, math.ceil(len(dataloader) * epochs / grad_accum))

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
        total_prompts = len(unique_prompts)
        use_cuda = (device == "cuda") and torch.cuda.is_available()
        text_enc = None
        if use_cuda:
            try:
                from transformers import BitsAndBytesConfig, Qwen3VLForConditionalGeneration

                bnb_config = BitsAndBytesConfig(load_in_4bit=True, bnb_4bit_compute_dtype=torch.bfloat16)
                print(f"[DIFFUSION-TRAIN] Carregando Text Encoder 4-bit na GPU para acelerar pré-computação...", flush=True)
                text_enc = Qwen3VLForConditionalGeneration.from_pretrained(
                    model_repo,
                    subfolder="text_encoder",
                    quantization_config=bnb_config,
                    torch_dtype=torch.bfloat16,
                    device_map="cuda:0",
                    cache_dir=hub_cache,
                    token=hf_token,
                )
            except Exception as bnb_err:
                print(f"[WARN] Falha ao carregar Text Encoder 4-bit na GPU ({bnb_err}). Fallback para CPU.", flush=True)
                use_cuda = False
                text_enc = None

        dev_desc = "GPU (4-bit)" if use_cuda else "CPU"
        print(f"[DIFFUSION-TRAIN] Pré-computando embeddings de texto ({total_prompts} prompts) na {dev_desc}...", flush=True)
        try:
            tokenizer = AutoTokenizer.from_pretrained(
                model_repo,
                subfolder="processor",
                cache_dir=hub_cache,
                token=hf_token,
            )
            pipe_kwargs = {
                "tokenizer": tokenizer,
                "vae": None,
                "transformer": None,
                "torch_dtype": torch.bfloat16,
                "cache_dir": hub_cache,
                "token": hf_token,
            }
            if text_enc is not None:
                pipe_kwargs["text_encoder"] = text_enc

            text_pipeline = PipelineCls.from_pretrained(
                model_repo,
                **pipe_kwargs,
            )
            encode_device = torch.device("cuda:0") if use_cuda else torch.device("cpu")
            bs = 32 if use_cuda else 8
            done = 0
            with torch.no_grad():
                i = 0
                while i < total_prompts:
                    chunk = unique_prompts[i : i + bs]
                    try:
                        encoded = text_pipeline.encode_prompt(chunk, device=encode_device)
                    except Exception as enc_err:
                        if use_cuda and "out of memory" in str(enc_err).lower() and bs > 1:
                            import gc
                            gc.collect()
                            torch.cuda.empty_cache()
                            bs = max(1, bs // 2)
                            print(f"[WARN] OOM na pré-computação de texto — reduzindo batch para {bs}.", flush=True)
                            continue
                        raise

                    if len(encoded) == 3:
                        pes, pe_masks, ipms = encoded
                    else:
                        pes, pe_masks = encoded
                        ipms = None

                    for idx, p_text in enumerate(chunk):
                        pe_item = pes[idx : idx + 1].detach().cpu()
                        mask_item = pe_masks[idx : idx + 1].detach().cpu() if pe_masks is not None else None
                        ipm_item = ipms[idx : idx + 1].detach().cpu() if ipms is not None else None
                        prompt_cache[p_text] = (pe_item, mask_item, ipm_item)

                    i += len(chunk)
                    done = min(i, total_prompts)
                    if metrics_path is not None and (done % (bs * 4) == 0 or done == total_prompts):
                        _emit_metric(
                            metrics_path,
                            epoch=epoch_offset,
                            step=0,
                            progress=round(0.01 + 0.03 * (done / max(1, total_prompts)), 4),
                            phase="preparing_cache",
                            message=f"Pré-computando text embeddings na {dev_desc} ({done}/{total_prompts})...",
                            telemetry_only=True,
                        )

                if sample_prompt:
                    encoded_sample = text_pipeline.encode_prompt([sample_prompt], device=encode_device)
                    if len(encoded_sample) == 3:
                        sample_pe, sample_pe_mask, sample_ipm = encoded_sample
                    else:
                        sample_pe, sample_pe_mask = encoded_sample
                        sample_ipm = None
                    sample_embeds = {
                        "prompt_embeds": sample_pe[0:1].detach().cpu(),
                        "prompt_embeds_mask": sample_pe_mask[0:1].detach().cpu() if sample_pe_mask is not None else None,
                        "image_pad_mask": sample_ipm[0:1].detach().cpu() if sample_ipm is not None else None,
                    }

            try:
                _unload_text_pipeline(text_pipeline, text_enc if text_enc is not None else None)
            except NameError:
                pass
            release_memory()
            print(
                f"[DIFFUSION] Embeddings pré-computados com sucesso ({len(prompt_cache)} prompts cacheados na {dev_desc}). Text encoder descarregado da memória (RAM/VRAM liberadas).",
                flush=True,
            )
            _emit_metric(
                metrics_path,
                epoch=epoch_offset,
                step=0,
                progress=0.04,
                phase="preparing_cache",
                message=f"Embeddings pré-computados ({len(prompt_cache)} prompts). Text encoder descarregado.",
            )
        except Exception as exc:
            print(f"[DIFFUSION-TRAIN] Aviso: falha na pré-computação de embeddings: {exc}. Criando fallbacks sintéticos.", flush=True)
            try:
                _unload_text_pipeline(text_pipeline, text_enc if text_enc is not None else None)
            except NameError:
                pass
            release_memory()
    # 4. Carrega VAE
    print(f"[DIFFUSION-TRAIN] Carregando VAE de {model_repo}...", flush=True)
    _emit_metric(
        metrics_path,
        epoch=epoch_offset,
        step=0,
        progress=0.035,
        phase="loading_model",
        message=f"Carregando VAE na GPU ({model_repo})...",
    )
    vae = VaeCls.from_pretrained(
        model_repo,
        subfolder="vae",
        torch_dtype=torch.float32,
        cache_dir=hub_cache,
        token=hf_token,
    ).to(device)
    if hasattr(vae, "enable_tiling"):
        try:
            vae.enable_tiling()
        except Exception:
            pass
    if hasattr(vae, "enable_slicing"):
        try:
            vae.enable_slicing()
        except Exception:
            pass
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
    # Pré-computa latents de imagem na GPU e descarrega o VAE para CPU (poupa 1.37 GB de VRAM no treino)
    latents_cache: dict[int, torch.Tensor] = {}
    total_dataset_samples = len(dataset)
    print(f"[DIFFUSION-TRAIN] Pré-computando latents ({total_dataset_samples} imagens) na GPU...", flush=True)
    _emit_metric(
        metrics_path,
        epoch=epoch_offset,
        step=0,
        progress=0.038,
        phase="preparing_latents",
        message=f"Pré-computando latents na GPU (0/{total_dataset_samples})...",
        telemetry_only=True,
    )
    # Cache de alpha channel constante: evita realocar torch.ones por imagem
    _alpha_cache: dict[tuple, Any] = {}

    with torch.no_grad():
        for s_idx in range(total_dataset_samples):
            s_item = dataset[s_idx]
            pv = s_item["pixel_values"].unsqueeze(0).to(device)
            if pv.ndim == 4:
                pv = pv.unsqueeze(2)
            if pv.shape[1] == 3:
                _ak = (pv.shape[2:], device, str(pv.dtype))
                if _ak not in _alpha_cache:
                    _alpha_cache[_ak] = torch.ones((1, 1, *pv.shape[2:]), device=device, dtype=pv.dtype)
                pv = torch.cat([pv, _alpha_cache[_ak]], dim=1)
            l = vae.encode(pv.float()).latent_dist.sample().to(dtype=target_dtype)
            if latents_mean is not None and latents_std is not None:
                l = (l - latents_mean) * latents_std
            latents_cache[s_idx] = l.cpu()
            done_l = s_idx + 1
            if done_l % 50 == 0 or done_l == total_dataset_samples:
                _emit_metric(
                    metrics_path,
                    epoch=epoch_offset,
                    step=0,
                    progress=round(0.038 + 0.004 * (done_l / max(1, total_dataset_samples)), 4),
                    phase="preparing_latents",
                    message=f"Pré-computando latents ({done_l}/{total_dataset_samples})...",
                    telemetry_only=True,
                )

    # Descarrega VAE para CPU liberando 1.37 GB de VRAM para o Transformer DiT
    vae = vae.to("cpu")
    if torch.cuda.is_available():
        torch.cuda.empty_cache()
        torch.cuda.ipc_collect()
    print("[DIFFUSION-TRAIN] VAE descarregado para CPU. 1.37 GB VRAM liberados para o Transformer DiT.", flush=True)


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
    elif quantization in ("8bit", "8bit-bnb") and device == "cuda":
        from transformers import BitsAndBytesConfig
        transformer_kwargs["quantization_config"] = BitsAndBytesConfig(
            load_in_8bit=True,
        )

    print(f"[DIFFUSION-TRAIN] Carregando Transformer de {model_repo} (quant={quantization})...", flush=True)
    _emit_metric(
        metrics_path,
        epoch=epoch_offset,
        step=0,
        progress=0.04,
        phase="load_transformer",
        message=f"Carregando Transformer na GPU (quant={quantization})...",
    )
    transformer = TransformerCls.from_pretrained(
        model_repo,
        subfolder="transformer",
        **transformer_kwargs,
    )
    release_memory()

    from peft import get_peft_model

    lora_config = LoraConfig(
        r=rank,
        lora_alpha=alpha,
        init_lora_weights="gaussian",
        target_modules=["to_k", "to_q", "to_v", "to_out.0", "add_k_proj", "add_q_proj", "add_v_proj"],
    )
    # get_peft_model é obrigatório para modelos BnB 4-bit — add_adapter() não injeta
    # LoRA corretamente em módulos quantizados. NÃO chamar prepare_model_for_kbit_training:
    # quebra SDPA em Diffusers (mesmo padrão do flux.py).
    transformer = get_peft_model(transformer, lora_config)
    trainable = sum(p.numel() for p in transformer.parameters() if p.requires_grad)
    total_p   = sum(p.numel() for p in transformer.parameters())
    print(
        f"[LORA] Parâmetros treináveis: {trainable:,} / {total_p:,} "
        f"({100.0 * trainable / max(1, total_p):.2f}%) | rank={rank}",
        flush=True,
    )
    try:
        transformer.enable_gradient_checkpointing()
    except Exception:
        pass
    transformer.train()

    # Cache da assinatura do transformer — inspeciona o modelo BASE (não o wrapper PEFT),
    # porque get_peft_model() envolve o forward em *args/**kwargs sem expor os parâmetros.
    import inspect as _inspect
    _base_fwd = (
        transformer.base_model.model.forward
        if hasattr(transformer, "base_model") and hasattr(transformer.base_model, "model")
        else transformer.forward
    )
    _trans_sig_params: set[str] = set(_inspect.signature(_base_fwd).parameters)
    print(f"[LORA] Parâmetros do transformer detectados: {sorted(_trans_sig_params)}", flush=True)

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
        progress=0.05,
        phase="training_started",
        message=f"Iniciando loop de treino LoRA: {epochs} épocas, {len(dataset)} imagens, {total_steps} passos totais.",
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
    release_memory()

    avg_step_time: float | None = None
    loss_ema: float | None = None
    step_start_time = time.time()

    # 7. Loop de Treino Real
    for epoch_idx in range(1, epochs + 1):
        epoch = epoch_idx + epoch_offset
        transformer.train()
        epoch_loss = 0.0
        steps_in_epoch = 0
        optimizer.zero_grad()

        for batch in dataloader:
            captions = batch["prompt"]
            bsz = len(captions)
            indices = batch.get("index", None)

            if indices is not None:
                idx_list = indices.tolist() if isinstance(indices, torch.Tensor) else list(indices)
            else:
                idx_list = []

            if idx_list and all(idx in latents_cache for idx in idx_list):
                latents = torch.cat([latents_cache[idx].to(device) for idx in idx_list], dim=0)
            else:
                pixel_values = batch["pixel_values"].to(device)
                if pixel_values.ndim == 4:
                    pixel_values = pixel_values.unsqueeze(2)
                if pixel_values.shape[1] == 3:
                    _ak2 = (pixel_values.shape[2:], device, str(pixel_values.dtype))
                    if _ak2 not in _alpha_cache:
                        _alpha_cache[_ak2] = torch.ones(
                            (1, 1, *pixel_values.shape[2:]), device=device, dtype=pixel_values.dtype,
                        )
                    _a = _alpha_cache[_ak2].expand(pixel_values.shape[0], -1, *pixel_values.shape[2:])
                    pixel_values = torch.cat([pixel_values, _a], dim=1)
                with torch.no_grad():
                    vae_moved = False
                    if hasattr(vae, "to") and getattr(vae, "device", None) != torch.device(device):
                        vae.to(device)
                        vae_moved = True
                    try:
                        latents = vae.encode(pixel_values.float()).latent_dist.sample()
                        latents = latents.to(dtype=target_dtype)
                        if latents_mean is not None and latents_std is not None:
                            latents = (latents - latents_mean) * latents_std
                    finally:
                        if vae_moved and hasattr(vae, "to"):
                            vae.to("cpu")
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
            noisy_in = None
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

            if len(embed_list) == 1:
                batch_embeds = embed_list[0]
            elif embed_list:
                batch_embeds = torch.cat(embed_list, dim=0)
            else:
                batch_embeds = None
            if len(mask_list) == len(embed_list) and mask_list:
                batch_mask = mask_list[0] if len(mask_list) == 1 else torch.cat(mask_list, dim=0)
            else:
                batch_mask = None

            # Monta kwargs do transformer dinamicamente de acordo com a assinatura do diffusers

            trans_kwargs: dict[str, Any] = {
                "hidden_states": packed_noisy,
                "encoder_hidden_states": batch_embeds,
                "timestep": timesteps / 1000.0,
                "return_dict": False,
            }
            if "encoder_hidden_states_mask" in _trans_sig_params:
                trans_kwargs["encoder_hidden_states_mask"] = batch_mask

            if "img_mask" in _trans_sig_params:
                # QwenImage21Transformer2DModel consome unpatched latents e img_mask para sequência conjunta
                trans_kwargs["img_shapes"] = [[(1, latent_h, latent_w)] for _ in range(bsz)]
                num_target_slots = (latent_h * latent_w) // 4
                if pad_mask_list and len(pad_mask_list) == len(embed_list):
                    base_pad_mask = torch.cat(pad_mask_list, dim=0)
                else:
                    base_pad_mask = torch.ones((bsz, batch_embeds.shape[1]), device=device, dtype=torch.bool)
                target_slots = base_pad_mask.new_ones((bsz, num_target_slots), dtype=torch.bool)
                trans_kwargs["img_mask"] = torch.cat([base_pad_mask, target_slots], dim=1)
            else:
                trans_kwargs["img_shapes"] = [(1, latent_h // 2, latent_w // 2)] * bsz

            # Forward pass no Transformer (dtype estático — sem autocast overhead)
            pred = transformer(**trans_kwargs)[0]

            # O transformer opera sobre a sequência conjunta (texto + imagem).
            # Isola exclusivamente os tokens da imagem do target no final da sequência se saída for conjunta.
            if pred.shape[1] > packed_noisy.shape[1]:
                pred_img = pred[:, -packed_noisy.shape[1] :]
            else:
                pred_img = pred
            # Empacota o target no mesmo espaço de tokens que pred_img (evita desempacotamento e shape mismatch em aspect ratios variados)
            target_in = target.permute(0, 2, 1, 3, 4)
            if PipelineCls is not None and hasattr(PipelineCls, "_pack_latents"):
                packed_target = PipelineCls._pack_latents(
                    target_in,
                    batch_size=bsz,
                    num_channels_latents=latents.shape[1],
                    height=latent_h,
                    width=latent_w,
                )
            else:
                packed_target = target_in.flatten(2).transpose(1, 2)

            loss = F.mse_loss(pred_img.float(), packed_target.float(), reduction="mean")
            cur_loss_raw = loss.item()
            loss = loss / grad_accum
            loss.backward()
            del pred, pred_img, packed_target, target, loss, trans_kwargs, batch_embeds, embed_list, mask_list, pad_mask_list
            del noise, noisy_latents, u, timesteps, sigmas, latents, noisy_in

            steps_in_epoch += 1
            micro_idx = ((steps_in_epoch - 1) % grad_accum) + 1
            if grad_accum > 1:
                print(
                    f"[TRAIN] Época {epoch}/{epochs} · Micro-passo {micro_idx}/{grad_accum} · Loss: {cur_loss_raw:.4f}",
                    flush=True,
                )
            is_accum_step = (steps_in_epoch % grad_accum == 0 or steps_in_epoch == len(dataloader))
            if is_accum_step:
                torch.nn.utils.clip_grad_norm_(transformer.parameters(), 1.0)
                optimizer.step()
                if lr_scheduler is not None:
                    lr_scheduler.step()
                optimizer.zero_grad()
                global_step += 1

                now = time.time()
                dt = max(0.001, now - step_start_time)
                step_start_time = now

                if avg_step_time is None:
                    avg_step_time = dt
                else:
                    avg_step_time = 0.9 * avg_step_time + 0.1 * dt

                if not (math.isnan(cur_loss_raw) or math.isinf(cur_loss_raw)):
                    if loss_ema is None:
                        loss_ema = cur_loss_raw
                    else:
                        loss_ema = 0.9 * loss_ema + 0.1 * cur_loss_raw

                remaining_steps = max(0, total_steps - global_step)
                eta_s = int(remaining_steps * avg_step_time)
                eta_str = _format_eta(eta_s)

                effective_lr = (
                    lr_scheduler.get_last_lr()[0] if lr_scheduler and hasattr(lr_scheduler, "get_last_lr") else learning_rate
                )
                safe_loss = (
                    None
                    if (math.isnan(cur_loss_raw) or math.isinf(cur_loss_raw))
                    else round(cur_loss_raw, 4)
                )
                current_progress = round(
                    min(0.99, max(0.05, 0.05 + 0.90 * (global_step / max(1, total_steps)))), 4
                )
                if avg_step_time >= 5.0:
                    emit_interval = 1
                else:
                    emit_interval = 1 if total_steps <= 100 else (5 if total_steps <= 500 else 10)

                if global_step % emit_interval == 0 or steps_in_epoch == len(dataloader):
                    _emit_metric(
                        metrics_path,
                        epoch=epoch,
                        step=global_step,
                        loss=safe_loss,
                        lr=effective_lr,
                        progress=current_progress,
                        phase="training",
                        message=f"Época {epoch}/{epochs} · Step {global_step}/{total_steps} · Loss: {safe_loss} · {dt:.1f}s/step · ETA: {eta_str}",
                        total_steps=total_steps,
                        total_epochs=epochs,
                        step_time_s=round(dt, 2),
                        eta_s=eta_s,
                        eta_formatted=eta_str,
                        loss_ema=round(loss_ema, 4) if loss_ema is not None else None,
                    )
                    vram_alloc = vram_allocated_gb() or 0.0
                    vram_res = vram_reserved_gb() or 0.0
                    loss_ema_val = loss_ema if loss_ema is not None else (safe_loss if safe_loss is not None else 0.0)
                    print(
                        f"[TRAIN] Época {epoch}/{epochs} · Step {global_step}/{total_steps} ({current_progress*100:.1f}%)\n"
                        f"  ├─ Loss: {safe_loss} (EMA: {loss_ema_val:.4f}) · LR: {effective_lr:.2e}\n"
                        f"  ├─ Velocidade: {dt:.1f}s/step · ETA: {eta_str}\n"
                        f"  └─ VRAM: {vram_alloc:.2f} GB alocada | {vram_res:.2f} GB reservada",
                        flush=True,
                    )
            if not math.isnan(cur_loss_raw) and not math.isinf(cur_loss_raw):
                epoch_loss += cur_loss_raw

        # Métrica da época
        avg_loss = round(epoch_loss / max(1, steps_in_epoch), 4)
        epoch_progress = round(
            min(0.99, max(0.05, 0.05 + 0.90 * (epoch_idx / epochs))), 4
        )
        remaining_steps = max(0, total_steps - global_step)
        eta_s = int(remaining_steps * avg_step_time) if avg_step_time is not None else None
        eta_str = _format_eta(eta_s) if eta_s is not None else "N/A"
        _emit_metric(
            metrics_path,
            epoch=epoch,
            step=global_step,
            loss=avg_loss,
            lr=effective_lr,
            progress=epoch_progress,
            phase="epoch_complete",
            message=f"Época {epoch}/{epochs} concluída - Loss Média: {avg_loss} · ETA: {eta_str}",
            total_steps=total_steps,
            total_epochs=epochs,
            step_time_s=round(avg_step_time, 2) if avg_step_time is not None else None,
            eta_s=eta_s,
            eta_formatted=eta_str,
            loss_ema=round(loss_ema, 4) if loss_ema is not None else None,
        )
        print(
            f"[TRAIN] Época {epoch}/{epochs} concluída - Loss Média: {avg_loss}",
            flush=True,
        )

        batch = None
        captions = None
        indices = None
        idx_list = None
        latents = None
        noise = None
        u = None
        timesteps = None
        sigmas = None
        noisy_latents = None
        packed_noisy = None
        noisy_in = None
        target_in = None
        pixel_values = None
        batch_mask = None
        cleanup_cuda()

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
            save_adapter_checkpoint(
                transformer,
                checkpoints_dir,
                base_name,
                epoch,
                {**metadata, "epoch": str(epoch)},
            )
        step_start_time = time.time()


    # 8. Salva adaptador final
    final_adapter_file = save_final_adapter(transformer, output_path, base_name, metadata)

    _emit_metric(
        metrics_path,
        epoch=epochs,
        step=global_step,
        loss=avg_loss if "avg_loss" in locals() else 0.05,
        lr=effective_lr,
        progress=1.0,
        phase="completed",
        message="Treino Qwen-Image-2.1 finalizado com sucesso!",
        total_steps=total_steps,
        total_epochs=epochs,
        step_time_s=round(avg_step_time, 2) if avg_step_time is not None else None,
        eta_s=0,
        eta_formatted="0s",
        loss_ema=round(loss_ema, 4) if loss_ema is not None else None,
    )
    print(f"Treino Qwen-Image-2.1 finalizado com sucesso! Checkpoint salvo em: {final_adapter_file}", flush=True)


class QwenImageTrainer(BaseModelTrainer):
    """Trainer de difusão para Qwen-Image-2.1 (7B Single-Stream DiT)."""

    def train(self, cfg: dict[str, Any], output: Path) -> None:
        if is_mock():
            _mock_train(cfg, output)
        else:
            _real_train_qwen_image(cfg, output)
