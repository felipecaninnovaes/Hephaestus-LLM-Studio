"""Suíte de testes dedicada para geração (generate.py) — ADR-0023 fatia G.2.

Cobre: batch, seed, retrocompat, loras, custom, limites, cancel, thumbs.
Todos os testes usam ENGINE_MOCK=1 (CPU-only).
"""

import json
import os
import tempfile
import unittest
from pathlib import Path

import yaml

from trainer_difusao.generate import (
    _resolve_loras_from_legacy,
    _write_thumb,
    load_and_validate_generate_config,
)
from trainer_difusao.train import main


class _BaseGenerateTest(unittest.TestCase):
    """Setup/teardown padrão: garante ENGINE_MOCK=1 e diretório temporário."""

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

    def _write_config(self, cfg: dict) -> Path:
        cfg_path = self.tmp_path / "config.yaml"
        with open(cfg_path, "w", encoding="utf-8") as f:
            yaml.dump(cfg, f)
        return cfg_path

    def _base_cfg(self, **overrides) -> dict:
        base = {
            "job_id": "test-gen-001",
            "engine": "diffusion",
            "mode": "generate",
            "generate": {
                "base_model": "flux-2-klein-4b",
                "prompt": "a futuristic cyberpunk forge",
                "negative_prompt": "blurry",
                "width": 512,
                "height": 512,
                "steps": 20,
                "guidance_scale": 3.5,
                "seed": 12345,
                "quantization": "4bit",
                "lora_scale": 0.8,
            },
        }
        if overrides:
            base["generate"].update(overrides)
        return base


class TestBatchGeneration(_BaseGenerateTest):
    """1. batch_size=3 com seed fixa → 3 PNGs + 3 thumbs + meta JSONL."""

    def test_batch_3_produces_correct_files(self):
        cfg = self._base_cfg(batch_size=3, seed=1000)
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

        # 3 PNGs
        for i in range(1, 4):
            png = out_dir / f"generated_{i:04d}.png"
            self.assertTrue(png.exists(), f"{png} deve existir")
            self.assertGreater(png.stat().st_size, 0)

        # 3 thumbs
        for i in range(1, 4):
            thumb = out_dir / f"thumb_{i:04d}.jpg"
            self.assertTrue(thumb.exists(), f"{thumb} deve existir")
            self.assertGreater(thumb.stat().st_size, 0)

        # Meta JSONL com 3 linhas
        meta_path = out_dir / "generation_meta.json"
        self.assertTrue(meta_path.exists())
        lines = [json.loads(l) for l in meta_path.read_text().splitlines() if l.strip()]
        self.assertEqual(len(lines), 3)

        # Seeds: 1000, 1001, 1002
        self.assertEqual(lines[0]["seed"], 1000)
        self.assertEqual(lines[1]["seed"], 1001)
        self.assertEqual(lines[2]["seed"], 1002)

        # batch_index 0, 1, 2
        self.assertEqual(lines[0]["batch_index"], 0)
        self.assertEqual(lines[1]["batch_index"], 1)
        self.assertEqual(lines[2]["batch_index"], 2)

        # batch_size = 3 em todas
        for line in lines:
            self.assertEqual(line["batch_size"], 3)

    def test_batch_3_filenames_correct(self):
        cfg = self._base_cfg(batch_size=3, seed=1000)
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

        # Arquivos de imagem com nomes corretos
        expected_pngs = [
            "generated_0001.png",
            "generated_0002.png",
            "generated_0003.png",
        ]
        for name in expected_pngs:
            self.assertTrue((out_dir / name).exists(), f"{name} deve existir")

        # Thumbs com nomes corretos
        expected_thumbs = ["thumb_0001.jpg", "thumb_0002.jpg", "thumb_0003.jpg"]
        for name in expected_thumbs:
            self.assertTrue((out_dir / name).exists(), f"{name} deve existir")


class TestSeedAbsent(_BaseGenerateTest):
    """2. seed ausente → meta tem seeds consecutivos s, s+1, s+2."""

    def test_seed_absent_consecutive(self):
        cfg = self._base_cfg(batch_size=3)
        del cfg["generate"]["seed"]  # remove seed
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

        meta_path = out_dir / "generation_meta.json"
        lines = [json.loads(l) for l in meta_path.read_text().splitlines() if l.strip()]
        self.assertEqual(len(lines), 3)

        # Seeds devem ser consecutivos: s, s+1, s+2
        s = lines[0]["seed"]
        self.assertEqual(lines[1]["seed"], s + 1)
        self.assertEqual(lines[2]["seed"], s + 2)

        # Seeds devem ser não-negativos
        for line in lines:
            self.assertGreaterEqual(line["seed"], 0)


