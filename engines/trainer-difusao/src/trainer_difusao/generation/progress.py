"""
Telemetria fina e callbacks de progresso do sampler durante a geração.
"""
from __future__ import annotations

_GENERATE_PROGRESS_BASE = 0.55
_GENERATE_PROGRESS_SPAN = 0.35
_SAMPLER_EMIT_MIN_DELTA_PERMILLE = 10
_MOCK_TELEMETRY_SUBSTEPS = 4


def _sampler_progress(
    image_index: int, batch_size: int, sampler_step: int, num_sampler_steps: int
) -> float:
    """Progresso interpolado do sampler step dentro da fatia da imagem atual."""
    batch = max(1, int(batch_size))
    total = max(1, int(num_sampler_steps))
    frac = (int(sampler_step) + 1) / total
    frac = max(0.0, min(1.0, frac))
    return _GENERATE_PROGRESS_BASE + _GENERATE_PROGRESS_SPAN * (int(image_index) + frac) / batch


def _should_emit(
    prev_progress: float | None,
    cur_progress: float,
    *,
    is_first: bool = False,
    is_last: bool = False,
) -> bool:
    """Throttle do callback do sampler: no máximo ~50 eventos por imagem."""
    if is_first or is_last:
        return True
    if prev_progress is None:
        return True
    try:
        return (
            int(float(cur_progress) * 1000) - int(float(prev_progress) * 1000)
        ) >= _SAMPLER_EMIT_MIN_DELTA_PERMILLE
    except (TypeError, ValueError):
        return True


def _make_sampler_callback(
    emitter: object, image_index: int, batch_size: int, num_sampler_steps: int
):
    """Constrói `callback_on_step_end` do diffusers p/ a imagem atual."""
    total = max(1, int(num_sampler_steps))
    state: dict[str, float | None] = {"prev": None}

    def _callback(pipe: object, step: int, timestep: object, callback_kwargs: dict | None = None):
        try:
            s = int(step)
            progress = _sampler_progress(image_index, batch_size, s, total)
            if _should_emit(
                state["prev"], progress, is_first=(s <= 0), is_last=(s >= total - 1)
            ):
                emitter.emit(  # type: ignore[attr-defined]
                    phase="generating",
                    message=f"Gerando imagem {int(image_index) + 1}/{int(batch_size)} · step {s + 1}/{total}",
                    progress=progress,
                    step=int(image_index),
                    total_steps=int(batch_size),
                )
                state["prev"] = progress
        except Exception:
            pass
        return callback_kwargs if callback_kwargs is not None else {}

    return _callback


def _pipe_call_kwargs_with_callback(
    emitter: object, image_index: int, batch_size: int, num_sampler_steps: int
) -> dict[str, object]:
    """Kwargs de callback p/ `pipe(...)`; `{}` (no-op) se construção falhar."""
    try:
        return {
            "callback_on_step_end": _make_sampler_callback(
                emitter, image_index, batch_size, num_sampler_steps
            ),
            "callback_on_step_end_tensor_inputs": ["latents"],
        }
    except Exception:
        return {}
