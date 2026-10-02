"""
Testes comportamentais para emissão de métricas de sistema (Fatia 3a) no trainer-difusao.

Cobre:
1. Emissão via common_pkg.metrics._emit_metric inclui systemMetrics no telemetry.jsonl.
2. systemMetrics tem as 4 chaves: cpuPct, ramUsedGb, diskReadMbS, diskWriteMbS.
3. Valores são não-negativos e JSON estrito (sem NaN/Inf).
4. Exceção na coleta de sensores não derruba o pipeline de métricas.
5. Custo: coleta não é chamada mais de uma vez por emissão.
"""
import json
import math
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

from trainer_difusao.common_pkg.metrics import _emit_metric


class TestTrainerDifusaoSystemMetrics(unittest.TestCase):
    def setUp(self):
        self.old_mock = os.environ.get("ENGINE_MOCK")
        os.environ["ENGINE_MOCK"] = "1"

    def tearDown(self):
        if self.old_mock is not None:
            os.environ["ENGINE_MOCK"] = self.old_mock
        else:
            os.environ.pop("ENGINE_MOCK", None)

    def test_emit_metric_includes_system_metrics_in_telemetry(self):
        """Valida que _emit_metric inclui systemMetrics com as 4 chaves no telemetry.jsonl."""
        with tempfile.TemporaryDirectory() as tmpdir:
            metrics_path = Path(tmpdir) / "metrics.jsonl"
            telemetry_path = Path(tmpdir) / "telemetry.jsonl"

            _emit_metric(
                metrics_path,
                epoch=1,
                step=10,
                loss=0.5,
                progress=0.1,
            )

            self.assertTrue(telemetry_path.exists())
            lines = telemetry_path.read_text(encoding="utf-8").strip().splitlines()
            self.assertEqual(len(lines), 1)

            # JSON estrito
            parsed = json.loads(
                lines[0],
                parse_constant=lambda c: (_ for _ in ()).throw(ValueError(f"Unexpected constant: {c}")),
            )
            self.assertIn("systemMetrics", parsed)
            sys_m = parsed["systemMetrics"]
            for k in ("cpuPct", "ramUsedGb", "diskReadMbS", "diskWriteMbS"):
                self.assertIn(k, sys_m)
                if sys_m[k] is not None:
                    self.assertGreaterEqual(sys_m[k], 0.0)

    def test_emit_metric_system_metrics_strict_json_with_nan(self):
        """Valida que valores NaN/Inf em system_metrics são convertidos para null (RFC 8259)."""
        with tempfile.TemporaryDirectory() as tmpdir:
            metrics_path = Path(tmpdir) / "metrics.jsonl"
            telemetry_path = Path(tmpdir) / "telemetry.jsonl"

            _emit_metric(
                metrics_path,
                epoch=1,
                step=10,
                system_metrics={
                    "cpu_pct": float("nan"),
                    "ram_used_gb": float("inf"),
                    "disk_read_mb_s": 10.5,
                    "disk_write_mb_s": -1.0,
                },
            )

            lines = telemetry_path.read_text(encoding="utf-8").strip().splitlines()
            parsed = json.loads(
                lines[0],
                parse_constant=lambda c: (_ for _ in ()).throw(ValueError(f"Unexpected constant: {c}")),
            )
            sys_m = parsed["systemMetrics"]
            self.assertIsNone(sys_m["cpuPct"])
            self.assertIsNone(sys_m["ramUsedGb"])
            self.assertEqual(sys_m["diskReadMbS"], 10.5)
            # clamped
            self.assertEqual(sys_m["diskWriteMbS"], 0.0)

    def test_emit_metric_system_sensors_exception_does_not_halt_emission(self):
        """Exceção no coletor global de sensores não interrompe emissão nem quebra métricas."""
        with tempfile.TemporaryDirectory() as tmpdir:
            metrics_path = Path(tmpdir) / "metrics.jsonl"
            telemetry_path = Path(tmpdir) / "telemetry.jsonl"

            with patch("trainer_difusao.common_pkg.metrics.get_global_sensors") as mock_get:
                mock_sensor = MagicMock()
                mock_sensor.collect.side_effect = RuntimeError("Sensors fault")
                mock_get.return_value = mock_sensor

                _emit_metric(
                    metrics_path,
                    epoch=1,
                    step=10,
                    loss=0.42,
                )

            # telemetry.jsonl e metrics.jsonl ainda foram gravados
            self.assertTrue(metrics_path.exists())
            self.assertTrue(telemetry_path.exists())
            lines = telemetry_path.read_text(encoding="utf-8").strip().splitlines()
            parsed = json.loads(lines[0])
            self.assertEqual(parsed["step"], 10)
            self.assertNotIn("systemMetrics", parsed)

    def test_emit_metric_collect_called_at_most_once(self):
        """Custo: coleta não pode ser chamada mais de uma vez por emissão."""
        with tempfile.TemporaryDirectory() as tmpdir:
            metrics_path = Path(tmpdir) / "metrics.jsonl"

            with patch("trainer_difusao.common_pkg.metrics.get_global_sensors") as mock_get:
                mock_sensor = MagicMock()
                mock_sensor.collect.return_value = {
                    "cpuPct": 15.0,
                    "ramUsedGb": 2.0,
                    "diskReadMbS": 0.0,
                    "diskWriteMbS": 0.0,
                }
                mock_get.return_value = mock_sensor

                _emit_metric(
                    metrics_path,
                    epoch=1,
                    step=10,
                )

                self.assertEqual(mock_sensor.collect.call_count, 1)


if __name__ == "__main__":
    unittest.main()
