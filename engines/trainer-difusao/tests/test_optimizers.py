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


@pytest.mark.parametrize("scheduler_name", ["cosine", "constant_with_warmup"])
def test_create_lr_scheduler_resume_continuity(scheduler_name):
    """Garante que a retomada com epoch_offset continua a cauda exata da curva original."""
    pytest.importorskip("torch")
    pytest.importorskip("diffusers")
    import torch

    steps_per_epoch = 7
    warmup_steps = 10
    offset_epochs = 6
    resume_epochs = 4
    total_epochs = offset_epochs + resume_epochs  # 10
    lr = 5e-5

    total_steps = steps_per_epoch * total_epochs  # 70
    offset_steps = steps_per_epoch * offset_epochs  # 42
    remaining_steps = steps_per_epoch * resume_epochs  # 28

    # 1. Execução ininterrupta de 10 épocas
    p_full = torch.nn.Parameter(torch.zeros(1))
    opt_full = torch.optim.AdamW([p_full], lr=lr)
    sched_full = _create_lr_scheduler(
        opt_full, scheduler_name, total_steps=total_steps, warmup_steps=warmup_steps, last_step=0
    )
    assert sched_full is not None

    full_lrs = []
    for _ in range(total_steps):
        full_lrs.append(opt_full.param_groups[0]["lr"])
        opt_full.step()
        sched_full.step()

    # 2. Execução retomada (offset=6, epochs=4)
    p_resumed = torch.nn.Parameter(torch.zeros(1))
    opt_resumed = torch.optim.AdamW([p_resumed], lr=lr)
    sched_resumed = _create_lr_scheduler(
        opt_resumed,
        scheduler_name,
        total_steps=total_steps,
        warmup_steps=warmup_steps,
        last_step=offset_steps,
    )
    assert sched_resumed is not None

    resumed_lrs = []
    for _ in range(remaining_steps):
        resumed_lrs.append(opt_resumed.param_groups[0]["lr"])
        opt_resumed.step()
        sched_resumed.step()

    # A sequência de LRs da retomada deve ser idêntica à cauda da ininterrupta
    tail_lrs = full_lrs[offset_steps:]
    assert len(tail_lrs) == len(resumed_lrs) == remaining_steps
    assert resumed_lrs == tail_lrs

    # 3. Caso offset=0 inalterado
    p_zero = torch.nn.Parameter(torch.zeros(1))
    opt_zero = torch.optim.AdamW([p_zero], lr=lr)
    sched_zero = _create_lr_scheduler(
        opt_zero, scheduler_name, total_steps=total_steps, warmup_steps=warmup_steps, last_step=0
    )
    assert sched_zero is not None
    zero_first_lr = opt_zero.param_groups[0]["lr"]
    assert zero_first_lr == full_lrs[0]
