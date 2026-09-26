"""Unified training loop runner for diffusion model LoRA training (Template Method pattern)."""

from __future__ import annotations

import math
import random
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Protocol, TypedDict

from trainer_difusao.common import (
    ENABLE_TEXT_ENCODER_UNLOAD,
    TextEmbedsCache,
    _cached_encode,
    _cleanup_cuda,
    _cycling_batches,
    _die,
    _emit_metric,
    _load_lora_weights,
    _precompute_text_cache,
    _precompute_text_cache_with_cleanup,
    _prune_checkpoints,
    _resolve_output_name,
    _setup_cache_dir,
    save_adapter_checkpoint,
    save_final_adapter,
    _validate_train_aux,
)
from trainer_difusao.dataset import DiffusionDataset, build_dataloader
from trainer_difusao.optimizers import _create_lr_scheduler, _create_optimizer

__all__ = ["LoraTrainConfig", "parse_lora_train_config", "ModelAdapter", "ModelComponents", "TrainingLoopRunner"]


@dataclass(frozen=True)
class LoraTrainConfig:
    """Configuração de treino LoRA unificada para todas as arquiteturas de difusão."""

    seed: int
    model_id: str
    dataset_path: Path
    epochs: int
    batch_size: int
    learning_rate: float
    rank: int
    alpha: int
    trigger_word: str
    base_name: str
    resolution: int
    enable_bucket: bool
    grad_accum: int
    optimizer_name: str
    lr_scheduler_name: str
    lr_warmup_steps: int
    checkpoint_interval: int
    epoch_offset: int
    weights_path: str | None
    mixed_precision: str
    quantization: str
    sample_prompt: str
    sample_interval: int
    sample_seed: int
    custom_checkpoint_path: str | None
    text_encoder_path: str | None


def parse_lora_train_config(
    cfg: dict[str, Any],
    *,
    default_model_id: str,
    default_resolution: int,
    arch_name: str = "esta arquitetura",
    quant_default: str = "none",
    allow_custom_checkpoint: bool = True,
    allow_text_encoder_path: bool = False,
) -> LoraTrainConfig:
    """Parsa configuração de treino LoRA com validação e defaults por arquitetura.
    
    Substitui parsing duplicado entre sd15.py, sdxl.py, flux.py e qwen_image.py.
    Usa _validate_train_aux para validação de chaves auxiliares.
    """
    aux = _validate_train_aux(cfg, quant_default=quant_default)

    seed = int(cfg.get("seed", 42))
    model_id = cfg.get("model_id") or default_model_id
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

    resolution = int(lora_cfg.get("resolution", default_resolution))
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

    # custom_checkpoint_path: permite carregamento de UNet custom via from_single_file
    raw_custom_cp = cfg.get("custom_checkpoint_path")
    custom_checkpoint_path: str | None = None
    if raw_custom_cp:
        if not allow_custom_checkpoint:
            _die(f"custom_checkpoint_path não é suportado para arch {arch_name}.")
        if not isinstance(raw_custom_cp, str) or not raw_custom_cp.strip():
            _die("custom_checkpoint_path deve ser uma string não vazia.")
        custom_checkpoint_path = raw_custom_cp.strip()

    # text_encoder_path: permite carregamento de text encoder custom (flux-2-klein, qwen-image)
    raw_enc = cfg.get("text_encoder_path")
    text_encoder_path: str | None = None
    if raw_enc:
        if not allow_text_encoder_path:
            _die(
                f"text_encoder_path só é suportado com arch flux-2-klein-4b "
                f"(treino {arch_name} não usa encoder custom)."
            )
        if not isinstance(raw_enc, str) or not raw_enc.strip():
            _die("text_encoder_path deve ser uma string não vazia.")
        text_encoder_path = raw_enc.strip()

    mixed_precision = str(lora_cfg.get("mixed_precision", "fp16")).lower().strip()

    quantization = aux["quantization"] or "none"

    return LoraTrainConfig(
        seed=seed,
        model_id=model_id,
        dataset_path=dataset_path,
        epochs=epochs,
        batch_size=batch_size,
        learning_rate=learning_rate,
        rank=rank,
        alpha=alpha,
        trigger_word=trigger_word,
        base_name=base_name,
        resolution=resolution,
        enable_bucket=enable_bucket,
        grad_accum=grad_accum,
        optimizer_name=optimizer_name,
        lr_scheduler_name=lr_scheduler_name,
        lr_warmup_steps=lr_warmup_steps,
        checkpoint_interval=checkpoint_interval,
        epoch_offset=epoch_offset,
        weights_path=weights_path,
        mixed_precision=mixed_precision,
        quantization=quantization,
        sample_prompt=sample_prompt,
        sample_interval=sample_interval,
        sample_seed=sample_seed,
        custom_checkpoint_path=custom_checkpoint_path,
        text_encoder_path=text_encoder_path,
    )


