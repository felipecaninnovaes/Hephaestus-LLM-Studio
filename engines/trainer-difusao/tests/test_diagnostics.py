"""
Testes comportamentais para Fatia 3b (diagnósticos de treinamento para difusão).
Cobre:
(a) loop mock com loss forçado a NaN -> diagnostics.nanCount > 0 no telemetry.jsonl;
(b) gradNormL2 e lrPerGroup presentes nos steps emitidos;
(c) loraNorms presente com adaptador LoRA e com <=16 chaves;
(d) exceção no cálculo de diagnostics não interrompe o treino.
"""
import json
import math
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

import yaml
from trainer_difusao.train import main
from trainer_difusao.common_pkg.diagnostics import (
    DiagnosticsTracker,
    compute_grad_norm_l2,
    compute_lora_norms,
    extract_lora_group,
)


class TestTrainingDiagnostics(unittest.TestCase):
    def setUp(self):
        self.old_mock = os.environ.get("ENGINE_MOCK")
        os.environ["ENGINE_MOCK"] = "1"

    def tearDown(self):
        if self.old_mock is not None:
            os.environ["ENGINE_MOCK"] = self.old_mock
        else:
            os.environ.pop("ENGINE_MOCK", None)
        os.environ.pop("MOCK_INJECT_NAN", None)

    def test_mock_loop_forced_nan_records_nan_count(self):
        """(a) loop mock com loss forçado a NaN -> diagnostics.nanCount > 0 no telemetry.jsonl."""
        os.environ["MOCK_INJECT_NAN"] = "1"
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            cfg_path = tmp_path / "config.yaml"
            out_dir = tmp_path / "output"

            cfg = {
                "job_id": "test-diag-nan",
                "model": "sd15",
                "lora": {
                    "epochs": 2,
                    "batch_size": 1,
                    "learning_rate": 0.0001,
                    "rank": 4,
                },
            }
            with open(cfg_path, "w", encoding="utf-8") as f:
                yaml.dump(cfg, f)

            main(["train", "--config", str(cfg_path), "--output", str(out_dir)])

            telemetry_file = out_dir / "telemetry.jsonl"
            self.assertTrue(telemetry_file.exists())
            lines = [
                json.loads(line)
                for line in telemetry_file.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]

            # Encontra eventos de training com diagnostics
            diag_events = [line for line in lines if "diagnostics" in line]
            self.assertGreater(len(diag_events), 0)
            first_diag = diag_events[0]["diagnostics"]
            self.assertGreater(first_diag["nanCount"], 0)

    def test_diagnostics_grad_norm_and_lr_per_group_present(self):
        """(b) gradNormL2 e lrPerGroup presentes nos steps emitidos e grad_norm em metrics."""
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            cfg_path = tmp_path / "config.yaml"
            out_dir = tmp_path / "output"

            cfg = {
                "job_id": "test-diag-fields",
                "model": "flux",
                "lora": {
                    "epochs": 2,
                    "batch_size": 1,
                    "learning_rate": 0.0002,
                    "rank": 8,
                },
            }
            with open(cfg_path, "w", encoding="utf-8") as f:
                yaml.dump(cfg, f)

            main(["train", "--config", str(cfg_path), "--output", str(out_dir)])

            telemetry_file = out_dir / "telemetry.jsonl"
            self.assertTrue(telemetry_file.exists())
            lines = [
                json.loads(line)
                for line in telemetry_file.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]

            diag_events = [line for line in lines if "diagnostics" in line]
            self.assertGreater(len(diag_events), 0)
            for ev in diag_events:
                diag = ev["diagnostics"]
                self.assertIn("gradNormL2", diag)
                self.assertIsInstance(diag["gradNormL2"], (int, float))
                self.assertIn("lrPerGroup", diag)
                self.assertIsInstance(diag["lrPerGroup"], list)
                self.assertEqual(diag["lrPerGroup"], [0.0002])

                # Verifica que grad_norm está também em metrics escalar
                self.assertIn("metrics", ev)
                self.assertIn("grad_norm", ev["metrics"])
                self.assertIsInstance(ev["metrics"]["grad_norm"], (int, float))

    def test_lora_norms_present_and_max_16_keys(self):
        """(c) loraNorms presente com adaptador LoRA e com <=16 chaves."""
        # Teste 1: no mock telemetry
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            cfg_path = tmp_path / "config.yaml"
            out_dir = tmp_path / "output"

            cfg = {
                "job_id": "test-diag-lora",
                "model": "sdxl",
                "lora": {
                    "epochs": 1,
                    "learning_rate": 0.0001,
                    "rank": 16,
                },
            }
            with open(cfg_path, "w", encoding="utf-8") as f:
                yaml.dump(cfg, f)

            main(["train", "--config", str(cfg_path), "--output", str(out_dir)])

            telemetry_file = out_dir / "telemetry.jsonl"
            lines = [
                json.loads(line)
                for line in telemetry_file.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]
            diag_events = [line for line in lines if "diagnostics" in line]
            self.assertGreater(len(diag_events), 0)
            for ev in diag_events:
                diag = ev["diagnostics"]
                self.assertIn("loraNorms", diag)
                self.assertIsInstance(diag["loraNorms"], dict)
                self.assertLessEqual(len(diag["loraNorms"]), 16)

        # Teste 2: com modelo mock contendo 30 blocos
        try:
            import torch
            import torch.nn as nn

            class FakeLoraModule(nn.Module):
                def __init__(self):
                    super().__init__()
                    # Cria 30 módulos com parâmetros lora
                    for i in range(30):
                        setattr(self, f"block_{i}", nn.Parameter(torch.ones(4, 4)))

            model = FakeLoraModule()
            norms = compute_lora_norms(model, max_keys=16)
            self.assertLessEqual(len(norms), 16)
        except ImportError:
            pass

    def test_diagnostics_exception_does_not_interrupt_training(self):
        """(d) exceção no cálculo de diagnostics não interrompe o treino."""
        tracker = DiagnosticsTracker()
        tracker.observe_step(0.5, 1.0)

        # Simula objeto que gera erro no optimizer/model
        class BuggyOptimizer:
            @property
            def param_groups(self):
                raise RuntimeError("Erro intencional no optimizer param_groups")

        diag = tracker.build_diagnostics(
            grad_norm_l2=1.5,
            optimizer=BuggyOptimizer(),
            default_lr=0.001,
            model=None,
        )
        # O build_diagnostics se recupera graciosamente
        self.assertIn("gradNormL2", diag)
        self.assertEqual(diag["gradNormL2"], 1.5)
        self.assertEqual(diag["nanCount"], 0)
        self.assertEqual(diag["infCount"], 0)
        self.assertEqual(diag["lrPerGroup"], [0.001])

    def test_tracker_nan_and_inf_produces_strict_json(self):
        """Teste: DiagnosticsTracker real com loss NaN e grad norm Inf -> linha do telemetry.jsonl
        é JSON estrito (sem NaN/Inf como literais), nanCount > 0, gradNormL2 é null, e lrPerGroup com NaN vira null.
        """
        from trainer_difusao.common_pkg.metrics import _emit_metric

        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            metrics_path = tmp_path / "metrics.jsonl"
            telemetry_path = tmp_path / "telemetry.jsonl"

            tracker = DiagnosticsTracker()
            # Observa NaN em loss e Inf em grad_norm
            tracker.observe_step(float("nan"), float("inf"))

            # Cria optimizer mock com lr com NaN
            mock_opt = MagicMock()
            mock_opt.param_groups = [{"lr": float("nan")}, {"lr": 0.001}]

            diag = tracker.build_diagnostics(
                grad_norm_l2=float("inf"),
                optimizer=mock_opt,
                default_lr=0.001,
                model=None,
                step=1,
            )

            # Emite métrica
            _emit_metric(
                metrics_path=metrics_path,
                epoch=1,
                step=1,
                loss=float("nan"),
                lr=float("inf"),
                grad_norm=float("inf"),
                diagnostics=diag,
            )

            # Verifica que tanto metrics.jsonl quanto telemetry.jsonl são JSON estrito
            for target_file in (telemetry_path, metrics_path):
                self.assertTrue(target_file.exists())
                for line in target_file.read_text(encoding="utf-8").splitlines():
                    if not line.strip():
                        continue
                    # JSON estrito que lança se encontrar NaN / Infinity / -Infinity
                    parsed = json.loads(
                        line,
                        parse_constant=lambda c: (_ for _ in ()).throw(ValueError(f"Unexpected constant: {c}")),
                    )
                    if "diagnostics" in parsed:
                        diag_parsed = parsed["diagnostics"]
                        self.assertGreater(diag_parsed["nanCount"], 0)
                        self.assertGreater(diag_parsed["infCount"], 0)
                        self.assertIsNone(diag_parsed["gradNormL2"])
                        self.assertIsNone(diag_parsed["lrPerGroup"][0])
                        self.assertEqual(diag_parsed["lrPerGroup"][1], 0.001)
                    if "metrics" in parsed and "grad_norm" in parsed["metrics"]:
                        self.assertIsNone(parsed["metrics"]["grad_norm"])
                        self.assertIsNone(parsed["metrics"]["loss"])
                        self.assertIsNone(parsed["metrics"]["lr"])
