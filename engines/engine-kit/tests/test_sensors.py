"""
Testes comportamentais e unitários para SystemSensors e telemetria de sistema (ADR-0021 / Fatia 3a).

Cobre:
1. SystemSensors: leitura de /proc, cálculo de deltas (CPU, RAM, taxas de disco).
2. Primeira emissão: baseline sem delta anterior retorna taxas 0.0 (não negativos) ou None caso inacessível.
3. Valores nunca negativos: cpuPct >= 0, ramUsedGb >= 0, diskReadMbS >= 0, diskWriteMbS >= 0.
4. Falha / exceção no coletor: retorno de None, emissão segue sem derrubar o treino.
5. TelemetryEmitter: presença do campo systemMetrics com as 4 chaves exatas (camelCase).
6. Custo: coleta não é chamada mais de uma vez por emissão.
7. JSON estrito: sem NaN/Inf (null conforme RFC 8259).
"""
import io
import json
import math
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

from engine_kit.sensors import (
    SystemSensors,
    collect_system_metrics,
    sanitize_system_metrics,
)
from engine_kit.telemetry import TelemetryEmitter


class TestSystemSensors(unittest.TestCase):
    def test_sensors_collect_real_system(self):
        """Em ambiente Linux com /proc, coleta dados reais válidos."""
        sensors = SystemSensors()
        # Primeira coleta estabelece baseline
        m1 = sensors.collect()
        self.assertIsNotNone(m1)
        self.assertIn("cpuPct", m1)
        self.assertIn("ramUsedGb", m1)
        self.assertIn("diskReadMbS", m1)
        self.assertIn("diskWriteMbS", m1)

        # Baseline: taxas devem ser 0.0 na primeira coleta
        if m1["cpuPct"] is not None:
            self.assertEqual(m1["cpuPct"], 0.0)
        if m1["diskReadMbS"] is not None:
            self.assertEqual(m1["diskReadMbS"], 0.0)
        if m1["diskWriteMbS"] is not None:
            self.assertEqual(m1["diskWriteMbS"], 0.0)
        if m1["ramUsedGb"] is not None:
            self.assertGreaterEqual(m1["ramUsedGb"], 0.0)

        # Segunda coleta após pequeno intervalo
        time.sleep(0.05)
        m2 = sensors.collect()
        self.assertIsNotNone(m2)
        for k in ("cpuPct", "ramUsedGb", "diskReadMbS", "diskWriteMbS"):
            if m2[k] is not None:
                self.assertGreaterEqual(m2[k], 0.0)

    def test_sensors_delta_math(self):
        """Valida precisão do cálculo de taxas entre duas coletas mockadas."""
        sensors = SystemSensors(pid=999999)
        # Mock dos métodos internos de leitura
        with patch.object(sensors, "_read_cpu_ticks", side_effect=[100.0, 150.0]), \
             patch.object(sensors, "_read_ram_used_gb", side_effect=[0.5, 0.6]), \
             patch.object(sensors, "_read_io_bytes", side_effect=[(1048576, 2097152), (3145728, 4194304)]), \
             patch("time.monotonic", side_effect=[10.0, 11.0]):

            # Primeira coleta (t = 10.0)
            first = sensors.collect()
            self.assertEqual(first["cpuPct"], 0.0)
            self.assertEqual(first["ramUsedGb"], 0.5)
            self.assertEqual(first["diskReadMbS"], 0.0)
            self.assertEqual(first["diskWriteMbS"], 0.0)

            # Segunda coleta (t = 11.0, dt = 1.0s)
            # Delta CPU: 50 ticks. Com clk_tck=100 e cpu_count=2:
            # cpu_sec = 50 / 100 = 0.5s. cpu_pct = (0.5 / (1.0 * 2)) * 100 = 25.0%
            sensors._clk_tck = 100.0
            sensors._cpu_count = 2.0

            second = sensors.collect()
            self.assertEqual(second["cpuPct"], 25.0)
            self.assertEqual(second["ramUsedGb"], 0.6)
            # Delta read: (3145728 - 1048576) = 2097152 bytes = 2.0 MB / 1.0s = 2.0 MB/s
            self.assertEqual(second["diskReadMbS"], 2.0)
            # Delta write: (4194304 - 2097152) = 2097152 bytes = 2.0 MB / 1.0s = 2.0 MB/s
            self.assertEqual(second["diskWriteMbS"], 2.0)

    def test_sensors_graceful_on_read_failure(self):
        """Se arquivos /proc não existirem ou falharem na leitura, não levanta exceção."""
        sensors = SystemSensors(pid=999999)
        with patch.object(sensors, "_read_cpu_ticks", side_effect=OSError("No such file")), \
             patch.object(sensors, "_read_ram_used_gb", side_effect=OSError("No such file")), \
             patch.object(sensors, "_read_io_bytes", side_effect=OSError("No such file")):
            res = sensors.collect()
            self.assertIsNotNone(res)
            self.assertIsNone(res["cpuPct"])
            self.assertIsNone(res["ramUsedGb"])
            self.assertIsNone(res["diskReadMbS"])
            self.assertIsNone(res["diskWriteMbS"])

    def test_sanitize_system_metrics_strict(self):
        """Testa conversão camelCase/snake_case e sanitização de NaN/Inf e valores negativos."""
        raw = {
            "cpu_pct": float("nan"),
            "ram_used_gb": 1.5,
            "disk_read_mb_s": float("inf"),
            "disk_write_mb_s": -2.0,
        }
        sanitized = sanitize_system_metrics(raw)
        self.assertIsNotNone(sanitized)
        self.assertIsNone(sanitized["cpuPct"])
        self.assertEqual(sanitized["ramUsedGb"], 1.5)
        self.assertIsNone(sanitized["diskReadMbS"])
        # Valores negativos são clampados para 0.0
        self.assertEqual(sanitized["diskWriteMbS"], 0.0)


