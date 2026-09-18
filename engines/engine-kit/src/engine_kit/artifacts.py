"""
Gerenciamento, limpeza e geração de artefatos de treinamento e modelos.
"""
import json
import re
import struct
from pathlib import Path
from typing import Optional, Set


def prune_checkpoints(
    checkpoints_dir: Path | str,
    keep_last_n: int = 2,
    prefix: str = "",
    suffix: str = ".safetensors",
    keep_files: Optional[Set[str]] = None,
) -> list[Path]:
    """Remove checkpoints antigos mantendo os últimos `keep_last_n` por época/modificação.

    Preserva explicitamente qualquer arquivo nomeado em `keep_files` (ex: 'best.safetensors').
    Retorna a lista de arquivos removidos.
    """
    cp_dir = Path(checkpoints_dir)
    if not cp_dir.is_dir():
        return []

    preserve = keep_files or set()
    epoch_regex = re.compile(r"epoch_(\d+)")

    candidates: list[tuple[int, float, Path]] = []
    for f in cp_dir.iterdir():
        if not f.is_file():
            continue
        if prefix and not f.name.startswith(prefix):
            continue
        if suffix and not f.name.endswith(suffix):
            continue
        if f.name in preserve:
            continue

        m = epoch_regex.search(f.name)
        epoch = int(m.group(1)) if m else -1
        mtime = f.stat().st_mtime
        candidates.append((epoch, mtime, f))

    # Ordena por época ascendente, depois por mtime
    candidates.sort(key=lambda x: (x[0], x[1]))

    removed: list[Path] = []
    if len(candidates) > keep_last_n:
        to_remove = candidates[:-keep_last_n]
        for _, _, p in to_remove:
            try:
                p.unlink()
                removed.append(p)
            except OSError:
                pass

    return removed


def make_fake_safetensors(
    output_path: Path | str,
    metadata: Optional[dict] = None,
) -> Path:
    """Gera um arquivo safetensors válido (formato HuggingFace) puramente com stdlib.

    Estrutura: 8 bytes LE (tamanho do cabeçalho JSON) + JSON bytes + buffer de tensores (vazio).
    """
    out = Path(output_path)
    out.parent.mkdir(parents=True, exist_ok=True)

    header_dict = {
        "__metadata__": metadata or {"format": "pt", "generator": "hephaestus-mock"}
    }
    header_bytes = json.dumps(header_dict, separators=(",", ":")).encode("utf-8")
    # Padding para alinhar com múltiplos de 8 bytes se desejado
    header_len = len(header_bytes)

    with open(out, "wb") as f:
        f.write(struct.pack("<Q", header_len))
        f.write(header_bytes)

    return out


def make_fake_artifact(
    output_path: Path | str,
    magic_bytes: bytes = b"MOCK_ARTIFACT_V1",
) -> Path:
    """Gera um artefato binário com header de identificação para testes e modo mock."""
    out = Path(output_path)
    out.parent.mkdir(parents=True, exist_ok=True)
    with open(out, "wb") as f:
        f.write(magic_bytes)
        f.write(b"\x00" * 32)
    return out
