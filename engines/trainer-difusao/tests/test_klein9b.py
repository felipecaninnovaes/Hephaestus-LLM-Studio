"""FLUX.2 Klein 9B (base): variantes 4B/9B coexistindo, treino e geração (CPU, ENGINE_MOCK=1).

O arch do config decide o repo (envs são por variante), o unload do text encoder é
forçado no 9B e o metadata do adapter carrega `base_model` por arch.
"""
from __future__ import annotations

import json
import struct
from pathlib import Path
from types import SimpleNamespace

import pytest
import yaml

from trainer_difusao.common import _canonical_model_name
from trainer_difusao.generation.config import load_and_validate_generate_config
from trainer_difusao.generation.pipelines import ensure_pipeline, pipeline_cache_key
from trainer_difusao.generation.text_encoder import _flux2_repo_id
from trainer_difusao.klein import (
    KLEIN_4B,
    KLEIN_9B,
    canonical_klein_arch,
    klein_repo_id,
)
from trainer_difusao.serve_pkg.state import _make_spec, _spec_matches
from trainer_difusao.train import main

REPO_4B = "black-forest-labs/FLUX.2-klein-base-4B"
REPO_4B_DISTILLED = "black-forest-labs/FLUX.2-klein-4B"
REPO_9B = "black-forest-labs/FLUX.2-klein-base-9B"
_ENVS = ("FLUX_MODEL_ID", "FLUX_DISTILLED_MODEL_ID", "FLUX_9B_MODEL_ID")


@pytest.fixture(autouse=True)
def _clean_env(monkeypatch):
    for name in _ENVS:
        monkeypatch.delenv(name, raising=False)
    monkeypatch.setenv("ENGINE_MOCK", "1")


# --------------------------------------------------------------- variant resolution
def test_default_repos_per_arch():
    assert klein_repo_id(KLEIN_4B) == REPO_4B
    assert klein_repo_id(KLEIN_4B, distilled=True) == REPO_4B_DISTILLED
    assert klein_repo_id(KLEIN_9B) == REPO_9B


def test_env_override_is_per_arch_and_coexists(monkeypatch):
    monkeypatch.setenv("FLUX_MODEL_ID", "org/custom-4b")
    monkeypatch.setenv("FLUX_DISTILLED_MODEL_ID", "org/custom-4b-distilled")
    monkeypatch.setenv("FLUX_9B_MODEL_ID", "org/custom-9b")
    # 4B e 9B no mesmo nó: cada arch lê só a própria env.
    assert klein_repo_id(KLEIN_4B) == "org/custom-4b"
    assert klein_repo_id(KLEIN_4B, distilled=True) == "org/custom-4b-distilled"
    assert klein_repo_id(KLEIN_9B) == "org/custom-9b"


def test_flux_model_id_env_never_leaks_into_9b(monkeypatch):
    monkeypatch.setenv("FLUX_MODEL_ID", "org/custom-4b")
    assert klein_repo_id(KLEIN_9B) == REPO_9B
    assert _flux2_repo_id(distilled=False, arch=KLEIN_9B) == REPO_9B
    assert _flux2_repo_id(distilled=False) == "org/custom-4b"  # default arch = 4B


def test_9b_env_never_leaks_into_4b(monkeypatch):
    monkeypatch.setenv("FLUX_9B_MODEL_ID", "org/custom-9b")
    assert klein_repo_id(KLEIN_4B) == REPO_4B
    assert _flux2_repo_id(distilled=True, arch=KLEIN_4B) == REPO_4B_DISTILLED


def test_9b_has_no_distilled_repo():
    with pytest.raises(ValueError):
        klein_repo_id(KLEIN_9B, distilled=True)
    with pytest.raises(SystemExit):
        _flux2_repo_id(distilled=True, arch=KLEIN_9B)


def test_unknown_arch_is_not_klein():
    with pytest.raises(ValueError):
        klein_repo_id("sdxl")
    assert canonical_klein_arch("sdxl") is None
    assert canonical_klein_arch(None) is None


