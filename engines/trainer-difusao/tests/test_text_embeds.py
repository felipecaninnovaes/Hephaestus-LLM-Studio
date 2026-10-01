"""Testes unitários para cache de text embeddings e offload de text encoders."""
from __future__ import annotations

from pathlib import Path
from typing import Any
import pytest
torch = pytest.importorskip("torch")

from trainer_difusao.common_pkg.text_embeds import (
    ENABLE_TEXT_ENCODER_UNLOAD,
    TextEmbedsCache,
    _cached_encode,
    _cleanup_encoders,
    _offload_encoders_to_cpu,
    _precompute_sample_embeds_flux,
    _precompute_sample_embeds_sd15,
    _precompute_sample_embeds_sdxl,
    _precompute_text_cache,
    _precompute_text_cache_with_cleanup,
    _temporary_device_encoders,
)


class MockEncoder:
    def __init__(self, initial_device: str = "cpu"):
        self.device = initial_device
        self.call_count = 0

    def to(self, device: str) -> MockEncoder:
        self.device = str(device)
        self.call_count += 1
        return self


def test_offload_encoders_to_cpu():
    enc1 = MockEncoder("cuda:0")
    enc2 = MockEncoder("cuda:1")
    _offload_encoders_to_cpu([enc1, enc2, None])
    assert enc1.device == "cpu"
    assert enc2.device == "cpu"


def test_temporary_device_encoders_with_flag(monkeypatch):
    import trainer_difusao.common_pkg.text_embeds as mod

    monkeypatch.setattr(mod, "ENABLE_TEXT_ENCODER_UNLOAD", True)

    enc = MockEncoder("cpu")
    target_device = "cuda:0"

    with _temporary_device_encoders([enc], target_device):
        assert enc.device == target_device

    assert enc.device == "cpu"


def test_temporary_device_encoders_without_flag(monkeypatch):
    import trainer_difusao.common_pkg.text_embeds as mod

    monkeypatch.setattr(mod, "ENABLE_TEXT_ENCODER_UNLOAD", False)

    enc = MockEncoder("cpu")
    with _temporary_device_encoders([enc], "cuda:0"):
        assert enc.device == "cpu"
    assert enc.device == "cpu"


def test_temporary_device_encoders_restores_on_exception(monkeypatch):
    import trainer_difusao.common_pkg.text_embeds as mod

    monkeypatch.setattr(mod, "ENABLE_TEXT_ENCODER_UNLOAD", True)

    enc = MockEncoder("cpu")
    with pytest.raises(RuntimeError):
        with _temporary_device_encoders([enc], "cuda:0"):
            assert enc.device == "cuda:0"
            raise RuntimeError("simulated error during generation")

    assert enc.device == "cpu"


def test_precompute_text_cache_with_cleanup(tmp_path: Path):
    cache = TextEmbedsCache(tmp_path, enabled=True)
    captions = ["a photo of a cat", "a photo of a dog"]
    enc = MockEncoder("cuda:0")

    def mock_encode(caps: list[str]) -> dict[str, Any]:
        return {"hidden": torch.ones(len(caps), 4, 8)}

    _precompute_text_cache_with_cleanup(
        cache,
        captions,
        mock_encode,
        batch_size=2,
        unload_encoders=True,
        encoders=[enc],
    )

    assert enc.device == "cpu"
    assert cache.get("a photo of a cat") is not None
    assert cache.get("a photo of a dog") is not None


def test_cached_encode_hit_and_miss(tmp_path: Path, monkeypatch):
    import trainer_difusao.common_pkg.text_embeds as mod
    monkeypatch.setattr(mod, "ENABLE_TEXT_ENCODER_UNLOAD", True)

    cache = TextEmbedsCache(tmp_path, enabled=True)
    cache.put("cached_prompt", {"hidden": torch.zeros(4)})

    enc = MockEncoder("cpu")

    def mock_encode(caps: list[str]) -> dict[str, Any]:
        # On miss, encoder must be temporarily on target_device
        assert enc.device == "cuda:0"
        return {"hidden": torch.ones(len(caps), 4)}

    res = _cached_encode(
        ["cached_prompt", "miss_prompt"],
        mock_encode,
        cache,
        encoders=[enc],
        device="cuda:0",
    )

    assert "hidden" in res
    assert res["hidden"].shape[0] == 2
    # After cached_encode finishes, enc is back on cpu
    assert enc.device == "cpu"


