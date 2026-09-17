"""Telemetria fina de geração (bug 004): progresso interpolado do sampler.

Cobre os helpers puros (`_sampler_progress`, `_should_emit`,
`_make_sampler_callback`, `_pipe_call_kwargs_with_callback`) e o mock
(ENGINE_MOCK=1): múltiplos progressos intermediários monotônicos em
(0.55, 0.90] com 'step' na mensagem. O path real (GPU/pesos) é exercido
apenas via helpers — sem torch, sem rede.
"""

import json
import os
import tempfile
import unittest
from pathlib import Path

import yaml

from trainer_difusao.generate import (
    _MOCK_TELEMETRY_SUBSTEPS,
    _make_sampler_callback,
    _pipe_call_kwargs_with_callback,
    _sampler_progress,
    _should_emit,
)
from trainer_difusao.train import main


class _Recorder:
    """Emissor fake: registra chamadas sem I/O."""

    def __init__(self, fail=False):
        self.events = []
        self.fail = fail

    def emit(self, **kwargs):
        if self.fail:
            raise RuntimeError("boom")
        self.events.append(kwargs)
        return kwargs


class TestSamplerProgress(unittest.TestCase):
    def test_formula_primeira_e_ultima_fatia(self):
        # batch=1, 20 steps: primeiro step > 0.55, último == 0.90
        first = _sampler_progress(0, 1, 0, 20)
        last = _sampler_progress(0, 1, 19, 20)
        self.assertGreater(first, 0.55)
        self.assertLess(first, 0.90)
        self.assertAlmostEqual(last, 0.90, places=9)

    def test_formula_meio_do_batch(self):
        # batch=2, imagem 0 último substep == início da fatia da imagem 1
        end_img0 = _sampler_progress(0, 2, 3, 4)
        start_img1 = 0.55 + 0.35 * 1 / 2
        self.assertAlmostEqual(end_img0, start_img1, places=9)

    def test_monotonico_nos_steps(self):
        progs = [_sampler_progress(1, 3, s, 20) for s in range(20)]
        for a, b in zip(progs, progs[1:]):
            self.assertLess(a, b)
        self.assertTrue(all(0.55 < p <= 0.90 for p in progs))

    def test_guards_zerados(self):
        self.assertIsInstance(_sampler_progress(0, 0, 0, 0), float)


class TestShouldEmit(unittest.TestCase):
    def test_primeiro_e_ultimo_sempre(self):
        self.assertTrue(_should_emit(0.8, 0.8001, is_first=True))
        self.assertTrue(_should_emit(0.8, 0.8001, is_last=True))

    def test_prev_none_emite(self):
        self.assertTrue(_should_emit(None, 0.56))

    def test_throttle_delta_minimo(self):
        self.assertFalse(_should_emit(0.60, 0.605))
        self.assertFalse(_should_emit(0.60, 0.609))
        self.assertTrue(_should_emit(0.60, 0.61))
        self.assertTrue(_should_emit(0.60, 0.65))

    def test_chamada_2_args_compativel(self):
        self.assertFalse(_should_emit(0.60, 0.601))
        self.assertTrue(_should_emit(0.60, 0.62))


