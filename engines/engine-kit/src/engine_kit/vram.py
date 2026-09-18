"""
Gestão e telemetria de VRAM e GPU (com imports opcionais/lazy de PyTorch).
"""
import sys
from typing import Optional


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


def cleanup_cuda() -> None:
    """Executa coleta de lixo Python e esvazia cache CUDA se disponível."""
    import gc
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
