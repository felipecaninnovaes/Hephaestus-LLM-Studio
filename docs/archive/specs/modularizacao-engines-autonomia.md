# Levantamento de Arquitetura: Modularização das Engines e Eliminação de Duplicações - Rumo à Autonomia 24/7

Documento de mapeamento, auditoria estática e plano executivo de **modularização**, **desacoplamento**, **redução de arquivos gigantes (>400-1.700 linhas)** e **eliminação de duplicações de código** nas engines de Inteligência Artificial do **Hephaestus LLM Studio** (`engines/trainer-difusao`, `engines/trainer-yolo` e `engines/trainer-clip`).

- **Data do levantamento:** 2026-09-18.
- **Escopo:** Exclusivamente as engines Python em `engines/` e seus contratos com o Orchestrator Rust (zero alterações ou análises de Frontend/Web).
- **Objetivo:** Tornar as engines fáceis de manter, modulares (<250-300 linhas por arquivo), sem dependências cruzadas frágeis, com reutilização limpa de primitives e preparadas para operação autônoma contínua.

---

## 1. Diagnóstico Geral e Métricas do Código Atual

As 3 engines somam atualmente **5.909 linhas** de código de implementação (`src/`) e **9.929 linhas** em modelos e testes (`tests/` e `models/`), totalizando quase **16.000 linhas de Python**.

O código sofre do fenômeno de **"God Modules"** e **"Copy-Paste Architecture"**: lógicas complexas foram sendo adicionadas sequencialmente em arquivos monolíticos, gerando arquivos de difícil leitura, misturando responsabilidades de rede, I/O de disco, manipulação tensorial, formatação de metadados e algoritmos de mock.

### 1.1 Tabela de Linhas de Código (LOC) por Arquivo

| Arquivo | LOC | Responsabilidades Misturadas / Diagnóstico | Severidade |
|:---|:---:|:---|:---:|
| `engines/trainer-difusao/.../generate.py` | **1.709** | Validação CLI, montagem Diffusers, 2 geradores de JSONL, text encoder override, LoRA PEFT, upscale, mock Pillow | **Crítico** |
| `engines/trainer-difusao/.../models/flux.py` | **1.586** | RoPE, patchify, Qwen3 encoding, quantização, e uma única função de treino de **1.240 linhas** (`_real_train_flux`) | **Crítico** |
| `engines/trainer-difusao/.../common.py` | **760** | 8 clusters desconexos: helpers de mock, telemetria manual, I/O safetensors, cache HF, merge de text-encoders | **Alto** |
| `engines/trainer-difusao/.../models/sdxl.py` | **657** | Monolito de treino SDXL (~515L), loop de treino ~85% idêntico ao de SD1.5 | **Médio** |
| `engines/trainer-difusao/.../models/sd15.py` | **593** | Monolito de treino SD1.5 (~495L), loop de treino quase idêntico ao de SDXL | **Médio** |
| `engines/trainer-yolo/.../autolabel.py` | **506** | Parser de config, VLM/OpenAI API HTTP client, 3 geradores de caption mock, pipeline de anotação | **Alto** |
| `engines/trainer-difusao/.../serve.py` | **463** | HTTP Server, estado global mutável, reload de pipeline, lógica de eviction, conversão de SystemExit em 400 | **Alto** |
| `engines/trainer-yolo/.../train.py` | **413** | God module YOLO: exporta `_die`, `_seed_bytes`, `METRIC_KEYS`, CLI router, fixer de dataset.yaml | **Médio** |
| `engines/trainer-yolo/.../autotrack.py` | **396** | Leitor de dataset.yaml, gerador de boxes determinístico, acoplamento direto com Ultralytics | **Médio** |
| `engines/trainer-difusao/.../upscale.py` | **334** | Remap ESRGAN->BasicSR, RRDBNet inline, tiling, upscale de imagem | **Baixo** |
| `engines/trainer-clip/.../serve.py` | **221** | Servidor HTTP de embeddings, mock hash determinístico, OpenCLIP ViT-B-32 | **Médio** |
| `engines/trainer-yolo/.../predict.py` | **205** | Extração de bounding boxes do Ultralytics idêntica à do autotrack | **Médio** |
| `engines/trainer-difusao/.../telemetry.py` | **121** | `TelemetryEmitter` (ADR-0021) — **100% IDÊNTICO byte-a-byte ao do YOLO** | **Crítico (Duplicação)** |
| `engines/trainer-yolo/.../telemetry.py` | **121** | `TelemetryEmitter` (ADR-0021) — **100% IDÊNTICO byte-a-byte ao da Difusão** | **Crítico (Duplicação)** |

