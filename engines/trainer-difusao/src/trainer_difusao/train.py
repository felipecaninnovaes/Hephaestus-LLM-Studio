"""Motor de treino de Difusão LoRA do Hephaestus (trainer-difusao).

Suporta SD 1.5, SDXL e FLUX.2 Klein 4B.
ENGINE_MOCK=1 (default no dev) → stdlib pura, gera metrics.jsonl e adapter.safetensors.
ENGINE_MOCK=0 (@gpu)           → treino real com PyTorch, Diffusers e PEFT.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import struct
import sys
import time
from pathlib import Path
from typing import Any

import yaml


def _die(msg: str) -> None:
    print(f"ERROR: {msg}", file=sys.stderr)
    sys.exit(1)


def _seed_bytes(seed: int, length: int = 1024) -> bytes:
    h = hashlib.sha256(struct.pack("<q", seed)).digest()
    out = bytearray()
    while len(out) < length:
        h = hashlib.sha256(h).digest()
        out.extend(h)
    return bytes(out[:length])


def _synthetic_loss(seed: int, epoch: int, total_epochs: int) -> float:
    raw = _seed_bytes(seed + epoch * 13, 16)
    val = struct.unpack("<d", raw[:8])[0]
    norm = abs(val) / (1e300 if abs(val) > 1e300 else 1.0)
    norm = (norm % 1.0) * 0.05
    decay = 0.5 * (1.0 - (epoch / (total_epochs + 1)))
    return round(max(0.01, decay + norm), 4)


def _canonical_model_name(raw_model: str) -> str:
    norm = raw_model.strip().lower()
    if norm in ("flux", "flux2", "flux-2", "flux2-klein-4b", "flux.2-klein-4b"):
        return "flux-2-klein-4b"
    if norm in ("sdxl", "sdxl-1.0"):
        return "sdxl"
    if norm in ("sd15", "sd-1.5", "stable-diffusion-v1-5"):
        return "sd15"
    return norm


def _emit_metric(
    metrics_path: Path,
    epoch: int,
    step: int,
    loss: float | None = None,
    lr: float | None = None,
    progress: float | None = None,
    phase: str | None = None,
    message: str | None = None,
) -> None:
    """Emite uma linha estruturada em metrics.jsonl com flush imediato para consumo pelo orquestrador."""
    try:
        metrics_path.parent.mkdir(parents=True, exist_ok=True)
        payload: dict[str, Any] = {
            "epoch": epoch,
            "step": step,
        }
        if loss is not None:
            payload["loss"] = loss
        if lr is not None:
            payload["lr"] = lr
        if progress is not None:
            payload["progress"] = progress
        if phase is not None:
            payload["phase"] = phase
        if message is not None:
            payload["message"] = message

        with open(metrics_path, "a", encoding="utf-8") as f:
            f.write(json.dumps(payload) + "\n")
            f.flush()
    except Exception as e:
        print(f"[WARN] Falha ao emitir métrica para {metrics_path}: {e}", file=sys.stderr, flush=True)


def _generate_mock_safetensors(output_file: Path, lora_params: dict[str, Any]) -> None:
    """Gera um arquivo .safetensors sintético em conformidade com a especificação HuggingFace."""
    base_model = lora_params.get("base_model", "flux-2-klein-4b")
    rank = int(lora_params.get("rank", 16))
    alpha = int(lora_params.get("alpha", 16))
    trigger_word = str(lora_params.get("trigger_word", ""))

    quantization = str(lora_params.get("quantization", "4bit"))
    metadata = {
        "format": "pt",
        "framework": "diffusers",
        "model_type": "lora",
        "lora_rank": str(rank),
        "lora_alpha": str(alpha),
        "base_model": str(base_model),
        "quantization": str(quantization),
    }
    if trigger_word:
        metadata["trigger_word"] = trigger_word

    if "flux" in base_model:
        tensor_name = "transformer.single_transformer_blocks.0.linear1.lora_A.weight"
    else:
        tensor_name = "lora_unet_up_blocks_0_attentions_0_proj_in.lora_down.weight"

    tensor_size = rank * 320 * 4
    header = {
        "__metadata__": metadata,
        tensor_name: {
            "dtype": "F32",
            "shape": [rank, 320],
            "data_offsets": [0, tensor_size],
        },
    }
    header_json = json.dumps(header).encode("utf-8")
    header_len = len(header_json)
    pad_len = (8 - (header_len % 8)) % 8
    header_json += b" " * pad_len
    header_len += pad_len

    data_bytes = b"\x00" * tensor_size

    with open(output_file, "wb") as f:
        f.write(struct.pack("<Q", header_len))
        f.write(header_json)
        f.write(data_bytes)


def _generate_mock_sample(
    output_dir: Path, epoch: int, prompt: str, seed: int = 42
) -> None:
    """Gera uma imagem de teste sintética para validação do fluxo de artefatos de sample."""
    samples_dir = output_dir / "samples"
    samples_dir.mkdir(parents=True, exist_ok=True)
    sample_file = samples_dir / f"sample_epoch_{epoch:03d}.png"
    try:
        from PIL import Image, ImageDraw

        img = Image.new("RGB", (512, 512), color=(24, 24, 37))
        draw = ImageDraw.Draw(img)
        draw.rectangle([16, 16, 496, 496], outline=(129, 140, 248), width=3)
        draw.text(
            (32, 210),
            f"Hephaestus Diffusion Sample\nEpoch: {epoch} | Seed: {seed}\nPrompt: {prompt[:50]}",
            fill=(240, 240, 250),
        )
        img.save(sample_file, format="PNG")
    except Exception:
        import base64

        # Fallback para PNG mínimo 1x1 se Pillow não estiver instalado
        tiny_png = base64.b64decode(
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkWPjfDwAEeQHzG4L5eAAAAABJRU5ErkJggg=="
        )
        sample_file.write_bytes(tiny_png)


def _mock_train(cfg: dict[str, Any], output: Path) -> None:
    output.mkdir(parents=True, exist_ok=True)
    metrics_path = output / "metrics.jsonl"

    seed = int(cfg.get("seed", 42))
    lora_cfg = cfg.get("lora", {})
    epochs = int(lora_cfg.get("epochs", 10))
    learning_rate = float(lora_cfg.get("learning_rate", 0.0001))
    raw_model = cfg.get("model", "flux")
    base_model = _canonical_model_name(raw_model)

    samples_cfg = cfg.get("samples", {})
    sample_prompt = str(samples_cfg.get("prompt", "") or "").strip()
    sample_interval = int(samples_cfg.get("interval", 1))
    sample_seed = int(samples_cfg.get("seed", seed))

    sleep_ms = int(os.environ.get("MOCK_EPOCH_SLEEP_MS", "5"))

    if metrics_path.exists():
        metrics_path.unlink()

    # Amostra baseline Época 0 (se configurada)
    if sample_prompt:
        _generate_mock_sample(output, 0, sample_prompt, seed=sample_seed)
        _emit_metric(
            metrics_path,
            epoch=0,
            step=1,
            progress=0.05,
            phase="baseline_ready",
            message="Amostra baseline gerada com sucesso (Época 0).",
        )

    for ep in range(1, epochs + 1):
        loss = _synthetic_loss(seed, ep, epochs)
        progress = round(ep / epochs, 4)
        _emit_metric(
            metrics_path,
            epoch=ep,
            step=ep * 10,
            loss=loss,
            lr=learning_rate,
            progress=progress,
            phase="training",
            message=f"Época {ep}/{epochs} concluída · Loss: {loss}",
        )

        if sample_prompt and sample_interval > 0 and (ep % sample_interval == 0 or ep == epochs):
            _generate_mock_sample(output, ep, sample_prompt, seed=sample_seed)

        if sleep_ms > 0:
            time.sleep(sleep_ms / 1000.0)

    adapter_path = output / "adapter.safetensors"
    lora_info = dict(lora_cfg)
    lora_info["base_model"] = base_model
    lora_info["quantization"] = str(lora_cfg.get("quantization") or cfg.get("quantization") or "4bit")
    _generate_mock_safetensors(adapter_path, lora_info)


# ==============================================================================
# PIPELINE REAL DE TREINO (ENGINE_MOCK=0 / @gpu)
# ==============================================================================


def _setup_cache_dir(hf_token: str | None = None) -> str:
    """Configura diretório de cache persistente para Hugging Face e PyTorch no volume /outputs."""
    if Path("/outputs").exists():
        cache_base = Path("/outputs/.cache/huggingface")
    else:
        cache_base = Path.home() / ".cache" / "huggingface"

    hub_cache = cache_base / "hub"
    hub_cache.mkdir(parents=True, exist_ok=True)
    (cache_base.parent / "torch").mkdir(parents=True, exist_ok=True)

    cache_base_str = str(cache_base)
    hub_cache_str = str(hub_cache)
    torch_cache_str = str(cache_base.parent / "torch")

    os.environ["HF_HOME"] = cache_base_str
    os.environ["HF_HUB_CACHE"] = hub_cache_str
    os.environ["HUGGINGFACE_HUB_CACHE"] = hub_cache_str
    os.environ["TRANSFORMERS_CACHE"] = hub_cache_str
    os.environ["DIFFUSERS_CACHE"] = hub_cache_str
    os.environ["TORCH_HOME"] = torch_cache_str

    token = (hf_token or os.environ.get("HF_TOKEN") or os.environ.get("HUGGING_FACE_HUB_TOKEN") or "").strip()
    if token:
        os.environ["HF_TOKEN"] = token
        os.environ["HUGGING_FACE_HUB_TOKEN"] = token
        try:
            import huggingface_hub
            huggingface_hub.login(token=token, add_to_git_credential=False)
            print("[INFO] Autenticado com sucesso no Hugging Face Hub via HF_TOKEN.", flush=True)
        except Exception as e:
            print(f"[WARN] Falha ao registrar token no huggingface_hub: {e}", flush=True)

    return hub_cache_str


class DiffusionDataset:
    """Dataset simples para leitura de pares imagem + legenda (.txt)."""

    def __init__(
        self, dataset_path: Path, resolution: int = 512, trigger_word: str = ""
    ):

        self.samples: list[tuple[Path, str]] = []
        self.resolution = resolution
        self.trigger_word = trigger_word.strip()

        # Busca em images/ ou na raiz do dataset
        target_dir = dataset_path / "images"
        if not target_dir.exists():
            target_dir = dataset_path

        valid_exts = {".webp", ".png", ".jpg", ".jpeg"}
        if target_dir.exists():
            for p in sorted(target_dir.iterdir()):
                if p.suffix.lower() in valid_exts:
                    txt_path = p.with_suffix(".txt")
                    caption = ""
                    if txt_path.exists():
                        caption = txt_path.read_text(encoding="utf-8").strip()
                    if not caption and self.trigger_word:
                        caption = self.trigger_word
                    self.samples.append((p, caption))

        if not self.samples:
            _die(f"Nenhuma imagem encontrada para treino em: {target_dir}")

    def __len__(self) -> int:
        return len(self.samples)

    def __getitem__(self, idx: int) -> dict[str, Any]:
        import numpy as np
        import torch
        from PIL import Image

        img_path, caption = self.samples[idx]
        image = Image.open(img_path).convert("RGB")
        image = image.resize(
            (self.resolution, self.resolution), Image.Resampling.BILINEAR
        )

        # Normaliza para [-1.0, 1.0]
        img_np = (np.array(image, dtype=np.float32) / 127.5) - 1.0
        # HWC -> CHW
        img_tensor = torch.from_numpy(img_np).permute(2, 0, 1)

        return {"pixel_values": img_tensor, "prompt": caption}


def _save_lora_safetensors(
    model: Any, output_file: Path, metadata: dict[str, str]
) -> None:
    """Salva os pesos do adaptador LoRA em formato .safetensors canônico."""
    import safetensors.torch
    from peft import get_peft_model_state_dict

    lora_state_dict = get_peft_model_state_dict(model)
    safetensors.torch.save_file(lora_state_dict, str(output_file), metadata=metadata)


def _create_optimizer(unet: Any, optimizer_name: str, lr: float) -> Any:
    """Cria otimizador selecionado (adamw8bit, adamw, prodigy)."""
    import torch

    opt_type = optimizer_name.lower().strip()
    if opt_type == "adamw8bit":
        try:
            import bitsandbytes as bnb

            print("Usando otimizador 8-bit AdamW (bitsandbytes).", flush=True)
            return bnb.optim.AdamW8bit(unet.parameters(), lr=lr)
        except Exception as e:
            print(
                f"[WARN] bitsandbytes não disponível ({e}), fallback para AdamW padrão.",
                flush=True,
            )
            return torch.optim.AdamW(unet.parameters(), lr=lr)
    elif opt_type == "prodigy":
        try:
            import prodigyopt

            print("Usando otimizador adaptativo Prodigy.", flush=True)
            return prodigyopt.Prodigy(unet.parameters(), lr=lr or 1.0)
        except Exception as e:
            print(
                f"[WARN] Prodigy não instalado ({e}), fallback para AdamW.",
                flush=True,
            )
            return torch.optim.AdamW(unet.parameters(), lr=lr)
    else:
        print("Usando otimizador AdamW (PyTorch).", flush=True)
        return torch.optim.AdamW(unet.parameters(), lr=lr)


def _create_lr_scheduler(
    optimizer: Any,
    scheduler_name: str,
    warmup_steps: int,
    total_steps: int,
) -> Any:
    """Cria scheduler de taxa de aprendizado via diffusers ou torch."""
    try:
        from diffusers.optimization import get_scheduler

        return get_scheduler(
            scheduler_name.lower().strip() or "cosine",
            optimizer=optimizer,
            num_warmup_steps=warmup_steps,
            num_training_steps=max(1, total_steps),
        )
    except Exception as e:
        print(
            f"[WARN] Não foi possível instanciar scheduler '{scheduler_name}': {e}",
            flush=True,
        )
        return None


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
    """Gera uma imagem de teste para SD 1.5 com os pesos LoRA ativos e seed fixa determinística."""
    try:
        import torch
        from diffusers import StableDiffusionPipeline

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
        generator = torch.Generator(device="cuda" if torch.cuda.is_available() else "cpu").manual_seed(seed)
        with torch.inference_mode():
            latents = pipe(prompt, generator=generator, num_inference_steps=20, guidance_scale=7.5, output_type="latent").images
            latents = latents.to(dtype=torch.float32) / 0.18215
            decoded = vae.decode(latents).sample
            image = (decoded / 2 + 0.5).clamp(0, 1)
            image = image.cpu().permute(0, 2, 3, 1).float().numpy()
            img = pipe.numpy_to_pil(image)[0]
            output_path.parent.mkdir(parents=True, exist_ok=True)
            img.save(output_path)
            print(f"[SD 1.5] Amostra de validação salva (seed={seed}) em: {output_path}", flush=True)
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
        from torch.utils.data import DataLoader
        from transformers import CLIPTextModel, CLIPTokenizer
    except ImportError as e:
        _die(f"Dependência ausente para treino real SD 1.5: {e}")

    if not torch.cuda.is_available():
        _die("CUDA não disponível para treino real de difusão (ENGINE_MOCK=0)")

    device = torch.device("cuda")

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

    samples_cfg = cfg.get("samples", {})
    sample_prompt = str(samples_cfg.get("prompt", "") or "").strip()
    sample_interval = int(samples_cfg.get("interval", 1))
    sample_seed = int(samples_cfg.get("seed", seed))

    resolution = int(lora_cfg.get("resolution", 512))
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
    quantization = str(lora_cfg.get("quantization") or cfg.get("quantization") or "none").lower().strip()

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
    tokenizer = CLIPTokenizer.from_pretrained(model_id, subfolder="tokenizer", cache_dir=hub_cache)
    text_encoder = CLIPTextModel.from_pretrained(
        model_id, subfolder="text_encoder", torch_dtype=target_dtype, cache_dir=hub_cache
    ).to(device)
    # VAE em float32 para prevenir underflow/overflow numérico (NaN)
    vae = AutoencoderKL.from_pretrained(
        model_id, subfolder="vae", torch_dtype=torch.float32, cache_dir=hub_cache
    ).to(device)
    unet = UNet2DConditionModel.from_pretrained(
        model_id, subfolder="unet", torch_dtype=target_dtype, cache_dir=hub_cache
    ).to(device)
    noise_scheduler = DDPMScheduler.from_pretrained(model_id, subfolder="scheduler", cache_dir=hub_cache)

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

    _emit_metric(
        metrics_path,
        epoch=0,
        step=3,
        progress=0.06,
        phase="setup_lora",
        message=f"Adaptadores LoRA injetados no UNet (rank={rank}, alpha={alpha}).",
    )

    optimizer = _create_optimizer(unet, optimizer_name, learning_rate)

    dataset = DiffusionDataset(dataset_path, resolution=resolution, trigger_word=trigger_word)
    dataloader = DataLoader(
        dataset, batch_size=batch_size, shuffle=True, drop_last=False
    )

    total_train_steps = max(1, (len(dataloader) * epochs) // grad_accum)
    lr_scheduler = _create_lr_scheduler(
        optimizer, lr_scheduler_name, lr_warmup_steps, total_train_steps
    )

    _emit_metric(
        metrics_path,
        epoch=0,
        step=4,
        progress=0.08,
        phase="dataset_ready",
        message=f"Dataset pronto: {len(dataset)} imagens.",
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
        epoch=0,
        step=7,
        progress=0.10,
        phase="training_started",
        message=f"Iniciando loop de treino SD 1.5: {epochs} épocas, {total_train_steps} passos totais.",
    )

    print(
        f"Iniciando treino LoRA SD 1.5: {epochs} épocas, {len(dataset)} imagens, res={resolution}, "
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
            captions = batch["prompt"]

            # Codifica imagens no espaço latente via VAE em float32, convertendo latents para target_dtype
            with torch.no_grad():
                latents = (vae.encode(pixel_values).latent_dist.sample() * 0.18215).to(dtype=target_dtype)

            # Adiciona ruído gaussiano aos latents
            noise = torch.randn_like(latents)
            timesteps = torch.randint(
                0,
                noise_scheduler.config.num_train_timesteps,
                (latents.shape[0],),
                device=device,
            ).long()
            noisy_latents = noise_scheduler.add_noise(latents, noise, timesteps)

            # Codifica texto das legendas
            with torch.no_grad():
                text_inputs = tokenizer(
                    captions,
                    padding="max_length",
                    max_length=tokenizer.model_max_length,
                    truncation=True,
                    return_tensors="pt",
                ).input_ids.to(device)
                encoder_hidden_states = text_encoder(text_inputs)[0]

            # Forward no UNet com LoRA
            model_pred = unet(noisy_latents, timesteps, encoder_hidden_states).sample
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
                    message=f"Época {epoch}/{epochs} · Step {global_step}/{total_train_steps} · Loss: {safe_loss}",
                )
                print(
                    f"[SD 1.5] Época {epoch}/{epochs} · Step {global_step} · Loss: {cur_loss_raw:.4f} · LR: {effective_lr:.2e}",
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
            f"[SD 1.5] Época {epoch}/{epochs} concluída - Step {global_step} - Loss Médio: {avg_loss}",
            flush=True,
        )

        if sample_prompt and sample_interval > 0 and (epoch % sample_interval == 0 or epoch == epochs):
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

    # Salva adapter.safetensors final
    adapter_file = output / "adapter.safetensors"
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
    _save_lora_safetensors(unet, adapter_file, metadata)
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
    print(f"Treino SD 1.5 finalizado com sucesso! Checkpoint salvo em: {adapter_file}")


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
    """Gera uma imagem de teste para SDXL com os pesos LoRA ativos e seed fixa determinística."""
    try:
        import torch
        from diffusers import StableDiffusionXLPipeline

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
        generator = torch.Generator(device="cuda" if torch.cuda.is_available() else "cpu").manual_seed(seed)
        with torch.inference_mode():
            latents = pipe(prompt, generator=generator, num_inference_steps=20, guidance_scale=7.0, output_type="latent").images
            latents = latents.to(dtype=torch.float32) / vae.config.scaling_factor
            decoded = vae.decode(latents).sample
            image = (decoded / 2 + 0.5).clamp(0, 1)
            image = image.cpu().permute(0, 2, 3, 1).float().numpy()
            img = pipe.numpy_to_pil(image)[0]
            output_path.parent.mkdir(parents=True, exist_ok=True)
            img.save(output_path)
            print(f"[SDXL] Amostra de validação salva (seed={seed}) em: {output_path}", flush=True)
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
    quantization = str(lora_cfg.get("quantization") or cfg.get("quantization") or "none").lower().strip()

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
        model_id, subfolder="text_encoder", torch_dtype=target_dtype, cache_dir=hub_cache
    ).to(device)
    text_encoder_two = CLIPTextModelWithProjection.from_pretrained(
        model_id, subfolder="text_encoder_2", torch_dtype=target_dtype, cache_dir=hub_cache
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
        optimizer, lr_scheduler_name, lr_warmup_steps, total_train_steps
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

        if sample_prompt and sample_interval > 0 and (epoch % sample_interval == 0 or epoch == epochs):
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

    adapter_file = output / "adapter.safetensors"
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
    _save_lora_safetensors(unet, adapter_file, metadata)
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
    print(f"Treino SDXL finalizado com sucesso! Checkpoint salvo em: {adapter_file}")


def _pack_latents(latents: Any) -> Any:
    """Empacota tensores latentes do VAE no formato patch 2x2 do FLUX.1: [B, C, H, W] -> [B, (H//2)*(W//2), C*4]."""
    b, c, h, w = latents.shape
    latents = latents.view(b, c, h // 2, 2, w // 2, 2)
    latents = latents.permute(0, 2, 4, 1, 3, 5)
    latents = latents.reshape(b, (h // 2) * (w // 2), c * 4)
    return latents


def _patchify_latents_flux2(latents: Any) -> Any:
    """Aplica patchify 2x2 nos latentes do FLUX.2 Klein: [B, C, H, W] -> [B, C*4, H//2, W//2]."""
    b, c, h, w = latents.shape
    latents = latents.view(b, c, h // 2, 2, w // 2, 2).permute(0, 1, 3, 5, 2, 4)
    return latents.reshape(b, c * 4, h // 2, w // 2)


def _pack_latents_flux2(latents: Any) -> Any:
    """Empacota latentes patchificados do FLUX.2 Klein para entrada no transformer: [B, C, H, W] -> [B, H*W, C]."""
    b, c, h, w = latents.shape
    return latents.reshape(b, c, h * w).permute(0, 2, 1)


def _prepare_latent_image_ids(
    batch_size: int, height: int, width: int, device: Any, dtype: Any
) -> Any:
    """Gera coordenadas de posição 2D para o Rotary Embedding (RoPE) de imagem do FLUX.1."""
    import torch

    h = height // 16
    w = width // 16
    latent_image_ids = torch.zeros(h, w, 3, device=device, dtype=dtype)
    latent_image_ids[..., 1] = latent_image_ids[..., 1] + torch.arange(h, device=device)[:, None]
    latent_image_ids[..., 2] = latent_image_ids[..., 2] + torch.arange(w, device=device)[None, :]
    latent_image_ids = latent_image_ids.reshape(h * w, 3)
    return latent_image_ids.repeat(batch_size, 1, 1)


def _prepare_text_ids(seq_len: int, device: Any, dtype: Any, batch_size: int = 1) -> Any:
    """Gera coordenadas 1D de posição para o Rotary Embedding (RoPE) textual do FLUX.1."""
    import torch

    txt_ids = torch.zeros(seq_len, 3, device=device, dtype=dtype)
    return txt_ids.repeat(batch_size, 1, 1)


def _prepare_flux2_latent_ids(latents: Any) -> Any:
    """Gera coordenadas de posição 4D (T, H, W, L) para o Rotary Embedding (RoPE) do FLUX.2 Klein."""
    import torch

    batch_size, _, height, width = latents.shape
    t = torch.arange(1, device=latents.device)
    h = torch.arange(height, device=latents.device)
    w = torch.arange(width, device=latents.device)
    l = torch.arange(1, device=latents.device)
    coords = torch.cartesian_prod(t, h, w, l)
    return coords.unsqueeze(0).expand(batch_size, -1, -1)


def _prepare_flux2_text_ids(prompt_embeds: Any) -> Any:
    """Gera coordenadas de posição 4D (T, H, W, L) para o Rotary Embedding (RoPE) textual do FLUX.2 Klein."""
    import torch

    batch_size, seq_len, _ = prompt_embeds.shape
    t = torch.arange(1, device=prompt_embeds.device)
    h = torch.arange(1, device=prompt_embeds.device)
    w = torch.arange(1, device=prompt_embeds.device)
    l = torch.arange(seq_len, device=prompt_embeds.device)
    coords = torch.cartesian_prod(t, h, w, l)
    return coords.unsqueeze(0).expand(batch_size, -1, -1)


def _encode_qwen3_prompt(
    text_encoder: Any,
    tokenizer: Any,
    prompts: list[str],
    device: Any,
    dtype: Any,
    max_length: int = 512,
    hidden_states_layers: tuple[int, ...] = (9, 18, 27),
) -> Any:
    """Codifica prompts de texto usando o modelo Qwen3 para FLUX.2 Klein 4B, extraindo e concatenando camadas intermediárias."""
    import torch

    all_input_ids = []
    all_attention_masks = []
    for p in prompts:
        if hasattr(tokenizer, "apply_chat_template") and getattr(tokenizer, "chat_template", None):
            messages = [{"role": "user", "content": p}]
            try:
                text = tokenizer.apply_chat_template(
                    messages,
                    tokenize=False,
                    add_generation_prompt=True,
                    enable_thinking=False,
                )
            except Exception:
                text = tokenizer.apply_chat_template(
                    messages,
                    tokenize=False,
                    add_generation_prompt=True,
                )
        else:
            text = p
        inputs = tokenizer(
            text,
            return_tensors="pt",
            padding="max_length",
            truncation=True,
            max_length=max_length,
        )
        all_input_ids.append(inputs["input_ids"])
        all_attention_masks.append(inputs["attention_mask"])

    input_ids = torch.cat(all_input_ids, dim=0).to(device)
    attention_mask = torch.cat(all_attention_masks, dim=0).to(device)

    with torch.no_grad():
        output = text_encoder(
            input_ids=input_ids,
            attention_mask=attention_mask,
            output_hidden_states=True,
            use_cache=False,
        )
        num_layers = len(output.hidden_states)
        layers_to_use = [k for k in hidden_states_layers if k < num_layers]
        if not layers_to_use:
            layers_to_use = [num_layers - 1]

        out = torch.stack([output.hidden_states[k] for k in layers_to_use], dim=1)
        out = out.to(dtype=dtype, device=device)

        batch_size, num_channels, seq_len, hidden_dim = out.shape
        prompt_embeds = out.permute(0, 2, 1, 3).reshape(batch_size, seq_len, num_channels * hidden_dim)

    return prompt_embeds


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
) -> None:
    """Gera uma imagem de teste para FLUX.2 Klein ou FLUX.1 com pesos LoRA ativos e seed fixa determinística."""
    try:
        import torch

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
                    image = pipe(
                        prompt=prompt,
                        generator=generator,
                        num_inference_steps=4,
                        guidance_scale=1.0,
                        height=resolution,
                        width=resolution,
                    ).images[0]
                    output_path.parent.mkdir(parents=True, exist_ok=True)
                    image.save(output_path)
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
            image = pipe(
                prompt=prompt,
                generator=generator,
                num_inference_steps=20,
                guidance_scale=3.5,
                height=resolution,
                width=resolution,
            ).images[0]
            output_path.parent.mkdir(parents=True, exist_ok=True)
            image.save(output_path)
            print(f"[FLUX] Amostra de validação salva (seed={seed}) em: {output_path}", flush=True)
    except Exception as e:
        print(f"[WARN] Falha ao gerar amostra de validação FLUX: {e}", flush=True)


def _real_train_flux(cfg: dict[str, Any], output: Path) -> None:
    """Treino real LoRA para FLUX.2 Klein 4B via Diffusers/PEFT com quantização 4-bit NF4 e persistência em cache."""
    output.mkdir(parents=True, exist_ok=True)
    metrics_path = output / "metrics.jsonl"
    if metrics_path.exists():
        metrics_path.unlink()

    hf_token = (
        cfg.get("hf_token")
        or os.environ.get("HF_TOKEN")
        or os.environ.get("HUGGING_FACE_HUB_TOKEN")
        or None
    )
    if hf_token:
        hf_token = hf_token.strip()

    hub_cache = _setup_cache_dir(hf_token=hf_token)

    try:
        import torch
        import torch.nn.functional as F
        from diffusers import (
            AutoencoderKL,
            FlowMatchEulerDiscreteScheduler,
            FluxPipeline,  # noqa: F401
            FluxTransformer2DModel,
        )
        from peft import LoraConfig, get_peft_model
        from torch.utils.data import DataLoader
        from transformers import (
            AutoModelForCausalLM,
            AutoTokenizer,
            BitsAndBytesConfig,
            CLIPTextModel,
            T5EncoderModel,
        )
    except ImportError as e:
        _die(f"Dependência ausente para treino real FLUX.2 Klein 4B: {e}")

    if not torch.cuda.is_available():
        _die("CUDA não disponível para treino real de difusão (ENGINE_MOCK=0)")

    device = torch.device("cuda")
    target_dtype = torch.bfloat16 if torch.cuda.is_bf16_supported() else torch.float16

    seed = int(cfg.get("seed", 42))
    model_id = (
        cfg.get("model_id")
        or os.environ.get("FLUX_MODEL_ID")
        or "unsloth/FLUX.2-klein-4B"
    )
    is_flux2 = any(k in model_id.lower() for k in ["klein", "flux.2", "flux-2"])

    dataset_path = Path(cfg.get("dataset_path", "/datasets"))
    lora_cfg = cfg.get("lora", {})
    epochs = int(lora_cfg.get("epochs", 10))
    batch_size = int(lora_cfg.get("batch_size", 1))
    learning_rate = float(lora_cfg.get("learning_rate", 1e-4))
    rank = int(lora_cfg.get("rank", 16))
    alpha = int(lora_cfg.get("alpha", 16))
    trigger_word = str(lora_cfg.get("trigger_word", ""))

    quantization = str(
        lora_cfg.get("quantization")
        or cfg.get("quantization")
        or os.environ.get("FLUX_QUANTIZATION")
        or "4bit"
    ).lower().strip()
    is_4bit = quantization in ("4bit", "4bit-nf4", "nf4")
    is_8bit = quantization in ("8bit", "8bit-bnb", "int8")
    is_quantized = is_4bit or is_8bit

    if is_4bit:
        quant_label = "4-bit NF4"
        subfolder_quant = "flux2_klein_4bit" if is_flux2 else "flux1_4bit"
        bnb_config = BitsAndBytesConfig(
            load_in_4bit=True,
            bnb_4bit_quant_type="nf4",
            bnb_4bit_compute_dtype=target_dtype,
            bnb_4bit_use_double_quant=True,
        )
    elif is_8bit:
        quant_label = "8-bit BitsAndBytes"
        subfolder_quant = "flux2_klein_8bit" if is_flux2 else "flux1_8bit"
        bnb_config = BitsAndBytesConfig(
            load_in_8bit=True,
        )
    else:
        quant_label = "Nenhum (FP16/BF16 pleno)"
        subfolder_quant = None
        bnb_config = None

    _emit_metric(
        metrics_path,
        epoch=0,
        step=1,
        progress=0.01,
        phase="init",
        message=f"Inicializando motor FLUX: {model_id} ({quant_label})...",
    )

    # Classes condicionais para FLUX.2 / Klein se disponíveis no Diffusers instalado
    Flux2Transformer_cls = FluxTransformer2DModel
    AutoencoderKL_cls = AutoencoderKL
    if is_flux2:
        try:
            from diffusers import Flux2Transformer2DModel
            Flux2Transformer_cls = Flux2Transformer2DModel
        except ImportError:
            pass
        try:
            from diffusers import AutoencoderKLFlux2
            AutoencoderKL_cls = AutoencoderKLFlux2
        except ImportError:
            pass

    samples_cfg = cfg.get("samples", {})
    sample_prompt = str(samples_cfg.get("prompt", "") or "").strip()
    sample_interval = int(samples_cfg.get("interval", 1))
    sample_seed = int(samples_cfg.get("seed", seed))

    resolution = int(lora_cfg.get("resolution", 512))
    grad_accum = max(1, int(lora_cfg.get("gradient_accumulation_steps", 1)))
    optimizer_name = str(lora_cfg.get("optimizer", "adamw8bit"))
    lr_scheduler_name = str(lora_cfg.get("lr_scheduler", "cosine"))
    lr_warmup_steps = int(lora_cfg.get("lr_warmup_steps", 0))

    # Diretório persistente de cache para pesos pré-quantizados (evita re-quantizar a cada job)
    if subfolder_quant:
        quant_base = (
            Path(f"/outputs/.cache/quantized/{subfolder_quant}")
            if Path("/outputs").exists()
            else Path.home() / ".cache" / "hephaestus" / "quantized" / subfolder_quant
        )
        transformer_cache_dir = quant_base / "transformer"
        text_encoder_cache_dir = quant_base / ("text_encoder" if is_flux2 else "text_encoder_2")
        quant_base.mkdir(parents=True, exist_ok=True)
    else:
        transformer_cache_dir = None
        text_encoder_cache_dir = None

    print(
        f"Carregando modelos base FLUX ({model_id}) [is_flux2={is_flux2}, quantização: {quant_label}, res: {resolution}, dtype: {target_dtype}]...",
        flush=True,
    )

    # 1. Carregamento do Transformer (DiT): do cache quantizado se já existir, senão quantiza e salva
    if transformer_cache_dir and transformer_cache_dir.exists() and (transformer_cache_dir / "config.json").exists():
        _emit_metric(
            metrics_path,
            epoch=0,
            step=2,
            progress=0.03,
            phase="load_transformer",
            message=f"Carregando Transformer quantizado em {quant_label} do cache persistente...",
        )
        print(
            f"Carregando Transformer quantizado em {quant_label} do cache persistente: {transformer_cache_dir}",
            flush=True,
        )
        transformer = Flux2Transformer_cls.from_pretrained(
            transformer_cache_dir,
            torch_dtype=target_dtype,
        )
    else:
        step_msg_trans = (
            f"Baixando e quantizando Transformer FLUX em {quant_label} ({model_id})..."
            if is_quantized
            else f"Baixando e carregando Transformer FLUX em precisão plena ({model_id})..."
        )
        _emit_metric(
            metrics_path,
            epoch=0,
            step=2,
            progress=0.02,
            phase="quantizing_transformer" if is_quantized else "load_transformer",
            message=step_msg_trans,
        )
        print(step_msg_trans, flush=True)
        try:
            transformer = Flux2Transformer_cls.from_pretrained(
                model_id,
                subfolder="transformer",
                quantization_config=bnb_config,
                torch_dtype=target_dtype,
                cache_dir=hub_cache,
                token=hf_token,
            )
        except Exception as e:
            if "gated" in str(e).lower() or "401" in str(e) or "403" in str(e) or "not a valid model identifier" in str(e).lower():
                _die(
                    f"Falha ao baixar modelo FLUX ({model_id}). Este repositório é restrito no Hugging Face.\n"
                    f"1. Aceite a licença do modelo em https://huggingface.co/{model_id}\n"
                    f"2. Defina a variável HF_TOKEN no env.gpu com o seu token de acesso: https://huggingface.co/settings/tokens\n"
                    f"Erro original: {e}"
                )
            raise
        if transformer_cache_dir:
            try:
                transformer_cache_dir.mkdir(parents=True, exist_ok=True)
                transformer.save_pretrained(transformer_cache_dir)
                print(
                    f"Transformer {quant_label} persistido em cache para execuções futuras: {transformer_cache_dir}",
                    flush=True,
                )
            except Exception as e:
                print(
                    f"[WARN] Não foi possível persistir transformer quantizado em disco: {e}",
                    flush=True,
                )

    _emit_metric(
        metrics_path,
        epoch=0,
        step=3,
        progress=0.04,
        phase="transformer_ready",
        message=f"Transformer FLUX ({quant_label}) carregado com sucesso.",
    )

    # 2. Carregamento do(s) Text Encoder(s)
    if is_flux2:
        # FLUX.2 Klein: utiliza um único Text Encoder Qwen3
        tokenizer_two = None
        text_encoder_two = None

        if text_encoder_cache_dir and text_encoder_cache_dir.exists() and (text_encoder_cache_dir / "config.json").exists():
            _emit_metric(
                metrics_path,
                epoch=0,
                step=4,
                progress=0.05,
                phase="load_text_encoder",
                message=f"Carregando Text Encoder Qwen3 quantizado em {quant_label} do cache persistente...",
            )
            print(
                f"Carregando Text Encoder Qwen3 quantizado em {quant_label} do cache persistente: {text_encoder_cache_dir}",
                flush=True,
            )
            text_encoder_one = AutoModelForCausalLM.from_pretrained(
                text_encoder_cache_dir,
                torch_dtype=target_dtype,
            )
        else:
            step_msg_enc = (
                f"Baixando e quantizando Text Encoder Qwen3 em {quant_label} ({model_id})..."
                if is_quantized
                else f"Baixando e carregando Text Encoder Qwen3 em precisão plena ({model_id})..."
            )
            _emit_metric(
                metrics_path,
                epoch=0,
                step=4,
                progress=0.04,
                phase="quantizing_text_encoder" if is_quantized else "load_text_encoder",
                message=step_msg_enc,
            )
            print(step_msg_enc, flush=True)
            text_encoder_one = AutoModelForCausalLM.from_pretrained(
                model_id,
                subfolder="text_encoder",
                quantization_config=bnb_config,
                torch_dtype=target_dtype,
                cache_dir=hub_cache,
                token=hf_token,
            )
            if text_encoder_cache_dir:
                try:
                    text_encoder_cache_dir.mkdir(parents=True, exist_ok=True)
                    text_encoder_one.save_pretrained(text_encoder_cache_dir)
                    print(
                        f"Text Encoder Qwen3 {quant_label} persistido em cache para execuções futuras: {text_encoder_cache_dir}",
                        flush=True,
                    )
                except Exception as e:
                    print(
                        f"[WARN] Não foi possível persistir Text Encoder Qwen3 quantizado em disco: {e}",
                        flush=True,
                    )

        tokenizer_one = AutoTokenizer.from_pretrained(
            model_id, subfolder="tokenizer", cache_dir=hub_cache, token=hf_token
        )
    else:
        # FLUX.1: utiliza Text Encoder CLIP + T5-XXL
        if text_encoder_cache_dir and text_encoder_cache_dir.exists() and (text_encoder_cache_dir / "config.json").exists():
            _emit_metric(
                metrics_path,
                epoch=0,
                step=4,
                progress=0.05,
                phase="load_text_encoder",
                message=f"Carregando Text Encoder T5 quantizado em {quant_label} do cache persistente...",
            )
            print(
                f"Carregando Text Encoder T5 quantizado em {quant_label} do cache persistente: {text_encoder_cache_dir}",
                flush=True,
            )
            text_encoder_two = T5EncoderModel.from_pretrained(
                text_encoder_cache_dir,
                torch_dtype=target_dtype,
            )
        else:
            step_msg_t5 = (
                f"Baixando e quantizando Text Encoder T5 em {quant_label} ({model_id})..."
                if is_quantized
                else f"Baixando e carregando Text Encoder T5 em precisão plena ({model_id})..."
            )
            _emit_metric(
                metrics_path,
                epoch=0,
                step=4,
                progress=0.04,
                phase="quantizing_text_encoder" if is_quantized else "load_text_encoder",
                message=step_msg_t5,
            )
            print(step_msg_t5, flush=True)
            text_encoder_two = T5EncoderModel.from_pretrained(
                model_id,
                subfolder="text_encoder_2",
                quantization_config=bnb_config,
                torch_dtype=target_dtype,
                cache_dir=hub_cache,
                token=hf_token,
            )
            if text_encoder_cache_dir:
                try:
                    text_encoder_cache_dir.mkdir(parents=True, exist_ok=True)
                    text_encoder_two.save_pretrained(text_encoder_cache_dir)
                    print(
                        f"Text Encoder T5 {quant_label} persistido em cache para execuções futuras: {text_encoder_cache_dir}",
                        flush=True,
                    )
                except Exception as e:
                    print(
                        f"[WARN] Não foi possível persistir Text Encoder T5 quantizado em disco: {e}",
                        flush=True,
                    )

        tokenizer_one = AutoTokenizer.from_pretrained(
            model_id, subfolder="tokenizer", use_fast=False, cache_dir=hub_cache, token=hf_token
        )
        tokenizer_two = AutoTokenizer.from_pretrained(
            model_id, subfolder="tokenizer_2", use_fast=False, cache_dir=hub_cache, token=hf_token
        )
        text_encoder_one = CLIPTextModel.from_pretrained(
            model_id, subfolder="text_encoder", torch_dtype=target_dtype, cache_dir=hub_cache, token=hf_token
        ).to(device)

    _emit_metric(
        metrics_path,
        epoch=0,
        step=5,
        progress=0.06,
        phase="text_encoder_ready",
        message=f"Text Encoder ({quant_label}) pronto.",
    )

    # 3. Componentes auxiliares (VAE float32, Scheduler Flow Matching)
    vae = AutoencoderKL_cls.from_pretrained(
        model_id, subfolder="vae", torch_dtype=torch.float32, cache_dir=hub_cache, token=hf_token
    ).to(device)
    noise_scheduler = FlowMatchEulerDiscreteScheduler.from_pretrained(
        model_id, subfolder="scheduler", cache_dir=hub_cache, token=hf_token
    )

    vae.requires_grad_(False)
    text_encoder_one.requires_grad_(False)
    if text_encoder_two is not None:
        text_encoder_two.requires_grad_(False)
    transformer.requires_grad_(False)

    # 4. Injeção de adaptadores LoRA via PEFT nas camadas lineares do Transformer FLUX
    target_modules = [
        "to_k", "to_q", "to_v", "to_out.0",
        "add_k_proj", "add_v_proj", "add_q_proj", "to_add_out",
        "to_qkv_mlp_proj", "to_out_mlp_proj",
        "linear1", "linear2",
    ]
    lora_config = LoraConfig(
        r=rank,
        lora_alpha=alpha,
        init_lora_weights="gaussian",
        target_modules=target_modules,
    )
    transformer = get_peft_model(transformer, lora_config)
    transformer.enable_gradient_checkpointing()
    transformer.train()

    _emit_metric(
        metrics_path,
        epoch=0,
        step=6,
        progress=0.07,
        phase="setup_lora",
        message=f"Adaptadores LoRA injetados no Transformer (rank={rank}, alpha={alpha}).",
    )

    # 5. Dataset de treino
    dataset = DiffusionDataset(
        dataset_path, resolution=resolution, trigger_word=trigger_word
    )
    if len(dataset) == 0:
        _die(f"Nenhum par imagem+legenda (.txt) encontrado em: {dataset_path}")

    dataloader = DataLoader(
        dataset,
        batch_size=batch_size,
        shuffle=True,
        drop_last=False,
    )

    _emit_metric(
        metrics_path,
        epoch=0,
        step=7,
        progress=0.08,
        phase="dataset_ready",
        message=f"Dataset carregado com sucesso: {len(dataset)} amostras.",
    )

    # 6. Otimizador e LR Scheduler
    optimizer = _create_optimizer(transformer, optimizer_name, learning_rate)
    total_train_steps = (len(dataloader) * epochs) // grad_accum
    lr_scheduler = _create_lr_scheduler(
        optimizer, lr_scheduler_name, total_train_steps, lr_warmup_steps
    )

    # Amostra baseline (Época 0) para comparação pré-treino
    if sample_prompt:
        _emit_metric(
            metrics_path,
            epoch=0,
            step=8,
            progress=0.09,
            phase="generating_baseline_sample",
            message=f"Gerando amostra baseline pré-treino (Época 0): '{sample_prompt[:40]}...'",
        )
        sample_baseline_file = output / "samples" / "sample_epoch_000.png"
        _generate_sample_flux(
            transformer=transformer,
            vae=vae,
            text_encoder_one=text_encoder_one,
            text_encoder_two=text_encoder_two,
            tokenizer_one=tokenizer_one,
            tokenizer_two=tokenizer_two,
            scheduler=noise_scheduler,
            prompt=sample_prompt,
            output_path=sample_baseline_file,
            seed=sample_seed,
            is_flux2=is_flux2,
            resolution=resolution,
        )
        _emit_metric(
            metrics_path,
            epoch=0,
            step=9,
            progress=0.10,
            phase="baseline_ready",
            message="Amostra baseline gerada com sucesso (Época 0).",
        )

    _emit_metric(
        metrics_path,
        epoch=0,
        step=10,
        progress=0.10,
        phase="training_started",
        message=f"Iniciando loop de treino LoRA: {epochs} épocas, {total_train_steps} passos totais.",
    )

    print(
        f"Iniciando treino LoRA FLUX (is_flux2={is_flux2}, 4-bit NF4): {epochs} épocas, {len(dataset)} imagens, "
        f"rank={rank}, alpha={alpha}, lr={learning_rate}, res={resolution}px, ga={grad_accum}x",
        flush=True,
    )

    shift_factor = getattr(vae.config, "shift_factor", 0.0)
    scaling_factor = getattr(vae.config, "scaling_factor", 0.3611)

    global_step = 0
    safe_avg_loss = None
    for epoch in range(1, epochs + 1):
        epoch_loss = 0.0
        steps_in_epoch = 0
        optimizer.zero_grad()

        for batch in dataloader:
            pixel_values = batch["pixel_values"].to(device)
            captions = batch["prompt"]
            bsz = pixel_values.shape[0]

            # Codifica imagens com VAE (em float32 para evitar instabilidade numérica)
            with torch.no_grad():
                latents = vae.encode(pixel_values.float()).latent_dist.sample()

                if is_flux2:
                    latents = _patchify_latents_flux2(latents)
                    if hasattr(vae, "bn") and getattr(vae.bn, "running_mean", None) is not None:
                        latents_bn_mean = vae.bn.running_mean.view(1, -1, 1, 1).to(latents.device, latents.dtype)
                        latents_bn_std = torch.sqrt(
                            vae.bn.running_var.view(1, -1, 1, 1) + getattr(vae.config, "batch_norm_eps", 1e-5)
                        ).to(latents.device, latents.dtype)
                        latents = (latents - latents_bn_mean) / latents_bn_std
                    else:
                        latents = (latents - shift_factor) * scaling_factor
                    latents = latents.to(dtype=target_dtype)
                    img_ids = _prepare_flux2_latent_ids(latents)
                    packed_latents = _pack_latents_flux2(latents)

                    prompt_embeds = _encode_qwen3_prompt(
                        text_encoder_one, tokenizer_one, captions, device, target_dtype
                    )
                    txt_ids = _prepare_flux2_text_ids(prompt_embeds)
                    pooled_prompt_embeds = None
                else:
                    latents = (latents - shift_factor) * scaling_factor
                    latents = latents.to(dtype=target_dtype)
                    packed_latents = _pack_latents(latents)
                    img_ids = _prepare_latent_image_ids(bsz, resolution, resolution, device, target_dtype)

                    clip_inputs = tokenizer_one(
                        captions,
                        padding="max_length",
                        max_length=77,
                        truncation=True,
                        return_tensors="pt",
                    ).to(device)
                    pooled_prompt_embeds = text_encoder_one(clip_inputs.input_ids).pooler_output

                    t5_inputs = tokenizer_two(
                        captions,
                        padding="max_length",
                        max_length=512,
                        truncation=True,
                        return_tensors="pt",
                    ).to(device)
                    prompt_embeds = text_encoder_two(t5_inputs.input_ids)[0]
                    txt_ids = _prepare_text_ids(prompt_embeds.shape[1], device, prompt_embeds.dtype, batch_size=bsz)

            # Ruído gaussiano e timesteps aleatórios para Flow Matching
            noise = torch.randn_like(packed_latents)
            u = torch.normal(mean=0.0, std=1.0, size=(bsz,), device=device)
            timesteps = torch.sigmoid(u)

            # Interpolação do fluxo retificado: x_t = (1 - t) * x_0 + t * noise
            t_expanded = timesteps.view(-1, 1, 1).to(dtype=target_dtype)
            noisy_latents = (1.0 - t_expanded) * packed_latents + t_expanded * noise
            target = noise - packed_latents

            # Forward no Transformer FLUX com adaptadores LoRA ativos
            if is_flux2:
                model_pred = transformer(
                    hidden_states=noisy_latents,
                    timestep=timesteps,
                    encoder_hidden_states=prompt_embeds,
                    txt_ids=txt_ids,
                    img_ids=img_ids,
                    return_dict=False,
                )[0]
            else:
                guidance = torch.full((bsz,), 3.5, device=device, dtype=target_dtype)
                model_pred = transformer(
                    hidden_states=noisy_latents,
                    timestep=timesteps,
                    guidance=guidance,
                    pooled_projections=pooled_prompt_embeds,
                    encoder_hidden_states=prompt_embeds,
                    txt_ids=txt_ids,
                    img_ids=img_ids,
                    return_dict=False,
                )[0]

            loss = F.mse_loss(model_pred.float(), target.float(), reduction="mean")
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
                lr_scheduler.get_last_lr()[0] if lr_scheduler else learning_rate
            )

            # Emite métricas intermediárias a cada 5 passos com flush e progresso contínuo
            if global_step % 5 == 0 or steps_in_epoch == len(dataloader):
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
                    message=f"Época {epoch}/{epochs} · Step {global_step}/{total_train_steps} · Loss: {safe_loss}",
                )
                print(
                    f"[FLUX] Época {epoch}/{epochs} · Step {global_step}/{total_train_steps} · Loss: {safe_loss} · LR: {effective_lr:.2e}",
                    flush=True,
                )

        avg_loss = epoch_loss / max(1, steps_in_epoch)
        safe_avg_loss = (
            None
            if (math.isnan(avg_loss) or math.isinf(avg_loss))
            else round(avg_loss, 4)
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
            message=f"Época {epoch}/{epochs} concluída · Loss Média: {safe_avg_loss}",
        )

        print(
            f"[FLUX] Concluída Época {epoch}/{epochs} · Loss Média: {safe_avg_loss} · LR: {effective_lr:.2e}",
            flush=True,
        )

        # Geração de amostra visual periódica
        if sample_prompt and sample_interval > 0 and (epoch % sample_interval == 0 or epoch == epochs):
            sample_file = output / "samples" / f"sample_epoch_{epoch:03d}.png"
            _generate_sample_flux(
                transformer=transformer,
                vae=vae,
                text_encoder_one=text_encoder_one,
                text_encoder_two=text_encoder_two,
                tokenizer_one=tokenizer_one,
                tokenizer_two=tokenizer_two,
                scheduler=noise_scheduler,
                prompt=sample_prompt,
                output_path=sample_file,
                seed=sample_seed,
                is_flux2=is_flux2,
                resolution=resolution,
            )

    # Salva adaptador LoRA final em safetensors com metadados
    adapter_file = output / "adapter.safetensors"
    metadata = {
        "format": "pt",
        "model_type": "lora",
        "base_model": "flux-2-klein-4b" if is_flux2 else "flux-1",
        "lora_rank": str(rank),
        "lora_alpha": str(alpha),
        "trigger_word": trigger_word,
        "quantization": quantization,
    }
    _save_lora_safetensors(transformer, adapter_file, metadata)
    _emit_metric(
        metrics_path,
        epoch=epochs,
        step=global_step,
        loss=safe_avg_loss,
        lr=effective_lr,
        progress=1.0,
        phase="completed",
        message="Treino FLUX LoRA finalizado com sucesso!",
    )
    print(f"Treino FLUX finalizado com sucesso! Checkpoint salvo em: {adapter_file}")


def _real_train(cfg: dict[str, Any], output: Path) -> None:
    raw_model = cfg.get("model", "sdxl")
    base_model = _canonical_model_name(raw_model)

    if "flux" in base_model:
        _real_train_flux(cfg, output)
    elif base_model == "sdxl":
        _real_train_sdxl(cfg, output)
    elif base_model == "sd15":
        _real_train_sd15(cfg, output)
    else:
        _die(f"Modelo base de difusão desconhecido: {base_model}")


def cmd_train(args: list[str]) -> None:
    parser = argparse.ArgumentParser(
        prog="trainer-difusao train",
        description="Diffusion LoRA trainer — FLUX.2 Klein 4B, SDXL, SD 1.5 (mock/real)",
    )
    parser.add_argument("--config", required=True, help="Caminho para config.yaml")
    parser.add_argument("--output", required=True, help="Diretório de saída")

    opts = parser.parse_args(args)
    if not os.path.exists(opts.config):
        _die(f"Arquivo de configuração não encontrado: {opts.config}")

    with open(opts.config, "r", encoding="utf-8") as f:
        cfg = yaml.safe_load(f)

    output = Path(opts.output)

    is_mock = os.environ.get("ENGINE_MOCK", "1") == "1"
    if is_mock:
        _mock_train(cfg, output)
    else:
        _real_train(cfg, output)


def cmd_health() -> None:
    is_mock = os.environ.get("ENGINE_MOCK", "1") == "1"
    mode = "mock" if is_mock else "real"
    print(json.dumps({"status": "ok", "engine": "trainer-difusao", "mode": mode}))


def main(argv: list[str] | None = None) -> None:
    if argv is None:
        argv = sys.argv[1:]

    if not argv:
        cmd_health()
        return

    if argv[0] in ("-h", "--help"):
        print("Uso: python -m trainer_difusao [health|train|generate] [args...]")
        return

    if argv[0] == "train":
        cmd_train(argv[1:])
    elif argv[0] == "generate":
        from trainer_difusao.generate import cmd_generate
        cmd_generate(argv[1:])
    elif argv[0] == "health":
        cmd_health()
    else:
        _die(f"Subcomando desconhecido: {argv[0]}")


if __name__ == "__main__":
    main()
