"""Quantizacao para geracao: BitsAndBytes (4/8bit) + TorchAO (2/6bit).

`pipeline_cache_key` JA inclui quantization — nada a mudar aqui; 6bit gera
chave distinta de 4bit automaticamente (string diferente na tupla).
"""

from __future__ import annotations

import sys
from typing import Any

QUANT_LEVELS = ("none", "2bit", "4bit", "6bit", "8bit")


def _die(msg: str) -> None:
    print(f"ERROR: {msg}", file=sys.stderr)
    sys.exit(1)


def build_torchao_config(quant: str) -> Any:
    """Constroi TorchAoConfig para 2bit/6bit (intX weight-only).

    Usa `IntxWeightOnlyConfig(weight_dtype=torch.int2/int6)` da versao
    instalada do torchao (verificado contra torchao 0.18 + torch 2.14).
    Qualquer indisponibilidade (torchao ausente, dtype nao suportado,
    assinatura divergente) -> _die honesto: o job falha, nunca degrade
    silenciosamente para outro nivel.
    """
    if quant not in ("2bit", "6bit"):
        _die(f"build_torchao_config so atende '2bit'/'6bit' (recebido: {quant}).")
    try:
        import torch
        from transformers import TorchAoConfig
    except ImportError as e:
        _die(
            f"Quantizacao {quant} exige 'torchao' + 'transformers' com TorchAoConfig "
            f"(nao instalados ou incompativeis): {e}"
        )
    try:
        from torchao.quantization import IntxWeightOnlyConfig
    except ImportError as e:
        _die(f"Quantizacao {quant} exige 'torchao' instalado (extra train): {e}")
    want = "int2" if quant == "2bit" else "int6"
    dtype = getattr(torch, want, None)
    if dtype is None:
        _die(
            f"Quantizacao {quant} exige torch com suporte a torch.{want} "
            f"(torch instalada: {torch.__version__})."
        )
    try:
        return TorchAoConfig(IntxWeightOnlyConfig(weight_dtype=dtype))
    except Exception as e:  # noqa: BLE001 — assinatura do torchao varia por versao
        _die(
            f"Falha ao montar TorchAoConfig para {quant} "
            f"(torchao incompatível com esta versao): {e}"
        )
