"""Fábrica de otimizadores e schedulers de taxa de aprendizado."""

from __future__ import annotations

from typing import Any


def _create_optimizer(unet: Any, optimizer_name: str, lr: float) -> Any:
    """Cria otimizador selecionado (adamw8bit, paged_adamw8bit, paged_adamw32bit, adamw, prodigy) filtrando apenas parâmetros com gradiente ativo."""
    trainable_params = [
        p for p in unet.parameters() if getattr(p, "requires_grad", False)
    ]
    if not trainable_params:
        raise ValueError(
            "Nenhum parâmetro com requires_grad=True encontrado para treinar no otimizador."
        )

    import torch
    from trainer_difusao.common import _die

    opt_type = optimizer_name.lower().strip()
    if opt_type in ("paged_adamw8bit", "paged_adamw_8bit"):
        try:
            import bitsandbytes as bnb

            print(
                f"Usando otimizador Paged 8-bit AdamW (bitsandbytes) para {len(trainable_params)} tensores treináveis.",
                flush=True,
            )
            return bnb.optim.PagedAdamW8bit(trainable_params, lr=lr)
        except Exception as e:
            if torch.cuda.is_available():
                _die(
                    f"Otimizador paged_adamw8bit requer 'bitsandbytes' funcional com suporte a CUDA: {e}"
                )
            print(
                f"[WARN] bitsandbytes indisponível em ambiente sem CUDA ({e}), fallback para AdamW em CPU.",
                flush=True,
            )
            return torch.optim.AdamW(trainable_params, lr=lr)
    elif opt_type in ("paged_adamw32bit", "paged_adamw_32bit"):
        try:
            import bitsandbytes as bnb

            print(
                f"Usando otimizador Paged 32-bit AdamW (bitsandbytes) para {len(trainable_params)} tensores treináveis.",
                flush=True,
            )
            return bnb.optim.PagedAdamW32bit(trainable_params, lr=lr)
        except Exception as e:
            if torch.cuda.is_available():
                _die(
                    f"Otimizador paged_adamw32bit requer 'bitsandbytes' funcional com suporte a CUDA: {e}"
                )
            print(
                f"[WARN] bitsandbytes indisponível em ambiente sem CUDA ({e}), fallback para AdamW em CPU.",
                flush=True,
            )
            return torch.optim.AdamW(trainable_params, lr=lr)
    elif opt_type in ("adamw8bit", "adamw_8bit"):
        try:
            import bitsandbytes as bnb

            print(
                f"Usando otimizador 8-bit AdamW (bitsandbytes) para {len(trainable_params)} tensores treináveis.",
                flush=True,
            )
            return bnb.optim.AdamW8bit(trainable_params, lr=lr)
        except Exception as e:
            print(
                f"[WARN] bitsandbytes não disponível ({e}), fallback para AdamW padrão.",
                flush=True,
            )
            return torch.optim.AdamW(trainable_params, lr=lr)
    elif opt_type == "prodigy":
        try:
            import prodigyopt

            print(
                f"Usando otimizador adaptativo Prodigy para {len(trainable_params)} tensores treináveis.",
                flush=True,
            )
            return prodigyopt.Prodigy(trainable_params, lr=lr or 1.0)
        except Exception as e:
            print(
                f"[WARN] Prodigy não instalado ({e}), fallback para AdamW.",
                flush=True,
            )
            return torch.optim.AdamW(trainable_params, lr=lr)
    else:
        print(
            f"Usando otimizador AdamW (PyTorch) para {len(trainable_params)} tensores treináveis.",
            flush=True,
        )
        return torch.optim.AdamW(trainable_params, lr=lr)

LR_RESUME_MODES = ("continue", "restart")