@pytest.mark.parametrize(
    "raw,expected",
    [
        ("flux", KLEIN_4B),
        ("flux2", KLEIN_4B),
        ("flux-2-klein-4b", KLEIN_4B),
        ("flux-2-klein-9b", KLEIN_9B),
        ("FLUX2-Klein-9B", KLEIN_9B),
        ("flux.2-klein-9b", KLEIN_9B),
    ],
)
def test_canonical_model_name_klein(raw, expected):
    assert _canonical_model_name(raw) == expected


# ------------------------------------------------------------------ training adapter
@pytest.fixture
def adapter_env(monkeypatch, tmp_path):
    torch = pytest.importorskip("torch")
    pytest.importorskip("transformers")
    import trainer_difusao.models.flux_adapter as fa

    seen: list[str] = []

    def _fake_quant_base(model_id, **kw):
        seen.append(model_id)
        return tmp_path / "quant" / model_id.replace("/", "_")

    monkeypatch.setattr(fa, "resolve_quant_base_dir", _fake_quant_base)
    monkeypatch.setattr(torch.cuda, "is_bf16_supported", lambda *a, **k: False, raising=False)
    return fa, seen


def _train_cfg(model: str, **extra):
    return {
        "job_id": "j",
        "model": model,
        "dataset_path": "/datasets/ds",
        "cache_text_embeddings": False,
        "lora": {"epochs": 1, "rank": 16, "alpha": 16, "quantization": "4bit", "resolution": 768},
        **extra,
    }


def test_parse_9b_selects_9b_repo_and_forces_unload(adapter_env, monkeypatch):
    fa, seen = adapter_env
    monkeypatch.setenv("FLUX_MODEL_ID", "org/custom-4b")  # não pode vazar p/ 9B
    tcfg = fa.FluxAdapter().parse_lora_train_config(_train_cfg("flux-2-klein-9b"))
    assert tcfg.model_id == REPO_9B
    assert tcfg.extra["is_flux2"] is True
    assert tcfg.extra["klein_arch"] == KLEIN_9B
    assert tcfg.extra["force_text_encoder_unload"] is True
    assert seen == [REPO_9B]


def test_parse_9b_env_override(adapter_env, monkeypatch):
    fa, _ = adapter_env
    monkeypatch.setenv("FLUX_9B_MODEL_ID", "org/mirror-9b")
    tcfg = fa.FluxAdapter().parse_lora_train_config(_train_cfg("flux-2-klein-9b"))
    assert tcfg.model_id == "org/mirror-9b"


@pytest.mark.parametrize("model", ["flux-2-klein-4b", "flux"])
def test_parse_4b_unchanged(adapter_env, model):
    fa, _ = adapter_env
    tcfg = fa.FluxAdapter().parse_lora_train_config(_train_cfg(model))
    assert tcfg.model_id == REPO_4B
    assert tcfg.extra["klein_arch"] == KLEIN_4B
    assert tcfg.extra["force_text_encoder_unload"] is False


def test_parse_4b_honors_flux_model_id_env(adapter_env, monkeypatch):
    fa, _ = adapter_env
    monkeypatch.setenv("FLUX_MODEL_ID", "org/custom-4b-klein")
    tcfg = fa.FluxAdapter().parse_lora_train_config(_train_cfg("flux-2-klein-4b"))
    assert tcfg.model_id == "org/custom-4b-klein"
    assert tcfg.extra["is_flux2"] is True


@pytest.mark.parametrize(
    "model,expected_base",
    [("flux-2-klein-9b", "flux-2-klein-9b"), ("flux-2-klein-4b", "flux-2-klein-4b")],
)
def test_checkpoint_metadata_base_model_per_arch(adapter_env, model, expected_base):
    fa, _ = adapter_env
    adapter = fa.FluxAdapter()
    tcfg = adapter.parse_lora_train_config(_train_cfg(model))
    meta = adapter.checkpoint_metadata(tcfg, epoch=2)
    assert meta["base_model"] == expected_base
    assert meta["epoch"] == "2"
    assert meta["lora_rank"] == "16"


