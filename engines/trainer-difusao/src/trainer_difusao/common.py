"""Utilitários compartilhados e helpers comuns para o trainer de difusão."""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import struct
import sys
from pathlib import Path
from typing import Any


def _die(msg: str) -> None:
    print(f"ERROR: {msg}", file=sys.stderr)
    sys.exit(1)


def _seed_bytes(seed: int, length: int = 1024) -> bytes:
    h = hashlib.sha256(struct.pack("<q", seed)).digest()
    out = bytearray()
    while len(out) < length:
        h = hashlib.sha256(h).digest()
        out.extend(h)
    return bytes(out[:length])


def _synthetic_loss(seed: int, epoch: int, total_epochs: int) -> float:
    raw = _seed_bytes(seed + epoch * 13, 16)
    val = struct.unpack("<d", raw[:8])[0]
    norm = abs(val) / (1e300 if abs(val) > 1e300 else 1.0)
    norm = (norm % 1.0) * 0.05
    decay = 0.5 * (1.0 - (epoch / (total_epochs + 1)))
    return round(max(0.01, decay + norm), 4)


def _canonical_model_name(raw_model: str) -> str:
    norm = raw_model.strip().lower()
    if norm in ("flux", "flux2", "flux-2", "flux2-klein-4b", "flux.2-klein-4b"):
        return "flux-2-klein-4b"
    if norm in ("sdxl", "sdxl-1.0"):
        return "sdxl"
    if norm in ("sd15", "sd-1.5", "stable-diffusion-v1-5"):
        return "sd15"
    return norm


def _resolve_output_name(cfg: dict[str, Any]) -> str:
    """Resolve o nome base para os arquivos de pesos (.safetensors).

    Prioriza cfg['output_name'] (ADR-0022), limpando extensão e caracteres inválidos.
    Fallback para 'adapter'.
    """
    raw_name = cfg.get("output_name")
    if raw_name and isinstance(raw_name, str) and raw_name.strip():
        name = raw_name.strip()
        if name.endswith(".safetensors"):
            name = name[:-12]
        clean = "".join(c if (c.isalnum() or c in ("-", "_", ".")) else "_" for c in name)
        clean = clean.strip("._-")
        if clean:
            return clean

    return "adapter"



def _emit_metric(
    metrics_path: Path,
    epoch: int,
    step: int,
    loss: float | None = None,
    lr: float | None = None,
    progress: float | None = None,
    phase: str | None = None,
    message: str | None = None,
) -> None:
    """Emite uma linha estruturada em metrics.jsonl com flush imediato para consumo pelo orquestrador."""
    try:
        metrics_path.parent.mkdir(parents=True, exist_ok=True)
        payload: dict[str, Any] = {
            "epoch": epoch,
            "step": step,
        }
        if loss is not None:
            payload["loss"] = loss
        if lr is not None:
            payload["lr"] = lr
        if progress is not None:
            payload["progress"] = progress
        if phase is not None:
            payload["phase"] = phase
        if message is not None:
            payload["message"] = message

        with open(metrics_path, "a", encoding="utf-8") as f:
            f.write(json.dumps(payload) + "\n")
            f.flush()

        # ADR-0021: Espelha em telemetry.jsonl no formato canônico
        try:
            import datetime

            telemetry_path = metrics_path.parent / "telemetry.jsonl"
            now_iso = datetime.datetime.now(datetime.timezone.utc).isoformat()
            t_phase = phase or ("training" if epoch > 0 else "preparing")
            t_msg = message or (
                f"Treinando Época {epoch}, Passo {step}"
                if epoch > 0
                else "Preparando pipeline de difusão..."
            )
            t_prog = progress if progress is not None else 0.0

            vram_val = None
            try:
                import torch

                if torch.cuda.is_available():
                    vram_val = round(torch.cuda.memory_allocated() / (1024**3), 2)
            except Exception:
                pass

            t_payload: dict[str, Any] = {
                "timestamp": now_iso,
                "phase": t_phase,
                "phaseMessage": t_msg,
                "progress": round(t_prog, 4),
                "step": step,
                "epoch": epoch,
            }
            if vram_val is not None:
                t_payload["vramUsedGb"] = vram_val
            m_dict: dict[str, Any] = {}
            if loss is not None:
                m_dict["loss"] = loss
            if lr is not None:
                m_dict["lr"] = lr
            if m_dict:
                t_payload["metrics"] = m_dict

            with open(telemetry_path, "a", encoding="utf-8") as tf:
                tf.write(json.dumps(t_payload) + "\n")
                tf.flush()
        except Exception:
            pass
    except Exception as e:
        print(
            f"[WARN] Falha ao emitir métrica para {metrics_path}: {e}",
            file=sys.stderr,
            flush=True,
        )


