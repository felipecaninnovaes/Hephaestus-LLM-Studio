"""
Emissão de métricas em metrics.jsonl e telemetria estruturada.
"""
from __future__ import annotations

import datetime
import json
import math
import sys
import time
from pathlib import Path
from typing import Any

from engine_kit.telemetry import sanitize_finite_floats
from engine_kit.vram import vram_allocated_gb, vram_reserved_gb as _get_vram_reserved_gb

from engine_kit.sensors import get_global_sensors, sanitize_system_metrics

def _format_eta(seconds: int | float | None) -> str:
    """Formata segundos em representação legível humana de ETA (ex.: '2h 15m', '45s')."""
    if seconds is None or seconds < 0:
        return "N/A"
    sec = int(round(seconds))
    if sec < 60:
        return f"{sec}s"
    m = sec // 60
    s = sec % 60
    if m < 60:
        return f"{m}m {s}s" if s > 0 else f"{m}m"
    h = m // 60
    rem_m = m % 60
    return f"{h}h {rem_m}m" if rem_m > 0 else f"{h}h"

class StepTimer:
    """Tempo por passo de otimizador (EMA), imune a pausas longas (amostras/checkpoints)."""

    def __init__(self, alpha: float = 0.3) -> None:
        self._alpha = alpha
        self._last = time.monotonic()
        self.step_time_s: float | None = None

    def pause_reset(self) -> None:
        """Descarta o intervalo corrente (ex.: após amostra/checkpoint no meio do treino)."""
        self._last = time.monotonic()

    def tick(self) -> float:
        now = time.monotonic()
        dt = now - self._last
        self._last = now
        self.step_time_s = dt if self.step_time_s is None else (
            self._alpha * dt + (1.0 - self._alpha) * self.step_time_s
        )
        return self.step_time_s


def format_step_console_line(
    *,
    epoch: int,
    total_epochs: int,
    step: int,
    total_steps: int,
    loss: float | None,
    step_time_s: float,
    eta_s: int,
    progress: float,
    vram_gb: float | None,
) -> str:
    """Linha de console por passo, no formato do engine-kit (`[TELEMETRY] [FASE] (pct%) | VRAM ... ETA`)."""
    vram_str = f" | VRAM: {vram_gb:.1f}GB" if vram_gb is not None else ""
    loss_str = f"{loss:.4f}" if loss is not None and math.isfinite(loss) else "null"
    return (
        f"[TELEMETRY] [TRAINING] ({int(progress * 100)}%){vram_str} "
        f"Época {epoch}/{total_epochs} · Step {step}/{total_steps} · Loss: {loss_str} · "
        f"{step_time_s:.1f}s/step · ETA: {_format_eta(eta_s)}"
    )


def emit_training_step(
    metrics_path: Path,
    *,
    epoch: int,
    total_epochs: int,
    step: int,
    total_steps: int,
    loss: float | None,
    lr: float | None,
    grad_norm: float | None,
    diagnostics: dict[str, Any] | None,
    progress: float,
    timer: StepTimer,
) -> None:
    """Emite métrica de treino de UM passo de otimizador (metrics/telemetry.jsonl + linha de console)."""
    step_time_s = timer.tick()
    eta_s = int(step_time_s * max(0, total_steps - step))
    vram_used = vram_allocated_gb()
    line = format_step_console_line(
        epoch=epoch,
        total_epochs=total_epochs,
        step=step,
        total_steps=total_steps,
        loss=loss,
        step_time_s=step_time_s,
        eta_s=eta_s,
        progress=progress,
        vram_gb=vram_used,
    )
    _emit_metric(
        metrics_path,
        epoch=epoch,
        step=step,
        loss=loss,
        lr=lr,
        grad_norm=grad_norm,
        diagnostics=diagnostics,
        progress=progress,
        phase="training",
        message=f"Época {epoch}/{total_epochs} · Step {step}/{total_steps} · Loss: {loss}",
        total_steps=total_steps,
        total_epochs=total_epochs,
        eta_s=eta_s,
        step_time_s=step_time_s,
        vram_reserved_gb=vram_used,
    )
    print(line, flush=True)


