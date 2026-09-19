import json
import tempfile
import unittest
from pathlib import Path

from engine_kit.telemetry import TelemetryEmitter


class TestTelemetryEmitter(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp_dir = Path(self._tmp.name)

    def tearDown(self):
        self._tmp.cleanup()

    def test_emit_generates_telemetry_and_metrics(self):
        emitter = TelemetryEmitter(self.tmp_dir)
        ev = emitter.emit(
            phase="training",
            message="Epoch 1 complete",
            progress=0.25,
            epoch=1,
            total_epochs=4,
            metrics={"loss": 0.42, "accuracy": 0.91},
        )

        self.assertEqual(ev["phase"], "training")
        self.assertEqual(ev["progress"], 0.25)
        self.assertEqual(ev["epoch"], 1)
        self.assertEqual(ev["totalEpochs"], 4)
        self.assertEqual(ev["metrics"]["loss"], 0.42)

        # Verifica escrita em telemetry.jsonl
        telemetry_file = self.tmp_dir / "telemetry.jsonl"
        self.assertTrue(telemetry_file.exists())
        lines = telemetry_file.read_text(encoding="utf-8").strip().splitlines()
        self.assertEqual(len(lines), 1)
        data = json.loads(lines[0])
        self.assertEqual(data["phase"], "training")

        # Verifica espelho em metrics.jsonl
        metrics_file = self.tmp_dir / "metrics.jsonl"
        self.assertTrue(metrics_file.exists())
        legacy_lines = metrics_file.read_text(encoding="utf-8").strip().splitlines()
        self.assertEqual(len(legacy_lines), 1)
        legacy_data = json.loads(legacy_lines[0])
        self.assertEqual(legacy_data["loss"], 0.42)

    def test_emit_error(self):
        emitter = TelemetryEmitter(self.tmp_dir)
        ev = emitter.error("Failure occurred", ValueError("Invalid batch size"))
        self.assertEqual(ev["phase"], "error")
        self.assertIn("Failure occurred: Invalid batch size", ev["message"])
        self.assertEqual(ev["metrics"]["error_type"], "ValueError")