class TestSamplerCallback(unittest.TestCase):
    def test_callback_emite_progresso_interpolado(self):
        rec = _Recorder()
        cb = _make_sampler_callback(rec, 0, 2, 20)
        for s in range(20):
            out = cb(None, s, None, {"latents": object()})
            self.assertIn("latents", out)
        self.assertGreaterEqual(len(rec.events), 2)  # primeiro + último
        self.assertLessEqual(len(rec.events), 52)  # throttle ~50/imagem
        progs = [e["progress"] for e in rec.events]
        self.assertEqual(progs, sorted(progs))
        self.assertTrue(all(0.55 < p <= 0.55 + 0.35 / 2 + 1e-9 for p in progs))
        first, last = rec.events[0], rec.events[-1]
        self.assertEqual(first["phase"], "generating")
        self.assertIn("step 1/20", first["message"])
        self.assertIn("step 20/20", last["message"])
        for e in rec.events:
            self.assertEqual(e["step"], 0)  # contador de IMAGENS
            self.assertEqual(e["total_steps"], 2)

    def test_callback_throttle_100_steps(self):
        rec = _Recorder()
        cb = _make_sampler_callback(rec, 1, 2, 100)
        for s in range(100):
            cb(None, s, None, None)
        self.assertLessEqual(len(rec.events), 52)
        self.assertGreaterEqual(len(rec.events), 2)

    def test_callback_never_quebra_geracao(self):
        rec = _Recorder(fail=True)
        cb = _make_sampler_callback(rec, 0, 1, 10)
        for s in range(10):  # não deve levantar mesmo com emitter explodindo
            self.assertEqual(cb(None, s, None, None), {})
        self.assertEqual(rec.events, [])

    def test_pipe_kwargs_fallback_noop(self):
        rec = _Recorder()
        kwargs = _pipe_call_kwargs_with_callback(rec, 0, 1, 20)
        self.assertIn("callback_on_step_end", kwargs)
        # Pipeline legada sem suporte a callback: TypeError → caller chama sem kwargs
        def legacy_pipe(**kw):
            if "callback_on_step_end" in kw:
                raise TypeError(
                    "got an unexpected keyword argument 'callback_on_step_end'"
                )
            return "ok"

        try:
            legacy_pipe(prompt="x", **kwargs)
        except TypeError as exc:
            # guarda do generate.py: só retrya se for erro de binding do callback
            self.assertIn("callback_on_step_end", str(exc))
            self.assertEqual(legacy_pipe(prompt="x"), "ok")
        else:
            self.fail("pipeline legada deveria rejeitar o callback")

    def test_pipe_kwargs_fallback_reraises_internal_typeerror(self):
        # TypeError interna (sem 'callback_on_step_end') → propaga, sem retry
        def broken_pipe(**kw):
            raise TypeError("object1 and object2 cannot be converted to a Tensor")

        with self.assertRaises(TypeError) as ctx:
            try:
                broken_pipe(
                    prompt="x",
                    **_pipe_call_kwargs_with_callback(_Recorder(), 0, 1, 20),
                )
            except TypeError as exc:
                if "callback_on_step_end" not in str(exc):
                    raise
                self.fail("TypeError interna não deveria cair no fallback")
        self.assertNotIn("callback_on_step_end", str(ctx.exception))


class TestMockIntermediateTelemetry(unittest.TestCase):
    """Mock produz N incrementos interpolados por imagem, monotônicos."""

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

    def _run_mock(self, batch_size):
        cfg = {
            "job_id": "test-telemetry-004",
            "generate": {
                "base_model": "flux-2-klein-4b",
                "prompt": "telemetry probe",
                "width": 256,
                "height": 256,
                "steps": 20,
                "seed": 7,
                "batch_size": batch_size,
            },
        }
        cfg_path = self.tmp_path / "config.yaml"
        with open(cfg_path, "w", encoding="utf-8") as f:
            yaml.dump(cfg, f)
        out_dir = self.tmp_path / f"output_b{batch_size}"
        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])
        return out_dir

    def _generating_events(self, out_dir):
        lines = (out_dir / "telemetry.jsonl").read_text().splitlines()
        return [json.loads(l) for l in lines if json.loads(l).get("phase") == "generating"]

    def test_mock_batch_configuravel_multiplos_progressos(self):
        for batch in (1, 3):
            with self.subTest(batch=batch):
                out_dir = self._run_mock(batch)
                events = self._generating_events(out_dir)
                # 3-5 incrementos por imagem
                self.assertGreaterEqual(len(events), 3 * batch)
                self.assertLessEqual(len(events), 5 * batch)
                progs = [e["progress"] for e in events]
                self.assertEqual(progs, sorted(progs))  # monotônicos
                for p in progs:
                    self.assertGreater(p, 0.55)
                    self.assertLessEqual(p, 0.90)
                for e in events:
                    self.assertIn("step", e["phaseMessage"])
                # step/totalSteps = contador de IMAGENS
                steps = sorted({e["step"] for e in events})
                self.assertEqual(steps, list(range(batch)))
                for e in events:
                    self.assertEqual(e["totalSteps"], batch)

    def test_mock_substeps_constante_no_intervalo(self):
        self.assertGreaterEqual(_MOCK_TELEMETRY_SUBSTEPS, 3)
        self.assertLessEqual(_MOCK_TELEMETRY_SUBSTEPS, 5)


if __name__ == "__main__":
    unittest.main()
