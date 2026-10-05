"""Paridade cache de latents x caminho online (VAE), por família, em CPU/fp32.

Mesma semente + VAE fp32 + dtype fp32 ⇒ o latent amostrado do cache é idêntico
(atol=0) ao de ``latent_dist.sample()``: ``DiagonalGaussianDistribution.sample``
é ``mean + std * randn`` com o mesmo gerador, e mean/std são gravados sem perda
(fp32). Os testes passam pelas funções de produção de cada família
(``forward_and_loss`` / ``_qwen_normalized_latents``), não por reimplementações.
"""
from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace

import pytest

torch = pytest.importorskip("torch")
from diffusers import AutoencoderKL, AutoencoderKLFlux2, DDPMScheduler  # noqa: E402
from torch.utils.data import default_collate  # noqa: E402

from test_latent_cache import _CountingVae, _make_dataset_dir  # noqa: E402
from trainer_difusao.common_pkg.latent_cache import (  # noqa: E402
    LatentCache,
    latents_from_batch,
    prepare_latent_cache,
    vae_encode_dist,
)
from trainer_difusao.dataset import DiffusionDataset, build_dataloader  # noqa: E402
from trainer_difusao.models.flux_adapter import FluxAdapter  # noqa: E402
from trainer_difusao.models.qwen_image import (  # noqa: E402
    _qwen_encode_dist,
    _qwen_normalized_latents,
)
from trainer_difusao.models.qwen_pkg.qwen_image_2 import AutoencoderKLQwenImage21  # noqa: E402
from trainer_difusao.models.sd_family.adapter import SDXLAdapter  # noqa: E402

SEEDS = (0, 11)


@pytest.fixture(autouse=True)
def _no_dataloader_workers(monkeypatch):
    # padrão determinístico/rápido; o teste (d) sobrescreve para 2 workers
    monkeypatch.setenv("DIFFUSION_DATALOADER_WORKERS", "0")


# --- fixtures / helpers --------------------------------------------------------


def _cache_for(tmp_path: Path) -> LatentCache:
    return LatentCache(
        tmp_path / "job",
        True,
        namespace_fields={"arch": "t", "model_id": "m", "vae_fingerprint": "f", "dtype": "fp32"},
    )


def _datasets(tmp_path: Path, vae, encode_dist, *, n_per_bucket: int = 3):
    """(dataset em modo pixels, dataset em modo cache) sobre as mesmas imagens."""
    root = _make_dataset_dir(tmp_path / "ds")
    ds_pix = DiffusionDataset(root, resolution=64, enable_bucket=True)
    ds_cache = DiffusionDataset(root, resolution=64, enable_bucket=True)
    ok = prepare_latent_cache(
        _cache_for(tmp_path), [ds_cache], encode_dist,
        device="cpu", dtype=torch.float32,
    )
    assert ok
    return ds_pix, ds_cache


def _bucket_batches(ds_pix, ds_cache, n: int = 1):
    """Um batch de ``n`` índices do mesmo bucket por bucket, nos dois modos.

    Com ``n=1`` o cache (codificado imagem a imagem) e o caminho online codificam
    exatamente o mesmo tensor ⇒ paridade bit a bit (atol=0). Com ``n>1`` o encode
    online em batch difere ~1 ulp do por-imagem (ordem de acumulação das convs).
    """
    for _bucket, idxs in sorted(ds_pix.buckets.items()):
        idxs = idxs[:n]
        yield default_collate([ds_pix[i] for i in idxs]), default_collate([ds_cache[i] for i in idxs])


def _tiny_kl(seed: int = 0, latent_channels: int = 4) -> AutoencoderKL:
    torch.manual_seed(seed)
    return AutoencoderKL(
        block_out_channels=(8, 16),
        down_block_types=("DownEncoderBlock2D",) * 2,
        up_block_types=("UpDecoderBlock2D",) * 2,
        latent_channels=latent_channels, layers_per_block=1, norm_num_groups=4, sample_size=64,
    ).requires_grad_(False).eval()


def _tiny_flux2_vae(seed: int = 0) -> AutoencoderKLFlux2:
    torch.manual_seed(seed)
    vae = AutoencoderKLFlux2(
        block_out_channels=(8, 16),
        down_block_types=("DownEncoderBlock2D",) * 2,
        up_block_types=("UpDecoderBlock2D",) * 2,
        latent_channels=4, layers_per_block=1, norm_num_groups=4, sample_size=64,
        patch_size=(2, 2),
    ).requires_grad_(False).eval()
    # Estatísticas de BN não triviais (o default é média 0 / var 1: não provaria nada)
    vae.bn.running_mean.copy_(torch.randn(16))
    vae.bn.running_var.copy_(torch.rand(16) + 0.5)
    return vae


