"""Fatia flux2-motor-treino (EngGen): sampler/upscale/quant + anti-contaminacao.

Usa os fixtures/estilo dos testes existentes (ENGINE_MOCK=1, unittest).
"""

import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import yaml

from trainer_difusao.generate import (
    _build_generation_meta,
    _real_generate,
    load_and_validate_generate_config,
    pipeline_cache_key,
)
from trainer_difusao.train import main


class _Base(unittest.TestCase):
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

    def _base_cfg(self, **overrides) -> dict:
        base = {
            "job_id": "test-enggen-001",
            "generate": {
                "base_model": "flux-2-klein-4b",
                "prompt": "uma forja ao amanhecer",
                "width": 256,
                "height": 256,
                "steps": 4,
                "seed": 7,
                "quantization": "none",
            },
        }
        base["generate"].update(overrides)
        return base


class TestSamplerValidation(_Base):
    """(a) sampler: default implicito; flux rejeita SD-only; SD aceita tudo."""

    def test_default_sampler(self):
        params = load_and_validate_generate_config(self._base_cfg())
        self.assertEqual(params["sampler"], "default")

    def test_flux_rejects_sd_sampler(self):
        with self.assertRaises(SystemExit):
            load_and_validate_generate_config(
                self._base_cfg(base_model="flux-2-klein-4b", sampler="dpmpp_2m")
            )

    def test_flux_accepts_flow_match(self):
        for s in ("default", "euler", "heun"):
            params = load_and_validate_generate_config(
                self._base_cfg(base_model="flux-2-klein-4b", sampler=s)
            )
            self.assertEqual(params["sampler"], s)

    def test_sd_accepts_all(self):
        from trainer_difusao.schedulers import SAMPLER_CHOICES

        for s in SAMPLER_CHOICES:
            params = load_and_validate_generate_config(
                self._base_cfg(base_model="sdxl", sampler=s)
            )
            self.assertEqual(params["sampler"], s)

    def test_unknown_sampler_fails(self):
        with self.assertRaises(SystemExit):
            load_and_validate_generate_config(
                self._base_cfg(base_model="sdxl", sampler="bagre")
            )


class TestUpscaleQuantValidation(_Base):
    """(a) upscale/quant: casos invalidos -> SystemExit; validos passam."""

    def test_upscale_absent_is_none(self):
        params = load_and_validate_generate_config(self._base_cfg())
        self.assertIsNone(params["upscale"])

    def test_upscale_valid(self):
        params = load_and_validate_generate_config(
            self._base_cfg(upscale={"model": "4x", "scale": 2})
        )
        self.assertEqual(params["upscale"], {"model": "4x", "scale": 2})

    def test_upscale_all_models_valid(self):
        for model in ("4x", "ultrasharp", "siax"):
            params = load_and_validate_generate_config(
                self._base_cfg(upscale={"model": model, "scale": 2})
            )
            self.assertEqual(params["upscale"], {"model": model, "scale": 2})

    def test_upscale_9x_fails(self):
        with self.assertRaises(SystemExit):
            load_and_validate_generate_config(
                self._base_cfg(upscale={"model": "9x", "scale": 4})
            )

    def test_upscale_bad_model_fails(self):
        with self.assertRaises(SystemExit):
            load_and_validate_generate_config(
                self._base_cfg(upscale={"model": "8x", "scale": 4})
            )

    def test_upscale_bad_scale_fails(self):
        with self.assertRaises(SystemExit):
            load_and_validate_generate_config(
                self._base_cfg(upscale={"model": "4x", "scale": 3})
            )

    def test_upscale_not_dict_fails(self):
        with self.assertRaises(SystemExit):
            load_and_validate_generate_config(self._base_cfg(upscale="4x"))

    def test_quant_levels_valid(self):
        for q in ("none", "2bit", "4bit", "6bit", "8bit"):
            params = load_and_validate_generate_config(self._base_cfg(quantization=q))
            self.assertEqual(params["quantization"], q)

    def test_quant_invalid_fails(self):
        with self.assertRaises(SystemExit):
            load_and_validate_generate_config(self._base_cfg(quantization="16bit"))


