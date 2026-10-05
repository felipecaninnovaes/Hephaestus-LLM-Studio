"""Testes do cache de latents do VAE (distribuição mean/std em disco, VAE fora da GPU)."""
from __future__ import annotations

import json
from pathlib import Path

import pytest

torch = pytest.importorskip("torch")
from diffusers import AutoencoderKL, DDPMScheduler, UNet2DConditionModel  # noqa: E402
from PIL import Image  # noqa: E402

from trainer_difusao.common_pkg.latent_cache import (  # noqa: E402
    LatentCache,
    batch_pixel_hw,
    latents_from_batch,
    prepare_latent_cache,
    resolve_cache_latents,
    sample_latent_dist,
    vae_encode_dist,
    vae_fingerprint,
    vae_on_device,
)
from trainer_difusao.dataset import DiffusionDataset, build_dataloader  # noqa: E402
from trainer_difusao.models.loop import (  # noqa: E402
    TrainingLoopRunner,
    parse_lora_train_config,
)
from trainer_difusao.models.sd_family.adapter import SD15Adapter  # noqa: E402


def _tiny_vae(seed: int = 0) -> AutoencoderKL:
    torch.manual_seed(seed)
    vae = AutoencoderKL(
        block_out_channels=(8, 16),
        down_block_types=("DownEncoderBlock2D",) * 2,
        up_block_types=("UpDecoderBlock2D",) * 2,
        latent_channels=4,
        layers_per_block=1,
        norm_num_groups=4,
        sample_size=64,
    )
    vae.requires_grad_(False)
    return vae.eval()


def _make_dataset_dir(root: Path) -> Path:
    """6 imagens sintéticas em 2 buckets: 3 em 64x64 e 3 em 64x128 (retrato)."""
    images = root / "images"
    images.mkdir(parents=True)
    rng = torch.Generator().manual_seed(123)
    for i in range(6):
        size = (64, 64) if i < 3 else (64, 128)
        arr = (torch.rand((size[1], size[0], 3), generator=rng) * 255).to(torch.uint8).numpy()
        Image.fromarray(arr).save(images / f"img_{i}.png")
        (images / f"img_{i}.txt").write_text(f"caption {i}", encoding="utf-8")
    return root


class _CountingVae:
    """Envolve o VAE contando encode() e registrando cada .to(device)."""

    def __init__(self, vae: AutoencoderKL) -> None:
        self._vae = vae
        self.encode_calls = 0
        self.to_calls: list[str] = []

    def encode(self, x):
        self.encode_calls += 1
        return self._vae.encode(x)

    def to(self, device):
        self.to_calls.append(str(device))
        if str(device) == "cpu":  # sem GPU aqui: devices CUDA só são registrados
            self._vae.to(device)
        return self

    def __getattr__(self, name):
        return getattr(self._vae, name)


@pytest.fixture(autouse=True)
def _no_dataloader_workers(monkeypatch):
    monkeypatch.setenv("DIFFUSION_DATALOADER_WORKERS", "0")


def _cache(tmp_path: Path, **overrides) -> LatentCache:
    fields = {"arch": "sd15", "model_id": "m", "vae_fingerprint": "f", "dtype": "torch.float32"}
    fields.update(overrides)
    return LatentCache(tmp_path / "out", True, namespace_fields=fields)


# --- chave --------------------------------------------------------------------


def test_key_changes_with_image_bucket_vae_and_dtype(tmp_path: Path) -> None:
    a = tmp_path / "a.png"
    b = tmp_path / "b.png"
    Image.new("RGB", (64, 64), (10, 20, 30)).save(a)
    Image.new("RGB", (64, 64), (200, 20, 30)).save(b)

    cache = _cache(tmp_path)
    base = cache.path_for(cache.key_for(a, (64, 64)))

    # mesma imagem + mesmo bucket => mesma chave (estável)
    assert cache.path_for(cache.key_for(a, (64, 64))) == base
    # imagem diferente
    assert cache.path_for(cache.key_for(b, (64, 64))) != base
    # bucket diferente
    assert cache.path_for(cache.key_for(a, (64, 128))) != base
    # VAE diferente (fingerprint), repo diferente, dtype diferente => namespace/dir diferente
    for override in (
        {"vae_fingerprint": "outro"},
        {"model_id": "outro/repo"},
        {"dtype": "torch.bfloat16"},
        {"custom_checkpoint_identity": "x#1"},
    ):
        other = _cache(tmp_path, **override)
        assert other.path_for(other.key_for(a, (64, 64))) != base, override
    # sem tocar nas demais: o conteúdo do arquivo (não o nome) define a chave
    a2 = tmp_path / "renomeada.png"
    a2.write_bytes(a.read_bytes())
    assert cache.path_for(cache.key_for(a2, (64, 64))) == base


