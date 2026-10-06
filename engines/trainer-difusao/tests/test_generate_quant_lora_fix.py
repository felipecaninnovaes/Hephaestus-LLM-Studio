"""Regressões da geração: quantização no Klein (era no-op) e LoRAs da engine (0 chaves).

Bug 1: `quantization` nunca chegava ao Flux2KleinPipeline (transformer + Qwen3 em bf16).
Bug 2: adapter salvo pela engine (chaves sem prefixo de componente) carregava 0 chaves
no diffusers (só um warning) e a geração rodava sem a LoRA.
Tudo CPU/ENGINE_MOCK=1 (loaders mockados, modelos tiny reais p/ LoRA).
"""
from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

import pytest

torch = pytest.importorskip("torch")
pytest.importorskip("safetensors")
from safetensors.torch import load_file, save_file  # noqa: E402

from trainer_difusao.generation import runner  # noqa: E402
from trainer_difusao.generation.adapters import (  # noqa: E402
    DaemonLoraCache,
    LoraLoadError,
    prepare_lora_for_load,
)
from trainer_difusao.generation.pipelines import pipeline_cache_key  # noqa: E402
from trainer_difusao.generation.text_encoder import (  # noqa: E402
    load_flux2_quantized_components,
)
from trainer_difusao.loaders.quant_cache import resolve_quant_base_dir  # noqa: E402
from trainer_difusao.serve_pkg.state import _make_spec, _spec_matches  # noqa: E402

REPO = "black-forest-labs/FLUX.2-klein-base-4B"


