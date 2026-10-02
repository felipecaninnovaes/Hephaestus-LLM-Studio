"""
Cálculo de diagnósticos de treinamento para difusão (spec telemetria §3.5/3b).
Calcula gradNormL2, nanCount, infCount, lrPerGroup e loraNorms de forma resiliente.
"""
from __future__ import annotations

import math
from typing import Any
from engine_kit.telemetry import sanitize_finite_floats


def extract_lora_group(name: str) -> str:
    """Extrai o grupo/módulo de topo para agregação de normas LoRA (<=16 chaves)."""
    n = name
    for p in ("base_model.model.", "base_model.", "transformer.", "unet.", "model."):
        if n.startswith(p):
            n = n[len(p):]
    if n.startswith("lora_unet_"):
        n = n[len("lora_unet_"):]
    elif n.startswith("lora_"):
        n = n[len("lora_"):]
    parts = n.split(".")
    for prefix in ("down_blocks", "up_blocks", "mid_block"):
        if parts[0].startswith(prefix):
            return prefix
    return parts[0] if parts and parts[0] else "adapter"


def compute_lora_norms(model: Any, max_keys: int = 16) -> dict[str, float]:
    """Calcula a norma L2 acumulada por grupo de adaptador LoRA (<= max_keys)."""
    try:
        import torch
    except ImportError:
        return {}

    group_sq_sums: dict[str, float] = {}
    try:
        for name, param in model.named_parameters():
            if not param.requires_grad and "lora_" not in name:
                continue
            if "lora_" not in name and "lora" not in name.lower():
                continue
            group = extract_lora_group(name)
            p_data = param.detach()
            if p_data.is_floating_point():
                sq_sum = float(torch.sum(p_data.float() ** 2).item())
            else:
                sq_sum = 0.0
            group_sq_sums[group] = group_sq_sums.get(group, 0.0) + sq_sum

        norms = {k: round(math.sqrt(v), 4) for k, v in sorted(group_sq_sums.items())}
        if len(norms) > max_keys:
            # Mantém os top max_keys-1 maiores e agrupa o restante em 'other'
            sorted_items = sorted(norms.items(), key=lambda x: x[1], reverse=True)
            top_items = dict(sorted_items[: max_keys - 1])
            other_sq = sum(v ** 2 for _, v in sorted_items[max_keys - 1 :])
            top_items["other"] = round(math.sqrt(other_sq), 4)
            return top_items
        return norms
    except Exception:
        return {}


def compute_grad_norm_l2(parameters: Any) -> float:
    """Calcula norma L2 total de gradientes de parâmetros treináveis."""
    try:
        import torch
    except ImportError:
        return 0.0

    if hasattr(parameters, "parameters"):
        parameters = parameters.parameters()

    total_sq = 0.0
    for p in parameters:
        if p.grad is not None:
            g = p.grad.detach()
            if g.is_floating_point():
                total_sq += float(torch.sum(g.float() ** 2).item())
    return round(math.sqrt(total_sq), 4)


def extract_lr_per_group(optimizer: Any, default_lr: float | None = None) -> list[float]:
    """Extrai learning rates atuais de cada grupo de parâmetros do optimizer."""
    try:
        if optimizer is not None and hasattr(optimizer, "param_groups"):
            lrs = [float(g.get("lr", 0.0)) for g in optimizer.param_groups]
            if lrs:
                return lrs
    except Exception:
        pass
    if default_lr is not None:
        return [float(default_lr)]
    return []


class DiagnosticsTracker:
    """Rastreia e acumula métricas de diagnóstico na janela de passos."""

    def __init__(self, lora_interval_steps: int = 50) -> None:
        self.nan_count: int = 0
        self.inf_count: int = 0
        self.lora_interval_steps: int = lora_interval_steps
        self.cached_lora_norms: dict[str, float] = {}

    def observe_step(self, loss_val: float | None, grad_norm_val: float | None = None) -> None:
        """Observa loss e grad_norm e contabiliza NaNs e Infs."""
        if loss_val is not None:
            if math.isnan(loss_val):
                self.nan_count += 1
            elif math.isinf(loss_val):
                self.inf_count += 1

        if grad_norm_val is not None:
            if math.isnan(grad_norm_val):
                self.nan_count += 1
            elif math.isinf(grad_norm_val):
                self.inf_count += 1

    def build_diagnostics(
        self,
        grad_norm_l2: float,
        optimizer: Any,
        default_lr: float | None = None,
        model: Any = None,
        step: int = 0,
        force_lora: bool = False,
    ) -> dict[str, Any]:
        """Constrói o payload de diagnóstico garantindo nunca levantar exceção."""
        try:
            lr_groups = extract_lr_per_group(optimizer, default_lr)
            # loraNorms calculado a cada N steps ou se forçado (ex: fim de época)
            if model is not None and (force_lora or step % self.lora_interval_steps == 0 or not self.cached_lora_norms):
                norms = compute_lora_norms(model)
                if norms:
                    self.cached_lora_norms = norms

            diag: dict[str, Any] = {
                "gradNormL2": float(grad_norm_l2) if math.isfinite(float(grad_norm_l2)) else None,
                "nanCount": int(self.nan_count),
                "infCount": int(self.inf_count),
                "lrPerGroup": lr_groups,
            }
            if self.cached_lora_norms:
                diag["loraNorms"] = dict(self.cached_lora_norms)
            return sanitize_finite_floats(diag)
        except Exception:
            fallback = {
                "gradNormL2": (
                    float(grad_norm_l2)
                    if math.isfinite(float(grad_norm_l2))
                    else None
                ),
                "nanCount": int(self.nan_count),
                "infCount": int(self.inf_count),
                "lrPerGroup": [float(default_lr)] if default_lr is not None else [],
            }
            return sanitize_finite_floats(fallback)