def _emit_metric(
    metrics_path: Path,
    epoch: int = 0,
    step: int = 0,
    loss: float | None = None,
    lr: float | None = None,
    progress: float | None = None,
    phase: str | None = None,
    message: str | None = None,
    telemetry_only: bool = False,
    total_steps: int | None = None,
    total_epochs: int | None = None,
    step_time_s: float | None = None,
    eta_s: int | None = None,
    eta_formatted: str | None = None,
    vram_reserved_gb: float | None = None,
    loss_ema: float | None = None,
    speed: str | None = None,
    grad_norm: float | None = None,
    diagnostics: dict[str, Any] | None = None,
    system_metrics: dict[str, Any] | None = None,
) -> None:
    try:
        metrics_path.parent.mkdir(parents=True, exist_ok=True)
        payload: dict[str, Any] = {
            "epoch": epoch,
            "step": step,
        }
        if loss is not None:
            payload["loss"] = loss
        if loss_ema is not None:
            payload["loss_ema"] = loss_ema
        if lr is not None:
            payload["lr"] = lr
        if grad_norm is not None:
            payload["grad_norm"] = grad_norm
        if progress is not None:
            payload["progress"] = progress
        if total_steps is not None:
            payload["total_steps"] = total_steps
        if total_epochs is not None:
            payload["total_epochs"] = total_epochs
        if step_time_s is not None:
            payload["step_time_s"] = step_time_s
        if eta_s is not None:
            payload["eta_s"] = eta_s
        if eta_formatted is not None:
            payload["eta_formatted"] = eta_formatted
        elif eta_s is not None:
            payload["eta_formatted"] = _format_eta(eta_s)
        if phase is not None:
            payload["phase"] = phase
        if message is not None:
            payload["message"] = message
        # Add VRAM metrics to metrics.jsonl (calculated once, used for both outputs)
        vram_val = vram_allocated_gb()
        vram_res = vram_reserved_gb if vram_reserved_gb is not None else _get_vram_reserved_gb()
        if vram_val is not None:
            payload["vramUsedGb"] = float(vram_val)
        if vram_res is not None:
            payload["vramReservedGb"] = float(vram_res)
        if not telemetry_only:
            payload = sanitize_finite_floats(payload)
            with open(metrics_path, "a", encoding="utf-8") as f:
                f.write(json.dumps(payload, allow_nan=False) + "\n")
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

            # VRAM values already calculated above for metrics.jsonl

            t_payload: dict[str, Any] = {
                "timestamp": now_iso,
                "phase": t_phase,
                "phaseMessage": t_msg,
                "message": t_msg,
                "progress": round(t_prog, 4),
                "step": step,
                "epoch": epoch,
            }
            if total_steps is not None:
                t_payload["totalSteps"] = int(total_steps)
            if total_epochs is not None:
                t_payload["totalEpochs"] = int(total_epochs)
            if vram_val is not None:
                t_payload["vramUsedGb"] = float(vram_val)
            if vram_res is not None:
                t_payload["vramReservedGb"] = float(vram_res)
            if step_time_s is not None:
                t_payload["stepTimeSeconds"] = float(step_time_s)
            if speed is not None:
                t_payload["speed"] = str(speed)
            elif step_time_s is not None:
                t_payload["speed"] = f"{step_time_s:.1f}s/step"
            if eta_s is not None:
                t_payload["etaSeconds"] = int(eta_s)
            if eta_formatted is not None:
                t_payload["etaFormatted"] = str(eta_formatted)
            elif eta_s is not None:
                t_payload["etaFormatted"] = _format_eta(eta_s)

            m_dict: dict[str, Any] = {}
            if loss is not None:
                m_dict["loss"] = loss
            if loss_ema is not None:
                m_dict["lossEma"] = loss_ema
            if lr is not None:
                m_dict["lr"] = lr
            if grad_norm is not None:
                m_dict["grad_norm"] = grad_norm
            if m_dict:
                t_payload["metrics"] = m_dict
            if diagnostics is not None:
                try:
                    clean_diag: dict[str, Any] = {}
                    for k, v in diagnostics.items():
                        if v is None:
                            continue
                        if k in ("grad_norm_l2", "gradNormL2"):
                            f_val = float(v)
                            clean_diag["gradNormL2"] = f_val if math.isfinite(f_val) else None
                        elif k in ("nan_count", "nanCount"):
                            clean_diag["nanCount"] = int(v)
                        elif k in ("inf_count", "infCount"):
                            clean_diag["infCount"] = int(v)
                        elif k in ("lr_per_group", "lrPerGroup"):
                            clean_diag["lrPerGroup"] = [
                                float(x) if math.isfinite(float(x)) else None for x in v
                            ]
                        elif k in ("lora_norms", "loraNorms"):
                            clean_diag["loraNorms"] = {
                                str(lk): (float(lv) if math.isfinite(float(lv)) else None)
                                for lk, lv in v.items()
                            }
                        else:
                            clean_diag[k] = sanitize_finite_floats(v)
                    if clean_diag:
                        t_payload["diagnostics"] = clean_diag
                except Exception as diag_err:
                    print(
                        f"[WARN] Falha ao processar diagnostics na emissão de métrica: {diag_err}",
                        file=sys.stderr,
                        flush=True,
                    )
            if system_metrics is not None:
                try:
                    clean_sys = sanitize_system_metrics(system_metrics)
                    if clean_sys:
                        t_payload["systemMetrics"] = clean_sys
                except Exception as sys_err:
                    print(
                        f"[WARN] Falha ao sanitizar system_metrics na emissão de métrica: {sys_err}",
                        file=sys.stderr,
                        flush=True,
                    )
            else:
                try:
                    collected = get_global_sensors().collect()
                    if collected is not None:
                        clean_sys = sanitize_system_metrics(collected)
                        if clean_sys:
                            t_payload["systemMetrics"] = clean_sys
                except Exception as sys_err:
                    print(
                        f"[WARN] Falha ao coletar system_metrics na emissão de métrica: {sys_err}",
                        file=sys.stderr,
                        flush=True,
                    )
            t_payload = sanitize_finite_floats(t_payload)
            with open(telemetry_path, "a", encoding="utf-8") as tf:
                tf.write(json.dumps(t_payload, allow_nan=False) + "\n")
                tf.flush()
        except Exception:
            pass
    except Exception as e:
        print(
            f"[WARN] Falha ao emitir métrica para {metrics_path}: {e}",
            file=sys.stderr,
            flush=True,
        )
