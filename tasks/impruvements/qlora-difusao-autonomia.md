# Levantamento e Especificação Técnica: Implementação de QLoRA para Modelos de Difusão (FLUX, SDXL e SD 1.5)

Documento de mapeamento, arquitetura e checklist executivo para a implementação canônica da técnica de **QLoRA** (*Quantized Low-Rank Adaptation*) no ecossistema de difusão (`engines/trainer-difusao`) do **Hephaestus LLM Studio**.

- **Data:** 2026-09-18
- **Escopo:** Modelos de Difusão (`FLUX.2 Klein 4B / FLUX.1`, `SDXL 1.0`, `SD 1.5`), Otimizadores Paginados (`bitsandbytes`), Contratos OpenAPI, Validação no BFF Rust e Interface de Treino (`apps/web`).
- **Status:** Planejado / Pronto para execução

---

## 1. Fundamentação Teórica & Diagnóstico do Repositório

### 1.1 O que é o QLoRA Canônico (Dettmers et al., 2023)
O QLoRA baseia-se em três pilares para viabilizar o treinamento de modelos massivos com até 65% de economia de VRAM sem perda de qualidade visual:
1. **Quantização 4-bit NormalFloat (NF4) com Double Quantization:** O modelo base tem seus tensores quantizados em blocos de 64 valores na distribuição NF4, e as constantes de quantização são quantizadas em 8-bit (economizando ~0.37 bits por parâmetro).
2. **Backpropagation através de Pesos 4-bit Congelados:** Os pesos base permanecem congelados em 4-bit na VRAM e são desquantizados em tempo real (*on-the-fly*) para `compute_dtype` (`bfloat16` ou `float16`) durante o forward pass. Os gradientes fluem pelas ativações até os adaptadores LoRA (matrizes $A$ e $B$ de baixo rank).
3. **Paged Optimizers:** Uso de memória unificada CUDA para paginação dos estados do otimizador (AdamW) entre VRAM e RAM de sistema durante picos de alocação de gradientes, eliminando erros de CUDA OOM.

### 1.2 Situação Atual no Hephaestus LLM Studio

| Componente | Situação Atual | O que falta para QLoRA Canônico |
|:---|:---|:---|
| **FLUX (`flux.py`)** | Suporta `4bit` NF4 (`BitsAndBytesConfig`) com persistência em disco + LoRA PEFT. | Falta `prepare_model_for_kbit_training` e otimizadores paginados. |
| **SD 1.5 (`sd15.py`)** | Aceita parâmetro `quantization: "4bit"`, mas carrega UNet em precisão plena (`FP16`/`BF16`). | Carregar UNet via `BitsAndBytesConfig(load_in_4bit=True)`, remover `.to(device)` e aplicar `prepare_model_for_kbit_training`. |
| **SDXL (`sdxl.py`)** | Aceita parâmetro `quantization: "4bit"`, mas carrega UNet em precisão plena (`FP16`/`BF16`). | Idem ao SD 1.5. |
| **Otimizadores (`optimizers.py`)** | Suporta `adamw8bit`, `adamw`, `prodigy`. | Adicionar `paged_adamw8bit` e `paged_adamw32bit` via `bitsandbytes.optim`. Falha explícita se bnb ausente em GPU. |
| **Gerador (`generation/runner.py`)** | Já carrega LoRAs via `pipe.load_lora_weights()`. | **Nenhuma alteração necessária**. QLoRA salva apenas as matrizes LoRA padrão em `.safetensors`. |
| **Contratos & Backend Rust** | Enum do otimizador aceita `adamw8bit`, `adamw`, `prodigy`. | Adicionar `paged_adamw8bit` e `paged_adamw32bit` em `openapi.yaml` e `models.rs`. |
| **Frontend Studio (`apps/web`)** | Permite escolher `4bit NF4` e otimizadores legados. | Adicionar opções de Paged AdamW e calibrar estimativa de VRAM. |

---

## 2. Por que o Gerador / Inferência não precisa de alterações?

