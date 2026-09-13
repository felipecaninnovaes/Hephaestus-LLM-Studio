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

    def test_train_real_without_cuda_fails_honestly(self):
        os.environ["ENGINE_MOCK"] = "0"
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            cfg_path = tmp_path / "config.yaml"
            out_dir = tmp_path / "output"

            cfg = {
                "job_id": "test-diff-job-real",
                "model": "flux",
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


if __name__ == "__main__":
    unittest.main()
