"""Adapters for SD 1.5 and SDXL training using unified TrainingLoopRunner."""

from __future__ import annotations

from pathlib import Path
from typing import Any, Callable

from trainer_difusao.models.loop import LoraTrainConfig, ModelComponents, parse_lora_train_config
from trainer_difusao.models.sd_pkg import (
    _compute_sdxl_embeddings,
    _generate_sample_sd15,
    _generate_sample_sdxl,
)
from trainer_difusao.common import _die

__all__ = ["SD15Adapter", "SDXLAdapter"]


class SD15Adapter:
    """Adapter for Stable Diffusion 1.5 training via TrainingLoopRunner."""

    arch_label = "SD 1.5"
    metadata_base_model = "sd15"

    def parse_lora_train_config(self, cfg: dict[str, Any]) -> LoraTrainConfig:
        """Parse training config with SD15 defaults."""
        return parse_lora_train_config(
            cfg,
            default_model_id="runwayml/stable-diffusion-v1-5",
            default_resolution=512,
            arch_name="sd15",
            quant_default="none",
            allow_custom_checkpoint=True,
            allow_text_encoder_path=False,
        )

    def load_and_inject_lora(self, tcfg: LoraTrainConfig, hub_cache: str, metrics_path: Path) -> ModelComponents:
        """Load SD15 base model, inject LoRA, and return components."""
        import torch
        from diffusers import AutoencoderKL, DDPMScheduler, UNet2DConditionModel
        from peft import LoraConfig, get_peft_model
        from transformers import BitsAndBytesConfig, CLIPTextModel, CLIPTokenizer

        from trainer_difusao.common import _emit_metric, _load_lora_weights

        device = torch.device("cuda")

        # Determine dtype
        target_dtype = (
            torch.bfloat16
            if (tcfg.mixed_precision == "bf16" and torch.cuda.is_bf16_supported())
            else torch.float16
        )

        is_4bit = tcfg.quantization == "4bit"
        is_8bit = tcfg.quantization == "8bit"
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

        # Load tokenizer and text encoder
        tokenizer = CLIPTokenizer.from_pretrained(
            tcfg.model_id, subfolder="tokenizer", cache_dir=hub_cache
        )
        text_encoder = CLIPTextModel.from_pretrained(
            tcfg.model_id,
            subfolder="text_encoder",
            torch_dtype=target_dtype,
            cache_dir=hub_cache,
        ).to(device)

        # Load VAE in float32 to prevent NaN
        vae = AutoencoderKL.from_pretrained(
            tcfg.model_id, subfolder="vae", torch_dtype=torch.float32, cache_dir=hub_cache
        ).to(device)

        # Load UNet with optional custom checkpoint
        if tcfg.custom_checkpoint_path:
            try:
                if is_quantized:
                    unet = UNet2DConditionModel.from_single_file(
                        tcfg.custom_checkpoint_path,
                        quantization_config=bnb_config,
                        torch_dtype=target_dtype,
                    )
                else:
                    unet = UNet2DConditionModel.from_single_file(
                        tcfg.custom_checkpoint_path, torch_dtype=target_dtype
                    ).to(device)
            except Exception as exc:
                _die(
                    f"Falha ao carregar checkpoint sd15 custom "
                    f"({tcfg.custom_checkpoint_path}): layout não reconhecido ({exc})"
                )
            print(
                f"[SD15] Checkpoint custom aplicado ao UNet: {tcfg.custom_checkpoint_path}",
                flush=True,
            )
        else:
            if is_quantized:
                unet = UNet2DConditionModel.from_pretrained(
                    tcfg.model_id,
                    subfolder="unet",
                    quantization_config=bnb_config,
                    torch_dtype=target_dtype,
                    cache_dir=hub_cache,
                )
            else:
                unet = UNet2DConditionModel.from_pretrained(
                    tcfg.model_id, subfolder="unet", torch_dtype=target_dtype, cache_dir=hub_cache
                ).to(device)

        noise_scheduler = DDPMScheduler.from_pretrained(
            tcfg.model_id, subfolder="scheduler", cache_dir=hub_cache
        )

        # Freeze models
        vae.requires_grad_(False)
        text_encoder.requires_grad_(False)
        unet.requires_grad_(False)

        # Enable gradient checkpointing
        unet.enable_gradient_checkpointing()

        # Inject LoRA
        lora_config = LoraConfig(
            r=tcfg.rank,
            lora_alpha=tcfg.alpha,
            init_lora_weights="gaussian",
            target_modules=["to_k", "to_q", "to_v", "to_out.0"],
        )
        unet = get_peft_model(unet, lora_config)
        if tcfg.weights_path:
            _load_lora_weights(unet, tcfg.weights_path)

        _emit_metric(
            metrics_path,
            epoch=0,
            step=3,
            progress=0.06,
            phase="setup_lora",
            message=f"Adaptadores LoRA injetados no UNet (rank={tcfg.rank}, alpha={tcfg.alpha}).",
        )

        return ModelComponents(
            trainable_module=unet,
            device=device,
            dtype=target_dtype,
            extra={
                "tokenizer": tokenizer,
                "text_encoder": text_encoder,
                "vae": vae,
                "noise_scheduler": noise_scheduler,
            },
        )

    def build_text_cache_encode_fn(self, comp: ModelComponents) -> Callable[[list[str]], dict[str, Any]]:
        """Build function that encodes captions to text embeddings for SD15."""
        import torch

        tokenizer = comp["extra"]["tokenizer"]
        text_encoder = comp["extra"]["text_encoder"]
        device = comp["device"]
        dtype = comp["dtype"]

        def encode_fn(caps: list[str]) -> dict[str, Any]:
            inputs = tokenizer(
                caps,
                padding="max_length",
                max_length=tokenizer.model_max_length,
                truncation=True,
                return_tensors="pt",
            ).input_ids.to(device)
            with torch.no_grad():
                hidden = text_encoder(inputs)[0].to(dtype=dtype)
            return {"hidden": hidden}

        return encode_fn

    def text_cache_encoders(self, comp: ModelComponents) -> list[Any]:
        """Return list of text encoders for offload/cleanup."""
        return [comp["extra"]["text_encoder"]]

    def precompute_sample_embeds(self, comp: ModelComponents, tcfg: LoraTrainConfig) -> Any | None:
        """Precompute embeddings for sample prompt."""
        if not tcfg.sample_prompt:
            return None

        try:
            from trainer_difusao.common import _precompute_sample_embeds_sd15

            tokenizer = comp["extra"]["tokenizer"]
            text_encoder = comp["extra"]["text_encoder"]
            device = comp["device"]
            dtype = comp["dtype"]

            return _precompute_sample_embeds_sd15(
                tokenizer=tokenizer,
                text_encoder=text_encoder,
                prompt=tcfg.sample_prompt,
                device=device,
                dtype=dtype,
            )
        except Exception as e:
            print(f"[WARN] Falha ao pré-computar sample embeds SD 1.5: {e}", flush=True)
            return None

    def forward_and_loss(self, comp: ModelComponents, batch: dict, tcfg: LoraTrainConfig, cached_encode: dict[str, Any]) -> Any:
        """Forward pass and loss computation for SD15."""
        import torch
        import torch.nn.functional as F

        unet = comp["trainable_module"]
        vae = comp["extra"]["vae"]
        noise_scheduler = comp["extra"]["noise_scheduler"]
        device = comp["device"]
        dtype = comp["dtype"]

        pixel_values = batch["pixel_values"].to(device, dtype=torch.float32)
        cur_bs = pixel_values.shape[0]

        with torch.no_grad():
            latents = (
                vae.encode(pixel_values).latent_dist.sample()
                * vae.config.scaling_factor
            ).to(dtype=dtype)

        noise = torch.randn_like(latents)
        timesteps = torch.randint(
            0, noise_scheduler.config.num_train_timesteps, (cur_bs,), device=device
        ).long()
        noisy_latents = noise_scheduler.add_noise(latents, noise, timesteps)

        encoder_hidden_states = cached_encode["hidden"].to(device, dtype=dtype)

        model_pred = unet(
            noisy_latents,
            timesteps,
            encoder_hidden_states,
            return_dict=False,
        )[0]

        loss = F.mse_loss(model_pred.float(), noise.float(), reduction="mean")
        return loss

    def generate_sample(
        self, comp: ModelComponents, sample_file: Path, tcfg: LoraTrainConfig, *, epoch: int, metrics_path: Path, sample_embeds: Any | None
    ) -> None:
        """Generate sample image for SD15."""
        unet = comp["trainable_module"]
        vae = comp["extra"]["vae"]
        text_encoder = comp["extra"]["text_encoder"]
        tokenizer = comp["extra"]["tokenizer"]
        noise_scheduler = comp["extra"]["noise_scheduler"]

        _generate_sample_sd15(
            unet,
            vae,
            text_encoder,
            tokenizer,
            noise_scheduler,
            tcfg.sample_prompt,
            sample_file,
            seed=tcfg.sample_seed,
            metrics_path=metrics_path,
            epoch=epoch,
            sample_embeds=sample_embeds,
        )

    def checkpoint_metadata(self, tcfg: LoraTrainConfig, *, epoch: int | None = None) -> dict[str, str]:
        """Return metadata dict for checkpoint."""
        metadata = {
            "format": "pt",
            "framework": "diffusers",
            "model_type": "lora",
            "base_model": self.metadata_base_model,
            "lora_rank": str(tcfg.rank),
            "lora_alpha": str(tcfg.alpha),
            "trigger_word": tcfg.trigger_word,
            "quantization": tcfg.quantization,
        }
        if epoch is not None:
            metadata["epoch"] = str(epoch)
        return metadata