def test_parse_9b_rejects_custom_checkpoint_and_text_encoder(adapter_env, tmp_path, capsys):
    fa, _ = adapter_env
    ckpt = tmp_path / "m.safetensors"
    ckpt.write_bytes(b"x")
    with pytest.raises(SystemExit):
        fa.FluxAdapter().parse_lora_train_config(
            _train_cfg("flux-2-klein-9b", custom_checkpoint_path=str(ckpt))
        )
    assert "custom_checkpoint_path" in capsys.readouterr().err
    with pytest.raises(SystemExit):
        fa.FluxAdapter().parse_lora_train_config(
            _train_cfg("flux-2-klein-9b", text_encoder_path=str(tmp_path))
        )
    assert "text_encoder_path" in capsys.readouterr().err


def test_parse_4b_still_accepts_text_encoder_path(adapter_env, tmp_path):
    fa, _ = adapter_env
    tcfg = fa.FluxAdapter().parse_lora_train_config(
        _train_cfg("flux-2-klein-4b", text_encoder_path=str(tmp_path))
    )
    assert tcfg.text_encoder_path == str(tmp_path)


# ---------------------------------------------------- forced unload in the train loop
class _Enc:
    def __init__(self):
        self.device = "cuda"

    def to(self, device):
        self.device = device
        return self


def _unload_tcfg(*, force: bool, epoch_offset: int = 0):
    return SimpleNamespace(
        extra={"force_text_encoder_unload": force}, epoch_offset=epoch_offset
    )


@pytest.mark.parametrize("flag", [False, True])
@pytest.mark.parametrize("cache", [False, True])
@pytest.mark.parametrize("epoch_offset", [0, 3])
def test_unload_forced_for_9b_regardless_of_env_flag_cache_and_resume(
    monkeypatch, flag, cache, epoch_offset
):
    pytest.importorskip("torch")
    from trainer_difusao.models import loop as loop_mod

    monkeypatch.setattr(loop_mod, "ENABLE_TEXT_ENCODER_UNLOAD", flag)
    out = loop_mod.resolve_text_encoder_unload(
        _unload_tcfg(force=True, epoch_offset=epoch_offset), cache
    )
    assert out == (True, True, True)  # cache ligado, forçado, descarrega


def test_unload_default_flags_unchanged_for_4b(monkeypatch):
    pytest.importorskip("torch")
    from trainer_difusao.models import loop as loop_mod

    monkeypatch.setattr(loop_mod, "ENABLE_TEXT_ENCODER_UNLOAD", False)
    assert loop_mod.resolve_text_encoder_unload(_unload_tcfg(force=False), True) == (True, False, False)
    assert loop_mod.resolve_text_encoder_unload(_unload_tcfg(force=False), False) == (False, False, False)
    monkeypatch.setattr(loop_mod, "ENABLE_TEXT_ENCODER_UNLOAD", True)
    assert loop_mod.resolve_text_encoder_unload(_unload_tcfg(force=False), True) == (True, False, True)
    # resume (epoch_offset>0) mantém o comportamento legado: sem unload
    assert loop_mod.resolve_text_encoder_unload(_unload_tcfg(force=False, epoch_offset=2), True) == (
        True,
        False,
        False,
    )
    assert loop_mod.resolve_text_encoder_unload(_unload_tcfg(force=False), False) == (False, False, False)


def test_temporary_device_encoders_force_ignores_env_flag(monkeypatch):
    import trainer_difusao.common_pkg.text_embeds as mod

    monkeypatch.setattr(mod, "ENABLE_TEXT_ENCODER_UNLOAD", False)
    enc = _Enc()
    enc.device = "cpu"
    with mod._temporary_device_encoders([enc], "cuda:0", force=True):
        assert enc.device == "cuda:0"
    assert enc.device == "cpu"
    with mod._temporary_device_encoders([enc], "cuda:0"):
        assert enc.device == "cpu"


