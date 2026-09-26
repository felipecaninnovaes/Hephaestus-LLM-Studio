"""
Módulo de Telemetria Padronizada para Engines do Hephaestus Studio (ADR-0021).

Permite aos motores de treinamento e inferência emitir eventos estruturados
em tempo real para 'telemetry.jsonl' (com espelhamento em 'metrics.jsonl').
"""
import datetime
import json
import os
import sys
from pathlib import Path
from typing import Any, Optional


def format_eta(seconds: Optional[int | float]) -> str:
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


class TelemetryEmitter:
    """Emissor atômico de telemetria estruturada."""

    def __init__(
        self,
        output_dir: Path | str,
        filename: str = "telemetry.jsonl",
        legacy_filename: Optional[str] = "metrics.jsonl",
    ):
        self.output_dir = Path(output_dir)
        self._ensure_writable()
        self.telemetry_path = self.output_dir / filename
        self.legacy_path = self.output_dir / legacy_filename if legacy_filename else None
        self._current_phase = "init"
        self._last_progress = 0.0

    def _ensure_writable(self) -> None:
        """Fail-fast: saída não-gravável aborta ANTES de gastar GPU.

        Engines rodam como uid 1000 (`studio`) sobre volumes compartilhados com
        o orquestrador (root); sem permissão de escrita o único desfecho do job
        é EACCES nos artefatos — melhor morrer em segundos do que após a geração
        (docs/PITFALLS.md, infra).
        """
        try:
            self.output_dir.mkdir(parents=True, exist_ok=True)
            probe = self.output_dir / ".write-probe"
            probe.write_bytes(b"")
            probe.unlink()
        except OSError as exc:
            uid = os.getuid() if hasattr(os, "getuid") else -1
            raise RuntimeError(
                f"Sem permissão de escrita em {self.output_dir} "
                f"(errno {exc.errno}: {exc.strerror}); uid efetivo do engine = {uid}. "
                "Verifique dono/modo do diretório no volume compartilhado com o orquestrador."
            ) from exc

    def emit(
        self,
        phase: str,
        message: str,
        progress: float,
        step: Optional[int] = None,
        total_steps: Optional[int] = None,
        epoch: Optional[int] = None,
        total_epochs: Optional[int] = None,
        metrics: Optional[dict[str, Any]] = None,
        vram_used_gb: Optional[float] = None,
        vram_reserved_gb: Optional[float] = None,
        step_time_seconds: Optional[float] = None,
        speed: Optional[str] = None,
        eta_seconds: Optional[int] = None,
        eta_formatted: Optional[str] = None,
    ) -> dict[str, Any]:
        """Emite um evento estruturado de telemetria com flush imediato."""
        self._current_phase = phase
        self._last_progress = max(0.0, min(1.0, float(progress)))

        # Tenta capturar VRAM alocada via PyTorch se disponível
        if vram_used_gb is None:
            try:
                from engine_kit.vram import vram_allocated_gb
                vram_used_gb = vram_allocated_gb()
            except Exception:
                pass
            if vram_used_gb is None:
                try:
                    import torch
                    if torch.cuda.is_available():
                        vram_used_gb = round(torch.cuda.memory_allocated() / (1024 ** 3), 2)
                except Exception:
                    pass

        # Tenta capturar VRAM reservada via PyTorch se disponível
        if vram_reserved_gb is None:
            try:
                from engine_kit.vram import vram_reserved_gb as _get_vram_reserved
                vram_reserved_gb = _get_vram_reserved()
            except Exception:
                pass

        now_iso = datetime.datetime.now(datetime.timezone.utc).isoformat()
        event: dict[str, Any] = {
            "timestamp": now_iso,
            "phase": phase,
            "phaseMessage": message,
            "message": message,
            "progress": round(self._last_progress, 4),
        }
        if step is not None:
            event["step"] = int(step)
        if total_steps is not None:
            event["totalSteps"] = int(total_steps)
        if epoch is not None:
            event["epoch"] = int(epoch)
        if total_epochs is not None:
            event["totalEpochs"] = int(total_epochs)
        if vram_used_gb is not None:
            event["vramUsedGb"] = float(vram_used_gb)
        if vram_reserved_gb is not None:
            event["vramReservedGb"] = float(vram_reserved_gb)
        if step_time_seconds is not None:
            event["stepTimeSeconds"] = float(step_time_seconds)
        if speed is not None:
            event["speed"] = str(speed)
        elif step_time_seconds is not None:
            event["speed"] = f"{step_time_seconds:.1f}s/step"
        if eta_seconds is not None:
            event["etaSeconds"] = int(eta_seconds)
        if eta_formatted is not None:
            event["etaFormatted"] = str(eta_formatted)
        elif eta_seconds is not None:
            event["etaFormatted"] = format_eta(eta_seconds)
        if metrics:
            clean_metrics: dict[str, Any] = {}
            for k, v in metrics.items():
                if v is None:
                    continue
                if k == "loss_ema":
                    clean_metrics["lossEma"] = v
                else:
                    clean_metrics[k] = v
            event["metrics"] = clean_metrics
        line = json.dumps(event) + "\n"

        # 1. Grava no telemetry.jsonl canônico
        try:
            with open(self.telemetry_path, "a", encoding="utf-8") as f:
                f.write(line)
                f.flush()
        except Exception as e:
            print(f"[WARN] Falha ao gravar telemetria para {self.telemetry_path}: {e}", file=sys.stderr, flush=True)

        # 2. Espelha no metrics.jsonl para compatibilidade com leitores legados
        if self.legacy_path is not None:
            try:
                legacy_payload = dict(event)
                if step is not None:
                    legacy_payload["step"] = step
                if epoch is not None:
                    legacy_payload["epoch"] = epoch
                if metrics:
                    legacy_payload.update(metrics)
                legacy_payload["phase"] = phase
                legacy_payload["phaseMessage"] = message
                legacy_payload["message"] = message
                legacy_payload["progress"] = self._last_progress
                legacy_line = json.dumps(legacy_payload) + "\n"
                with open(self.legacy_path, "a", encoding="utf-8") as f:
                    f.write(legacy_line)
                    f.flush()
            except Exception:
                pass

        # 3. Emite log limpo no stdout
        pct = int(self._last_progress * 100)
        vram_str = f" | VRAM: {vram_used_gb:.1f}GB" if vram_used_gb is not None else ""

        log_msg = message
        extras: list[str] = []

        eff_speed = event.get("speed")
        if eff_speed and eff_speed not in message and "s/step" not in message and "it/s" not in message:
            extras.append(eff_speed)

        eff_eta = event.get("etaFormatted")
        if eff_eta and eff_eta not in message and "ETA" not in message:
            extras.append(f"ETA: {eff_eta}" if not eff_eta.startswith("ETA") else eff_eta)

        if extras:
            suffix = " · ".join(extras)
            log_msg = f"{log_msg} · {suffix}" if log_msg else suffix

        print(f"[TELEMETRY] [{phase.upper()}] ({pct}%){vram_str} {log_msg}", flush=True)
        return event

    def error(self, message: str, exc: Optional[BaseException] = None) -> dict[str, Any]:
        """Emite evento terminal de erro da fase."""
        err_msg = f"{message}: {exc}" if exc else message
        return self.emit(
            phase="error",
            message=err_msg,
            progress=self._last_progress,
            metrics={"error": True, "error_type": exc.__class__.__name__ if exc else "GenericError"},
        )