class ModelComponents(TypedDict):
    """Componentes carregados de um modelo de difusão com LoRA injetado."""

    trainable_module: Any  # UNet ou Transformer com LoRA injetado
    device: Any  # torch.device
    dtype: Any  # torch.dtype
    extra: dict[str, Any]  # tokenizers, encoders, vae, noise_scheduler, etc (opaco por arquitetura)


class ModelAdapter(Protocol):
    """Protocolo que abstrai as diferenças de arquitetura (SD15, SDXL, Flux, Qwen-Image)."""

    arch_label: str
    metadata_base_model: str

    def parse_lora_train_config(self, cfg: dict[str, Any]) -> LoraTrainConfig:
        """Parseia cfg com defaults específicos da arquitetura (model_id, resolution, quant_default)."""
        ...

    def load_and_inject_lora(self, tcfg: LoraTrainConfig, hub_cache: str, metrics_path: Path) -> ModelComponents:
        """Carrega modelo base, injeta LoRA, retorna componentes para treino."""
        ...

    def build_text_cache_encode_fn(self, comp: ModelComponents) -> Callable[[list[str]], dict[str, Any]]:
        """Retorna função que encoda captions para o cache de embeddings de texto."""
        ...

    def text_cache_encoders(self, comp: ModelComponents) -> list[Any]:
        """Retorna lista de encoders de texto para offload/cleanup."""
        ...

    def precompute_sample_embeds(self, comp: ModelComponents, tcfg: LoraTrainConfig) -> Any | None:
        """Pré-computa embeddings da amostra se sample_prompt fornecido."""
        ...

    def forward_and_loss(self, comp: ModelComponents, batch: dict, tcfg: LoraTrainConfig, cached_encode: dict[str, Any]) -> Any:
        """Forward pass e cálculo de loss (retorna tensor de loss)."""
        ...

    def generate_sample(
        self, comp: ModelComponents, sample_file: Path, tcfg: LoraTrainConfig, *, epoch: int, metrics_path: Path, sample_embeds: Any | None
    ) -> None:
        """Gera amostra visual (salva em sample_file)."""
        ...

    def checkpoint_metadata(self, tcfg: LoraTrainConfig, *, epoch: int | None = None) -> dict[str, str]:
        """Retorna metadados para checkpoint (base_model, lora_rank, etc)."""
        ...