# ------------------------------------------------------------------ mock train 9B
def _read_meta(adapter_file: Path) -> dict:
    data = adapter_file.read_bytes()
    header_len = struct.unpack("<Q", data[:8])[0]
    return json.loads(data[8 : 8 + header_len].decode("utf-8"))["__metadata__"]


def _run_main(tmp_path, mode, cfg):
    cfg_path = tmp_path / "config.yaml"
    cfg_path.write_text(yaml.dump(cfg), encoding="utf-8")
    out = tmp_path / "out"
    main([mode, "--config", str(cfg_path), "--output", str(out)])
    return out


@pytest.mark.parametrize("model", ["flux-2-klein-9b", "flux-2-klein-4b"])
def test_mock_train_metadata_per_arch(tmp_path, model):
    out = _run_main(
        tmp_path,
        "train",
        {"job_id": "t", "model": model, "seed": 1, "lora": {"epochs": 2, "rank": 16, "alpha": 16}},
    )
    meta = _read_meta(out / "adapter.safetensors")
    assert meta["base_model"] == model
    assert meta["lora_rank"] == "16"


def test_mock_train_9b_rejects_custom_options(tmp_path, capsys):
    for key in ("custom_checkpoint_path", "text_encoder_path"):
        with pytest.raises(SystemExit):
            _run_main(
                tmp_path,
                "train",
                {"job_id": "t", "model": "flux-2-klein-9b", "lora": {"epochs": 1}, key: "/x/y.safetensors"},
            )
        assert key in capsys.readouterr().err


# --------------------------------------------------------------- generation config
def _gen_cfg(**gen):
    return {"job_id": "g", "generate": {"prompt": "p", "base_model": "flux-2-klein-9b", **gen}}


def test_generate_config_accepts_9b():
    params = load_and_validate_generate_config(_gen_cfg(quantization="4bit", sampler="euler"))
    assert params["base_model"] == KLEIN_9B
    assert params["guidance_scale"] == 3.5
    assert params["distilled"] is False


def test_generate_config_9b_via_alias_and_model_fallback():
    p = load_and_validate_generate_config({"job_id": "g", "model": "flux2-klein-9b", "generate": {"prompt": "p"}})
    assert p["base_model"] == KLEIN_9B


@pytest.mark.parametrize("sampler", ["default", "euler", "heun"])
def test_generate_config_9b_flux2_sampler_allowlist(sampler):
    assert load_and_validate_generate_config(_gen_cfg(sampler=sampler))["sampler"] == sampler


@pytest.mark.parametrize("sampler", ["dpmpp_2m", "euler_a", "ddim"])
def test_generate_config_9b_rejects_sd_samplers(sampler):
    with pytest.raises(SystemExit):
        load_and_validate_generate_config(_gen_cfg(sampler=sampler))


def test_generate_config_9b_rejects_distilled(capsys):
    with pytest.raises(SystemExit):
        load_and_validate_generate_config(_gen_cfg(distilled=True))
    assert "distilled" in capsys.readouterr().err


def test_generate_config_9b_rejects_text_encoder_path(capsys):
    with pytest.raises(SystemExit):
        load_and_validate_generate_config(_gen_cfg(text_encoder_path="/enc"))
    assert "text_encoder_path" in capsys.readouterr().err


def test_generate_config_9b_rejects_custom_checkpoint(capsys):
    cfg = {
        "job_id": "g",
        "generate": {"prompt": "p", "custom_checkpoint_path": "/m.safetensors", "arch": "flux-2-klein-9b"},
    }
    with pytest.raises(SystemExit):
        load_and_validate_generate_config(cfg)
    assert "custom_checkpoint_path" in capsys.readouterr().err


def test_generate_config_4b_distilled_and_encoder_still_accepted():
    p = load_and_validate_generate_config(
        {"job_id": "g", "generate": {"prompt": "p", "base_model": "flux-2-klein-4b", "distilled": True, "text_encoder_path": "/enc"}}
    )
    assert p["distilled"] is True and p["text_encoder_path"] == "/enc"