def test_precompute_sample_embeds_sd15():
    class DummyTokenizer:
        model_max_length = 77

        def __call__(self, text, **kwargs):
            class Output:
                input_ids = torch.zeros(1, 77, dtype=torch.long)
            return Output()

    class DummyTextEncoder:
        def __call__(self, input_ids):
            return [torch.ones(1, 77, 768)]

    embeds = _precompute_sample_embeds_sd15(
        DummyTokenizer(),
        DummyTextEncoder(),
        "a cat in space",
        device="cpu",
        dtype=torch.float32,
    )
    assert "prompt_embeds" in embeds
    assert "negative_prompt_embeds" in embeds
    assert embeds["prompt_embeds"].shape == (1, 77, 768)
    assert embeds["negative_prompt_embeds"].shape == (1, 77, 768)


def test_precompute_sample_embeds_sdxl():
    class DummyTokenizer:
        model_max_length = 77

        def __call__(self, text, **kwargs):
            class Output:
                input_ids = torch.zeros(1, 77, dtype=torch.long)
            return Output()

    class DummyEncoderOne:
        def __call__(self, input_ids, output_hidden_states=False):
            class Out:
                hidden_states = [None, torch.ones(1, 77, 768), torch.ones(1, 77, 768)]
            return Out()

    class DummyEncoderTwo:
        def __call__(self, input_ids, output_hidden_states=False):
            class Out:
                hidden_states = [None, torch.ones(1, 77, 1280), torch.ones(1, 77, 1280)]
                text_embeds = torch.ones(1, 1280)
            return Out()

    embeds = _precompute_sample_embeds_sdxl(
        DummyTokenizer(),
        DummyTokenizer(),
        DummyEncoderOne(),
        DummyEncoderTwo(),
        "a modern architectural house",
        device="cpu",
        dtype=torch.float32,
    )
    assert "prompt_embeds" in embeds
    assert "pooled_prompt_embeds" in embeds
    assert "negative_prompt_embeds" in embeds
    assert "negative_pooled_prompt_embeds" in embeds
    assert embeds["prompt_embeds"].shape == (1, 77, 2048)
    assert embeds["pooled_prompt_embeds"].shape == (1, 1280)
    class DummyTokenizerOne:
        model_max_length = 77

        def __call__(self, text, **kwargs):
            class Output:
                input_ids = torch.zeros(1, 77, dtype=torch.long)
                def to(self, device): return self
            return Output()

    class DummyTokenizerTwo:
        def __call__(self, text, **kwargs):
            class Output:
                input_ids = torch.zeros(1, 512, dtype=torch.long)
                def to(self, device): return self
            return Output()

    class DummyEncoderOne:
        def __call__(self, input_ids):
            class Out:
                pooler_output = torch.ones(1, 768)
            return Out()

    class DummyEncoderTwo:
        def __call__(self, input_ids):
            return [torch.ones(1, 512, 4096)]

    embeds = _precompute_sample_embeds_flux(
        DummyTokenizerOne(),
        DummyTokenizerTwo(),
        DummyEncoderOne(),
        DummyEncoderTwo(),
        "a majestic lion",
        device="cpu",
        is_flux2=False,
        dtype=torch.float32,
    )
    assert "prompt_embeds" in embeds
    assert "pooled_prompt_embeds" in embeds
    assert embeds["prompt_embeds"].shape == (1, 512, 4096)
    assert embeds["pooled_prompt_embeds"].shape == (1, 768)


def test_namespace_absent_env_uses_legacy_path(tmp_path: Path, monkeypatch):
    """Sem TEXT_EMBEDS_CACHE_DIR, o cache usa {output}/text_embeds_cache/ (legado)."""
    monkeypatch.delenv("TEXT_EMBEDS_CACHE_DIR", raising=False)
    cache = TextEmbedsCache(tmp_path, enabled=True, namespace_fields={"model_id": "x"})
    assert cache.dir == tmp_path / "text_embeds_cache"
    cache.put("a caption", {"hidden": torch.zeros(2)})
    assert cache.get("a caption") is not None
    assert not (cache.dir / "namespace.json").exists()