def _parse_lr_resume_mode(lora_cfg: dict[str, Any]) -> str:
    """Lê `lora.lr_resume_mode` (padrão `continue`); valor inválido encerra via `_die`."""
    from trainer_difusao.common import _die

    raw = lora_cfg.get("lr_resume_mode")
    if raw is None or raw == "":
        return "continue"
    mode = str(raw).strip().lower() if isinstance(raw, str) else ""
    if mode not in LR_RESUME_MODES:
        _die(
            f"lora.lr_resume_mode inválido: {raw!r}. Valores aceitos: "
            f"{', '.join(LR_RESUME_MODES)}."
        )
    return mode


def _create_lr_scheduler(
    optimizer: Any,
    scheduler_name: str,
    total_steps: int,
    warmup_steps: int = 0,
) -> Any:
    """Cria scheduler de taxa de aprendizado via diffusers, partindo do passo 0.

    O LR de pico é o `initial_lr`/`lr` dos param_groups no momento da criação.
    """
    try:
        from diffusers.optimization import get_scheduler

        return get_scheduler(
            scheduler_name.lower().strip() or "cosine",
            optimizer=optimizer,
            num_warmup_steps=warmup_steps,
            num_training_steps=max(1, total_steps),
        )
    except Exception as e:
        print(
            f"[WARN] Não foi possível instanciar scheduler '{scheduler_name}': {e}",
            flush=True,
        )
        return None


def _build_optimizer_and_scheduler(
    module: Any,
    *,
    optimizer_name: str,
    learning_rate: float,
    optimizer_state_path: Any,
    lr_scheduler_name: str,
    lr_warmup_steps: int,
    steps_per_epoch: int,
    epochs: int,
    epoch_offset: int,
    lr_resume_mode: str,
) -> tuple[Any, Any]:
    """Cria optimizer + scheduler decidindo (modo, LR inicial) da retomada.

    Único ponto de decisão para Qwen e loop compartilhado. A curva sempre cobre só
    as épocas desta execução (`steps_per_epoch * epochs`), do passo 0, com o warmup
    da requisição. Em retomada (`epoch_offset > 0`):
    - `continue`: o LR de pico é o `lr` do optimizer restaurado (lido antes de
      qualquer override); sem optimizer state restaurado cai em `restart` com aviso.
    - `restart`: o LR de pico é o da requisição.
    Em treino novo o modo é ignorado e vale o LR da requisição.
    """
    from trainer_difusao.common import _load_optimizer_state, _override_optimizer_lr

    optimizer = _create_optimizer(module, optimizer_name, learning_rate)
    restored = bool(optimizer_state_path) and _load_optimizer_state(
        optimizer, optimizer_state_path
    )
    resuming = epoch_offset > 0
    peak_lr = learning_rate
    mode = lr_resume_mode
    if resuming and mode == "continue":
        if restored:
            peak_lr = float(optimizer.param_groups[0]["lr"])
        else:
            mode = "restart"
            print(
                "[WARN] Retomada com lr_resume_mode=continue, mas o estado do optimizer "
                "não foi restaurado (checkpoint só com pesos): usando modo restart, "
                f"LR da requisição {learning_rate:.3g}.",
                flush=True,
            )
    # Sempre normaliza lr/initial_lr: o state restaurado traz o initial_lr da curva antiga.
    _override_optimizer_lr(optimizer, peak_lr)
    scheduler = _create_lr_scheduler(
        optimizer,
        lr_scheduler_name,
        max(1, steps_per_epoch * epochs),
        lr_warmup_steps,
    )
    if resuming:
        origin = "checkpoint" if mode == "continue" else "requisição"
        verb = "continua" if mode == "continue" else "reinicia"
        print(
            f"Retomada: LR {verb} de {peak_lr:.3g} ({origin}), curva "
            f"{lr_scheduler_name} em {epochs} épocas (modo {mode}).",
            flush=True,
        )
    else:
        print(
            f"Treino novo: LR {peak_lr:.3g}, curva {lr_scheduler_name} em {epochs} épocas.",
            flush=True,
        )
    return optimizer, scheduler