---

## 2. Mapa de Duplicações de Código e Drift Comprovado

A auditoria identificou **15 clusters de duplicação** entre engines e dentro da mesma engine. Alguns já causaram divergências reais de comportamento (**drift**):

### 2.1 Duplicações Cross-Engine (Inter-Engines)

1. **`telemetry.py` (100% Idêntico - 121 linhas):**
   - `engines/trainer-difusao/src/trainer_difusao/telemetry.py` e `engines/trainer-yolo/src/trainer_yolo/telemetry.py` são **rigorosamente idênticos** (classe `TelemetryEmitter`, ADR-0021, escrita atômica em `telemetry.jsonl` e espelhamento em `metrics.jsonl`).
   - Qualquer evolução ou correção no contrato de telemetria precisa ser feita manualmente duas vezes.

2. **Cadeia SHA-256 de Expansão Determinística (`_seed_bytes` / `mock_vector`):**
   - `trainer-difusao/common.py:21-28` e `trainer-yolo/train.py:43-50`: código de expansão de seed pseudo-aleatória determinística para simulação em CPU.
   - `trainer-clip/serve.py:28-37` (`mock_vector`): terceira implementação do mesmo conceito para gerar vetores de embedding determinísticos normalizados.

3. **Helper de Terminação e Falha Honesta (`_die()`):**
   - Implementado em 6 locais diferentes:
     - `trainer-difusao/common.py:16`
     - `trainer-difusao/generate.py:27`
     - `trainer-difusao/quantization.py:15`
     - `trainer-difusao/upscale.py:54`
     - `trainer-yolo/train.py:349`
     - `trainer-yolo/autolabel.py` (importa de `train.py`, criando um acoplamento invertido desnecessário).

4. **Boilerplate de Servidor HTTP Daemon (`serve.py`):**
   - `trainer_difusao/serve.py` e `trainer_clip/serve.py` utilizam `ThreadingHTTPServer` e `BaseHTTPRequestHandler`, reimplementando `_send(code, obj)`, serialização de JSON, extração de headers e leitura de `Content-Length`.
   - **Gap Real de Autonomia:** `trainer_difusao` possui handler de `SIGTERM` e encerramento gracioso via thread; `trainer_clip` não possui **nenhum tratamento de sinal**, sendo encerrado abruptamente pelo Docker.

5. **Leitura de VRAM com BUG de Drift Comprovado:**
   - `trainer-difusao/telemetry.py:48-54` e `trainer-yolo/telemetry.py:48-54`: usam `torch.cuda.memory_allocated() / 1024**3`.
   - `trainer-difusao/common.py:116-123`: usa `1024**3`.
   - `trainer-difusao/serve.py:131`: **BUG!** Utiliza `1023**3` em vez de `1024**3`, gerando leitura inflada em ~0,1% e inconsistência com o restante do sistema.

6. **Semântica Divergente de `ENGINE_MOCK`:**
   - 7 pontos de parsing no código:
     - Estrito `os.environ.get("ENGINE_MOCK") == "1"`: `difusao/serve.py:33`, `difusao/generate.py:1705`, `yolo/train.py`, `yolo/predict.py`, `yolo/autotrack.py`, `clip/serve.py:25`.
     - Tolerante `os.environ.get("ENGINE_MOCK", "").lower() in ("1", "true", "yes")`: `difusao/train.py:87` e `:103`.
   - **Risco 24/7:** Se `ENGINE_MOCK=true` for passado em um container, o `train.py` executa em modo mock, enquanto `serve.py` tenta subir os modelos reais na GPU!

