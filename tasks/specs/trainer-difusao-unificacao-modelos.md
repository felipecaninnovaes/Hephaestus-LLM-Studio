# Unificação dos 4 Trainers de Difusão — Template Method (`models/sd15.py`, `sdxl.py`, `flux.py`, `qwen_image.py`)

**Escopo:** `engines/trainer-difusao/src/trainer_difusao/models/`
**Data:** 2026-09-26
**Status:** Pendente aprovação/implementação (spec-only, sem código ainda)
**Supersede/estende:** `tasks/specs/engines-auditoria-global.md` Fase 2 #7 (`BitsAndBytesConfig` duplicada), #8 (boilerplate de `cfg` parsing duplicado) e #12 (`qwen_image` usa `prompt_cache` em RAM em vez de `TextEmbedsCache`) — esta spec resolve os três como efeito colateral de uma extração estrutural maior, em vez de 3 fixes pontuais.

---

## 1. Motivação (evidência, não opinião)

`trainer-difusao` concentra 10.029 das ~12.6k linhas de `engines/*` (engine-kit 656, trainer-clip 308, trainer-yolo 1.616). Dentro dela, os 4 arquivos de modelo somam 3.250 linhas:

| Arquivo | Linhas | Papel |
|---|---:|---|
| `models/flux.py` | 1.026 | Flux-1 + Flux-2-Klein (DiT, flow-matching) |
| `models/qwen_image.py` | 914 | Qwen-Image-2.1 (DiT + VLM text encoder) |
| `models/sdxl.py` | 666 | SDXL 1.0 (UNet clássico) |
| `models/sd15.py` | 644 | SD 1.5 (UNet clássico) |

Similaridade textual (`difflib.SequenceMatcher`, medido nos 4 arquivos):

| Par | Ratio |
|---|---:|
| `sd15.py` × `sdxl.py` | **0.71** |
| `sdxl.py` × `flux.py` | 0.41 |
| `sd15.py` × `flux.py` | 0.38 |
| `sd15.py`/`sdxl.py` × `qwen_image.py` | 0.15–0.16 |
| `flux.py` × `qwen_image.py` | 0.14 |

`diff sd15.py sdxl.py` e a leitura integral dos 4 arquivos confirmam: a função `_real_train_*` de cada modelo reimplementa do zero a **mesma orquestração de laço de treino**, só variando a física do forward (UNet+DDPM vs DiT+flow-matching) e as classes de tokenizer/encoder. Trechos idênticos ou quase-idênticos nos 4 arquivos:

1. Setup: `output.mkdir` → reset de `metrics.jsonl` → `_setup_cache_dir()` → import guard com `_die()` → `torch.cuda.is_available()` guard.
2. Parsing de `cfg`/`lora_cfg`: `seed`, `epochs`, `batch_size`, `learning_rate`, `rank`, `alpha`, `trigger_word`, `resolution`, `enable_bucket`, `grad_accum`, `optimizer_name`, `lr_scheduler_name`, `lr_warmup_steps`, `checkpoint_interval`, `epoch_offset`, `weights_path`, `mixed_precision`/`target_dtype`, `quantization`/`bnb_config` (idêntico byte-a-byte em `sd15`/`sdxl`/`flux`).
3. Dataset principal + dataset de controle (prior-preservation) com `_cycling_batches` e `control_ratio`.
4. `optimizer = _create_optimizer(...)`, `lr_scheduler = _create_lr_scheduler(...)`, pré-cômputo de `sample_embeds`, `TextEmbedsCache` com/sem `_precompute_text_cache_with_cleanup`.
5. Amostra baseline (época 0) condicionada a `epoch_offset == 0`, com o mesmo par de `_emit_metric(phase="generating_baseline_sample"/"baseline_ready"/"baseline_failed")`.
6. Laço `for epoch_idx in range(1, epochs+1)`: acumulação de gradiente (`is_accum_step`), `clip_grad_norm_`, `optimizer.step()`/`lr_scheduler.step()`/`zero_grad()`, `epoch_loss` guardado com `math.isnan`/`isinf`, emissão de métrica a cada 5 steps de otimização.
7. Fim de época: `_emit_metric(phase="epoch_complete")`, `save_adapter_checkpoint` + `_prune_checkpoints(keep_last_n=2)` respeitando `checkpoint_interval`, `_cleanup_cuda()`, geração de amostra periódica (`sample_interval`) com o mesmo trio `generating_sample`/`sample_ready`/`sample_failed`.
8. Fechamento: `save_final_adapter` + `_emit_metric(phase="completed", progress=1.0)` + print de conclusão.