No QLoRA, a quantização 4-bit ocorre no modelo base **durante o treino**. Os parâmetros treináveis são **exclusivamente os adaptadores LoRA** (pesos $\Delta W = \frac{\alpha}{r} B \cdot A$).

1. Ao final do treino, a função `_save_lora_safetensors` extrai apenas os tensores com prefixo `lora_` e grava um `.safetensors` canônico de 20MB a 200MB.
2. Esse arquivo é indistinguível de um LoRA treinado em FP16 ou FP32.
3. No gerador (`runner.py`), a chamada:
   ```python
   pipe.load_lora_weights(lora_path, adapter_name=name)
   ```
   funciona perfeitamente, quer o gerador esteja rodando o modelo base em FP16/BF16, quer esteja rodando em 4-bit ou 8-bit.

---

## 3. Plano Executivo em 4 Fases

### Fase 1: Motor de Treino Difusão (`engines/trainer-difusao`)

#### 1.1 `optimizers.py`: Implementar Paged Optimizers
- Suportar `paged_adamw8bit` (`bnb.optim.PagedAdamW8bit`) e `paged_adamw32bit` (`bnb.optim.PagedAdamW32bit`).
- Normalizar variações de nomenclatura (`paged_adamw_8bit` e `paged_adamw8bit`).
- **Eliminação de fallback silencioso (Regra 3.6 / ADR-0013):** Se o usuário solicitar otimizador paginado e o `bitsandbytes` falhar ao importar em ambiente CUDA, chamar `_die()` com diagnóstico claro (proibido recuar silenciosamente para AdamW padrão).

#### 1.2 `sd15.py` e `sdxl.py`: Quantização Real do UNet
- Criar `BitsAndBytesConfig` quando `quantization` for `4bit` ou `8bit`:
  ```python
  if is_4bit:
      bnb_config = BitsAndBytesConfig(
          load_in_4bit=True,
          bnb_4bit_quant_type="nf4",
          bnb_4bit_compute_dtype=target_dtype,
          bnb_4bit_use_double_quant=True,
      )
  elif is_8bit:
      bnb_config = BitsAndBytesConfig(load_in_8bit=True)
  ```
- Repassar `quantization_config=bnb_config` para:
  - `UNet2DConditionModel.from_pretrained(..., quantization_config=bnb_config)`
  - `UNet2DConditionModel.from_single_file(..., quantization_config=bnb_config)`
- **Atenção crítica:** Remover `.to(device)` para modelos quantizados (BitsAndBytes já posiciona os tensores no dispositivo CUDA; chamar `.to()` em modelo 4/8-bit dispara exceção do PyTorch).

#### 1.3 `flux.py`, `sd15.py` e `sdxl.py`: Estabilização Numérica com `prepare_model_for_kbit_training`
- Importar `from peft import prepare_model_for_kbit_training`.
- Executar `model = prepare_model_for_kbit_training(model, use_gradient_checkpointing=True)` antes de `get_peft_model(model, lora_config)`.
- Isso garante:
  1. Cast de camadas de normalização (`RMSNorm`, `LayerNorm`, `GroupNorm`) para `torch.float32` prevenindo `NaN loss`.
  2. Ativação de hooks de gradiente nas entradas para compatibilidade entre `gradient_checkpointing` e pesos congelados.

#### 1.4 `mock.py`: Suporte nos Testes de CI
- Atualizar gerador sintético de `.safetensors` para aceitar `paged_adamw8bit` e `paged_adamw32bit`, registrando nos metadados do arquivo e garantindo que o pipeline de testes em CPU continue 100% verde.

---

### Fase 2: Contratos & Validação Backend Rust

#### 2.1 `packages/contracts/openapi.yaml`
- Atualizar enum de `optimizer` em `DiffusionTrainRequest`:
  ```yaml
  optimizer:
    type: string
    enum: [adamw8bit, adamw, prodigy, paged_adamw8bit, paged_adamw32bit]
    default: paged_adamw8bit
  ```

