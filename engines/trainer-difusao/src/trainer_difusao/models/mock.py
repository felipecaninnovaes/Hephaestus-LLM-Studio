"""Pipeline sintético de treino (ENGINE_MOCK=1) para desenvolvimento local e CI."""

from __future__ import annotations

import json
import os
import struct
import time
import shutil
from pathlib import Path
from typing import Any

from trainer_difusao.common import (
    _canonical_model_name,
    _count_control_images,
    _emit_metric,
    _normalize_train_quantization,
    _resolve_output_name,
    _synthetic_loss,
    _validate_train_aux,
)
from trainer_difusao.models.base import BaseModelTrainer


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
    if "epoch" in lora_params:
        metadata["epoch"] = str(lora_params["epoch"])
    if trigger_word:
        metadata["trigger_word"] = trigger_word
    if lora_params.get("custom_checkpoint_path"):
        metadata["custom_checkpoint_path"] = str(lora_params["custom_checkpoint_path"])
    if lora_params.get("text_encoder_path"):
        metadata["text_encoder_path"] = str(lora_params["text_encoder_path"])

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

    output_file.parent.mkdir(parents=True, exist_ok=True)
    tmp_file = output_file.parent / f".tmp_{output_file.name}"
    with open(tmp_file, "wb") as f:
        f.write(struct.pack("<Q", header_len))
        f.write(header_json)
        f.write(data_bytes)
    os.replace(tmp_file, output_file)


def _generate_mock_sample(
    output_dir: Path,
    epoch: int,
    prompt: str,
    seed: int = 42,
    metrics_path: Path | None = None,
) -> None:
    """Gera uma imagem de teste sintética para validação do fluxo de artefatos de sample.
    
    Grava de forma atômica via arquivo temporário (.tmp_*) e rename para evitar que
    leitores assíncronos (como o loop de streaming do orquestrador) leiam imagens incompletas.
    """
    samples_dir = output_dir / "samples"
    samples_dir.mkdir(parents=True, exist_ok=True)
    sample_file = samples_dir / f"sample_epoch_{epoch:03d}.png"
    tmp_file = sample_file.with_name(f".tmp_{sample_file.name}")
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
        img.save(tmp_file, format="PNG")
        os.replace(tmp_file, sample_file)
    except Exception:
        import base64

        tiny_png = base64.b64decode(
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkWPjfDwAEeQHzG4L5eAAAAABJRU5ErkJggg=="
        )
        tmp_file.write_bytes(tiny_png)
        os.replace(tmp_file, sample_file)
    if metrics_path is not None:
        from trainer_difusao.common_pkg.metrics import _emit_metric
        total_substeps = 4
        for k in range(1, total_substeps + 1):
            _emit_metric(
                metrics_path,
                phase="generating_sample",
                message=f"Gerando amostra visual da Época {epoch} (passo {k}/{total_substeps})...",
                telemetry_only=True,
            )


