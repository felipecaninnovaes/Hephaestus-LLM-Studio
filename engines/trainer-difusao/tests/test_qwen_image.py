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

try:
    import torch
    HAS_TORCH = True
except ImportError:
    HAS_TORCH = False


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

    @unittest.skipUnless(HAS_TORCH, "requer torch")
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

    @unittest.skipUnless(HAS_TORCH, "requer torch")
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
            message="Época 1/5 · Step 1/15 · Loss: 0.3456 · 2.5s/step · ETA: 35s",
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
            message="Época 1/5 concluída - Loss Média: 0.3200 · ETA: 25s",
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
        self.assertIn("2.5s/step · ETA: 35s", t_lines[0]["message"])
        self.assertEqual(t_lines[1]["phase"], "epoch_complete")
        self.assertIn("Época 1/5 concluída - Loss Média: 0.3200 · ETA: 25s", t_lines[1]["message"])
    def test_format_eta_trainer_difusao(self):
        from trainer_difusao.common import _format_eta
        self.assertEqual(_format_eta(None), "N/A")
        self.assertEqual(_format_eta(-5), "N/A")
        self.assertEqual(_format_eta(0), "0s")
        self.assertEqual(_format_eta(45), "45s")
        self.assertEqual(_format_eta(60), "1m")
        self.assertEqual(_format_eta(125), "2m 5s")
        self.assertEqual(_format_eta(3600), "1h")
        self.assertEqual(_format_eta(8100), "2h 15m")

    def test_qwen_enriched_telemetry_emission(self):
        from trainer_difusao.common import _emit_metric
        metrics_file = self.tmp_path / "enriched" / "metrics.jsonl"
        telemetry_file = self.tmp_path / "enriched" / "telemetry.jsonl"

        _emit_metric(
            metrics_file,
            epoch=1,
            step=10,
            loss=0.2541,
            lr=1.5e-4,
            progress=0.20,
            phase="training",
            message="Época 1/5 · Step 10/50 · Loss: 0.2541 · 6.2s/step · ETA: 4m 8s",
            total_steps=50,
            total_epochs=5,
            step_time_s=6.2,
            eta_s=248,
            eta_formatted="4m 8s",
            vram_reserved_gb=10.5,
            loss_ema=0.2600,
        )

        self.assertTrue(metrics_file.exists())
        self.assertTrue(telemetry_file.exists())

        m_data = json.loads(metrics_file.read_text().strip())
        self.assertEqual(m_data["total_steps"], 50)
        self.assertEqual(m_data["total_epochs"], 5)
        self.assertEqual(m_data["step_time_s"], 6.2)
        self.assertEqual(m_data["eta_s"], 248)
        self.assertEqual(m_data["eta_formatted"], "4m 8s")
        self.assertEqual(m_data["loss_ema"], 0.26)

        t_data = json.loads(telemetry_file.read_text().strip())
        self.assertEqual(t_data["step"], 10)
        self.assertEqual(t_data["totalSteps"], 50)
        self.assertEqual(t_data["totalEpochs"], 5)
        self.assertEqual(t_data["stepTimeSeconds"], 6.2)
        self.assertEqual(t_data["speed"], "6.2s/step")
        self.assertEqual(t_data["etaSeconds"], 248)
        self.assertEqual(t_data["etaFormatted"], "4m 8s")
        self.assertEqual(t_data["vramReservedGb"], 10.5)
        self.assertEqual(t_data["metrics"]["loss"], 0.2541)
        self.assertEqual(t_data["metrics"]["lossEma"], 0.26)
        self.assertEqual(t_data["metrics"]["lr"], 1.5e-4)
        self.assertIn("6.2s/step · ETA: 4m 8s", m_data["message"])
        self.assertIn("6.2s/step · ETA: 4m 8s", t_data["message"])
        self.assertIn("6.2s/step · ETA: 4m 8s", t_data["phaseMessage"])

    def test_qwen_adaptive_emit_and_eta_logic(self):
        from trainer_difusao.common import _format_eta
        # Testa gatilho adaptativo: step_time >= 5.0 -> emit_interval = 1
        avg_step_time = 5.2
        total_steps = 1000
        emit_interval = 1 if avg_step_time >= 5.0 else (1 if total_steps <= 100 else (5 if total_steps <= 500 else 10))
        self.assertEqual(emit_interval, 1)

        # Testa steps rápidos: avg_step_time < 5.0 com total_steps > 500 -> emit_interval = 10
        fast_step_time = 0.8
        emit_interval_fast = 1 if fast_step_time >= 5.0 else (1 if total_steps <= 100 else (5 if total_steps <= 500 else 10))
        self.assertEqual(emit_interval_fast, 10)

        # Testa cálculo de ETA
        remaining_steps = 50
        eta_s = int(remaining_steps * avg_step_time)
        self.assertEqual(eta_s, 260)
        self.assertEqual(_format_eta(eta_s), "4m 20s")

        # Testa cálculo de micro-step index
        grad_accum = 4
        micro_indices = [((s - 1) % grad_accum) + 1 for s in range(1, 9)]
        self.assertEqual(micro_indices, [1, 2, 3, 4, 1, 2, 3, 4])

    @unittest.skipUnless(HAS_TORCH, "requer torch")
    def test_qwen_batched_prompt_encoding_logic(self):
        """Valida que a lógica de chunking de prompts e desempacotamento de tensors produz shapes consistentes."""
        import torch
        unique_prompts = [f"Prompt número {i}" for i in range(15)]
        prompt_cache = {}
        bs = 4

        class DummyPipeline:
            def encode_prompt(self, chunk, device=None):
                n = len(chunk)
                pes = torch.randn(n, 16, 4096)
                pe_masks = torch.ones(n, 16)
                ipms = torch.ones(n, 16)
                return pes, pe_masks, ipms

        pipeline = DummyPipeline()
        for i in range(0, len(unique_prompts), bs):
            chunk = unique_prompts[i : i + bs]
            encoded = pipeline.encode_prompt(chunk)
            pes, pe_masks, ipms = encoded
            for idx, p_text in enumerate(chunk):
                pe_item = pes[idx : idx + 1]
                mask_item = pe_masks[idx : idx + 1]
                ipm_item = ipms[idx : idx + 1]
                prompt_cache[p_text] = (pe_item, mask_item, ipm_item)

        self.assertEqual(len(prompt_cache), 15)
        first_pe, first_mask, first_ipm = prompt_cache["Prompt número 0"]
        self.assertEqual(first_pe.shape, (1, 16, 4096))
        self.assertEqual(first_mask.shape, (1, 16))
        self.assertEqual(first_ipm.shape, (1, 16))

    def test_qwen_lora_alpha_not_shadowed_by_image_alpha(self):
        """Garante que a variável de canal alpha da imagem não sobrescreve o lora_alpha escalar."""
        import inspect
        from trainer_difusao.models import qwen_image
        src = inspect.getsource(qwen_image._real_train_qwen_image)
        # Não deve haver 'alpha = torch.ones' (deve ser 'alpha_channel = torch.ones')
        self.assertNotIn("alpha = torch.ones", src)
        self.assertIn("alpha_channel = torch.ones", src)

    @unittest.skipUnless(HAS_TORCH, "requer torch")
    def test_qwen_sample_does_not_pass_image_pad_mask_when_unsupported(self):
        """Garante que _generate_sample_qwen não injeta image_pad_mask se o pipeline não o aceita."""
        from unittest.mock import MagicMock
        from trainer_difusao.models.qwen_pkg.sample import _generate_sample_qwen
        import torch

        mock_pipe_instance = MagicMock()
        # __call__ sem image_pad_mask nos argumentos
        def fake_call(prompt=None, height=512, width=512, generator=None, num_inference_steps=20, true_cfg_scale=3.5, prompt_embeds=None, callback_on_step_end=None):
            res = MagicMock()
            img = MagicMock()
            res.images = [img]
            return res
        mock_pipe_instance.__call__ = fake_call

        mock_pipe_cls = MagicMock(return_value=mock_pipe_instance)

        sample_embeds = {
            "prompt_embeds": torch.randn(1, 16, 4096),
            "prompt_embeds_mask": torch.ones(1, 16, dtype=torch.bool),
            "image_pad_mask": torch.zeros(1, 16, dtype=torch.bool),
        }

        from unittest.mock import patch
        with patch("diffusers.QwenImage21Pipeline", mock_pipe_cls, create=True), \
             patch("trainer_difusao.models.qwen_pkg.sample.os.replace"):
            out_file = self.tmp_path / "sample_test.png"
            _generate_sample_qwen(
                transformer=MagicMock(),
                vae=MagicMock(),
                scheduler=MagicMock(),
                prompt="test prompt",
                output_path=out_file,
                resolution=1024,
                sample_embeds=sample_embeds,
            )
            self.assertTrue(mock_pipe_cls.called)

    @unittest.skipUnless(HAS_TORCH, "requer torch")
    def test_qwen_sample_cycle_breaking_and_memory_cleanup(self):
        """Valida que _generate_sample_qwen quebra ciclos em pipe.components, anula vae/transformer e invoca release_memory."""
        from unittest.mock import MagicMock, patch
        from trainer_difusao.models.qwen_pkg.sample import _generate_sample_qwen
        import torch
        from PIL import Image

        mock_vae = MagicMock()
        mock_vae.device = torch.device("cpu")
        mock_vae.dtype = torch.float32
        mock_transformer = MagicMock()
        mock_transformer.training = True
        mock_transformer.dtype = torch.float16
        mock_scheduler = MagicMock()

        class DummyPipeline:
            def __init__(self, **kwargs):
                self.vae = kwargs.get("vae")
                self.transformer = kwargs.get("transformer")
                self.scheduler = kwargs.get("scheduler")
                self.components = {
                    "vae": self.vae,
                    "transformer": self.transformer,
                    "scheduler": self.scheduler,
                }

            def __call__(self, **kwargs):
                res = MagicMock()
                res.images = [Image.new("RGB", (64, 64))]
                return res

        captured_pipe = []

        def dummy_pipe_factory(**kwargs):
            p = DummyPipeline(**kwargs)
            captured_pipe.append(p)
            return p

        out_file = self.tmp_path / "sample_cycle.png"
        sample_embeds = {
            "prompt_embeds": torch.randn(1, 16, 4096),
            "prompt_embeds_mask": torch.ones(1, 16, dtype=torch.bool),
        }

        with patch("diffusers.QwenImage21Pipeline", side_effect=dummy_pipe_factory, create=True), \
             patch("trainer_difusao.models.qwen_pkg.sample.release_memory") as mock_release_mem, \
             patch("trainer_difusao.models.qwen_pkg.sample.os.replace"):
            _generate_sample_qwen(
                transformer=mock_transformer,
                vae=mock_vae,
                scheduler=mock_scheduler,
                prompt="test prompt",
                output_path=out_file,
                resolution=512,
                sample_embeds=sample_embeds,
            )

        self.assertEqual(len(captured_pipe), 1)
        pipe = captured_pipe[0]
        self.assertIsNone(pipe.vae)
        self.assertIsNone(pipe.transformer)
        self.assertIsNone(pipe.components["vae"])
        self.assertIsNone(pipe.components["transformer"])
        self.assertIsNone(pipe.components["scheduler"])
        self.assertTrue(mock_release_mem.called)
        self.assertTrue(mock_vae.to.called)
        self.assertTrue(mock_transformer.train.called)

    @unittest.skipUnless(HAS_TORCH, "requer torch")
    def test_real_generate_unloads_residual_lora_when_reusing_cached_pipeline(self):
        """Valida que _real_generate descarrega LoRA residual (unload_lora_weights) ao reaproveitar pipeline em cache com ou sem LoRA."""
        from unittest.mock import MagicMock, patch
        from trainer_difusao.generation.runner import _real_generate
        from PIL import Image

        call_order = []
        mock_pipe = MagicMock()
        mock_pipe.unload_lora_weights.side_effect = lambda: call_order.append("unload_lora_weights")
        mock_pipe.load_lora_weights.side_effect = lambda *a, **kw: call_order.append("load_lora_weights")
        mock_pipe.set_adapters = MagicMock()
        mock_pipe.scheduler = MagicMock()
        mock_pipe.scheduler.config = {}

        class DummyOut:
            images = [Image.new("RGB", (256, 256))]

        mock_pipe.return_value = DummyOut()

        lora_dir = self.tmp_path / "lora_test"
        lora_dir.mkdir(parents=True)
        lora_file = lora_dir / "adapter.safetensors"
        lora_file.write_bytes(b"dummy")

        # Cenário 1: Reuso com novos adaptadores LoRA (deve descarregar residuais antes de carregar os novos)
        params_with_lora = load_and_validate_generate_config(
            {
                "job_id": "test-lora-unload-qwen-with-lora",
                "generate": {
                    "base_model": "qwen-image-2.1",
                    "prompt": "a magical forge",
                    "width": 256,
                    "height": 256,
                    "steps": 1,
                    "seed": 42,
                    "quantization": "none",
                    "loras": [{"path": str(lora_file), "scale": 0.8}],
                },
            }
        )

        with patch("torch.cuda.is_available", return_value=False):
            _real_generate(params_with_lora, self.tmp_path / "out1", pipeline=mock_pipe)

        mock_pipe.unload_lora_weights.assert_called_once()
        mock_pipe.load_lora_weights.assert_called_once()
        self.assertEqual(call_order, ["unload_lora_weights", "load_lora_weights"])

        # Cenário 2: Reuso sem adaptadores LoRA (deve descarregar residuais mesmo sem novos LoRAs)
        call_order.clear()
        mock_pipe.unload_lora_weights.reset_mock()
        mock_pipe.load_lora_weights.reset_mock()

        params_without_lora = load_and_validate_generate_config(
            {
                "job_id": "test-lora-unload-qwen-no-lora",
                "generate": {
                    "base_model": "qwen-image-2.1",
                    "prompt": "a magical forge without lora",
                    "width": 256,
                    "height": 256,
                    "steps": 1,
                    "seed": 42,
                    "quantization": "none",
                },
            }
        )

        with patch("torch.cuda.is_available", return_value=False):
            _real_generate(params_without_lora, self.tmp_path / "out2", pipeline=mock_pipe)

        mock_pipe.unload_lora_weights.assert_called_once()
        mock_pipe.load_lora_weights.assert_not_called()
        self.assertEqual(call_order, ["unload_lora_weights"])

if __name__ == "__main__":
    unittest.main()