class TestSchedulerMapping(unittest.TestCase):
    """build_scheduler: classes corretas + from_config + default=None."""

    def test_default_is_none(self):
        from trainer_difusao.schedulers import build_scheduler

        self.assertIsNone(build_scheduler("default", "sd", {}))

    def test_sd_mapping(self):
        from diffusers import (
            DDIMScheduler,
            DPMSolverMultistepScheduler,
            EulerAncestralDiscreteScheduler,
            EulerDiscreteScheduler,
            HeunDiscreteScheduler,
        )

        from trainer_difusao.schedulers import build_scheduler

        base = dict(EulerDiscreteScheduler().config)
        cases = {
            "euler": EulerDiscreteScheduler,
            "euler_a": EulerAncestralDiscreteScheduler,
            "heun": HeunDiscreteScheduler,
            "dpmpp_2m": DPMSolverMultistepScheduler,
            "dpmpp_2m_karras": DPMSolverMultistepScheduler,
            "dpmpp_2m_sde": DPMSolverMultistepScheduler,
            "dpmpp_2m_sde_karras": DPMSolverMultistepScheduler,
            "ddim": DDIMScheduler,
        }
        for sampler, cls in cases.items():
            with self.subTest(sampler=sampler):
                self.assertIsInstance(build_scheduler(sampler, "sd", base), cls)

    def test_dpmpp_sde_mapping(self):
        # DPMSolverSDEScheduler exige torchsde (só @gpu): no dev sem torchsde
        # o diffusers levanta ImportError honesto (job falha, nunca silencioso).
        from diffusers import EulerDiscreteScheduler

        from trainer_difusao.schedulers import build_scheduler

        base = dict(EulerDiscreteScheduler().config)
        try:
            import torchsde  # noqa: F401
        except ImportError:
            with self.assertRaises(ImportError):
                build_scheduler("dpmpp_sde", "sd", base)
            return
        from diffusers import DPMSolverSDEScheduler

        self.assertIsInstance(
            build_scheduler("dpmpp_sde", "sd", base), DPMSolverSDEScheduler
        )
        from diffusers import EulerDiscreteScheduler

        from trainer_difusao.schedulers import build_scheduler

        base = dict(EulerDiscreteScheduler().config)
        plain = build_scheduler("dpmpp_2m", "sd", base)
        self.assertEqual(plain.config.algorithm_type, "dpmsolver++")
        karras = build_scheduler("dpmpp_2m_karras", "sd", base)
        self.assertTrue(karras.config.use_karras_sigmas)
        sde = build_scheduler("dpmpp_2m_sde", "sd", base)
        self.assertEqual(sde.config.algorithm_type, "sde-dpmsolver++")
        sde_k = build_scheduler("dpmpp_2m_sde_karras", "sd", base)
        self.assertEqual(sde_k.config.algorithm_type, "sde-dpmsolver++")
        self.assertTrue(sde_k.config.use_karras_sigmas)

    def test_flux_mapping(self):
        from diffusers import (
            FlowMatchEulerDiscreteScheduler,
            FlowMatchHeunDiscreteScheduler,
        )

        from trainer_difusao.schedulers import build_scheduler

        base = dict(FlowMatchEulerDiscreteScheduler().config)
        self.assertIsInstance(
            build_scheduler("euler", "flux", base), FlowMatchEulerDiscreteScheduler
        )
        self.assertIsInstance(
            build_scheduler("heun", "flux", base), FlowMatchHeunDiscreteScheduler
        )
        with self.assertRaises(ValueError):
            build_scheduler("dpmpp_2m", "flux", base)


class TestSchedulerNoContamination(unittest.TestCase):
    """(b) swapped_scheduler restaura o original inclusive em excecao."""

    def _pipe(self):
        from diffusers import EulerDiscreteScheduler

        return mock.MagicMock(
            scheduler=EulerDiscreteScheduler(), components={"scheduler": 1}
        )

    def test_restore_after_use(self):
        from trainer_difusao.schedulers import build_scheduler, swapped_scheduler

        pipe = self._pipe()
        orig = pipe.scheduler
        fresh = build_scheduler("dpmpp_2m", "sd", dict(orig.config))
        self.assertIsNot(fresh, orig)
        with swapped_scheduler(pipe, fresh):
            self.assertIs(pipe.scheduler, fresh)
        self.assertIs(pipe.scheduler, orig)

    def test_restore_on_exception(self):
        from trainer_difusao.schedulers import build_scheduler, swapped_scheduler

        pipe = self._pipe()
        orig = pipe.scheduler
        fresh = build_scheduler("euler", "sd", dict(orig.config))
        with self.assertRaises(RuntimeError), swapped_scheduler(pipe, fresh):
            raise RuntimeError("cancel simulado")
        self.assertIs(pipe.scheduler, orig)

    def test_real_generate_restores_cached_pipe(self):
        """_real_generate com pipe stub: scheduler original intacto apos request."""
        import torch
        from PIL import Image

        params = load_and_validate_generate_config(
            {
                "job_id": "sched-restore",
                "generate": {
                    "base_model": "sdxl",
                    "prompt": "teste",
                    "width": 256,
                    "height": 256,
                    "steps": 2,
                    "seed": 1,
                    "quantization": "none",
                    "sampler": "dpmpp_2m",
                },
            }
        )
        from diffusers import EulerDiscreteScheduler

        orig_scheduler = EulerDiscreteScheduler()

        class _Out:
            def __init__(self, img):
                self.images = [img]

        class _StubPipe:
            def __init__(self):
                self.scheduler = orig_scheduler
                self.components = {"scheduler": orig_scheduler}

            def __call__(self, **kwargs):
                return _Out(Image.new("RGB", (256, 256), (10, 10, 10)))

        stub = _StubPipe()
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(
            torch.cuda, "is_available", return_value=False
        ):
            _real_generate(params, Path(tmp), pipeline=stub)
        self.assertIs(stub.scheduler, orig_scheduler)