7. **Bloco de Finalização de Treino e Checkpoints Triplicado:**
   - O fluxo de salvar `adapter.safetensors`, gerar metadados, criar cópia definitiva, emitir métrica de conclusão e salvar checkpoints por época está duplicado em:
     - `engines/trainer-difusao/src/trainer_difusao/models/flux.py:1554-1569`
     - `engines/trainer-difusao/src/trainer_difusao/models/sd15.py:560-576`
     - `engines/trainer-difusao/src/trainer_difusao/models/sdxl.py:624-640`

### 2.2 Duplicações Intra-Engine (Dentro de Cada Engine)

1. **`trainer-difusao/generate.py`:**
   - Escrita de `generation_meta.json` + criação de symlink legado duplicada verbatim entre o caminho mock (linhas 774-796) e o caminho real (linhas 1653-1675).
   - Upscale pós-geração e regravação do PNG com metadados `pnginfo` duplicados (linhas 736-755 vs 1553-1570).
   - Injeção e carga de pesos multi-LoRA repetida com pequenas variações para Flux2 e SD.

2. **`trainer-yolo`:**
   - 4 validadores de configuração YAML (`train.py`, `autolabel.py`, `autotrack.py`, `predict.py`) que fazem rigorosamente os mesmos checks estruturais (~110 linhas duplicadas).
   - Extração de bounding boxes de resultados do Ultralytics (`r.boxes.xywhn / cls / conf -> topleft clamped -> dict round(4)`) duplicada verbatim entre `autotrack.py` (linhas 325-355) e `predict.py` (linhas 135-165) (~30 linhas x2).
   - 3 geradores de caption mock em `autolabel.py` (`_generate_caption_mock`, `_generate_caption_florence`, `_generate_caption_qwen`) são 90% idênticos.
   - 3 rotinas ad-hoc de gravação de `metrics.jsonl` no YOLO, enquanto a classe canônica `TelemetryEmitter` (ADR-0021) está presente na pasta mas **não é usada por nenhum submódulo do YOLO**.

---

## 3. Diagnóstico dos Arquivos Gigantes e God Modules

### 3.1 `engines/trainer-difusao/src/trainer_difusao/generate.py` (1.709 linhas)

O maior arquivo do monorepo agrega **8 responsabilidades distintas**:
1. **Validação de Configuração (L49-305):** 255 linhas validando 22 campos com chamadas a `_die()`. O daemon de inferência (`serve.py`) é obrigado a interceptar `SystemExit` e converter em HTTP 400.
2. **Resolução de LoRA e Cache Key (L308-366):** Compatibilidade com campos legados e cálculo de hash de pipeline.
3. **Metadados PNG iTXt (L368-457):** Injeção de metadados em headers binários de PNG para rastreabilidade de geração.
4. **Telemetria de Passos do Sampler (L459-560):** Callbacks granulares de progresso a cada passo de denoising.
5. **Geração Mock com Desenho Pillow (L562-786):** 225 linhas gerando imagens sintéticas com círculos, gradientes e texto para CI e desenvolvimento local.
6. **Override de Text Encoder para FLUX.2 (L788-983):** 250 linhas cuidando de fusão de checkpoints loose e cache de encoders.
7. **Motor de Geração Real `_real_generate` (L985-1600):** 615 linhas executando quantização (bnb/torchao), dispatch por arquitetura, multi-LoRA, img2img, troca dinâmica de schedulers e loop de batch.
8. **CLI Router (L1600-1709):** Ponto de entrada de linha de comando.

### 3.2 `engines/trainer-difusao/src/trainer_difusao/models/flux.py` (1.586 linhas)