class TestTelemetrySystemMetrics(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp_dir = Path(self._tmp.name)

    def tearDown(self):
        self._tmp.cleanup()

    def test_telemetry_emitter_includes_system_metrics_automatically(self):
        """TelemetryEmitter inclui systemMetrics automaticamente se não fornecido."""
        emitter = TelemetryEmitter(self.tmp_dir)
        ev = emitter.emit(
            phase="training",
            message="Step 1",
            progress=0.1,
            step=1,
        )
        self.assertIn("systemMetrics", ev)
        sys_m = ev["systemMetrics"]
        for k in ("cpuPct", "ramUsedGb", "diskReadMbS", "diskWriteMbS"):
            self.assertIn(k, sys_m)
            if sys_m[k] is not None:
                self.assertGreaterEqual(sys_m[k], 0.0)

    def test_telemetry_emitter_custom_system_metrics_override(self):
        """Passagem explícita de system_metrics é aceita e sanitizada."""
        emitter = TelemetryEmitter(self.tmp_dir)
        ev = emitter.emit(
            phase="training",
            message="Step 2",
            progress=0.2,
            step=2,
            system_metrics={
                "cpu_pct": 45.5,
                "ram_used_gb": 3.2,
                "disk_read_mb_s": 12.0,
                "disk_write_mb_s": 0.5,
            },
        )
        sys_m = ev["systemMetrics"]
        self.assertEqual(sys_m["cpuPct"], 45.5)
        self.assertEqual(sys_m["ramUsedGb"], 3.2)
        self.assertEqual(sys_m["diskReadMbS"], 12.0)
        self.assertEqual(sys_m["diskWriteMbS"], 0.5)

    def test_sensors_collect_called_at_most_once_per_emission(self):
        """Custo: coleta não pode ser chamada mais de uma vez por emissão."""
        emitter = TelemetryEmitter(self.tmp_dir)
        mock_sensor = MagicMock()
        mock_sensor.collect.return_value = {
            "cpuPct": 10.0,
            "ramUsedGb": 1.0,
            "diskReadMbS": 0.0,
            "diskWriteMbS": 0.0,
        }
        emitter._sensors = mock_sensor

        emitter.emit(
            phase="training",
            message="Step 1",
            progress=0.1,
            step=1,
        )
        self.assertEqual(mock_sensor.collect.call_count, 1)

    def test_collector_exception_does_not_halt_emission(self):
        """Exceção no coletor gera linha sem systemMetrics (ou com null) e o treino segue."""
        emitter = TelemetryEmitter(self.tmp_dir)
        mock_sensor = MagicMock()
        mock_sensor.collect.side_effect = RuntimeError("Kernel procfs locked")
        emitter._sensors = mock_sensor

        # Não deve lançar exceção
        ev = emitter.emit(
            phase="training",
            message="Step Error",
            progress=0.1,
            step=1,
        )
        self.assertEqual(ev["step"], 1)
        self.assertNotIn("systemMetrics", ev)

    def test_strict_json_serialization(self):
        """Verifica que a linha gravada em telemetry.jsonl é JSON RFC 8259 estrito (sem NaN/Inf)."""
        emitter = TelemetryEmitter(self.tmp_dir)
        emitter.emit(
            phase="training",
            message="Step NaN System",
            progress=0.5,
            step=5,
            system_metrics={
                "cpuPct": float("nan"),
                "ramUsedGb": float("inf"),
                "diskReadMbS": None,
                "diskWriteMbS": float("-inf"),
            },
        )
        telemetry_file = self.tmp_dir / "telemetry.jsonl"
        content = telemetry_file.read_text(encoding="utf-8").strip()
        lines = content.splitlines()
        self.assertTrue(len(lines) > 0)
        parsed = json.loads(
            lines[-1],
            parse_constant=lambda c: (_ for _ in ()).throw(ValueError(f"Unexpected constant: {c}")),
        )
        sys_m = parsed["systemMetrics"]
        self.assertIsNone(sys_m["cpuPct"])
        self.assertIsNone(sys_m["ramUsedGb"])
        self.assertIsNone(sys_m["diskReadMbS"])
        self.assertIsNone(sys_m["diskWriteMbS"])


if __name__ == "__main__":
    unittest.main()
