"""Testes unitários para cache de text embeddings e offload de text encoders."""
from __future__ import annotations

from pathlib import Path
from typing import Any
import pytest
import torch

from trainer_difusao.common_pkg.text_embeds import (
    ENABLE_TEXT_ENCODER_UNLOAD,
    TextEmbedsCache,
    _cached_encode,
    _cleanup_encoders,
    _offload_encoders_to_cpu,
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
