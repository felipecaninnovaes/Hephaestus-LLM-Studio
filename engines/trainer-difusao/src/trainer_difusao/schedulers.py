"""Mapeamento sampler -> scheduler diffusers (fatia flux2-motor-treino).

O scheduler NUNCA entra na spec do daemon nem no `pipeline_cache_key`
(nao altera pesos): a aplicacao e sempre fresh-instance por request, via
`cls.from_config(base_config)` a partir do scheduler ORIGINAL do pipeline,
com restore garantido no finally (`swapped_scheduler`).

SD (sdxl/sd15): euler, euler_a, heun, dpmpp_2m[_karras], dpmpp_2m_sde[_karras],
dpmpp_sde, ddim.
Flux (flow-match): apenas default, euler, heun (restricao validada em
`load_and_validate_generate_config`).

Nota: os schedulers SDE (`dpmpp_2m_sde[_karras]` e `dpmpp_sde`) exigem
`torchsde` instalado no ambiente @gpu; sem ele, o diffusers levanta
ImportError honesto no request (job falha, nunca silencioso).
"""

from __future__ import annotations

import contextlib
from typing import Any

SAMPLER_CHOICES = (
    "default",
    "euler",
    "euler_a",
    "heun",
    "dpmpp_2m",
    "dpmpp_2m_karras",
    "dpmpp_2m_sde",
    "dpmpp_2m_sde_karras",
    "dpmpp_sde",
    "ddim",
)

# Flux usa flow-match: so estes samplers tem scheduler FlowMatch dedicado.
FLUX_SAMPLER_CHOICES = ("default", "euler", "heun")


def build_scheduler(
    sampler: str, arch: str, base_config: dict[str, Any]
) -> object | None:
    """Constroi um scheduler fresh a partir da config do scheduler original.

    `sampler="default"` (ou None) -> None (mantem o scheduler nativo).
    `arch`: "flux" (flow-match) ou "sd" (sdxl/sd15/custom).
    Instancia SEMPRE com `cls.from_config(base_config)`.

    Levanta ValueError p/ sampler/arch desconhecido (caller converte p/ _die)
    e ImportError se o diffusers nao estiver instalado.
    """
    if sampler is None or sampler == "default":
        return None
    if arch == "flux":
        if sampler == "euler":
            from diffusers import FlowMatchEulerDiscreteScheduler

            return FlowMatchEulerDiscreteScheduler.from_config(base_config)
        if sampler == "heun":
            from diffusers import FlowMatchHeunDiscreteScheduler

            return FlowMatchHeunDiscreteScheduler.from_config(base_config)
        raise ValueError(
            f"Sampler '{sampler}' incompativel com FLUX.2 (flow-match). "
            f"Use: {', '.join(FLUX_SAMPLER_CHOICES)}."
        )
    if arch == "sd":
        if sampler == "euler":
            from diffusers import EulerDiscreteScheduler

            return EulerDiscreteScheduler.from_config(base_config)
        if sampler == "euler_a":
            from diffusers import EulerAncestralDiscreteScheduler

            return EulerAncestralDiscreteScheduler.from_config(base_config)
        if sampler == "heun":
            from diffusers import HeunDiscreteScheduler

            return HeunDiscreteScheduler.from_config(base_config)
        if sampler == "dpmpp_2m":
            from diffusers import DPMSolverMultistepScheduler

            return DPMSolverMultistepScheduler.from_config(
                base_config, solver_order=2, algorithm_type="dpmsolver++"
            )
        if sampler == "dpmpp_2m_karras":
            from diffusers import DPMSolverMultistepScheduler

            return DPMSolverMultistepScheduler.from_config(
                base_config,
                solver_order=2,
                algorithm_type="dpmsolver++",
                use_karras_sigmas=True,
            )
        if sampler == "dpmpp_2m_sde":
            from diffusers import DPMSolverMultistepScheduler

            return DPMSolverMultistepScheduler.from_config(
                base_config, solver_order=2, algorithm_type="sde-dpmsolver++"
            )
        if sampler == "dpmpp_2m_sde_karras":
            from diffusers import DPMSolverMultistepScheduler

            return DPMSolverMultistepScheduler.from_config(
                base_config,
                solver_order=2,
                algorithm_type="sde-dpmsolver++",
                use_karras_sigmas=True,
            )
        if sampler == "dpmpp_sde":
            from diffusers import DPMSolverSDEScheduler

            return DPMSolverSDEScheduler.from_config(base_config)
        if sampler == "ddim":
            from diffusers import DDIMScheduler

            return DDIMScheduler.from_config(base_config)
        raise ValueError(
            f"Sampler desconhecido: '{sampler}'. Use: {', '.join(SAMPLER_CHOICES)}."
        )
    raise ValueError(f"arch de scheduler desconhecida: '{arch}' (use 'flux' ou 'sd').")


@contextlib.contextmanager
def swapped_scheduler(pipe: object, fresh: object | None):
    """Troca `pipe.scheduler` por `fresh` com restore garantido no finally.

    `fresh=None` -> no-op. Cobre excecao/cancel: o pipeline vem do cache do
    daemon e e reusado entre requests — sem o restore, a spec vaza para o
    proximo request (contaminacao). A variante img2img (`pipe.components`)
    compartilha o MESMO objeto scheduler, entao restaurar o `call_pipe`
    restaura o cacheado tambem.
    """
    if fresh is None:
        yield None
        return
    orig = pipe.scheduler
    pipe.scheduler = fresh
    try:
        yield fresh
    finally:
        pipe.scheduler = orig
