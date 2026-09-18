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

def _create_lr_scheduler(
    optimizer: Any,
    scheduler_name: str,
    total_steps: int,
    warmup_steps: int = 0,
) -> Any:
    """Cria scheduler de taxa de aprendizado via diffusers ou torch."""
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
