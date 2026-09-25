"""
Testes unitários para o pacote sd_pkg e I/O de adaptadores LoRA.
"""
from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

try:
    import torch
    import torch.nn as nn
    from peft import LoraConfig, get_peft_model
    from safetensors.torch import load_file
    HAS_TORCH_PEFT = True
except ImportError:
    HAS_TORCH_PEFT = False

from trainer_difusao.common_pkg.lora_io import (
    save_adapter_checkpoint,
    save_final_adapter,
)
from trainer_difusao.models.sd_pkg import (
    _compute_sdxl_embeddings,
    _generate_sample_sd15,
    _generate_sample_sdxl,
)
from trainer_difusao.models.sd15 import (
    _generate_sample_sd15 as _reexported_sample_sd15,
)
from trainer_difusao.models.sdxl import (
    _compute_sdxl_embeddings as _reexported_compute_sdxl,
    _generate_sample_sdxl as _reexported_sample_sdxl,
)


class TestSdPkgExports(unittest.TestCase):
    def test_reexports_backward_compatibility(self):
        self.assertIs(_generate_sample_sd15, _reexported_sample_sd15)
        self.assertIs(_compute_sdxl_embeddings, _reexported_compute_sdxl)
        self.assertIs(_generate_sample_sdxl, _reexported_sample_sdxl)


@unittest.skipUnless(HAS_TORCH_PEFT, "requer torch, peft e safetensors")
class TestSdxlEmbeddings(unittest.TestCase):
    def test_compute_sdxl_embeddings_shapes_and_dtype(self):
        class DummyTokenizer:
            model_max_length = 77

            def __call__(self, texts, **kwargs):
                class Tokens:
                    input_ids = torch.zeros((len(texts), 77), dtype=torch.long)

                return Tokens()

        class DummyEncoderOne:
            def __call__(self, tokens, output_hidden_states=True):
                class Output:
                    hidden_states = [
                        None,
                        torch.randn((tokens.shape[0], 77, 768), dtype=torch.float32),
                        torch.randn((tokens.shape[0], 77, 768), dtype=torch.float32),
                    ]

                return Output()

        class DummyEncoderTwo:
            def __call__(self, tokens, output_hidden_states=True):
                class Output:
                    hidden_states = [
                        None,
                        torch.randn((tokens.shape[0], 77, 1280), dtype=torch.float32),
                        torch.randn((tokens.shape[0], 77, 1280), dtype=torch.float32),
                    ]
                    text_embeds = torch.randn((tokens.shape[0], 1280), dtype=torch.float32)

                return Output()

        tok_one = DummyTokenizer()
        tok_two = DummyTokenizer()
        enc_one = DummyEncoderOne()
        enc_two = DummyEncoderTwo()

        prompt_embeds, pooled_embeds = _compute_sdxl_embeddings(
            prompts=["a photo of a cat", "a photo of a dog"],
            tokenizer_one=tok_one,
            tokenizer_two=tok_two,
            text_encoder_one=enc_one,
            text_encoder_two=enc_two,
            device="cpu",
            target_dtype=torch.float16,
        )

        self.assertEqual(prompt_embeds.shape, (2, 77, 2048))
        self.assertEqual(pooled_embeds.shape, (2, 1280))
        self.assertEqual(prompt_embeds.dtype, torch.float16)
        self.assertEqual(pooled_embeds.dtype, torch.float16)


@unittest.skipUnless(HAS_TORCH_PEFT, "requer torch, peft e safetensors")
class TestLoraIoHelpers(unittest.TestCase):
    def setUp(self):
        self.model = get_peft_model(
            nn.Sequential(nn.Linear(8, 8)),
            LoraConfig(r=4, lora_alpha=4, target_modules=["0"]),
        )

    def test_save_adapter_checkpoint(self):
        with tempfile.TemporaryDirectory() as tmp:
            ckpt_dir = Path(tmp) / "checkpoints"
            metadata = {"format": "pt", "model_type": "lora", "epoch": "3"}
            saved_path = save_adapter_checkpoint(
                self.model,
                checkpoints_dir=ckpt_dir,
                base_name="my_model",
                epoch=3,
                metadata=metadata,
            )
            expected = ckpt_dir / "my_model_epoch_003.safetensors"
            self.assertEqual(saved_path, expected)
            self.assertTrue(expected.exists())

            tensors = load_file(str(expected))
            self.assertTrue(len(tensors) > 0)
            self.assertEqual(list(Path(tmp).glob("**/.tmp_*")), [])

    def test_save_final_adapter_custom_name(self):
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = Path(tmp) / "output"
            metadata = {"format": "pt", "model_type": "lora"}
            saved_path = save_final_adapter(
                self.model,
                output_dir=out_dir,
                base_name="custom_lora",
                metadata=metadata,
            )
            expected_custom = out_dir / "custom_lora.safetensors"
            expected_canonical = out_dir / "adapter.safetensors"

            self.assertEqual(saved_path, expected_custom)
            self.assertTrue(expected_custom.exists())
            self.assertTrue(expected_canonical.exists())

    def test_save_final_adapter_canonical_name(self):
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = Path(tmp) / "output"
            metadata = {"format": "pt", "model_type": "lora"}
            saved_path = save_final_adapter(
                self.model,
                output_dir=out_dir,
                base_name="adapter",
                metadata=metadata,
            )
            expected = out_dir / "adapter.safetensors"
            self.assertEqual(saved_path, expected)
            self.assertTrue(expected.exists())


@unittest.skipUnless(HAS_TORCH_PEFT, "requer torch, peft e safetensors")
class TestSampleGeneration(unittest.TestCase):
    def test_generate_sample_sd15_restores_unet_train_mode(self):
        class DummyUnet:
            def __init__(self):
                self.training = True

            def eval(self):
                self.training = False

            def train(self):
                self.training = True

        unet = DummyUnet()
        with tempfile.TemporaryDirectory() as tmp:
            out_png = Path(tmp) / "sample.png"
            # Sem diffusers pipeline mockado completo, gera warning mas deve restaurar unet.train()
            _generate_sample_sd15(
                unet=unet,
                vae=None,
                text_encoder=None,
                tokenizer=None,
                noise_scheduler=None,
                prompt="test prompt",
                output_path=out_png,
            )
            self.assertTrue(unet.training)

    def test_generate_sample_sdxl_restores_unet_train_mode(self):
        class DummyUnet:
            def __init__(self):
                self.training = True

            def eval(self):
                self.training = False

            def train(self):
                self.training = True

        unet = DummyUnet()
        with tempfile.TemporaryDirectory() as tmp:
            out_png = Path(tmp) / "sample.png"
            _generate_sample_sdxl(
                unet=unet,
                vae=None,
                text_encoder_one=None,
                text_encoder_two=None,
                tokenizer_one=None,
                tokenizer_two=None,
                noise_scheduler=None,
                prompt="test prompt",
                output_path=out_png,
            )
            self.assertTrue(unet.training)


if __name__ == "__main__":
    unittest.main()