O problema deste arquivo não é a quantidade de classes, mas sim o fato de conter uma **única função monstro de 1.240 linhas** (`_real_train_flux`, L341-1580):
- Mistura cálculo tensorial puro (RoPE, patchify) com carregamento de checkpoints pesados, injeção de adaptadores LoRA PEFT, montagem de otimizadores e o loop de épocas.
- As funções de RoPE (`_pack_latents`, `_patchify_latents_flux2`, `_prepare_*_ids`) não dependem de PyTorch pesado ou Diffusers e poderiam ser testadas isoladamente sem GPU.

### 3.3 `engines/trainer-difusao/src/trainer_difusao/common.py` (760 linhas)

Um típico "God Module" contendo 8 clusters independentes:
- Helpers de mock (`_seed_bytes`, `_synthetic_loss`).
- Emissão de métricas manuais (`_emit_metric`), que compete com `telemetry.py`.
- Gerenciamento de infraestrutura local (`_setup_cache_dir`, `_cleanup_cuda`, `_prune_checkpoints`).
- Persistência e carga atômica de pesos LoRA.
- Validações de parâmetros de treino.
- `TextEmbedsCache` e funções de pré-computação de embeddings de texto (160 linhas).
- Validação de configurações TorchAO.
- Cache de merge de text-encoders em disco (11 funções, 220 linhas).

### 3.4 `engines/trainer-yolo/src/trainer_yolo/train.py` (413 linhas)

Atua como ponto de acoplamento central de todo o pacote YOLO:
- Exporta símbolos privados (`_die`, `METRIC_KEYS`, `_seed_bytes`) que são consumidos por `autolabel.py`, `autotrack.py` e `predict.py`.
- O acoplamento com a biblioteca `ultralytics` é feito com imports no corpo de funções sem uma camada de adapter, obrigando testes unitários a aplicarem monkeypatches frágeis em `sys.modules`.

---

## 4. Arquitetura Proposta: Camada Compartilhada `engine-kit`

Para eliminar as duplicações de infraestrutura e garantir conformidade com os contratos do Orchestrator sem violar a regra de independência de cada engine, propõe-se a criação de uma biblioteca interna compartilhada: **`engines/engine-kit/`**.

### 4.1 Princípios Inegociáveis do `engine-kit`

1. **Stdlib-Only no Core:** O pacote não terá dependências externas obrigatórias (`dependencies = []`).
2. **Imports Opcionais e Tardios (Lazy):** Módulos que interagem com CUDA ou VRAM fazem `import torch` apenas dentro de blocos `try/except`.
3. **Isolamento de Builds e Locks Mantido:** **Não** será utilizado workspace raiz do `uv`. Cada engine continuará possuindo seu próprio `pyproject.toml` e `uv.lock`, consumindo o `engine-kit` via path dependency:
   ```toml
   # Em engines/trainer-difusao/pyproject.toml, trainer-yolo e trainer-clip:
   [dependencies]
   "hephaestus-engine-kit",
   ...

   [tool.uv.sources]
   hephaestus-engine-kit = { path = "../engine-kit", editable = true }
   ```
4. **Compatibilidade de Dockerfiles:** Nos builds de containers, o diretório `engines/engine-kit` será copiado antes da instalação das engines, garantindo cache rápido.

### 4.2 Estrutura de Pastas do `engine-kit`

```text
engines/engine-kit/
├── pyproject.toml              # name = "hephaestus-engine-kit", deps: []
├── tests/
│   ├── test_telemetry.py       # Validação estrita do contrato ADR-0021
│   ├── test_mock.py            # Validação determinística de seeds e vetores
│   ├── test_httpd.py           # Validação do servidor daemon e sinais
│   └── test_runtime.py         # Testes de atomic write e signals
└── src/
    └── engine_kit/
        ├── __init__.py
        ├── telemetry.py        # Única fonte do TelemetryEmitter (ADR-0021)
        ├── mock.py             # is_mock(), seed_bytes(), mock_vector(), synthetic_metrics()
        ├── runtime.py          # die(), atomic_write(), cancel_sentinel()
        ├── vram.py             # vram_used_gb() (divisor 1024³ correto), cleanup_cuda()
        ├── httpd.py            # JSONHandlerMixin, serve_daemon(), graceful SIGTERM
        └── artifacts.py        # prune_checkpoints(), atomic_save_safetensors()
```

