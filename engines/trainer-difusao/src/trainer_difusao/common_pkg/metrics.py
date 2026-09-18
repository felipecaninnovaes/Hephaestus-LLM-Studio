"""
Emissão de métricas em metrics.jsonl e telemetria estruturada.
"""
from __future__ import annotations

import datetime
import json
import sys
from pathlib import Path
from typing import Any

from engine_kit.vram import vram_allocated_gb


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
            telemetry_path = metrics_path.parent / "telemetry.jsonl"
            now_iso = datetime.datetime.now(datetime.timezone.utc).isoformat()
            t_phase = phase or ("training" if epoch > 0 else "preparing")
            t_msg = message or (
                f"Treinando Época {epoch}, Passo {step}"
                if epoch > 0
                else "Preparando pipeline de difusão..."
            )
            t_prog = progress if progress is not None else 0.0

            vram_val = vram_allocated_gb()

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
