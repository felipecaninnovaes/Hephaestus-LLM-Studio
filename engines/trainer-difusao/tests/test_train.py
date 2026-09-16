import json
import os
import struct
import tempfile
import unittest
from pathlib import Path

import yaml
from trainer_difusao.train import main


class TestTrainerDifusao(unittest.TestCase):
    def setUp(self):
        self.old_mock = os.environ.get("ENGINE_MOCK")
        os.environ["ENGINE_MOCK"] = "1"

    def tearDown(self):
        if self.old_mock is not None:
            os.environ["ENGINE_MOCK"] = self.old_mock
        else:
            os.environ.pop("ENGINE_MOCK", None)

    def test_train_mock_produces_artifacts_flux2_klein(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            cfg_path = tmp_path / "config.yaml"
            out_dir = tmp_path / "output"

            cfg = {
                "job_id": "test-diff-job-flux",
                "model": "flux",
                "seed": 42,
                "lora": {
                    "trigger_word": "ohwx",
                    "epochs": 3,
                    "batch_size": 1,
                    "learning_rate": 0.0001,
                    "rank": 16,
                    "alpha": 16,
                },
            }
            with open(cfg_path, "w", encoding="utf-8") as f:
                yaml.dump(cfg, f)

            # Executa comando train para FLUX.2 Klein 4B
            main(["train", "--config", str(cfg_path), "--output", str(out_dir)])

            # Verifica metrics.jsonl
            metrics_file = out_dir / "metrics.jsonl"
            self.assertTrue(metrics_file.exists())
            lines = [
                json.loads(line)
                for line in metrics_file.read_text().splitlines()
                if line.strip()
            ]
            self.assertEqual(len(lines), 3)
            self.assertEqual(lines[0]["epoch"], 1)
            self.assertIn("loss", lines[0])

            # Verifica adapter.safetensors
            adapter_file = out_dir / "adapter.safetensors"
            self.assertTrue(adapter_file.exists())
            data = adapter_file.read_bytes()
            self.assertGreater(len(data), 8)
            header_len = struct.unpack("<Q", data[:8])[0]
            header_json = json.loads(data[8 : 8 + header_len].decode("utf-8"))
            self.assertIn("__metadata__", header_json)
            meta = header_json["__metadata__"]
            self.assertEqual(meta["format"], "pt")
            self.assertEqual(meta["framework"], "diffusers")
            self.assertEqual(meta["base_model"], "flux-2-klein-4b")
            self.assertEqual(meta["trigger_word"], "ohwx")
            self.assertEqual(meta["lora_rank"], "16")
            self.assertEqual(meta["quantization"], "4bit")

    def test_train_mock_custom_quantization_propagation(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            cfg_path = tmp_path / "config.yaml"
            out_dir = tmp_path / "output"

            cfg = {
                "job_id": "test-diff-job-8bit",
                "model": "flux",
                "lora": {
                    "epochs": 1,
                    "quantization": "8bit",
                },
            }
            with open(cfg_path, "w", encoding="utf-8") as f:
                yaml.dump(cfg, f)

            main(["train", "--config", str(cfg_path), "--output", str(out_dir)])
            adapter_file = out_dir / "adapter.safetensors"
            data = adapter_file.read_bytes()
            header_len = struct.unpack("<Q", data[:8])[0]
            header_json = json.loads(data[8 : 8 + header_len].decode("utf-8"))
            meta = header_json["__metadata__"]
            self.assertEqual(meta["quantization"], "8bit")

    def test_train_mock_produces_artifacts_sdxl(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            cfg_path = tmp_path / "config.yaml"
            out_dir = tmp_path / "output"

            cfg = {
                "job_id": "test-diff-job-sdxl",
                "model": "sdxl",
                "seed": 42,
                "lora": {
                    "epochs": 2,
                    "batch_size": 1,
                    "learning_rate": 0.0001,
                    "rank": 32,
                    "alpha": 32,
                },
            }
            with open(cfg_path, "w", encoding="utf-8") as f:
                yaml.dump(cfg, f)

            main(["train", "--config", str(cfg_path), "--output", str(out_dir)])

            adapter_file = out_dir / "adapter.safetensors"
            data = adapter_file.read_bytes()
            header_len = struct.unpack("<Q", data[:8])[0]
            header_json = json.loads(data[8 : 8 + header_len].decode("utf-8"))
            meta = header_json["__metadata__"]
            self.assertEqual(meta["base_model"], "sdxl")
            self.assertEqual(meta["lora_rank"], "32")

    def test_train_mock_produces_artifacts_sd15(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            cfg_path = tmp_path / "config.yaml"
            out_dir = tmp_path / "output"

            cfg = {
                "job_id": "test-diff-job-sd15",
                "model": "sd15",
                "seed": 42,
                "lora": {
                    "trigger_word": "sks style",
                    "epochs": 2,
                    "batch_size": 1,
                    "learning_rate": 0.0001,
                    "rank": 8,
                    "alpha": 8,
                },
            }
            with open(cfg_path, "w", encoding="utf-8") as f:
                yaml.dump(cfg, f)

            main(["train", "--config", str(cfg_path), "--output", str(out_dir)])

            adapter_file = out_dir / "adapter.safetensors"
            data = adapter_file.read_bytes()
            header_len = struct.unpack("<Q", data[:8])[0]
            header_json = json.loads(data[8 : 8 + header_len].decode("utf-8"))
            meta = header_json["__metadata__"]
            self.assertEqual(meta["base_model"], "sd15")
            self.assertEqual(meta["lora_rank"], "8")
            self.assertEqual(meta["trigger_word"], "sks style")

    def test_train_real_without_cuda_fails_honestly(self):
        os.environ["ENGINE_MOCK"] = "0"
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            cfg_path = tmp_path / "config.yaml"
            out_dir = tmp_path / "output"

            for m in ("flux", "sdxl", "sd15"):
                cfg = {
                    "job_id": f"test-diff-job-real-{m}",
                    "model": m,
                    "lora": {"epochs": 1},
                }
                with open(cfg_path, "w", encoding="utf-8") as f:
                    yaml.dump(cfg, f)

                with self.assertRaises(SystemExit) as ctx:
                    main(["train", "--config", str(cfg_path), "--output", str(out_dir)])
                self.assertEqual(ctx.exception.code, 1)

    def test_health_mock(self):
        # health não levanta exceção
        main(["health"])

    def test_train_mock_produces_sample_images(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            cfg_path = tmp_path / "config.yaml"
            out_dir = tmp_path / "output"

            cfg = {
                "job_id": "test-diff-job-samples",
                "model": "sdxl",
                "seed": 42,
                "lora": {
                    "epochs": 4,
                    "learning_rate": 0.0002,
                },
                "samples": {
                    "prompt": "a cinematic portrait of cybernetic warrior",
                    "interval": 2,
                },
            }
            with open(cfg_path, "w", encoding="utf-8") as f:
                yaml.dump(cfg, f)

            main(["train", "--config", str(cfg_path), "--output", str(out_dir)])

            # Verifica métricas com lr e evento baseline na época 0
            metrics_file = out_dir / "metrics.jsonl"
            self.assertTrue(metrics_file.exists())
            lines = [json.loads(l) for l in metrics_file.read_text().splitlines() if l.strip()]
            self.assertEqual(len(lines), 5)
            self.assertEqual(lines[0]["epoch"], 0)
            self.assertEqual(lines[0]["phase"], "baseline_ready")
            self.assertEqual(lines[1]["epoch"], 1)
            self.assertEqual(lines[1]["lr"], 0.0002)

            # Verifica amostra baseline (época 0) e amostras geradas nas épocas 2 e 4
            samples_dir = out_dir / "samples"
            self.assertTrue(samples_dir.is_dir())
            sample0 = samples_dir / "sample_epoch_000.png"
            sample2 = samples_dir / "sample_epoch_002.png"
            sample4 = samples_dir / "sample_epoch_004.png"
            self.assertTrue(sample0.exists())
            self.assertTrue(sample2.exists())
            self.assertTrue(sample4.exists())
            self.assertGreater(sample0.stat().st_size, 0)
            self.assertGreater(sample2.stat().st_size, 0)
            self.assertGreater(sample4.stat().st_size, 0)

    def test_flux_pack_latents_transformation(self):
        from unittest.mock import MagicMock

        from trainer_difusao.train import _pack_latents

        # Simula tensores VAE com formato [B, C, H, W] = [2, 16, 64, 64]
        mock_latents = MagicMock()
        mock_latents.shape = (2, 16, 64, 64)
        mock_view = MagicMock()
        mock_permute = MagicMock()
        mock_reshaped = MagicMock()

        mock_latents.view.return_value = mock_view
        mock_view.permute.return_value = mock_permute
        mock_permute.reshape.return_value = mock_reshaped

        res = _pack_latents(mock_latents)

        mock_latents.view.assert_called_once_with(2, 16, 32, 2, 32, 2)
        mock_view.permute.assert_called_once_with(0, 2, 4, 1, 3, 5)
        mock_permute.reshape.assert_called_once_with(2, 1024, 64)
        self.assertEqual(res, mock_reshaped)

    def test_generate_mock_produces_image(self):
        from PIL import Image

        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            cfg_path = tmp_path / "config.yaml"
            out_dir = tmp_path / "output"

            cfg = {
                "job_id": "test-diff-gen-001",
                "engine": "diffusion",
                "mode": "generate",
                "generate": {
                    "base_model": "flux-2-klein-4b",
                    "prompt": "a futuristic cyberpunk forge with glowing violet lasers",
                    "negative_prompt": "blurry, low quality",
                    "width": 512,
                    "height": 512,
                    "steps": 20,
                    "guidance_scale": 3.5,
                    "seed": 12345,
                    "quantization": "4bit",
                    "lora_scale": 0.8,
                },
            }
            with open(cfg_path, "w", encoding="utf-8") as f:
                yaml.dump(cfg, f)

            main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

            gen_file = out_dir / "generated.png"
            self.assertTrue(gen_file.exists())
            self.assertGreater(gen_file.stat().st_size, 0)

            # Valida que é uma imagem PNG válida com as dimensões especificadas
            with Image.open(gen_file) as img:
                self.assertEqual(img.size, (512, 512))
                self.assertEqual(img.format, "PNG")

    def test_generate_validation_fails_without_prompt(self):
        from trainer_difusao.generate import load_and_validate_generate_config

        cfg = {
            "job_id": "test-fail",
            "generate": {
                "base_model": "sdxl",
                "prompt": "",
            },
        }
        with self.assertRaises(SystemExit):
            load_and_validate_generate_config(cfg)

    def test_resolve_output_name(self):
        from trainer_difusao.common import _resolve_output_name

        self.assertEqual(_resolve_output_name({}), "adapter")
        self.assertEqual(_resolve_output_name({"output_name": ""}), "adapter")
        self.assertEqual(_resolve_output_name({"output_name": "  "}), "adapter")
        self.assertEqual(
            _resolve_output_name({"output_name": "meu-modelo"}),
            "meu-modelo",
        )
        self.assertEqual(
            _resolve_output_name({"output_name": "meu-modelo.safetensors"}),
            "meu-modelo",
        )
        self.assertEqual(
            _resolve_output_name({"output_name": "  custom_model_v1.safetensors  "}),
            "custom_model_v1",
        )
        self.assertEqual(
            _resolve_output_name({"output_name": "model/with:invalid*chars"}),
            "model_with_invalid_chars",
        )

    def test_train_mock_produces_epoch_checkpoints_and_custom_output_name(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            cfg_path = tmp_path / "config.yaml"
            out_dir = tmp_path / "output"

            cfg = {
                "job_id": "test-checkpoints-job",
                "model": "flux",
                "output_name": "minha-lora-klein.safetensors",
                "seed": 42,
                "lora": {
                    "epochs": 3,
                    "batch_size": 1,
                    "learning_rate": 0.00003,
                    "rank": 16,
                    "alpha": 16,
                },
            }
            with open(cfg_path, "w", encoding="utf-8") as f:
                yaml.dump(cfg, f)

            main(["train", "--config", str(cfg_path), "--output", str(out_dir)])

            # Verifica checkpoints por época
            checkpoints_dir = out_dir / "checkpoints"
            self.assertTrue(checkpoints_dir.exists())

            for ep in (1, 2, 3):
                ckpt_file = checkpoints_dir / f"minha-lora-klein_epoch_{ep:03d}.safetensors"
                self.assertTrue(ckpt_file.exists(), f"Checkpoint da época {ep} deve existir")
                data = ckpt_file.read_bytes()
                header_len = struct.unpack("<Q", data[:8])[0]
                meta = json.loads(data[8 : 8 + header_len].decode("utf-8"))["__metadata__"]
                self.assertEqual(meta.get("epoch"), str(ep))

            # Verifica adaptador final com nome semântico e cópia retroativa
            final_file = out_dir / "minha-lora-klein.safetensors"
            self.assertTrue(final_file.exists())
            self.assertGreater(final_file.stat().st_size, 0)

            adapter_compat_file = out_dir / "adapter.safetensors"
            self.assertTrue(adapter_compat_file.exists())
            self.assertEqual(final_file.read_bytes(), adapter_compat_file.read_bytes())


    def test_train_mock_checkpoint_interval(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            cfg_path = tmp_path / "config.yaml"
            out_dir = tmp_path / "output"

            cfg = {
                "job_id": "test-interval-job",
                "model": "flux",
                "output_name": "interval-lora",
                "checkpoint_interval": 2,
                "lora": {
                    "epochs": 5,
                    "batch_size": 1,
                    "learning_rate": 0.0001,
                },
            }
            with open(cfg_path, "w", encoding="utf-8") as f:
                yaml.dump(cfg, f)

            main(["train", "--config", str(cfg_path), "--output", str(out_dir)])

            checkpoints_dir = out_dir / "checkpoints"
            self.assertTrue(checkpoints_dir.exists())

            # Com interval=2 e 5 épocas, devem existir épocas 2, 4 e 5 (última época sempre salva)
            self.assertFalse((checkpoints_dir / "interval-lora_epoch_001.safetensors").exists())
            self.assertTrue((checkpoints_dir / "interval-lora_epoch_002.safetensors").exists())
            self.assertFalse((checkpoints_dir / "interval-lora_epoch_003.safetensors").exists())
            self.assertTrue((checkpoints_dir / "interval-lora_epoch_004.safetensors").exists())
            self.assertTrue((checkpoints_dir / "interval-lora_epoch_005.safetensors").exists())

    def test_train_mock_resume_with_offset_and_weights(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            cfg_path = tmp_path / "config.yaml"
            out_dir = tmp_path / "output"
            dummy_weights = tmp_path / "prev_checkpoint.safetensors"
            dummy_weights.write_bytes(b"dummy_weights_content")

            cfg = {
                "job_id": "test-resume-job",
                "model": "flux",
                "output_name": "resumed-lora",
                "epoch_offset": 5,
                "weights_path": str(dummy_weights),
                "lora": {
                    "epochs": 3,
                    "batch_size": 1,
                    "learning_rate": 0.0001,
                },
            }
            with open(cfg_path, "w", encoding="utf-8") as f:
                yaml.dump(cfg, f)

            main(["train", "--config", str(cfg_path), "--output", str(out_dir)])

            metrics_file = out_dir / "metrics.jsonl"
            self.assertTrue(metrics_file.exists())
            lines = [
                json.loads(line)
                for line in metrics_file.read_text().splitlines()
                if line.strip()
            ]
            self.assertEqual(len(lines), 3)
            # As épocas devem ser 6, 7 e 8
            self.assertEqual(lines[0]["epoch"], 6)
            self.assertEqual(lines[1]["epoch"], 7)
            self.assertEqual(lines[2]["epoch"], 8)

            checkpoints_dir = out_dir / "checkpoints"
            self.assertTrue((checkpoints_dir / "resumed-lora_epoch_006.safetensors").exists())
            self.assertTrue((checkpoints_dir / "resumed-lora_epoch_007.safetensors").exists())
            self.assertTrue((checkpoints_dir / "resumed-lora_epoch_008.safetensors").exists())


    def test_resolve_bucket_reso_preserves_aspect_ratio(self):
        from trainer_difusao.dataset import _resolve_bucket_reso

        # Paisagem 16:9 -> bucket largo (w > h), lados múltiplos de 64
        bw, bh = _resolve_bucket_reso(1920, 1080, base_res=512)
        self.assertGreater(bw, bh)
        self.assertEqual(bw % 64, 0)
        self.assertEqual(bh % 64, 0)
        self.assertAlmostEqual(bw / bh, 1920 / 1080, delta=0.5)

        # Retrato 3:4 -> bucket alto (h > w)
        w2, h2 = _resolve_bucket_reso(768, 1024, base_res=512)
        self.assertGreater(h2, w2)
        self.assertEqual(w2 % 64, 0)
        self.assertEqual(h2 % 64, 0)

        # Quadrado -> bucket quadrado
        w3, h3 = _resolve_bucket_reso(1024, 1024, base_res=512)
        self.assertEqual(w3, h3)
        self.assertEqual(w3, 512)

    def test_diffusion_dataset_enable_bucket_groups_by_aspect_ratio(self):
        from PIL import Image

        from trainer_difusao.dataset import DiffusionDataset

        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            images_dir = tmp_path / "images"
            images_dir.mkdir()
            for name, size in (("a.png", (1920, 1080)), ("b.png", (768, 1024)), ("c.jpg", (512, 512))):
                Image.new("RGB", size, (120, 120, 120)).save(images_dir / name)

            ds = DiffusionDataset(tmp_path, resolution=512, enable_bucket=True)
            self.assertEqual(len(ds), 3)
            self.assertGreaterEqual(len(ds.buckets), 2)  # ao menos paisagem e retrato/quadrado

            # Amostras do mesmo bucket compartilham exatamente as mesmas dims
            for bucket, idxs in ds.buckets.items():
                dims = {ds.bucket_dims[i] for i in idxs}
                self.assertEqual(dims, {bucket})

            # Sem bucketing: tudo quadrado na resolução base
            ds_sq = DiffusionDataset(tmp_path, resolution=512, enable_bucket=False)
            self.assertEqual(len(ds_sq.buckets), 0)
            self.assertTrue(all(d == (512, 512) for d in ds_sq.bucket_dims))

    def test_bucket_batch_sampler_keeps_uniform_batches(self):
        from PIL import Image

        from trainer_difusao.dataset import (
            BucketBatchSampler,
            DiffusionDataset,
        )

        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            images_dir = tmp_path / "images"
            images_dir.mkdir()
            sizes = [(1920, 1080)] * 5 + [(768, 1024)] * 3 + [(512, 512)] * 2
            for i, size in enumerate(sizes):
                Image.new("RGB", size, (100, 100, 100)).save(images_dir / f"img_{i}.png")

            ds = DiffusionDataset(tmp_path, resolution=512, enable_bucket=True)
            sampler = BucketBatchSampler(ds, batch_size=4, seed=42)

            seen = set()
            for batch in sampler:
                self.assertLessEqual(len(batch), 4)
                self.assertEqual(len({ds.bucket_dims[i] for i in batch}), 1)  # batch uniforme
                seen.update(batch)
            self.assertEqual(seen, set(range(len(ds))))
            self.assertEqual(len(sampler), 4)  # ceil(5/4) + ceil(3/4) + ceil(2/4) = 2+1+1


if __name__ == "__main__":
    unittest.main()


