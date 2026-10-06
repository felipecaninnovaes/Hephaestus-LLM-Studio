"""
Gerenciamento de adaptadores LoRA nomeados em cache LRU para inferência no daemon quente.
Evita unload/reload repetidos de pesos LoRA entre requisições (ADR-0023).
"""
from __future__ import annotations

import hashlib
import os
from collections import OrderedDict
from typing import Any, List

from trainer_difusao.klein import KLEIN_ARCHS

# Prefixos de chave que o diffusers já sabe rotear por componente / converter
# (diffusers `transformer.`/`unet.`/`text_encoder*.`, ai-toolkit/ComfyUI
# `diffusion_model.`, PEFT cru `base_model.model.`, kohya `lora_*`).
_KNOWN_LORA_KEY_PREFIXES = (
    "transformer.",
    "unet.",
    "text_encoder.",
    "text_encoder_2.",
    "diffusion_model.",
    "base_model.model.",
    "lora_unet_",
    "lora_te",
    "lora_transformer_",
)
_LORA_TENSOR_MARKERS = (".lora_A.", ".lora_B.", ".lora_down.", ".lora_up.")


class LoraLoadError(RuntimeError):
    """LoRA que não pôde ser carregada/aplicada — a requisição DEVE falhar (PITFALLS:34)."""


def lora_component_prefix(base_model: str) -> str:
    """Prefixo de componente que o loader do diffusers exige p/ a arquitetura."""
    return "unet." if base_model in ("sdxl", "sd15") else "transformer."


def prepare_lora_for_load(path: str, base_model: str) -> Any:
    """Devolve o argumento p/ `pipe.load_lora_weights`: o `path` ou um state_dict remapeado.

    O adapter salvo pela engine (`_save_lora_safetensors`, contrato ComfyUI) NÃO tem
    prefixo de componente (`transformer_blocks.0...lora_A.weight`); o diffusers filtra
    por `transformer.`/`unet.` e carregaria 0 chaves (só um warning). Arquivo nesse
    layout → state_dict com o prefixo da arquitetura. Já prefixado/kohya/ComfyUI →
    passthrough (o diffusers converte). Arquivo sem nenhum tensor LoRA → LoraLoadError.
    """
    if not str(path).endswith(".safetensors"):
        return path
    try:
        from safetensors.torch import load_file

        state = load_file(path)
    except Exception as exc:
        raise LoraLoadError(f"LoRA {path} ilegível como safetensors: {exc}") from exc
    keys = list(state)
    if not any(m in k for k in keys for m in _LORA_TENSOR_MARKERS):
        raise LoraLoadError(
            f"LoRA {path} não contém nenhum tensor LoRA (lora_A/lora_B/lora_down/"
            f"lora_up) — {len(keys)} chave(s) no arquivo. Requisição abortada."
        )
    if any(k.startswith(_KNOWN_LORA_KEY_PREFIXES) for k in keys):
        return path
    prefix = lora_component_prefix(base_model)
    print(
        f"[DIFFUSION-GEN] LoRA {path}: layout da engine sem prefixo de componente — "
        f"remapeando {len(keys)} chaves com '{prefix}'.",
        flush=True,
    )
    return {prefix + k: v for k, v in state.items()}


def count_injected_lora_modules(pipe: Any, adapter_name: str) -> int | None:
    """Nº de módulos LoRA do adapter nos componentes do pipeline (None = não verificável)."""
    components = getattr(pipe, "components", None)
    if not isinstance(components, dict):
        return None
    try:
        import torch
    except ImportError:
        return None
    total = 0
    for comp in components.values():
        if not isinstance(comp, torch.nn.Module):
            continue
        if adapter_name not in (getattr(comp, "peft_config", None) or {}):
            continue
        for sub in comp.modules():
            lora_a = getattr(sub, "lora_A", None)
            if lora_a is not None and adapter_name in lora_a:
                total += 1
    return total


def _is_model_cpu_offloaded(pipe: Any) -> bool:
    """True se o pipeline usa `enable_model_cpu_offload` (hooks accelerate CpuOffload)."""
    components = getattr(pipe, "components", None)
    if not isinstance(components, dict):
        return False
    try:
        from accelerate.hooks import CpuOffload
    except ImportError:
        return False
    return any(isinstance(getattr(c, "_hf_hook", None), CpuOffload) for c in components.values())


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
                load_arg = prepare_lora_for_load(path, base_model)
                offloaded = _is_model_cpu_offloaded(pipe)
                try:
                    pipe.load_lora_weights(load_arg, adapter_name=name)
                except Exception as exc:
                    # diffusers remove os hooks de offload antes de injetar e só os
                    # recoloca no caminho de sucesso: sem isto o pipeline quente fica
                    # sem offload (device errado) após uma LoRA inválida.
                    if offloaded:
                        pipe.enable_model_cpu_offload()
                    raise LoraLoadError(
                        f"Falha ao carregar LoRA {path}: {exc}"
                    ) from exc
                injected = count_injected_lora_modules(pipe, name)
                if injected == 0:
                    self._delete_adapter(pipe, base_model, name)
                    raise LoraLoadError(
                        f"LoRA {path} não injetou nenhuma camada no modelo "
                        f"({base_model}): 0 chaves aplicadas (formato/arquitetura "
                        "incompatível). Requisição abortada."
                    )
                if injected is not None:
                    print(
                        f"[DIFFUSION-GEN] LoRA {name} ({path}): {injected} módulo(s) "
                        "LoRA injetado(s).",
                        flush=True,
                    )

                self.adapters[name] = {"key": fp, "path": path, "scale": scale}
                self.key_to_name[fp] = name

            needed_names.append(name)
            needed_scales.append(scale)

        self._set_active_adapters(pipe, base_model, needed_names, needed_scales)
    def _delete_adapter(self, pipe: Any, base_model: str, name: str) -> None:
        """Deleta adaptador do pipeline ou do transformer se suportado."""
        target = (
            pipe.transformer
            if (base_model in KLEIN_ARCHS and hasattr(pipe, "transformer"))
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
            if (base_model in KLEIN_ARCHS and hasattr(pipe, "transformer"))
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
            if (base_model in KLEIN_ARCHS and hasattr(pipe, "transformer"))
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