# --------------------------------------------------------------------------- Bug 1
class TestQuantReachesKleinComponents(unittest.TestCase):
    def _run(self, quant, **kw):
        tf, te, tok = mock.MagicMock(), mock.MagicMock(), mock.MagicMock()
        torchao_sentinel = object()
        with (
            mock.patch(
                "trainer_difusao.loaders.transformer_loader.load_or_quantize_transformer",
                return_value=tf,
            ) as m_tf,
            mock.patch(
                "trainer_difusao.loaders.text_encoder_loader.load_or_quantize_text_encoder",
                return_value=(te, tok),
            ) as m_te,
            mock.patch(
                "trainer_difusao.common_pkg.train_config._build_intx_torchao_config",
                return_value=torchao_sentinel,
            ),
        ):
            out = load_flux2_quantized_components(
                REPO, quant, torch.bfloat16, **kw
            )
        return out, m_tf, m_te, (tf, te, tok), torchao_sentinel

    def test_each_quant_value_reaches_transformer_and_text_encoder(self):
        for quant in ("4bit", "8bit", "2bit", "6bit"):
            with self.subTest(quant=quant):
                out, m_tf, m_te, (tf, te, tok), sentinel = self._run(quant)
                self.assertIs(out["transformer"], tf)
                self.assertIs(out["text_encoder"], te)
                self.assertIs(out["tokenizer"], tok)
                for m in (m_tf, m_te):
                    kw = m.call_args.kwargs
                    self.assertEqual(kw["quant_format"], quant)
                    self.assertEqual(kw["model_id"], REPO)
                    if quant in ("4bit", "8bit"):
                        bnb = kw["bnb_config"]
                        self.assertIsNotNone(bnb)
                        self.assertEqual(bnb.load_in_4bit, quant == "4bit")
                        self.assertEqual(bnb.load_in_8bit, quant == "8bit")
                        self.assertIsNone(kw["torchao_quant_cfg"])
                    else:
                        self.assertIsNone(kw["bnb_config"])
                        self.assertIs(kw["torchao_quant_cfg"], sentinel)

    def test_uses_training_quant_cache_location(self):
        """Mesma convenção do treino: resolve_quant_base_dir(model, quant, ...)."""
        out, m_tf, m_te, *_ = self._run("4bit")
        expected = resolve_quant_base_dir(model_id=REPO, quant_format="4bit")
        self.assertEqual(m_tf.call_args.kwargs["quant_base"], expected)
        self.assertEqual(m_tf.call_args.kwargs["transformer_cache_dir"], expected / "transformer")
        self.assertEqual(m_te.call_args.kwargs["quant_base"], expected)
        self.assertEqual(m_te.call_args.kwargs["text_encoder_cache_dir"], expected / "text_encoder")

    def test_text_encoder_path_isolates_cache_and_reaches_loader(self):
        out, m_tf, m_te, *_ = self._run("4bit", text_encoder_path="/enc/custom.safetensors")
        self.assertEqual(m_te.call_args.kwargs["text_encoder_path"], "/enc/custom.safetensors")
        expected = resolve_quant_base_dir(
            model_id=REPO, quant_format="4bit", text_encoder_path="/enc/custom.safetensors"
        )
        self.assertEqual(m_tf.call_args.kwargs["quant_base"], expected)
        self.assertNotEqual(
            expected, resolve_quant_base_dir(model_id=REPO, quant_format="4bit")
        )

    def test_custom_checkpoint_without_cache_uses_quantized_single_file(self):
        with tempfile.TemporaryDirectory() as td:
            cp = Path(td) / "custom.safetensors"
            cp.write_bytes(b"0" * 16)
            tf = mock.MagicMock()
            with (
                mock.patch(
                    "trainer_difusao.generation.text_encoder._load_flux2_custom_transformer",
                    return_value=tf,
                ) as m_single,
                mock.patch(
                    "trainer_difusao.loaders.transformer_loader.load_or_quantize_transformer"
                ) as m_tf,
                mock.patch(
                    "trainer_difusao.loaders.text_encoder_loader.load_or_quantize_text_encoder",
                    return_value=(mock.MagicMock(), mock.MagicMock()),
                ),
                mock.patch(
                    "trainer_difusao.loaders.quant_cache._is_cache_valid", return_value=False
                ),
            ):
                out = load_flux2_quantized_components(
                    REPO, "4bit", torch.bfloat16, custom_cp=str(cp)
                )
        self.assertIs(out["transformer"], tf)
        m_tf.assert_not_called()
        self.assertTrue(m_single.call_args.kwargs["quantization_config"].load_in_4bit)

    def test_unquantized_component_aborts_instead_of_silent_bf16(self):
        plain = mock.Mock(spec=["modules", "parameters"])  # sem hf_quantizer/is_quantized
        with (
            mock.patch(
                "trainer_difusao.loaders.transformer_loader.load_or_quantize_transformer",
                return_value=plain,
            ),
            mock.patch("trainer_difusao.loaders.text_encoder_loader.load_or_quantize_text_encoder"),
            self.assertRaises(SystemExit),
        ):
            load_flux2_quantized_components(REPO, "4bit", torch.bfloat16)

    def test_none_is_rejected_by_helper(self):
        with self.assertRaises(SystemExit):
            load_flux2_quantized_components(REPO, "none", torch.bfloat16)


class _Stop(Exception):
    pass


