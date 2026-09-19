"""
Primitivas de runtime, tratamento de falhas e I/O atômico em disco.
"""
import os
import sys
import uuid
from pathlib import Path
from typing import Union


def die(message: str, code: int = 1) -> None:
    """Emite mensagem de erro no stderr e encerra o processo com código de saída."""
    print(f"[FATAL] {message}", file=sys.stderr, flush=True)
    sys.exit(code)


def atomic_write(target_path: Union[Path, str], content: Union[bytes, str], mode: str = "w") -> Path:
    """Escreve dados de forma atômica utilizando arquivo temporário seguido de rename.

    Garante que processos externos não leiam arquivos parcialmente gravados.
    """
    target = Path(target_path).resolve()
    target.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = target.parent / f".tmp_{target.name}_{uuid.uuid4().hex[:8]}"

    try:
        if "b" in mode or isinstance(content, bytes):
            with open(tmp_path, "wb") as f:
                f.write(content if isinstance(content, bytes) else content.encode("utf-8"))
                f.flush()
                os.fsync(f.fileno())
        else:
            with open(tmp_path, "w", encoding="utf-8") as f:
                f.write(content if isinstance(content, str) else content.decode("utf-8"))
                f.flush()
                os.fsync(f.fileno())
        os.replace(tmp_path, target)
    finally:
        if tmp_path.exists():
            try:
                tmp_path.unlink()
            except OSError:
                pass

    return target


def is_cancelled(sentinel_path: Union[Path, str]) -> bool:
    """Verifica se o sentinela de cancelamento do orquestrador foi acionado."""
    p = Path(sentinel_path)
    return p.exists()