class TestQuantCacheKey(unittest.TestCase):
    """(c) 6bit gera cache key distinta de 4bit (quant ja esta na chave)."""

    def test_6bit_distinct_from_4bit(self):
        p4 = {
            "base_model": "sdxl",
            "quantization": "4bit",
            "distilled": False,
            "custom_checkpoint_path": None,
        }
        p6 = dict(p4, quantization="6bit")
        self.assertNotEqual(pipeline_cache_key(p4), pipeline_cache_key(p6))

    def test_sampler_upscale_not_in_key(self):
        base = {
            "base_model": "sdxl",
            "quantization": "none",
            "distilled": False,
            "custom_checkpoint_path": None,
            "sampler": "dpmpp_2m",
            "upscale": {"model": "4x", "scale": 2},
        }
        plain = {k: v for k, v in base.items() if k not in ("sampler", "upscale")}
        self.assertEqual(pipeline_cache_key(base), pipeline_cache_key(plain))


class TestMockUpscaleDimensions(_Base):
    """(d) mock upscale muda as dimensoes do PNG conforme o scale."""

    def _run(self, cfg: dict) -> Path:
        cfg_path = self.tmp_path / "config.yaml"
        with open(cfg_path, "w", encoding="utf-8") as f:
            yaml.dump(cfg, f)
        out_dir = self.tmp_path / f"out-{cfg['generate'].get('seed', 0)}"
        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])
        return out_dir

    def test_scale2_doubles(self):
        from PIL import Image

        out = self._run(
            self._base_cfg(
                base_model="sdxl", seed=11, upscale={"model": "4x", "scale": 2}
            )
        )
        with Image.open(out / "generated_0001.png") as img:
            self.assertEqual(img.size, (512, 512))
        entry = json.loads((out / "generation_meta.json").read_text().splitlines()[0])
        self.assertEqual(entry["sampler"], "default")
        self.assertEqual(
            entry["upscale"],
            {
                "model": "4x",
                "scale": 2,
                "original_width": 256,
                "original_height": 256,
                "final_width": 512,
                "final_height": 512,
            },
        )
        payload = json.loads(
            Image.open(out / "generated_0001.png").info["hephaestus.generation"]
        )
        self.assertEqual(payload["upscale"], entry["upscale"])
        self.assertEqual(payload["sampler"], "default")

    def test_scale4_quadruples(self):
        from PIL import Image

        out = self._run(
            self._base_cfg(
                base_model="sdxl",
                seed=12,
                sampler="euler",
                upscale={"model": "4x", "scale": 4},
            )
        )
        with Image.open(out / "generated_0001.png") as img:
            self.assertEqual(img.size, (1024, 1024))
        entry = json.loads((out / "generation_meta.json").read_text().splitlines()[0])
        self.assertEqual(entry["sampler"], "euler")
        self.assertEqual(entry["upscale"]["final_width"], 1024)

    def test_no_upscale_no_key(self):
        out = self._run(self._base_cfg(base_model="sdxl", seed=13))
        entry = json.loads((out / "generation_meta.json").read_text().splitlines()[0])
        self.assertNotIn("upscale", entry)
        self.assertEqual(entry["sampler"], "default")


