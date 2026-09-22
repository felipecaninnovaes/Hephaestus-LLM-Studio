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
    ) -> dict[str, Any]:
        """Emite um evento estruturado de telemetria com flush imediato."""
        self._current_phase = phase
        self._last_progress = max(0.0, min(1.0, float(progress)))

        # Tenta capturar VRAM alocada via PyTorch se disponível
        if vram_used_gb is None:
            try:
                import torch
                if torch.cuda.is_available():
                    vram_used_gb = round(torch.cuda.memory_allocated() / (1024 ** 3), 2)
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
        if metrics:
            event["metrics"] = {k: v for k, v in metrics.items() if v is not None}

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
        print(f"[TELEMETRY] [{phase.upper()}] ({pct}%){vram_str} {message}", flush=True)
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
