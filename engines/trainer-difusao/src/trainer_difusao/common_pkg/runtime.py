"""
Gerenciamento de runtime CUDA, cache Hugging Face e retenção de checkpoints.
"""
from __future__ import annotations

import os
from pathlib import Path

from engine_kit.artifacts import prune_checkpoints as _prune_checkpoints_impl
from engine_kit.vram import cleanup_cuda as _cleanup_cuda_impl


def _prune_checkpoints(checkpoints_dir: Path, keep_last_n: int = 2) -> None:
    """Mantém apenas os últimos keep_last_n checkpoints de época para não esgotar o disco."""
    try:
        _prune_checkpoints_impl(
            checkpoints_dir,
            keep_last_n=keep_last_n,
            suffix=".safetensors",
            keep_files={"best.safetensors"},
        )
    except Exception as e:
        print(f"[CHECKPOINT] Aviso: falha ao podar checkpoints antigos: {e}", flush=True)


def _cleanup_cuda() -> None:
    """Invoca coleta de lixo e limpeza de cache VRAM CUDA de forma segura."""
    _cleanup_cuda_impl()


def _setup_cache_dir(hf_token: str | None = None) -> str:
    """Configura diretório de cache persistente para Hugging Face e PyTorch no volume /data/outputs ou /outputs."""
    if Path("/data/outputs").exists():
        cache_base = Path("/data/outputs/.cache/huggingface")
    elif Path("/outputs").exists():
        cache_base = Path("/outputs/.cache/huggingface")
    else:
        cache_base = Path.home() / ".cache" / "huggingface"

    hub_cache = cache_base / "hub"
    hub_cache.mkdir(parents=True, exist_ok=True)
    (cache_base.parent / "torch").mkdir(parents=True, exist_ok=True)

    cache_base_str = str(cache_base)
    hub_cache_str = str(hub_cache)
    torch_cache_str = str(cache_base.parent / "torch")

    os.environ["HF_HOME"] = cache_base_str
    os.environ["HF_HUB_CACHE"] = hub_cache_str
    os.environ["HUGGINGFACE_HUB_CACHE"] = hub_cache_str
    os.environ["TRANSFORMERS_CACHE"] = hub_cache_str
    os.environ["DIFFUSERS_CACHE"] = hub_cache_str
    os.environ["TORCH_HOME"] = torch_cache_str
    os.environ["HF_HUB_DISABLE_XET"] = "1"
    os.environ["HF_HUB_ENABLE_HF_TRANSFER"] = "0"

    token = (
        hf_token
        or os.environ.get("HF_TOKEN")
        or os.environ.get("HUGGING_FACE_HUB_TOKEN")
        or ""
    ).strip()
    if token:
        os.environ["HF_TOKEN"] = token
        os.environ["HUGGING_FACE_HUB_TOKEN"] = token
        try:
            import huggingface_hub

            huggingface_hub.login(token=token, add_to_git_credential=False)
            print("[INFO] Autenticado com sucesso no Hugging Face Hub via HF_TOKEN.", flush=True)
        except Exception as e:
            print(f"[WARN] Falha ao registrar token no huggingface_hub: {e}", flush=True)

    return hub_cache_str


def _ensure_qwen_diffusers_compat() -> None:
    """Registra aliases em diffusers para compatibilidade entre model_index.json e diffusers."""
    try:
        import diffusers
    except ImportError:
        return

    pairs = [
        ("QwenImage21Transformer2DModel", "QwenImageTransformer2DModel"),
        ("QwenImage21Pipeline", "QwenImagePipeline"),
        ("AutoencoderKLQwenImage21", "AutoencoderKLQwenImage"),
    ]
    for a, b in pairs:
        if not hasattr(diffusers, a) and hasattr(diffusers, b):
            setattr(diffusers, a, getattr(diffusers, b))
        elif not hasattr(diffusers, b) and hasattr(diffusers, a):
            setattr(diffusers, b, getattr(diffusers, a))
