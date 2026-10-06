"""Cache e pré-computação em disco de embeddings de texto do dataset."""
from __future__ import annotations

import contextlib
import json
import os
from pathlib import Path
from typing import Any
from trainer_difusao.common_pkg.metrics import _emit_metric
from trainer_difusao.common_pkg.train_config import (
    EMBEDS_NAMESPACE_SCHEMA_VERSION,
    _caption_cache_key,
    _embeds_namespace_key,
)

# Flag de performance para descarregar text encoders após pré-compute (adr-difusao-vram).
ENABLE_TEXT_ENCODER_UNLOAD = os.environ.get("ENABLE_TEXT_ENCODER_UNLOAD", "false").lower() in ("true", "1", "yes")

# Contrato orchestrator↔engine: quando setada, embeds vivem em
# $TEXT_EMBEDS_CACHE_DIR/<namespace>/<sha256(caption)[:16]>.pt (compartilhado
# entre jobs/containers no mesmo nó). Sem a env, fallback para
# {output}/text_embeds_cache/<key>.pt (comportamento legado, por job).
TEXT_EMBEDS_CACHE_DIR_ENV = "TEXT_EMBEDS_CACHE_DIR"


class TextEmbedsCache:
    """Cache em disco dos prompt embeddings.

    Sem ``TEXT_EMBEDS_CACHE_DIR``: ``{output}/text_embeds_cache/{sha256(caption)[:16]}.pt``
    (fallback legado, por job).
    Com ``TEXT_EMBEDS_CACHE_DIR``: ``$TEXT_EMBEDS_CACHE_DIR/<namespace>/{sha256(caption)[:16]}.pt``,
    onde ``<namespace> = sha256(json(sorted(namespace_fields)))[:16]`` — compartilhado
    entre jobs com o mesmo modelo/encoder/quantização/seq-len/dtype.
    """

    def __init__(
        self,
        output: Path,
        enabled: bool,
        *,
        namespace_fields: dict[str, Any] | None = None,
    ):
        self.enabled = bool(enabled)
        self._broken = False
        env_dir = os.environ.get(TEXT_EMBEDS_CACHE_DIR_ENV)
        if env_dir:
            fields = dict(namespace_fields or {})
            fields["_schema_version"] = EMBEDS_NAMESPACE_SCHEMA_VERSION
            self._namespace_fields = fields
            namespace = _embeds_namespace_key(fields)
            self.dir = Path(env_dir) / namespace
            self._namespaced = True
        else:
            self.dir = Path(output) / "text_embeds_cache"
            self._namespace_fields = None
            self._namespaced = False
        self._namespace_ready = False

    def _ensure_namespace_dir(self) -> None:
        """Cria o diretório do namespace (0o777, compartilhado entre uid 1000 de
        containers distintos — PITFALLS:51) e grava `namespace.json` uma vez."""
        self.dir.mkdir(parents=True, exist_ok=True)
        if self._namespaced:
            try:
                os.chmod(self.dir, 0o777)
            except OSError:
                pass
            if self._namespace_ready:
                return
            meta_path = self.dir / "namespace.json"
            if not meta_path.exists():
                tmp = self.dir / ".tmp_namespace.json"
                tmp.write_text(json.dumps(self._namespace_fields, sort_keys=True, indent=2), encoding="utf-8")
                os.replace(tmp, meta_path)
            self._namespace_ready = True
        else:
            self._namespace_ready = True

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
            if not isinstance(data, dict):
                return None
            try:
                os.utime(path, None)
            except OSError:
                pass
            return data
        except Exception:
            # Arquivo corrompido (ex.: escrita concorrente nunca commitada via
            # rename, truncamento) é tratado como miss; put() reescreve.
            return None

    def put(self, caption: str, payload: dict[str, Any]) -> None:
        """Grava atomicamente o payload (tensores movidos para CPU)."""
        if not self.enabled or self._broken:
            return
        try:
            import torch

            self._ensure_namespace_dir()
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
def _temporary_device_encoders(encoders: list[Any], device: Any, *, force: bool = False):
    """Garante que encoders estejam em `device` durante o bloco e retorna para CPU ao sair.

    `force` ignora ENABLE_TEXT_ENCODER_UNLOAD (archs cujo encoder é descarregado à força).
    """
    if not (ENABLE_TEXT_ENCODER_UNLOAD or force) or not encoders or device is None:
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
    force_device_swap: bool = False,
) -> dict[str, Any]:
    """Resolve os embeddings do batch via cache (hit) ou encoder (miss com warm)."""
    import torch

    if not cache.enabled:
        return encode_fn(captions)
    hits = [cache.get(c) for c in captions]
    miss_idx = [i for i, h in enumerate(hits) if h is None]
    if miss_idx:
        with _temporary_device_encoders(encoders or [], device, force=force_device_swap):
            out = encode_fn([captions[i] for i in miss_idx])
        for k, i in enumerate(miss_idx):
            payload = {name: t[k].detach().cpu() for name, t in out.items()}
            cache.put(captions[i], payload)
            hits[i] = payload
    names = list(hits[0].keys())
    return {name: torch.stack([h[name] for h in hits]) for name in names}