def test_vae_fingerprint_changes_with_weights() -> None:
    assert vae_fingerprint(_tiny_vae(0)) == vae_fingerprint(_tiny_vae(0))
    assert vae_fingerprint(_tiny_vae(0)) != vae_fingerprint(_tiny_vae(1))


def test_cache_lives_inside_job_output(tmp_path: Path) -> None:
    cache = _cache(tmp_path)
    assert (tmp_path / "out") in cache.dir.parents
    assert cache.dir.parent.name == "latents_cache"


# --- amostragem ---------------------------------------------------------------


def test_sampling_matches_latent_dist_sample_same_seed() -> None:
    vae = _tiny_vae()
    x = torch.randn(2, 3, 64, 64)
    with torch.no_grad():
        dist = vae.encode(x).latent_dist
        mean, std = vae_encode_dist(vae, x)
        for seed in (0, 7):
            g1 = torch.Generator().manual_seed(seed)
            g2 = torch.Generator().manual_seed(seed)
            expected = dist.sample(generator=g1)
            got = sample_latent_dist(mean, std, g2)
            torch.testing.assert_close(got, expected, rtol=0, atol=0)
        # RNG global (como o treino): mesma semente => mesmo latent
        torch.manual_seed(5)
        expected = dist.sample()
        torch.manual_seed(5)
        got = sample_latent_dist(mean, std)
        torch.testing.assert_close(got, expected, rtol=0, atol=0)


def test_sampling_is_stochastic_across_calls() -> None:
    mean = torch.zeros(1, 4, 8, 8)
    std = torch.ones(1, 4, 8, 8)
    assert not torch.equal(sample_latent_dist(mean, std), sample_latent_dist(mean, std))


# --- cache em disco -----------------------------------------------------------


def test_prepare_encodes_each_image_once_and_reuses_on_second_run(tmp_path: Path) -> None:
    ds_root = _make_dataset_dir(tmp_path / "ds")
    vae = _CountingVae(_tiny_vae())
    ds = DiffusionDataset(ds_root, resolution=64, enable_bucket=True)
    assert len(ds.buckets) == 2
    cache = _cache(tmp_path)
    metrics = tmp_path / "out" / "metrics.jsonl"

    ok = prepare_latent_cache(
        cache, [ds], lambda px: vae_encode_dist(vae, px),
        device="cpu", dtype=torch.float32, metrics_path=metrics,
    )
    assert ok and vae.encode_calls == 6

    # Telemetria própria com step/totalSteps
    events = [
        json.loads(line)
        for line in (tmp_path / "out" / "telemetry.jsonl").read_text().splitlines()
        if line.strip()
    ]
    cl = [e for e in events if e["phase"] == "caching_latents"]
    assert cl and cl[-1]["step"] == 6 and cl[-1]["totalSteps"] == 6
    assert all(0.0 < e["progress"] <= 0.10 for e in cl)

    # Segunda execução (ex.: novo processo) reaproveita tudo
    ds2 = DiffusionDataset(ds_root, resolution=64, enable_bucket=True)
    assert prepare_latent_cache(
        cache, [ds2], lambda px: vae_encode_dist(vae, px), device="cpu", dtype=torch.float32
    )
    assert vae.encode_calls == 6


def test_corrupted_cache_entry_is_recomputed_with_log(tmp_path: Path, capsys) -> None:
    ds_root = _make_dataset_dir(tmp_path / "ds")
    vae = _CountingVae(_tiny_vae())
    ds = DiffusionDataset(ds_root, resolution=64, enable_bucket=True)
    cache = _cache(tmp_path)
    enc = lambda px: vae_encode_dist(vae, px)  # noqa: E731
    assert prepare_latent_cache(cache, [ds], enc, device="cpu", dtype=torch.float32)
    assert vae.encode_calls == 6

    victim = cache.path_for(cache.key_for(ds.samples[0][0], ds.bucket_dims[0]))
    victim.write_bytes(b"lixo truncado")

    ds2 = DiffusionDataset(ds_root, resolution=64, enable_bucket=True)
    assert prepare_latent_cache(cache, [ds2], enc, device="cpu", dtype=torch.float32)
    assert vae.encode_calls == 7  # só a entrada corrompida foi recomputada
    assert "corrompido" in capsys.readouterr().out
    assert cache.get(cache.key_for(ds.samples[0][0], ds.bucket_dims[0])) is not None