class SDXLAdapter:
    """Adapter for Stable Diffusion XL (SDXL 1.0) training via TrainingLoopRunner."""

    arch_label = "SDXL"
    metadata_base_model = "sdxl"

    def parse_lora_train_config(self, cfg: dict[str, Any]) -> LoraTrainConfig:
        """Parse training config with SDXL defaults."""
        return parse_lora_train_config(
            cfg,
            default_model_id="stabilityai/stable-diffusion-xl-base-1.0",
            default_resolution=1024,
            arch_name="sdxl",
            quant_default="none",
            allow_custom_checkpoint=True,
            allow_text_encoder_path=False,
        )

    def load_and_inject_lora(self, tcfg: LoraTrainConfig, hub_cache: str, metrics_path: Path) -> ModelComponents:
        """Load SDXL base model, inject LoRA, and return components."""
        import torch
        from diffusers import AutoencoderKL, DDPMScheduler, UNet2DConditionModel
        from peft import LoraConfig, get_peft_model
        from transformers import (
            AutoTokenizer,
            BitsAndBytesConfig,
            CLIPTextModel,
            CLIPTextModelWithProjection,
        )

        from trainer_difusao.common import _emit_metric, _load_lora_weights

        device = torch.device("cuda")

        # Determine dtype
        target_dtype = (
            torch.bfloat16
            if (tcfg.mixed_precision == "bf16" and torch.cuda.is_bf16_supported())
            else torch.float16
        )

        is_4bit = tcfg.quantization == "4bit"
        is_8bit = tcfg.quantization == "8bit"
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

        # Load tokenizers and text encoders
        tokenizer_one = AutoTokenizer.from_pretrained(
            tcfg.model_id, subfolder="tokenizer", use_fast=False, cache_dir=hub_cache
        )
        tokenizer_two = AutoTokenizer.from_pretrained(
            tcfg.model_id, subfolder="tokenizer_2", use_fast=False, cache_dir=hub_cache
        )
        text_encoder_one = CLIPTextModel.from_pretrained(
            tcfg.model_id,
            subfolder="text_encoder",
            torch_dtype=target_dtype,
            cache_dir=hub_cache,
        ).to(device)
        text_encoder_two = CLIPTextModelWithProjection.from_pretrained(
            tcfg.model_id,
            subfolder="text_encoder_2",
            torch_dtype=target_dtype,
            cache_dir=hub_cache,
        ).to(device)

        # Load VAE in float32 to prevent NaN
        vae = AutoencoderKL.from_pretrained(
            tcfg.model_id, subfolder="vae", torch_dtype=torch.float32, cache_dir=hub_cache
        ).to(device)

        # Load UNet with optional custom checkpoint
        if tcfg.custom_checkpoint_path:
            try:
                if is_quantized:
                    unet = UNet2DConditionModel.from_single_file(
                        tcfg.custom_checkpoint_path,
                        quantization_config=bnb_config,
                        torch_dtype=target_dtype,
                    )
                else:
                    unet = UNet2DConditionModel.from_single_file(
                        tcfg.custom_checkpoint_path, torch_dtype=target_dtype
                    ).to(device)
            except Exception as exc:
                _die(
                    f"Falha ao carregar checkpoint sdxl custom "
                    f"({tcfg.custom_checkpoint_path}): layout não reconhecido ({exc})"
                )
            print(
                f"[SDXL] Checkpoint custom aplicado ao UNet: {tcfg.custom_checkpoint_path}",
                flush=True,
            )
        else:
            if is_quantized:
                unet = UNet2DConditionModel.from_pretrained(
                    tcfg.model_id,
                    subfolder="unet",
                    quantization_config=bnb_config,
                    torch_dtype=target_dtype,
                    cache_dir=hub_cache,
                )
            else:
                unet = UNet2DConditionModel.from_pretrained(
                    tcfg.model_id, subfolder="unet", torch_dtype=target_dtype, cache_dir=hub_cache
                ).to(device)

        noise_scheduler = DDPMScheduler.from_pretrained(
            tcfg.model_id, subfolder="scheduler", cache_dir=hub_cache
        )

        # Freeze models
        vae.requires_grad_(False)
        text_encoder_one.requires_grad_(False)
        text_encoder_two.requires_grad_(False)
        unet.requires_grad_(False)

        # Enable gradient checkpointing
        unet.enable_gradient_checkpointing()

        # Inject LoRA
        lora_config = LoraConfig(
            r=tcfg.rank,
            lora_alpha=tcfg.alpha,
            init_lora_weights="gaussian",
            target_modules=["to_k", "to_q", "to_v", "to_out.0"],
        )
        unet = get_peft_model(unet, lora_config)
        if tcfg.weights_path:
            _load_lora_weights(unet, tcfg.weights_path)

        _emit_metric(
            metrics_path,
            epoch=0,
            step=3,
            progress=0.06,
            phase="setup_lora",
            message=f"Adaptadores LoRA injetados no UNet SDXL (rank={tcfg.rank}, alpha={tcfg.alpha}).",
        )

        return ModelComponents(
            trainable_module=unet,
            device=device,
            dtype=target_dtype,
            extra={
                "tokenizer_one": tokenizer_one,
                "tokenizer_two": tokenizer_two,
                "text_encoder_one": text_encoder_one,
                "text_encoder_two": text_encoder_two,
                "vae": vae,
                "noise_scheduler": noise_scheduler,
            },
        )

    def build_text_cache_encode_fn(self, comp: ModelComponents) -> Callable[[list[str]], dict[str, Any]]:
        """Build function that encodes captions to text embeddings for SDXL."""
        device = comp["device"]

        tokenizer_one = comp["extra"]["tokenizer_one"]
        tokenizer_two = comp["extra"]["tokenizer_two"]
        text_encoder_one = comp["extra"]["text_encoder_one"]
        text_encoder_two = comp["extra"]["text_encoder_two"]

        def encode_fn(caps: list[str]) -> dict[str, Any]:
            hidden, pooled = _compute_sdxl_embeddings(
                caps,
                tokenizer_one,
                tokenizer_two,
                text_encoder_one,
                text_encoder_two,
                device,
            )
            return {"hidden": hidden, "pooled": pooled}

        return encode_fn

    def text_cache_encoders(self, comp: ModelComponents) -> list[Any]:
        """Return list of text encoders for offload/cleanup."""
        return [comp["extra"]["text_encoder_one"], comp["extra"]["text_encoder_two"]]

    def precompute_sample_embeds(self, comp: ModelComponents, tcfg: LoraTrainConfig) -> Any | None:
        """Precompute embeddings for sample prompt."""
        if not tcfg.sample_prompt:
            return None

        try:
            from trainer_difusao.common import _precompute_sample_embeds_sdxl

            tokenizer_one = comp["extra"]["tokenizer_one"]
            tokenizer_two = comp["extra"]["tokenizer_two"]
            text_encoder_one = comp["extra"]["text_encoder_one"]
            text_encoder_two = comp["extra"]["text_encoder_two"]
            device = comp["device"]
            dtype = comp["dtype"]

            return _precompute_sample_embeds_sdxl(
                tokenizer_one=tokenizer_one,
                tokenizer_two=tokenizer_two,
                text_encoder_one=text_encoder_one,
                text_encoder_two=text_encoder_two,
                prompt=tcfg.sample_prompt,
                device=device,
                dtype=dtype,
            )
        except Exception as e:
            print(f"[WARN] Falha ao pré-computar sample embeds SDXL: {e}", flush=True)
            return None

    def forward_and_loss(self, comp: ModelComponents, batch: dict, tcfg: LoraTrainConfig, cached_encode: dict[str, Any]) -> Any:
        """Forward pass and loss computation for SDXL."""
        import torch
        import torch.nn.functional as F

        unet = comp["trainable_module"]
        vae = comp["extra"]["vae"]
        noise_scheduler = comp["extra"]["noise_scheduler"]
        device = comp["device"]
        dtype = comp["dtype"]

        pixel_values = batch["pixel_values"].to(device, dtype=torch.float32)
        cur_bs = pixel_values.shape[0]

        with torch.no_grad():
            latents = (
                vae.encode(pixel_values).latent_dist.sample()
                * vae.config.scaling_factor
            ).to(dtype=dtype)

        noise = torch.randn_like(latents)
        timesteps = torch.randint(
            0, noise_scheduler.config.num_train_timesteps, (cur_bs,), device=device
        ).long()
        noisy_latents = noise_scheduler.add_noise(latents, noise, timesteps)

        prompt_embeds = cached_encode["hidden"].to(device, dtype=dtype)
        pooled_prompt_embeds = cached_encode["pooled"].to(device, dtype=dtype)

        # Micro-conditioning: original size, target size, and crop offsets
        if tcfg.enable_bucket:
            bh, bw = pixel_values.shape[2], pixel_values.shape[3]
            batch_time_ids = torch.tensor(
                [[bh, bw, 0, 0, bh, bw]],
                dtype=dtype,
                device=device,
            ).repeat(cur_bs, 1)
        else:
            batch_time_ids = torch.tensor(
                [[tcfg.resolution, tcfg.resolution, 0, 0, tcfg.resolution, tcfg.resolution]],
                dtype=dtype,
                device=device,
            ).repeat(cur_bs, 1)

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

        loss = F.mse_loss(model_pred.float(), noise.float(), reduction="mean")
        return loss

    def generate_sample(
        self, comp: ModelComponents, sample_file: Path, tcfg: LoraTrainConfig, *, epoch: int, metrics_path: Path, sample_embeds: Any | None
    ) -> None:
        """Generate sample image for SDXL."""
        unet = comp["trainable_module"]
        vae = comp["extra"]["vae"]
        text_encoder_one = comp["extra"]["text_encoder_one"]
        text_encoder_two = comp["extra"]["text_encoder_two"]
        tokenizer_one = comp["extra"]["tokenizer_one"]
        tokenizer_two = comp["extra"]["tokenizer_two"]
        noise_scheduler = comp["extra"]["noise_scheduler"]

        _generate_sample_sdxl(
            unet,
            vae,
            text_encoder_one,
            text_encoder_two,
            tokenizer_one,
            tokenizer_two,
            noise_scheduler,
            tcfg.sample_prompt,
            sample_file,
            seed=tcfg.sample_seed,
            metrics_path=metrics_path,
            epoch=epoch,
            sample_embeds=sample_embeds,
        )

    def checkpoint_metadata(self, tcfg: LoraTrainConfig, *, epoch: int | None = None) -> dict[str, str]:
        """Return metadata dict for checkpoint."""
        metadata = {
            "format": "pt",
            "framework": "diffusers",
            "model_type": "lora",
            "base_model": self.metadata_base_model,
            "lora_rank": str(tcfg.rank),
            "lora_alpha": str(tcfg.alpha),
            "trigger_word": tcfg.trigger_word,
            "quantization": tcfg.quantization,
        }
        if epoch is not None:
            metadata["epoch"] = str(epoch)
        return metadata