def _precompute_sample_embeds_sd15(
    tokenizer: Any,
    text_encoder: Any,
    prompt: str,
    device: Any,
    dtype: Any = None,
) -> dict[str, Any]:
    """Pré-computa prompt_embeds e negative_prompt_embeds para amostragem no SD 1.5."""
    import torch

    with torch.no_grad():
        text_inputs = tokenizer(
            prompt,
            padding="max_length",
            max_length=tokenizer.model_max_length,
            truncation=True,
            return_tensors="pt",
        )
        prompt_embeds = text_encoder(text_inputs.input_ids.to(device))[0]
        if dtype is not None:
            prompt_embeds = prompt_embeds.to(dtype=dtype)

        uncond_inputs = tokenizer(
            "",
            padding="max_length",
            max_length=tokenizer.model_max_length,
            truncation=True,
            return_tensors="pt",
        )
        neg_prompt_embeds = text_encoder(uncond_inputs.input_ids.to(device))[0]
        if dtype is not None:
            neg_prompt_embeds = neg_prompt_embeds.to(dtype=dtype)

    return {
        "prompt_embeds": prompt_embeds,
        "negative_prompt_embeds": neg_prompt_embeds,
    }


def _precompute_sample_embeds_sdxl(
    tokenizer_one: Any,
    tokenizer_two: Any,
    text_encoder_one: Any,
    text_encoder_two: Any,
    prompt: str,
    device: Any,
    dtype: Any = None,
) -> dict[str, Any]:
    """Pré-computa prompt_embeds e pooled embeddings para amostragem no SDXL."""
    import torch

    with torch.no_grad():
        def _encode_single(text: str) -> tuple[Any, Any]:
            tokens_one = tokenizer_one(
                [text],
                padding="max_length",
                max_length=tokenizer_one.model_max_length,
                truncation=True,
                return_tensors="pt",
            ).input_ids.to(device)
            enc_one = text_encoder_one(tokens_one, output_hidden_states=True)
            hidden_one = enc_one.hidden_states[-2]

            tokens_two = tokenizer_two(
                [text],
                padding="max_length",
                max_length=tokenizer_two.model_max_length,
                truncation=True,
                return_tensors="pt",
            ).input_ids.to(device)
            enc_two = text_encoder_two(tokens_two, output_hidden_states=True)
            hidden_two = enc_two.hidden_states[-2]
            pooled = enc_two.text_embeds

            concat = torch.concat([hidden_one, hidden_two], dim=-1)
            if dtype is not None:
                concat = concat.to(dtype=dtype)
                pooled = pooled.to(dtype=dtype)
            return concat, pooled

        prompt_embeds, pooled_prompt_embeds = _encode_single(prompt)
        neg_prompt_embeds, neg_pooled_prompt_embeds = _encode_single("")

    return {
        "prompt_embeds": prompt_embeds,
        "pooled_prompt_embeds": pooled_prompt_embeds,
        "negative_prompt_embeds": neg_prompt_embeds,
        "negative_pooled_prompt_embeds": neg_pooled_prompt_embeds,
    }


def _precompute_sample_embeds_flux(
    tokenizer_one: Any,
    tokenizer_two: Any,
    text_encoder_one: Any,
    text_encoder_two: Any,
    prompt: str,
    device: Any,
    is_flux2: bool = False,
    dtype: Any = None,
) -> dict[str, Any]:
    """Pré-computa prompt_embeds e pooled_prompt_embeds para amostragem no FLUX."""
    import torch

    with torch.no_grad():
        if is_flux2:
            from trainer_difusao.models.flux_pkg.encoding import _encode_qwen3_prompt

            prompt_embeds = _encode_qwen3_prompt(
                text_encoder_one,
                tokenizer_one,
                [prompt],
                device,
                dtype=dtype,
            )
            negative_prompt_embeds = _encode_qwen3_prompt(
                text_encoder_one,
                tokenizer_one,
                [""],
                device,
                dtype=dtype,
            )
            return {
                "prompt_embeds": prompt_embeds,
                "negative_prompt_embeds": negative_prompt_embeds,
                "pooled_prompt_embeds": None,
            }

        clip_inputs = tokenizer_one(
            [prompt],
            padding="max_length",
            max_length=tokenizer_one.model_max_length,
            truncation=True,
            return_tensors="pt",
        ).to(device)
        pooled = text_encoder_one(clip_inputs.input_ids).pooler_output
        t5_inputs = tokenizer_two(
            [prompt],
            padding="max_length",
            max_length=512,
            truncation=True,
            return_tensors="pt",
        ).to(device)
        hidden = text_encoder_two(t5_inputs.input_ids)[0]
        if dtype is not None:
            hidden = hidden.to(dtype=dtype)
            pooled = pooled.to(dtype=dtype)

    return {
        "prompt_embeds": hidden,
        "pooled_prompt_embeds": pooled,
    }