def test_put_is_atomic_no_tmp_left(tmp_path: Path) -> None:
    cache = _cache(tmp_path)
    assert cache.put("k", torch.zeros(4, 2, 2), torch.ones(4, 2, 2))
    assert [p.name for p in cache.dir.iterdir()] == ["k.pt"]


def test_write_failure_falls_back_to_online_vae(tmp_path: Path, monkeypatch) -> None:
    ds_root = _make_dataset_dir(tmp_path / "ds")
    vae = _tiny_vae()
    ds = DiffusionDataset(ds_root, resolution=64, enable_bucket=True)
    cache = _cache(tmp_path)
    monkeypatch.setattr(cache, "put", lambda *a, **k: False)
    ok = prepare_latent_cache(
        cache, [ds], lambda px: vae_encode_dist(vae, px), device="cpu", dtype=torch.float32
    )
    assert not ok
    assert "pixel_values" in ds[0] and "latent_mean" not in ds[0]


def test_cached_dataset_serves_distribution_and_shapes_match_old_path(tmp_path: Path) -> None:
    ds_root = _make_dataset_dir(tmp_path / "ds")
    vae = _tiny_vae()
    ds = DiffusionDataset(ds_root, resolution=64, enable_bucket=True)
    pixel_items = [ds[i] for i in range(len(ds))]
    cache = _cache(tmp_path)
    assert prepare_latent_cache(
        cache, [ds], lambda px: vae_encode_dist(vae, px), device="cpu", dtype=torch.float32
    )
    for i, old in enumerate(pixel_items):
        item = ds[i]
        assert "pixel_values" not in item
        with torch.no_grad():
            dist = vae.encode(old["pixel_values"].unsqueeze(0)).latent_dist
        assert tuple(item["latent_mean"].shape) == tuple(dist.mean[0].shape)
        torch.testing.assert_close(item["latent_mean"], dist.mean[0], rtol=0, atol=1e-6)
        torch.testing.assert_close(item["latent_std"], dist.std[0], rtol=0, atol=1e-6)
        assert (item["height"], item["width"]) == (
            old["pixel_values"].shape[1], old["pixel_values"].shape[2],
        )
    batch = next(iter(build_dataloader(ds, batch_size=2, seed=0)))
    assert batch_pixel_hw(batch) == (int(batch["height"][0]), int(batch["width"][0]))
    out = latents_from_batch(batch, "cpu", lambda px: pytest.fail("não deve usar o VAE"))
    assert tuple(out.shape) == tuple(batch["latent_mean"].shape)


def test_cache_stores_training_dtype(tmp_path: Path) -> None:
    ds_root = _make_dataset_dir(tmp_path / "ds")
    vae = _tiny_vae()
    ds = DiffusionDataset(ds_root, resolution=64, enable_bucket=True)
    cache = _cache(tmp_path, dtype="torch.bfloat16")
    assert prepare_latent_cache(
        cache, [ds], lambda px: vae_encode_dist(vae, px), device="cpu", dtype=torch.bfloat16
    )
    item = ds[0]
    # mean em fp32 (PITFALLS:78: nada de arredondamento duplo); só std no dtype de treino
    assert item["latent_mean"].dtype == torch.float32
    assert item["latent_std"].dtype == torch.bfloat16
    with torch.no_grad():
        ref = vae.encode(ds.load_pixel_values(0).unsqueeze(0)).latent_dist.mean[0]
    torch.testing.assert_close(item["latent_mean"], ref, rtol=0, atol=0)  # sem perda
    assert latents_from_batch(
        {"latent_mean": item["latent_mean"][None], "latent_std": item["latent_std"][None]},
        "cpu", lambda px: None,
    ).dtype == torch.float32  # amostra em fp32 como o caminho do VAE fp32


def test_schema_bump_invalidates_old_mean_dtype_caches(tmp_path: Path, monkeypatch) -> None:
    import trainer_difusao.common_pkg.latent_cache as lc

    new = _cache(tmp_path)
    monkeypatch.setattr(lc, "LATENT_CACHE_SCHEMA_VERSION", 1)  # v1 gravava mean arredondado
    old = _cache(tmp_path)
    assert new.dir != old.dir


# --- VAE offload --------------------------------------------------------------


def test_vae_on_device_moves_only_inside_block() -> None:
    vae = _CountingVae(_tiny_vae())
    with vae_on_device(vae, "cuda:0", offloaded=True):
        assert vae.to_calls == ["cuda:0"]
    assert vae.to_calls == ["cuda:0", "cpu"]

    vae2 = _CountingVae(_tiny_vae())
    with vae_on_device(vae2, "cuda:0", offloaded=False):
        pass
    assert vae2.to_calls == []


