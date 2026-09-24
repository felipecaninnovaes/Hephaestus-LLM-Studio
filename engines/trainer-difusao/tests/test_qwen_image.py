"""Testes dedicados para o suporte ao Qwen-Image-2.1 (ADR-0023 / novo-modelo)."""

import json
import os
import tempfile
import unittest
from contextlib import nullcontext
from pathlib import Path

import yaml

from trainer_difusao.common import _canonical_model_name
from trainer_difusao.generate import cmd_generate, load_and_validate_generate_config
from trainer_difusao.models import BaseModelTrainer, QwenImageTrainer, get_trainer
from trainer_difusao.train import cmd_train


class TestQwenImage(unittest.TestCase):
    def setUp(self):
        self.old_mock = os.environ.get("ENGINE_MOCK")
        os.environ["ENGINE_MOCK"] = "1"
        self._tmpdir = tempfile.TemporaryDirectory()
        self.tmp_path = Path(self._tmpdir.name)

    def tearDown(self):
        self._tmpdir.cleanup()
        if self.old_mock is not None:
            os.environ["ENGINE_MOCK"] = self.old_mock
        else:
            os.environ.pop("ENGINE_MOCK", None)

    def test_canonical_name(self):
        for raw in ("qwen", "qwen-image", "qwen-image-2.1", "qwen2.1", "qwen_image", "qwen-image-2-1"):
            self.assertEqual(_canonical_model_name(raw), "qwen-image-2.1")

    def test_get_trainer_factory(self):
        trainer = get_trainer("qwen-image-2.1", is_mock=False)
        self.assertIsInstance(trainer, QwenImageTrainer)
        self.assertIsInstance(trainer, BaseModelTrainer)

        alias_trainer = get_trainer("qwen", is_mock=False)
        self.assertIsInstance(alias_trainer, QwenImageTrainer)

    def test_generate_config_validation(self):
        cfg = {
            "job_id": "test-qwen-gen",
            "generate": {
                "base_model": "qwen-image-2.1",
                "prompt": "Um dragão fofo em fundo transparente",
                "width": 1024,
                "height": 1024,
                "steps": 25,
            },
        }
        params = load_and_validate_generate_config(cfg)
        self.assertEqual(params["base_model"], "qwen-image-2.1")
        self.assertEqual(params["width"], 1024)
        self.assertEqual(params["height"], 1024)
        self.assertEqual(params["steps"], 25)
        self.assertEqual(params["guidance_scale"], 3.5)

    def test_mock_train_qwen_image(self):
        dataset_dir = self.tmp_path / "dataset"
        dataset_dir.mkdir(parents=True)
        img_file = dataset_dir / "sample.jpg"
        from PIL import Image

        Image.new("RGB", (256, 256), color="red").save(img_file)
        (dataset_dir / "sample.txt").write_text("a photo of a dragon", encoding="utf-8")

        output_dir = self.tmp_path / "train_output"
        cfg = {
            "dataset_path": str(dataset_dir),
            "model": "qwen-image-2.1",
            "seed": 42,
            "lora": {
                "rank": 16,
                "alpha": 16,
                "epochs": 2,
                "learning_rate": 0.0002,
                "trigger_word": "qwen_dragon",
            },
        }
        cfg_file = self.tmp_path / "train_cfg.yaml"
        with open(cfg_file, "w", encoding="utf-8") as f:
            yaml.dump(cfg, f)

        cmd_train(["--config", str(cfg_file), "--output", str(output_dir)])

        metrics_file = output_dir / "metrics.jsonl"
        self.assertTrue(metrics_file.exists())
        lines = [json.loads(line) for line in metrics_file.read_text().splitlines() if line.strip()]
        self.assertEqual(len(lines), 2)
        self.assertEqual(lines[0]["epoch"], 1)
        self.assertIn("loss", lines[0])

        adapter_file = output_dir / "adapter.safetensors"
        self.assertTrue(adapter_file.exists())
        self.assertGreater(adapter_file.stat().st_size, 100)

    def test_mock_generate_qwen_image(self):
        output_dir = self.tmp_path / "gen_output"
        cfg = {
            "job_id": "test-qwen-gen-run",
            "generate": {
                "base_model": "qwen-image-2.1",
                "prompt": "Neon cyberpunk city at night",
                "width": 512,
                "height": 512,
                "steps": 10,
                "batch_size": 2,
            },
        }
        cfg_file = self.tmp_path / "gen_cfg.yaml"
        with open(cfg_file, "w", encoding="utf-8") as f:
            yaml.dump(cfg, f)

        cmd_generate(["--config", str(cfg_file), "--output", str(output_dir)])

        meta_file = output_dir / "generation_meta.json"
        self.assertTrue(meta_file.exists())
        lines = [json.loads(line) for line in meta_file.read_text().splitlines() if line.strip()]
        self.assertEqual(len(lines), 2)
        self.assertEqual(lines[0]["base_model"], "qwen-image-2.1")
        self.assertTrue((output_dir / "generated_0001.png").exists())
        self.assertTrue((output_dir / "generated_0002.png").exists())
        self.assertTrue((output_dir / "thumb_0001.jpg").exists())
        self.assertTrue((output_dir / "thumb_0002.jpg").exists())

    def test_mock_train_qwen_image_with_samples(self):
        dataset_dir = self.tmp_path / "dataset_samples"
        dataset_dir.mkdir(parents=True)
        img_file = dataset_dir / "sample.jpg"
        from PIL import Image

        Image.new("RGB", (256, 256), color="blue").save(img_file)
        (dataset_dir / "sample.txt").write_text("a photo of a dragon", encoding="utf-8")

        output_dir = self.tmp_path / "train_output_samples"
        cfg = {
            "dataset_path": str(dataset_dir),
            "model": "qwen-image-2.1",
            "seed": 42,
            "samples": {
                "prompt": "a majestic dragon on a mountain",
                "interval": 1,
                "seed": 123,
            },
            "lora": {
                "rank": 16,
                "alpha": 16,
                "epochs": 2,
                "learning_rate": 0.0002,
                "trigger_word": "qwen_dragon",
            },
        }
        cfg_file = self.tmp_path / "train_samples_cfg.yaml"
        with open(cfg_file, "w", encoding="utf-8") as f:
            yaml.dump(cfg, f)

        cmd_train(["--config", str(cfg_file), "--output", str(output_dir)])

        # Valida que sample_epoch_000.png e sample_epoch_001.png foram produzidos
        sample_0 = output_dir / "samples" / "sample_epoch_000.png"
        sample_1 = output_dir / "samples" / "sample_epoch_001.png"
        sample_2 = output_dir / "samples" / "sample_epoch_002.png"

        self.assertTrue(sample_0.exists(), "sample_epoch_000.png deve ser produzido (baseline época 0)")
        self.assertTrue(sample_1.exists(), "sample_epoch_001.png deve ser produzido (amostra época 1)")
        self.assertTrue(sample_2.exists(), "sample_epoch_002.png deve ser produzido (amostra época 2)")

    def test_generate_sample_qwen_unit(self):
        from unittest.mock import MagicMock, patch
        from PIL import Image
        import torch
        from trainer_difusao.models.qwen_pkg import _generate_sample_qwen

        mock_transformer = MagicMock()
        mock_transformer.training = True
        mock_transformer.dtype = torch.float32

        def set_eval():
            mock_transformer.training = False

        def set_train():
            mock_transformer.training = True

        mock_transformer.eval.side_effect = set_eval
        mock_transformer.train.side_effect = set_train

        mock_vae = MagicMock()
        mock_vae.dtype = torch.float32
        mock_scheduler = MagicMock()

        output_img = self.tmp_path / "unit_samples" / "sample_epoch_000.png"
        metrics_file = self.tmp_path / "unit_metrics.jsonl"

        mock_pipe_instance = MagicMock()
        mock_img = Image.new("RGB", (64, 64), color="green")
        mock_result = MagicMock()
        mock_result.images = [mock_img]
        mock_pipe_instance.return_value = mock_result

        import diffusers
        p1 = patch("diffusers.QwenImagePipeline", return_value=mock_pipe_instance)
        p2 = patch("diffusers.QwenImage21Pipeline", return_value=mock_pipe_instance) if hasattr(diffusers, "QwenImage21Pipeline") else None
        with p1, (p2 if p2 else nullcontext()):
            sample_embeds = {
                "prompt_embeds": torch.randn(1, 16, 64),
                "prompt_embeds_mask": torch.ones(1, 16, dtype=torch.bool),
            }
            _generate_sample_qwen(
                transformer=mock_transformer,
                vae=mock_vae,
                scheduler=mock_scheduler,
                prompt="test unit prompt",
                output_path=output_img,
                seed=42,
                resolution=64,
                metrics_path=metrics_file,
                epoch=0,
                sample_embeds=sample_embeds,
            )

            # Verifica que o arquivo final foi salvo atomicamente e existe
            self.assertTrue(output_img.exists())
            # Verifica que nenhum arquivo .tmp_ sobrou
            tmp_img = output_img.with_name(f".tmp_{output_img.name}")
            self.assertFalse(tmp_img.exists())
            # Verifica que o transformer retornou para o modo train
            self.assertTrue(mock_transformer.training)
            # Verifica chamada do pipeline com prompt_embeds
            call_kwargs = mock_pipe_instance.call_args[1]
            self.assertIn("prompt_embeds", call_kwargs)
            self.assertEqual(call_kwargs["num_inference_steps"], 20)
            self.assertEqual(call_kwargs["height"], 64)
            self.assertEqual(call_kwargs["width"], 64)
            self.assertEqual(call_kwargs.get("true_cfg_scale", call_kwargs.get("guidance_scale")), 3.5)

    def test_release_system_memory(self):
        from trainer_difusao.models.qwen_image import _release_system_memory
        # Deve executar sem levantar exceção em qualquer ambiente
        _release_system_memory()

    def test_generate_sample_qwen_guidance_dispatch(self):
        from unittest.mock import MagicMock, patch
        from PIL import Image
        import torch
        from trainer_difusao.models.qwen_pkg import _generate_sample_qwen

        mock_transformer = MagicMock()
        mock_transformer.training = True
        mock_transformer.dtype = torch.float32
        mock_vae = MagicMock()
        mock_vae.dtype = torch.float32
        mock_scheduler = MagicMock()

        sample_embeds = {
            "prompt_embeds": torch.randn(1, 16, 64),
            "prompt_embeds_mask": torch.ones(1, 16, dtype=torch.bool),
        }

        # Sub-caso 1: Pipeline cuja assinatura aceita explicitamente true_cfg_scale
        class MockPipeTrueCfg:
            def __call__(self, prompt_embeds=None, true_cfg_scale=3.5, **kwargs):
                res = MagicMock()
                res.images = [Image.new("RGB", (64, 64))]
                return res

        pipe_inst1 = MockPipeTrueCfg()
        with patch("diffusers.QwenImage21Pipeline", return_value=pipe_inst1, create=True), \
             patch("diffusers.QwenImagePipeline", return_value=pipe_inst1):
            out_img = self.tmp_path / "sample_true_cfg.png"
            _generate_sample_qwen(
                transformer=mock_transformer,
                vae=mock_vae,
                scheduler=mock_scheduler,
                prompt="dragon",
                output_path=out_img,
                sample_embeds=sample_embeds,
            )
            self.assertTrue(out_img.exists())

        # Sub-caso 2: Pipeline legado cuja assinatura aceita guidance_scale
        class MockPipeGuidance:
            def __call__(self, prompt_embeds=None, guidance_scale=3.5, **kwargs):
                res = MagicMock()
                res.images = [Image.new("RGB", (64, 64))]
                return res

        pipe_inst2 = MockPipeGuidance()
        with patch("diffusers.QwenImage21Pipeline", return_value=pipe_inst2, create=True), \
             patch("diffusers.QwenImagePipeline", return_value=pipe_inst2):
            out_img = self.tmp_path / "sample_guidance.png"
            _generate_sample_qwen(
                transformer=mock_transformer,
                vae=mock_vae,
                scheduler=mock_scheduler,
                prompt="dragon",
                output_path=out_img,
                sample_embeds=sample_embeds,
            )
            self.assertTrue(out_img.exists())

        # Sub-caso 3: Pipeline que levanta TypeError com true_cfg_scale e faz fallback para guidance_scale
        called_with = []
        class MockPipeFallback:
            def __call__(self, **kwargs):
                if "true_cfg_scale" in kwargs:
                    raise TypeError("unexpected keyword argument 'true_cfg_scale'")
                called_with.append(kwargs)
                res = MagicMock()
                res.images = [Image.new("RGB", (64, 64))]
                return res

        pipe_inst3 = MockPipeFallback()
        with patch("diffusers.QwenImage21Pipeline", return_value=pipe_inst3, create=True), \
             patch("diffusers.QwenImagePipeline", return_value=pipe_inst3):
            out_img = self.tmp_path / "sample_fallback.png"
            _generate_sample_qwen(
                transformer=mock_transformer,
                vae=mock_vae,
                scheduler=mock_scheduler,
                prompt="dragon",
                output_path=out_img,
                sample_embeds=sample_embeds,
            )
            self.assertTrue(out_img.exists())
            self.assertEqual(len(called_with), 1)
            self.assertIn("guidance_scale", called_with[0])

    def test_qwen_train_step_metrics_emission(self):
        from trainer_difusao.common import _emit_metric
        metrics_file = self.tmp_path / "test_steps" / "metrics.jsonl"
        telemetry_file = self.tmp_path / "test_steps" / "telemetry.jsonl"

        # Emite métrica de passo de treino
        _emit_metric(
            metrics_file,
            epoch=1,
            step=1,
            loss=0.3456,
            lr=2e-4,
            progress=0.11,
            phase="training",
            message="Época 1/5 · Step 1/15 · Loss: 0.3456",
        )
        # Emite métrica de conclusão de época
        _emit_metric(
            metrics_file,
            epoch=1,
            step=3,
            loss=0.3200,
            lr=2e-4,
            progress=0.23,
            phase="epoch_complete",
            message="Época 1/5 concluída - Loss Média: 0.3200",
        )

        self.assertTrue(metrics_file.exists())
        self.assertTrue(telemetry_file.exists())

        lines = [json.loads(line) for line in metrics_file.read_text().splitlines() if line.strip()]
        self.assertEqual(len(lines), 2)
        self.assertEqual(lines[0]["phase"], "training")
        self.assertEqual(lines[0]["step"], 1)
        self.assertEqual(lines[0]["loss"], 0.3456)
        self.assertEqual(lines[1]["phase"], "epoch_complete")

        t_lines = [json.loads(line) for line in telemetry_file.read_text().splitlines() if line.strip()]
        self.assertEqual(len(t_lines), 2)
        self.assertEqual(t_lines[0]["phase"], "training")
        self.assertEqual(t_lines[0]["step"], 1)
        self.assertIn("Época 1/5 · Step 1/15", t_lines[0]["message"])
        self.assertEqual(t_lines[1]["phase"], "epoch_complete")

if __name__ == "__main__":
    unittest.main()
