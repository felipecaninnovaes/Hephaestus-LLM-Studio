"""Utilitários compartilhados e helpers comuns para o trainer de difusão."""

from __future__ import annotations

import hashlib
import json
import os
import struct
import sys
from pathlib import Path
from typing import Any


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

        # ADR-0021: Espelha em telemetry.jsonl no formato canônico
        try:
            import datetime

            telemetry_path = metrics_path.parent / "telemetry.jsonl"
            now_iso = datetime.datetime.now(datetime.timezone.utc).isoformat()
            t_phase = phase or ("training" if epoch > 0 else "preparing")
            t_msg = message or (
                f"Treinando Época {epoch}, Passo {step}"
                if epoch > 0
                else "Preparando pipeline de difusão..."
            )
            t_prog = progress if progress is not None else 0.0

            vram_val = None
            try:
                import torch

                if torch.cuda.is_available():
                    vram_val = round(torch.cuda.memory_allocated() / (1024**3), 2)
            except Exception:
                pass

            t_payload: dict[str, Any] = {
                "timestamp": now_iso,
                "phase": t_phase,
                "phaseMessage": t_msg,
                "progress": round(t_prog, 4),
                "step": step,
                "epoch": epoch,
            }
            if vram_val is not None:
                t_payload["vramUsedGb"] = vram_val
            m_dict: dict[str, Any] = {}
            if loss is not None:
                m_dict["loss"] = loss
            if lr is not None:
                m_dict["lr"] = lr
            if m_dict:
                t_payload["metrics"] = m_dict

            with open(telemetry_path, "a", encoding="utf-8") as tf:
                tf.write(json.dumps(t_payload) + "\n")
                tf.flush()
        except Exception:
            pass
    except Exception as e:
        print(
            f"[WARN] Falha ao emitir métrica para {metrics_path}: {e}",
            file=sys.stderr,
            flush=True,
        )


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

    token = (
        hf_token
        or os.environ.get("HF_TOKEN")
        or os.environ.get("HUGGING_FACE_HUB_TOKEN")
        or ""
    ).strip()
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


def _save_lora_safetensors(
    model: Any, output_file: Path, metadata: dict[str, str]
) -> None:
    """Salva os pesos do adaptador LoRA em formato .safetensors canônico."""
    import safetensors.torch
    from peft import get_peft_model_state_dict

    lora_state_dict = get_peft_model_state_dict(model)
    safetensors.torch.save_file(lora_state_dict, str(output_file), metadata=metadata)
