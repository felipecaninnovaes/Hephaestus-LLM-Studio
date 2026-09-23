# Guia de Introdução de Novos Modelos

Este documento estabelece o protocolo canônico e o checklist transversal para adicionar suporte a novos modelos no Hephaestus LLM Studio (modelos de difusão, detecção YOLO ou visão/embeddings).

Seguir a ordem sequencial dos pilares garante zero quebras em contratos, CI determinístico e isolamento seguro de recursos de GPU/VRAM.

---

## 1. Pilar Contracts & Policies (`packages/`)
*Fonte única da verdade antes de qualquer alteração de código.*

1. **`packages/policies/vram-table.yaml`:**
   - Adicionar os requisitos mínimos de VRAM para o modelo nos modos suportados (`train`, `generate`, `predict`):
     ```yaml
     entries:
       - { engine: diffusion, model: novo-modelo-4b, mode: train, vram_min_gb: 8 }
       - { engine: diffusion, model: novo-modelo-4b, mode: generate, vram_min_gb: 8 }
     ```
   - O agendador de VRAM do `manager` consome essa tabela diretamente.
2. **`packages/contracts/openapi.yaml`:**
   - Adicionar o identificador canônico do modelo nos enums e schemas da API:
     - `DiffusionBaseModel` (para difusão) ou `YoloBaseModel` (para detecção).
     - Exemplos: `flux-2-klein-4b`, `sdxl-1.0`, `sd15`, `yolo11n`.

---

## 2. Pilar Engines (`engines/`)
*Implementação do treinamento, inferência e desacoplamento.*

### A. Para Modelos de Difusão (`engines/trainer-difusao`)
1. **Pipeline de Treino (`src/trainer_difusao/models/<modelo>.py`):**
   - Implementar o trainer herdando de `BaseModelTrainer` ou função `_real_train_<modelo>`.
   - **Garantia de Mock (`ENGINE_MOCK=1`):** A execução simulada (`_mock_train_*`) deve rodar em CPU pura, emitir métricas via `_emit_metric` e salvar `.safetensors` sintético sem inicializar CUDA ou carregar pesos pesados.
   - **Cache de Text Embeddings:** Integrar com `TextEmbedsCache` e `_precompute_text_cache_with_cleanup`.
   - **Pré-computação de Amostra Modular (ADR-0021):**
     - Criar ou reutilizar helper em `common_pkg/text_embeds.py` (ex.: `_precompute_sample_embeds_<modelo>`).
     - Pré-computar os tensores de prompt da amostra na Época 0 antes do offload de VRAM.
     - Passar `sample_embeds` para o gerador de amostras, permitindo que a amostragem instancie o pipeline Diffusers com `text_encoder=None` sem alocar encoders na GPU durante o treino.
   - **Descarregamento de Text Encoders:** Honrar `ENABLE_TEXT_ENCODER_UNLOAD` e a trava de retomada (`epoch_offset == 0`).
   - **Salvamento de Pesos:** Usar `_save_lora_safetensors` garantindo metadados corretos (`base_model`, `lora_rank`, `lora_alpha`, `trigger_word`) e sem prefixos PEFT corrompidos nas chaves.
2. **Pipeline de Inferência / Daemon (`generation/` e `serve_pkg/`):**
   - Registrar a classe Diffusers apropriada em `generation/pipelines.py` (ex.: `FluxPipeline`, `StableDiffusionXLPipeline`).
   - Adicionar o mapeamento em `generation/runner.py` e no handler do daemon residente em `serve_pkg/handler.py` caso o modelo participe da aba interativa de Geração.
3. **Testes Unitários:**
   - Adicionar cobertura em `tests/` cobrindo validação de config, modo mock determinístico e integridade de tensores.

### B. Para Modelos YOLO (`engines/trainer-yolo`)
1. Adicionar o mapeamento de pesos e arquitetura em `yolo_adapter.py` e `config.py`.
2. Assegurar emissão de telemetria compatível com `engine-kit` (`metrics.jsonl` e `telemetry.jsonl`).

---

## 3. Pilar Services (`services/` — Rust)
*BFF, gerenciamento de estado e orquestração de nós.*

1. **`services/manager`:**
   - **Validação de Job:** Verificar se `services/manager/src/domain/` ou schemas de validação aceitam o novo identificador.
   - **Nome Semântico Automático (ADR-0022):** Garantir que a geração automática de nomes de modelo (`derive_semantic_model_name`) lida adequadamente com o novo slug base.
2. **`services/orchestrator`:**
   - **Subcomandos CLI (`app/stages/execute.rs`):** Validar se `resolve_subcommand_args` encaminha corretamente `--config` e `--output` para `(engine, mode)`.
   - **Ambiente e Volumes (`app/mod.rs` e `main.rs`):** Verificar se caches de modelo necessários (`HF_HOME`, `TORCH_HOME`) estão configurados e montados nos volumes de persistência (`/outputs/.cache` ou `/data/models`).
3. **`services/api-principal`:**
   - Garantir que as rotas de proxy no BFF encaminham o payload do novo modelo sem rejeição de validação local.

---

## 4. Pilar Apps (`apps/web` — Next.js 16)
*Interface com o usuário do Studio.*

1. **Formulários de Setup (Forja):**
   - Atualizar componentes de configuração de treino (ex: `apps/web/src/components/forja/ForjaDifusaoSetup.tsx` ou similar).
   - Definir presets adequados para o novo modelo:
     - Resolução nativa recomendada (ex: 512x512 para SD 1.5, 1024x1024 para SDXL/FLUX).
     - Opções de quantização permitidas (4-bit NF4, 8-bit, FP16/BF16).
     - Ranks de LoRA sugeridos (ex: 8 a 64).
2. **Playground & Galeria:**
   - Adicionar o modelo aos seletores e filtros compatíveis na aba de geração.

---

## 5. Pilar Infra (`infra/`)
*Ambientes de execução e conteinerização.*

1. **Dependências Python:**
   - Se o modelo exigir pacotes externos adicionais (ex.: transformers específicos, bibliotecas de tokenização ou kernels customizados), declarar em `engines/<engine>/pyproject.toml`.
   - Recompilar ou validar o `Dockerfile` e `Dockerfile.gpu` da engine.
2. **Variáveis de Ambiente (`infra/env.gpu.example`):**
   - Documentar tokens necessários (ex.: `HF_TOKEN` para modelos com licença gated) ou identificadores de repositório (ex.: `FLUX_MODEL_ID`).

---

## 6. Checklist de Verificação Canônica

Antes de abrir PR ou fechar commit da fatia de introdução do modelo, executar:

```bash
# 1. Validação de contratos e compilação Rust
cargo check --workspace && cargo fmt --all -- --check && cargo test --workspace

# 2. Compilação e testes da engine correspondente
cd engines/<engine> && uv run pytest
python -m compileall engines/*/src

# 3. Validação dos arquivos Compose
docker compose -f infra/compose.yaml config -q
docker compose -f infra/compose.gpu.yaml --env-file infra/env.gpu.example config -q

# 4. Integridade de lint/build web (se UI alterada)
npm run lint --workspace=web && npm run build --workspace=web
```