class TestRunnerKleinQuantWiring(unittest.TestCase):
    """_real_generate: quant ≠ none → componentes quantizados no from_pretrained."""

    def _params(self, **over):
        p = {
            "base_model": "flux-2-klein-4b",
            "prompt": "x",
            "negative_prompt": "",
            "width": 512,
            "height": 512,
            "steps": 2,
            "guidance_scale": 4.0,
            "quantization": "4bit",
            "distilled": False,
            "batch_size": 1,
            "loras": [],
            "seed": 1,
            "custom_checkpoint_path": None,
            "arch": None,
            "text_encoder_path": None,
        }
        p.update(over)
        return p

    def _generate(self, params, *, cuda=True):
        captured = {}

        def fake_from_pretrained(repo, **kwargs):
            captured["repo"] = repo
            captured["kwargs"] = kwargs
            raise _Stop()

        fake_cls = mock.MagicMock()
        fake_cls.from_pretrained.side_effect = fake_from_pretrained
        quant_sentinel = {
            "transformer": object(),
            "text_encoder": object(),
            "tokenizer": object(),
        }
        with tempfile.TemporaryDirectory() as td, (
            mock.patch("torch.cuda.is_available", return_value=cuda)
        ), mock.patch("torch.cuda.is_bf16_supported", return_value=True), mock.patch.object(
            runner, "_setup_cache_dir", return_value="/tmp/hub"
        ), mock.patch(
            "diffusers.Flux2KleinPipeline", fake_cls
        ), mock.patch.object(
            runner, "load_flux2_quantized_components", return_value=dict(quant_sentinel)
        ) as m_quant:
            with self.assertRaises(SystemExit):  # _die ao falhar o from_pretrained fake
                runner._real_generate(params, Path(td))
        return captured, m_quant, quant_sentinel

    def test_each_quant_value_loads_quantized_components(self):
        for quant in ("4bit", "8bit", "2bit", "6bit"):
            with self.subTest(quant=quant):
                captured, m_quant, sentinel = self._generate(self._params(quantization=quant))
                self.assertEqual(m_quant.call_args.args[:2], (REPO, quant))
                for k, v in sentinel.items():
                    self.assertIs(captured["kwargs"][k], v)

    def test_distilled_repo_and_custom_paths_forwarded(self):
        captured, m_quant, _ = self._generate(
            self._params(
                distilled=True,
                custom_checkpoint_path="/m/custom.safetensors",
                arch="flux-2-klein-4b",
                text_encoder_path="/m/enc.safetensors",
            )
        )
        self.assertNotEqual(m_quant.call_args.args[0], REPO)
        self.assertEqual(m_quant.call_args.kwargs["custom_cp"], "/m/custom.safetensors")
        self.assertEqual(m_quant.call_args.kwargs["text_encoder_path"], "/m/enc.safetensors")

    def test_none_keeps_bf16_path(self):
        captured, m_quant, _ = self._generate(self._params(quantization="none"))
        m_quant.assert_not_called()
        self.assertEqual(set(captured["kwargs"]), {"torch_dtype"})


class TestDaemonKeyIncludesQuant(unittest.TestCase):
    def test_pipeline_cache_key_and_spec_differ_per_quant(self):
        keys, specs = set(), []
        for q in ("none", "2bit", "4bit", "6bit", "8bit"):
            p = {"base_model": "flux-2-klein-4b", "quantization": q, "distilled": False}
            keys.add(pipeline_cache_key(p))
            specs.append(_make_spec(p))
        self.assertEqual(len(keys), 5)
        for i, a in enumerate(specs):
            for j, b in enumerate(specs):
                self.assertEqual(_spec_matches(a, b), i == j)


# --------------------------------------------------------------------------- Bug 2
def _write(path: Path, keys: dict[str, tuple[int, ...]] | list[str]) -> str:
    items = keys if isinstance(keys, dict) else {k: (2, 2) for k in keys}
    save_file({k: torch.ones(*shape) for k, shape in items.items()}, str(path))
    return str(path)


class TestPrepareLoraForLoad(unittest.TestCase):
    def setUp(self):
        self._td = tempfile.TemporaryDirectory()
        self.tmp = Path(self._td.name)

    def tearDown(self):
        self._td.cleanup()

    def test_engine_layout_flux_gets_transformer_prefix(self):
        f = _write(
            self.tmp / "a.safetensors",
            ["transformer_blocks.0.attn.to_q.lora_A.weight", "transformer_blocks.0.attn.to_q.lora_B.weight"],
        )
        out = prepare_lora_for_load(f, "flux-2-klein-4b")
        self.assertEqual(
            sorted(out),
            [
                "transformer.transformer_blocks.0.attn.to_q.lora_A.weight",
                "transformer.transformer_blocks.0.attn.to_q.lora_B.weight",
            ],
        )

    def test_engine_layout_sd_gets_unet_prefix(self):
        f = _write(self.tmp / "s.safetensors", ["down_blocks.0.attn1.to_k.lora_A.weight"])
        for arch in ("sdxl", "sd15"):
            self.assertEqual(
                list(prepare_lora_for_load(f, arch)), ["unet.down_blocks.0.attn1.to_k.lora_A.weight"]
            )

    def test_already_prefixed_and_external_formats_pass_through(self):
        cases = {
            "diffusers": ["transformer.x.lora_A.weight"],
            "unet": ["unet.x.lora_A.weight"],
            "comfy": ["diffusion_model.x.lora_A.weight"],
            "peft": ["base_model.model.x.lora_A.weight"],
            "kohya": ["lora_unet_down_blocks_0.lora_down.weight", "lora_unet_down_blocks_0.alpha"],
        }
        for name, keys in cases.items():
            with self.subTest(fmt=name):
                f = _write(self.tmp / f"{name}.safetensors", keys)
                self.assertEqual(prepare_lora_for_load(f, "flux-2-klein-4b"), f)

    def test_file_without_lora_tensors_fails_naming_file(self):
        f = _write(self.tmp / "nolora.safetensors", ["some.weight"])
        with self.assertRaises(LoraLoadError) as cm:
            prepare_lora_for_load(f, "flux-2-klein-4b")
        self.assertIn(f, str(cm.exception))

    def test_unreadable_safetensors_fails_naming_file(self):
        bad = self.tmp / "bad.safetensors"
        bad.write_bytes(b"not-a-safetensors")
        with self.assertRaises(LoraLoadError) as cm:
            prepare_lora_for_load(str(bad), "flux-2-klein-4b")
        self.assertIn(str(bad), str(cm.exception))