def test_vae_on_device_returns_to_cpu_on_error() -> None:
    vae = _CountingVae(_tiny_vae())
    with pytest.raises(RuntimeError):
        with vae_on_device(vae, "cuda:0", offloaded=True):
            raise RuntimeError("falha na amostra")
    assert vae.to_calls[-1] == "cpu"


# --- config -------------------------------------------------------------------


def test_cache_latents_config_default_true_and_opt_out() -> None:
    assert resolve_cache_latents({}) is True
    assert resolve_cache_latents({"cache_latents": False}) is False
    assert resolve_cache_latents({"lora": {"cache_latents": "false"}}) is False
    assert resolve_cache_latents({"cache_latents": "true"}) is True


def test_cache_latents_disabled_with_random_augmentation(capsys) -> None:
    assert resolve_cache_latents({}, augmentation_active=True) is False
    assert "augmentação" in capsys.readouterr().out


def test_lora_train_config_reads_cache_latents() -> None:
    kw = dict(default_model_id="m", default_resolution=64)
    assert parse_lora_train_config({}, **kw).cache_latents is True
    assert parse_lora_train_config({"cache_latents": False}, **kw).cache_latents is False
    assert parse_lora_train_config({"lora": {"cache_latents": False}}, **kw).cache_latents is False


# --- runner: offload do VAE + laço de 2 épocas --------------------------------


def _tiny_sd15_comp(vae) -> dict:
    torch.manual_seed(0)
    unet = UNet2DConditionModel(
        sample_size=32, in_channels=4, out_channels=4, layers_per_block=1,
        block_out_channels=(8, 16),
        down_block_types=("DownBlock2D", "CrossAttnDownBlock2D"),
        up_block_types=("CrossAttnUpBlock2D", "UpBlock2D"),
        cross_attention_dim=8, attention_head_dim=4, norm_num_groups=4,
    )
    return {
        "trainable_module": unet,
        "device": torch.device("cpu"),
        "dtype": torch.float32,
        "extra": {"vae": vae, "noise_scheduler": DDPMScheduler(num_train_timesteps=100)},
    }


def test_runner_offloads_vae_and_trains_two_epochs_with_one_encode_per_image(tmp_path: Path) -> None:
    ds_root = _make_dataset_dir(tmp_path / "ds")
    vae = _CountingVae(_tiny_vae())
    adapter = SD15Adapter()
    comp = _tiny_sd15_comp(vae)
    tcfg = adapter.parse_lora_train_config({"lora": {"resolution": 64, "batch_size": 2}})
    ds = DiffusionDataset(ds_root, resolution=64, enable_bucket=True)
    out = tmp_path / "job"
    out.mkdir()
    runner = TrainingLoopRunner(adapter)

    assert runner._setup_latent_cache(comp, tcfg, out, [ds], out / "metrics.jsonl") is True
    assert vae.encode_calls == 6
    assert vae.to_calls[-1] == "cpu"  # VAE saiu da GPU depois do pré-compute

    loader = build_dataloader(ds, batch_size=2, seed=0)
    cached_encode = {"hidden": torch.randn(2, 5, 8)}
    for _epoch in range(2):
        for batch in loader:
            enc = {"hidden": cached_encode["hidden"][: len(batch["prompt"])]}
            loss = adapter.forward_and_loss(comp, batch, tcfg, enc)
            assert torch.isfinite(loss)
            loss.backward()
    assert vae.encode_calls == 6  # antes: 12 (6 imagens x 2 épocas)
    assert vae.to_calls[-1] == "cpu"  # VAE permaneceu fora da GPU durante o laço


def test_runner_respects_cache_latents_false(tmp_path: Path) -> None:
    ds_root = _make_dataset_dir(tmp_path / "ds")
    vae = _CountingVae(_tiny_vae())
    adapter = SD15Adapter()
    comp = _tiny_sd15_comp(vae)
    tcfg = adapter.parse_lora_train_config(
        {"cache_latents": False, "lora": {"resolution": 64, "batch_size": 2}}
    )
    ds = DiffusionDataset(ds_root, resolution=64, enable_bucket=True)
    assert TrainingLoopRunner(adapter)._setup_latent_cache(
        comp, tcfg, tmp_path, [ds], tmp_path / "m.jsonl"
    ) is False
    assert vae.encode_calls == 0 and vae.to_calls == []
    assert "pixel_values" in ds[0]