def _tiny_qwen_vae(seed: int = 0) -> AutoencoderKLQwenImage21:
    torch.manual_seed(seed)
    return AutoencoderKLQwenImage21(
        base_dim=8, decoder_base_dim=8, z_dim=4, dim_mult=[1, 2], num_res_blocks=1,
        attn_scales=[], temperal_downsample=[False], is_residual=False,
        latents_mean=[0.5, -0.25, 0.1, 0.0], latents_std=[2.0, 1.5, 1.0, 0.5],
    ).requires_grad_(False).eval()


class _CaptureTransformer(torch.nn.Module):
    """Transformer falso: registra o que recebe e devolve zeros (loss = f(latents, ruído))."""

    def __init__(self) -> None:
        super().__init__()
        self.calls: list[dict] = []

    def forward(self, hidden_states, *args, **kwargs):
        self.calls.append({"hidden_states": hidden_states.clone(), **kwargs})
        return (torch.zeros_like(hidden_states),)


# --- (a) Qwen: VAE causal-3D tiny (5D + alpha) ----------------------------------


@pytest.mark.parametrize("seed", SEEDS)
def test_qwen_cached_equals_online_same_seed(tmp_path: Path, seed: int) -> None:
    vae = _CountingVae(_tiny_qwen_vae())
    ds_pix, ds_cache = _datasets(
        tmp_path, vae, lambda px: _qwen_encode_dist(vae, px, torch.float32)
    )
    encodes_after_precompute = vae.encode_calls
    assert encodes_after_precompute == 6
    for pix_batch, cache_batch in _bucket_batches(ds_pix, ds_cache):
        assert "pixel_values" not in cache_batch and "latent_mean" in cache_batch
        torch.manual_seed(seed)
        online = _qwen_normalized_latents(vae, pix_batch, "cpu", torch.float32)
        torch.manual_seed(seed)
        cached = _qwen_normalized_latents(vae, cache_batch, "cpu", torch.float32)
        assert online.ndim == 4 and online.shape[1] == 4  # frame dim removida
        assert cached.shape == online.shape and cached.dtype == online.dtype
        torch.testing.assert_close(cached, online, rtol=0, atol=0)
    # o caminho cacheado não tocou no VAE (só os encodes online do próprio teste)
    online_calls = vae.encode_calls - encodes_after_precompute
    assert online_calls == len(ds_pix.buckets)


@pytest.mark.parametrize("seed", SEEDS)
def test_qwen_cached_equals_dist_sample_with_generator(tmp_path: Path, seed: int) -> None:
    """Gerador explícito: cache == normalização manual de ``latent_dist.sample(generator)``."""
    from trainer_difusao.models.qwen_image import _qwen_vae_input

    vae = _tiny_qwen_vae()
    ds_pix, ds_cache = _datasets(
        tmp_path, vae, lambda px: _qwen_encode_dist(vae, px, torch.float32)
    )
    m = torch.tensor(vae.config.latents_mean).view(1, -1, 1, 1, 1)
    s = torch.tensor(vae.config.latents_std).view(1, -1, 1, 1, 1)
    for pix_batch, cache_batch in _bucket_batches(ds_pix, ds_cache):
        with torch.no_grad():
            dist = vae.encode(_qwen_vae_input(pix_batch["pixel_values"])).latent_dist
        expected = ((dist.sample(generator=torch.Generator().manual_seed(seed)) - m) / s).squeeze(2)
        got = _qwen_normalized_latents(
            vae, cache_batch, "cpu", torch.float32, generator=torch.Generator().manual_seed(seed)
        )
        torch.testing.assert_close(got, expected, rtol=0, atol=0)


def test_qwen_cache_keeps_5d_alpha_input_contract(tmp_path: Path) -> None:
    """VAE 4-canais/alpha inalterado: RGB vira RGBA opaco 5D antes do encode (PITFALLS:72,74,80)."""
    seen: list[torch.Tensor] = []

    class _Spy(_CountingVae):
        def encode(self, x):
            seen.append(x)
            return super().encode(x)

    vae = _Spy(_tiny_qwen_vae())
    _datasets(tmp_path, vae, lambda px: _qwen_encode_dist(vae, px, torch.float32))
    assert seen and all(x.ndim == 5 and x.shape[1] == 4 and x.shape[2] == 1 for x in seen)
    assert all(torch.all(x[:, 3] == 1.0) for x in seen)  # alpha opaco


