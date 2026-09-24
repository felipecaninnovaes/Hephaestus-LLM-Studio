import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from trainer_difusao.loaders import (
    _apply_loose_encoder_state,
    _custom_checkpoint_identity,
    _is_cache_valid,
    _load_flux2_loose_encoder_merged,
    _save_quant_metadata,
    get_quant_cache_root,
    load_flux2_custom_transformer,
    load_or_quantize_text_encoder,
    load_or_quantize_transformer,
    resolve_quant_base_dir,
)


class TestLoaders(unittest.TestCase):
    def test_quant_cache_validation_and_metadata(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            base_dir = Path(tmpdir)
            comp_dir = base_dir / "transformer"

            self.assertFalse(_is_cache_valid(comp_dir, "test/model", "4bit"))

            comp_dir.mkdir(parents=True, exist_ok=True)
            self.assertFalse(_is_cache_valid(comp_dir, "test/model", "4bit"))

            (comp_dir / "config.json").write_text("{}", encoding="utf-8")
            self.assertFalse(_is_cache_valid(comp_dir, "test/model", "4bit"))

            _save_quant_metadata(
                base_dir,
                model_id="test/other-model",
                quant_label="4-bit NF4",
                quant_format="4bit",
                target_dtype="torch.bfloat16",
                is_flux2=True,
            )
            self.assertFalse(_is_cache_valid(comp_dir, "test/model", "4bit"))

            _save_quant_metadata(
                base_dir,
                model_id="test/model",
                quant_label="4-bit NF4",
                quant_format="4bit",
                target_dtype="torch.bfloat16",
                is_flux2=True,
            )
            self.assertTrue(_is_cache_valid(comp_dir, "test/model", "4bit"))
            self.assertFalse(_is_cache_valid(comp_dir, "test/model", "8bit"))

    def test_resolve_quant_base_dir(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            base_dir = Path(tmpdir)
            p1 = resolve_quant_base_dir("black-forest-labs/FLUX.2-klein-base-4B", "4bit", base_dir=base_dir)
            self.assertIn("FLUX.2-klein-base-4B_4bit", str(p1))

            p2 = resolve_quant_base_dir("repo/x", "8bit", subfolder="custom_subfolder", base_dir=base_dir)
            self.assertEqual(p2, base_dir / "custom_subfolder")

    def test_custom_checkpoint_identity(self):
        self.assertIsNone(_custom_checkpoint_identity(None))
        self.assertIsNone(_custom_checkpoint_identity(""))
        with tempfile.NamedTemporaryFile() as tf:
            tf.write(b"checkpoint-content-1234")
            tf.flush()
            ident = _custom_checkpoint_identity(tf.name)
            self.assertIsNotNone(ident)
            self.assertTrue(ident.startswith(tf.name + "#"))

    def test_transformer_loader_cached(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            base_dir = Path(tmpdir)
            comp_dir = base_dir / "transformer"
            comp_dir.mkdir(parents=True, exist_ok=True)
            (comp_dir / "config.json").write_text("{}", encoding="utf-8")
            _save_quant_metadata(
                base_dir,
                model_id="test/model",
                quant_label="4bit",
                quant_format="4bit",
                target_dtype="torch.bfloat16",
            )

            mock_cls = mock.Mock()
            mock_model = mock.Mock()
            mock_cls.from_pretrained.return_value = mock_model

            cached_called = []
            res = load_or_quantize_transformer(
                model_id="test/model",
                transformer_cls=mock_cls,
                quant_format="4bit",
                quant_base=base_dir,
                transformer_cache_dir=comp_dir,
                on_cached=lambda p: cached_called.append(p),
            )
            self.assertIs(res, mock_model)
            self.assertEqual(len(cached_called), 1)
            mock_cls.from_pretrained.assert_called_once_with(comp_dir, torch_dtype=None)

    def test_transformer_loader_quantizes_and_saves(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            base_dir = Path(tmpdir)
            comp_dir = base_dir / "transformer"

            mock_cls = mock.Mock()
            mock_model = mock.Mock()
            mock_model.save_pretrained.side_effect = lambda p: (Path(p) / "config.json").write_text("{}")
            mock_cls.from_pretrained.return_value = mock_model

            res = load_or_quantize_transformer(
                model_id="test/model",
                transformer_cls=mock_cls,
                quant_format="4bit",
                quant_base=base_dir,
                transformer_cache_dir=comp_dir,
            )
            self.assertIs(res, mock_model)
            self.assertTrue((base_dir / "metadata.json").exists())
            self.assertTrue((comp_dir / "config.json").exists())

    def test_text_encoder_loader_cached(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            base_dir = Path(tmpdir)
            comp_dir = base_dir / "text_encoder"
            comp_dir.mkdir(parents=True, exist_ok=True)
            (comp_dir / "config.json").write_text("{}", encoding="utf-8")
            _save_quant_metadata(
                base_dir,
                model_id="test/model",
                quant_label="4bit",
                quant_format="4bit",
                target_dtype="torch.bfloat16",
            )

            mock_cls = mock.Mock()
            mock_model = mock.Mock()
            mock_cls.from_pretrained.return_value = mock_model

            res = load_or_quantize_text_encoder(
                model_id="test/model",
                encoder_cls=mock_cls,
                encoder_type="qwen3",
                quant_format="4bit",
                quant_base=base_dir,
                text_encoder_cache_dir=comp_dir,
            )
            self.assertIs(res, mock_model)
            mock_cls.from_pretrained.assert_called_once_with(comp_dir, torch_dtype=None)

    def test_apply_loose_encoder_state_layout_check(self):
        mock_enc = mock.Mock()
        mock_enc.load_state_dict.return_value = (["missing_key_1"], [])
        with self.assertRaises(SystemExit):
            _apply_loose_encoder_state(mock_enc, {}, "enc.safetensors", "repo/x")


if __name__ == "__main__":
    unittest.main()
