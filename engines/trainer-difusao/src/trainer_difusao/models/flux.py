"""Pipeline de treino LoRA para FLUX.1 e FLUX.2 Klein na GPU."""

from __future__ import annotations

import json
import math
import os
import random
import re
import shutil
from pathlib import Path
from typing import Any

from trainer_difusao.common import (
    ENABLE_TEXT_ENCODER_UNLOAD,
    TextEmbedsCache,
    _build_intx_torchao_config,
    _cached_encode,
    _cleanup_cuda,
    _cycling_batches,
    _die,
    _emit_metric,
    _load_lora_weights,
    _offload_encoders_to_cpu,
    _precompute_text_cache,
    _precompute_text_cache_with_cleanup,
    _prune_checkpoints,
    _resolve_output_name,
    _save_lora_safetensors,
    _setup_cache_dir,
    _temporary_device_encoders,
    _validate_train_aux,
)
from trainer_difusao.dataset import DiffusionDataset, build_dataloader
from trainer_difusao.models.base import BaseModelTrainer
from trainer_difusao.optimizers import _create_lr_scheduler, _create_optimizer



from trainer_difusao.models.flux_pkg import (
    _custom_checkpoint_identity,
    _encode_qwen3_prompt,
    _generate_sample_flux,
    _is_cache_valid,
    _pack_latents,
    _pack_latents_flux2,
    _patchify_latents_flux2,
    _prepare_flux2_latent_ids,
    _prepare_flux2_text_ids,
    _prepare_latent_image_ids,
    _prepare_text_ids,
    _save_quant_metadata,
)

