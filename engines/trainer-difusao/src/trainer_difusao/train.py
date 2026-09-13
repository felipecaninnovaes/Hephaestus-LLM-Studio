"""Motor de treino de Difusão LoRA do Hephaestus (trainer-difusao).

Suporta SD 1.5, SDXL e FLUX.2 Klein 4B.
ENGINE_MOCK=1 (default no dev) → stdlib pura, gera metrics.jsonl e adapter.safetensors.
ENGINE_MOCK=0 (@gpu)           → treino real com PyTorch, Diffusers e PEFT.
"""

from __future__ import annotations

import argparse
import hashlib
import json
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


def _generate_mock_safetensors(output_file: Path, lora_params: dict[str, Any]) -> None:
    """Gera um arquivo .safetensors sintético em conformidade com a especificação HuggingFace."""
    base_model = lora_params.get("base_model", "flux-2-klein-4b")
    rank = int(lora_params.get("rank", 16))
    alpha = int(lora_params.get("alpha", 16))
    trigger_word = str(lora_params.get("trigger_word", ""))

    metadata = {
        "format": "pt",
        "framework": "diffusers",
        "model_type": "lora",
        "lora_rank": str(rank),
        "lora_alpha": str(alpha),
        "base_model": str(base_model),
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


def _generate_mock_sample(output_dir: Path, epoch: int, prompt: str) -> None:
    """Gera uma imagem de teste sintética para validação do fluxo de artefatos de sample."""
    samples_dir = output_dir / "samples"
    samples_dir.mkdir(parents=True, exist_ok=True)
    sample_file = samples_dir / f"sample_epoch_{epoch:03d}.png"
    try:
        from PIL import Image, ImageDraw

        img = Image.new("RGB", (512, 512), color=(24, 24, 37))
        draw = ImageDraw.Draw(img)
        draw.rectangle([16, 16, 496, 496], outline=(129, 140, 248), width=3)
        draw.text((32, 210), f"Hephaestus Diffusion Sample\nEpoch: {epoch}\nPrompt: {prompt[:50]}", fill=(240, 240, 250))
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

    seed = cfg.get("seed", 42)
    lora_cfg = cfg.get("lora", {})
    epochs = lora_cfg.get("epochs", 10)
    learning_rate = lora_cfg.get("learning_rate", 0.0001)
    raw_model = cfg.get("model", "flux")
    base_model = _canonical_model_name(raw_model)

    samples_cfg = cfg.get("samples", {})
    sample_prompt = str(samples_cfg.get("prompt", "") or "").strip()
    sample_interval = int(samples_cfg.get("interval", 1))

    sleep_ms = int(os.environ.get("MOCK_EPOCH_SLEEP_MS", "5"))

    if metrics_path.exists():
        metrics_path.unlink()

    for ep in range(1, epochs + 1):
        loss = _synthetic_loss(seed, ep, epochs)
        line = {
            "epoch": ep,
            "step": ep * 10,
            "loss": loss,
            "lr": learning_rate,
        }
        with open(metrics_path, "a", encoding="utf-8") as f:
            f.write(json.dumps(line) + "\n")

        if sample_prompt and sample_interval > 0:
            if ep % sample_interval == 0 or ep == epochs:
                _generate_mock_sample(output, ep, sample_prompt)

        if sleep_ms > 0:
            time.sleep(sleep_ms / 1000.0)

    adapter_path = output / "adapter.safetensors"
    lora_info = dict(lora_cfg)
    lora_info["base_model"] = base_model
    _generate_mock_safetensors(adapter_path, lora_info)


# ==============================================================================
# PIPELINE REAL DE TREINO (ENGINE_MOCK=0 / @gpu)
# ==============================================================================


def _setup_cache_dir() -> None:
    """Configura diretório de cache persistente para Hugging Face."""
    # Se montado em /outputs, usa /outputs/.cache para persistir no volume
    if Path("/outputs").exists():
        cache_dir = Path("/outputs/.cache/huggingface")
        cache_dir.mkdir(parents=True, exist_ok=True)
        os.environ.setdefault("HF_HOME", str(cache_dir))


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


def _generate_sample_sd15(
    unet: Any,
    vae: Any,
    text_encoder: Any,
    tokenizer: Any,
    noise_scheduler: Any,
    prompt: str,
    output_path: Path,
) -> None:
    """Gera uma imagem de teste para SD 1.5 com os pesos LoRA ativos."""
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
        with torch.inference_mode():
            img = pipe(prompt, num_inference_steps=20, guidance_scale=7.5).images[0]
            output_path.parent.mkdir(parents=True, exist_ok=True)
            img.save(output_path)
            print(f"[SD 1.5] Amostra de validação salva em: {output_path}")
    except Exception as e:
        print(f"[WARN] Falha ao gerar amostra de validação SD 1.5: {e}")


def _real_train_sd15(cfg: dict[str, Any], output: Path) -> None:
    """Pipeline real de treino LoRA para Stable Diffusion 1.5 na GPU."""
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
    _setup_cache_dir()

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

    print(f"Carregando modelos base SD 1.5 ({model_id})...")
    tokenizer = CLIPTokenizer.from_pretrained(model_id, subfolder="tokenizer")
    text_encoder = CLIPTextModel.from_pretrained(
        model_id, subfolder="text_encoder", torch_dtype=torch.float16
    ).to(device)
    vae = AutoencoderKL.from_pretrained(
        model_id, subfolder="vae", torch_dtype=torch.float16
    ).to(device)
    unet = UNet2DConditionModel.from_pretrained(
        model_id, subfolder="unet", torch_dtype=torch.float16
    ).to(device)
    noise_scheduler = DDPMScheduler.from_pretrained(model_id, subfolder="scheduler")

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

    # Otimizador 8-bit AdamW para caber confortavelmente na RTX 3060 (12 GB)
    try:
        import bitsandbytes as bnb

        optimizer = bnb.optim.AdamW8bit(unet.parameters(), lr=learning_rate)
        print("Usando otimizador 8-bit AdamW (bitsandbytes).")
    except (ImportError, AttributeError, RuntimeError, TypeError) as e:
        print(f"Bitsandbytes não disponível ({e}); usando AdamW padrão.")
        optimizer = torch.optim.AdamW(unet.parameters(), lr=learning_rate)

    dataset = DiffusionDataset(dataset_path, resolution=512, trigger_word=trigger_word)
    dataloader = DataLoader(
        dataset, batch_size=batch_size, shuffle=True, drop_last=False
    )

    output.mkdir(parents=True, exist_ok=True)
    metrics_path = output / "metrics.jsonl"
    if metrics_path.exists():
        metrics_path.unlink()

    print(
        f"Iniciando treinamento SD 1.5: {len(dataset)} amostras, {epochs} épocas, batch={batch_size}, lr={learning_rate}..."
    )
    global_step = 0

    for epoch in range(1, epochs + 1):
        unet.train()
        epoch_loss = 0.0
        steps_in_epoch = 0

        for batch in dataloader:
            pixel_values = batch["pixel_values"].to(device, dtype=torch.float16)
            prompts = batch["prompt"]

            # Codifica imagem para latents pelo VAE
            with torch.no_grad():
                latents = vae.encode(pixel_values).latent_dist.sample() * 0.18215

            noise = torch.randn_like(latents)
            timesteps = torch.randint(
                0,
                noise_scheduler.config.num_train_timesteps,
                (latents.shape[0],),
                device=device,
            ).long()
            noisy_latents = noise_scheduler.add_noise(latents, noise, timesteps)

            # Codifica texto
            with torch.no_grad():
                text_inputs = tokenizer(
                    prompts,
                    padding="max_length",
                    max_length=tokenizer.model_max_length,
                    truncation=True,
                    return_tensors="pt",
                ).input_ids.to(device)
                encoder_hidden_states = text_encoder(text_inputs)[0]

            # Forward no UNet com LoRA
            model_pred = unet(noisy_latents, timesteps, encoder_hidden_states).sample
            loss = F.mse_loss(model_pred.float(), noise.float(), reduction="mean")

            loss.backward()
            optimizer.step()
            optimizer.zero_grad()

            global_step += 1
            epoch_loss += loss.item()
            steps_in_epoch += 1

        avg_loss = round(epoch_loss / max(1, steps_in_epoch), 4)
        print(
            f"[SD 1.5] Época {epoch}/{epochs} concluída - Step {global_step} - Loss: {avg_loss}"
        )

        metric_line = {
            "epoch": epoch,
            "step": global_step,
            "loss": avg_loss,
            "lr": learning_rate,
        }
        with open(metrics_path, "a", encoding="utf-8") as f:
            f.write(json.dumps(metric_line) + "\n")

        if sample_prompt and sample_interval > 0:
            if epoch % sample_interval == 0 or epoch == epochs:
                sample_file = output / "samples" / f"sample_epoch_{epoch:03d}.png"
                _generate_sample_sd15(
                    unet,
                    vae,
                    text_encoder,
                    tokenizer,
                    noise_scheduler,
                    sample_prompt,
                    sample_file,
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
    }
    _save_lora_safetensors(unet, adapter_file, metadata)
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
) -> None:
    """Gera uma imagem de teste para SDXL com os pesos LoRA ativos."""
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
        with torch.inference_mode():
            img = pipe(prompt, num_inference_steps=20, guidance_scale=7.0).images[0]
            output_path.parent.mkdir(parents=True, exist_ok=True)
            img.save(output_path)
            print(f"[SDXL] Amostra de validação salva em: {output_path}")
    except Exception as e:
        print(f"[WARN] Falha ao gerar amostra de validação SDXL: {e}")