def test_qwen_batched_online_encode_matches_within_one_ulp_scale(tmp_path: Path) -> None:
    """Batch>1 no caminho online: diferença só do encode em batch vs por imagem (~1e-6 em fp32)."""
    vae = _tiny_qwen_vae()
    ds_pix, ds_cache = _datasets(
        tmp_path, vae, lambda px: _qwen_encode_dist(vae, px, torch.float32)
    )
    for pix_batch, cache_batch in _bucket_batches(ds_pix, ds_cache, n=2):
        torch.manual_seed(5)
        online = _qwen_normalized_latents(vae, pix_batch, "cpu", torch.float32)
        torch.manual_seed(5)
        cached = _qwen_normalized_latents(vae, cache_batch, "cpu", torch.float32)
        torch.testing.assert_close(cached, online, rtol=0, atol=1e-5)


def test_flux2_batched_online_encode_matches_within_one_ulp_scale(tmp_path: Path) -> None:
    vae = _tiny_flux2_vae()
    ds_pix, ds_cache = _datasets(tmp_path, vae, lambda px: vae_encode_dist(vae, px.float()))
    comp = _flux_comp(vae, is_flux2=True, transformer=_CaptureTransformer())
    adapter = FluxAdapter()
    for pix_batch, cache_batch in _bucket_batches(ds_pix, ds_cache, n=2):
        _, call_o = _flux_run(adapter, comp, pix_batch, 5, 2)
        _, call_c = _flux_run(adapter, comp, cache_batch, 5, 2)
        torch.testing.assert_close(call_c["hidden_states"], call_o["hidden_states"], rtol=0, atol=1e-5)


# --- (b) Flux.1 / Flux.2 via FluxAdapter.forward_and_loss -----------------------


def _flux_comp(vae, *, is_flux2: bool, transformer) -> dict:
    return {
        "trainable_module": transformer,
        "device": torch.device("cpu"),
        "dtype": torch.float32,
        "extra": {
            "vae": vae,
            "noise_scheduler": SimpleNamespace(config=SimpleNamespace(shift=3.0)),
            "text_encoder_one": None, "text_encoder_two": None,
            "tokenizer_one": None, "tokenizer_two": None,
            "is_flux2": is_flux2,
            "shift_factor": 0.1159, "scaling_factor": 0.3611,
            "latents_mean": None, "latents_std": None,
        },
    }


def _flux_run(adapter, comp, batch, seed, bsz):
    transformer = comp["trainable_module"]
    transformer.calls.clear()
    enc = {"hidden": torch.randn(bsz, 5, 8, generator=torch.Generator().manual_seed(99)),
           "pooled": torch.randn(bsz, 8, generator=torch.Generator().manual_seed(98))}
    torch.manual_seed(seed)
    loss = adapter.forward_and_loss(comp, batch, SimpleNamespace(), enc)
    return loss, transformer.calls[-1]


@pytest.mark.parametrize("seed", SEEDS)
def test_flux1_cached_equals_online_same_seed(tmp_path: Path, seed: int) -> None:
    vae = _CountingVae(_tiny_kl(latent_channels=4))
    ds_pix, ds_cache = _datasets(tmp_path, vae, lambda px: vae_encode_dist(vae, px.float()))
    comp = _flux_comp(vae, is_flux2=False, transformer=_CaptureTransformer())
    adapter = FluxAdapter()
    for pix_batch, cache_batch in _bucket_batches(ds_pix, ds_cache):
        bsz = len(pix_batch["prompt"])
        loss_o, call_o = _flux_run(adapter, comp, pix_batch, seed, bsz)
        calls_before = vae.encode_calls
        loss_c, call_c = _flux_run(adapter, comp, cache_batch, seed, bsz)
        assert vae.encode_calls == calls_before  # cache não usa o VAE
        torch.testing.assert_close(loss_c, loss_o, rtol=0, atol=0)
        torch.testing.assert_close(call_c["hidden_states"], call_o["hidden_states"], rtol=0, atol=0)
        # img_ids (derivados de altura/largura de pixel) idênticos nos dois caminhos
        torch.testing.assert_close(call_c["img_ids"], call_o["img_ids"], rtol=0, atol=0)


