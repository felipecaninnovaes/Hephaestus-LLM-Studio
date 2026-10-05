"""Cache em disco da distribuição do VAE (mean/std) por imagem + bucket.

O treino de LoRA codificava cada imagem com o VAE a cada época e mantinha o VAE
na GPU o treino inteiro. Aqui a distribuição ``latent_dist`` (mean e std, NÃO
uma amostra) é pré-computada uma vez; no laço de treino amostra-se
``mean + std * randn`` com o RNG do treino, preservando o comportamento
estocástico de ``latent_dist.sample()``. Depois do pré-compute o VAE sai da GPU.

Layout: ``{output}/latents_cache/<namespace>/<key>.pt`` — sempre dentro do
diretório do job (o orchestrator só abre permissões dos diretórios listados em
``SHARED_ENGINE_CACHE_DIRS``; PITFALLS:57).

* ``<namespace>`` = hash de arch + identidade do VAE (modelo, fingerprint dos
  pesos, checkpoint custom) + dtype de armazenamento + versão do pré-processamento.
* ``<key>`` = hash de (conteúdo do arquivo de imagem, largura, altura do bucket).

O dataset atual não aplica crop/flip aleatório por época (resize bilinear
determinístico para o bucket); se isso mudar, o cache DEVE ser desligado via
``augmentation_active`` (ver ``resolve_cache_latents``).
"""
from __future__ import annotations

import contextlib
import hashlib
import json
import os
from pathlib import Path
from typing import Any, Callable, Iterator

from trainer_difusao.common_pkg.metrics import _emit_metric

LATENT_CACHE_SCHEMA_VERSION = 1
LATENT_CACHE_DIRNAME = "latents_cache"
# Pré-processamento aplicado por DiffusionDataset.load_pixel_values: RGB, resize
# bilinear para o bucket, normalização [-1, 1]. Mudar isso invalida o cache.
LATENT_PREPROCESS_ID = "rgb-bilinear-pm1-v1"

_LATENT_PROGRESS_START = 0.08
_LATENT_PROGRESS_SPAN = 0.01


def resolve_cache_latents(cfg: dict[str, Any], *, augmentation_active: bool = False) -> bool:
    """Lê ``cache_latents`` (default true) do cfg, no topo ou na seção ``lora``.

    ``augmentation_active``: com augmentação aleatória por época o cache de
    distribuição ficaria inválido (cada época veria uma imagem diferente), então
    é desativado com log claro.
    """
    raw = cfg.get("cache_latents", None)
    if raw is None:
        raw = (cfg.get("lora") or {}).get("cache_latents", True)
    if isinstance(raw, str):
        enabled = raw.strip().lower() in ("1", "true", "yes")
    else:
        enabled = bool(raw)
    if enabled and augmentation_active:
        print(
            "[INFO] cache_latents desativado: augmentação aleatória por época ativa "
            "(o cache de latents exige pré-processamento determinístico).",
            flush=True,
        )
        return False
    return enabled