def test_namespace_differs_per_field(tmp_path: Path, monkeypatch):
    """Cada campo que afeta o embedding (model, encoder md5, quantização, seq
    len, dtype) produz um namespace distinto."""
    monkeypatch.setenv("TEXT_EMBEDS_CACHE_DIR", str(tmp_path))
    base = {
        "model_id": "black-forest-labs/FLUX.1-dev",
        "text_encoder_identity": None,
        "quant_format": "4bit",
        "max_sequence_length": 512,
        "dtype": "torch.bfloat16",
    }
    base_cache = TextEmbedsCache(tmp_path, enabled=True, namespace_fields=dict(base))

    variants = [
        {**base, "model_id": "other/model"},
        {**base, "text_encoder_identity": "/weights/text_encoder.safetensors#deadbeef"},
        {**base, "quant_format": "8bit"},
        {**base, "max_sequence_length": 256},
        {**base, "dtype": "torch.float16"},
    ]
    dirs = {base_cache.dir}
    for variant in variants:
        c = TextEmbedsCache(tmp_path, enabled=True, namespace_fields=variant)
        assert c.dir not in dirs, f"namespace colidiu para variante {variant}"
        dirs.add(c.dir)


def test_namespace_same_fields_same_dir_and_second_instance_hits(tmp_path: Path, monkeypatch):
    """Mesmos campos → mesmo namespace; uma segunda instância lê o arquivo
    gravado pela primeira (cache compartilhado entre processos/jobs)."""
    monkeypatch.setenv("TEXT_EMBEDS_CACHE_DIR", str(tmp_path))
    fields = {"model_id": "m", "quant_format": "none", "dtype": "torch.float16"}

    cache_a = TextEmbedsCache(tmp_path, enabled=True, namespace_fields=dict(fields))
    cache_a.put("hello", {"hidden": torch.ones(3)})

    cache_b = TextEmbedsCache(tmp_path, enabled=True, namespace_fields=dict(fields))
    assert cache_b.dir == cache_a.dir
    hit = cache_b.get("hello")
    assert hit is not None
    assert torch.equal(hit["hidden"], torch.ones(3))


def test_namespace_dir_perms_and_namespace_json(tmp_path: Path, monkeypatch):
    """Diretório do namespace nasce 0o777 (compartilhado uid 1000, PITFALLS:51)
    e `namespace.json` reflete exatamente os campos usados na chave."""
    monkeypatch.setenv("TEXT_EMBEDS_CACHE_DIR", str(tmp_path))
    fields = {"model_id": "m", "quant_format": "4bit", "dtype": "torch.bfloat16"}
    cache = TextEmbedsCache(tmp_path, enabled=True, namespace_fields=dict(fields))
    cache.put("hi", {"hidden": torch.ones(1)})

    meta_path = cache.dir / "namespace.json"
    assert meta_path.exists()
    import json

    data = json.loads(meta_path.read_text(encoding="utf-8"))
    assert data["model_id"] == "m"
    assert data["quant_format"] == "4bit"
    assert data["dtype"] == "torch.bfloat16"
    assert "_schema_version" in data

    import stat

    mode = stat.S_IMODE(cache.dir.stat().st_mode)
    assert mode == 0o777


def test_corrupt_pt_file_treated_as_miss_and_rewritten(tmp_path: Path, monkeypatch):
    """Arquivo .pt corrompido (ex.: escrita concorrente truncada) é tratado
    como miss; put() subsequente reescreve com sucesso."""
    monkeypatch.delenv("TEXT_EMBEDS_CACHE_DIR", raising=False)
    cache = TextEmbedsCache(tmp_path, enabled=True)
    cache.dir.mkdir(parents=True, exist_ok=True)
    from trainer_difusao.common_pkg.train_config import _caption_cache_key

    corrupt_path = cache.dir / f"{_caption_cache_key('broken')}.pt"
    corrupt_path.write_bytes(b"not a real torch file")

    assert cache.get("broken") is None
    assert not cache._broken  # corrupção de UM arquivo não derruba o cache inteiro

    cache.put("broken", {"hidden": torch.ones(1)})
    hit = cache.get("broken")
    assert hit is not None
    assert torch.equal(hit["hidden"], torch.ones(1))


def test_get_hit_touches_mtime_for_lru(tmp_path: Path, monkeypatch):
    """Hit em get() atualiza mtime (os.utime) para refletir uso no LRU do sweeper."""
    monkeypatch.setenv("TEXT_EMBEDS_CACHE_DIR", str(tmp_path))
    cache = TextEmbedsCache(tmp_path, enabled=True, namespace_fields={"model_id": "m"})
    cache.put("caption", {"hidden": torch.ones(1)})
    from trainer_difusao.common_pkg.train_config import _caption_cache_key

    path = cache.dir / f"{_caption_cache_key('caption')}.pt"
    import os as _os

    old_time = path.stat().st_mtime
    _os.utime(path, (old_time - 1000, old_time - 1000))
    assert path.stat().st_mtime == old_time - 1000

    assert cache.get("caption") is not None
    assert path.stat().st_mtime > old_time - 1000
