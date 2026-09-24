"""
Gestão e telemetria de VRAM e GPU (com imports opcionais/lazy de PyTorch).
"""
import gc
import sys
from contextlib import contextmanager
from typing import Any, Generator, Optional

def vram_allocated_gb() -> Optional[float]:
    """Retorna VRAM alocada pelo PyTorch em GB (divisor canônico 1024³)."""
    try:
        import torch
        if torch.cuda.is_available():
            return round(torch.cuda.memory_allocated() / (1024 ** 3), 2)
    except Exception:
        pass
    return None


def vram_reserved_gb() -> Optional[float]:
    """Retorna VRAM reservada pelo allocator PyTorch em GB (divisor canônico 1024³)."""
    try:
        import torch
        if torch.cuda.is_available():
            return round(torch.cuda.memory_reserved() / (1024 ** 3), 2)
    except Exception:
        pass
    return None

def get_vram_usage() -> dict[str, Optional[float]]:
    """Retorna dicionário com VRAM alocada e reservada em GB."""
    return {
        "allocated_gb": vram_allocated_gb(),
        "reserved_gb": vram_reserved_gb(),
    }


def log_vram(tag: str = "vram") -> None:
    """Registra no stdout a VRAM alocada e reservada se CUDA estiver ativo."""
    alloc = vram_allocated_gb()
    res = vram_reserved_gb()
    if alloc is not None or res is not None:
        alloc_str = f"{alloc:.2f}GB" if alloc is not None else "N/A"
        res_str = f"{res:.2f}GB" if res is not None else "N/A"
        print(f"[{tag.upper()}] VRAM allocated: {alloc_str} | reserved: {res_str}", flush=True)


def cleanup_cuda() -> None:
    """Executa coleta de lixo Python e esvazia cache CUDA se disponível."""
    gc.collect()
    try:
        import torch
        if torch.cuda.is_available():
            torch.cuda.empty_cache()
            torch.cuda.ipc_collect()
    except Exception:
        pass

def require_cuda(operation_name: str = "GPU operation") -> None:
    """Valida se CUDA está disponível e falha honestamente caso contrário."""
    try:
        import torch
        if not torch.cuda.is_available():
            print(f"[FATAL] {operation_name} exige GPU CUDA, mas CUDA não está disponível.", file=sys.stderr, flush=True)
            sys.exit(1)
    except ImportError:
        print(f"[FATAL] {operation_name} exige PyTorch com suporte CUDA instalado.", file=sys.stderr, flush=True)
        sys.exit(1)


@contextmanager
def vram_guard(tag: str = "vram") -> Generator[None, None, None]:
    """Context manager para monitoramento e limpeza automática de VRAM.

    Ao sair do bloco with, executa gc.collect(), e se torch.cuda.is_available(),
    executa torch.cuda.empty_cache() e torch.cuda.ipc_collect().
    Trata gracefully caso torch não esteja instalado no ambiente (fallback seguro sem erro).
    """
    try:
        yield
    finally:
        cleanup_cuda()


def release_memory(*objects: Any) -> None:
    """Executa gc.collect(), malloc_trim e esvazia cache CUDA se disponível.

    Nota: em Python, para liberar memória de objetos grandes mantidos em variáveis
    locais do chamador (ex.: encoders de ~14GB), o chamador deve deletar explicitamente
    sua referência local ('del model' ou 'model = None') antes ou após a chamada.
    """
    cleanup_cuda()