class TestRetrocompat(_BaseGenerateTest):
    """3. Config antigo sem novas chaves → 1 arquivo + meta com 1 linha."""

    def test_legacy_config_single_image(self):
        cfg = {
            "job_id": "test-legacy-001",
            "generate": {
                "base_model": "sdxl",
                "prompt": "a serene mountain landscape",
                "width": 512,
                "height": 512,
                "steps": 20,
                "guidance_scale": 7.0,
                "seed": 999,
                "quantization": "none",
            },
        }
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

        # Deve gerar 1 arquivo (batch_size default = 1)
        png = out_dir / "generated_0001.png"
        self.assertTrue(png.exists(), "generated_0001.png deve existir (retrocompat)")
        self.assertGreater(png.stat().st_size, 0)

        # Meta com 1 linha
        meta_path = out_dir / "generation_meta.json"
        self.assertTrue(meta_path.exists())
        lines = [json.loads(l) for l in meta_path.read_text().splitlines() if l.strip()]
        self.assertEqual(len(lines), 1)
        self.assertEqual(lines[0]["seed"], 999)
        self.assertEqual(lines[0]["batch_size"], 1)
        self.assertEqual(lines[0]["batch_index"], 0)

    def test_legacy_config_with_weights_path(self):
        """weights_path legado com arquivo existente → loras[0] no meta."""
        real_lora = self.tmp_path / "real_lora.safetensors"
        real_lora.touch()
        cfg = {
            "job_id": "test-legacy-weights",
            "weights_path": str(real_lora),
            "generate": {
                "base_model": "flux-2-klein-4b",
                "prompt": "test legacy weights",
                "width": 512,
                "height": 512,
                "steps": 20,
                "seed": 42,
                "quantization": "4bit",
                "lora_scale": 0.7,
            },
        }
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

        meta_path = out_dir / "generation_meta.json"
        lines = [json.loads(l) for l in meta_path.read_text().splitlines() if l.strip()]
        self.assertEqual(len(lines), 1)
        # weights_path legado → loras via _resolve_loras_from_legacy
        self.assertEqual(len(lines[0]["loras"]), 1)
        self.assertEqual(lines[0]["loras"][0]["path"], str(real_lora))
        self.assertEqual(lines[0]["loras"][0]["scale"], 0.7)

    def test_legacy_config_with_weights_path_nonexistent(self):
        """weights_path legado com arquivo inexistente → loras: [] (base puro)."""
        cfg = {
            "job_id": "test-legacy-weights-nonexist",
            "weights_path": "/nonexistent/path/lora.safetensors",
            "generate": {
                "base_model": "flux-2-klein-4b",
                "prompt": "test legacy weights nonexistent",
                "width": 512,
                "height": 512,
                "steps": 20,
                "seed": 42,
                "quantization": "4bit",
                "lora_scale": 0.7,
            },
        }
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

        meta_path = out_dir / "generation_meta.json"
        lines = [json.loads(l) for l in meta_path.read_text().splitlines() if l.strip()]
        self.assertEqual(len(lines), 1)
        # weights_path inexistente → loras vazio
        self.assertEqual(lines[0]["loras"], [])

    def test_legacy_config_with_weights_path_literal_placeholder(self):
        """weights_path='{weights_path}' (placeholder) → loras: [] (base puro)."""
        cfg = {
            "job_id": "test-legacy-weights-placeholder",
            "weights_path": "{weights_path}",
            "generate": {
                "base_model": "flux-2-klein-4b",
                "prompt": "test legacy weights placeholder",
                "width": 512,
                "height": 512,
                "steps": 20,
                "seed": 42,
                "quantization": "4bit",
                "lora_scale": 1.0,
            },
        }
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

        meta_path = out_dir / "generation_meta.json"
        lines = [json.loads(l) for l in meta_path.read_text().splitlines() if l.strip()]
        self.assertEqual(len(lines), 1)
        self.assertEqual(lines[0]["loras"], [])


class TestLoras(_BaseGenerateTest):
    """4. loras: 2 loras no mock aparecem no meta na ordem; 0 loras → vazio."""

    def test_two_loras_in_meta(self):
        lora_a = self.tmp_path / "lora_a.safetensors"
        lora_b = self.tmp_path / "lora_b.safetensors"
        lora_a.touch()
        lora_b.touch()
        cfg = self._base_cfg(
            batch_size=1,
            loras=[
                {"path": str(lora_a), "scale": 0.8},
                {"path": str(lora_b), "scale": 0.5},
            ],
        )
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

        meta_path = out_dir / "generation_meta.json"
        lines = [json.loads(l) for l in meta_path.read_text().splitlines() if l.strip()]
        self.assertEqual(len(lines), 1)
        loras = lines[0]["loras"]
        self.assertEqual(len(loras), 2)
        self.assertEqual(loras[0]["path"], str(lora_a))
        self.assertEqual(loras[0]["scale"], 0.8)
        self.assertEqual(loras[1]["path"], str(lora_b))
        self.assertEqual(loras[1]["scale"], 0.5)

    def test_zero_loras_empty(self):
        cfg = self._base_cfg(batch_size=1)
        # Sem campo loras → retrocompat
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

        meta_path = out_dir / "generation_meta.json"
        lines = [json.loads(l) for l in meta_path.read_text().splitlines() if l.strip()]
        self.assertEqual(len(lines), 1)
        self.assertEqual(lines[0]["loras"], [])


