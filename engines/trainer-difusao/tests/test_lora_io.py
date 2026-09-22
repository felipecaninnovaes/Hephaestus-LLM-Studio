"""Regressão: chaves salvas não devem conter prefixo PEFT ('base_model.model.')."""

import tempfile
import unittest
from pathlib import Path

import torch
import torch.nn as nn
from peft import LoraConfig, get_peft_model
from safetensors.torch import load_file


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


if __name__ == "__main__":
    unittest.main()