__all__ = [
    "FluxTrainer",
    "_real_train_flux",
    "_custom_checkpoint_identity",
    "_encode_qwen3_prompt",
    "_generate_sample_flux",
    "_is_cache_valid",
    "_pack_latents",
    "_pack_latents_flux2",
    "_patchify_latents_flux2",
    "_prepare_flux2_latent_ids",
    "_prepare_flux2_text_ids",
    "_prepare_latent_image_ids",
    "_prepare_text_ids",
    "_save_quant_metadata",
]
def _real_train_flux(cfg: dict[str, Any], output: Path) -> None:
    """Treino real LoRA para FLUX.2 Klein 4B e FLUX.1 via Diffusers/PEFT com quantização e persistência em cache."""
    output.mkdir(parents=True, exist_ok=True)
    metrics_path = output / "metrics.jsonl"
    if metrics_path.exists():
        metrics_path.unlink()

    hf_token = (
        cfg.get("hf_token")
        or os.environ.get("HF_TOKEN")
        or os.environ.get("HUGGING_FACE_HUB_TOKEN")
        or None
    )
    if hf_token:
        hf_token = hf_token.strip()

    hub_cache = _setup_cache_dir(hf_token=hf_token)

    try:
        import torch
        import torch.nn.functional as F
        from diffusers import (
            AutoencoderKL,
            FlowMatchEulerDiscreteScheduler,
            FluxPipeline,  # noqa: F401
            FluxTransformer2DModel,
        )
        from peft import LoraConfig, get_peft_model
        from transformers import (
            AutoModelForCausalLM,
            AutoTokenizer,
            BitsAndBytesConfig,
            CLIPTextModel,
            T5EncoderModel,
        )
    except ImportError as e:
        _die(f"Dependência ausente para treino real FLUX.2 Klein 4B: {e}")

    if not torch.cuda.is_available():
        _die("CUDA não disponível para treino real de difusão (ENGINE_MOCK=0)")

    device = torch.device("cuda")
    target_dtype = torch.bfloat16 if torch.cuda.is_bf16_supported() else torch.float16

    seed = int(cfg.get("seed", 42))
    model_id = (
        cfg.get("model_id")
        or os.environ.get("FLUX_MODEL_ID")
        or "black-forest-labs/FLUX.2-klein-base-4B"
    )
    is_flux2 = any(k in model_id.lower() for k in ["klein", "flux.2", "flux-2"])

    # --- pesos custom (feat/pesos-custom-flux2): só arch flux-2 chega aqui ---
    # generate.py normaliza aliases p/ "flux-2-klein-4b"; no treino o YAML traz
    # arch resolvido em cfg["model"]. sdxl/sd15 custom são roteados p/ seus
    # loaders (from_single_file mecânico); outro arch ⇒ falha honesta.
    raw_train_arch = str(cfg.get("model", "") or "").strip().lower()
    raw_custom_cp = cfg.get("custom_checkpoint_path")
    custom_checkpoint_path: str | None = None
    if raw_custom_cp:
        if not isinstance(raw_custom_cp, str) or not raw_custom_cp.strip():
            _die("custom_checkpoint_path deve ser uma string não vazia.")
        custom_checkpoint_path = raw_custom_cp.strip()
        if raw_train_arch not in ("flux-2-klein-4b", "flux", "flux2", "flux-2"):
            _die(
                "treino custom não suportado para este arch "
                f"({raw_train_arch or 'indefinido'}). "
                "Checkpoints custom de treino usam arch 'flux-2-klein-4b' "
                "(sdxl/sd15 custom seguem pelos loaders de sdxl.py/sd15.py)."
            )
    raw_encoder_path = cfg.get("text_encoder_path")
    if not raw_encoder_path:
        text_encoder_path: str | None = None
    else:
        if not isinstance(raw_encoder_path, str) or not raw_encoder_path.strip():
            _die("text_encoder_path deve ser uma string não vazia.")
        text_encoder_path = raw_encoder_path.strip()
        if not is_flux2:
            _die(
                "text_encoder_path só é suportado com arch flux-2-klein-4b "
                f"(modelo atual: {model_id})."
            )

    dataset_path = Path(cfg.get("dataset_path", "/datasets"))
    lora_cfg = cfg.get("lora", {})
    epochs = int(lora_cfg.get("epochs", 10))
    batch_size = int(lora_cfg.get("batch_size", 1))
    learning_rate = float(lora_cfg.get("learning_rate", 1e-4))
    rank = int(lora_cfg.get("rank", 16))
    alpha = int(lora_cfg.get("alpha", 16))
    trigger_word = str(lora_cfg.get("trigger_word", ""))
    base_name = _resolve_output_name(cfg)

    aux = _validate_train_aux(cfg, quant_default=None)
    control_dataset_path = aux["control_dataset_path"]
    control_ratio = aux["control_ratio"]
    cache_text_embeddings = aux["cache_text_embeddings"]
    raw_quant = (
        lora_cfg.get("quantization")
        or cfg.get("quantization")
        or os.environ.get("FLUX_QUANTIZATION")
        or aux["quantization"]
        or "4bit"
    )
    quantization = str(raw_quant).strip().lower()
    # Normaliza aliases legados e valida o enum canônico (none/2bit/4bit/6bit/8bit).
    aux_check = _validate_train_aux(
        {**cfg, "quantization": quantization},
        quant_default=None,
    )
    quantization = aux_check["quantization"] or "none"
    is_4bit = quantization == "4bit"
    is_8bit = quantization == "8bit"
    is_2bit = quantization == "2bit"
    is_6bit = quantization == "6bit"
    is_quantized = is_4bit or is_8bit or is_2bit or is_6bit

    if is_4bit:
        quant_label = "4-bit NF4"
        quant_format = "4bit"
        bnb_config = BitsAndBytesConfig(
            load_in_4bit=True,
            bnb_4bit_quant_type="nf4",
            bnb_4bit_compute_dtype=target_dtype,
            bnb_4bit_use_double_quant=True,
        )
        torchao_quant_cfg = None
    elif is_8bit:
        quant_label = "8-bit BitsAndBytes"
        quant_format = "8bit"
        bnb_config = BitsAndBytesConfig(
            load_in_8bit=True,
        )
        torchao_quant_cfg = None
    elif is_2bit or is_6bit:
        # 2bit/6bit via torchao intx weight-only (quantização do modelo base na
        # carga; LoRA treina em cima). Só faz sentido em CUDA — sem torchao/GPU
        # _build_intx_torchao_config dá erro honesto, nunca fallback silencioso.
        quant_label = f"{'2' if is_2bit else '6'}-bit TorchAO intx weight-only"
        quant_format = "2bit" if is_2bit else "6bit"
        bnb_config = None
        torchao_quant_cfg = _build_intx_torchao_config(quant_format)
    else:
        quant_label = "Nenhum (FP16/BF16 pleno)"
        quant_format = "full"
        bnb_config = None
        torchao_quant_cfg = None

    force_requantize = bool(
        cfg.get("force_requantize", False)
        or os.environ.get("FLUX_FORCE_REQUANTIZE", "0").lower() in ("1", "true", "yes")
    )

    _emit_metric(
        metrics_path,
        epoch=0,
        step=1,
        progress=0.01,
        phase="init",
        message=f"Inicializando motor FLUX: {model_id} ({quant_label})...",
    )

    # Classes condicionais para FLUX.2 / Klein se disponíveis no Diffusers instalado
    Flux2Transformer_cls = FluxTransformer2DModel
    AutoencoderKL_cls = AutoencoderKL
    if is_flux2:
        try:
            from diffusers import Flux2Transformer2DModel

            Flux2Transformer_cls = Flux2Transformer2DModel
        except ImportError:
            pass
        try:
            from diffusers import AutoencoderKLFlux2

            AutoencoderKL_cls = AutoencoderKLFlux2
        except ImportError:
            pass

    samples_cfg = cfg.get("samples", {})
    sample_prompt = str(samples_cfg.get("prompt", "") or "").strip()
    sample_interval = int(samples_cfg.get("interval", 1))
    sample_seed = int(samples_cfg.get("seed", seed))

    resolution = int(lora_cfg.get("resolution", 512))
    enable_bucket = bool(lora_cfg.get("enable_bucket", True))
    grad_accum = max(1, int(lora_cfg.get("gradient_accumulation_steps", 1)))
    optimizer_name = str(lora_cfg.get("optimizer", "adamw8bit"))
    lr_scheduler_name = str(lora_cfg.get("lr_scheduler", "cosine"))
    lr_warmup_steps = int(lora_cfg.get("lr_warmup_steps", 0))

    checkpoint_interval = max(
        1, int(cfg.get("checkpoint_interval") or lora_cfg.get("checkpoint_interval") or 1)
    )
    epoch_offset = max(
        0, int(cfg.get("epoch_offset") or lora_cfg.get("epoch_offset") or 0)
    )
    weights_path = cfg.get("weights_path")

    # Cache quantizado isolado por (model_id, quant, custom_checkpoint, encoder).
    # NUNCA reusar pesos de outro checkpoint/encoder: a identidade entra no
    # subfolder E na validade do metadata.json (defesa em profundidade).
    custom_identity = _custom_checkpoint_identity(custom_checkpoint_path)
    if is_quantized:
        model_slug = re.sub(r"[^a-zA-Z0-9_\-\.]", "_", model_id)
        if custom_identity:
            import hashlib as _hl

            custom_slug = _hl.md5(custom_identity.encode("utf-8")).hexdigest()[:12]
            model_slug = f"{model_slug}_custom{custom_slug}"
        if text_encoder_path:
            from trainer_difusao.common import _text_encoder_cache_slug as _enc_slug_fn

            model_slug = f"{model_slug}_enc{_enc_slug_fn(text_encoder_path)}"
        subfolder_quant = f"{model_slug}_{quant_format}"
        quant_base = (
            Path(f"/outputs/.cache/quantized/{subfolder_quant}")
            if Path("/outputs").exists()
            else Path.home() / ".cache" / "hephaestus" / "quantized" / subfolder_quant
        )
        if force_requantize and quant_base.exists():
            print(
                f"[INFO] Forçando re-quantização (force_requantize=True): expurgando cache existente em {quant_base}...",
                flush=True,
            )
            shutil.rmtree(quant_base, ignore_errors=True)

        transformer_cache_dir = quant_base / "transformer"
        text_encoder_cache_dir = quant_base / ("text_encoder" if is_flux2 else "text_encoder_2")
        quant_base.mkdir(parents=True, exist_ok=True)
    else:
        subfolder_quant = None
        quant_base = None
        transformer_cache_dir = None
        text_encoder_cache_dir = None

    print(
        f"Carregando modelos base FLUX ({model_id}) [is_flux2={is_flux2}, quantização: {quant_label}, res: {resolution}, dtype: {target_dtype}]...",
        flush=True,
    )

    # 1. Carregamento do Transformer (DiT): do cache quantizado se já existir e for válido, senão quantiza e salva
    transformer_is_cached = (
        not force_requantize
        and _is_cache_valid(
            transformer_cache_dir,
            expected_model_id=model_id,
            expected_quant=quant_format,
            expected_custom=custom_identity,
        )
    )
    if transformer_is_cached:
        _emit_metric(
            metrics_path,
            epoch=0,
            step=2,
            progress=0.03,
            phase="load_transformer",
            message=f"Carregando Transformer quantizado em {quant_label} do cache persistente...",
        )
        print(
            f"Carregando Transformer quantizado em {quant_label} do cache persistente validado: {transformer_cache_dir}",
            flush=True,
        )
        transformer = Flux2Transformer_cls.from_pretrained(
            transformer_cache_dir,
            torch_dtype=target_dtype,
        )
    else:
        if transformer_cache_dir and transformer_cache_dir.exists():
            print(
                f"[INFO] Cache do transformer em {transformer_cache_dir} é inválido ou pertence a outro modelo. Refazendo quantização...",
                flush=True,
            )
            shutil.rmtree(transformer_cache_dir, ignore_errors=True)

        step_msg_trans = (
            f"Baixando e quantizando Transformer FLUX em {quant_label} ({model_id})..."
            if is_quantized
            else f"Baixando e carregando Transformer FLUX em precisão plena ({model_id})..."
        )
        _emit_metric(
            metrics_path,
            epoch=0,
            step=2,
            progress=0.02,
            phase="quantizing_transformer" if is_quantized else "load_transformer",
            message=step_msg_trans,
        )
        print(step_msg_trans, flush=True)
        try:
            # 2bit/6bit: torchao intx weight-only exige diffusers.TorchAoConfig
            # (quant_type=IntxWeightOnlyConfig). BitsAndBytes não cobre esses
            # níveis — passar quantization_config=None seria precisão plena
            # silenciosa; aqui o config torchao é sempre aplicado quando pedido.
            if torchao_quant_cfg is not None:
                from diffusers import TorchAoConfig as _DiffTorchAoConfig

                effective_quant_cfg: Any = _DiffTorchAoConfig(quant_type=torchao_quant_cfg)
            else:
                effective_quant_cfg = bnb_config
            transformer = Flux2Transformer_cls.from_pretrained(
                model_id,
                subfolder="transformer",
                quantization_config=effective_quant_cfg,
                torch_dtype=target_dtype,
                cache_dir=hub_cache,
                token=hf_token,
            )
            if custom_checkpoint_path:
                # Base custom: transformer do arquivo sobre o carregado do repo.
                # from_single_file sem quantização e depois quantizar seria
                # silenciosamente divergente — aplica o state_dict com falha
                # honesta se o layout não for reconhecido.
                try:
                    from safetensors.torch import load_file as _st_load

                    custom_state = _st_load(custom_checkpoint_path)
                except Exception as exc:
                    _die(
                        f"Falha ao ler checkpoint flux-2 custom "
                        f"({custom_checkpoint_path}): {exc}"
                    )
                try:
                    missing, unexpected = transformer.load_state_dict(
                        custom_state, strict=False
                    )
                except Exception as exc:
                    _die(
                        f"Falha ao aplicar checkpoint flux-2 custom "
                        f"({custom_checkpoint_path}): layout não reconhecido ({exc})"
                    )
                if missing or unexpected:
                    _die(
                        f"Checkpoint flux-2 custom ({custom_checkpoint_path}) com "
                        f"layout não reconhecido: {len(missing)} chave(s) "
                        f"ausente(s), {len(unexpected)} inesperada(s)."
                    )
                print(
                    f"[FLUX] Checkpoint custom aplicado ao transformer: "
                    f"{custom_checkpoint_path}",
                    flush=True,
                )
        except Exception as e:
            if (
                "gated" in str(e).lower()
                or "401" in str(e)
                or "403" in str(e)
                or "not a valid model identifier" in str(e).lower()
            ):
                _die(
                    f"Falha ao baixar modelo FLUX ({model_id}). Este repositório é restrito no Hugging Face.\n"
                    f"1. Aceite a licença do modelo em https://huggingface.co/{model_id}\n"
                    f"2. Defina a variável HF_TOKEN no env.gpu com o seu token de acesso: https://huggingface.co/settings/tokens\n"
                    f"Erro original: {e}"
                )
            raise
        if transformer_cache_dir and quant_base:
            try:
                transformer_cache_dir.mkdir(parents=True, exist_ok=True)
                transformer.save_pretrained(transformer_cache_dir)
                _save_quant_metadata(
                    quant_base, model_id, quant_label, quant_format, target_dtype,
                    is_flux2, custom_checkpoint=custom_identity,
                )
                print(
                    f"Transformer {quant_label} persistido em cache para execuções futuras: {transformer_cache_dir}",
                    flush=True,
                )
            except Exception as e:
                print(
                    f"[WARN] Não foi possível persistir transformer quantizado em disco: {e}",
                    flush=True,
                )

    _emit_metric(
        metrics_path,
        epoch=0,
        step=3,
        progress=0.04,
        phase="transformer_ready",
        message=f"Transformer FLUX ({quant_label}) carregado com sucesso.",
    )

    # 2. Carregamento do(s) Text Encoder(s)
    if is_flux2:
        tokenizer_two = None
        text_encoder_two = None

        text_enc_is_cached = (
            not force_requantize
            and _is_cache_valid(
                text_encoder_cache_dir,
                expected_model_id=model_id,
                expected_quant=quant_format,
                expected_custom=custom_identity,
            )
        )
        if text_enc_is_cached:
            _emit_metric(
                metrics_path,
                epoch=0,
                step=4,
                progress=0.05,
                phase="load_text_encoder",
                message=f"Carregando Text Encoder Qwen3 quantizado em {quant_label} do cache persistente...",
            )
            print(
                f"Carregando Text Encoder Qwen3 quantizado em {quant_label} do cache persistente: {text_encoder_cache_dir}",
                flush=True,
            )
            text_encoder_one = AutoModelForCausalLM.from_pretrained(
                text_encoder_cache_dir,
                torch_dtype=target_dtype,
            )
        else:
            if text_encoder_cache_dir and text_encoder_cache_dir.exists():
                print(
                    f"[INFO] Cache do text encoder em {text_encoder_cache_dir} é inválido ou pertence a outro modelo. Refazendo quantização...",
                    flush=True,
                )
                shutil.rmtree(text_encoder_cache_dir, ignore_errors=True)

            step_msg_enc = (
                f"Baixando e quantizando Text Encoder Qwen3 em {quant_label} ({model_id})..."
                if is_quantized
                else f"Baixando e carregando Text Encoder Qwen3 em precisão plena ({model_id})..."
            )
            _emit_metric(
                metrics_path,
                epoch=0,
                step=4,
                progress=0.04,
                phase="quantizing_text_encoder" if is_quantized else "load_text_encoder",
                message=step_msg_enc,
            )
            print(step_msg_enc, flush=True)
            # Mesmo roteamento torchao do transformer: text encoder Qwen3 também
            # carrega quantizado em 2bit/6bit via transformers.TorchAoConfig.
            if torchao_quant_cfg is not None:
                from transformers import TorchAoConfig as _HfTorchAoConfig

                text_quant_cfg: Any = _HfTorchAoConfig(quant_type=torchao_quant_cfg)
            else:
                text_quant_cfg = bnb_config
            text_encoder_one = AutoModelForCausalLM.from_pretrained(
                model_id,
                subfolder="text_encoder",
                quantization_config=text_quant_cfg,
                torch_dtype=target_dtype,
                cache_dir=hub_cache,
                token=hf_token,
            )
            if text_encoder_path:
                # Override honest: mesmo esquema da geração (dir HF completo
                # ou .safetensors solto). Arquivo solto + quant: os pesos
                # custom NÃO podem ser aplicados via load_state_dict sobre um
                # modelo já quantizado — usa-se o cache de merge compartilhado
                # (bf16 mesclado em disco; quant por carga), mesmo padrão da
                # geração (_load_flux2_loose_encoder_merged).
                from trainer_difusao.common import (
                    _cleanup_merge_tmp_dir,
                    _custom_text_encoder_merge_dir,
                    _load_loose_text_encoder_state,
                    _merged_text_encoder_tmp_dir,
                    _merged_text_encoder_valid,
                    _publish_merged_text_encoder,
                    _sweep_text_encoder_merge_cache,
                    _write_merged_text_encoder_metadata,
                )

                enc_p = Path(text_encoder_path)
                if enc_p.is_dir():
                    try:
                        text_encoder_one = AutoModelForCausalLM.from_pretrained(
                            str(enc_p),
                            quantization_config=text_quant_cfg,
                            torch_dtype=target_dtype,
                        )
                    except Exception as exc:
                        _die(
                            f"Falha ao carregar text_encoder custom de dir "
                            f"({text_encoder_path}): {exc}"
                        )
                    print(
                        f"[FLUX] Text encoder custom (dir): {text_encoder_path}",
                        flush=True,
                    )
                elif enc_p.is_file() and text_quant_cfg is not None:
                    # Rota merge: encoder base em bf16 (SEM quant) → aplica
                    # state_dict → persist merged → recarrega quantizado.
                    # O base já carregado acima veio quantizado — recarrega em
                    # bf16 p/ o merge (custo pago 1x: merged fica em disco).
                    enc_merged_dir, enc_md5 = _custom_text_encoder_merge_dir(
                        text_encoder_path
                    )
                    if _merged_text_encoder_valid(enc_merged_dir, enc_md5):
                        try:
                            text_encoder_one = (
                                AutoModelForCausalLM.from_pretrained(
                                    str(enc_merged_dir),
                                    quantization_config=text_quant_cfg,
                                    torch_dtype=target_dtype,
                                )
                            )
                        except Exception as exc:
                            _die(
                                f"Falha ao carregar text_encoder custom do cache "
                                f"de merge ({enc_merged_dir}): {exc}"
                            )
                        print(
                            f"[FLUX] Text encoder custom (merge em cache: "
                            f"{enc_merged_dir}): {text_encoder_path}",
                            flush=True,
                        )
                    else:
                        try:
                            merge_base = AutoModelForCausalLM.from_pretrained(
                                model_id,
                                subfolder="text_encoder",
                                torch_dtype=target_dtype,
                                cache_dir=hub_cache,
                                token=hf_token,
                            )
                        except Exception as exc:
                            _die(
                                f"Falha ao carregar text encoder base do repo "
                                f"({model_id}) para aplicar override "
                                f"({text_encoder_path}): {exc}"
                            )
                        enc_state = _load_loose_text_encoder_state(
                            text_encoder_path
                        )
                        try:
                            enc_missing, enc_unexpected = (
                                merge_base.load_state_dict(enc_state, strict=False)
                            )
                        except Exception as exc:
                            _die(
                                f"Falha ao aplicar text_encoder custom "
                                f"({text_encoder_path}): layout não "
                                f"reconhecido ({exc})"
                            )
                        _ign = {"lm_head.weight", "model.lm_head.weight"}
                        _enc_miss = [k for k in (enc_missing or []) if k not in _ign]
                        _enc_unexp = [k for k in (enc_unexpected or []) if k not in _ign]
                        if _enc_miss or _enc_unexp:
                            _die(
                                f"text_encoder custom ({text_encoder_path}) com "
                                f"layout não reconhecido: "
                                f"{len(_enc_miss)} chave(s) "
                                f"ausente(s) {_enc_miss[:5]}, "
                                f"{len(_enc_unexp)} inesperada(s) "
                                f"{_enc_unexp[:5]}."
                            )
                        if hasattr(merge_base, "tie_weights"):
                            try:
                                merge_base.tie_weights()
                            except Exception:
                                pass
                        enc_parent = enc_merged_dir.parent
                        enc_tmp = _merged_text_encoder_tmp_dir(enc_merged_dir)
                        try:
                            enc_parent.mkdir(parents=True, exist_ok=True)
                            _cleanup_merge_tmp_dir(enc_tmp)
                            merge_base.save_pretrained(str(enc_tmp))
                            _write_merged_text_encoder_metadata(
                                enc_tmp, md5=enc_md5,
                                basename=enc_p.name, model_id=model_id,
                            )
                            _publish_merged_text_encoder(enc_tmp, enc_merged_dir)
                        except SystemExit:
                            raise
                        except Exception as exc:
                            if _merged_text_encoder_valid(enc_merged_dir, enc_md5):
                                _cleanup_merge_tmp_dir(enc_tmp)
                                print(
                                    f"[FLUX] Text encoder custom (merge concorrente "
                                    f"detectado em {enc_merged_dir}): {text_encoder_path}",
                                    flush=True,
                                )
                            else:
                                _cleanup_merge_tmp_dir(enc_tmp)
                                _die(
                                    f"Falha ao persistir cache de merge do "
                                    f"text_encoder custom ({enc_merged_dir}): {exc}"
                                )
                        try:
                            text_encoder_one = (
                                AutoModelForCausalLM.from_pretrained(
                                    str(enc_merged_dir),
                                    quantization_config=text_quant_cfg,
                                    torch_dtype=target_dtype,
                                )
                            )
                        except Exception as exc:
                            _die(
                                f"Falha ao carregar text_encoder custom do cache "
                                f"de merge ({enc_merged_dir}): {exc}"
                            )
                        print(
                            f"[FLUX] Text encoder custom (merge novo executado: "
                            f"{enc_merged_dir}): {text_encoder_path}",
                            flush=True,
                        )
                        _sweep_text_encoder_merge_cache(enc_merged_dir)
                elif enc_p.is_file():
                    enc_state = _load_loose_text_encoder_state(text_encoder_path)
                    try:
                        enc_missing, enc_unexpected = (
                            text_encoder_one.load_state_dict(enc_state, strict=False)
                        )
                    except Exception as exc:
                        _die(
                            f"Falha ao aplicar text_encoder custom "
                            f"({text_encoder_path}): layout não reconhecido ({exc})"
                        )
                    _ign = {"lm_head.weight", "model.lm_head.weight"}
                    _enc_miss = [k for k in (enc_missing or []) if k not in _ign]
                    _enc_unexp = [k for k in (enc_unexpected or []) if k not in _ign]
                    if _enc_miss or _enc_unexp:
                        _die(
                            f"text_encoder custom ({text_encoder_path}) com layout "
                            f"não reconhecido: "
                            f"{len(_enc_miss)} chave(s) "
                            f"ausente(s) {_enc_miss[:5]}, "
                            f"{len(_enc_unexp)} inesperada(s) "
                            f"{_enc_unexp[:5]}."
                        )
                    if hasattr(text_encoder_one, "tie_weights"):
                        try:
                            text_encoder_one.tie_weights()
                        except Exception:
                            pass
                    print(
                        f"[FLUX] Text encoder custom (.safetensors sobre repo): "
                        f"{text_encoder_path}",
                        flush=True,
                    )
                else:
                    _die(
                        f"text_encoder_path não encontrado: {text_encoder_path}. "
                        "Use um diretório HF ou arquivo .safetensors válido."
                    )
            if text_encoder_cache_dir and quant_base:
                try:
                    text_encoder_cache_dir.mkdir(parents=True, exist_ok=True)
                    text_encoder_one.save_pretrained(text_encoder_cache_dir)
                    _save_quant_metadata(
                        quant_base, model_id, quant_label, quant_format, target_dtype,
                        is_flux2, custom_checkpoint=custom_identity,
                    )
                    print(
                        f"Text Encoder Qwen3 {quant_label} persistido em cache para execuções futuras: {text_encoder_cache_dir}",
                        flush=True,
                    )
                except Exception as e:
                    print(
                        f"[WARN] Não foi possível persistir Text Encoder Qwen3 quantizado em disco: {e}",
                        flush=True,
                    )
        if text_encoder_path and Path(text_encoder_path).is_dir():
            tokenizer_one = AutoTokenizer.from_pretrained(
                str(text_encoder_path), cache_dir=hub_cache, token=hf_token
            )
        else:
            tokenizer_one = AutoTokenizer.from_pretrained(
                model_id, subfolder="tokenizer", cache_dir=hub_cache, token=hf_token
            )
    else:
        # FLUX.1: utiliza Text Encoder CLIP + T5-XXL
        t5_is_cached = (
            not force_requantize
            and _is_cache_valid(
                text_encoder_cache_dir,
                expected_model_id=model_id,
                expected_quant=quant_format,
                expected_custom=custom_identity,
            )
        )
        if t5_is_cached:
            _emit_metric(
                metrics_path,
                epoch=0,
                step=4,
                progress=0.05,
                phase="load_text_encoder",
                message=f"Carregando Text Encoder T5 quantizado em {quant_label} do cache persistente...",
            )
            print(
                f"Carregando Text Encoder T5 quantizado em {quant_label} do cache persistente: {text_encoder_cache_dir}",
                flush=True,
            )
            text_encoder_two = T5EncoderModel.from_pretrained(
                text_encoder_cache_dir,
                torch_dtype=target_dtype,
            )
        else:
            if text_encoder_cache_dir and text_encoder_cache_dir.exists():
                print(
                    f"[INFO] Cache do text encoder T5 em {text_encoder_cache_dir} é inválido ou pertence a outro modelo. Refazendo quantização...",
                    flush=True,
                )
                shutil.rmtree(text_encoder_cache_dir, ignore_errors=True)

            step_msg_t5 = (
                f"Baixando e quantizando Text Encoder T5 em {quant_label} ({model_id})..."
                if is_quantized
                else f"Baixando e carregando Text Encoder T5 em precisão plena ({model_id})..."
            )
            print(step_msg_t5, flush=True)
            # T5-XXL também carrega quantizado em 2bit/6bit via transformers.TorchAoConfig.
            if torchao_quant_cfg is not None:
                from transformers import TorchAoConfig as _HfTorchAoConfig2

                t5_quant_cfg: Any = _HfTorchAoConfig2(quant_type=torchao_quant_cfg)
            else:
                t5_quant_cfg = bnb_config
            text_encoder_two = T5EncoderModel.from_pretrained(
                model_id,
                subfolder="text_encoder_2",
                quantization_config=t5_quant_cfg,
                torch_dtype=target_dtype,
                cache_dir=hub_cache,
                token=hf_token,
            )
            if text_encoder_cache_dir and quant_base:
                try:
                    text_encoder_cache_dir.mkdir(parents=True, exist_ok=True)
                    text_encoder_two.save_pretrained(text_encoder_cache_dir)
                    _save_quant_metadata(
                        quant_base, model_id, quant_label, quant_format, target_dtype,
                        is_flux2, custom_checkpoint=custom_identity,
                    )
                    print(
                        f"Text Encoder T5 {quant_label} persistido em cache para execuções futuras: {text_encoder_cache_dir}",
                        flush=True,
                    )
                except Exception as e:
                    print(
                        f"[WARN] Não foi possível persistir Text Encoder T5 quantizado em disco: {e}",
                        flush=True,
                    )

        tokenizer_one = AutoTokenizer.from_pretrained(
            model_id, subfolder="tokenizer", use_fast=False, cache_dir=hub_cache, token=hf_token
        )
        tokenizer_two = AutoTokenizer.from_pretrained(
            model_id, subfolder="tokenizer_2", use_fast=False, cache_dir=hub_cache, token=hf_token
        )
        text_encoder_one = CLIPTextModel.from_pretrained(
            model_id, subfolder="text_encoder", torch_dtype=target_dtype, cache_dir=hub_cache, token=hf_token
        ).to(device)

    _emit_metric(
        metrics_path,
        epoch=0,
        step=5,
        progress=0.06,
        phase="text_encoder_ready",
        message=f"Text Encoder ({quant_label}) pronto.",
    )

    # 3. Componentes auxiliares (VAE float32, Scheduler Flow Matching)
    vae = AutoencoderKL_cls.from_pretrained(
        model_id, subfolder="vae", torch_dtype=torch.float32, cache_dir=hub_cache, token=hf_token
    ).to(device)
    noise_scheduler = FlowMatchEulerDiscreteScheduler.from_pretrained(
        model_id, subfolder="scheduler", cache_dir=hub_cache, token=hf_token
    )

    vae.requires_grad_(False)
    text_encoder_one.requires_grad_(False)
    if text_encoder_two is not None:
        text_encoder_two.requires_grad_(False)
    transformer.requires_grad_(False)

    # 4. Injeção de adaptadores LoRA via PEFT nas camadas lineares do Transformer FLUX
    target_modules = [
        "to_k", "to_q", "to_v", "to_out.0",
        "add_k_proj", "add_v_proj", "add_q_proj", "to_add_out",
        "to_qkv_mlp_proj", "to_out_mlp_proj",
        "linear1", "linear2",
    ]
    lora_config = LoraConfig(
        r=rank,
        lora_alpha=alpha,
        init_lora_weights="gaussian",
        target_modules=target_modules,
    )
    # Gradient checkpointing economiza ~50% VRAM (diffusers usa use_reentrant=False nativo)
    # Nota: prepare_model_for_kbit_training do PEFT é exclusivo de modelos NLP/transformers;
    # em diffusers ele quebra o SDPA ao fazer cast de norm_q/norm_k p/ float32 enquanto value é bf16.
    transformer.enable_gradient_checkpointing()
    transformer = get_peft_model(transformer, lora_config)
    if weights_path:
        _load_lora_weights(transformer, weights_path)
    transformer.train()

    # Confirma congelamento dos pesos base e isolamento estrito da LoRA
    trainable_params_count = sum(p.numel() for p in transformer.parameters() if p.requires_grad)
    frozen_params_count = sum(p.numel() for p in transformer.parameters() if not p.requires_grad)
    print(
        f"[FLUX] Parâmetros treináveis LoRA: {trainable_params_count:,} | "
        f"Pesos base congelados: {frozen_params_count:,}",
        flush=True,
    )

    _emit_metric(
        metrics_path,
        epoch=0,
        step=6,
        progress=0.07,
        phase="setup_lora",
        message=f"Adaptadores LoRA injetados no Transformer (rank={rank}, alpha={alpha}, treináveis: {trainable_params_count:,}).",
    )

    # 5. Dataset de treino (+ controle para prior-preservation, se configurado)
    dataset = DiffusionDataset(
        dataset_path,
        resolution=resolution,
        trigger_word=trigger_word,
        enable_bucket=enable_bucket,
        metrics_path=metrics_path,
    )
    if len(dataset) == 0:
        _die(f"Nenhum par imagem+legenda (.txt) encontrado em: {dataset_path}")

    dataloader = build_dataloader(dataset, batch_size, seed=seed)

    # Dataset de controle: mesma resolução/bucketing, caption VAZIA (sem
    # trigger word). Intercalação por step via _cycling_batches com prob.
    # control_ratio — mesma pipeline de ruído/loss no mesmo step.
    control_dataset = None
    control_iter = None
    control_n = 0
    if control_dataset_path is not None:
        control_dataset = DiffusionDataset(
            control_dataset_path,
            resolution=resolution,
            trigger_word="",
            enable_bucket=enable_bucket,
            empty_captions=True,
            metrics_path=metrics_path,
        )
        control_iter = _cycling_batches(
            build_dataloader(control_dataset, batch_size, seed=seed)
        )
        control_n = len(control_dataset)

    _emit_metric(
        metrics_path,
        epoch=0,
        step=7,
        progress=0.08,
        phase="dataset_ready",
        message=(
            f"Dataset carregado com sucesso: {len(dataset)} amostras"
            + (f" em {len(dataset.buckets)} buckets de aspect ratio." if enable_bucket else ".")
        ),
    )
    print(
        f"[FLUX] Treino: dataset={len(dataset)} imagens, "
        f"control_dataset_images={control_n}, control_ratio={control_ratio}, "
        f"cache_text_embeddings={cache_text_embeddings}, quantization={quantization}",
        flush=True,
    )

    # 6. Otimizador e LR Scheduler
    optimizer = _create_optimizer(transformer, optimizer_name, learning_rate)
    steps_per_epoch = math.ceil(len(dataloader) / grad_accum)
    total_train_steps = max(1, steps_per_epoch * epochs)
    lr_scheduler = _create_lr_scheduler(
        optimizer, lr_scheduler_name, total_train_steps, lr_warmup_steps
    )

    # Cache de text embeddings pré-computado UMA vez no início (miss → on-the-fly
    # + warm; falha → segue sem cache). O que é cacheado por arch:
    # - FLUX.2 Klein: saída do Qwen3 (_encode_qwen3_prompt: 3 camadas ocultas
    #   concatenadas); txt_ids são determinísticos p/ seq_len e vão no payload.
    # - FLUX.1: saída do CLIP (pooled) + saída do T5; txt_ids idem.
    text_cache = TextEmbedsCache(output, cache_text_embeddings)
    if cache_text_embeddings:
        def _encode_flux_all(caps: list[str]) -> dict[str, Any]:
            with torch.no_grad():
                if is_flux2:
                    hidden = _encode_qwen3_prompt(
                        text_encoder_one, tokenizer_one, caps, device, target_dtype
                    )
                    return {"hidden": hidden}
                clip_inputs = tokenizer_one(
                    caps, padding="max_length", max_length=77,
                    truncation=True, return_tensors="pt",
                ).to(device)
                pooled = text_encoder_one(clip_inputs.input_ids).pooler_output
                t5_inputs = tokenizer_two(
                    caps, padding="max_length", max_length=512,
                    truncation=True, return_tensors="pt",
                ).to(device)
                hidden = text_encoder_two(t5_inputs.input_ids)[0]
                return {"hidden": hidden, "pooled": pooled}
        should_unload = ENABLE_TEXT_ENCODER_UNLOAD and epoch_offset == 0
        if should_unload:
            _precompute_text_cache_with_cleanup(
                text_cache,
                [c for _, c in dataset.samples]
                + ([c for _, c in control_dataset.samples] if control_dataset else []),
                _encode_flux_all,
                metrics_path=metrics_path,
                unload_encoders=True,
                encoders=[text_encoder_one, text_encoder_two],
            )
        else:
            _precompute_text_cache(
                text_cache,
                [c for _, c in dataset.samples]
                + ([c for _, c in control_dataset.samples] if control_dataset else []),
                _encode_flux_all,
                metrics_path=metrics_path,
            )

    # Amostra baseline (Época 0) para comparação pré-treino (apenas se não estiver retomando)
    if sample_prompt and epoch_offset == 0:
        _emit_metric(
            metrics_path,
            epoch=0,
            step=8,
            progress=0.09,
            phase="generating_baseline_sample",
            message=f"Gerando amostra baseline pré-treino (Época 0): '{sample_prompt[:40]}...'",
        )
        sample_baseline_file = output / "samples" / "sample_epoch_000.png"
        with _temporary_device_encoders([text_encoder_one, text_encoder_two], device):
            _generate_sample_flux(
                transformer=transformer,
                vae=vae,
                text_encoder_one=text_encoder_one,
                text_encoder_two=text_encoder_two,
                tokenizer_one=tokenizer_one,
                tokenizer_two=tokenizer_two,
                scheduler=noise_scheduler,
                prompt=sample_prompt,
                output_path=sample_baseline_file,
                seed=sample_seed,
                is_flux2=is_flux2,
                resolution=resolution,
                metrics_path=metrics_path,
                epoch=0,
            )
        _emit_metric(
            metrics_path,
            epoch=0,
            step=9,
            progress=0.10,
            phase="baseline_ready",
            message="Amostra baseline gerada com sucesso (Época 0).",
        )

    _emit_metric(
        metrics_path,
        epoch=epoch_offset,
        step=10,
        progress=0.10,
        phase="training_started",
        message=f"Iniciando loop de treino LoRA: {epochs} épocas (offset={epoch_offset}), {total_train_steps} passos totais.",
    )

    flow_shift = float(getattr(noise_scheduler.config, "shift", 3.0) or 3.0)

    if is_flux2:
        # FLUX.2 Klein: VAE com 32 canais e espaço latente retreinado (AutoencoderKLFlux2).
        # Jamais herdar os valores legados do Flux.1 (shift=0.1159 / scaling=0.3611).
        # Extrai os parâmetros reais da config do checkpoint Klein:
        shift_factor = getattr(vae.config, "shift_factor", 0.0) or 0.0
        scaling_factor = getattr(vae.config, "scaling_factor", 1.0) or 1.0
        latents_mean = getattr(vae.config, "latents_mean", None)
        latents_std = getattr(vae.config, "latents_std", None)
    else:
        # FLUX.1: VAE clássica com 16 canais
        shift_factor = getattr(vae.config, "shift_factor", None)
        if shift_factor is None or shift_factor == 0.0:
            shift_factor = 0.1159
        scaling_factor = getattr(vae.config, "scaling_factor", None)
        if scaling_factor is None or scaling_factor == 0.0:
            scaling_factor = 0.3611
        latents_mean = None
        latents_std = None

    print(
        f"[FLUX] Iniciando treino: {epochs} épocas (offset={epoch_offset}), {total_train_steps} passos totais, lr={learning_rate}, "
        f"is_flux2={is_flux2}, shift_factor={shift_factor}, scaling_factor={scaling_factor}, "
        f"tem_bn={hasattr(vae, 'bn') and getattr(vae.bn, 'running_mean', None) is not None}, "
        f"tem_stats_config={latents_mean is not None}",
        flush=True,
    )

    global_step = 0
    safe_avg_loss = None

    def _encode_flux2_batch(caps: list[str]) -> dict[str, Any]:
        hidden = _encode_qwen3_prompt(
            text_encoder_one, tokenizer_one, caps, device, target_dtype
        )
        return {"hidden": hidden}

    def _encode_flux1_batch(caps: list[str]) -> dict[str, Any]:
        clip_inputs = tokenizer_one(
            caps,
            padding="max_length",
            max_length=77,
            truncation=True,
            return_tensors="pt",
        ).to(device)
        pooled = text_encoder_one(clip_inputs.input_ids).pooler_output
        t5_inputs = tokenizer_two(
            caps,
            padding="max_length",
            max_length=512,
            truncation=True,
            return_tensors="pt",
        ).to(device)
        hidden = text_encoder_two(t5_inputs.input_ids)[0]
        return {"hidden": hidden, "pooled": pooled}

    for epoch_idx in range(1, epochs + 1):
        epoch = epoch_idx + epoch_offset
        epoch_loss = 0.0
        steps_in_epoch = 0
        optimizer.zero_grad()

        for batch in dataloader:
            # Prior-preservation: com prob. control_ratio usa o batch de controle
            # (regularização, captions vazias) no loss do mesmo step.
            if control_iter is not None and random.random() < control_ratio:
                batch = next(control_iter)
            pixel_values = batch["pixel_values"].to(device)
            captions = batch["prompt"]
            bsz = pixel_values.shape[0]

            # Codifica imagens com VAE (em float32 para evitar instabilidade numérica)
            with torch.no_grad():
                latents = vae.encode(pixel_values.float()).latent_dist.sample()

                if is_flux2:
                    latents = _patchify_latents_flux2(latents)
                    if hasattr(vae, "bn") and getattr(vae.bn, "running_mean", None) is not None:
                        latents_bn_mean = vae.bn.running_mean.view(1, -1, 1, 1).to(latents.device, latents.dtype)
                        latents_bn_std = torch.sqrt(
                            vae.bn.running_var.view(1, -1, 1, 1) + getattr(vae.config, "batch_norm_eps", 1e-5)
                        ).to(latents.device, latents.dtype)
                        latents = (latents - latents_bn_mean) / latents_bn_std
                    elif latents_mean is not None and latents_std is not None:
                        t_mean = torch.tensor(latents_mean, device=latents.device, dtype=latents.dtype).view(1, -1, 1, 1)
                        t_std = torch.tensor(latents_std, device=latents.device, dtype=latents.dtype).view(1, -1, 1, 1)
                        latents = (latents - t_mean) / t_std
                    else:
                        latents = (latents - shift_factor) * scaling_factor
                    latents = latents.to(dtype=target_dtype)
                    img_ids = _prepare_flux2_latent_ids(latents)
                    packed_latents = _pack_latents_flux2(latents)

                    prompt_embeds = _cached_encode(
                        captions,
                        _encode_flux2_batch,
                        text_cache,
                        encoders=[text_encoder_one, text_encoder_two],
                        device=device,
                    )["hidden"].to(device, dtype=target_dtype)
                    txt_ids = _prepare_flux2_text_ids(prompt_embeds)
                    pooled_prompt_embeds = None
                else:
                    latents = (latents - shift_factor) * scaling_factor
                    latents = latents.to(dtype=target_dtype)
                    packed_latents = _pack_latents(latents)
                    img_ids = _prepare_latent_image_ids(
                        bsz,
                        pixel_values.shape[2],
                        pixel_values.shape[3],
                        device,
                        target_dtype,
                    )

                    cached = _cached_encode(
                        captions,
                        _encode_flux1_batch,
                        text_cache,
                        encoders=[text_encoder_one, text_encoder_two],
                        device=device,
                    )
                    prompt_embeds = cached["hidden"].to(device, dtype=target_dtype)
                    pooled_prompt_embeds = cached["pooled"].to(device, dtype=target_dtype)
                    txt_ids = _prepare_text_ids(prompt_embeds.shape[1], device, prompt_embeds.dtype, batch_size=bsz)

            # Ruído gaussiano e timesteps amostrados com shifted logit-normal para Flow Matching
            noise = torch.randn_like(packed_latents)
            u = torch.normal(mean=0.0, std=1.0, size=(bsz,), device=device)
            t_sigmoid = torch.sigmoid(u)
            # Deslocamento de fluxo (time-shift schedule do Flux)
            flow_shift = float(getattr(noise_scheduler.config, "shift", 3.0) or 3.0)
            timesteps = (flow_shift * t_sigmoid) / (1.0 + (flow_shift - 1.0) * t_sigmoid)

            # Interpolação do fluxo retificado: x_t = (1 - t) * x_0 + t * noise
            t_expanded = timesteps.view(-1, 1, 1).to(dtype=target_dtype)
            noisy_latents = (1.0 - t_expanded) * packed_latents + t_expanded * noise
            target = noise - packed_latents

            # Forward no Transformer FLUX com adaptadores LoRA ativos
            if is_flux2:
                model_pred = transformer(
                    hidden_states=noisy_latents,
                    timestep=timesteps,
                    encoder_hidden_states=prompt_embeds,
                    txt_ids=txt_ids,
                    img_ids=img_ids,
                    return_dict=False,
                )[0]
            else:
                # Treino de LoRA em FLUX.1-dev exige guidance=1.0 (não usar 3.5 da inferência)
                guidance = torch.full((bsz,), 1.0, device=device, dtype=target_dtype)
                model_pred = transformer(
                    hidden_states=noisy_latents,
                    timestep=timesteps,
                    guidance=guidance,
                    pooled_projections=pooled_prompt_embeds,
                    encoder_hidden_states=prompt_embeds,
                    txt_ids=txt_ids,
                    img_ids=img_ids,
                    return_dict=False,
                )[0]

            loss = F.mse_loss(model_pred.float(), target.float(), reduction="mean")
            cur_loss_raw = loss.item()
            loss = loss / grad_accum
            loss.backward()

            steps_in_epoch += 1
            is_accum_step = (steps_in_epoch % grad_accum == 0) or (steps_in_epoch == len(dataloader))
            if is_accum_step:
                torch.nn.utils.clip_grad_norm_(transformer.parameters(), 1.0)
                optimizer.step()
                if lr_scheduler is not None:
                    lr_scheduler.step()
                optimizer.zero_grad()
                global_step += 1

            if not math.isnan(cur_loss_raw) and not math.isinf(cur_loss_raw):
                epoch_loss += cur_loss_raw

            effective_lr = (
                lr_scheduler.get_last_lr()[0] if lr_scheduler else learning_rate
            )

            # Emite métricas intermediárias a cada 5 passos de otimização ou no fim da época
            if is_accum_step and (global_step % 5 == 0 or steps_in_epoch == len(dataloader)):
                safe_loss = (
                    None
                    if (math.isnan(cur_loss_raw) or math.isinf(cur_loss_raw))
                    else round(cur_loss_raw, 4)
                )
                current_progress = round(
                    min(0.99, max(0.10, 0.10 + 0.89 * (global_step / max(1, total_train_steps)))), 4
                )
                _emit_metric(
                    metrics_path,
                    epoch=epoch,
                    step=global_step,
                    loss=safe_loss,
                    lr=effective_lr,
                    progress=current_progress,
                    phase="training",
                    message=f"Época {epoch}/{epochs} · Step {global_step}/{total_train_steps} · Loss: {safe_loss}",
                )
                print(
                    f"[FLUX] Época {epoch}/{epochs} · Step {global_step}/{total_train_steps} · Loss: {safe_loss} · LR: {effective_lr:.2e}",
                    flush=True,
                )

        avg_loss = epoch_loss / max(1, steps_in_epoch)
        safe_avg_loss = (
            None
            if (math.isnan(avg_loss) or math.isinf(avg_loss))
            else round(avg_loss, 4)
        )
        epoch_progress = round(
            min(0.99, max(0.10, 0.10 + 0.89 * (epoch_idx / epochs))), 4
        )
        _emit_metric(
            metrics_path,
            epoch=epoch,
            step=global_step,
            loss=safe_avg_loss,
            lr=effective_lr,
            progress=epoch_progress,
            phase="epoch_complete",
            message=f"Época {epoch}/{epochs + epoch_offset} concluída · Loss Média: {safe_avg_loss}",
        )

        print(
            f"[FLUX] Concluída Época {epoch}/{epochs + epoch_offset} · Loss Média: {safe_avg_loss} · LR: {effective_lr:.2e}",
            flush=True,
        )

        # Salva checkpoint da época respeitando checkpoint_interval
        if epoch_idx % checkpoint_interval == 0 or epoch_idx == epochs:
            checkpoints_dir = output / "checkpoints"
            checkpoints_dir.mkdir(parents=True, exist_ok=True)
            ckpt_file = checkpoints_dir / f"{base_name}_epoch_{epoch:03d}.safetensors"
            _save_lora_safetensors(
                transformer,
                ckpt_file,
                metadata={
                    "format": "pt",
                    "model_type": "lora",
                    "base_model": "flux-2-klein-4b" if is_flux2 else "flux-1",
                    "lora_rank": str(rank),
                    "lora_alpha": str(alpha),
                    "trigger_word": trigger_word,
                    "quantization": quantization,
                    "epoch": str(epoch),
                },
            )
            _prune_checkpoints(checkpoints_dir, keep_last_n=2)

        _cleanup_cuda()

        # Geração de amostra visual periódica
        if sample_prompt and sample_interval > 0 and (epoch_idx % sample_interval == 0 or epoch_idx == epochs):
            _emit_metric(
                metrics_path,
                epoch=epoch,
                phase="generating_sample",
                message=f"Iniciando geração de amostra visual (Época {epoch})...",
                telemetry_only=True,
            )
            sample_file = output / "samples" / f"sample_epoch_{epoch:03d}.png"
            with _temporary_device_encoders([text_encoder_one, text_encoder_two], device):
                _generate_sample_flux(
                    transformer=transformer,
                    vae=vae,
                    text_encoder_one=text_encoder_one,
                    text_encoder_two=text_encoder_two,
                    tokenizer_one=tokenizer_one,
                    tokenizer_two=tokenizer_two,
                    scheduler=noise_scheduler,
                    prompt=sample_prompt,
                    output_path=sample_file,
                    seed=sample_seed,
                    is_flux2=is_flux2,
                    resolution=resolution,
                    metrics_path=metrics_path,
                    epoch=epoch,
                )
            _emit_metric(
                metrics_path,
                epoch=epoch,
                phase="sample_ready",
                message=f"Amostra visual da Época {epoch} pronta.",
                telemetry_only=True,
            )

    # Salva adaptador LoRA final em safetensors com metadados
    final_adapter_file = output / f"{base_name}.safetensors"
    metadata = {
        "format": "pt",
        "model_type": "lora",
        "base_model": "flux-2-klein-4b" if is_flux2 else "flux-1",
        "lora_rank": str(rank),
        "lora_alpha": str(alpha),
        "trigger_word": trigger_word,
        "quantization": quantization,
    }
    _save_lora_safetensors(transformer, final_adapter_file, metadata)
    if base_name != "adapter":
        shutil.copy2(final_adapter_file, output / "adapter.safetensors")

    _emit_metric(
        metrics_path,
        epoch=epochs,
        step=global_step,
        loss=safe_avg_loss,
        lr=effective_lr,
        progress=1.0,
        phase="completed",
        message="Treino FLUX LoRA finalizado com sucesso!",
    )
    print(f"Treino FLUX finalizado com sucesso! Checkpoint salvo em: {final_adapter_file}")


class FluxTrainer(BaseModelTrainer):
    """Trainer de difusão para FLUX.2 Klein 4B e FLUX.1."""

    def train(self, cfg: dict[str, Any], output: Path) -> None:
        _real_train_flux(cfg, output)
