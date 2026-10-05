"""Testes para quant_cache e daemon LoRA cache (ADR-0023)."""
import os
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from trainer_difusao.loaders.quant_cache import (
    _custom_checkpoint_identity,
    _is_cache_valid,
    _save_quant_metadata,
    get_bitsandbytes_version,
    get_quant_cache_root,
    resolve_quant_base_dir,
    save_atomic_dir,
)
from trainer_difusao.generation.adapters import DaemonLoraCache


class FakePipe:
    """Pipeline fake para testar o cache de LoRAs."""

    def __init__(self):
        self.calls = []
        self.adapters = {}
        self.active_adapters = []
        self.transformer = self

    def load_lora_weights(self, path, adapter_name):
        self.calls.append(("load_lora_weights", path, adapter_name))
        self.adapters[adapter_name] = path

    def set_adapters(self, names, scales=None):
        self.calls.append(("set_adapters", list(names), list(scales) if scales else []))
        self.active_adapters = list(names)

    def disable_lora(self):
        self.calls.append(("disable_lora",))
        self.lora_enabled = False
        self.active_adapters = []

    def enable_lora(self):
        self.calls.append(("enable_lora",))
        self.lora_enabled = True

    def delete_adapters(self, name):
        self.calls.append(("delete_adapters", name))
        self.adapters.pop(name, None)
        if name in self.active_adapters:
            self.active_adapters.remove(name)

    def unload_lora_weights(self):
        self.calls.append(("unload_lora_weights",))
        self.adapters.clear()
        self.active_adapters.clear()
class TestQuantCache(unittest.TestCase):
    def test_quant_cache_hit_and_version(self):
        with tempfile.TemporaryDirectory() as td:
            base = Path(td)
            t_dir = base / "transformer"
            t_dir.mkdir(parents=True)
            (t_dir / "config.json").write_text("{}")
            _save_quant_metadata(
                base,
                model_id="Comfy-Org/Qwen-Image-2.1/qwen_image_2.1",
                quant_label="4-bit BitsAndBytes",
                quant_format="4bit",
                target_dtype="torch.bfloat16",
                bnb_version="0.43.0",
            )
            # Hit with exact bnb_version
            self.assertTrue(
                _is_cache_valid(
                    t_dir,
                    "Comfy-Org/Qwen-Image-2.1/qwen_image_2.1",
                    "4bit",
                    expected_bnb_version="0.43.0",
                )
            )
            # Miss with different bnb_version
            self.assertFalse(
                _is_cache_valid(
                    t_dir,
                    "Comfy-Org/Qwen-Image-2.1/qwen_image_2.1",
                    "4bit",
                    expected_bnb_version="0.44.0",
                )
            )

    def test_quant_cache_corrupted_fallback(self):
        with tempfile.TemporaryDirectory() as td:
            base = Path(td)
            t_dir = base / "transformer"
            t_dir.mkdir(parents=True)
            # Missing config.json -> invalid
            self.assertFalse(
                _is_cache_valid(t_dir, "Comfy-Org/Qwen-Image-2.1/qwen_image_2.1", "4bit")
            )
            # Invalid json in metadata.json
            (t_dir / "config.json").write_text("{}")
            (base / "metadata.json").write_text("corrupted json {")
            self.assertFalse(
                _is_cache_valid(t_dir, "Comfy-Org/Qwen-Image-2.1/qwen_image_2.1", "4bit")
            )

    def test_save_atomic_dir(self):
        with tempfile.TemporaryDirectory() as td:
            target = Path(td) / "dest"

            def writer(p):
                (p / "config.json").write_text('{"ok": true}')
                (p / "model.safetensors").write_text("weights")

            save_atomic_dir(target, writer)
            self.assertTrue((target / "config.json").exists())
            self.assertTrue((target / "model.safetensors").exists())