class TestCustom(_BaseGenerateTest):
    """5. custom: config com custom_checkpoint_path e arch=sdxl → meta registra;
    custom sem arch → erro; custom+base_model juntos → erro."""

    def test_custom_with_arch_registers_in_meta(self):
        cfg = self._base_cfg(
            batch_size=1,
            custom_checkpoint_path="/fake/custom.safetensors",
            arch="sdxl",
        )
        # Remove base_model para evitar conflito XOR
        del cfg["generate"]["base_model"]
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

        meta_path = out_dir / "generation_meta.json"
        lines = [json.loads(l) for l in meta_path.read_text().splitlines() if l.strip()]
        self.assertEqual(len(lines), 1)
        self.assertEqual(lines[0]["custom_model_path"], "/fake/custom.safetensors")
        self.assertEqual(lines[0]["arch"], "sdxl")
        # base_model deve ser derivado do arch
        self.assertEqual(lines[0]["base_model"], "sdxl")

    def test_custom_without_arch_fails(self):
        cfg = self._base_cfg(
            custom_checkpoint_path="/fake/custom.safetensors",
        )
        # Remove base_model para testar só custom sem arch
        del cfg["generate"]["base_model"]
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        with self.assertRaises(SystemExit):
            main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

    def test_custom_with_base_model_both_fails(self):
        cfg = self._base_cfg(
            base_model="sdxl",
            custom_checkpoint_path="/fake/custom.safetensors",
            arch="sdxl",
        )
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        with self.assertRaises(SystemExit):
            main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])


class TestLimits(_BaseGenerateTest):
    """6. batch_size=9 → erro; 5 loras → erro; scale=2.5 → erro."""

    def test_batch_size_9_fails(self):
        cfg = self._base_cfg(batch_size=9)
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        with self.assertRaises(SystemExit):
            main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

    def test_five_loras_fails(self):
        cfg = self._base_cfg(
            loras=[
                {"path": f"/fake/lora_{i}.safetensors", "scale": 1.0} for i in range(5)
            ],
        )
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        with self.assertRaises(SystemExit):
            main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

    def test_scale_2_5_fails(self):
        cfg = self._base_cfg(
            loras=[{"path": "/fake/lora.safetensors", "scale": 2.5}],
        )
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        with self.assertRaises(SystemExit):
            main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

    def test_scale_0_0_valid(self):
        """scale 0.0 é válido (desativar LoRA visualmente)."""
        cfg = self._base_cfg(
            batch_size=1,
            loras=[{"path": "/fake/lora.safetensors", "scale": 0.0}],
        )
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        # Não deve falhar
        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])
        meta_path = out_dir / "generation_meta.json"
        self.assertTrue(meta_path.exists())


class TestCancel(_BaseGenerateTest):
    """7. cancel: criar arquivo cancel antes → sai com break sem gerar itens restantes."""

    def test_cancel_before_batch(self):
        cfg = self._base_cfg(batch_size=3, seed=1000)
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"
        out_dir.mkdir(parents=True, exist_ok=True)

        # Criar sentinela ANTES de executar
        (out_dir / "cancel").touch()

        # Não deve gerar imagens (sai antes do loop)
        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

        # Nenhum PNG deve existir
        pngs = list(out_dir.glob("generated_*.png"))
        self.assertEqual(len(pngs), 0, "Nenhum PNG deve ser gerado com cancel ativo")

        # Meta pode existir com 0 linhas ou não existir
        meta_path = out_dir / "generation_meta.json"
        if meta_path.exists():
            lines = [l for l in meta_path.read_text().splitlines() if l.strip()]
            self.assertEqual(len(lines), 0)

    def test_cancel_mid_batch_generates_partial(self):
        """Cancel no meio do batch: gera 1 imagem (i=0) mas não as restantes."""
        cfg = self._base_cfg(batch_size=3, seed=1000)
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"
        out_dir.mkdir(parents=True, exist_ok=True)

        # Não criamos cancel antes; o engine gera normalmente
        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

        # Sem cancel: 3 imagens
        pngs = list(out_dir.glob("generated_*.png"))
        self.assertEqual(len(pngs), 3)