#### 2.2 `services/api-principal/src/jobs/models.rs`
- Expandir a lista de validação do BFF:
  ```rust
  const ALLOWED_DIFFUSION_OPTIMIZERS: &[&str] = &[
      "adamw8bit",
      "adamw",
      "prodigy",
      "paged_adamw8bit",
      "paged_adamw32bit",
  ];
  ```
- Atualizar testes de serialização e asserções unitárias em `models.rs`.

---

### Fase 3: Políticas de VRAM (`packages/policies/vram-table.yaml`)

- Ajustar as barreiras mínimas de VRAM para nós GPU aproveitando o ganho de QLoRA:
  - **SD 1.5 QLoRA:** Reduz de `6 GB` para `5 GB`.
  - **SDXL 1.0 QLoRA:** Reduz de `10 GB` para `8 GB` (viabilizando treino de SDXL em GPUs de 8GB/12GB como RTX 3060/4060).
  - **FLUX.2 Klein QLoRA:** Barreira de `8 GB` mantida com garantia de ausência de picos de OOM via `paged_adamw8bit`.

---

### Fase 4: Interface Web Studio (`apps/web`)

#### 4.1 `apps/web/types/studio.ts`
- Atualizar tipagem `DiffusionOptimizer`:
  ```typescript
  export type DiffusionOptimizer =
    | "adamw8bit"
    | "adamw"
    | "prodigy"
    | "paged_adamw8bit"
    | "paged_adamw32bit";
  ```

#### 4.2 `apps/web/components/studio/ForjaDifusaoSetup.tsx`
- Adicionar no select de otimizadores:
  - `paged_adamw8bit`: `"Paged AdamW 8-bit (BitsAndBytes — Recomendado p/ QLoRA)"`
  - `paged_adamw32bit`: `"Paged AdamW 32-bit (BitsAndBytes — Máxima precisão)"`
- Atualizar presets ("Econômico / Baixa VRAM") para selecionar automaticamente `4-bit NF4` + `paged_adamw8bit`.
- Ajustar estimador `estimateVramGb` para refletir os menores consumos com QLoRA.

---

## 4. Checklist de Execução

- [ ] **1. Otimizadores Paginados nas Engines**
  - [ ] Implementar `PagedAdamW8bit` e `PagedAdamW32bit` em `engines/trainer-difusao/src/trainer_difusao/optimizers.py`.
  - [ ] Implementar fail-fast em ambiente CUDA caso `bitsandbytes` não esteja disponível.
- [ ] **2. Quantização UNet e Preparação K-Bit**
  - [ ] Injetar `BitsAndBytesConfig` (4-bit NF4 com double quant) no carregamento de `UNet2DConditionModel` em `sd15.py` e `sdxl.py`.
  - [ ] Remover chamadas `.to(device)` para modelos quantizados.
  - [ ] Aplicar `prepare_model_for_kbit_training` em `flux.py`, `sd15.py` e `sdxl.py`.
- [ ] **3. Suporte no Mock e Testes Python**
  - [ ] Atualizar `mock.py` para suportar os novos otimizadores.
  - [ ] Adicionar testes unitários em `engines/trainer-difusao/tests/test_optimizers.py`.
- [ ] **4. Contratos e Backend Rust**
  - [ ] Atualizar `packages/contracts/openapi.yaml`.
  - [ ] Atualizar `ALLOWED_DIFFUSION_OPTIMIZERS` em `services/api-principal/src/jobs/models.rs`.
  - [ ] Ajustar testes unitários em `services/api-principal`.
- [ ] **5. Políticas de VRAM e Frontend**
  - [ ] Atualizar `packages/policies/vram-table.yaml`.
  - [ ] Atualizar `apps/web/types/studio.ts` e `ForjaDifusaoSetup.tsx`.
- [ ] **6. Validação e Homologação**
  - [ ] `cargo check --workspace && cargo test --workspace`
  - [ ] `python -m compileall engines/*/src`
  - [ ] `cd engines/trainer-difusao && uv run pytest`
  - [ ] `npm run build --workspace=web`
