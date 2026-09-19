"""
Geração determinística de legendas mock (estilos default, Florence-2 e Qwen2-VL).
"""
from __future__ import annotations

import hashlib

MOCK_DESCRIPTIONS = [
    "Uma foto em close de uma placa de circuito impresso com solda fria e componentes SMD.",
    "Placa de circuito verde com conectores banhados a ouro e trilhas metálicas visíveis.",
    "Vista superior de placa de circuito eletrônico com capacitores e resistores alinhados.",
    "Foto detalhada de circuito impresso destacando pontos de solda e microcontrolador central.",
    "Componentes eletrônicos montados em superfície com solda de precisão sob luz de bancada.",
    "Macro de conexões eletrônicas em substrato cerâmico com filamentos condutores expostos.",
    "Detalhe de placa controladora com barramentos de cobre e conectores de alta densidade.",
    "Placa de circuito industrial exibindo pontos de teste e marcações serigráficas nítidas.",
]


def _generate_caption_mock(seed: int, filename: str, prompt: str | None) -> str:
    """Gera legenda determinística a partir de (seed, filename, prompt)."""
    h_input = f"{seed}:{filename}"
    h = int(hashlib.sha256(h_input.encode("utf-8")).hexdigest(), 16)
    base = MOCK_DESCRIPTIONS[h % len(MOCK_DESCRIPTIONS)]

    if prompt and prompt.strip():
        return f"{prompt.strip()} — {base}"
    return base


def _generate_caption_florence(seed: int, filename: str, prompt: str | None) -> str:
    """Gera legenda no estilo dense captioning da arquitetura Florence-2."""
    h_input = f"florence:{seed}:{filename}"
    h = int(hashlib.sha256(h_input.encode("utf-8")).hexdigest(), 16)
    base = MOCK_DESCRIPTIONS[h % len(MOCK_DESCRIPTIONS)]

    prefix = (
        prompt.strip()
        if (prompt and prompt.strip())
        else "A detailed high-resolution photograph"
    )
    return f"{prefix} showing {base.lower().rstrip('.')} with sharp contours and balanced lighting."


def _generate_caption_qwen(seed: int, filename: str, prompt: str | None) -> str:
    """Gera legenda analítica detalhada no estilo Qwen2-VL."""
    h_input = f"qwen:{seed}:{filename}"
    h = int(hashlib.sha256(h_input.encode("utf-8")).hexdigest(), 16)
    base = MOCK_DESCRIPTIONS[h % len(MOCK_DESCRIPTIONS)]

    user_instruction = f" ({prompt.strip()})" if (prompt and prompt.strip()) else ""
    return f"This image exhibits {base.lower().rstrip('.')}{user_instruction}. The subject displays consistent geometry, technical textures and high visual definition."


def _generate_caption(seed: int, filename: str, prompt: str | None) -> str:
    """Alias retrocompatível para _generate_caption_mock."""
    return _generate_caption_mock(seed, filename, prompt)