@pytest.mark.parametrize("seed", SEEDS)
def test_flux2_patchify_and_bn_cached_equals_online_same_seed(tmp_path: Path, seed: int) -> None:
    vae = _CountingVae(_tiny_flux2_vae())
    ds_pix, ds_cache = _datasets(tmp_path, vae, lambda px: vae_encode_dist(vae, px.float()))
    assert vae.bn.running_mean.abs().sum() > 0  # BN de verdade
    comp = _flux_comp(vae, is_flux2=True, transformer=_CaptureTransformer())
    adapter = FluxAdapter()
    for pix_batch, cache_batch in _bucket_batches(ds_pix, ds_cache):
        bsz = len(pix_batch["prompt"])
        loss_o, call_o = _flux_run(adapter, comp, pix_batch, seed, bsz)
        calls_before = vae.encode_calls
        loss_c, call_c = _flux_run(adapter, comp, cache_batch, seed, bsz)
        assert vae.encode_calls == calls_before
        # patchify aplicado: C*4 canais e H/2 x W/2 → tokens = (H/2)*(W/2), canais = 16
        assert call_c["hidden_states"].shape == call_o["hidden_states"].shape
        assert call_c["hidden_states"].shape[-1] == 16
        torch.testing.assert_close(loss_c, loss_o, rtol=0, atol=0)
        torch.testing.assert_close(call_c["hidden_states"], call_o["hidden_states"], rtol=0, atol=0)
        torch.testing.assert_close(call_c["img_ids"], call_o["img_ids"], rtol=0, atol=0)


def test_flux2_bn_stats_follow_latents_device_when_vae_is_offloaded(tmp_path: Path) -> None:
    """Com o VAE na CPU e latents noutro device, ``vae.bn`` é lido sem mover o VAE:
    o adapter faz ``.to(latents.device, latents.dtype)`` nas estatísticas. Aqui o
    "outro device" é ``meta`` (sem GPU): os stats viram meta sem erro e o VAE não é tocado."""
    vae = _CountingVae(_tiny_flux2_vae())
    ds_pix, ds_cache = _datasets(tmp_path, vae, lambda px: vae_encode_dist(vae, px.float()))
    _, cache_batch = next(iter(_bucket_batches(ds_pix, ds_cache)))
    comp = _flux_comp(vae, is_flux2=True, transformer=_CaptureTransformer())
    cache_batch = {
        k: (v.to("meta") if torch.is_tensor(v) and k.startswith("latent") else v)
        for k, v in cache_batch.items()
    }
    comp["device"] = torch.device("meta")
    comp["trainable_module"].to("meta")
    enc = {"hidden": torch.randn(2, 5, 8, device="meta")}
    adapter = FluxAdapter()
    to_calls_before = list(vae.to_calls)
    adapter.forward_and_loss(comp, cache_batch, SimpleNamespace(), enc)
    assert vae.to_calls == to_calls_before
    assert next(vae._vae.parameters()).device.type == "cpu"  # VAE ficou na CPU


# --- (c) SDXL: time_ids a partir de height/width ---------------------------------


def _sdxl_comp(vae, unet):
    return {
        "trainable_module": unet, "device": torch.device("cpu"), "dtype": torch.float32,
        "extra": {"vae": vae, "noise_scheduler": DDPMScheduler(num_train_timesteps=100)},
    }


class _CaptureUnet(torch.nn.Module):
    def __init__(self) -> None:
        super().__init__()
        self.calls: list[dict] = []

    def forward(self, sample, timestep, encoder_hidden_states, added_cond_kwargs=None, return_dict=False):
        self.calls.append({"sample": sample.clone(), **(added_cond_kwargs or {})})
        return (torch.zeros_like(sample),)


