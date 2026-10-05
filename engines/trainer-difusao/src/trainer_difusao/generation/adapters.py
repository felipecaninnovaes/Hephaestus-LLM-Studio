"""
Gerenciamento de adaptadores LoRA nomeados em cache LRU para inferência no daemon quente.
Evita unload/reload repetidos de pesos LoRA entre requisições (ADR-0023).
"""
from __future__ import annotations

import os
from collections import OrderedDict
from typing import Any, List


class DaemonLoraCache:
    """Gerencia adaptadores LoRA nomeados carregados no pipeline via LRU.

    Chave = caminho + st_size + st_mtime_ns (ou md5 parcial).
    Capacidade configurada via env DIFFUSION_DAEMON_LORA_CACHE (default: 4).
    """

    def __init__(self, capacity: int | None = None) -> None:
        if capacity is None:
            env_val = os.environ.get("DIFFUSION_DAEMON_LORA_CACHE", "4")
            try:
                capacity = max(1, int(env_val))
            except ValueError:
                capacity = 4
        self.capacity: int = capacity
        # adapter_name -> {"key": fp, "path": path, "scale": scale}
        self.adapters: OrderedDict[str, dict[str, Any]] = OrderedDict()
        self.key_to_name: dict[str, str] = {}
        self._name_counter: int = 0

    @staticmethod
    def compute_file_fingerprint(path_str: str) -> str:
        """Fingerprint rápido do arquivo usando path, size e mtime (com hash do cabeçalho se legível)."""
        try:
            st = os.stat(path_str)
            size = st.st_size
            mtime = st.st_mtime_ns
        except OSError:
            return f"{path_str}#unknown"

        import hashlib

        h = hashlib.md5()
        try:
            with open(path_str, "rb") as f:
                h.update(f.read(1024 * 1024))
            md5_part = h.hexdigest()[:12]
        except OSError:
            md5_part = "unreadable"

        return f"{path_str}#{size}_{mtime}_{md5_part}"

    def clear(self, pipe: Any = None) -> None:
        """Invalida completamente o cache de adaptadores (ex: ao trocar de base pipeline)."""
        if pipe is not None and hasattr(pipe, "unload_lora_weights"):
            try:
                pipe.unload_lora_weights()
            except Exception as e:
                print(
                    f"[DIFFUSION-GEN] [AVISO] Falha ao descarregar LoRA no clear do cache: {e}",
                    flush=True,
                )
        self.adapters.clear()
        self.key_to_name.clear()
        self._name_counter = 0

    def apply_loras(
        self,
        pipe: Any,
        loras_effective: List[dict[str, Any]],
        base_model: str,
    ) -> None:
        """Aplica LoRAs solicitadas reutilizando adaptadores cacheados e evictando LRU se necessário.

        Se loras_effective for vazio, desativa todos os adaptadores sem vazamento (PITFALLS:77).
        """
        if not loras_effective:
            self._disable_adapters(pipe, base_model)
            return
        effective_capacity = max(self.capacity, len(loras_effective))
        needed_names: List[str] = []
        needed_scales: List[float] = []
        active_keys = {self.compute_file_fingerprint(l["path"]) for l in loras_effective}

        for lora in loras_effective:
            path = lora["path"]
            scale = float(lora.get("scale", 1.0))
            fp = self.compute_file_fingerprint(path)

            if fp in self.key_to_name:
                name = self.key_to_name[fp]
                self.adapters.move_to_end(name)
                self.adapters[name]["scale"] = scale
            else:
                while len(self.adapters) >= effective_capacity:
                    # Evict LRU that is NOT part of the currently active request
                    evict_candidate = None
                    for cand_name, cand_info in self.adapters.items():
                        if cand_info["key"] not in active_keys:
                            evict_candidate = cand_name
                            break
                    if evict_candidate is None:
                        break
                    evict_info = self.adapters.pop(evict_candidate)
                    self.key_to_name.pop(evict_info["key"], None)
                    self._delete_adapter(pipe, base_model, evict_candidate)
                    print(
                        f"[DIFFUSION-GEN] LRU eviction de LoRA: {evict_candidate} ({evict_info['path']})",
                        flush=True,
                    )

                self._name_counter += 1
                name = f"cached_lora_{self._name_counter}"
                print(
                    f"[DIFFUSION-GEN] Carregando LoRA {name}: {path} (scale={scale})",
                    flush=True,
                )
                pipe.load_lora_weights(path, adapter_name=name)

                self.adapters[name] = {"key": fp, "path": path, "scale": scale}
                self.key_to_name[fp] = name

            needed_names.append(name)
            needed_scales.append(scale)

        self._set_active_adapters(pipe, base_model, needed_names, needed_scales)
    def _delete_adapter(self, pipe: Any, base_model: str, name: str) -> None:
        """Deleta adaptador do pipeline ou do transformer se suportado."""
        target = (
            pipe.transformer
            if (base_model == "flux-2-klein-4b" and hasattr(pipe, "transformer"))
            else pipe
        )
        if hasattr(target, "delete_adapters"):
            try:
                target.delete_adapters(name)
                return
            except Exception as e:
                print(
                    f"[DIFFUSION-GEN] [AVISO] Falha ao deletar adaptador {name} em {type(target).__name__}: {e}",
                    flush=True,
                )
        elif hasattr(target, "delete_adapter"):
            try:
                target.delete_adapter(name)
                return
            except Exception as e:
                print(
                    f"[DIFFUSION-GEN] [AVISO] Falha ao deletar adaptador {name} em {type(target).__name__} (delete_adapter): {e}",
                    flush=True,
                )
        if hasattr(pipe, "delete_adapters") and pipe is not target:
            try:
                pipe.delete_adapters(name)
            except Exception as e:
                print(
                    f"[DIFFUSION-GEN] [AVISO] Falha ao deletar adaptador {name} no pipe: {e}",
                    flush=True,
                )
        elif hasattr(pipe, "delete_adapter") and pipe is not target:
            try:
                pipe.delete_adapter(name)
            except Exception as e:
                print(
                    f"[DIFFUSION-GEN] [AVISO] Falha ao deletar adaptador {name} no pipe (delete_adapter): {e}",
                    flush=True,
                )

    def _disable_adapters(self, pipe: Any, base_model: str) -> None:
        """Desativa adaptadores para requisição sem LoRA sem vazamento."""
        target = (
            pipe.transformer
            if (base_model == "flux-2-klein-4b" and hasattr(pipe, "transformer"))
            else pipe
        )
        if hasattr(target, "disable_lora"):
            try:
                target.disable_lora()
            except Exception:
                pass
        if hasattr(target, "set_adapters"):
            try:
                target.set_adapters([])
            except Exception:
                pass
        if hasattr(pipe, "disable_lora") and pipe is not target:
            try:
                pipe.disable_lora()
            except Exception:
                pass
        if hasattr(pipe, "set_adapters") and pipe is not target:
            try:
                pipe.set_adapters([])
            except Exception:
                pass
        print(
            "[DIFFUSION-GEN] Requisição sem LoRA: adaptadores desativados (disable_lora/set_adapters([])).",
            flush=True,
        )
    def _set_active_adapters(
        self, pipe: Any, base_model: str, names: List[str], scales: List[float]
    ) -> None:
        """Ativa lista de adaptadores com seus respectivos pesos/escalas."""
        target = (
            pipe.transformer
            if (base_model == "flux-2-klein-4b" and hasattr(pipe, "transformer"))
            else pipe
        )
        if hasattr(target, "enable_lora"):
            try:
                target.enable_lora()
            except Exception:
                pass
        if hasattr(pipe, "enable_lora") and pipe is not target:
            try:
                pipe.enable_lora()
            except Exception:
                pass

        if hasattr(target, "set_adapters"):
            try:
                target.set_adapters(names, scales)
                print(
                    f"[DIFFUSION-GEN] Multi-LoRA aplicado via {type(target).__name__}.set_adapters: {names}",
                    flush=True,
                )
                return
            except Exception as exc:
                print(
                    f"[DIFFUSION-GEN] [AVISO] {type(target).__name__}.set_adapters falhou ({exc}). "
                    f"Fallback: aplicando apenas o primeiro LoRA ({names[0]}). Ref: ADR-0023 spike S1.",
                    flush=True,
                )
                try:
                    target.set_adapters([names[0]], [scales[0]])
                    return
                except Exception as exc2:
                    print(
                        f"[DIFFUSION-GEN] [ERRO] Fallback set_adapters 1 LoRA também falhou ({exc2}). LoRA não aplicada.",
                        flush=True,
                    )
        if hasattr(pipe, "set_adapters") and pipe is not target:
            pipe.set_adapters(names, scales)
            print(
                f"[DIFFUSION-GEN] Multi-LoRA aplicado via pipe.set_adapters: {names}",
                flush=True,
            )
__all__ = ["DaemonLoraCache"]