`qwen_image.py` tem a mesma esqueleto (setup → laço época/step → accum-step → checkpoint → sample → final) mas já ganhou telemetria enriquecida (ETA por EMA, `step_time_s`, `total_steps`, `total_epochs`, VRAM alocada/reservada — fatia `feat/engines-live-telemetry-eta`) que **os outros 3 arquivos não têm**. Isso é uma inconsistência viva: a mesma feature foi implementada 1x em vez de 1x-compartilhada-por-4.

`BaseModelTrainer` (`models/base.py`) é só uma interface (`train()` abstrato) — não existe Template Method hoje. `models/mock.py` já prova que um único `MockTrainer` cobre as 4 archs (precedente direto do padrão proposto aqui para os trainers reais).

## 2. Não-Objetivos (preservar 100%)

- **Zero mudança numérica/algorítmica.** Loss (`F.mse_loss` no ruído para UNet; flow-matching target para DiT), VAE encode/scaling, injeção de LoRA (`target_modules`, `rank`, `alpha`), scheduler de ruído, bucketing, prior-preservation — tudo migra **verbatim** para dentro dos hooks por arquitetura. Isto é refactor estrutural, não reescrita.
- **Não reabrir pitfalls já pagos** (`docs/PITFALLS.md` § Engines): shadowing de `alpha` (LoRA vs canal alpha da imagem), VAE 4-canais, `image_pad_mask` do `QwenImage21Pipeline`, unpack de latents com `_pack_latents`, descarregamento de VLM text encoder. A extração deve mover esse código para dentro de `forward_and_loss`/`load_components` sem tocar a lógica interna.
- **Não mexe em `models/mock.py`, `generation/*`, `serve_pkg/*`, `loaders/*`** (já modularizados na fatia `refactor/modularizacao-engines`). Só os 4 arquivos de trainer real + um novo módulo de runner compartilhado.
- **Não implementa aqui** os itens #1–#6, #9–#11, #13–#18 do `engines-auditoria-global.md` — ficam com seus donos/fases originais.

## 3. Arquitetura Proposta — Template Method

### 3.1 Novo módulo `models/loop.py`

```python
@dataclass(frozen=True)
class LoraTrainConfig:
    seed: int
    model_id: str
    dataset_path: Path
    epochs: int
    batch_size: int
    learning_rate: float
    rank: int
    alpha: int
    trigger_word: str
    base_name: str
    resolution: int
    enable_bucket: bool
    grad_accum: int
    optimizer_name: str
    lr_scheduler_name: str
    lr_warmup_steps: int
    checkpoint_interval: int
    epoch_offset: int
    weights_path: str | None
    mixed_precision: str
    quantization: str
    sample_prompt: str
    sample_interval: int
    sample_seed: int
    custom_checkpoint_path: str | None
    text_encoder_path: str | None  # sempre None exceto qwen/flux-2-klein

def parse_lora_train_config(
    cfg: dict[str, Any],
    *,
    default_model_id: str,
    default_resolution: int,
    quant_default: str = "none",
    allow_custom_checkpoint: bool = True,
    allow_text_encoder_path: bool = False,
) -> LoraTrainConfig:
    """Substitui o parsing duplicado (item #8 da auditoria). Usa
    trainer_difusao.common_pkg.train_config._validate_train_aux internamente."""
```