---

## 5. Plano de Decomposição Modular de `trainer-difusao`

Nenhum arquivo no pacote `trainer-difusao` deverá ultrapassar **250 a 300 linhas**. Todas as refatorações utilizarão **Facades de Re-export** para garantir **100% de retrocompatibilidade** com os testes existentes.

### 5.1 Decomposição de `generate.py` (1.709 L -> Pacote `generation/`)

```text
engines/trainer-difusao/src/trainer_difusao/
├── generate.py                 # FACADE: Re-exporta os símbolos públicos e funções da CLI
└── generation/
    ├── __init__.py
    ├── config.py               # (~250L) load_and_validate_generate_config
    ├── artifacts.py            # (~150L) _write_thumb, _png_info, escrita de metadados JSONL
    ├── progress.py             # (~120L) callbacks do sampler e telemetria de denoising
    ├── mock_render.py          # (~180L) desenho determinístico Pillow (círculos, cards)
    ├── mock.py                 # (~120L) _mock_generate
    ├── pipelines.py            # (~230L) cache de pipeline unificado, ensure_pipeline
    ├── lora.py                 # (~120L) resolução e injeção de pesos multi-LoRA
    ├── text_encoder.py         # (~250L) merge e override de text encoder para FLUX.2
    └── runner.py               # (~250L) _real_generate: batch loop e img2img
```

### 5.2 Decomposição de `models/flux.py` (1.586 L -> Pacote `models/flux/`)

```text
engines/trainer-difusao/src/trainer_difusao/models/
├── flux.py                     # FACADE: Re-exporta símbolos para train.py e testes
└── flux/
    ├── __init__.py
    ├── rope.py                 # (~130L) tensores puros: pack_latents, patchify, rope ids
    ├── encoding.py             # (~150L) encode_qwen3_prompt, closures de batch
    ├── quant_cache.py          # (~200L) cache de quantização BNB / TorchAO
    ├── components.py           # (~250L) montagem de transformer, text encoders e VAE
    ├── sample.py               # (~90L) geração de amostras de validação
    ├── train_loop.py           # (~280L) loop de épocas, otimizadores e backward
    └── persistence.py          # (~100L) salvamento de checkpoints e adaptadores
```

### 5.3 Decomposição de `common.py` (760 L -> Pacote `common_pkg/`)

```text
engines/trainer-difusao/src/trainer_difusao/
├── common.py                   # FACADE: Mantém compatibilidade com todos os imports
└── common_pkg/
    ├── __init__.py
    ├── metrics.py              # (~100L) ponte para engine_kit.telemetry
    ├── runtime.py              # (~90L) setup_cache_dir, env vars do HuggingFace
    ├── lora_io.py              # (~70L) save_lora_safetensors, load_lora_weights
    ├── train_config.py         # (~130L) validações auxiliares de treino e batches
    ├── text_embeds.py          # (~170L) TextEmbedsCache e pré-computação
    └── encoder_merge.py        # (~220L) 11 funções do cache de merge de encoders
```

### 5.4 Decomposição de `serve.py` (463 L -> Pacote `serve_pkg/`)

- Extrair classe `DaemonState` (dataclass com lock e lifecycle) eliminando variáveis globais soltas de módulo (`_busy`, `_loaded_spec`, `_pipeline_cache`, `_server`).
- **Unificação Crítica:** Eliminar a divergência entre `_spec_key` de `serve.py` e `pipeline_cache_key` de `generate.py`, adotando uma única chave canônica de especificação.

---

## 6. Plano de Decomposição Modular de `trainer-yolo` e `trainer-clip`

### 6.1 Modularização de `trainer-yolo`

