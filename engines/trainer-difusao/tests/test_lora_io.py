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


@unittest.skipUnless(HAS_TORCH_PEFT, "requer torch, peft e safetensors")
class TestLoadLoraWeightsResume(unittest.TestCase):
    """Regressão: _load_lora_weights deve de fato restaurar os pesos salvos por
    _save_lora_safetensors num PeftModel NOVO (antes do fix, 100% das chaves
    voltavam em unexpected_keys e nada era carregado, silenciosamente)."""

    def _make_model(self, rank: int = 4):
        return get_peft_model(
            nn.Sequential(nn.Linear(8, 8), nn.Linear(8, 8)),
            LoraConfig(r=rank, lora_alpha=rank, target_modules=["0", "1"]),
        )

    def test_roundtrip_restores_identical_lora_tensors_on_fresh_model(self):
        from peft import get_peft_model_state_dict

        from trainer_difusao.common_pkg.lora_io import (
            _load_lora_weights,
            _save_lora_safetensors,
        )

        model = self._make_model()
        # lora_B inicializa em zero no PEFT; randomiza para que um load que não
        # faça nada (bug original) produza tensores DIFERENTES do arquivo salvo.
        with torch.no_grad():
            for name, param in model.named_parameters():
                if "lora_B" in name:
                    param.copy_(torch.randn_like(param))

        saved_sd = {k: v.clone() for k, v in get_peft_model_state_dict(model).items()}

        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "adapter.safetensors"
            _save_lora_safetensors(model, out, {"format": "pt"})

            fresh_model = self._make_model()
            _load_lora_weights(fresh_model, out)

            fresh_sd = get_peft_model_state_dict(fresh_model)
            self.assertEqual(set(fresh_sd.keys()), set(saved_sd.keys()))
            for key, tensor in saved_sd.items():
                self.assertTrue(
                    torch.equal(fresh_sd[key], tensor),
                    f"tensor '{key}' não foi restaurado corretamente",
                )

    def test_file_with_peft_prefix_also_loads(self):
        from peft import get_peft_model_state_dict

        from trainer_difusao.common_pkg.lora_io import _load_lora_weights
        import safetensors.torch

        model = self._make_model()
        with torch.no_grad():
            for name, param in model.named_parameters():
                if "lora_B" in name:
                    param.copy_(torch.randn_like(param))
        # Grava SEM normalizar (chaves ainda com o prefixo 'base_model.model.').
        raw_sd = get_peft_model_state_dict(model)

        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "adapter_prefixed.safetensors"
            safetensors.torch.save_file(dict(raw_sd), str(out), metadata={"format": "pt"})

            fresh_model = self._make_model()
            _load_lora_weights(fresh_model, out)

            fresh_sd = get_peft_model_state_dict(fresh_model)
            for key, tensor in raw_sd.items():
                self.assertTrue(torch.equal(fresh_sd[key], tensor))

    def test_missing_weights_path_file_dies(self):
        from trainer_difusao.common_pkg.lora_io import _load_lora_weights

        model = self._make_model()
        with self.assertRaises(SystemExit):
            _load_lora_weights(model, "/nonexistent/path/adapter.safetensors")

    def test_rank_mismatch_dies(self):
        from trainer_difusao.common_pkg.lora_io import (
            _load_lora_weights,
            _save_lora_safetensors,
        )

        source_model = self._make_model(rank=4)
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "adapter.safetensors"
            _save_lora_safetensors(source_model, out, {"format": "pt"})

            # Modelo de destino com rank diferente (8 != 4): shape de lora_A/lora_B diverge.
            target_model = self._make_model(rank=8)
            with self.assertRaises(SystemExit):
                _load_lora_weights(target_model, out)

    def test_unexpected_keys_in_file_dies(self):
        from trainer_difusao.common_pkg.lora_io import _load_lora_weights
        import safetensors.torch

        model = self._make_model()
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "adapter_bogus.safetensors"
            bogus_sd = {"nonexistent_module.lora_A.weight": torch.randn(4, 8)}
            safetensors.torch.save_file(bogus_sd, str(out), metadata={"format": "pt"})

            with self.assertRaises(SystemExit):
                _load_lora_weights(model, out)