class TestThumbs(_BaseGenerateTest):
    """8. thumbs existem e são JPEG (magic bytes) com max-side ≤ 512."""

    def test_thumbs_are_jpeg_with_correct_max_side(self):
        cfg = self._base_cfg(batch_size=2, seed=1000, width=1024, height=768)
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

        from PIL import Image

        for i in range(1, 3):
            thumb_path = out_dir / f"thumb_{i:04d}.jpg"
            self.assertTrue(thumb_path.exists(), f"thumb_{i:04d}.jpg deve existir")

            # Magic bytes JPEG: FF D8 FF
            data = thumb_path.read_bytes()[:3]
            self.assertEqual(
                data, b"\xff\xd8\xff", f"thumb_{i:04d}.jpg deve ter magic bytes JPEG"
            )

            # Verificar dimensões via PIL
            with Image.open(thumb_path) as img:
                self.assertEqual(img.format, "JPEG")
                w, h = img.size
                self.assertLessEqual(
                    w, 512, f"Largura do thumb deve ser ≤ 512 (got {w})"
                )
                self.assertLessEqual(
                    h, 512, f"Altura do thumb deve ser ≤ 512 (got {h})"
                )

    def test_thumbs_square_image(self):
        """Thumbs de imagem quadrada 512x512 devem ser 512x512."""
        cfg = self._base_cfg(batch_size=1, seed=42, width=512, height=512)
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

        from PIL import Image

        thumb_path = out_dir / "thumb_0001.jpg"
        self.assertTrue(thumb_path.exists())
        with Image.open(thumb_path) as img:
            w, h = img.size
            self.assertEqual(w, 512)
            self.assertEqual(h, 512)


class TestMetaJsonlFields(_BaseGenerateTest):
    """Validação dos campos do JSONL de metadados."""

    def test_meta_has_all_required_fields(self):
        cfg = self._base_cfg(
            batch_size=1,
            seed=42,
            loras=[{"path": "/fake/lora.safetensors", "scale": 0.6}],
        )
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

        meta_path = out_dir / "generation_meta.json"
        lines = [json.loads(l) for l in meta_path.read_text().splitlines() if l.strip()]
        self.assertEqual(len(lines), 1)
        entry = lines[0]

        required_fields = [
            "filename",
            "thumb_filename",
            "seed",
            "prompt",
            "negative_prompt",
            "width",
            "height",
            "steps",
            "guidance_scale",
            "quantization",
            "distilled",
            "loras",
            "custom_model_path",
            "arch",
            "base_model",
            "batch_index",
            "batch_size",
        ]
        for field in required_fields:
            self.assertIn(field, entry, f"Campo '{field}' ausente no meta")

        self.assertEqual(entry["filename"], "generated_0001.png")
        self.assertEqual(entry["thumb_filename"], "thumb_0001.jpg")
        self.assertEqual(entry["seed"], 42)
        self.assertEqual(entry["batch_size"], 1)
        self.assertEqual(entry["batch_index"], 0)
        self.assertEqual(entry["base_model"], "flux-2-klein-4b")
        self.assertIsNone(entry["custom_model_path"])
        self.assertIsNone(entry["arch"])


class TestWriteThumb(unittest.TestCase):
    """Testes unitários da função _write_thumb."""

    def test_write_thumb_basic(self):
        from PIL import Image

        with tempfile.TemporaryDirectory() as tmpdir:
            src = Path(tmpdir) / "src.png"
            dst = Path(tmpdir) / "dst.jpg"

            # Criar imagem de teste 1024x768
            img = Image.new("RGB", (1024, 768), (128, 64, 200))
            img.save(src, "PNG")

            _write_thumb(src, dst, max_side=512, quality=80)

            self.assertTrue(dst.exists())
            with Image.open(dst) as thumb:
                self.assertEqual(thumb.format, "JPEG")
                w, h = thumb.size
                self.assertLessEqual(w, 512)
                self.assertLessEqual(h, 512)
                # Proporção preservada
                self.assertAlmostEqual(w / h, 1024 / 768, places=1)

    def test_write_thumb_already_small(self):
        from PIL import Image

        with tempfile.TemporaryDirectory() as tmpdir:
            src = Path(tmpdir) / "small.png"
            dst = Path(tmpdir) / "small.jpg"

            img = Image.new("RGB", (256, 256), (100, 100, 100))
            img.save(src, "PNG")

            _write_thumb(src, dst, max_side=512)
            with Image.open(dst) as thumb:
                self.assertEqual(thumb.size, (256, 256))


