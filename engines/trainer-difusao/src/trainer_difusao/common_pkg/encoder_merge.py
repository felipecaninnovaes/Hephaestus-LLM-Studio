"""
Cache de merge e persistência de text_encoder custom (.safetensors).
"""
from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
from pathlib import Path
from typing import Any

from trainer_difusao.common_pkg.core import _die

TEXT_ENCODER_CUSTOM_CACHE_ENV = "TEXT_ENCODER_CUSTOM_CACHE"
_TEXT_ENCODER_CUSTOM_CACHE_DEFAULT = "~/.cache/hephaestus/text_encoder_custom"


def _custom_text_encoder_merge_dir(encoder_path: str) -> tuple[Path, str]:
    """Resolve (merged_dir, md5_16) do cache de merge p/ um encoder solto."""
    root = Path(
        os.environ.get(
            TEXT_ENCODER_CUSTOM_CACHE_ENV, _TEXT_ENCODER_CUSTOM_CACHE_DEFAULT
        )
    ).expanduser()
    try:
        h = hashlib.md5()
        with open(encoder_path, "rb") as f:
            for chunk in iter(lambda: f.read(1024 * 1024), b""):
                h.update(chunk)
    except OSError as exc:
        _die(
            f"Falha ao fingerprintar text_encoder custom ({encoder_path}): {exc}"
        )
    md5 = h.hexdigest()[:16]
    slug = re.sub(r"[^A-Za-z0-9_.\-]+", "_", Path(encoder_path).name).strip("._-")
    slug = (slug or "encoder")[:64]
    return root / f"{md5}-{slug}" / "merged", md5


def _text_encoder_cache_slug(text_encoder_path: str) -> str:
    """Slug de 12 hex p/ isolamento do cache quantizado (flux.py)."""
    if Path(text_encoder_path).is_file():
        try:
            return _custom_text_encoder_merge_dir(text_encoder_path)[1][:12]
        except SystemExit:
            raise
        except Exception:
            pass
    return hashlib.md5(text_encoder_path.encode("utf-8")).hexdigest()[:12]


def _merged_text_encoder_valid(merged_dir: Path, expected_md5: str) -> bool:
    """Cache de merge válido = metadata.json legível com md5 igual ao esperado."""
    try:
        meta = json.loads((merged_dir / "metadata.json").read_text(encoding="utf-8"))
    except Exception:
        return False
    return isinstance(meta, dict) and meta.get("md5") == expected_md5


def _write_merged_text_encoder_metadata(
    merged_dir: Path, *, md5: str, basename: str, model_id: str
) -> None:
    """Grava metadata.json do merge (md5/basename/model_id)."""
    (merged_dir / "metadata.json").write_text(
        json.dumps({"md5": md5, "basename": basename, "model_id": model_id}, indent=2),
        encoding="utf-8",
    )


def _load_loose_text_encoder_state(encoder_path: str) -> dict[str, Any]:
    """Lê o state_dict de um .safetensors solto (erro honesto se inválido)."""
    try:
        from safetensors.torch import load_file as _st_load

        return _st_load(str(encoder_path))
    except Exception as exc:
        _die(
            f"Falha ao ler text_encoder custom ({encoder_path}): "
            f"arquivo .safetensors inválido ({exc})"
        )


TEXT_ENCODER_CACHE_MAX_GB_ENV = "TEXT_ENCODER_CACHE_MAX_GB"
_TEXT_ENCODER_CACHE_MAX_GB_DEFAULT = 48.0


def _custom_text_encoder_cache_root() -> Path:
    """Root do cache de merge ($TEXT_ENCODER_CUSTOM_CACHE ou default)."""
    return Path(
        os.environ.get(
            TEXT_ENCODER_CUSTOM_CACHE_ENV, _TEXT_ENCODER_CUSTOM_CACHE_DEFAULT
        )
    ).expanduser()


def _merged_text_encoder_tmp_dir(merged_dir: Path) -> Path:
    """Dir temporário irmão do merge p/ publicação atômica (.tmp-<pid>)."""
    return merged_dir.parent / f".tmp-{os.getpid()}"


def _cleanup_merge_tmp_dir(tmp_dir: Path) -> None:
    """Remove o .tmp-<pid> órfão (best-effort, nunca derruba o job)."""
    try:
        if tmp_dir.is_symlink() or tmp_dir.is_file():
            tmp_dir.unlink()
        elif tmp_dir.is_dir():
            shutil.rmtree(tmp_dir, ignore_errors=True)
    except Exception:
        pass


def _publish_merged_text_encoder(tmp_dir: Path, merged_dir: Path) -> None:
    """Publica o merge via os.replace atômico (remove destino prévio)."""
    if merged_dir.is_symlink() or merged_dir.is_file():
        merged_dir.unlink()
    elif merged_dir.is_dir():
        shutil.rmtree(merged_dir, ignore_errors=True)
    os.replace(tmp_dir, merged_dir)


def _dir_size_bytes(path: Path) -> int:
    """Soma os tamanhos dos arquivos sob path (best-effort, ignora erros)."""
    total = 0
    try:
        for root, _dirs, files in os.walk(path):
            for name in files:
                try:
                    total += (Path(root) / name).stat().st_size
                except OSError:
                    continue
    except OSError:
        pass
    return total


def _text_encoder_cache_max_bytes() -> int:
    """Teto do cache de merge em bytes ($TEXT_ENCODER_CACHE_MAX_GB, default 48)."""
    try:
        return int(float(os.environ.get(
            TEXT_ENCODER_CACHE_MAX_GB_ENV,
            str(_TEXT_ENCODER_CACHE_MAX_GB_DEFAULT),
        )) * 1024**3)
    except (TypeError, ValueError):
        return int(_TEXT_ENCODER_CACHE_MAX_GB_DEFAULT * 1024**3)


def _sweep_text_encoder_merge_cache(keep_dir: Path) -> None:
    """Expurga merges mais antigos (mtime) até o root caber no teto."""
    try:
        root = _custom_text_encoder_cache_root()
        keep_entry = keep_dir if keep_dir.name != "merged" else keep_dir.parent
        if not root.is_dir():
            return
        max_bytes = _text_encoder_cache_max_bytes()
        total = _dir_size_bytes(root)
        if total <= max_bytes:
            return
        try:
            entries = [
                p for p in root.iterdir() if not p.name.startswith(".tmp-")
            ]
        except OSError:
            return

        def _entry_mtime(p: Path) -> float:
            target = p / "merged" if p.is_dir() else p
            try:
                return (target if target.exists() else p).stat().st_mtime
            except OSError:
                return 0.0

        entries.sort(key=_entry_mtime)
        for entry in entries:
            if total <= max_bytes:
                break
            try:
                same = entry.resolve() == keep_entry.resolve()
            except OSError:
                same = entry == keep_entry
            if same:
                continue
            size = _dir_size_bytes(entry)
            try:
                if entry.is_symlink() or entry.is_file():
                    entry.unlink()
                elif entry.is_dir():
                    shutil.rmtree(entry)
                else:
                    continue
            except OSError:
                continue
            total -= size
        print(
            f"[WARN] Cache de merge do text_encoder custom expurgado até o teto "
            f"({max_bytes / 1024**3:.1f} GiB): {root}",
            flush=True,
        )
    except Exception as exc:
        print(
            f"[WARN] Falha ao expurgar cache de merge do text_encoder custom: {exc}",
            flush=True,
        )
