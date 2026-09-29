"""Regressão: chaves salvas não devem conter prefixo PEFT ('base_model.model.')."""

import tempfile
import unittest
from pathlib import Path

try:
    import torch
    import torch.nn as nn
    from peft import LoraConfig, get_peft_model
    from safetensors.torch import load_file
    HAS_TORCH_PEFT = True
except ImportError:
    HAS_TORCH_PEFT = False


@unittest.skipUnless(HAS_TORCH_PEFT, "requer torch, peft e safetensors")
class TestSaveLoraKeys(unittest.TestCase):
    def test_saved_keys_are_canonical_without_peft_prefix(self):
        from peft import get_peft_model_state_dict

        from trainer_difusao.common_pkg.lora_io import _save_lora_safetensors

        model = get_peft_model(
            nn.Sequential(nn.Linear(8, 8), nn.Linear(8, 8)),
            LoraConfig(r=4, lora_alpha=4, target_modules=["0", "1"]),
        )
        # Pré-condição: o PEFT emite o prefixo — valida que a normalização é necessária.
        raw_keys = list(get_peft_model_state_dict(model))
        self.assertTrue(any(k.startswith("base_model.model.") for k in raw_keys))

        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "adapter.safetensors"
            _save_lora_safetensors(model, out, {"format": "pt"})

            saved = load_file(str(out))
            self.assertTrue(saved)
            for key in saved:
                self.assertFalse(
                    key.startswith("base_model.model."),
                    f"chave com prefixo PEFT vazou no arquivo: {key}",
                )

    def test_file_written_atomically_without_tmp_leftover(self):
        from trainer_difusao.common_pkg.lora_io import _save_lora_safetensors

        model = get_peft_model(
            nn.Sequential(nn.Linear(8, 8)),
            LoraConfig(r=4, lora_alpha=4, target_modules=["0"]),
        )
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "adapter.safetensors"
            _save_lora_safetensors(model, out, {"format": "pt"})
            self.assertTrue(out.exists())
            self.assertEqual(list(Path(tmp).glob(".tmp_*")), [])


@unittest.skipUnless(HAS_TORCH_PEFT, "requer torch, peft e safetensors")
class TestOptimizerState(unittest.TestCase):
    def _make_model_and_optimizer(self):
        model = get_peft_model(
            nn.Sequential(nn.Linear(8, 8)),
            LoraConfig(r=4, lora_alpha=4, target_modules=["0"]),
        )
        optimizer = torch.optim.Adam(model.parameters(), lr=1e-3)
        return model, optimizer

    def test_save_load_roundtrip_preserves_momentum(self):
        from trainer_difusao.common_pkg.lora_io import (
            _load_optimizer_state,
            _save_optimizer_state,
        )

        model, optimizer = self._make_model_and_optimizer()
        # Executa dois passos de treino para popular o estado interno do Adam.
        # LoRA inicializa lora_B em zero, então o gradiente de lora_A é zero no
        # primeiro passo (dOutput/dA depende de B); dois passos garantem momentum
        # não-zero em pelo menos um param group.
        for _ in range(2):
            optimizer.zero_grad()
            loss = model(torch.randn(2, 8)).sum()
            loss.backward()
            optimizer.step()

        # Confirma pré-condição: ao menos um tensor de momentum não é zero.
        state_before = optimizer.state_dict()["state"]
        self.assertTrue(state_before)
        nonzero_key = next(
            k for k, v in state_before.items() if torch.any(v["exp_avg"] != 0)
        )
        first_exp_avg = state_before[nonzero_key]["exp_avg"]
        self.assertTrue(torch.any(first_exp_avg != 0))

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "adapter_optimizer.pt"
            _save_optimizer_state(optimizer, path)
            self.assertTrue(path.exists())
            self.assertEqual(list(Path(tmp).glob(".tmp_*")), [])

            # Novo optimizer "fresh" (sem momentum) recebendo o estado salvo.
            fresh_model, fresh_optimizer = self._make_model_and_optimizer()
            ok = _load_optimizer_state(fresh_optimizer, path)
            self.assertTrue(ok)

            restored_state = fresh_optimizer.state_dict()["state"]
            self.assertTrue(restored_state)
            restored_exp_avg = restored_state[nonzero_key]["exp_avg"]
            self.assertTrue(torch.equal(restored_exp_avg, first_exp_avg))

    def test_load_missing_file_returns_false_without_raising(self):
        from trainer_difusao.common_pkg.lora_io import _load_optimizer_state

        _, optimizer = self._make_model_and_optimizer()
        ok = _load_optimizer_state(optimizer, "/nonexistent/path/optimizer.pt")
        self.assertFalse(ok)

    def test_load_incompatible_state_dict_returns_false_without_raising(self):
        from trainer_difusao.common_pkg.lora_io import (
            _load_optimizer_state,
            _save_optimizer_state,
        )

        # Optimizer de origem com dois grupos de parâmetros distintos.
        model_a = nn.Sequential(nn.Linear(8, 8), nn.Linear(8, 8))
        optimizer_a = torch.optim.Adam(
            [
                {"params": model_a[0].parameters()},
                {"params": model_a[1].parameters()},
            ],
            lr=1e-3,
        )
        loss = model_a(torch.randn(2, 8)).sum()
        loss.backward()
        optimizer_a.step()

        # Optimizer de destino com um único grupo de parâmetros (rank/módulos diferentes).
        model_b = nn.Linear(8, 8)
        optimizer_b = torch.optim.Adam(model_b.parameters(), lr=1e-3)

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "optimizer.pt"
            _save_optimizer_state(optimizer_a, path)
            ok = _load_optimizer_state(optimizer_b, path)
            self.assertFalse(ok)

    def test_save_adapter_checkpoint_without_optimizer_writes_no_pt_file(self):
        from trainer_difusao.common_pkg.lora_io import save_adapter_checkpoint

        model = get_peft_model(
            nn.Sequential(nn.Linear(8, 8)),
            LoraConfig(r=4, lora_alpha=4, target_modules=["0"]),
        )
        with tempfile.TemporaryDirectory() as tmp:
            ckpt_dir = Path(tmp) / "checkpoints"
            save_adapter_checkpoint(
                model, ckpt_dir, "adapter", 1, metadata={"format": "pt"}
            )
            pt_files = list(ckpt_dir.glob("*_optimizer.pt"))
            self.assertEqual(pt_files, [], "checkpoint sem optimizer não deve gerar .pt")

    def test_save_adapter_checkpoint_with_optimizer_writes_pt_file(self):
        from trainer_difusao.common_pkg.lora_io import save_adapter_checkpoint

        model, optimizer = self._make_model_and_optimizer()
        with tempfile.TemporaryDirectory() as tmp:
            ckpt_dir = Path(tmp) / "checkpoints"
            save_adapter_checkpoint(
                model, ckpt_dir, "adapter", 1, metadata={"format": "pt"}, optimizer=optimizer
            )
            expected = ckpt_dir / "adapter_epoch_001_optimizer.pt"
            self.assertTrue(expected.exists())

    def test_save_final_adapter_with_optimizer_copies_canonical_pt(self):
        from trainer_difusao.common_pkg.lora_io import save_final_adapter

        model, optimizer = self._make_model_and_optimizer()
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = Path(tmp) / "output"
            save_final_adapter(
                model, out_dir, "custom_lora", metadata={"format": "pt"}, optimizer=optimizer
            )
            self.assertTrue((out_dir / "custom_lora_optimizer.pt").exists())
            self.assertTrue((out_dir / "adapter_optimizer.pt").exists())


if __name__ == "__main__":
    unittest.main()
