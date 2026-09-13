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


if __name__ == "__main__":
    unittest.main()

