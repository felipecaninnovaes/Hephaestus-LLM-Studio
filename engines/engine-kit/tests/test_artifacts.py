import json
import struct
import tempfile
import unittest
from pathlib import Path

from engine_kit.artifacts import make_fake_artifact, make_fake_safetensors, prune_checkpoints


class TestArtifacts(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp_dir = Path(self._tmp.name)

    def tearDown(self):
        self._tmp.cleanup()

    def test_prune_checkpoints(self):
        cp_dir = self.tmp_dir / "checkpoints"
        cp_dir.mkdir()

        # Cria 4 checkpoints com épocas distintas
        (cp_dir / "model_epoch_001.safetensors").write_text("ep1")
        (cp_dir / "model_epoch_002.safetensors").write_text("ep2")
        (cp_dir / "model_epoch_003.safetensors").write_text("ep3")
        (cp_dir / "model_epoch_004.safetensors").write_text("ep4")
        (cp_dir / "best.safetensors").write_text("best")

        removed = prune_checkpoints(
            cp_dir,
            keep_last_n=2,
            prefix="model_",
            suffix=".safetensors",
            keep_files={"best.safetensors"},
        )

        self.assertEqual(len(removed), 2)
        self.assertFalse((cp_dir / "model_epoch_001.safetensors").exists())
        self.assertFalse((cp_dir / "model_epoch_002.safetensors").exists())
        self.assertTrue((cp_dir / "model_epoch_003.safetensors").exists())
        self.assertTrue((cp_dir / "model_epoch_004.safetensors").exists())
        self.assertTrue((cp_dir / "best.safetensors").exists())

    def test_make_fake_safetensors(self):
        out = self.tmp_dir / "test.safetensors"
        make_fake_safetensors(out, metadata={"key": "val"})
        self.assertTrue(out.exists())

        raw = out.read_bytes()
        header_len = struct.unpack("<Q", raw[:8])[0]
        header_json = json.loads(raw[8 : 8 + header_len].decode("utf-8"))
        self.assertEqual(header_json["__metadata__"]["key"], "val")

    def test_make_fake_artifact(self):
        out = self.tmp_dir / "best.pt"
        make_fake_artifact(out, magic_bytes=b"YOLO_MOCK")
        self.assertTrue(out.exists())
        self.assertTrue(out.read_bytes().startswith(b"YOLO_MOCK"))