class TestDaemonLoraCache(unittest.TestCase):
    def setUp(self):
        self.pipe = FakePipe()
        self.cache = DaemonLoraCache(capacity=2)
        self._tmpdir = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmpdir.name)

    def tearDown(self):
        self._tmpdir.cleanup()

    def _create_lora_file(self, name: str, content: bytes = b"lora-weights") -> str:
        p = self.tmp / f"{name}.safetensors"
        p.write_bytes(content)
        return str(p)

    def test_lora_cache_hit_without_reload(self):
        lora_a = self._create_lora_file("lora_a")

        # Req 1: load LoRA A
        self.cache.apply_loras(
            self.pipe, [{"path": lora_a, "scale": 0.8}], "flux-2-klein-4b"
        )
        self.assertEqual(len([c for c in self.pipe.calls if c[0] == "load_lora_weights"]), 1)
        self.assertEqual(self.pipe.active_adapters, ["cached_lora_1"])

        # Req 2: same LoRA A (cache hit) -> no load_lora_weights call!
        self.cache.apply_loras(
            self.pipe, [{"path": lora_a, "scale": 1.0}], "flux-2-klein-4b"
        )
        self.assertEqual(len([c for c in self.pipe.calls if c[0] == "load_lora_weights"]), 1)
        self.assertEqual(self.pipe.active_adapters, ["cached_lora_1"])

    def test_lora_lru_eviction(self):
        lora_a = self._create_lora_file("lora_a")
        lora_b = self._create_lora_file("lora_b")
        lora_c = self._create_lora_file("lora_c")

        # Capacity is 2
        # Req 1: LoRA A
        self.cache.apply_loras(
            self.pipe, [{"path": lora_a, "scale": 0.8}], "flux-2-klein-4b"
        )
        # Req 2: LoRA B
        self.cache.apply_loras(
            self.pipe, [{"path": lora_b, "scale": 0.8}], "flux-2-klein-4b"
        )
        # Req 3: LoRA A (hit! moves A to most recently used)
        self.cache.apply_loras(
            self.pipe, [{"path": lora_a, "scale": 0.8}], "flux-2-klein-4b"
        )

        # Req 4: LoRA C -> exceeds capacity (2), evicts least recently used (which is B)
        self.cache.apply_loras(
            self.pipe, [{"path": lora_c, "scale": 0.8}], "flux-2-klein-4b"
        )
        deletes = [c for c in self.pipe.calls if c[0] == "delete_adapters"]
        self.assertEqual(len(deletes), 1)
        self.assertEqual(deletes[0][1], "cached_lora_2")  # LoRA B was evicted

    def test_request_without_lora_disables_residual(self):
        lora_a = self._create_lora_file("lora_a")

        # Req 1: LoRA A
        self.cache.apply_loras(
            self.pipe, [{"path": lora_a, "scale": 0.8}], "flux-2-klein-4b"
        )
        self.assertEqual(self.pipe.active_adapters, ["cached_lora_1"])

        # Req 2: No LoRA
        self.cache.apply_loras(self.pipe, [], "flux-2-klein-4b")
        self.assertEqual(self.pipe.active_adapters, [])
        disables = [c for c in self.pipe.calls if c[0] == "disable_lora"]
        self.assertEqual(len(disables), 1)

    def test_lora_disabled_then_re_enabled(self):
        lora_a = self._create_lora_file("lora_a")

        # 1. Req com LoRA
        self.cache.apply_loras(
            self.pipe, [{"path": lora_a, "scale": 0.8}], "flux-2-klein-4b"
        )
        self.assertEqual(self.pipe.active_adapters, ["cached_lora_1"])

        # 2. Req sem LoRA (desativa)
        self.cache.apply_loras(self.pipe, [], "flux-2-klein-4b")
        self.assertEqual(self.pipe.active_adapters, [])
        self.assertFalse(getattr(self.pipe, "lora_enabled", True))

        # 3. Req seguinte com LoRA -> deve chamar enable_lora() e reativar adaptador!
        self.cache.apply_loras(
            self.pipe, [{"path": lora_a, "scale": 1.0}], "flux-2-klein-4b"
        )
        self.assertEqual(self.pipe.active_adapters, ["cached_lora_1"])
        self.assertTrue(getattr(self.pipe, "lora_enabled", False))
        enables = [c for c in self.pipe.calls if c[0] == "enable_lora"]
        self.assertGreaterEqual(len(enables), 1)

    def test_multi_lora_exceeding_capacity_in_single_request(self):
        # Capacidade = 2, mas a requisição pede 3 adaptadores simultâneos
        lora_a = self._create_lora_file("lora_a")
        lora_b = self._create_lora_file("lora_b")
        lora_c = self._create_lora_file("lora_c")

        self.cache.apply_loras(
            self.pipe,
            [
                {"path": lora_a, "scale": 0.8},
                {"path": lora_b, "scale": 0.5},
                {"path": lora_c, "scale": 0.3},
            ],
            "flux-2-klein-4b",
        )
        # Nenhum dos 3 deve ter sido evictado durante o carregamento da requisição
        self.assertEqual(len(self.pipe.active_adapters), 3)
        deletes = [c for c in self.pipe.calls if c[0] == "delete_adapters"]
        self.assertEqual(len(deletes), 0)

    def test_delete_adapter_singular_fallback(self):
        class SingularPipe:
            def __init__(self):
                self.calls = []
                self.adapters = {}
                self.transformer = self

            def load_lora_weights(self, path, adapter_name):
                self.calls.append(("load_lora_weights", path, adapter_name))
                self.adapters[adapter_name] = path

            def set_adapters(self, names, scales=None):
                self.calls.append(("set_adapters", list(names)))

            def delete_adapter(self, name):
                self.calls.append(("delete_adapter", name))
                self.adapters.pop(name, None)

        sing_pipe = SingularPipe()
        cache = DaemonLoraCache(capacity=1)
        la = self._create_lora_file("la")
        lb = self._create_lora_file("lb")
        cache.apply_loras(sing_pipe, [{"path": la, "scale": 1.0}], "sdxl")
        cache.apply_loras(sing_pipe, [{"path": lb, "scale": 1.0}], "sdxl")
        deletes = [c for c in sing_pipe.calls if c[0] == "delete_adapter"]
        self.assertEqual(len(deletes), 1)

    def test_img2img_runner_inherits_lora_adapters(self):
        # Simula o fluxo do runner: pipe base carregado com LoRA, img2img cria novo wrapper e herda
        base_pipe = FakePipe()
        lora_a = self._create_lora_file("lora_a")
        self.cache.apply_loras(base_pipe, [{"path": lora_a, "scale": 0.75}], "sdxl")

        # Img2Img novo wrapper sobre components
        img2img_pipe = FakePipe()
        self.cache.apply_loras(img2img_pipe, [{"path": lora_a, "scale": 0.75}], "sdxl")
        self.assertEqual(img2img_pipe.active_adapters, ["cached_lora_1"])

    def test_clear_invalidates_all_on_pipeline_change(self):
        lora_a = self._create_lora_file("lora_a")
        self.cache.apply_loras(
            self.pipe, [{"path": lora_a, "scale": 0.8}], "flux-2-klein-4b"
        )
        self.assertEqual(len(self.cache.adapters), 1)

        self.cache.clear(self.pipe)
        self.assertEqual(len(self.cache.adapters), 0)
        unloads = [c for c in self.pipe.calls if c[0] == "unload_lora_weights"]
        self.assertEqual(len(unloads), 1)


if __name__ == "__main__":
    unittest.main()