1. **Camada de Infraestrutura e Adaptação:**
   - `config.py` (~80L): Centralizar o parser genérico de YAML para as 4 ferramentas (`train`, `autolabel`, `autotrack`, `predict`).
   - `deterministic.py` (~100L): Extrair funções de simulação mock e boxes determinísticas (`_box_for_image`, `_xywhn_to_topleft_clamped`).
   - `dataset.py` (~110L): Unificar descoberta de imagens e preparação do `dataset.yaml`.
   - `metrics_io.py` (~80L): Adotar diretamente `engine_kit.telemetry.TelemetryEmitter`, eliminando as rotinas manuais discrepantes.
   - `yolo_adapter.py` (~90L): Isolar a dependência do `ultralytics` em um adapter fino. Permitir injeção de mock limpo em testes sem hacks em `sys.modules`.

2. **Modularização de `autolabel.py` (506 L -> Pacote `autolabel/`):**
   - `captions.py` (~70L): Unificar os 3 geradores de caption mock em uma única função parametrizada por tabela de estilo.
   - `vision_api.py` (~140L): Cliente HTTP OpenAI Vision com tratamento robusto de SSL, retry e payloads.
   - `pipeline.py` (~160L): Orquestrador da anotação de imagens e gravação de resultados.
   - `autolabel.py`: Facade mantendo a interface CLI e retrocompatibilidade.

### 6.2 Modularização de `trainer-clip`

1. **Separação de Responsabilidades:**
   - `mock_embed.py` (~35L): Gerador de embeddings mock usando `engine_kit.mock.mock_vector`.
   - `clip_backend.py` (~90L): Protocolo tipado `Embedder` com implementações `MockEmbedder` e `OpenClipEmbedder`.
   - `server.py` (~110L): Servidor HTTP usando `engine_kit.httpd` com graceful shutdown via `SIGTERM`.
2. **Criação de Testes Unitários:**
   - Atualmente, `trainer-clip` possui **zero testes** no monorepo.
   - Adicionar testes de contrato em porta efêmera executando em modo mock (compatíveis com o CI sem GPU).

---

## 7. Mapeamento de Riscos e Superfície de Compatibilidade

Para que a modularização seja realizada com **risco zero de regressão**, foram identificadas as amarras existentes nos testes e serviços:

1. **Patches Dinâmicos em Testes (`mock.patch`):**
   - Em `engines/trainer-difusao/tests/test_generate_flux2_motor.py`, há chamadas como:
     `mock.patch.object(_common, '_load_loose_text_encoder_state')`
   - O código refatorado deve continuar permitindo import tardio via módulo nas facades, garantindo que os patches dos testes existentes continuem funcionando perfeitamente.
2. **Símbolos Exportados em `train.py`:**
   - O arquivo `train.py` da difusão possui um `__all__` extenso com 29 símbolos. Todos devem ser mantidos nas facades até que os testes sejam migrados.
3. **Contratos Estritos com o Orchestrator Rust:**
   - Em `services/orchestrator/src/lib.rs:2460-2465`, o orquestrador espera arquivos específicos por engine (`telemetry.jsonl`, `metrics.jsonl`, `boxes.json`, `captions.jsonl`, `adapter.safetensors`, `best.pt`).
   - Os schemas de métricas e os nomes de artefatos são contratos congelados e não podem sofrer qualquer alteração durante as refatorações.
4. **Vetor Mock do CLIP:**
   - O cálculo de `mock_vector` é rigorosamente espelhado no código Rust do BFF (`MockEmbedder` em `src/search/embed.rs`). A matemática de hashing SHA-256 não pode ser alterada em nenhum bit.

---

## 8. Cronograma de Execução em Fases Independentes

A execução deve ser dividida em **5 fases atômicas**, onde cada fase é testada e aprovada pelo CI antes do início da próxima:

