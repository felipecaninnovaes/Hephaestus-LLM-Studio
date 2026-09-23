"""Cache e pré-computação em disco de embeddings de texto do dataset."""
from __future__ import annotations

import contextlib
import os
from pathlib import Path
from typing import Any
from trainer_difusao.common_pkg.metrics import _emit_metric
from trainer_difusao.common_pkg.train_config import _caption_cache_key

# Flag de performance para descarregar text encoders após pré-compute (adr-difusao-vram).
ENABLE_TEXT_ENCODER_UNLOAD = os.environ.get("ENABLE_TEXT_ENCODER_UNLOAD", "false").lower() in ("true", "1", "yes")


class TextEmbedsCache:
    """Cache em disco dos prompt embeddings: ``{output}/text_embeds_cache/{sha256(caption)[:16]}.pt``."""\

    def __init__(self, output: Path, enabled: bool):
        self.enabled = bool(enabled)
        self.dir = Path(output) / "text_embeds_cache"
        self._broken = False

    def get(self, caption: str) -> dict[str, Any] | None:
        """Retorna o payload em CPU ou None (miss)."""
        if not self.enabled or self._broken:
            return None
        path = self.dir / f"{_caption_cache_key(caption)}.pt"
        if not path.exists():
            return None
        try:
            import torch

            data = torch.load(str(path), map_location="cpu", weights_only=True)
            return data if isinstance(data, dict) else None
        except Exception:
            return None

    def put(self, caption: str, payload: dict[str, Any]) -> None:
        """Grava atomicamente o payload (tensores movidos para CPU)."""
        if not self.enabled or self._broken:
            return
        try:
            import torch

            self.dir.mkdir(parents=True, exist_ok=True)
            cpu_payload = {
                k: (v.cpu() if hasattr(v, "cpu") else v) for k, v in payload.items()
            }
            tmp = self.dir / f".tmp_{_caption_cache_key(caption)}.pt"
            torch.save(cpu_payload, tmp)
            os.replace(tmp, self.dir / f"{_caption_cache_key(caption)}.pt")
        except Exception as e:
            self._broken = True
            print(
                f"[WARN] Cache de text embeddings desabilitado (falha de escrita): {e}",
                flush=True,
            )