class TestServeSpecUnchanged(unittest.TestCase):
    """serve._make_spec/_spec_key ignoram sampler/upscale (sem reload espurio)."""

    def test_spec_ignores_sampler_upscale(self):
        from trainer_difusao.serve import _make_spec, _spec_key

        params = load_and_validate_generate_config(
            {
                "job_id": "spec-001",
                "generate": {
                    "base_model": "sdxl",
                    "prompt": "x",
                    "quantization": "none",
                    "sampler": "dpmpp_2m",
                    "upscale": {"model": "4x", "scale": 2},
                },
            }
        )
        spec_a = _make_spec(params)
        params_b = dict(params, sampler="euler", upscale=None)
        spec_b = _make_spec(params_b)
        self.assertEqual(_spec_key(spec_a), _spec_key(spec_b))
        self.assertNotIn("sampler", spec_a)
        self.assertNotIn("upscale", spec_a)


class TestMetaSamplerAlways(unittest.TestCase):
    def test_sampler_in_meta(self):
        meta = _build_generation_meta(
            params={
                "prompt": "p",
                "negative_prompt": "",
                "width": 64,
                "height": 64,
                "steps": 2,
                "guidance_scale": 7.0,
                "quantization": "none",
                "distilled": False,
                "base_model": "sdxl",
                "job_id": "j",
            },
            filename="generated_0001.png",
            thumb_filename="thumb_0001.jpg",
            seed=1,
            batch_index=0,
            batch_size=1,
            loras_effective=[],
        )
        self.assertEqual(meta["sampler"], "default")
        self.assertNotIn("upscale", meta)