def test_runner_rejects_9b_distilled_even_without_config_validation(tmp_path):
    pytest.importorskip("torch")
    from trainer_difusao.generation.runner import _real_generate

    params = load_and_validate_generate_config(_gen_cfg())
    params["distilled"] = True
    with pytest.raises(SystemExit):
        _real_generate(params, tmp_path / "o")


# ------------------------------------------------------------ daemon spec / cache key
def test_pipeline_cache_key_and_spec_differ_between_4b_and_9b():
    p4 = {"base_model": KLEIN_4B, "quantization": "4bit", "distilled": False}
    p9 = {"base_model": KLEIN_9B, "quantization": "4bit", "distilled": False}
    assert pipeline_cache_key(p4) != pipeline_cache_key(p9)
    assert not _spec_matches(_make_spec(p4), _make_spec(p9))
    cache = {pipeline_cache_key(p4): object()}
    assert ensure_pipeline(p9, cache)[0] is None
    assert ensure_pipeline(p4, cache)[0] is not None


def test_lora_component_prefix_for_9b_matches_4b():
    from trainer_difusao.generation.adapters import lora_component_prefix

    assert lora_component_prefix(KLEIN_9B) == lora_component_prefix(KLEIN_4B) == "transformer."


def test_lora_remap_applies_to_9b_adapter(tmp_path):
    torch = pytest.importorskip("torch")
    safetensors_torch = pytest.importorskip("safetensors.torch")
    from trainer_difusao.generation.adapters import prepare_lora_for_load

    path = tmp_path / "a.safetensors"
    safetensors_torch.save_file(
        {"single_transformer_blocks.0.attn.to_q.lora_A.weight": torch.zeros(4, 8)}, str(path)
    )
    out = prepare_lora_for_load(str(path), KLEIN_9B)
    assert list(out) == ["transformer.single_transformer_blocks.0.attn.to_q.lora_A.weight"]


def test_adapters_target_transformer_for_9b_pipeline():
    from trainer_difusao.generation.adapters import DaemonLoraCache

    transformer = SimpleNamespace(calls=[])
    transformer.disable_lora = lambda: transformer.calls.append("disable")
    transformer.set_adapters = lambda names, *a: transformer.calls.append(("set", names))
    pipe = SimpleNamespace(transformer=transformer)
    DaemonLoraCache().apply_loras(pipe, [], KLEIN_9B)
    assert transformer.calls == ["disable", ("set", [])]


# ------------------------------------------------------------- mock generate (9B)
def test_mock_generate_9b_writes_meta_with_arch(tmp_path):
    PIL = pytest.importorskip("PIL.Image")
    out = _run_main(
        tmp_path,
        "generate",
        {
            "job_id": "g",
            "generate": {
                "base_model": "flux-2-klein-9b",
                "prompt": "a red circle",
                "width": 512,
                "height": 512,
                "steps": 4,
                "seed": 7,
                "quantization": "4bit",
            },
        },
    )
    png = out / "generated.png"
    assert png.exists()
    with PIL.open(png) as img:
        assert img.size == (512, 512)
    entry = json.loads((out / "generation_meta.json").read_text().splitlines()[0])
    assert entry["base_model"] == KLEIN_9B


# ------------------------------------------------- runner wiring (real path, mocked)
def test_runner_9b_uses_9b_repo_with_quantized_components(monkeypatch):
    """_real_generate 9B: repo 9B (mesmo com FLUX_MODEL_ID 4B no ambiente) + quant real."""
    pytest.importorskip("torch")

    from test_generate_quant_lora_fix import TestRunnerKleinQuantWiring

    monkeypatch.setenv("FLUX_MODEL_ID", "org/custom-4b")
    helper = TestRunnerKleinQuantWiring()
    for base, quant, repo in (
        (KLEIN_9B, "4bit", REPO_9B),
        (KLEIN_4B, "4bit", "org/custom-4b"),
    ):
        captured, m_quant, sentinel = helper._generate(
            helper._params(base_model=base, quantization=quant)
        )
        assert captured["repo"] == repo
        assert m_quant.call_args.args[:2] == (repo, quant)
        for k, v in sentinel.items():
            assert captured["kwargs"][k] is v
