"""Mapeamento único da família FLUX.2 Klein: arch → (repo base, repo destilado, env override).

O arch vindo do config decide o repo; as envs são overrides POR variante
(`FLUX_MODEL_ID`/`FLUX_DISTILLED_MODEL_ID` valem só p/ 4B, `FLUX_9B_MODEL_ID` só p/ 9B),
de modo que 4B e 9B coexistem no mesmo nó. Módulo sem dependências pesadas.
"""
from __future__ import annotations

import os
from dataclasses import dataclass

KLEIN_4B = "flux-2-klein-4b"
KLEIN_9B = "flux-2-klein-9b"


@dataclass(frozen=True)
class KleinVariant:
    arch: str
    base_repo: str
    distilled_repo: str | None
    base_env: str
    distilled_env: str | None
    aliases: tuple[str, ...]


KLEIN_VARIANTS: dict[str, KleinVariant] = {
    KLEIN_4B: KleinVariant(
        arch=KLEIN_4B,
        base_repo="black-forest-labs/FLUX.2-klein-base-4B",
        distilled_repo="black-forest-labs/FLUX.2-klein-4B",
        base_env="FLUX_MODEL_ID",
        distilled_env="FLUX_DISTILLED_MODEL_ID",
        # "flux"/"flux2"/"flux-2": aliases legados que sempre significaram 4B.
        aliases=("flux", "flux2", "flux-2", "flux2-klein-4b", "flux.2-klein-4b"),
    ),
    KLEIN_9B: KleinVariant(
        arch=KLEIN_9B,
        base_repo="black-forest-labs/FLUX.2-klein-base-9B",
        distilled_repo=None,
        base_env="FLUX_9B_MODEL_ID",
        distilled_env=None,
        aliases=("flux2-klein-9b", "flux.2-klein-9b"),
    ),
}

KLEIN_ARCHS: tuple[str, ...] = tuple(KLEIN_VARIANTS)

_ALIAS_TO_ARCH: dict[str, str] = {
    alias: v.arch for v in KLEIN_VARIANTS.values() for alias in (v.arch, *v.aliases)
}


def canonical_klein_arch(raw: object) -> str | None:
    """Arch canônico da família Klein p/ `raw` (arch ou alias), ou None se não for Klein."""
    if not isinstance(raw, str):
        return None
    return _ALIAS_TO_ARCH.get(raw.strip().lower())


def is_klein_arch(arch: object) -> bool:
    return isinstance(arch, str) and arch in KLEIN_VARIANTS


def klein_repo_id(arch: str, *, distilled: bool = False) -> str:
    """Repo HF da variante `arch` (env da variante > default oficial).

    ValueError se o arch não é Klein ou se `distilled` não existe p/ a variante.
    """
    variant = KLEIN_VARIANTS.get(arch)
    if variant is None:
        raise ValueError(f"arch não pertence à família FLUX.2 Klein: {arch!r}")
    if distilled:
        if variant.distilled_repo is None or variant.distilled_env is None:
            raise ValueError(f"variante destilada não existe para {arch}")
        return os.environ.get(variant.distilled_env) or variant.distilled_repo
    return os.environ.get(variant.base_env) or variant.base_repo


def klein_9b_unsupported_option(
    arch: object,
    *,
    custom_checkpoint_path: object = None,
    text_encoder_path: object = None,
    distilled: bool = False,
) -> str | None:
    """Mensagem de erro se `arch` é 9B e usa opção exclusiva do 4B; None caso contrário."""
    if arch != KLEIN_9B:
        return None
    if distilled:
        return f"variante destilada (distilled=true) não existe para {KLEIN_9B}; use o modelo base."
    if custom_checkpoint_path:
        return (
            f"custom_checkpoint_path não é suportado com arch {KLEIN_9B} "
            f"(checkpoint custom é exclusivo do {KLEIN_4B}): {custom_checkpoint_path}"
        )
    if text_encoder_path:
        return (
            f"text_encoder_path não é suportado com arch {KLEIN_9B} "
            f"(encoder custom é exclusivo do {KLEIN_4B}): {text_encoder_path}"
        )
    return None

__all__ = [
    "KLEIN_4B",
    "KLEIN_9B",
    "KLEIN_ARCHS",
    "KLEIN_VARIANTS",
    "KleinVariant",
    "canonical_klein_arch",
    "is_klein_arch",
    "klein_9b_unsupported_option",
    "klein_repo_id",
]