def _precompute_text_cache(
    cache: TextEmbedsCache,
    captions: list[str],
    encode_fn: Any,
    batch_size: int = 32,
    metrics_path: Path | None = None,
) -> None:
    """Pré-computa UMA vez os prompt embeddings de todas as captions do dataset."""
    if not cache.enabled:
        return
    uniq = list(dict.fromkeys(c for c in captions if isinstance(c, str)))
    if not uniq:
        return
    total = len(uniq)
    if metrics_path is not None:
        _emit_metric(
            metrics_path,
            epoch=0,
            step=0,
            progress=0.07,
            phase="preparing_cache",
            message=f"Pré-computando cache de text embeddings: 0/{total} captions...",
            telemetry_only=True,
        )
    try:
        i = 0
        bs = batch_size
        while i < len(uniq):
            chunk = uniq[i : i + bs]
            try:
                out = encode_fn(chunk)
            except Exception as e:
                is_oom = "out of memory" in str(e).lower()
                if is_oom and bs > 1:
                    bs = max(1, bs // 2)
                    try:
                        import torch

                        if torch.cuda.is_available():
                            torch.cuda.empty_cache()
                    except ImportError:
                        pass
                    print(
                        f"[INFO] OOM no pré-compute do cache de text embeddings "
                        f"— reduzindo batch para {bs}.",
                        flush=True,
                    )
                    continue
                raise
            for k, cap in enumerate(chunk):
                cache.put(cap, {name: t[k].detach().cpu() for name, t in out.items()})
            i += len(chunk)
            if metrics_path is not None:
                done = min(i, total)
                _emit_metric(
                    metrics_path,
                    epoch=0,
                    step=0,
                    progress=round(0.07 + 0.01 * done / total, 4),
                    phase="preparing_cache",
                    message=(
                        f"Pré-computando cache de text embeddings: {done}/{total} captions..."
                    ),
                    telemetry_only=True,
                )
    except Exception as e:
        cache.enabled = False
        print(
            f"[WARN] Falha ao pré-computar cache de text embeddings, seguindo sem cache: {e}",
            flush=True,
        )
        return
    print(
        f"[INFO] Cache de text embeddings pré-computado: {len(uniq)} captions únicas.",
        flush=True,
    )
    if metrics_path is not None:
        _emit_metric(
            metrics_path,
            epoch=0,
            step=0,
            progress=0.08,
            phase="preparing_cache",
            message=f"Cache de text embeddings pré-computado: {total} captions únicas.",
            telemetry_only=True,
        )


def _offload_encoders_to_cpu(encoders: list[Any]) -> None:
    """Move encoders de texto para CPU e limpa cache CUDA para liberar VRAM."""
    try:
        import torch

        for enc in encoders:
            if enc is not None and hasattr(enc, "to"):
                enc.to("cpu")
        import gc

        gc.collect()
        if torch.cuda.is_available():
            torch.cuda.empty_cache()
        print("[INFO] Text encoders descarregados para CPU (VRAM liberada).", flush=True)
    except Exception as e:
        print(f"[WARN] Falha ao descarregar text encoders para CPU: {e}", flush=True)


# Alias retrocompatível
_cleanup_encoders = _offload_encoders_to_cpu


@contextlib.contextmanager
def _temporary_device_encoders(encoders: list[Any], device: Any):
    """Garante que encoders estejam em `device` durante o bloco e retorna para CPU ao sair."""
    if not ENABLE_TEXT_ENCODER_UNLOAD or not encoders or device is None:
        yield
        return

    try:
        for enc in encoders:
            if enc is not None and hasattr(enc, "to"):
                enc.to(device)
        yield
    finally:
        try:
            import torch

            for enc in encoders:
                if enc is not None and hasattr(enc, "to"):
                    enc.to("cpu")
            import gc

            gc.collect()
            if torch.cuda.is_available():
                torch.cuda.empty_cache()
        except Exception as e:
            print(f"[WARN] Falha ao retornar text encoders para CPU: {e}", flush=True)


def _precompute_text_cache_with_cleanup(
    cache: TextEmbedsCache,
    captions: list[str],
    encode_fn: Any,
    batch_size: int = 32,
    metrics_path: Path | None = None,
    unload_encoders: bool = False,
    encoders: list[Any] | None = None,
    cleanup_kwargs: dict[str, Any] = {},
) -> None:
    """Pré-computa embeddings + opcionalmente move encoders para CPU (liberando VRAM)."""
    _precompute_text_cache(cache, captions, encode_fn, batch_size, metrics_path)
    if unload_encoders and encoders:
        _offload_encoders_to_cpu(encoders)


def _cached_encode(
    captions: list[str],
    encode_fn: Any,
    cache: TextEmbedsCache,
    encoders: list[Any] | None = None,
    device: Any = None,
) -> dict[str, Any]:
    """Resolve os embeddings do batch via cache (hit) ou encoder (miss com warm)."""
    import torch

    if not cache.enabled:
        return encode_fn(captions)
    hits = [cache.get(c) for c in captions]
    miss_idx = [i for i, h in enumerate(hits) if h is None]
    if miss_idx:
        with _temporary_device_encoders(encoders or [], device):
            out = encode_fn([captions[i] for i in miss_idx])
        for k, i in enumerate(miss_idx):
            payload = {name: t[k].detach().cpu() for name, t in out.items()}
            cache.put(captions[i], payload)
            hits[i] = payload
    names = list(hits[0].keys())
    return {name: torch.stack([h[name] for h in hits]) for name in names}