def _sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def vae_fingerprint(vae: Any) -> str:
    """Fingerprint barato da identidade do VAE: config + nome/shape de cada
    tensor + amostra espaçada dos valores. Muda com repo/revisão/checkpoint
    custom sem precisar hashear todos os pesos."""
    h = hashlib.sha256()
    cfg = getattr(vae, "config", None)
    if cfg is not None:
        h.update(json.dumps(dict(cfg), sort_keys=True, default=str).encode("utf-8"))
    for name, tensor in vae.state_dict().items():
        h.update(name.encode("utf-8"))
        h.update(str(tuple(tensor.shape)).encode("utf-8"))
        flat = tensor.detach().flatten()
        if flat.numel() == 0:
            continue
        stride = max(1, flat.numel() // 64)
        h.update(flat[::stride][:64].float().cpu().numpy().tobytes())
    return h.hexdigest()[:32]


def vae_encode_dist(vae: Any, pixel_values: Any) -> tuple[Any, Any]:
    """Codifica e devolve ``(mean, std)`` de ``latent_dist`` (a distribuição, não uma amostra)."""
    dist = vae.encode(pixel_values).latent_dist
    return dist.mean, dist.std


def sample_latent_dist(mean: Any, std: Any, generator: Any | None = None) -> Any:
    """``mean + std * randn`` — mesma fórmula de ``DiagonalGaussianDistribution.sample``.

    Com ``generator=None`` usa o RNG global (como ``latent_dist.sample()`` no
    treino). Calcula em fp32; com VAE fp32 e mesma semente o resultado é idêntico
    a ``latent_dist.sample()``.
    """
    import torch

    noise = torch.randn(
        mean.shape, generator=generator, device=mean.device, dtype=torch.float32
    )
    return mean.float() + std.float() * noise


def batch_pixel_hw(batch: dict[str, Any]) -> tuple[int, int]:
    """(altura, largura) em pixels do batch, com ou sem ``pixel_values`` (modo cache)."""
    if "pixel_values" in batch:
        px = batch["pixel_values"]
        return int(px.shape[2]), int(px.shape[3])
    return int(batch["height"][0]), int(batch["width"][0])


def batch_size_of(batch: dict[str, Any]) -> int:
    if "pixel_values" in batch:
        return int(batch["pixel_values"].shape[0])
    return int(batch["latent_mean"].shape[0])


def latents_from_batch(
    batch: dict[str, Any],
    device: Any,
    encode_sample: Callable[[Any], Any],
    generator: Any | None = None,
) -> Any:
    """Latents amostrados do batch: via cache (``latent_mean``/``latent_std``) ou,
    sem cache, ``encode_sample(pixel_values)`` (caminho original com o VAE)."""
    if "latent_mean" in batch:
        return sample_latent_dist(
            batch["latent_mean"].to(device), batch["latent_std"].to(device), generator
        )
    return encode_sample(batch["pixel_values"].to(device))


class LatentCache:
    """Cache em disco de ``(mean, std)`` do VAE, escrita atômica, corrompido = miss."""

    def __init__(
        self,
        output: Path,
        enabled: bool,
        *,
        namespace_fields: dict[str, Any],
    ) -> None:
        self.enabled = bool(enabled)
        fields = dict(namespace_fields)
        fields["_schema_version"] = LATENT_CACHE_SCHEMA_VERSION
        fields["_preprocess"] = LATENT_PREPROCESS_ID
        self.namespace_fields = fields
        self.namespace = hashlib.sha256(
            json.dumps(fields, sort_keys=True, default=str).encode("utf-8")
        ).hexdigest()[:16]
        self.dir = Path(output) / LATENT_CACHE_DIRNAME / self.namespace
        self._digests: dict[str, str] = {}

    def key_for(self, image_path: Path | str, bucket_wh: tuple[int, int]) -> str:
        """Chave = hash do conteúdo da imagem + resolução final do bucket."""
        p = str(image_path)
        digest = self._digests.get(p)
        if digest is None:
            digest = _sha256_file(Path(p))
            self._digests[p] = digest
        w, h = bucket_wh
        return hashlib.sha256(f"{digest}:{int(w)}x{int(h)}".encode("utf-8")).hexdigest()[:32]

    def path_for(self, key: str) -> Path:
        return self.dir / f"{key}.pt"

    def get(self, key: str) -> dict[str, Any] | None:
        """Payload ``{"mean", "std"}`` em CPU ou None (miss/corrompido, com log)."""
        path = self.path_for(key)
        if not path.exists():
            return None
        try:
            import torch

            data = torch.load(str(path), map_location="cpu", weights_only=True)
            if (
                not isinstance(data, dict)
                or "mean" not in data
                or "std" not in data
                or tuple(data["mean"].shape) != tuple(data["std"].shape)
            ):
                raise ValueError("payload inválido")
            return data
        except Exception as exc:
            print(
                f"[WARN] Cache de latents corrompido ({path.name}): {exc} — recomputando.",
                flush=True,
            )
            return None

    def put(self, key: str, mean: Any, std: Any) -> bool:
        """Grava atomicamente (tmp + rename). Retorna False em falha de escrita."""
        try:
            import torch

            self.dir.mkdir(parents=True, exist_ok=True)
            tmp = self.dir / f".tmp_{key}.pt"
            torch.save({"mean": mean.detach().cpu(), "std": std.detach().cpu()}, tmp)
            os.replace(tmp, self.path_for(key))
            return True
        except Exception as exc:
            print(f"[WARN] Falha ao gravar cache de latents ({key}): {exc}", flush=True)
            with contextlib.suppress(OSError):
                (self.dir / f".tmp_{key}.pt").unlink()
            return False


def prepare_latent_cache(
    cache: LatentCache,
    datasets: list[Any],
    encode_dist: Callable[[Any], tuple[Any, Any]],
    *,
    device: Any,
    dtype: Any,
    metrics_path: Path | None = None,
) -> bool:
    """Pré-computa (ou reaproveita) a distribuição de todas as imagens dos datasets.

    ``encode_dist(pixel_values[1,C,H,W] em device) -> (mean, std)``. Só liga o
    cache nos datasets (``use_latent_cache``) se TODAS as entradas existirem; em
    qualquer falha retorna False e os datasets seguem com pixels (VAE online).
    Telemetria: ``phase=caching_latents`` com ``step``/``total_steps``.
    """
    import torch

    total = sum(len(ds) for ds in datasets)
    done = reused = 0
    plans: list[tuple[Any, list[str]]] = []
    every = max(1, total // 20)

    def _emit(msg: str) -> None:
        if metrics_path is None:
            return
        _emit_metric(
            metrics_path,
            epoch=0,
            step=done,
            total_steps=total,
            progress=round(_LATENT_PROGRESS_START + _LATENT_PROGRESS_SPAN * done / max(1, total), 4),
            phase="caching_latents",
            message=msg,
            telemetry_only=True,
        )

    _emit(f"Pré-computando cache de latents (VAE): 0/{total} imagens...")
    try:
        for ds in datasets:
            keys: list[str] = []
            for idx, (path, _) in enumerate(ds.samples):
                key = cache.key_for(path, ds.bucket_dims[idx])
                keys.append(key)
                if cache.get(key) is not None:
                    reused += 1
                else:
                    with torch.no_grad():
                        px = ds.load_pixel_values(idx).unsqueeze(0).to(device)
                        mean, std = encode_dist(px)
                    if not cache.put(key, mean[0].to(dtype), std[0].to(dtype)):
                        raise RuntimeError("falha de escrita do cache de latents")
                done += 1
                if done % every == 0 or done == total:
                    _emit(f"Pré-computando cache de latents (VAE): {done}/{total} imagens...")
            plans.append((ds, keys))
    except Exception as exc:
        print(
            f"[WARN] Falha ao pré-computar cache de latents, seguindo com VAE online: {exc}",
            flush=True,
        )
        return False
    for ds, keys in plans:
        ds.use_latent_cache(cache, keys)
    print(
        f"[INFO] Cache de latents pronto: {total} imagens "
        f"({total - reused} codificadas, {reused} reaproveitadas) em {cache.dir}",
        flush=True,
    )
    return True


@contextlib.contextmanager
def vae_on_device(vae: Any | None, device: Any, *, offloaded: bool) -> Iterator[None]:
    """Traz o VAE (offloaded em CPU) para ``device`` só dentro do bloco e o devolve
    à CPU liberando VRAM com o utilitário existente. No-op se não offloaded."""
    if not offloaded or vae is None:
        yield
        return
    from trainer_difusao.common_pkg.runtime import _cleanup_cuda

    vae.to(device)
    try:
        yield
    finally:
        vae.to("cpu")
        _cleanup_cuda()