def _mock_train(cfg: dict[str, Any], output: Path) -> None:
    """Loop sintético de treino que emite métricas, checkpoints por época e safetensors final."""
    output.mkdir(parents=True, exist_ok=True)
    metrics_path = output / "metrics.jsonl"

    aux = _validate_train_aux(cfg, quant_default=None)
    seed = int(cfg.get("seed", 42))
    lora_cfg = cfg.get("lora", {})
    epochs = int(lora_cfg.get("epochs", 10))
    learning_rate = float(lora_cfg.get("learning_rate", 0.0001))
    raw_model = cfg.get("model", "flux")
    base_model = _canonical_model_name(raw_model)
    base_name = _resolve_output_name(cfg)

    checkpoint_interval = max(
        1, int(cfg.get("checkpoint_interval") or lora_cfg.get("checkpoint_interval") or 1)
    )
    epoch_offset = max(
        0, int(cfg.get("epoch_offset") or lora_cfg.get("epoch_offset") or 0)
    )
    weights_path = cfg.get("weights_path")
    control_dataset_path = aux["control_dataset_path"]
    control_ratio = aux["control_ratio"]
    cache_text_embeddings = aux["cache_text_embeddings"]

    samples_cfg = cfg.get("samples", {})
    sample_prompt = str(samples_cfg.get("prompt", "") or "").strip()
    sample_interval = int(samples_cfg.get("interval", 1))
    sample_seed = int(samples_cfg.get("seed", seed))

    sleep_ms = int(os.environ.get("MOCK_EPOCH_SLEEP_MS", "5"))

    if metrics_path.exists():
        metrics_path.unlink()

    lora_info = dict(lora_cfg)
    lora_info["base_model"] = base_model
    raw_quant = lora_cfg.get("quantization") or cfg.get("quantization") or "4bit"
    lora_info["quantization"] = _normalize_train_quantization(raw_quant, default="4bit")

    # Telemetry do bloco Flux.2/motor-treino: linha control + cache no primeiro log.
    control_n = _count_control_images(control_dataset_path) if control_dataset_path else 0
    print(
        f"[MOCK] Treino: control_dataset_images={control_n}, "
        f"control_ratio={control_ratio}, cache_text_embeddings={cache_text_embeddings}",
        flush=True,
    )
    if control_dataset_path:
        print(
            f"[MOCK] control dataset: {control_n} imagens, ratio {control_ratio} (mock: no-op)",
            flush=True,
        )
    if cache_text_embeddings:
        print("[MOCK] cache_text_embeddings=True (mock: no-op)", flush=True)
    # feat/pesos-custom-flux2: reflete os campos novos no log/meta (honesto,
    # sem fake de sucesso de load — o mock nunca carrega pesos reais).
    custom_cp = cfg.get("custom_checkpoint_path")
    enc_path = cfg.get("text_encoder_path")
    if custom_cp:
        print(f"[MOCK] custom_checkpoint_path={custom_cp} (mock: no-op)", flush=True)
    if enc_path:
        print(f"[MOCK] text_encoder_path={enc_path} (mock: no-op)", flush=True)
    lora_info["custom_checkpoint_path"] = str(custom_cp) if custom_cp else None
    lora_info["text_encoder_path"] = str(enc_path) if enc_path else None

    if weights_path:
        w_path = Path(weights_path)
        if w_path.exists():
            print(
                f"[MOCK] Continuando treino a partir de pesos prévios: {w_path} (offset={epoch_offset})",
                flush=True,
            )
        else:
            print(
                f"[MOCK] Arquivo de pesos especificado mas não encontrado: {w_path}",
                flush=True,
            )

    # Amostra baseline Época 0 (se configurada e sem epoch_offset)
    if sample_prompt and epoch_offset == 0:
        _generate_mock_sample(output, 0, sample_prompt, seed=sample_seed, metrics_path=metrics_path)
        _emit_metric(
            metrics_path,
            epoch=0,
            step=1,
            progress=0.05,
            phase="baseline_ready",
            message="Amostra baseline gerada com sucesso (Época 0).",
        )

    checkpoints_dir = output / "checkpoints"
    checkpoints_dir.mkdir(parents=True, exist_ok=True)

    for ep_idx in range(1, epochs + 1):
        ep = ep_idx + epoch_offset
        loss = _synthetic_loss(seed, ep, epochs + epoch_offset)
        progress = round(ep_idx / epochs, 4)
        suffix = (
            f" · control_dataset_images={control_n} · "
            f"cache_text_embeddings={cache_text_embeddings}"
            if ep_idx == 1 and (control_n or cache_text_embeddings)
            else ""
        )
        _emit_metric(
            metrics_path,
            epoch=ep,
            step=ep_idx * 10,
            loss=loss,
            lr=learning_rate,
            progress=progress,
            phase="training",
            message=f"Época {ep}/{epochs + epoch_offset} concluída · Loss: {loss}{suffix}",
        )

        # Salva checkpoint da época respeitando checkpoint_interval
        if ep_idx % checkpoint_interval == 0 or ep_idx == epochs:
            ckpt_file = checkpoints_dir / f"{base_name}_epoch_{ep:03d}.safetensors"
            _generate_mock_safetensors(ckpt_file, {**lora_info, "epoch": str(ep)})

        if sample_prompt and sample_interval > 0 and (ep_idx % sample_interval == 0 or ep_idx == epochs):
            _emit_metric(
                metrics_path,
                epoch=ep,
                phase="generating_sample",
                message=f"Iniciando geração de amostra visual (Época {ep})...",
                telemetry_only=True,
            )
            _generate_mock_sample(output, ep, sample_prompt, seed=sample_seed, metrics_path=metrics_path)
            _emit_metric(
                metrics_path,
                epoch=ep,
                phase="sample_ready",
                message=f"Amostra visual da Época {ep} pronta.",
                telemetry_only=True,
            )

        if sleep_ms > 0:
            time.sleep(sleep_ms / 1000.0)

    # Salva adaptador final com nome semântico configurado
    final_adapter_path = output / f"{base_name}.safetensors"
    _generate_mock_safetensors(final_adapter_path, lora_info)

    # Compatibilidade retroativa: garante existência de adapter.safetensors
    if base_name != "adapter":
        shutil.copy2(final_adapter_path, output / "adapter.safetensors")



class MockTrainer(BaseModelTrainer):
    """Trainer sintético para ambiente sem GPU / CI."""

    def train(self, cfg: dict[str, Any], output: Path) -> None:
        _mock_train(cfg, output)