def _setup_cache_dir(hf_token: str | None = None) -> str:
    """Configura diretório de cache persistente para Hugging Face e PyTorch no volume /outputs."""
    if Path("/outputs").exists():
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


def _save_lora_safetensors(
    model: Any, output_file: Path, metadata: dict[str, str]
) -> None:
    """Salva os pesos do adaptador LoRA em formato .safetensors canônico de forma atômica."""
    import safetensors.torch
    from peft import get_peft_model_state_dict

    output_file.parent.mkdir(parents=True, exist_ok=True)
    tmp_file = output_file.parent / f".tmp_{output_file.name}"
    lora_state_dict = get_peft_model_state_dict(model)
    safetensors.torch.save_file(lora_state_dict, str(tmp_file), metadata=metadata)
    os.replace(tmp_file, output_file)


def _load_lora_weights(model: Any, weights_path: Path | str) -> None:
    """Carrega pesos prévios do adaptador LoRA a partir de um arquivo .safetensors."""
    import safetensors.torch
    from peft import set_peft_model_state_dict

    path = Path(weights_path)
    if not path.exists():
        print(f"[WARN] Arquivo de pesos para continuação não encontrado: {path}", flush=True)
        return

    print(f"[INFO] Carregando pesos LoRA prévios de: {path}", flush=True)
    state_dict = safetensors.torch.load_file(str(path))
    set_peft_model_state_dict(model, state_dict)
    print("[INFO] Pesos LoRA injetados com sucesso no modelo para continuação de treino.", flush=True)


# ---------------------------------------------------------------------------
# Infraestrutura auxiliar do treino (control dataset, cache de embeddings, quant)
# ---------------------------------------------------------------------------

# Níveis canônicos de quantização aceitos no treino (wire api-principal → engines).
_TRAIN_QUANT_LEVELS = ("none", "2bit", "4bit", "6bit", "8bit")
# Aliases legados que continuam aceitos na validação (compat com configs antigas).
_TRAIN_QUANT_ALIASES = {
    "4bit-nf4": "4bit",
    "nf4": "4bit",
    "8bit-bnb": "8bit",
    "int8": "8bit",
}

# Fração default/máxima de steps com prior-preservation (estilo DreamBooth, 10%).
_CONTROL_RATIO_DEFAULT = 0.1
_CONTROL_RATIO_MAX = 0.5


def _normalize_train_quantization(raw: Any, default: str | None = "none") -> str | None:
    """Normaliza o nível de quantização do treino para o enum canônico.

    Aceita none/2bit/4bit/6bit/8bit + aliases legados 4bit-nf4/nf4/8bit-bnb/int8.
    Valor ausente/vazio → ``default`` (None = ausente; o trainer aplica o default do arch).
    Valor desconhecido → _die (erro honesto, nunca degradação silenciosa).
    """
    if raw is None or (isinstance(raw, str) and not raw.strip()):
        if default is None:
            return None
        raw = default
    norm = str(raw).strip().lower()
    norm = _TRAIN_QUANT_ALIASES.get(norm, norm)
    if norm not in _TRAIN_QUANT_LEVELS:
        _die(
            f"Quantização de treino inválida: '{raw}'. "
            "Valores aceitos: none, 2bit, 4bit, 6bit, 8bit "
            "(aliases legados 4bit-nf4/8bit-bnb continuam aceitos)."
        )
    return norm


