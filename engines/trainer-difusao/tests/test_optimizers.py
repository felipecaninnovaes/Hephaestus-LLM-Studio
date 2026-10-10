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

from trainer_difusao.optimizers import (
    _build_optimizer_and_scheduler,
    _create_lr_scheduler,
    _create_optimizer,
    _parse_lr_resume_mode,
)


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


SPE, PEAK = 124, 8e-5  # passos/época e LR dos jobs reais be5bb333 -> 1413c181


def _lrs(opt, sched, steps):
    out = []
    for _ in range(steps):
        out.append(opt.param_groups[0]["lr"])
        opt.step()
        sched.step()
    return out


def _module():
    import torch

    return torch.nn.Linear(4, 2)


def _build(mod, **kw):
    args = dict(
        optimizer_name="adamw",
        learning_rate=PEAK,
        optimizer_state_path=None,
        lr_scheduler_name="cosine",
        lr_warmup_steps=0,
        steps_per_epoch=SPE,
        epochs=6,
        epoch_offset=4,
        lr_resume_mode="continue",
    )
    args.update(kw)
    return _build_optimizer_and_scheduler(mod, **args)


@pytest.fixture
def first_run(tmp_path):
    """1º job: 5 épocas cosine, parado após a 4ª; salva o optimizer."""
    pytest.importorskip("torch")
    pytest.importorskip("diffusers")
    import torch

    mod = _module()
    opt, sched = _build(mod, epochs=5, epoch_offset=0)
    lrs = _lrs(opt, sched, SPE * 4)
    path = tmp_path / "opt.pt"
    torch.save(opt.state_dict(), path)
    return path, lrs, opt.param_groups[0]["lr"]


def test_first_run_saved_lr_is_cosine_tail(first_run):
    import math

    _, _, saved = first_run
    assert saved == pytest.approx(PEAK / 2 * (1 + math.cos(math.pi * 4 / 5)), rel=1e-6)


def test_resume_continue_starts_at_saved_lr_and_decays_monotonically(first_run, capsys):
    path, _, saved = first_run
    opt, sched = _build(_module(), optimizer_state_path=path)
    lrs = _lrs(opt, sched, SPE * 6)
    assert lrs[0] == saved
    assert all(b <= a for a, b in zip(lrs, lrs[1:])), "LR subiu em algum passo"
    assert lrs[-1] < saved * 1e-4  # ~0 no último passo
    assert f"Retomada: LR continua de {saved:.3g} (checkpoint), curva cosine em 6 épocas" in capsys.readouterr().out
    print("continue: passo0=%.3e passo1=%.3e meio=%.3e ultimo=%.3e" % (lrs[0], lrs[1], lrs[SPE * 3], lrs[-1]))


def test_resume_restart_uses_request_lr_with_warmup(first_run):
    path, _, _ = first_run
    opt, sched = _build(_module(), optimizer_state_path=path, lr_resume_mode="restart", lr_warmup_steps=10)
    lrs = _lrs(opt, sched, SPE * 6)
    assert lrs[0] == 0.0
    assert lrs[10] == pytest.approx(PEAK)
    assert max(lrs) == pytest.approx(PEAK)
    assert lrs[-1] < PEAK * 1e-4
    opt2, sched2 = _build(_module(), optimizer_state_path=path, lr_resume_mode="restart")
    assert opt2.param_groups[0]["lr"] == PEAK


def test_resume_continue_without_optimizer_state_falls_back_to_restart(tmp_path, capsys):
    pytest.importorskip("torch")
    pytest.importorskip("diffusers")
    for path in (None, tmp_path / "ausente.pt"):
        opt, sched = _build(_module(), optimizer_state_path=path)
        assert opt.param_groups[0]["lr"] == PEAK
        out = capsys.readouterr().out
        assert "usando modo restart" in out
        assert "modo restart" in out


def test_new_training_ignores_mode_and_uses_request_lr(first_run):
    path, full_first, _ = first_run
    for mode in ("continue", "restart"):
        opt, sched = _build(_module(), epochs=5, epoch_offset=0, lr_resume_mode=mode)
        assert _lrs(opt, sched, SPE * 4) == full_first


def test_numeric_proof_three_scenarios(first_run):
    path, first, saved = first_run
    cont_o, cont_s = _build(_module(), optimizer_state_path=path)
    cont = _lrs(cont_o, cont_s, SPE * 6)
    rest_o, rest_s = _build(_module(), optimizer_state_path=path, lr_resume_mode="restart")
    rest = _lrs(rest_o, rest_s, SPE * 6)
    nofile_o, nofile_s = _build(_module(), optimizer_state_path=None)
    nofile = _lrs(nofile_o, nofile_s, SPE * 6)
    def show(name, l):
        idx = [0, 1, 123, 124, 371, 500, 743]
        print(name, " ".join("p%d=%.3e" % (i, l[i]) for i in idx))
    print("1o job (5 epocas), LR salvo apos a 4a epoca = %.3e" % saved)
    show("continue      ", cont)
    show("restart       ", rest)
    show("sem opt.state ", nofile)
    assert cont[0] == saved
    assert rest == nofile


def test_numeric_proof_with_real_job_checkpoint_lr(tmp_path, capsys):
    """Checkpoint com o LR gravado no job real be5bb333 (1.07e-5): a retomada começa nele, não em 5.23e-5."""
    pytest.importorskip("torch")
    pytest.importorskip("diffusers")
    import torch

    mod = _module()
    opt, _ = _build(mod, epochs=5, epoch_offset=0)
    for g in opt.param_groups:
        g["lr"] = 1.07e-5
    path = tmp_path / "real.pt"
    torch.save(opt.state_dict(), path)
    o, s = _build(_module(), optimizer_state_path=path)
    lrs = _lrs(o, s, SPE * 6)
    old = PEAK / 2 * (1 + __import__("math").cos(__import__("math").pi * 4 / 10))
    print("job real: passo0 retomada=%.3e (comportamento antigo: %.3e) ultimo=%.3e" % (lrs[0], old, lrs[-1]))
    assert lrs[0] == pytest.approx(1.07e-5)
    assert all(b <= a for a, b in zip(lrs, lrs[1:]))


def test_parse_lr_resume_mode():
    assert _parse_lr_resume_mode({}) == "continue"
    assert _parse_lr_resume_mode({"lr_resume_mode": "Restart"}) == "restart"
    with patch("trainer_difusao.common._die", side_effect=SystemExit(2)):
        for bad in ("foo", 3):
            with pytest.raises(SystemExit):
                _parse_lr_resume_mode({"lr_resume_mode": bad})