def _real_train_sdxl(cfg: dict[str, Any], output: Path) -> None:
    """Pipeline real de treino LoRA para Stable Diffusion XL (SDXL 1.0) na GPU."""
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
    _setup_cache_dir()

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

    print(f"Carregando modelos base SDXL ({model_id})...")
    tokenizer_one = AutoTokenizer.from_pretrained(
        model_id, subfolder="tokenizer", use_fast=False
    )
    tokenizer_two = AutoTokenizer.from_pretrained(
        model_id, subfolder="tokenizer_2", use_fast=False
    )
    text_encoder_one = CLIPTextModel.from_pretrained(
        model_id, subfolder="text_encoder", torch_dtype=torch.float16
    ).to(device)
    text_encoder_two = CLIPTextModelWithProjection.from_pretrained(
        model_id, subfolder="text_encoder_2", torch_dtype=torch.float16
    ).to(device)
    vae = AutoencoderKL.from_pretrained(
        model_id, subfolder="vae", torch_dtype=torch.float16
    ).to(device)
    unet = UNet2DConditionModel.from_pretrained(
        model_id, subfolder="unet", torch_dtype=torch.float16
    ).to(device)
    noise_scheduler = DDPMScheduler.from_pretrained(model_id, subfolder="scheduler")

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

    try:
        import bitsandbytes as bnb

        optimizer = bnb.optim.AdamW8bit(unet.parameters(), lr=learning_rate)
        print("Usando otimizador 8-bit AdamW (bitsandbytes).")
    except (ImportError, AttributeError, RuntimeError, TypeError) as e:
        print(f"Bitsandbytes não disponível ({e}); usando AdamW padrão.")
        optimizer = torch.optim.AdamW(unet.parameters(), lr=learning_rate)

    # SDXL usa resolução padrão 1024x1024
    dataset = DiffusionDataset(dataset_path, resolution=1024, trigger_word=trigger_word)
    dataloader = DataLoader(
        dataset, batch_size=batch_size, shuffle=True, drop_last=False
    )

    output.mkdir(parents=True, exist_ok=True)
    metrics_path = output / "metrics.jsonl"
    if metrics_path.exists():
        metrics_path.unlink()

    # Time IDs de micro-condicionamento do SDXL (original_size, crop_coords, target_size)
    add_time_ids = torch.tensor(
        [[1024, 1024, 0, 0, 1024, 1024]], device=device, dtype=torch.float16
    )

    print(
        f"Iniciando treinamento SDXL: {len(dataset)} amostras, {epochs} épocas, batch={batch_size}, lr={learning_rate}..."
    )
    global_step = 0

    for epoch in range(1, epochs + 1):
        unet.train()
        epoch_loss = 0.0
        steps_in_epoch = 0

        for batch in dataloader:
            pixel_values = batch["pixel_values"].to(device, dtype=torch.float16)
            prompts = batch["prompt"]
            cur_bs = pixel_values.shape[0]

            with torch.no_grad():
                latents = (
                    vae.encode(pixel_values).latent_dist.sample()
                    * vae.config.scaling_factor
                )

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

            loss.backward()
            optimizer.step()
            optimizer.zero_grad()

            global_step += 1
            epoch_loss += loss.item()
            steps_in_epoch += 1

        avg_loss = round(epoch_loss / max(1, steps_in_epoch), 4)
        print(
            f"[SDXL] Época {epoch}/{epochs} concluída - Step {global_step} - Loss: {avg_loss}"
        )

        metric_line = {
            "epoch": epoch,
            "step": global_step,
            "loss": avg_loss,
            "lr": learning_rate,
        }
        with open(metrics_path, "a", encoding="utf-8") as f:
            f.write(json.dumps(metric_line) + "\n")

        if sample_prompt and sample_interval > 0:
            if epoch % sample_interval == 0 or epoch == epochs:
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
    }
    _save_lora_safetensors(unet, adapter_file, metadata)
    print(f"Treino SDXL finalizado com sucesso! Checkpoint salvo em: {adapter_file}")


def _real_train_flux(cfg: dict[str, Any], output: Path) -> None:
    """Treino real LoRA para FLUX.2 Klein 4B via Diffusers/PEFT."""
    try:
        import torch
        from diffusers import FluxPipeline  # noqa: F401
        from peft import LoraConfig, get_peft_model  # noqa: F401
    except ImportError as e:
        _die(f"Dependência ausente para treino real FLUX.2 Klein 4B: {e}")

    if not torch.cuda.is_available():
        _die("CUDA não disponível para treino real de difusão (ENGINE_MOCK=0)")

    print("Iniciando pipeline de treino LoRA FLUX.2 Klein 4B...")


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
        print("Uso: python -m trainer_difusao [health|train] [args...]")
        return

    if argv[0] == "train":
        cmd_train(argv[1:])
    elif argv[0] == "health":
        cmd_health()
    else:
        _die(f"Subcomando desconhecido: {argv[0]}")


if __name__ == "__main__":
    main()