def _validate_train_aux(cfg: dict[str, Any], quant_default: str | None = None) -> dict[str, Any]:
    """Valida as chaves auxiliares do treino vindas do YAML gerado pelo api-principal.

    Cobre ``control_dataset_path`` (diretório staging do orchestrator com imagens
    de regularização), ``control_ratio`` (0..0.5, default 0.1),
    ``cache_text_embeddings`` (bool, default False) e a sintaxe de ``quantization``
    (``quant_default`` é o default do arch quando ausente: "none" p/ SD, "4bit" p/
    Flux; None = ausente permanece None e o chamador aplica o próprio fallback).
    Erro honesto via _die em valor inválido.
    """
    if not isinstance(cfg, dict):
        _die("Configuração de treino inválida: raiz deve ser um dicionário.")
    lora_cfg = cfg.get("lora", {}) or {}

    raw_control = cfg.get("control_dataset_path", None)
    if isinstance(raw_control, str) and not raw_control.strip():
        raw_control = None
    control_dataset_path: Path | None = None
    if raw_control is not None:
        control_dataset_path = Path(str(raw_control))
        if not control_dataset_path.exists() or not control_dataset_path.is_dir():
            _die(f"control_dataset_path inválido ou inexistente: {raw_control}")

    raw_ratio = cfg.get("control_ratio", None)
    if raw_ratio is None:
        raw_ratio = lora_cfg.get("control_ratio", _CONTROL_RATIO_DEFAULT)
    try:
        control_ratio = float(raw_ratio)
    except (TypeError, ValueError):
        _die(
            f"control_ratio inválido: {raw_ratio!r}. "
            f"Deve ser float entre 0 e {_CONTROL_RATIO_MAX}."
        )
    if not (0.0 <= control_ratio <= _CONTROL_RATIO_MAX):
        _die(
            f"control_ratio fora do intervalo permitido: {control_ratio}. "
            f"Deve estar entre 0 e {_CONTROL_RATIO_MAX}."
        )

    raw_cache = cfg.get("cache_text_embeddings", None)
    if raw_cache is None:
        raw_cache = lora_cfg.get("cache_text_embeddings", False)
    if isinstance(raw_cache, str):
        cache_text_embeddings = raw_cache.strip().lower() in ("1", "true", "yes")
    else:
        cache_text_embeddings = bool(raw_cache)

    raw_quant = lora_cfg.get("quantization", None)
    if raw_quant is None:
        raw_quant = cfg.get("quantization", None)
    quantization = _normalize_train_quantization(raw_quant, default=quant_default)

    return {
        "control_dataset_path": control_dataset_path,
        "control_ratio": control_ratio,
        "cache_text_embeddings": cache_text_embeddings,
        "quantization": quantization,
    }

def _count_control_images(control_path: Path) -> int:
    """Conta imagens de regularização (mesmas extensões do DiffusionDataset, sem abrir arquivos)."""
    target = control_path / "images"
    if not target.exists():
        target = control_path
    valid_exts = {".webp", ".png", ".jpg", ".jpeg"}
    try:
        return sum(
            1 for p in target.iterdir() if p.is_file() and p.suffix.lower() in valid_exts
        )
    except OSError:
        return 0


def _cycling_batches(loader: Any) -> Any:
    """Iterador infinito sobre um DataLoader (amostragem do control dataset com reposição)."""
    while True:
        for batch in loader:
            yield batch


def _caption_cache_key(caption: str) -> str:
    """Chave de invalidação natural do cache: sha256(caption)[:16]."""
    return hashlib.sha256(caption.encode("utf-8")).hexdigest()[:16]


class TextEmbedsCache:
    """Cache em disco dos prompt embeddings: ``{output}/text_embeds_cache/{sha256(caption)[:16]}.pt``.

    Miss → o trainer computa on-the-fly e grava (warm). Falha de escrita
    (disco cheio, permissão) → warning único, desabilita o cache e segue
    sem cache — nunca derruba o treino.
    """

    def __init__(self, output: Path, enabled: bool):
        self.enabled = bool(enabled)
        self.dir = Path(output) / "text_embeds_cache"
        self._broken = False

    def get(self, caption: str) -> dict[str, Any] | None:
        """Retorna o payload em CPU ou None (miss). Arquivo corrompido = miss (regenerado no put)."""
        if not self.enabled or self._broken:
            return None
        path = self.dir / f"{_caption_cache_key(caption)}.pt"
        if not path.exists():
            return None
        try:
            import torch

            data = torch.load(str(path), map_location="cpu", weights_only=True)
            return data if isinstance(data, dict) else None
        except Exception:
            return None

    def put(self, caption: str, payload: dict[str, Any]) -> None:
        """Grava atomicamente o payload (tensores movidos para CPU). Falha → warning único + segue sem cache."""
        if not self.enabled or self._broken:
            return
        try:
            import torch

            self.dir.mkdir(parents=True, exist_ok=True)
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