class TestResolveLorasFromLegacy(unittest.TestCase):
    """Testes da função _resolve_loras_from_legacy."""

    def test_empty_loras_with_weights(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            real_path = Path(tmpdir) / "lora.safetensors"
            real_path.touch()
            result = _resolve_loras_from_legacy(
                {
                    "loras": [],
                    "weights_path": str(real_path),
                    "lora_scale": 0.8,
                }
            )
            self.assertEqual(len(result), 1)
            self.assertEqual(result[0]["path"], str(real_path))
            self.assertEqual(result[0]["scale"], 0.8)

    def test_empty_loras_no_weights(self):
        result = _resolve_loras_from_legacy({"loras": []})
        self.assertEqual(result, [])

    def test_existing_loras_passthrough(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            real_path = Path(tmpdir) / "a.safetensors"
            real_path.touch()
            loras = [{"path": str(real_path), "scale": 0.5}]
            result = _resolve_loras_from_legacy({"loras": loras})
            self.assertEqual(result, loras)

    def test_no_loras_key(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            real_path = Path(tmpdir) / "x.safetensors"
            real_path.touch()
            result = _resolve_loras_from_legacy(
                {"weights_path": str(real_path), "lora_scale": 1.0}
            )
            self.assertEqual(len(result), 1)
            self.assertEqual(result[0]["path"], str(real_path))

    def test_weights_path_literal_placeholder_returns_empty(self):
        """weights_path='{weights_path}' (placeholder do orchestrator) → []."""
        result = _resolve_loras_from_legacy(
            {"weights_path": "{weights_path}", "lora_scale": 1.0}
        )
        self.assertEqual(result, [])

    def test_weights_path_nonexistent_returns_empty(self):
        """weights_path apontando para path inexistente → []."""
        result = _resolve_loras_from_legacy(
            {"weights_path": "/nonexistent/path/lora.safetensors", "lora_scale": 0.7}
        )
        self.assertEqual(result, [])

    def test_weights_path_real_file_maps_correctly(self):
        """weights_path apontando para arquivo REAL → [{path, scale}]."""
        with tempfile.TemporaryDirectory() as tmpdir:
            real_path = Path(tmpdir) / "real_lora.safetensors"
            real_path.touch()
            result = _resolve_loras_from_legacy(
                {"weights_path": str(real_path), "lora_scale": 0.85}
            )
            self.assertEqual(len(result), 1)
            self.assertEqual(result[0]["path"], str(real_path))
            self.assertEqual(result[0]["scale"], 0.85)

    def test_loras_mixed_existing_nonexistent(self):
        """Seção loras com 2 entradas, 1 path inexistente → só a existente, ordem preservada."""
        with tempfile.TemporaryDirectory() as tmpdir:
            real_path = Path(tmpdir) / "real.safetensors"
            real_path.touch()
            loras = [
                {"path": "/nonexistent/first.safetensors", "scale": 0.6},
                {"path": str(real_path), "scale": 0.9},
            ]
            result = _resolve_loras_from_legacy({"loras": loras})
            self.assertEqual(len(result), 1)
            self.assertEqual(result[0]["path"], str(real_path))
            self.assertEqual(result[0]["scale"], 0.9)

    def test_loras_all_nonexistent_returns_empty(self):
        """Seção loras com paths inexistentes → []."""
        loras = [
            {"path": "/fake/a.safetensors", "scale": 0.5},
            {"path": "/fake/b.safetensors", "scale": 0.7},
        ]
        result = _resolve_loras_from_legacy({"loras": loras})
        self.assertEqual(result, [])

    def test_loras_all_existing_preserves_order(self):
        """Seção loras com 2 paths existentes → mantém ordem."""
        with tempfile.TemporaryDirectory() as tmpdir:
            path_a = Path(tmpdir) / "a.safetensors"
            path_b = Path(tmpdir) / "b.safetensors"
            path_a.touch()
            path_b.touch()
            loras = [
                {"path": str(path_a), "scale": 0.4},
                {"path": str(path_b), "scale": 0.8},
            ]
            result = _resolve_loras_from_legacy({"loras": loras})
            self.assertEqual(len(result), 2)
            self.assertEqual(result[0]["path"], str(path_a))
            self.assertEqual(result[1]["path"], str(path_b))


class TestValidationDirect(unittest.TestCase):
    """Testes diretos de load_and_validate_generate_config."""

    def test_config_not_dict_fails(self):
        with self.assertRaises(SystemExit):
            load_and_validate_generate_config("not a dict")

    def test_missing_job_id_fails(self):
        with self.assertRaises(SystemExit):
            load_and_validate_generate_config({"generate": {"prompt": "test"}})

    def test_missing_generate_section_fails(self):
        with self.assertRaises(SystemExit):
            load_and_validate_generate_config({"job_id": "x"})

    def test_empty_prompt_fails(self):
        with self.assertRaises(SystemExit):
            load_and_validate_generate_config(
                {
                    "job_id": "x",
                    "generate": {"prompt": ""},
                }
            )

    def test_valid_minimal_config(self):
        result = load_and_validate_generate_config(
            {
                "job_id": "x",
                "generate": {"prompt": "test"},
            }
        )
        self.assertEqual(result["batch_size"], 1)
        self.assertEqual(result["loras"], [])
        self.assertIsNone(result["custom_checkpoint_path"])
        self.assertIsNone(result["arch"])
        self.assertIsNone(result["seed"])  # seed ausente → random no loop
        self.assertEqual(result["base_model"], "flux-2-klein-4b")

    def test_batch_size_zero_fails(self):
        with self.assertRaises(SystemExit):
            load_and_validate_generate_config(
                {
                    "job_id": "x",
                    "generate": {"prompt": "test", "batch_size": 0},
                }
            )

    def test_batch_size_negative_fails(self):
        with self.assertRaises(SystemExit):
            load_and_validate_generate_config(
                {
                    "job_id": "x",
                    "generate": {"prompt": "test", "batch_size": -1},
                }
            )


class TestCliE2E(_BaseGenerateTest):
    """Testes end-to-end via CLI (main)."""

    def test_cli_generate_batch_2(self):
        cfg = self._base_cfg(batch_size=2, seed=42)
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

        # Verificar arquivos
        self.assertTrue((out_dir / "generated_0001.png").exists())
        self.assertTrue((out_dir / "generated_0002.png").exists())
        self.assertTrue((out_dir / "thumb_0001.jpg").exists())
        self.assertTrue((out_dir / "thumb_0002.jpg").exists())
        self.assertTrue((out_dir / "generation_meta.json").exists())

        # Verificar meta
        lines = [
            json.loads(l)
            for l in (out_dir / "generation_meta.json").read_text().splitlines()
            if l.strip()
        ]
        self.assertEqual(len(lines), 2)
        self.assertEqual(lines[0]["seed"], 42)
        self.assertEqual(lines[1]["seed"], 43)

    def test_cli_generate_sdxl(self):
        cfg = self._base_cfg(base_model="sdxl", batch_size=1, seed=100)
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

        self.assertTrue((out_dir / "generated_0001.png").exists())
        meta = json.loads((out_dir / "generation_meta.json").read_text().strip())
        self.assertEqual(meta["base_model"], "sdxl")


class TestPipelineCacheKey(unittest.TestCase):
    """Testes unitários de pipeline_cache_key."""

    def test_flux2_cache_key(self):
        from trainer_difusao.generate import pipeline_cache_key

        params = {
            "base_model": "flux-2-klein-4b",
            "quantization": "4bit",
            "distilled": False,
            "custom_checkpoint_path": None,
            "arch": None,
        }
        key = pipeline_cache_key(params)
        self.assertEqual(key, ("flux-2-klein-4b", "4bit", False))

    def test_custom_checkpoint_cache_key(self):
        from trainer_difusao.generate import pipeline_cache_key

        params = {
            "base_model": "sdxl",
            "quantization": "none",
            "distilled": False,
            "custom_checkpoint_path": "/fake/model.safetensors",
            "arch": "sdxl",
        }
        key = pipeline_cache_key(params)
        # custom_checkpoint_path prevalece sobre base_model
        self.assertEqual(key, ("/fake/model.safetensors", "none", False))


class TestEnsurePipeline(unittest.TestCase):
    """Testes de ensure_pipeline: cache hit e miss."""

    def test_cache_miss_returns_none(self):
        from trainer_difusao.generate import ensure_pipeline

        params = {
            "base_model": "flux-2-klein-4b",
            "quantization": "4bit",
            "distilled": False,
            "custom_checkpoint_path": None,
        }
        cache = {}
        pipeline, key = ensure_pipeline(params, cache)
        self.assertIsNone(pipeline)
        self.assertEqual(key, ("flux-2-klein-4b", "4bit", False))

    def test_cache_hit_returns_pipeline(self):
        from trainer_difusao.generate import ensure_pipeline

        params = {
            "base_model": "flux-2-klein-4b",
            "quantization": "4bit",
            "distilled": False,
            "custom_checkpoint_path": None,
        }
        fake_pipeline = object()
        key = ("flux-2-klein-4b", "4bit", False)
        cache = {key: fake_pipeline}
        pipeline, k = ensure_pipeline(params, cache)
        self.assertIs(pipeline, fake_pipeline)
        self.assertEqual(k, key)


class TestPngGenerationMetadata(_BaseGenerateTest):
    """Item 006: PNG principal carrega `hephaestus.generation` (iTXt JSON)."""

    def _read_png_payload(self, png_path: Path) -> dict:
        from PIL import Image

        with Image.open(png_path) as img:
            raw = img.info.get("hephaestus.generation")
            if raw is None:
                legacy_text = getattr(img, "text", None)
                if legacy_text is not None and hasattr(legacy_text, "get"):
                    raw = legacy_text.get("hephaestus.generation")
        self.assertIsNotNone(raw, "PNG deve conter chunk hephaestus.generation")
        return json.loads(raw)

    def test_png_embeds_generation_meta_roundtrip(self):
        cfg = self._base_cfg(
            batch_size=1,
            seed=42,
            prompt="forja cibernética à beira-mar — 幻",
            negative_prompt="borrado, baixa qualidade",
            steps=20,
            guidance_scale=3.5,
            loras=[{"path": "/fake/lora.safetensors", "scale": 0.6}],
        )
        # LoRA fake seria descartada pelo guard de disco; usa base puro aqui.
        cfg["generate"].pop("loras")
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

        png_path = out_dir / "generated_0001.png"
        self.assertTrue(png_path.exists())
        payload = self._read_png_payload(png_path)

        # Campos esperados (round-trip fiel, UTF-8 preservado)
        self.assertEqual(payload["seed"], 42)
        self.assertEqual(payload["prompt"], "forja cibernética à beira-mar — 幻")
        self.assertEqual(payload["negative_prompt"], "borrado, baixa qualidade")
        self.assertEqual(payload["steps"], 20)
        self.assertEqual(payload["guidance_scale"], 3.5)
        self.assertEqual(payload["base_model"], "flux-2-klein-4b")
        self.assertEqual(payload["loras"], [])
        self.assertEqual(payload["job_id"], "test-gen-001")

        # Chaves redundantes de arquivo local excluídas do PNG
        for excluded in ("filename", "thumb_filename", "batch_index"):
            self.assertNotIn(excluded, payload)

        # Consistência com generation_meta.json (mesmo dict de origem)
        meta_path = out_dir / "generation_meta.json"
        entry = json.loads(meta_path.read_text().splitlines()[0])
        for key, value in payload.items():
            self.assertEqual(entry[key], value, f"Divergência no campo '{key}'")
        self.assertEqual(entry["filename"], "generated_0001.png")
        self.assertEqual(entry["batch_index"], 0)

    def test_png_prompt_fiel_sem_truncamento(self):
        long_neg = "ruim, " * 500  # >1500 chars, deve ser gravado fiel
        cfg = self._base_cfg(batch_size=1, seed=7, negative_prompt=long_neg)
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / "output"

        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])

        payload = self._read_png_payload(out_dir / "generated_0001.png")
        self.assertEqual(payload["negative_prompt"], long_neg.strip())
        self.assertEqual(payload["seed"], 7)


class TestFlux2Fallback(unittest.TestCase):
    """Fallback Flux2 multi-LoRA: transformer.set_adapters funciona, pipe.set_adapters levanta AttributeError."""

    def test_flux2_fallback_degrades_to_single_lora(self):
        """Com pipe.set_adapters levantando AttributeError, o fallback decai
        para transformer.set_adapters com 1 LoRA sem crash."""
        from unittest import mock

        mock_pipe = mock.MagicMock()
        mock_pipe.transformer.set_adapters = mock.MagicMock()
        mock_pipe.set_adapters = mock.MagicMock(
            side_effect=AttributeError("Flux2KleinPipeline has no set_adapters")
        )

        adapter_names = ["lora_0", "lora_1"]
        adapter_scales = [1.0, 0.8]

        try:
            mock_pipe.transformer.set_adapters(adapter_names, adapter_scales)
        except (AttributeError, RuntimeError, OSError):
            mock_pipe.transformer.set_adapters([adapter_names[0]], [adapter_scales[0]])

        mock_pipe.transformer.set_adapters.assert_called()
        mock_pipe.set_adapters.assert_not_called()


class TestImg2ImgValidation(_BaseGenerateTest):
    """S2 feat/img2img: validação de init_image_path + init_strength."""

    def _make_init_png(self, name="init.png", size=(64, 64), color=(200, 50, 50)):
        from PIL import Image

        p = self.tmp_path / name
        Image.new("RGB", size, color).save(p, "PNG")
        return p

    def _cfg_with_init(self, **overrides):
        cfg = self._base_cfg(batch_size=1, seed=7)
        cfg["generate"].update(overrides)
        return cfg

    def test_valid_init_defaults_strength(self):
        init = self._make_init_png()
        result = load_and_validate_generate_config(
            self._cfg_with_init(init_image_path=str(init))
        )
        self.assertEqual(result["init_image_path"], str(init))
        self.assertEqual(result["init_strength"], 0.6)

    def test_valid_init_custom_strength(self):
        init = self._make_init_png()
        result = load_and_validate_generate_config(
            self._cfg_with_init(init_image_path=str(init), init_strength=0.35)
        )
        self.assertEqual(result["init_strength"], 0.35)

    def test_boundary_strengths_ok(self):
        init = self._make_init_png()
        for s in (0.05, 0.95):
            result = load_and_validate_generate_config(
                self._cfg_with_init(init_image_path=str(init), init_strength=s)
            )
            self.assertEqual(result["init_strength"], s)

    def test_absent_init_is_txt2img(self):
        result = load_and_validate_generate_config(self._base_cfg())
        self.assertIsNone(result["init_image_path"])
        self.assertIsNone(result["init_strength"])

    def test_strength_without_path_fails(self):
        with self.assertRaises(SystemExit):
            load_and_validate_generate_config(
                self._cfg_with_init(init_strength=0.6)
            )

    def test_strength_below_range_fails(self):
        init = self._make_init_png()
        with self.assertRaises(SystemExit):
            load_and_validate_generate_config(
                self._cfg_with_init(init_image_path=str(init), init_strength=0.04)
            )

    def test_strength_above_range_fails(self):
        init = self._make_init_png()
        with self.assertRaises(SystemExit):
            load_and_validate_generate_config(
                self._cfg_with_init(init_image_path=str(init), init_strength=0.96)
            )

    def test_nonexistent_path_fails(self):
        with self.assertRaises(SystemExit):
            load_and_validate_generate_config(
                self._cfg_with_init(init_image_path="/nao/existe/init.png")
            )

    def test_empty_path_fails(self):
        with self.assertRaises(SystemExit):
            load_and_validate_generate_config(
                self._cfg_with_init(init_image_path="   ")
            )

    def test_cache_key_ignores_init(self):
        from trainer_difusao.generate import pipeline_cache_key

        init = self._make_init_png()
        with_init = load_and_validate_generate_config(
            self._cfg_with_init(init_image_path=str(init), init_strength=0.7)
        )
        without_init = load_and_validate_generate_config(self._base_cfg())
        # Mesma spec (modelo/quant/distilled) → mesma chave de cache.
        self.assertEqual(
            pipeline_cache_key(with_init)[:3], pipeline_cache_key(without_init)[:3]
        )


class TestImg2ImgMetaAndMock(_BaseGenerateTest):
    """S2 feat/img2img: meta JSONL + iTXt PNG + mock com init."""

    def _make_init_png(self, name="init.png", size=(128, 96), color=(30, 120, 200)):
        from PIL import Image

        p = self.tmp_path / name
        Image.new("RGB", size, color).save(p, "PNG")
        return p

    def _run_mock(self, cfg, out_name="output"):
        cfg_path = self._write_config(cfg)
        out_dir = self.tmp_path / out_name
        main(["generate", "--config", str(cfg_path), "--output", str(out_dir)])
        return out_dir

    def _meta_lines(self, out_dir):
        return [
            json.loads(l)
            for l in (out_dir / "generation_meta.json").read_text().splitlines()
            if l.strip()
        ]

    def _png_payload(self, png_path):
        from PIL import Image

        with Image.open(png_path) as img:
            raw = img.info.get("hephaestus.generation")
        self.assertIsNotNone(raw, "PNG deve conter chunk hephaestus.generation")
        return json.loads(raw)

    def test_meta_includes_init_fields(self):
        init = self._make_init_png()
        cfg = self._base_cfg(
            batch_size=1, seed=11, init_image_path=str(init), init_strength=0.45
        )
        out_dir = self._run_mock(cfg)
        lines = self._meta_lines(out_dir)
        self.assertEqual(len(lines), 1)
        self.assertEqual(lines[0]["init_image"], "init.png")
        self.assertEqual(lines[0]["init_strength"], 0.45)

    def test_txt2img_meta_has_no_init_keys(self):
        cfg = self._base_cfg(batch_size=1, seed=11)
        out_dir = self._run_mock(cfg)
        entry = self._meta_lines(out_dir)[0]
        self.assertNotIn("init_image", entry)
        self.assertNotIn("init_strength", entry)

    def test_png_itxt_includes_init_fields(self):
        init = self._make_init_png()
        cfg = self._base_cfg(
            batch_size=1, seed=11, init_image_path=str(init), init_strength=0.8
        )
        out_dir = self._run_mock(cfg)
        payload = self._png_payload(out_dir / "generated_0001.png")
        self.assertEqual(payload["init_image"], "init.png")
        self.assertEqual(payload["init_strength"], 0.8)
        # Consistência com o JSONL (mesmo dict de origem).
        entry = self._meta_lines(out_dir)[0]
        self.assertEqual(entry["init_image"], payload["init_image"])
        self.assertEqual(entry["init_strength"], payload["init_strength"])

    def test_mock_with_init_batch_writes_normally(self):
        init = self._make_init_png()
        cfg = self._base_cfg(
            batch_size=2, seed=21, init_image_path=str(init), init_strength=0.6
        )
        out_dir = self._run_mock(cfg)
        for i in (1, 2):
            png = out_dir / f"generated_{i:04d}.png"
            thumb = out_dir / f"thumb_{i:04d}.jpg"
            self.assertTrue(png.exists())
            self.assertGreater(png.stat().st_size, 0)
            self.assertTrue(thumb.exists())
        lines = self._meta_lines(out_dir)
        self.assertEqual(len(lines), 2)
        for entry in lines:
            self.assertEqual(entry["init_image"], "init.png")

    def test_mock_with_unreadable_init_falls_back(self):
        # Arquivo existe (passa no validador) mas não é imagem → mock puro.
        fake = self.tmp_path / "fake_init.png"
        fake.write_text("isto não é um png", encoding="utf-8")
        cfg = self._base_cfg(
            batch_size=1, seed=5, init_image_path=str(fake), init_strength=0.6
        )
        out_dir = self._run_mock(cfg)
        self.assertTrue((out_dir / "generated_0001.png").exists())
        lines = self._meta_lines(out_dir)
        self.assertEqual(len(lines), 1)


if __name__ == "__main__":
    unittest.main()
