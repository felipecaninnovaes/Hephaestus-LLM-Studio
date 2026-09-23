"""Testes dedicados para o suporte ao Qwen-Image-2.1 (ADR-0023 / novo-modelo)."""

import json
import os
import tempfile
import unittest
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


if __name__ == "__main__":
    unittest.main()