@unittest.skipUnless(HAS_TORCH_PEFT, "requer torch, peft e safetensors")
class TestLoadOptimizerStateShapeMismatch(unittest.TestCase):
    def test_incompatible_tensor_shape_same_param_count_returns_false(self):
        from trainer_difusao.common_pkg.lora_io import (
            _load_optimizer_state,
            _save_optimizer_state,
        )

        model_a = nn.Linear(8, 8)
        optimizer_a = torch.optim.Adam(model_a.parameters(), lr=1e-3)
        loss = model_a(torch.randn(2, 8)).sum()
        loss.backward()
        optimizer_a.step()

        # Mesmo número de parâmetros/posições, mas shapes diferentes (ex.: rank
        # de LoRA diferente entre o treino original e a retomada).
        model_b = nn.Linear(4, 4)
        optimizer_b = torch.optim.Adam(model_b.parameters(), lr=1e-3)

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "optimizer.pt"
            _save_optimizer_state(optimizer_a, path)
            ok = _load_optimizer_state(optimizer_b, path)
            self.assertFalse(ok)
            # Optimizer novo não deve ter sido corrompido/alterado pelo load.
            self.assertEqual(optimizer_b.state_dict()["state"], {})

    def _make_bnb_style_state(self, optimizer, param_shapes):
        """Monta um state_dict sintético no formato bitsandbytes 8-bit
        (state1/state2 com o shape do PARÂMETRO; absmax1/absmax2/qmap1/qmap2
        com shape de CODEBOOK/BLOCO, propositalmente diferente do parâmetro,
        para garantir que nunca são comparados)."""
        state = {}
        for idx, shape in enumerate(param_shapes):
            state[idx] = {
                "step": torch.tensor(1.0),
                "state1": torch.zeros(*shape, dtype=torch.uint8),
                "state2": torch.zeros(*shape, dtype=torch.uint8),
                "absmax1": torch.zeros(3),
                "absmax2": torch.zeros(3),
                "qmap1": torch.zeros(256),
                "qmap2": torch.zeros(256),
            }
        return {
            "state": state,
            "param_groups": optimizer.state_dict()["param_groups"],
        }

    def test_bnb_style_state_same_rank_is_not_rejected(self):
        from trainer_difusao.common_pkg.lora_io import _load_optimizer_state

        # Linear(8, 8): weight (8,8) idx0, bias (8,) idx1 — mesmas shapes do
        # state1/state2 sintético abaixo (simula retomada com o MESMO rank).
        model = nn.Linear(8, 8)
        optimizer = torch.optim.AdamW(model.parameters(), lr=1e-3)
        synthetic_state_dict = self._make_bnb_style_state(optimizer, [(8, 8), (8,)])

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bnb_optimizer.pt"
            torch.save(synthetic_state_dict, path)
            ok = _load_optimizer_state(optimizer, path)
            self.assertTrue(
                ok,
                "state1/state2 (bnb 8-bit) do mesmo rank não deveria ser rejeitado "
                "(absmax/qmap não têm shape de parâmetro e não podem ser comparados)",
            )

    def test_bnb_style_state_rank_change_returns_false(self):
        from trainer_difusao.common_pkg.lora_io import _load_optimizer_state

        # state1/state2 sintético salvo para um rank anterior: weight (4, 4).
        old_model = nn.Linear(4, 4)
        old_optimizer = torch.optim.AdamW(old_model.parameters(), lr=1e-3)
        synthetic_state_dict = self._make_bnb_style_state(old_optimizer, [(4, 4), (4,)])

        # Optimizer de destino com rank diferente: weight (8, 8).
        new_model = nn.Linear(8, 8)
        new_optimizer = torch.optim.AdamW(new_model.parameters(), lr=1e-3)
        # Param_groups do state sintético refletem o optimizer antigo (1 grupo,
        # 2 params) — mesma CONTAGEM que o novo, só o SHAPE de state1/state2 diverge.
        synthetic_state_dict["param_groups"] = new_optimizer.state_dict()["param_groups"]

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bnb_optimizer_old_rank.pt"
            torch.save(synthetic_state_dict, path)
            ok = _load_optimizer_state(new_optimizer, path)
            self.assertFalse(
                ok,
                "state1/state2 (bnb 8-bit) com shape de rank diferente deveria ser "
                "rejeitado (WARN + False), não estourar no primeiro optimizer.step()",
            )


@unittest.skipUnless(HAS_TORCH_PEFT, "requer torch, peft e safetensors")
class TestOverrideOptimizerLr(unittest.TestCase):
    def test_lr_from_new_request_prevails_after_restore(self):
        from trainer_difusao.common_pkg.lora_io import (
            _load_optimizer_state,
            _override_optimizer_lr,
            _save_optimizer_state,
        )

        model = nn.Linear(8, 8)
        old_optimizer = torch.optim.Adam(model.parameters(), lr=1e-3)
        loss = model(torch.randn(2, 8)).sum()
        loss.backward()
        old_optimizer.step()

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "optimizer.pt"
            _save_optimizer_state(old_optimizer, path)

            new_optimizer = torch.optim.Adam(model.parameters(), lr=1e-3)
            ok = _load_optimizer_state(new_optimizer, path)
            self.assertTrue(ok)
            for group in new_optimizer.param_groups:
                self.assertEqual(group["lr"], 1e-3)

            new_lr = 5e-5
            _override_optimizer_lr(new_optimizer, new_lr)
            for group in new_optimizer.param_groups:
                self.assertEqual(group["lr"], new_lr)
                self.assertEqual(group["initial_lr"], new_lr)


if __name__ == "__main__":
    unittest.main()
