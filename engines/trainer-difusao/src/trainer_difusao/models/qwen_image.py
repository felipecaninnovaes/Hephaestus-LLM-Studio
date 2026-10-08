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
    _prune_checkpoints,
    save_adapter_checkpoint,
    save_final_adapter,
)
from trainer_difusao.common_pkg.latent_cache import (
    LatentCache,
    prepare_latent_cache,
    resolve_cache_latents,
    sample_latent_dist,
    vae_encode_dist,
    vae_fingerprint,
)
from trainer_difusao.dataset import DiffusionDataset, build_dataloader
from trainer_difusao.models.base import BaseModelTrainer
from trainer_difusao.optimizers import _create_lr_scheduler, _create_optimizer
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


def _log_vram(tag: str) -> None:
    """Loga VRAM livre/total (driver) e alocada/reservada (torch) em GiB; no-op sem CUDA."""
    if not torch.cuda.is_available():
        return
    free, total = torch.cuda.mem_get_info()
    gib = 1024**3
    print(
        f"[VRAM] {tag}: usada={(total - free) / gib:.2f} GiB livre={free / gib:.2f} GiB "
        f"total={total / gib:.2f} GiB torch_alloc={torch.cuda.memory_allocated() / gib:.2f} GiB "
        f"torch_reserved={torch.cuda.memory_reserved() / gib:.2f} GiB "
        f"torch_peak_alloc={torch.cuda.max_memory_allocated() / gib:.2f} GiB",
        flush=True,
    )


def _release_prompt_encoder(prompt_encoder: Any) -> None:
    """Solta a referência do prompt_encoder ao text encoder (PITFALLS: `del` em parâmetro
    local é no-op; a memória só é liberada quando TODAS as referências somem). O chamador
    deve zerar também a sua variável local e chamar `cleanup_cuda()`."""
    prompt_encoder.text_encoder = None


