import json
import struct
import tempfile
import unittest
from pathlib import Path

import yaml
from trainer_difusao.train import main


class TestTrainerDifusao(unittest.TestCase):
    def test_train_mock_produces_artifacts(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            cfg_path = tmp_path / "config.yaml"
            out_dir = tmp_path / "output"

            cfg = {
                "job_id": "test-diff-job-001",
                "model": "sdxl",
                "seed": 42,
                "lora": {
                    "epochs": 3,
                    "batch_size": 1,
                    "learning_rate": 0.0001,
                    "rank": 16,
                    "alpha": 16,
                },
            }
            with open(cfg_path, "w", encoding="utf-8") as f:
                yaml.dump(cfg, f)

            # Executa comando train
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
            self.assertEqual(lines[2]["epoch"], 3)

            # Verifica adapter.safetensors
            adapter_file = out_dir / "adapter.safetensors"
            self.assertTrue(adapter_file.exists())
            data = adapter_file.read_bytes()
            self.assertGreater(len(data), 8)
            header_len = struct.unpack("<Q", data[:8])[0]
            header_json = json.loads(data[8 : 8 + header_len].decode("utf-8"))
            self.assertIn("__metadata__", header_json)
            self.assertEqual(header_json["__metadata__"]["format"], "pt")


if __name__ == "__main__":
    unittest.main()