`ModelAdapter` — Protocol implementado por 1 classe pequena por arquitetura (`SD15Adapter`, `SDXLAdapter`, `FluxAdapter` com `family: Literal["flux1","flux2"]`, `QwenImageAdapter`):

```python
class ModelComponents(TypedDict):
    trainable_module: Any        # unet ou transformer, já com LoRA injetado
    device: "torch.device"
    dtype: "torch.dtype"
    extra: dict[str, Any]        # tokenizers, encoders, vae, noise_scheduler — uso interno do adapter

class ModelAdapter(Protocol):
    arch_label: str              # "SD 1.5" | "SDXL" | "FLUX" | "Qwen-Image"
    metadata_base_model: str     # "sd15" | "sdxl" | "flux1" | "flux2" | "qwen-image"

    def load_and_inject_lora(self, tcfg: LoraTrainConfig, hub_cache: str, metrics_path: Path) -> ModelComponents: ...
    def build_text_cache_encode_fn(self, comp: ModelComponents) -> Callable[[list[str]], dict[str, Any]]: ...
    def text_cache_encoders(self, comp: ModelComponents) -> list[Any]: ...          # p/ unload/offload
    def precompute_sample_embeds(self, comp: ModelComponents, tcfg: LoraTrainConfig) -> Any | None: ...
    def forward_and_loss(self, comp: ModelComponents, batch: dict, tcfg: LoraTrainConfig, cached_encode: dict[str, Any]) -> "torch.Tensor": ...
    def generate_sample(self, comp: ModelComponents, sample_file: Path, tcfg: LoraTrainConfig, *, epoch: int, metrics_path: Path, sample_embeds: Any | None) -> None: ...
    def checkpoint_metadata(self, tcfg: LoraTrainConfig, *, epoch: int | None = None) -> dict[str, str]: ...
```

`TrainingLoopRunner` (única implementação do laço, hoje 4x duplicado):

```python
class TrainingLoopRunner:
    def __init__(self, adapter: ModelAdapter) -> None: ...

    def run(self, cfg: dict[str, Any], output: Path) -> None:
        # 1. mkdir + reset metrics.jsonl + _setup_cache_dir()
        # 2. tcfg = parse_lora_train_config(cfg, ...)  [arch injeta defaults via adapter]
        # 3. comp = adapter.load_and_inject_lora(tcfg, hub_cache, metrics_path)
        # 4. optimizer/scheduler via _create_optimizer/_create_lr_scheduler (já compartilhado)
        # 5. dataset + control_dataset + _cycling_batches (já compartilhado)
        # 6. sample_embeds = adapter.precompute_sample_embeds(...)
        # 7. TextEmbedsCache + _precompute_text_cache[_with_cleanup] via adapter.build_text_cache_encode_fn/text_cache_encoders
        # 8. baseline sample (epoch 0) via adapter.generate_sample
        # 9. for epoch_idx in range(1, epochs+1):
        #      for batch in dataloader:
        #        loss = adapter.forward_and_loss(comp, batch, tcfg, cached_encode)
        #        <accum-step: clip_grad_norm_, optimizer.step, lr_scheduler.step, zero_grad>
        #        <emit_metric cadence: ETA/EMA/step_time — canonicalizado do qwen_image>
        #      <epoch_complete emit, save_adapter_checkpoint+prune, _cleanup_cuda, sample periódica>
        # 10. save_final_adapter + emit_metric(completed, progress=1.0)
```

**Ganho colateral:** ETA por EMA + `step_time_s` + VRAM alocada/reservada (hoje só em `qwen_image.py`) passam a existir para SD15/SDXL/Flux automaticamente — sem trabalho extra, porque a emissão de métrica migra para dentro do `TrainingLoopRunner`.

### 3.2 Arquivos resultantes