def _qwen_sample_native(
    transformer: Any,
    vae: Any,
    prompt_payload: dict[str, Any],
    height: int,
    width: int,
    num_inference_steps: int,
    seed: int,
    device: Any,
    dtype: Any,
    output_path: Path | None = None,
) -> Any:
    """Procedural Qwen-Image-2.1 sampling using flow matching without OOP pipeline interface.

    ``prompt_payload`` é o embed pré-computado (``embeds``/``mask``/``slot_mask``, o mesmo
    payload do ``TextEmbedsCache``): o text encoder não participa da amostra.
    Returns a PIL Image or None if sampling failed.
    """
    import numpy as np
    from diffusers import FlowMatchEulerDiscreteScheduler
    from PIL import Image

    try:
        import torch

        with torch.no_grad():
            prompt_embeds, prompt_mask, slot_mask = pad_prompt_batch(
                [prompt_payload["embeds"]],
                [prompt_payload["mask"]],
                [prompt_payload["slot_mask"]],
                device,
                dtype,
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
            if height * width >= TILE_DECODE_ABOVE_PIXELS:
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


def _qwen_vae_input(images: torch.Tensor) -> torch.Tensor:
    """Entrada 5D (B, C, T=1, H, W) do VAE causal-3D; RGB ganha canal alpha opaco
    (VAE nativo RGBA, in_channels=4 — PITFALLS: VAE causal-3D / alpha_channel)."""
    if images.shape[1] == 3:
        alpha_channel = torch.ones_like(images[:, :1])
        images = torch.cat([images, alpha_channel], dim=1)
    return images.unsqueeze(2)


def _qwen_encode_dist(vae: Any, images: torch.Tensor, dtype: torch.dtype) -> tuple[torch.Tensor, torch.Tensor]:
    """(mean, std) de ``latent_dist`` de imagens (B, C, H, W) em [-1, 1]; shape (B, 64, 1, H', W')."""
    return vae_encode_dist(vae, _qwen_vae_input(images.to(dtype)))


def _qwen_normalized_latents(
    vae: Any,
    batch: dict[str, Any],
    device: Any,
    dtype: torch.dtype,
    generator: torch.Generator | None = None,
) -> torch.Tensor:
    """Latents normalizados (B, 64, H', W') do batch.

    Com cache (``latent_mean``/``latent_std``) amostra ``mean + std*randn`` (VAE fora
    da GPU); sem cache codifica ``pixel_values`` (VAE deve estar em ``device``).
    A normalização usa só ``vae.config`` (independe do device do VAE).
    """
    if "latent_mean" in batch:
        latents = sample_latent_dist(
            batch["latent_mean"].to(device), batch["latent_std"].to(device), generator
        ).to(dtype)
    else:
        images = batch["pixel_values"].to(device, dtype=dtype)
        latents = vae.encode(_qwen_vae_input(images)).latent_dist.sample()  # (B, 64, 1, H', W')
    # Normalize latents (ai-toolkit reference pattern)
    mean = torch.tensor(vae.config.latents_mean).view(1, -1, 1, 1, 1).to(device, dtype=dtype)
    std = torch.tensor(vae.config.latents_std).view(1, -1, 1, 1, 1).to(device, dtype=dtype)
    latents = (latents - mean) / std
    # Remove frame dimension for downstream processing
    return latents.squeeze(2)  # (B, 64, 1, H', W') -> (B, 64, H', W')


_QWEN_SUPPORTED_QUANT = ("none", "4bit", "8bit")
# Formato do cache quantizado do transformer por nível. O cache FP4 legado (formato "4bit")
# jamais casa com "4bit-nf4dq": diretório e `quant_format` do metadata são distintos.
_QWEN_QUANT_CACHE_FORMAT = {"4bit": "4bit-nf4dq", "8bit": "8bit"}


def _resolve_transformer_quantization(quantization: str | None) -> str:
    """Valida o nível de quantização do transformer Qwen (só none/4bit/8bit)."""
    if quantization not in _QWEN_SUPPORTED_QUANT:
        _die(
            f"Quantização '{quantization}' não suportada: "
            "Qwen-Image-2.1 suporta só none/4bit/8bit."
        )
    return str(quantization)


def _build_transformer_bnb_config(quantization: str, compute_dtype: Any) -> Any | None:
    """BitsAndBytesConfig do transformer: 4bit => NF4 + double quant; 8bit => int8; none => None."""
    quantization = _resolve_transformer_quantization(quantization)
    if quantization == "none":
        return None
    from transformers import BitsAndBytesConfig

    if quantization == "4bit":
        return BitsAndBytesConfig(
            load_in_4bit=True,
            bnb_4bit_quant_type="nf4",
            bnb_4bit_compute_dtype=compute_dtype,
            bnb_4bit_use_double_quant=True,
        )
    return BitsAndBytesConfig(load_in_8bit=True)


def _build_text_encoder_bnb_config(compute_dtype: Any) -> Any:
    """Text encoder Qwen3-VL-8B: sempre NF4 4-bit (congelado; bf16 ~16 GB não cabe em 12 GB)."""
    from transformers import BitsAndBytesConfig

    return BitsAndBytesConfig(
        load_in_4bit=True,
        bnb_4bit_quant_type="nf4",
        bnb_4bit_compute_dtype=compute_dtype,
        bnb_4bit_use_double_quant=True,
    )


def _maybe_save_epoch_checkpoint(
    model: Any,
    optimizer: Any,
    checkpoints_dir: Any,
    base_name: str,
    *,
    epoch: int,
    local_epoch_idx: int,
    epochs: int,
    checkpoint_interval: int,
    global_step: int,
    quantization: str | None = None,
) -> bool:
    """Salva checkpoint ao fim da época se `local_epoch_idx % checkpoint_interval == 0`
    ou for a última época desta execução (mesma regra de `loop.py`), e poda para 2.

    `local_epoch_idx` (1-based, relativo à execução) decide o intervalo; `epoch`
    (absoluta) nomeia o arquivo e vai nos metadados. Retorna True se salvou."""
    if local_epoch_idx % checkpoint_interval != 0 and local_epoch_idx != epochs:
        return False
    try:
        metadata = {
            "format": "pt",
            "model_type": "lora",
            "base_model": "qwen-image-2.1",
            "epoch": str(epoch),
            "step": str(global_step),
        }
        if quantization is not None:
            metadata["quantization"] = quantization
        ckpt_path = save_adapter_checkpoint(
            model,
            checkpoints_dir,
            base_name,
            epoch,
            metadata,
            optimizer=optimizer,
        )
        print(f"[CHECKPOINT] Salvo em {ckpt_path}", flush=True)
        _prune_checkpoints(checkpoints_dir, keep_last_n=2)
        return True
    except Exception as e:
        print(f"[WARN] Erro ao salvar checkpoint: {e}", flush=True)
        return False



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


def _sample_output_path(output_path: Path, epoch: int) -> Path:
    """Caminho da amostra `samples/sample_epoch_{epoch:03d}.png` (convenção do loop compartilhado).

    `epoch` é a época absoluta; 0 é o baseline. Cria `samples/` se faltar. É o único
    diretório que o orchestrator sobe/lista como amostras.
    """
    samples_dir = Path(output_path) / "samples"
    samples_dir.mkdir(parents=True, exist_ok=True)
    return samples_dir / f"sample_epoch_{epoch:03d}.png"


def _baseline_sample_path(output_path: Path, epoch_offset: int) -> Path | None:
    """Caminho do baseline (`sample_epoch_000.png`), ou None em resume (`epoch_offset > 0`).

    Igual ao loop compartilhado: o baseline só existe em treino novo.
    """
    if epoch_offset != 0:
        return None
    return _sample_output_path(output_path, 0)


def _current_lr(lr_scheduler: Any, optimizer: Any, default: float) -> float:
    """LR efetivo atual: o do scheduler; sem scheduler, o do param_group; senão `default`."""
    if lr_scheduler is not None:
        return float(lr_scheduler.get_last_lr()[0])
    groups = getattr(optimizer, "param_groups", None)
    if groups:
        return float(groups[0].get("lr", default))
    return default


class _GradAccumulator:
    """Acumulação de gradiente: loss/GA por micro-batch e passo de otimizador
    a cada `grad_accum` micro-batches ou no fim da época (regra do loop compartilhado)."""

    def __init__(self, module: Any, optimizer: Any, lr_scheduler: Any, grad_accum: int) -> None:
        self.module = module
        self.optimizer = optimizer
        self.lr_scheduler = lr_scheduler
        self.grad_accum = max(1, grad_accum)
        self.pending = 0

    def reset(self) -> None:
        self.pending = 0

    def backward(self, loss: Any) -> float:
        """Retropropaga loss/GA; devolve a loss original (não escalada) como float."""
        (loss / self.grad_accum).backward()
        self.pending += 1
        return float(loss.item())

    def step_if_due(self, is_last_batch: bool) -> float | None:
        """Dá o passo se o limite de GA ou o fim da época foi atingido; devolve grad_norm ou None."""
        if self.pending % self.grad_accum == 0 or is_last_batch:
            return self.flush()
        return None

    def flush(self) -> float | None:
        """Dá o passo com o gradiente acumulado, se houver; devolve o grad_norm (pré-clip)."""
        if self.pending == 0:
            return None
        grad_norm_raw = torch.nn.utils.clip_grad_norm_(self.module.parameters(), 1.0)
        if hasattr(grad_norm_raw, "item"):
            grad_norm = float(grad_norm_raw.item())
        elif grad_norm_raw is not None:
            grad_norm = float(grad_norm_raw)
        else:
            grad_norm = compute_grad_norm_l2(self.module.parameters())
        self.optimizer.step()
        if self.lr_scheduler is not None:
            self.lr_scheduler.step()
        self.optimizer.zero_grad()
        self.pending = 0
        return grad_norm


def _setup_optimization(
    module: Any,
    *,
    optimizer_name: str,
    learning_rate: float,
    optimizer_state_path: str | None,
    lr_scheduler_name: str,
    lr_warmup_steps: int,
    steps_per_epoch: int,
    epochs: int,
    epoch_offset: int,
    grad_accum: int,
) -> tuple[Any, Any, _GradAccumulator]:
    """Otimizador (`lora.optimizer`), scheduler e acumulador, como no loop compartilhado.

    O LR da requisição prevalece sobre o do optimizer state restaurado. O horizonte
    da curva é (epoch_offset + epochs) épocas; em resume o scheduler é posicionado
    nos passos das épocas já concluídas.
    """
    optimizer = _create_optimizer(module, optimizer_name, learning_rate)
    if optimizer_state_path:
        _load_optimizer_state(optimizer, optimizer_state_path)
        _override_optimizer_lr(optimizer, learning_rate)
    lr_scheduler = _create_lr_scheduler(
        optimizer,
        lr_scheduler_name,
        max(1, steps_per_epoch * (epoch_offset + epochs)),
        lr_warmup_steps,
        last_step=steps_per_epoch * epoch_offset,
    )
    return optimizer, lr_scheduler, _GradAccumulator(module, optimizer, lr_scheduler, grad_accum)


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
    except ImportError as exc:
        _die(f"Dependência ausente para treino real de Qwen-Image-2.1: {exc}")

    lora_cfg = cfg.get("lora", {})
    rank = int(lora_cfg.get("rank", 16))
    lora_alpha = int(lora_cfg.get("alpha", 16))
    epochs = int(lora_cfg.get("epochs", 10))
    learning_rate = float(lora_cfg.get("learning_rate", 2e-4))
    trigger_word = str(lora_cfg.get("trigger_word", "") or "").strip()
    raw_quant = lora_cfg.get("quantization") or cfg.get("quantization") or "4bit"
    quantization = _resolve_transformer_quantization(
        _normalize_train_quantization(raw_quant, default="4bit")
    )
    base_name = _resolve_output_name(cfg)
    seed = int(cfg.get("seed", 42))
    cache_latents = resolve_cache_latents(cfg)

    samples_cfg = cfg.get("samples", {})
    sample_prompt = str(samples_cfg.get("prompt", "") or "").strip()
    sample_interval = int(samples_cfg.get("interval", 1))
    sample_seed = int(samples_cfg.get("seed", seed))

    checkpoint_interval = max(1, int(cfg.get("checkpoint_interval") or lora_cfg.get("checkpoint_interval") or 1))
    epoch_offset = max(0, int(cfg.get("epoch_offset") or lora_cfg.get("epoch_offset") or 0))
    weights_path = cfg.get("weights_path")
    optimizer_state_path = cfg.get("optimizer_state_path")
    grad_accum = max(1, int(lora_cfg.get("gradient_accumulation_steps", 1)))
    optimizer_name = str(lora_cfg.get("optimizer", "adamw8bit"))
    lr_scheduler_name = str(lora_cfg.get("lr_scheduler", "cosine"))
    lr_warmup_steps = int(lora_cfg.get("lr_warmup_steps", 0))
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
        quant_mode = _QWEN_QUANT_CACHE_FORMAT.get(quantization)
        transformer = None
        if quant_mode is None:
            print(
                f"[QWEN-IMAGE] Quantização do transformer: none (dtype de treino {torch_dtype}, sem cache quantizado).",
                flush=True,
            )
            transformer = QwenImage21Transformer2DModel.from_pretrained(
                str(local_dir), torch_dtype=torch_dtype,
            )
        else:
            custom_identity = None
            if comfy_file and os.path.exists(comfy_file):
                try:
                    st = os.stat(comfy_file)
                    import hashlib
                    h = hashlib.md5()
                    with open(comfy_file, "rb") as f:
                        h.update(f.read(1024 * 1024))
                    custom_identity = f"{comfy_file}#{st.st_size}_{st.st_mtime_ns}_{h.hexdigest()[:12]}"
                except OSError:
                    custom_identity = f"{comfy_file}#unknown"

            quant_model_id = f"{COMFY_REPO}/qwen_image_2.1"
            quant_base = resolve_quant_base_dir(quant_model_id, quant_mode, custom_identity=custom_identity)
            transformer_cache_dir = quant_base / "transformer"
            print(
                f"[QWEN-IMAGE] Quantização do transformer: {quantization} (cache '{quant_mode}').",
                flush=True,
            )

            if _is_cache_valid(
                transformer_cache_dir,
                expected_model_id=quant_model_id,
                expected_quant=quant_mode,
                expected_custom=custom_identity,
                expected_bnb_version=bnb_ver,
            ):
                try:
                    print(
                        f"[QWEN-IMAGE] Carregando transformer {quantization} do cache quantizado validado: {transformer_cache_dir}",
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
                bnb_config = _build_transformer_bnb_config(quantization, torch_dtype)
                transformer = QwenImage21Transformer2DModel.from_pretrained(
                    str(local_dir), torch_dtype=torch_dtype, quantization_config=bnb_config,
                )
                try:
                    save_atomic_dir(transformer_cache_dir, lambda p: transformer.save_pretrained(p))
                    _save_quant_metadata(
                        quant_base,
                        model_id=quant_model_id,
                        quant_label=(
                            "4-bit NF4 + double quant BitsAndBytes"
                            if quantization == "4bit"
                            else "8-bit BitsAndBytes"
                        ),
                        quant_format=quant_mode,
                        target_dtype=torch_dtype,
                        custom_checkpoint=custom_identity,
                        bnb_version=bnb_ver,
                    )
                    print(
                        f"[QWEN-IMAGE] Transformer quantizado {quantization} persistido em cache para execuções futuras: {transformer_cache_dir}",
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
                bnb_config = _build_text_encoder_bnb_config(torch_dtype)
                print(
                    "[QWEN-IMAGE] Text encoder Qwen3-VL-8B: sempre NF4 4-bit "
                    "(congelado, só precompute; bf16 ~16 GB não cabe em 12 GB).",
                    flush=True,
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
    sampling_enabled = bool(sample_prompt) and sample_interval > 0
    # O prompt de amostra entra no mesmo cache dos prompts do dataset: assim baseline e
    # amostras por época usam o embed cacheado e o encoder pode ser descarregado de verdade.
    prompts_to_encode = unique_prompts + (
        [sample_prompt] if sampling_enabled and sample_prompt not in unique_prompts else []
    )

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

    # Pre-compute all prompts (dataset + prompt de amostra) com VRAM limpa
    _log_vram("antes do precompute de embeds")
    cached_count = 0
    for i, prompt in enumerate(prompts_to_encode):
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

    sample_payload = text_cache.get(sample_prompt) if sampling_enabled else None
    if sampling_enabled and sample_payload is None:
        print("[WARN] Embed do prompt de amostra indisponível; amostras desativadas.", flush=True)

    # Unload real do text encoder: a partir daqui nada mais codifica texto.
    _log_vram("antes do unload do text encoder")
    _release_prompt_encoder(prompt_encoder)
    text_encoder = None
    cleanup_cuda()
    _log_vram("depois do unload do text encoder")

    # Amostra baseline (sem LoRA treinado) com o embed cacheado, sem encoder
    # Mesma convenção do loop compartilhado: baseline só em treino novo (epoch_offset == 0).
    baseline_sample_path = _baseline_sample_path(output_path, epoch_offset)
    if sample_payload is not None and baseline_sample_path is not None:
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
                sample_payload,
                height=resolution,
                width=resolution,
                num_inference_steps=20,
                seed=sample_seed,
                device=device,
                dtype=torch_dtype,
                output_path=baseline_sample_path,
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
        _log_vram("depois da amostra baseline")

    # Cache de latents: codifica cada imagem UMA vez (distribuição do VAE) e tira o VAE da GPU
    latents_cached = False
    if cache_latents:
        latent_cache = LatentCache(
            output_path,
            True,
            namespace_fields={
                "arch": "qwen_image_2_1",
                "model_id": base_model_path,
                "vae_fingerprint": vae_fingerprint(vae),
                "dtype": str(torch_dtype),
            },
        )
        latents_cached = prepare_latent_cache(
            latent_cache,
            [dataset],
            lambda px: _qwen_encode_dist(vae, px, torch_dtype),
            device=device,
            dtype=torch_dtype,
            metrics_path=metrics_path,
        )
        if latents_cached:
            vae.to("cpu")
            cleanup_cuda()

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
    # 5-6. Otimizador + LR scheduler (mesmo contrato do loop compartilhado) e treino
    num_train_samples = len(dataset)
    steps_per_epoch = max(1, math.ceil(len(dataloader) / grad_accum))
    total_progress_steps = steps_per_epoch * epochs
    optimizer, lr_scheduler, accumulator = _setup_optimization(
        model_with_lora,
        optimizer_name=optimizer_name,
        learning_rate=learning_rate,
        optimizer_state_path=optimizer_state_path,
        lr_scheduler_name=lr_scheduler_name,
        lr_warmup_steps=lr_warmup_steps,
        steps_per_epoch=steps_per_epoch,
        epochs=epochs,
        epoch_offset=epoch_offset,
        grad_accum=grad_accum,
    )

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
    pending_losses: list[float] = []
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
            accumulator.reset()

            for batch_idx, batch in enumerate(dataloader):
                # Prepare batch
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

                # Latents: amostrados de mean/std em cache (VAE fora da GPU) ou
                # codificados online com o VAE trazido à GPU só durante o encode
                with torch.no_grad():
                    if not latents_cached:
                        vae.to(device)
                    latents = _qwen_normalized_latents(vae, batch, device, torch_dtype)
                    if not latents_cached:
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

                    # Backward com acumulação: loss/GA; o passo de otimizador (clip,
                    # optimizer.step, scheduler.step, zero_grad) ocorre a cada GA
                    # micro-batches ou no fim da época (mesma regra do loop compartilhado).
                    cur_loss_item = accumulator.backward(loss)
                    is_last_batch = batch_idx == len(dataloader) - 1
                    stepped_grad_norm = accumulator.step_if_due(is_last_batch)
                    diag_tracker.observe_step(cur_loss_item, stepped_grad_norm)

                    pending_losses.append(cur_loss_item)
                    epoch_loss += cur_loss_item
                    num_batches += 1
                    if stepped_grad_norm is not None:
                        last_grad_norm = stepped_grad_norm
                        global_step += 1
                        # Metrics: a cada 5 passos de otimizador ou no último da época
                        if global_step % 5 == 0 or is_last_batch:
                            avg_loss = sum(pending_losses) / len(pending_losses)
                            pending_losses.clear()
                            effective_lr = _current_lr(lr_scheduler, optimizer, learning_rate)
                            elapsed = time.time() - start_time
                            time_per_step = elapsed / global_step
                            remaining_steps = max(0, total_progress_steps - global_step)
                            eta_seconds = time_per_step * remaining_steps

                            progress = min(0.99, 0.45 + (global_step / total_progress_steps) * 0.50)
                            vram_used = vram_allocated_gb()
                            diag_payload = diag_tracker.build_diagnostics(
                                grad_norm_l2=last_grad_norm,
                                optimizer=optimizer,
                                default_lr=effective_lr,
                                model=model_with_lora,
                                step=global_step,
                            )
                            _emit_metric(
                                metrics_path,
                                epoch=epoch,
                                step=global_step,
                                loss=avg_loss,
                                lr=effective_lr,
                                grad_norm=last_grad_norm,
                                diagnostics=diag_payload,
                                progress=progress,
                                phase="training",
                                message=f"Epoch {epoch}/{epoch_offset + epochs}, step {global_step}/{total_progress_steps}, loss={(f'{avg_loss:.4f}' if math.isfinite(avg_loss) else 'null')}",
                                eta_s=int(eta_seconds),
                                step_time_s=time_per_step,
                                vram_reserved_gb=vram_used,
                            )

                # Cleanup
                torch.cuda.empty_cache()

            # Resto de acumulação (ex.: último batch pulado por falta de cache) vira passo.
            flushed_grad_norm = accumulator.flush()
            if flushed_grad_norm is not None:
                last_grad_norm = flushed_grad_norm
                global_step += 1

            # End of epoch
            avg_epoch_loss = epoch_loss / max(1, num_batches)
            epoch_diag = diag_tracker.build_diagnostics(
                grad_norm_l2=last_grad_norm,
                optimizer=optimizer,
                default_lr=_current_lr(lr_scheduler, optimizer, learning_rate),
                model=model_with_lora,
                step=global_step,
                force_lora=True,
            )
            _emit_metric(
                metrics_path,
                epoch=epoch,
                step=global_step,
                loss=avg_epoch_loss,
                lr=_current_lr(lr_scheduler, optimizer, learning_rate),
                grad_norm=last_grad_norm,
                diagnostics=epoch_diag,
                progress=0.45 + local_epoch_idx / epochs * 0.50,
                phase="epoch_complete",
                message=f"Epoch {epoch} completo, loss médio={(f'{avg_epoch_loss:.4f}' if math.isfinite(avg_epoch_loss) else 'null')}",
            )

            # Checkpoint uma vez por época (checkpoint_interval é em ÉPOCAS), após o flush
            # de acumulação; mesma regra do loop compartilhado (índice local da execução).
            _maybe_save_epoch_checkpoint(
                model_with_lora,
                optimizer,
                checkpoints_dir,
                base_name,
                epoch=epoch,
                local_epoch_idx=local_epoch_idx,
                epochs=epochs,
                checkpoint_interval=checkpoint_interval,
                global_step=global_step,
                quantization=quantization,
            )

            # Sample during training (reduced resolution)
            if sample_payload is not None and sample_interval > 0 and local_epoch_idx % sample_interval == 0:
                try:
                    sample_res = min(resolution, 512)
                    _log_vram(f"antes da amostra epoch {epoch}")
                    _qwen_sample_native(
                        transformer,
                        vae,
                        sample_payload,
                        height=sample_res,
                        width=sample_res,
                        num_inference_steps=20,
                        seed=sample_seed,
                        device=device,
                        dtype=torch_dtype,
                        output_path=_sample_output_path(output_path, epoch),
                    )
                    _log_vram(f"depois da amostra epoch {epoch}")
                except Exception as e:
                    print(f"[WARN] Erro ao gerar amostra epoch {epoch}: {e}", flush=True)
                if latents_cached:
                    # _qwen_sample_native deixa o VAE na GPU após o decode; devolve à CPU.
                    vae.to("cpu")
                    cleanup_cuda()

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
        lr=_current_lr(lr_scheduler, optimizer, learning_rate),
        progress=0.95,
        phase="saving",
        message="Salvando adaptador final...",
    )

    metadata = {
        "format": "pt",
        "model_type": "lora",
        "base_model": "qwen-image-2.1",
        "quantization": quantization,
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
        lr=_current_lr(lr_scheduler, optimizer, learning_rate),
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