@pytest.mark.parametrize("enable_bucket", [True, False])
def test_sdxl_time_ids_and_loss_cached_equal_pixel_path(
    tmp_path: Path, enable_bucket: bool, monkeypatch
) -> None:
    # O latent online sai do VAE com strides não contíguos (chunk de moments) e
    # ``randn_like`` herda esse layout, permutando o RUÍDO elemento a elemento em
    # relação ao cache (iid, estatisticamente igual). O latent em si é idêntico
    # (provado abaixo); para comparar também amostra/loss fixamos o ruído
    # independente de layout nos dois caminhos.
    monkeypatch.setattr(
        torch, "randn_like",
        lambda x, **k: torch.randn(x.shape, dtype=x.dtype, device=x.device),
    )
    vae = _CountingVae(_tiny_kl())
    root = _make_dataset_dir(tmp_path / "ds")
    ds_pix = DiffusionDataset(root, resolution=64, enable_bucket=enable_bucket)
    ds_cache = DiffusionDataset(root, resolution=64, enable_bucket=enable_bucket)
    assert prepare_latent_cache(
        _cache_for(tmp_path), [ds_cache], lambda px: vae_encode_dist(vae, px.float()),
        device="cpu", dtype=torch.float32,
    )
    unet = _CaptureUnet()
    comp = _sdxl_comp(vae, unet)
    adapter = SDXLAdapter()
    tcfg = SimpleNamespace(enable_bucket=enable_bucket, resolution=64)
    # sem bucket todas as imagens são 64x64 (resize), com bucket há (64,64) e (64,128)
    groups = sorted(ds_pix.buckets.values()) if enable_bucket else [[0], [4]]
    seen_hw = set()
    for idxs in groups:
        idxs = idxs[:1]
        pix_batch = default_collate([ds_pix[i] for i in idxs])
        cache_batch = default_collate([ds_cache[i] for i in idxs])
        bsz = len(idxs)
        enc = {"hidden": torch.randn(bsz, 5, 8), "pooled": torch.randn(bsz, 8)}

        # latent pré-ruído bit a bit igual (mesma semente)
        torch.manual_seed(3)
        lat_o = latents_from_batch(
            pix_batch, "cpu", lambda px: vae.encode(px.to(torch.float32)).latent_dist.sample()
        )
        torch.manual_seed(3)
        lat_c = latents_from_batch(cache_batch, "cpu", lambda px: pytest.fail("sem VAE"))
        torch.testing.assert_close(lat_c, lat_o, rtol=0, atol=0)

        unet.calls.clear()
        torch.manual_seed(3)
        loss_o = adapter.forward_and_loss(comp, pix_batch, tcfg, enc)
        o = unet.calls[-1]
        torch.manual_seed(3)
        loss_c = adapter.forward_and_loss(comp, cache_batch, tcfg, enc)
        c = unet.calls[-1]

        h, w = int(pix_batch["pixel_values"].shape[2]), int(pix_batch["pixel_values"].shape[3])
        seen_hw.add((h, w))
        expected_hw = (h, w) if enable_bucket else (64, 64)
        expected = torch.tensor(
            [[expected_hw[0], expected_hw[1], 0, 0, expected_hw[0], expected_hw[1]]] * bsz,
            dtype=torch.float32,
        )
        torch.testing.assert_close(o["time_ids"], expected)  # caminho de pixels (referência)
        torch.testing.assert_close(c["time_ids"], expected, rtol=0, atol=0)  # do cache
        torch.testing.assert_close(c["sample"], o["sample"], rtol=0, atol=0)
        torch.testing.assert_close(loss_c, loss_o, rtol=0, atol=0)
    if enable_bucket:
        assert seen_hw == {(64, 64), (128, 64)}  # (altura, largura) dos dois buckets


# --- (d) DataLoader com workers ---------------------------------------------------


def test_dataloader_with_two_workers_serves_cache_entries(tmp_path: Path, monkeypatch) -> None:
    monkeypatch.setenv("DIFFUSION_DATALOADER_WORKERS", "2")
    vae = _CountingVae(_tiny_kl())
    ds_pix, ds_cache = _datasets(tmp_path, vae, lambda px: vae_encode_dist(vae, px.float()))
    loader = build_dataloader(ds_cache, batch_size=2, seed=0)
    assert loader.num_workers == 2

    expected = {}
    for i in range(len(ds_cache)):
        with torch.no_grad():
            dist = vae.encode(ds_pix[i]["pixel_values"].unsqueeze(0)).latent_dist
        expected[i] = (dist.mean[0], dist.std[0])
    calls_before = vae.encode_calls

    seen = []
    for _epoch in range(2):  # workers persistentes, 2 épocas
        for batch in loader:
            assert "pixel_values" not in batch
            for k, idx in enumerate(batch["index"].tolist()):
                seen.append(idx)
                torch.testing.assert_close(batch["latent_mean"][k], expected[idx][0], rtol=0, atol=1e-6)
                torch.testing.assert_close(batch["latent_std"][k], expected[idx][1], rtol=0, atol=1e-6)
                h, w = batch["height"][k], batch["width"][k]
                assert (int(w), int(h)) == ds_cache.bucket_dims[idx]
    assert sorted(seen) == sorted(list(range(len(ds_cache))) * 2)
    assert vae.encode_calls == calls_before  # workers só leem o .pt, nenhum encode