class _FakeLoraPipe:
    """Pipe fake que registra o que chegou em load_lora_weights."""

    def __init__(self):
        self.loaded = []
        self.scales = None
        self.active = []
        self.transformer = self

    def load_lora_weights(self, src, adapter_name):
        self.loaded.append((src, adapter_name))

    def set_adapters(self, names, scales=None):
        self.active, self.scales = list(names), list(scales or [])

    def enable_lora(self):
        pass

    def delete_adapters(self, name):
        pass


class TestApplyLorasRemapAndScales(unittest.TestCase):
    def setUp(self):
        self._td = tempfile.TemporaryDirectory()
        self.tmp = Path(self._td.name)

    def tearDown(self):
        self._td.cleanup()

    def test_multi_lora_remapped_and_scales_preserved(self):
        eng = _write(self.tmp / "eng.safetensors", ["blocks.0.to_q.lora_A.weight"])
        ext = _write(self.tmp / "ext.safetensors", ["transformer.blocks.0.to_q.lora_A.weight"])
        pipe = _FakeLoraPipe()
        DaemonLoraCache(capacity=4).apply_loras(
            pipe,
            [{"path": eng, "scale": 0.7}, {"path": ext, "scale": 1.3}],
            "flux-2-klein-4b",
        )
        (first, _), (second, _) = pipe.loaded
        self.assertIsInstance(first, dict)
        self.assertTrue(all(k.startswith("transformer.") for k in first))
        self.assertEqual(second, ext)  # passthrough: caminho original
        self.assertEqual(pipe.scales, [0.7, 1.3])

    def test_lora_without_tensors_fails_request(self):
        bad = _write(self.tmp / "bad.safetensors", ["x.weight"])
        with self.assertRaises(LoraLoadError) as cm:
            DaemonLoraCache(capacity=2).apply_loras(
                _FakeLoraPipe(), [{"path": bad, "scale": 1.0}], "flux-2-klein-4b"
            )
        self.assertIn(bad, str(cm.exception))

    def test_zero_injected_modules_fails_and_is_not_cached(self):
        """Pipeline real-like: diffusers 'carregou' mas nada foi injetado → erro."""
        f = _write(self.tmp / "z.safetensors", ["transformer.x.lora_A.weight"])

        class Comp(torch.nn.Module):
            peft_config: dict = {}

        class Pipe(_FakeLoraPipe):
            def __init__(self):
                super().__init__()
                self.components = {"transformer": Comp()}

        cache = DaemonLoraCache(capacity=2)
        with self.assertRaises(LoraLoadError) as cm:
            cache.apply_loras(Pipe(), [{"path": f, "scale": 1.0}], "flux-2-klein-4b")
        self.assertIn(f, str(cm.exception))
        self.assertEqual(cache.adapters, {})
        self.assertEqual(cache.key_to_name, {})