```text
┌────────────────────────────────────────────────────────────────────────────────┐
│ FASE 1: Extração da Camada Compartilhada engine-kit (Stdlib-Only)             │
│ - Criar engines/engine-kit com pyproject.toml próprio                          │
│ - Implementar telemetry.py, mock.py, runtime.py, vram.py, httpd.py             │
│ - Criar suite de testes unitários em engines/engine-kit/tests/                │
│ - Validar via pytest isolado (sem tocar em nenhuma engine existente)          │
└──────────────────────────────────────┬─────────────────────────────────────────┘
                                       │
┌──────────────────────────────────────▼─────────────────────────────────────────┐
│ FASE 2: Adoção do engine-kit nas 3 Engines & Correção de Drift                 │
│ - Adicionar hephaestus-engine-kit como path dependency nos 3 pyproject.toml   │
│ - Atualizar Dockerfiles para incluir cópia de engine-kit                       │
│ - Substituir telemetry.py duplicados pela versão canônica do engine-kit        │
│ - Corrigir o bug do divisor 1023³ em difusao/serve.py                          │
│ - Unificar parsing de ENGINE_MOCK via engine_kit.mock.is_mock()               │
│ - Rodar testes de regressão de todas as engines                               │
└──────────────────────────────────────┬─────────────────────────────────────────┘
                                       │
┌──────────────────────────────────────▼─────────────────────────────────────────┐
│ FASE 3: Modularização de trainer-difusao (Quebra dos God Modules)              │
│ - Desmembrar generate.py no pacote generation/ mantendo facade                │
│ - Desmembrar common.py no pacote common_pkg/ mantendo facade                  │
│ - Desmembrar models/flux.py no pacote models/flux/ mantendo facade            │
│ - Refatorar serve.py com DaemonState e chave de spec unificada                │
│ - Deduplicar blocos de save final de adaptadores e checkpoints                │
│ - Validar suite de testes da difusão (177 testes passando)                    │
└──────────────────────────────────────┬─────────────────────────────────────────┘
                                       │
┌──────────────────────────────────────▼─────────────────────────────────────────┐
│ FASE 4: Modularização de trainer-yolo e Desacoplamento Ultralytics             │
│ - Extrair config.py, deterministic.py, dataset.py e yolo_adapter.py            │
│ - Desmembrar autolabel.py no pacote autolabel/                                │
│ - Migrar geradores manuais de métricas para o TelemetryEmitter                │
│ - Eliminar duplicações de extração de boxes entre autotrack e predict         │
│ - Validar suite de testes do YOLO (107 testes passando)                       │
└──────────────────────────────────────┬─────────────────────────────────────────┘
                                       │
┌──────────────────────────────────────▼─────────────────────────────────────────┐
│ FASE 5: Modernização de trainer-clip e Cobertura de Testes                     │
│ - Desmembrar serve.py em mock_embed.py, clip_backend.py e server.py           │
│ - Adicionar graceful shutdown de SIGTERM herdado de engine_kit.httpd          │
│ - Criar suite inicial de testes para trainer-clip em modo mock                │
│ - Execução completa do CI monorepo (cargo check + npm run build + pytest)     │
└────────────────────────────────────────────────────────────────────────────────┘
```

---

## 9. Resumo do Impacto

Ao concluir este plano:
1. **Zero duplicata de código de telemetria e infraestrutura:** `telemetry.py`, `_die()`, leitura de VRAM, helpers de mock e servidores HTTP unificados.
2. **Nenhum arquivo monolítico:** Todos os arquivos de implementação ficarão abaixo de ~250-300 linhas, focados em uma única responsabilidade.
3. **Drift e bugs silenciosos eliminados:** Corrigidos o divisor de VRAM (`1023**3`), as inconsistências de `ENGINE_MOCK` e a falta de encerramento gracioso no CLIP.
4. **Testabilidade pura:** Lógicas determinísticas e de pré-processamento poderão ser testadas em CPU sem carregar bibliotecas de ML pesadas.
5. **Autonomia 24/7:** Motores estáveis, previsíveis e fáceis de depurar em produção.
