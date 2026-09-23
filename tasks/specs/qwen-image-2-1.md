# Spec — Suporte ao Modelo Qwen-Image-2.1 (Treino & Geração)

Status: Planejado / Em implementação
Data: 2026-09-23
Origem: Solicitação de introdução do modelo `Qwen/Qwen-Image-2.1` para DreamBooth/LoRA e Inferência interativa no Studio.

---

## 1. Visão Geral do Modelo

- **Identificador Canônico:** `qwen-image-2.1`
- **Repositório Hugging Face:** `Qwen/Qwen-Image-2.1`
- **Arquitetura:** Diffusion Transformer (DiT) com 7B parâmetros (32 camadas Single-Stream DiT), scheduler Flow Matching (`FlowMatchEulerDiscreteScheduler`), suporte nativo a geração transparente (RGBA) e aspect ratios dinâmicos até 2048x2048.
- **Pipeline de Inferência:** `QwenImage21Pipeline` (diffusers).
- **Pipeline de Treinamento:** DreamBooth LoRA sobre o Transformer (`train_dreambooth_lora_qwen_image.py`).
- **Text Encoders:** Família Qwen2.5-VL / Qwen2.

---

## 2. Implementação por Pilares

### Pilar 1: Contracts & Policies (`packages/`)
1. **`packages/policies/vram-table.yaml`**:
   - `mode: train` → 16 GB base (10 GB com quantização 8-bit/4-bit).
   - `mode: generate` → 12 GB base (8 GB com cpu offload).
2. **`packages/contracts/openapi.yaml`**:
   - Adicionar `qwen-image-2.1` ao enum `DiffusionBaseModel` e descrições relacionadas.

### Pilar 2: Engines (`engines/trainer-difusao`)
1. **Canonical Name & Factory:**
   - Em `common.py`: reconhecer variações como `qwen-image`, `qwen-image-2.1`, `qwen2.1`, `qwen_image` mapeando para `qwen-image-2.1`.
   - Em `models/__init__.py`: registrar `QwenImageTrainer`.
2. **Trainer (`src/trainer_difusao/models/qwen_image.py`):**
   - Implementar `QwenImageTrainer(BaseModelTrainer)`.
   - `_mock_train_qwen_image`: mock determinístico em CPU pura (`ENGINE_MOCK=1`), emite métricas via `_emit_metric`, produz `.safetensors` sintético com metadados corretos.
   - `_real_train_qwen_image`: adaptação de `train_dreambooth_lora_qwen_image.py` com PEFT LoRA, integração com checkpoints e métricas do `engine-kit`.
3. **Geração (`generation/runner.py` e `serve_pkg/handler.py`):**
   - Suporte a `QwenImage21Pipeline` com fallback seguro caso diffusers não tenha a classe (mock/stub no ambiente de teste).
   - Respeito a `cpu_offload` e dimensões nativas.
4. **Testes Unitários (`tests/test_qwen_image.py`):**
   - Teste do ciclo mock de treino em CPU.
   - Teste de instanciação e validação de configurações.

### Pilar 3: Services Rust (`services/`)
1. **`services/manager`**:
   - Validar se `services/manager/src/domain/` ou schemas aceitam `qwen-image-2.1`.
   - Atualizar `derive_semantic_model_name` para reconhecer o prefixo `qwen-image`.
2. **`services/orchestrator`**:
   - Garantir compatibilidade de argumentos CLI e passagem de `--config`.

### Pilar 4: Apps Web (`apps/web`)
1. Adicionar `qwen-image-2.1` no seletor de modelos da Forja (`ForjaDifusaoSetup.tsx` ou similar) com presets de resolução (1024x1024, 2048x2048) e learning rate sugerido (2e-4).
2. Adicionar o modelo na listagem de modelos base do Playground de geração.

---

## 3. Critérios de Aceite e Verificação Canônica
- `cargo check --workspace && cargo fmt --all -- --check && cargo test --workspace` verde.
- `python -m compileall engines/*/src` verde.
- `cd engines/trainer-difusao && uv run pytest` verde.
- `npm run lint --workspace=web && npm run build --workspace=web` verde.
- `docker compose -f infra/compose.yaml config -q` verde.
