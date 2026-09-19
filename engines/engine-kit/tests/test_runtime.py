import tempfile
import unittest
from pathlib import Path

from engine_kit.runtime import atomic_write, is_cancelled


class TestRuntime(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp_dir = Path(self._tmp.name)

    def tearDown(self):
        self._tmp.cleanup()

    def test_atomic_write_text(self):
        dest = self.tmp_dir / "subdir" / "hello.txt"
        atomic_write(dest, "content 123")
        self.assertTrue(dest.exists())
        self.assertEqual(dest.read_text(encoding="utf-8"), "content 123")

    def test_atomic_write_bytes(self):
        dest = self.tmp_dir / "data.bin"
        atomic_write(dest, b"\x00\x01\x02\x03")
        self.assertTrue(dest.exists())
        self.assertEqual(dest.read_bytes(), b"\x00\x01\x02\x03")

    def test_is_cancelled(self):
        sentinel = self.tmp_dir / "cancel"
        self.assertFalse(is_cancelled(sentinel))
        sentinel.touch()
        self.assertTrue(is_cancelled(sentinel))