class TestFailedLoadRestoresOffload(unittest.TestCase):
    """diffusers remove os hooks de offload antes de injetar e não os recoloca se falhar."""

    def _fail_pipe(self):
        class Pipe(_FakeLoraPipe):
            restored = 0

            def load_lora_weights(self, src, adapter_name):
                raise ValueError("size mismatch")

            def enable_model_cpu_offload(self):
                type(self).restored += 1

        return Pipe()

    def test_offload_reenabled_and_error_names_file(self):
        with tempfile.TemporaryDirectory() as td:
            f = _write(Path(td) / "m.safetensors", ["transformer.x.lora_A.weight"])
            pipe = self._fail_pipe()
            with mock.patch(
                "trainer_difusao.generation.adapters._is_model_cpu_offloaded",
                return_value=True,
            ), self.assertRaises(LoraLoadError) as cm:
                DaemonLoraCache(capacity=2).apply_loras(
                    pipe, [{"path": f, "scale": 1.0}], "flux-2-klein-4b"
                )
            self.assertIn(f, str(cm.exception))
            self.assertEqual(type(pipe).restored, 1)

    def test_no_reenable_without_offload(self):
        with tempfile.TemporaryDirectory() as td:
            f = _write(Path(td) / "m.safetensors", ["transformer.x.lora_A.weight"])
            pipe = self._fail_pipe()
            with self.assertRaises(LoraLoadError):
                DaemonLoraCache(capacity=2).apply_loras(
                    pipe, [{"path": f, "scale": 1.0}], "flux-2-klein-4b"
                )
            self.assertEqual(type(pipe).restored, 0)


def _tiny_flux():
    from diffusers import Flux2Transformer2DModel

    return Flux2Transformer2DModel(
        patch_size=1, in_channels=16, num_layers=1, num_single_layers=1,
        attention_head_dim=8, num_attention_heads=2, joint_attention_dim=16,
        timestep_guidance_channels=16, axes_dims_rope=(2, 2, 2, 2),
    )


def _tiny_unet():
    from diffusers import UNet2DConditionModel

    return UNet2DConditionModel(
        sample_size=8, in_channels=4, out_channels=4, layers_per_block=1,
        block_out_channels=(8, 16),
        down_block_types=("CrossAttnDownBlock2D", "DownBlock2D"),
        up_block_types=("UpBlock2D", "CrossAttnUpBlock2D"),
        cross_attention_dim=8, attention_head_dim=2, norm_num_groups=4,
    )


