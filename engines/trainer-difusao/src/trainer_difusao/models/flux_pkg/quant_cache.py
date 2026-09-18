"""
Validação e integridade de cache de quantização persistida para FLUX.
"""
from __future__ import annotations

import json
from pathlib import Path
from typing import Any


def _custom_checkpoint_identity(custom_cp: str | None) -> str | None:
    """Identidade do checkpoint custom p/ isolamento do cache (path + md5 parcial)."""
    if not custom_cp:
        return None
    try:
        import hashlib

        h = hashlib.md5()
        with open(custom_cp, "rb") as f:
            for chunk in iter(lambda: f.read(1024 * 1024), b""):
                h.update(chunk)
                if h.digest_size and f.tell() >= 8 * 1024 * 1024:
                    break
        return f"{custom_cp}#{h.hexdigest()}"
    except OSError as exc:
        print(
            f"[WARN] Não foi possível fingerprintar checkpoint custom ({custom_cp}): "
            f"{exc}. Cache quantizado será invalidado.",
            flush=True,
        )
        return f"{custom_cp}#unreadable"


def _is_cache_valid(
    cache_dir: Path | None,
    expected_model_id: str,
    expected_quant: str,
    expected_custom: str | None = None,
) -> bool:
    """Verifica se o cache pertence exatamente ao model_id, quantização e custom esperados."""
    if not cache_dir or not cache_dir.exists():
        return False
    if not (cache_dir / "config.json").exists():
        return False
    meta_path = cache_dir.parent / "metadata.json"
    if not meta_path.exists():
        return False
    try:
        data = json.loads(meta_path.read_text(encoding="utf-8"))
        if data.get("model_id") != expected_model_id:
            return False
        if data.get("quant_format") != expected_quant:
            return False
        return data.get("custom_checkpoint") == expected_custom
    except Exception:
        return False


def _save_quant_metadata(
    quant_base: Path,
    model_id: str,
    quant_label: str,
    quant_format: str,
    target_dtype: Any,
    is_flux2: bool,
    custom_checkpoint: str | None = None,
) -> None:
    """Grava metadados da quantização persistida para garantir integridade e isolamento estrito."""
    try:
        quant_base.mkdir(parents=True, exist_ok=True)
        meta = {
            "model_id": model_id,
            "quantization": quant_label,
            "quant_format": quant_format,
            "target_dtype": str(target_dtype),
            "is_flux2": is_flux2,
            "custom_checkpoint": custom_checkpoint,
        }
        (quant_base / "metadata.json").write_text(json.dumps(meta, indent=2), encoding="utf-8")
    except Exception as e:
        print(f"[WARN] Não foi possível salvar metadata do cache quantizado: {e}", flush=True)