class TrainingLoopRunner:
    """Executor unificado do laço de treino LoRA para todas as arquiteturas de difusão.
    
    Usa Template Method: delega operações específicas de arquitetura para ModelAdapter,
    mantendo a orquestração de treino (dataset, optimizer, scheduler, loops, checkpointing) centralizada.
    """

    def __init__(self, adapter: ModelAdapter) -> None:
        self.adapter = adapter

    def run(self, cfg: dict[str, Any], output: Path) -> None:
        """Executa o loop de treino LoRA com checkpoint, sampling periódico e telemetria."""
        import torch
        import torch.nn.functional as F

        output.mkdir(parents=True, exist_ok=True)
        metrics_path = output / "metrics.jsonl"
        if metrics_path.exists():
            metrics_path.unlink()

        hub_cache = _setup_cache_dir()

        if not torch.cuda.is_available():
            _die("CUDA não disponível para treino real de difusão (ENGINE_MOCK=0)")

        # Parse configuração com defaults por arquitetura
        tcfg = self.adapter.parse_lora_train_config(cfg)

        # Determina dtype
        target_dtype = (
            torch.bfloat16
            if (tcfg.mixed_precision == "bf16" and torch.cuda.is_bf16_supported())
            else torch.float16
        )

        # Emite métrica de inicialização
        _emit_metric(
            metrics_path,
            epoch=0,
            step=1,
            progress=0.01,
            phase="init",
            message=f"Inicializando treino {self.adapter.arch_label}: {tcfg.model_id}...",
        )

        # Carrega modelos base e injeta LoRA
        print(
            f"Carregando modelos base {self.adapter.arch_label} ({tcfg.model_id}) [cache: {hub_cache}, res: {tcfg.resolution}, dtype: {target_dtype}]...",
            flush=True,
        )
        _emit_metric(
            metrics_path,
            epoch=0,
            step=2,
            progress=0.03,
            phase="loading_models",
            message=f"Baixando e carregando componentes {self.adapter.arch_label} ({tcfg.model_id})...",
        )
        comp = self.adapter.load_and_inject_lora(tcfg, hub_cache, metrics_path)
        device = comp["device"]
        dtype = comp["dtype"]

        # Cria otimizador e scheduler
        optimizer = _create_optimizer(comp["trainable_module"], tcfg.optimizer_name, tcfg.learning_rate)

        # Dataset principal
        dataset = DiffusionDataset(
            tcfg.dataset_path,
            resolution=tcfg.resolution,
            trigger_word=tcfg.trigger_word,
            enable_bucket=tcfg.enable_bucket,
            metrics_path=metrics_path,
        )
        dataloader = build_dataloader(dataset, tcfg.batch_size, seed=tcfg.seed)

        # Dataset de controle (prior-preservation)
        aux = _validate_train_aux(cfg, quant_default="none")
        control_dataset_path = aux["control_dataset_path"]
        control_ratio = aux["control_ratio"]
        cache_text_embeddings = aux["cache_text_embeddings"]

        control_dataset = None
        control_iter = None
        control_n = 0
        if control_dataset_path is not None:
            control_dataset = DiffusionDataset(
                control_dataset_path,
                resolution=tcfg.resolution,
                trigger_word="",
                enable_bucket=tcfg.enable_bucket,
                empty_captions=True,
                metrics_path=metrics_path,
            )
            control_iter = _cycling_batches(
                build_dataloader(control_dataset, tcfg.batch_size, seed=tcfg.seed)
            )
            control_n = len(control_dataset)

        # Scheduler e steps
        steps_per_epoch = math.ceil(len(dataloader) / tcfg.grad_accum)
        total_train_steps = max(1, steps_per_epoch * tcfg.epochs)
        lr_scheduler = _create_lr_scheduler(
            optimizer, tcfg.lr_scheduler_name, total_train_steps, tcfg.lr_warmup_steps
        )

        # Pré-computa embeddings da amostra
        sample_embeds = self.adapter.precompute_sample_embeds(comp, tcfg)

        # Cache de text embeddings
        text_cache = TextEmbedsCache(output, cache_text_embeddings)
        encode_fn = self.adapter.build_text_cache_encode_fn(comp)
        text_cache_encoders = self.adapter.text_cache_encoders(comp)

        if cache_text_embeddings:
            should_unload = ENABLE_TEXT_ENCODER_UNLOAD and tcfg.epoch_offset == 0
            if should_unload:
                _precompute_text_cache_with_cleanup(
                    text_cache,
                    [c for _, c in dataset.samples]
                    + ([c for _, c in control_dataset.samples] if control_dataset else []),
                    encode_fn,
                    metrics_path=metrics_path,
                    unload_encoders=True,
                    encoders=text_cache_encoders,
                )
            else:
                _precompute_text_cache(
                    text_cache,
                    [c for _, c in dataset.samples]
                    + ([c for _, c in control_dataset.samples] if control_dataset else []),
                    encode_fn,
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
            f"[{self.adapter.arch_label}] Treino: dataset={len(dataset)} imagens, "
            f"control_dataset_images={control_n}, control_ratio={control_ratio}, "
            f"cache_text_embeddings={cache_text_embeddings}, quantization={tcfg.quantization}",
            flush=True,
        )

        # Amostra baseline (Época 0) se sample_prompt fornecido e não estiver retomando
        if tcfg.sample_prompt and tcfg.epoch_offset == 0:
            _emit_metric(
                metrics_path,
                epoch=0,
                step=5,
                progress=0.09,
                phase="generating_baseline_sample",
                message=f"Gerando amostra baseline pré-treino (Época 0): '{tcfg.sample_prompt[:40]}...'",
            )
            sample_baseline_file = output / "samples" / "sample_epoch_000.png"
            self.adapter.generate_sample(comp, sample_baseline_file, tcfg, epoch=0, metrics_path=metrics_path, sample_embeds=sample_embeds)
            if sample_baseline_file.exists():
                _emit_metric(
                    metrics_path,
                    epoch=0,
                    step=6,
                    progress=0.10,
                    phase="baseline_ready",
                    message=f"Amostra baseline {self.adapter.arch_label} gerada com sucesso (Época 0).",
                )
            else:
                _emit_metric(
                    metrics_path,
                    epoch=0,
                    step=6,
                    progress=0.10,
                    phase="baseline_failed",
                    message=f"Falha ao gerar amostra baseline {self.adapter.arch_label} pré-treino.",
                )

        _emit_metric(
            metrics_path,
            epoch=tcfg.epoch_offset,
            step=7,
            progress=0.10,
            phase="training_started",
            message=f"Iniciando loop de treino {self.adapter.arch_label}: {tcfg.epochs} épocas (offset={tcfg.epoch_offset}), {total_train_steps} passos totais.",
        )

        print(
            f"Iniciando treino LoRA {self.adapter.arch_label}: {tcfg.epochs} épocas (offset={tcfg.epoch_offset}), {len(dataset)} imagens, res={tcfg.resolution}, "
            f"rank={tcfg.rank}, alpha={tcfg.alpha}, lr={tcfg.learning_rate}, grad_accum={tcfg.grad_accum}, opt={tcfg.optimizer_name}, "
            f"scheduler={tcfg.lr_scheduler_name}",
            flush=True,
        )

        global_step = 0
        safe_avg_loss = None

        # Loop principal de treino
        for epoch_idx in range(1, tcfg.epochs + 1):
            epoch = epoch_idx + tcfg.epoch_offset
            comp["trainable_module"].train()
            epoch_loss = 0.0
            steps_in_epoch = 0

            for batch in dataloader:
                # Prior-preservation: com prob. control_ratio usa batch de controle
                if control_iter is not None and random.random() < control_ratio:
                    batch = next(control_iter)

                pixel_values = batch["pixel_values"].to(device, dtype=torch.float32)
                cur_bs = pixel_values.shape[0]

                # Forward e loss via adapter
                cached_encode = _cached_encode(
                    batch["prompt"],
                    lambda caps: encode_fn(caps),
                    text_cache,
                    encoders=text_cache_encoders,
                    device=device,
                )
                loss = self.adapter.forward_and_loss(comp, batch, tcfg, cached_encode)
                loss = loss / tcfg.grad_accum
                loss.backward()

                cur_loss_raw = float(loss.item()) * tcfg.grad_accum
                steps_in_epoch += 1

                if steps_in_epoch % tcfg.grad_accum == 0 or steps_in_epoch == len(dataloader):
                    torch.nn.utils.clip_grad_norm_(comp["trainable_module"].parameters(), 1.0)
                    optimizer.step()
                    if lr_scheduler is not None:
                        lr_scheduler.step()
                    optimizer.zero_grad()
                    global_step += 1

                if not math.isnan(cur_loss_raw) and not math.isinf(cur_loss_raw):
                    epoch_loss += cur_loss_raw

                effective_lr = (
                    lr_scheduler.get_last_lr()[0] if lr_scheduler else tcfg.learning_rate
                )

                # Emite métricas a cada 5 passos de otimização
                if steps_in_epoch % tcfg.grad_accum == 0 and (
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
                        message=f"Época {epoch}/{tcfg.epochs + tcfg.epoch_offset} · Step {global_step}/{total_train_steps} · Loss: {safe_loss}",
                    )

            # Fim de época
            avg_loss = epoch_loss / max(1, steps_in_epoch)
            safe_avg_loss = (
                None
                if (math.isnan(avg_loss) or math.isinf(avg_loss))
                else round(avg_loss, 4)
            )
            effective_lr = (
                lr_scheduler.get_last_lr()[0] if lr_scheduler else tcfg.learning_rate
            )
            epoch_progress = round(
                min(0.99, max(0.10, 0.10 + 0.89 * (epoch_idx / tcfg.epochs))), 4
            )
            _emit_metric(
                metrics_path,
                epoch=epoch,
                step=global_step,
                loss=safe_avg_loss,
                lr=effective_lr,
                progress=epoch_progress,
                phase="epoch_complete",
                message=f"Época {epoch}/{tcfg.epochs + tcfg.epoch_offset} concluída · Loss Médio: {safe_avg_loss}",
            )
            print(
                f"[{self.adapter.arch_label}] Época {epoch}/{tcfg.epochs + tcfg.epoch_offset} concluída - Step {global_step} - Loss Médio: {avg_loss}",
                flush=True,
            )

            # Salva checkpoint respeitando checkpoint_interval
            if epoch_idx % tcfg.checkpoint_interval == 0 or epoch_idx == tcfg.epochs:
                checkpoints_dir = output / "checkpoints"
                metadata = self.adapter.checkpoint_metadata(tcfg, epoch=epoch)
                save_adapter_checkpoint(
                    comp["trainable_module"],
                    checkpoints_dir,
                    tcfg.base_name,
                    epoch,
                    metadata=metadata,
                )
                _prune_checkpoints(checkpoints_dir, keep_last_n=2)

            _cleanup_cuda()

            # Amostra periódica
            if (
                tcfg.sample_prompt
                and tcfg.sample_interval > 0
                and (epoch_idx % tcfg.sample_interval == 0 or epoch_idx == tcfg.epochs)
            ):
                sample_file = output / "samples" / f"sample_epoch_{epoch:03d}.png"
                _emit_metric(
                    metrics_path,
                    epoch=epoch,
                    phase="generating_sample",
                    message=f"Iniciando geração de amostra visual (Época {epoch})...",
                    telemetry_only=True,
                )
                self.adapter.generate_sample(comp, sample_file, tcfg, epoch=epoch, metrics_path=metrics_path, sample_embeds=sample_embeds)
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

        # Salva adapter final
        metadata = self.adapter.checkpoint_metadata(tcfg)
        final_adapter_file = save_final_adapter(comp["trainable_module"], output, tcfg.base_name, metadata)
        _emit_metric(
            metrics_path,
            epoch=tcfg.epochs,
            step=global_step,
            loss=safe_avg_loss,
            lr=effective_lr,
            progress=1.0,
            phase="completed",
            message=f"Treino {self.adapter.arch_label} finalizado com sucesso!",
        )
        print(f"Treino {self.adapter.arch_label} finalizado com sucesso! Checkpoint salvo em: {final_adapter_file}")