class TestCustomWeightsQuantGuard(_Base):
    """C2: quantizacao aplicada a pesos custom flux-2 (sem fallback silencioso)."""

    _QUANT = object()  # sentinela p/ assercao de kwargs

    def test_custom_transformer_receives_quantization_config(self):
        from diffusers import Flux2Transformer2DModel

        cp = self.tmp_path / "flux2.safetensors"
        cp.write_bytes(b"0" * 16)
        with mock.patch.object(
            Flux2Transformer2DModel, "from_single_file", return_value=mock.Mock()
        ) as m:
            from trainer_difusao.generate import _load_flux2_custom_transformer

            _load_flux2_custom_transformer(
                str(cp), "float32", quantization_config=self._QUANT
            )
        self.assertIs(m.call_args.kwargs["quantization_config"], self._QUANT)

    def test_custom_transformer_without_quant_unchanged(self):
        from diffusers import Flux2Transformer2DModel

        cp = self.tmp_path / "flux2.safetensors"
        cp.write_bytes(b"0" * 16)
        with mock.patch.object(
            Flux2Transformer2DModel, "from_single_file", return_value=mock.Mock()
        ) as m:
            from trainer_difusao.generate import _load_flux2_custom_transformer

            _load_flux2_custom_transformer(str(cp), "float32")
        self.assertNotIn("quantization_config", m.call_args.kwargs)

    def test_encoder_dir_receives_quantization_config(self):
        import transformers
        from trainer_difusao.generate import _load_flux2_text_encoder_override

        enc_dir = self.tmp_path / "enc"
        enc_dir.mkdir()
        (enc_dir / "config.json").write_text("{}")
        enc = mock.Mock()
        with mock.patch.object(
            transformers, "AutoModelForCausalLM"
        ) as m_enc, mock.patch.object(transformers, "AutoTokenizer"):
            m_enc.from_pretrained.return_value = enc
            _load_flux2_text_encoder_override(
                str(enc_dir), "repo/x", "float32",
                quantization_config=self._QUANT,
            )
        self.assertIs(
            m_enc.from_pretrained.call_args.kwargs["quantization_config"],
            self._QUANT,
        )

    def test_encoder_loose_file_with_quant_uses_merge_cache(self):
        """(a) arquivo solto + quant → merge em disco + carga quantizada.

        from_pretrained chamado com quantization_config sobre o merged dir,
        state_dict aplicado exatamente 1x, metadata com md5 correto.
        """
        import json
        import transformers
        from trainer_difusao.generate import _load_flux2_text_encoder_override

        import trainer_difusao.common as _common

        cache_root = self.tmp_path / "enc-cache"
        old_cache = os.environ.get("TEXT_ENCODER_CUSTOM_CACHE")
        os.environ["TEXT_ENCODER_CUSTOM_CACHE"] = str(cache_root)
        try:
            f = self.tmp_path / "enc.safetensors"
            f.write_bytes(b"custom-encoder-payload" * 64)
            merged_dir, md5 = _common._custom_text_encoder_merge_dir(str(f))
            base_enc = mock.Mock()
            base_enc.load_state_dict.return_value = ([], [])
            base_enc.save_pretrained.side_effect = (
                lambda p: Path(p).mkdir(parents=True, exist_ok=True)
            )
            merged_enc = mock.Mock()
            quant_enc = mock.Mock()

            calls = {"pretrained": []}

            def _fake_pretrained(pretrained_path, **kwargs):
                calls["pretrained"].append((pretrained_path, kwargs))
                if pretrained_path == "repo/x":
                    return base_enc
                self.assertEqual(pretrained_path, str(merged_dir))
                self.assertIs(kwargs.get("quantization_config"), self._QUANT)
                if not (merged_dir / "metadata.json").exists():
                    return merged_enc
                return quant_enc

            with mock.patch.object(
                transformers, "AutoModelForCausalLM"
            ) as m_enc, mock.patch.object(
                transformers, "AutoTokenizer"
            ), mock.patch.object(
                _common, "_load_loose_text_encoder_state",
                return_value={"w": 1},
            ) as m_state:
                m_enc.from_pretrained.side_effect = _fake_pretrained
                enc, _tok = _load_flux2_text_encoder_override(
                    str(f), "repo/x", "float32",
                    quantization_config=self._QUANT,
                )
            self.assertIs(enc, quant_enc)
            m_state.assert_called_once_with(str(f))
            base_enc.load_state_dict.assert_called_once_with({"w": 1}, strict=False)
            meta = json.loads((merged_dir / "metadata.json").read_text())
            self.assertEqual(meta["md5"], md5)
            self.assertEqual(meta["basename"], "enc.safetensors")
            merge_calls = [
                c for c in calls["pretrained"] if c[0] == str(merged_dir)
            ]
            self.assertEqual(len(merge_calls), 1)
            self.assertNotIn(".tmp-", merge_calls[0][0])
        finally:
            if old_cache is None:
                os.environ.pop("TEXT_ENCODER_CUSTOM_CACHE", None)
            else:
                os.environ["TEXT_ENCODER_CUSTOM_CACHE"] = old_cache

    def test_encoder_loose_file_merge_cache_hit_skips_state_dict(self):
        """(b) segunda chamada com cache válido → NÃO re-aplica state_dict."""
        import json
        import transformers
        from trainer_difusao.generate import _load_flux2_text_encoder_override

        import trainer_difusao.common as _common

        cache_root = self.tmp_path / "enc-cache"
        old_cache = os.environ.get("TEXT_ENCODER_CUSTOM_CACHE")
        os.environ["TEXT_ENCODER_CUSTOM_CACHE"] = str(cache_root)
        try:
            f = self.tmp_path / "enc.safetensors"
            f.write_bytes(b"custom-encoder-payload" * 64)
            merged_dir, md5 = _common._custom_text_encoder_merge_dir(str(f))
            merged_dir.mkdir(parents=True, exist_ok=True)
            (merged_dir / "metadata.json").write_text(json.dumps({"md5": md5}))
            quant_enc = mock.Mock()
            with mock.patch.object(
                transformers, "AutoModelForCausalLM"
            ) as m_enc, mock.patch.object(
                transformers, "AutoTokenizer"
            ), mock.patch.object(
                _common, "_load_loose_text_encoder_state"
            ) as m_state:
                m_enc.from_pretrained.return_value = quant_enc
                enc, _tok = _load_flux2_text_encoder_override(
                    str(f), "repo/x", "float32",
                    quantization_config=self._QUANT,
                )
            self.assertIs(enc, quant_enc)
            m_state.assert_not_called()
            self.assertIs(
                m_enc.from_pretrained.call_args.kwargs["quantization_config"],
                self._QUANT,
            )
            self.assertEqual(
                m_enc.from_pretrained.call_args.args[0], str(merged_dir)
            )
        finally:
            if old_cache is None:
                os.environ.pop("TEXT_ENCODER_CUSTOM_CACHE", None)
            else:
                os.environ["TEXT_ENCODER_CUSTOM_CACHE"] = old_cache

    def test_encoder_loose_file_merge_cache_md5_mismatch_remerges(self):
        """(c) metadata com md5 divergente → refaz merge."""
        import json
        import transformers
        from trainer_difusao.generate import _load_flux2_text_encoder_override

        import trainer_difusao.common as _common

        cache_root = self.tmp_path / "enc-cache"
        old_cache = os.environ.get("TEXT_ENCODER_CUSTOM_CACHE")
        os.environ["TEXT_ENCODER_CUSTOM_CACHE"] = str(cache_root)
        try:
            f = self.tmp_path / "enc.safetensors"
            f.write_bytes(b"custom-encoder-payload" * 64)
            merged_dir, md5 = _common._custom_text_encoder_merge_dir(str(f))
            merged_dir.mkdir(parents=True, exist_ok=True)
            (merged_dir / "metadata.json").write_text(
                json.dumps({"md5": "0" * 16})
            )
            base_enc = mock.Mock()
            base_enc.load_state_dict.return_value = ([], [])
            base_enc.save_pretrained.side_effect = (
                lambda p: Path(p).mkdir(parents=True, exist_ok=True)
            )
            with mock.patch.object(
                transformers, "AutoModelForCausalLM"
            ) as m_enc, mock.patch.object(
                transformers, "AutoTokenizer"
            ), mock.patch.object(
                _common, "_load_loose_text_encoder_state",
                return_value={"w": 1},
            ) as m_state:
                m_enc.from_pretrained.side_effect = [base_enc, mock.Mock()]
                _load_flux2_text_encoder_override(
                    str(f), "repo/x", "float32",
                    quantization_config=self._QUANT,
                )
            m_state.assert_called_once_with(str(f))
            base_enc.load_state_dict.assert_called_once()
            meta = json.loads((merged_dir / "metadata.json").read_text())
            self.assertEqual(meta["md5"], md5)
        finally:
            if old_cache is None:
                os.environ.pop("TEXT_ENCODER_CUSTOM_CACHE", None)
            else:
                os.environ["TEXT_ENCODER_CUSTOM_CACHE"] = old_cache

    def test_encoder_loose_file_without_quant_unchanged(self):
        """(d) sem quant → caminho atual preservado (bf16 direto, sem cache)."""
        import transformers
        from trainer_difusao.generate import _load_flux2_text_encoder_override

        import trainer_difusao.common as _common

        cache_root = self.tmp_path / "enc-cache"
        old_cache = os.environ.get("TEXT_ENCODER_CUSTOM_CACHE")
        os.environ["TEXT_ENCODER_CUSTOM_CACHE"] = str(cache_root)
        try:
            f = self.tmp_path / "enc.safetensors"
            f.write_bytes(b"custom-encoder-payload" * 64)
            base_enc = mock.Mock()
            base_enc.load_state_dict.return_value = ([], [])
            with mock.patch.object(
                transformers, "AutoModelForCausalLM"
            ) as m_enc, mock.patch.object(
                transformers, "AutoTokenizer"
            ), mock.patch.object(
                _common, "_load_loose_text_encoder_state",
                return_value={"w": 1},
            ):
                m_enc.from_pretrained.return_value = base_enc
                enc, _tok = _load_flux2_text_encoder_override(
                    str(f), "repo/x", "float32"
                )
            self.assertIs(enc, base_enc)
            self.assertNotIn(
                "quantization_config", m_enc.from_pretrained.call_args.kwargs
            )
            self.assertFalse(cache_root.exists())
        finally:
            if old_cache is None:
                os.environ.pop("TEXT_ENCODER_CUSTOM_CACHE", None)
            else:
                os.environ["TEXT_ENCODER_CUSTOM_CACHE"] = old_cache

    def test_encoder_loose_file_unexpected_keys_dies(self):
        """(e) load_state_dict com unexpected keys → _die (sem fallback)."""
        import transformers
        from trainer_difusao.generate import _load_flux2_text_encoder_override

        import trainer_difusao.common as _common

        cache_root = self.tmp_path / "enc-cache"
        old_cache = os.environ.get("TEXT_ENCODER_CUSTOM_CACHE")
        os.environ["TEXT_ENCODER_CUSTOM_CACHE"] = str(cache_root)
        try:
            f = self.tmp_path / "enc.safetensors"
            f.write_bytes(b"custom-encoder-payload" * 64)
            base_enc = mock.Mock()
            base_enc.load_state_dict.return_value = ([], ["qwen.foo"])
            with mock.patch.object(
                transformers, "AutoModelForCausalLM"
            ) as m_enc, mock.patch.object(
                transformers, "AutoTokenizer"
            ), mock.patch.object(
                _common, "_load_loose_text_encoder_state",
                return_value={"w": 1},
            ):
                m_enc.from_pretrained.return_value = base_enc
                with self.assertRaises(SystemExit):
                    _load_flux2_text_encoder_override(
                        str(f), "repo/x", "float32"
                    )
        finally:
            if old_cache is None:
                os.environ.pop("TEXT_ENCODER_CUSTOM_CACHE", None)
            else:
                os.environ["TEXT_ENCODER_CUSTOM_CACHE"] = old_cache

    def test_encoder_persist_failure_removes_tmp_and_race_is_cache_hit(self):
        """except de persist: remove .tmp-<pid>; corrida vira cache hit."""
        import transformers
        from trainer_difusao.generate import _load_flux2_text_encoder_override

        import trainer_difusao.common as _common

        cache_root = self.tmp_path / "enc-cache"
        old_cache = os.environ.get("TEXT_ENCODER_CUSTOM_CACHE")
        os.environ["TEXT_ENCODER_CUSTOM_CACHE"] = str(cache_root)
        try:
            f = self.tmp_path / "enc.safetensors"
            f.write_bytes(b"custom-encoder-payload" * 64)
            merged_dir, md5 = _common._custom_text_encoder_merge_dir(str(f))
            merged_parent = merged_dir.parent
            fresh_quant = mock.Mock()
            with mock.patch.object(
                transformers, "AutoModelForCausalLM"
            ) as m_enc, mock.patch.object(
                transformers, "AutoTokenizer"
            ), mock.patch.object(
                _common, "_load_loose_text_encoder_state",
                return_value={"w": 1},
            ):
                base_enc = mock.Mock()
                base_enc.load_state_dict.return_value = ([], [])

                def _save_and_race(path):
                    Path(path).mkdir(parents=True, exist_ok=True)
                    merged_dir.mkdir(parents=True, exist_ok=True)
                    (merged_dir / "metadata.json").write_text(
                        json.dumps({"md5": md5})
                    )
                    raise OSError("disco cheio no os.replace")

                base_enc.save_pretrained.side_effect = _save_and_race
                m_enc.from_pretrained.side_effect = [base_enc, fresh_quant]
                enc, _tok = _load_flux2_text_encoder_override(
                    str(f), "repo/x", "float32",
                    quantization_config=self._QUANT,
                )
            self.assertIs(enc, fresh_quant)
            leftovers = list(merged_parent.glob(".tmp-*"))
            self.assertEqual(leftovers, [])
            self.assertEqual(
                m_enc.from_pretrained.call_args.args[0], str(merged_dir)
            )
            self.assertIs(
                m_enc.from_pretrained.call_args.kwargs["quantization_config"],
                self._QUANT,
            )
        finally:
            if old_cache is None:
                os.environ.pop("TEXT_ENCODER_CUSTOM_CACHE", None)
            else:
                os.environ["TEXT_ENCODER_CUSTOM_CACHE"] = old_cache

    def test_encoder_persist_failure_without_race_dies_and_removes_tmp(self):
        """except de persist sem merge válido: _die + sem .tmp órfão."""
        import transformers
        from trainer_difusao.generate import _load_flux2_text_encoder_override

        import trainer_difusao.common as _common

        cache_root = self.tmp_path / "enc-cache"
        old_cache = os.environ.get("TEXT_ENCODER_CUSTOM_CACHE")
        os.environ["TEXT_ENCODER_CUSTOM_CACHE"] = str(cache_root)
        try:
            f = self.tmp_path / "enc.safetensors"
            f.write_bytes(b"custom-encoder-payload" * 64)
            merged_dir, _md5 = _common._custom_text_encoder_merge_dir(str(f))
            merged_parent = merged_dir.parent
            with mock.patch.object(
                transformers, "AutoModelForCausalLM"
            ) as m_enc, mock.patch.object(
                transformers, "AutoTokenizer"
            ), mock.patch.object(
                _common, "_load_loose_text_encoder_state",
                return_value={"w": 1},
            ):
                base_enc = mock.Mock()
                base_enc.load_state_dict.return_value = ([], [])
                base_enc.save_pretrained.side_effect = OSError("disco cheio")
                m_enc.from_pretrained.return_value = base_enc
                with self.assertRaises(SystemExit):
                    _load_flux2_text_encoder_override(
                        str(f), "repo/x", "float32",
                        quantization_config=self._QUANT,
                    )
            self.assertEqual(list(merged_parent.glob(".tmp-*")), [])
        finally:
            if old_cache is None:
                os.environ.pop("TEXT_ENCODER_CUSTOM_CACHE", None)
            else:
                os.environ["TEXT_ENCODER_CUSTOM_CACHE"] = old_cache

    def test_encoder_merge_sweep_keeps_current_and_warns_on_failure(self):
        """sweep: expurga os mais antigos até o teto, nunca o merge atual."""
        import time

        import trainer_difusao.common as _common

        cache_root = self.tmp_path / "enc-cache"
        old_cache = os.environ.get("TEXT_ENCODER_CUSTOM_CACHE")
        old_max = os.environ.get("TEXT_ENCODER_CACHE_MAX_GB")
        os.environ["TEXT_ENCODER_CUSTOM_CACHE"] = str(cache_root)
        try:
            current = cache_root / "c-current" / "merged"
            old1 = cache_root / "a-old1" / "merged"
            old2 = cache_root / "b-old2" / "merged"
            for d in (old1, old2, current):
                d.mkdir(parents=True, exist_ok=True)
                (d / "weights.bin").write_bytes(b"x" * 1024 * 1024)
                (d / "metadata.json").write_text(json.dumps({"md5": "0" * 16}))
            now = time.time()
            os.utime(old1, (now - 300, now - 300))
            os.utime(old2, (now - 200, now - 200))
            os.utime(current, (now - 100, now - 100))
            os.environ["TEXT_ENCODER_CACHE_MAX_GB"] = str(1 / 1024)
            _common._sweep_text_encoder_merge_cache(current)
            self.assertTrue(current.exists())
            self.assertFalse(old1.exists())
            self.assertFalse(old2.exists())
            with mock.patch.object(
                _common.shutil, "rmtree", side_effect=OSError("boom")
            ):
                _common._sweep_text_encoder_merge_cache(current)
            self.assertTrue(current.exists())
        finally:
            if old_cache is None:
                os.environ.pop("TEXT_ENCODER_CUSTOM_CACHE", None)
            else:
                os.environ["TEXT_ENCODER_CUSTOM_CACHE"] = old_cache
            if old_max is None:
                os.environ.pop("TEXT_ENCODER_CACHE_MAX_GB", None)
            else:
                os.environ["TEXT_ENCODER_CACHE_MAX_GB"] = old_max

    def test_encoder_cache_slug_uses_content_md5(self):
        """slug do cache quant: md5 de conteúdo; muda com os bytes."""
        import hashlib

        import trainer_difusao.common as _common

        f = self.tmp_path / "enc.safetensors"
        f.write_bytes(b"conteudo-a" * 64)
        slug_a = _common._text_encoder_cache_slug(str(f))
        md5_a = _common._custom_text_encoder_merge_dir(str(f))[1][:12]
        self.assertEqual(slug_a, md5_a)
        f.write_bytes(b"conteudo-b" * 64)
        slug_b = _common._text_encoder_cache_slug(str(f))
        self.assertEqual(
            slug_b, _common._custom_text_encoder_merge_dir(str(f))[1][:12]
        )
        self.assertNotEqual(slug_a, slug_b)
        self.assertNotEqual(
            slug_b, hashlib.md5(str(f).encode()).hexdigest()[:12]
        )

    def test_common_keeps_typing_any_for_get_type_hints(self):
        """from typing import Any restaurado: hints de common resolvem."""
        import typing

        import trainer_difusao.common as _common

        self.assertIn("Any", vars(_common))
        hints = typing.get_type_hints(_common._load_loose_text_encoder_state)
        self.assertEqual(hints["return"], dict[str, typing.Any])



class TestEncoderOverrideSdGuard(_Base):
    """N2: text_encoder_path com checkpoint sdxl/sd15 -> SystemExit (sem engolir)."""

    def test_dies_for_sdxl(self):
        params = load_and_validate_generate_config(
            self._base_cfg(
                base_model=None,
                custom_checkpoint_path="/tmp/model.safetensors",
                arch="sdxl",
                text_encoder_path="/tmp/enc",
            )
        )
        with self.assertRaises(SystemExit):
            _real_generate(params, self.tmp_path / "out")

    def test_dies_for_sd15(self):
        params = load_and_validate_generate_config(
            self._base_cfg(
                base_model=None,
                custom_checkpoint_path="/tmp/model.safetensors",
                arch="sd15",
                text_encoder_path="/tmp/enc",
            )
        )
        with self.assertRaises(SystemExit):
            _real_generate(params, self.tmp_path / "out")


if __name__ == "__main__":
    unittest.main()
