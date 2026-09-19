"""Testes unitários para fábrica de otimizadores (optimizers.py)."""

import sys
from unittest.mock import MagicMock, patch
import pytest

try:
    import torch
    import torch.nn as nn
    HAS_TORCH = True
except ImportError:
    HAS_TORCH = False
    torch = MagicMock()  # type: ignore
    nn = MagicMock()  # type: ignore

from trainer_difusao.optimizers import _create_optimizer, _create_lr_scheduler


class DummyParam:
    def __init__(self, requires_grad: bool = True):
        self.requires_grad = requires_grad


class DummyUnet:
    def __init__(self, requires_grad: bool = True):
        self._params = [DummyParam(requires_grad)]

    def parameters(self):
        return self._params


def test_create_optimizer_no_trainable_params():
    unet = DummyUnet(requires_grad=False)
    with pytest.raises(ValueError, match="Nenhum parâmetro com requires_grad=True"):
        _create_optimizer(unet, "adamw", lr=1e-4)


@pytest.mark.skipif(not HAS_TORCH, reason="Requer PyTorch real instalado")
def test_create_optimizer_adamw_default_real_torch():
    unet = DummyUnet(requires_grad=True)
    param = torch.nn.Parameter(torch.zeros(1))
    unet._params = [param]
    opt = _create_optimizer(unet, "adamw", lr=1e-4)
    assert isinstance(opt, torch.optim.AdamW)
    assert opt.defaults["lr"] == 1e-4


def test_create_optimizer_adamw_mocked():
    unet = DummyUnet(requires_grad=True)
    mock_torch = MagicMock()
    with patch.dict(sys.modules, {"torch": mock_torch}):
        _create_optimizer(unet, "adamw", lr=1e-4)
        mock_torch.optim.AdamW.assert_called_once()


def test_create_optimizer_unknown_falls_back_to_adamw():
    unet = DummyUnet(requires_grad=True)
    mock_torch = MagicMock()
    with patch.dict(sys.modules, {"torch": mock_torch}):
        _create_optimizer(unet, "unknown_optimizer", lr=2e-4)
        mock_torch.optim.AdamW.assert_called_once()


def test_create_optimizer_paged_adamw8bit_cpu_fallback():
    unet = DummyUnet(requires_grad=True)
    mock_torch = MagicMock()
    mock_torch.cuda.is_available.return_value = False
    with patch.dict(sys.modules, {"torch": mock_torch, "bitsandbytes": None}):
        _create_optimizer(unet, "paged_adamw8bit", lr=1e-4)
        mock_torch.optim.AdamW.assert_called_once()

    mock_torch.reset_mock()
    with patch.dict(sys.modules, {"torch": mock_torch, "bitsandbytes": None}):
        _create_optimizer(unet, "paged_adamw_8bit", lr=1e-4)
        mock_torch.optim.AdamW.assert_called_once()


def test_create_optimizer_paged_adamw8bit_cuda_fail_fast():
    unet = DummyUnet(requires_grad=True)
    mock_torch = MagicMock()
    mock_torch.cuda.is_available.return_value = True
    with patch.dict(sys.modules, {"torch": mock_torch, "bitsandbytes": None}):
        with pytest.raises(SystemExit):
            _create_optimizer(unet, "paged_adamw8bit", lr=1e-4)


def test_create_optimizer_paged_adamw32bit_cpu_fallback():
    unet = DummyUnet(requires_grad=True)
    mock_torch = MagicMock()
    mock_torch.cuda.is_available.return_value = False
    with patch.dict(sys.modules, {"torch": mock_torch, "bitsandbytes": None}):
        _create_optimizer(unet, "paged_adamw32bit", lr=1e-4)
        mock_torch.optim.AdamW.assert_called_once()

    mock_torch.reset_mock()
    with patch.dict(sys.modules, {"torch": mock_torch, "bitsandbytes": None}):
        _create_optimizer(unet, "paged_adamw_32bit", lr=1e-4)
        mock_torch.optim.AdamW.assert_called_once()


def test_create_optimizer_paged_adamw32bit_cuda_fail_fast():
    unet = DummyUnet(requires_grad=True)
    mock_torch = MagicMock()
    mock_torch.cuda.is_available.return_value = True
    with patch.dict(sys.modules, {"torch": mock_torch, "bitsandbytes": None}):
        with pytest.raises(SystemExit):
            _create_optimizer(unet, "paged_adamw32bit", lr=1e-4)


def test_create_optimizer_paged_adamw_mocked_success():
    unet = DummyUnet(requires_grad=True)
    mock_torch = MagicMock()
    mock_bnb = MagicMock()
    mock_paged_8bit = MagicMock()
    mock_paged_32bit = MagicMock()
    mock_bnb.optim.PagedAdamW8bit = mock_paged_8bit
    mock_bnb.optim.PagedAdamW32bit = mock_paged_32bit

    with patch.dict(sys.modules, {"torch": mock_torch, "bitsandbytes": mock_bnb}):
        _create_optimizer(unet, "paged_adamw8bit", lr=1e-4)
        mock_paged_8bit.assert_called_once()

        _create_optimizer(unet, "paged_adamw32bit", lr=2e-4)
        mock_paged_32bit.assert_called_once()


def test_create_lr_scheduler_fallback():
    mock_opt = MagicMock()
    # Sem diffusers instalado
    with patch.dict(sys.modules, {"diffusers": None, "diffusers.optimization": None}):
        res = _create_lr_scheduler(mock_opt, "cosine", total_steps=100, warmup_steps=10)
        assert res is None