class TestEngineSavedLoraAppliesToRealModels(unittest.TestCase):
    """Roundtrip real: _save_lora_safetensors → apply_loras → módulos injetados + pesos iguais."""

    def setUp(self):
        pytest.importorskip("diffusers")
        pytest.importorskip("peft")
        self._td = tempfile.TemporaryDirectory()
        self.tmp = Path(self._td.name)

    def tearDown(self):
        self._td.cleanup()

    def _train_and_save(self, model, targets, name):
        from peft import LoraConfig, get_peft_model

        from trainer_difusao.common_pkg.lora_io import _save_lora_safetensors

        pm = get_peft_model(
            model,
            LoraConfig(r=4, lora_alpha=4, init_lora_weights="gaussian", target_modules=targets),
        )
        out = self.tmp / name
        _save_lora_safetensors(pm, out, {})
        return str(out)

    def _lora_a(self, module, adapter):
        return {
            n: p.detach().clone()
            for n, p in module.named_parameters()
            if f"lora_A.{adapter}" in n
        }

    def test_flux2_klein_engine_lora_applies_with_scale(self):
        from diffusers import Flux2KleinPipeline

        targets = ["to_k", "to_q", "to_v", "to_out.0", "add_k_proj", "add_v_proj",
                   "add_q_proj", "to_add_out", "to_qkv_mlp_proj", "to_out_mlp_proj",
                   "linear1", "linear2"]
        path = self._train_and_save(_tiny_flux(), targets, "k.safetensors")
        saved = load_file(path)
        self.assertFalse(any(k.startswith("transformer.") for k in saved))  # formato preservado

        pipe = Flux2KleinPipeline(
            scheduler=None, vae=None, text_encoder=None, tokenizer=None,
            transformer=_tiny_flux(),
        )
        DaemonLoraCache(capacity=2).apply_loras(
            pipe, [{"path": path, "scale": 0.5}], "flux-2-klein-4b"
        )
        self.assertEqual(pipe.get_list_adapters(), {"transformer": ["cached_lora_1"]})
        loaded = self._lora_a(pipe.transformer, "cached_lora_1")
        self.assertEqual(len(loaded), sum(k.endswith("lora_A.weight") for k in saved))
        for n, t in loaded.items():
            src = n.replace(".lora_A.cached_lora_1.weight", ".lora_A.weight")
            self.assertTrue(torch.equal(t, saved[src]), n)

    def test_sd15_sdxl_engine_lora_applies(self):
        from diffusers import StableDiffusionPipeline

        path = self._train_and_save(
            _tiny_unet(), ["to_k", "to_q", "to_v", "to_out.0"], "sd.safetensors"
        )
        for arch in ("sd15", "sdxl"):
            with self.subTest(arch=arch):
                pipe = StableDiffusionPipeline(
                    vae=None, text_encoder=None, tokenizer=None, unet=_tiny_unet(),
                    scheduler=None, safety_checker=None, feature_extractor=None,
                    requires_safety_checker=False,
                )
                DaemonLoraCache(capacity=2).apply_loras(
                    pipe, [{"path": path, "scale": 1.0}], arch
                )
                self.assertEqual(pipe.get_list_adapters(), {"unet": ["cached_lora_1"]})

    def test_unmatched_modules_lora_fails_naming_file(self):
        from diffusers import Flux2KleinPipeline

        bad = _write(self.tmp / "z.safetensors", ["foo.bar.lora_A.weight", "foo.bar.lora_B.weight"])
        pipe = Flux2KleinPipeline(
            scheduler=None, vae=None, text_encoder=None, tokenizer=None,
            transformer=_tiny_flux(),
        )
        with self.assertRaises(LoraLoadError) as cm:
            DaemonLoraCache(capacity=2).apply_loras(
                pipe, [{"path": bad, "scale": 1.0}], "flux-2-klein-4b"
            )
        self.assertIn(bad, str(cm.exception))
        self.assertEqual(pipe.get_list_adapters(), {})

    def test_with_then_without_lora_no_residual(self):
        """Daemon: com LoRA → sem LoRA (saída base) → com LoRA de novo (cache hit, sem reload)."""
        from diffusers import Flux2KleinPipeline

        path = self._train_and_save(_tiny_flux(), ["to_q", "to_k", "to_v"], "k.safetensors")
        pipe = Flux2KleinPipeline(
            scheduler=None, vae=None, text_encoder=None, tokenizer=None,
            transformer=_tiny_flux(),
        )
        cache = DaemonLoraCache(capacity=2)
        cache.apply_loras(pipe, [{"path": path, "scale": 1.0}], "flux-2-klein-4b")
        self.assertEqual(pipe.transformer.active_adapters(), ["cached_lora_1"])
        cache.apply_loras(pipe, [], "flux-2-klein-4b")
        self.assertFalse(getattr(pipe.transformer, "_hf_peft_config_loaded", True) and
                         pipe.transformer.active_adapters())
        cache.apply_loras(pipe, [{"path": path, "scale": 1.0}], "flux-2-klein-4b")
        self.assertEqual(pipe.transformer.active_adapters(), ["cached_lora_1"])
        self.assertEqual(len(cache.adapters), 1)


class TestOneShotLoraFailure(unittest.TestCase):
    def test_cmd_generate_fails_explicitly_on_lora_error(self):
        import yaml

        with tempfile.TemporaryDirectory() as td:
            cfg = Path(td) / "c.yaml"
            cfg.write_text(
                yaml.safe_dump(
                    {
                        "job_id": "j",
                        "mode": "generate",
                        "model": "flux-2-klein-4b",
                        "generate": {"prompt": "x", "base_model": "flux-2-klein-4b"},
                    }
                )
            )
            with (
                mock.patch.object(runner, "is_mock", return_value=False),
                mock.patch.object(
                    runner, "_real_generate", side_effect=LoraLoadError("LoRA /x/y.safetensors 0 chaves")
                ),
                self.assertRaises(SystemExit) as cm,
            ):
                runner.cmd_generate(["--config", str(cfg), "--output", td])
        self.assertEqual(cm.exception.code, 1)


if __name__ == "__main__":
    unittest.main()