def _build_intx_torchao_config(quantization: str) -> Any:
    """Constrói o torchao IntxWeightOnlyConfig para quantização 2bit/6bit weight-only.

    Equivalente moderno aos knobs clássicos ("intx_weight_only", weight_type
    "int2"/"int6"): o chamador embrulha em ``diffusers.TorchAoConfig(quant_type=...)``
    (Transformer/UNet) ou ``transformers.TorchAoConfig(quant_type=...)``
    (text encoders). Só faz sentido em CUDA: sem torch/torchao ou sem GPU →
    erro honesto, nunca fallback silencioso para precisão plena.
    """
    norm = str(quantization).strip().lower()
    if norm not in ("2bit", "6bit"):
        _die(f"Quantização torchao intx inválida: '{quantization}'. Use '2bit' ou '6bit'.")
    try:
        import torch
    except ImportError:
        _die(f"Quantização '{norm}' exige torch instalado (extra 'train').")
    try:
        from torchao.quantization import IntxWeightOnlyConfig
    except ImportError as e:
        _die(f"Quantização '{norm}' exige torchao instalado (extra 'train'): {e}")
    if not torch.cuda.is_available():
        _die(
            f"Quantização '{norm}' (torchao int2/int6 weight-only) exige CUDA "
            "— sem fallback em CPU."
        )
    weight_dtype = torch.int2 if norm == "2bit" else torch.int6
    try:
        return IntxWeightOnlyConfig(weight_dtype=weight_dtype)
    except Exception as e:
        _die(f"Falha ao construir config torchao {norm}: {e}")

def _precompute_text_cache(
    cache: TextEmbedsCache,
    captions: list[str],
    encode_fn: Any,
    batch_size: int = 32,
) -> None:
    """Pré-computa UMA vez os prompt embeddings de todas as captions do dataset.

    ``encode_fn(caps)`` é o caminho honesto de cada arch (ex.: rodar os text
    encoders uma vez — SD: CLIP; SDXL: dual CLIP + pooled; Flux: T5+CLIP ou
    Qwen3 conforme o arch) e deve retornar ``{nome: Tensor[B, ...]}`` em
    qualquer device. Falha aqui → warning e segue sem cache (não derruba o treino).
    """
    if not cache.enabled:
        return
    uniq = list(dict.fromkeys(c for c in captions if isinstance(c, str)))
    if not uniq:
        return
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


def _cached_encode(captions: list[str], encode_fn: Any, cache: TextEmbedsCache) -> dict[str, Any]:
    """Resolve os embeddings do batch via cache (hit) ou encoder (miss com warm).

    Retorna ``{nome: Tensor[B, ...]}`` empilhado em CPU no caminho com cache
    (o chamador move para device/dtype — ``.to()`` é idempotente no caminho
    direto). Miss → computa on-the-fly só o subset ausente e grava no cache.
    """
    import torch

    if not cache.enabled:
        return encode_fn(captions)
    hits = [cache.get(c) for c in captions]
    miss_idx = [i for i, h in enumerate(hits) if h is None]
    if miss_idx:
        out = encode_fn([captions[i] for i in miss_idx])
        for k, i in enumerate(miss_idx):
            payload = {name: t[k].detach().cpu() for name, t in out.items()}
            cache.put(captions[i], payload)
            hits[i] = payload
    names = list(hits[0].keys())
    return {name: torch.stack([h[name] for h in hits]) for name in names}


# ---------------------------------------------------------------------------
# Cache de merge p/ text_encoder custom flux-2 em arquivo solto (.safetensors)
# ---------------------------------------------------------------------------
#
# Arquivo solto + quantização: os pesos custom (bf16) são mesclados UMA vez
# sobre o encoder do repo BFL e persistidos em disco; a quantização é então
# aplicada por carga sobre o merged (mesma semântica do encoder default).
# Chave: md5 do arquivo (16 hex, streaming) + slug do basename; o merge vive
# no subdiretório ``merged/``. Concorrência: merge em ``.tmp-<pid>`` irmão +
# os.replace (atômico) — o padrão part-{n}.tmp do chunk.rs NÃO se aplica aqui.

TEXT_ENCODER_CUSTOM_CACHE_ENV = "TEXT_ENCODER_CUSTOM_CACHE"
_TEXT_ENCODER_CUSTOM_CACHE_DEFAULT = "~/.cache/hephaestus/text_encoder_custom"


def _custom_text_encoder_merge_dir(encoder_path: str) -> tuple[Path, str]:
    """Resolve (merged_dir, md5_16) do cache de merge p/ um encoder solto.

    Root: $TEXT_ENCODER_CUSTOM_CACHE ou ~/.cache/hephaestus/text_encoder_custom.
    Falha de leitura honesta via _die (nunca fingerprint silencioso).
    """
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
    """Slug de 12 hex p/ isolamento do cache quantizado (flux.py).

    Arquivo solto → md5 de CONTEÚDO (mesmo fingerprint do merge): reescrever
    o mesmo path com bytes diferentes invalida o cache quant stale. Diretório
    HF (sem bytes únicos) ou falha de fingerprint → md5 do path (legado).
    """
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
    """Expurga merges mais antigos (mtime) até o root caber no teto.

    Nunca remove o merge atual (keep_dir ou seu pai ``<md5>-<slug>``).
    Falha = warning (nunca derruba o job).
    """
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