```
models/
├── base.py            # inalterado (BaseModelTrainer)
├── loop.py             # NOVO: LoraTrainConfig, parse_lora_train_config, ModelAdapter, TrainingLoopRunner
├── sd_family/
│   ├── __init__.py
│   ├── adapter.py      # NOVO: SD15Adapter, SDXLAdapter (parametrizados por family, ~250-300L total)
│   └── ...              # sample.py/embeddings.py já existem em models/sd_pkg/, só trocam de import
├── sd15.py              # ~40L: SD15Trainer(BaseModelTrainer) → TrainingLoopRunner(SD15Adapter()).run(cfg, output)
├── sdxl.py              # ~40L: idem para SDXLAdapter
├── flux.py              # FluxAdapter(family="flux1"|"flux2") + FluxTrainer fino (~500-600L, física própria preservada)
├── qwen_image.py         # QwenImageAdapter + QwenImageTrainer fino (~450-550L, física própria preservada)
└── mock.py              # inalterado
```

Estimativa de redução (linhas de **glue de orquestração** eliminadas, não de lógica de modelo):

| Arquivo | Antes | Depois (estimado) | Δ |
|---|---:|---:|---:|
| `sd15.py` | 644 | ~40 | -604 |
| `sdxl.py` | 666 | ~40 | -626 |
| `sd_family/adapter.py` (novo, compartilhado) | — | ~320 | +320 |
| `models/loop.py` (novo, compartilhado) | — | ~380 | +380 |
| `flux.py` | 1.026 | ~650 | -376 |
| `qwen_image.py` | 914 | ~620 | -294 |
| **Total models/** | **3.250** | **~2.050** | **~-1.200 (-37%)** |

## 4. Fases de Execução (cada uma = branch própria, gate `@reviewer`, smoke GPU no nó TrueNAS)

### Fase A — `models/loop.py` + unificação SD15/SDXL (menor risco, maior retorno imediato)
- Criar `models/loop.py` (`LoraTrainConfig`, `parse_lora_train_config`, `ModelAdapter`, `TrainingLoopRunner`) extraindo o laço **a partir do texto atual de `sdxl.py`** (mais completo: 2 encoders, pooled/time_ids) generalizado para 1 ou 2 encoders via hook.
- Criar `models/sd_family/adapter.py` com `SD15Adapter`/`SDXLAdapter` reaproveitando `models/sd_pkg/sample.py` e `models/sd_pkg/embeddings.py` sem alteração de assinatura pública.
- Reduzir `sd15.py`/`sdxl.py` a wrappers finos (`SD15Trainer.train` → `TrainingLoopRunner(SD15Adapter()).run(cfg, output)`).
- **Aceitação:** suíte Python completa verde (363+ testes citados em `tasks/active.md`), incluindo `tests/test_train.py` (mock path) e `tests/test_sd_pkg.py`; smoke real (`ENGINE_MOCK=0`) de 1 época SD 1.5 e 1 época SDXL no nó GPU, comparando `metrics.jsonl` (mesmas chaves/fases) e checkpoint gerado antes/depois do refactor.
- Resolve #7 e #8 do `engines-auditoria-global.md` para os 2 arquivos.

### Fase B — Extensão do `TrainingLoopRunner` para `flux.py`
- `FluxAdapter(family: Literal["flux1","flux2"])` cobrindo `_encode_flux1_batch`/`_encode_flux2_batch`, packed latents (`_pack_latents`/`_pack_latents_flux2`), `_encode_qwen3_prompt`.
- Único ponto de atenção: `flux.py` hoje usa `loaders/*` (quant_cache, transformer_loader, text_encoder_loader) com callbacks `on_cached`/`on_loading`/`on_ready` para telemetria de carregamento — preservar essas callbacks dentro de `FluxAdapter.load_and_inject_lora`.
- **Aceitação:** `tests/test_generate_flux2_motor.py` verde; smoke real 1 época Flux-2-Klein no nó GPU com LoRA custom checkpoint (`custom_checkpoint_path`) e sem, comparando artefato final.
- Resolve #7/#8 para `flux.py`.

### Fase C — Extensão para `qwen_image.py` + migração #12
- `QwenImageAdapter` cobrindo VLM text encoder (BitsAndBytes 4-bit + fallback CPU), packed latents, `image_pad_mask`.
- Migrar `prompt_cache` em RAM para `TextEmbedsCache` (resolve #12 do `engines-auditoria-global.md`) como parte da adaptação ao hook `build_text_cache_encode_fn`/`text_cache_encoders` do runner — alinha ao padrão dos outros 3.
- **Aceitação:** `tests/test_qwen_image.py` verde (cobre ciclos de descarte, LoRA residual, limpeza de memória — não pode regredir); smoke real 1 época Qwen-Image-2.1 no nó GPU medindo VRAM pico (não pode regredir vs. baseline atual, per fatia `fix/qwen-image-memory-leaks`).
- Resolve #7/#8/#12 para `qwen_image.py` — fecha a Fase 2 inteira do `engines-auditoria-global.md`.

## 5. Estratégia de Teste

- Testes de unidade **não exercitam o laço real** (exige CUDA) — cobrem hoje o caminho `ENGINE_MOCK=1` via `train.py:main` (`tests/test_train.py`) e funções puras isoladas. Pós-refactor, adicionar testes de unidade **sem CUDA** para as peças puramente lógicas extraídas para `models/loop.py`:
  - `parse_lora_train_config`: defaults, overrides, validação de `custom_checkpoint_path`/`text_encoder_path` (mensagens de erro exatas preservadas).
  - Aritmética de accum-step / cálculo de `progress` / `is_accum_step` (extrair como funções puras testáveis com `epochs`, `grad_accum`, `steps_in_epoch` sintéticos, sem `torch`).
  - Cadência de emissão de métrica (cada 5 steps, `emit_interval` adaptativo por `avg_step_time >= 5.0` herdado do `qwen_image`).
- Testes existentes que hoje fazem `mock.patch` em símbolos de `sd15.py`/`sdxl.py`/`flux.py`/`qwen_image.py` (ver `tests/test_models_decoupling.py`) precisam apontar para os novos módulos — mapear 1:1 antes de mover código, não depois.
- **Gate final obrigatório (Regra de Ouro):** cada fase exige smoke real (`ENGINE_MOCK=0`) no nó GPU (`dockeruser@10.15.1.2`) antes do merge — testes unitários não provam paridade numérica em GPU real.

## 6. Riscos e Mitigação

| Risco | Mitigação |
|---|---|
| Regressão silenciosa na física do forward (loss, scaling, dtype) ao mover código entre arquivos | Corte-e-cola literal do corpo de `forward_and_loss`/`load_and_inject_lora`; diff revisado linha-a-linha pelo `@reviewer` contra o arquivo original antes do merge de cada fase |
| `mock.patch(...)` de testes quebra por mudança de path de símbolo | Levantamento de todos os `mock.patch("trainer_difusao.models.X...")` antes de mover (grep em `tests/`), atualizar no mesmo commit |
| Hook mal desenhado força vazamento de detalhe de arquitetura pro runner genérico (ex.: `added_cond_kwargs` só existe em SDXL) | `forward_and_loss` recebe `comp.extra` (dict opaco por arquitetura) e devolve só o tensor de loss — o runner nunca inspeciona `extra` |
| Divergência de telemetria ao "promover" o padrão ETA/EMA do qwen para os outros 3 | Fase A/B validam `metrics.jsonl` campo-a-campo contra baseline pré-refactor via smoke GPU, não só "não quebrou o schema" |

## 7. Fora de Escopo desta Spec

- `trainer-yolo`, `trainer-clip`, `engine-kit` — sem duplicação estrutural comparável (achado desta investigação; ver §1).
- Split interno de `flux.py` em Flux-1 vs Flux-2-Klein como arquivos separados — mesmo arquivo, mesma engine, ganho de "custo de manutenção por engine" é marginal comparado à unificação entre arquivos.
- Itens #1–#6, #9–#11, #13–#18 do `engines-auditoria-global.md` (bugs/hardening não relacionados a duplicação estrutural).
