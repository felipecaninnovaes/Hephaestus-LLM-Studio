"""Telemetria por passo de otimizador: um registro por passo e linha de console com ETA."""
from __future__ import annotations

import contextlib
import io
import json
from pathlib import Path

from trainer_difusao.common_pkg.metrics import (
    StepTimer,
    emit_training_step,
    format_step_console_line,
)


def _run_steps(tmp_path: Path, total_steps: int) -> tuple[list[dict], list[str]]:
    metrics = tmp_path / "metrics.jsonl"
    timer = StepTimer()
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        for step in range(1, total_steps + 1):
            emit_training_step(
                metrics,
                epoch=1 + (step - 1) // 4,
                total_epochs=2,
                step=step,
                total_steps=total_steps,
                loss=0.25,
                lr=1e-4,
                grad_norm=0.5,
                diagnostics=None,
                progress=0.1 + 0.8 * step / total_steps,
                timer=timer,
            )
    records = [json.loads(line) for line in metrics.read_text().splitlines()]
    return records, buf.getvalue().splitlines()


def test_one_training_record_per_optimizer_step_not_every_fifth(tmp_path: Path) -> None:
    records, _ = _run_steps(tmp_path, 7)  # 7 passos: o antigo "% 5" emitiria só o 5
    training = [r for r in records if r.get("phase") == "training"]
    assert [r["step"] for r in training] == [1, 2, 3, 4, 5, 6, 7]
    assert all(r["total_steps"] == 7 and "eta_formatted" in r and "step_time_s" in r for r in training)


def test_console_line_per_step_has_progress_step_loss_speed_vram_and_eta(tmp_path: Path) -> None:
    _, lines = _run_steps(tmp_path, 3)
    assert len(lines) == 3
    for step, line in enumerate(lines, start=1):
        assert line.startswith("[TELEMETRY] [TRAINING] (")
        assert f"Step {step}/3" in line
        assert "Época " in line and "/2" in line
        assert "Loss: 0.2500" in line
        assert "s/step" in line
        assert "ETA: " in line


def test_console_line_formats_eta_with_format_eta() -> None:
    line = format_step_console_line(
        epoch=1, total_epochs=2, step=2, total_steps=100, loss=None,
        step_time_s=4.0, eta_s=392, progress=0.5, vram_gb=8.25,
    )
    assert "ETA: 6m 32s" in line
    assert "Loss: null" in line
    assert "VRAM: 8.2GB" in line or "VRAM: 8.3GB" in line


def test_step_timer_reset_drops_pause_interval() -> None:
    timer = StepTimer()
    timer.tick()
    timer.pause_reset()
    assert timer.tick() < 1.0
