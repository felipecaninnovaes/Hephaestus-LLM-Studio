# Backlog & Dívidas Técnicas — Hephaestus LLM Studio

Consolidação única de pendências e melhorias prioritárias. Rotas canônicas em `packages/contracts/openapi.yaml`, limites de VRAM em `packages/policies/vram-table.yaml`.

---

## 1. Motores & Treino (Prioridade Alta)

- **QLoRA Difusão Canônico:** Implementar técnica canônica (FLUX, SDXL, SD 1.5) com quantização 4-bit na UNet/Transformer, Paged Optimizers (`paged_adamw8bit`, `paged_adamw32bit`) e gradient checkpointing nativo diffusers. Spec: `tasks/specs/qlora-difusao.md`.
- **Validação @gpu Difusão & Img2Img:** Validar carregamento com pesos reais (`ENGINE_MOCK=0`) para SDXL/Flux-2-Klein, multi-LoRA PEFT e geração sequencial em daemon quente (ADR-0023).
- **AutoLabel v2:** Evoluir motor para modelos VLM reais (Florence-2 / Qwen-VL) com aceleração GPU (v1 atual é determinística mock).

---

## 2. Orquestração & Backend (Prioridade Média-Alta)

- **Pipelines & Abort Races:**
  - [x] Tratar abort durante `preparing` e `dispatched` para evitar término incorreto em `failed` em vez de `cancelled` (Quitado: Wave 2 `RD-021`).
  - Ancorar watchdog de `preparing` no `updated_at` (heartbeat do worker) para builds longos (>60min).
  - Spec: `tasks/specs/backend-autonomia.md`.
- **Modularização e Autonomia do Orchestrator (P0–P3):**
  - Decomposição do monólito `lib.rs` (7.9k LOC) em Clean Architecture (`domain/`, `ports/`, `adapters/`, `app/`, `server/`, `daemon/`, `testkit/`).
  - Autonomia do nó: spool outbox local durável para reports, heartbeat com backoff/jitter, reaper periódico de containers órfãos e GC por watermark.
  - [x] Eliminação de duplicidades de DTOs espelho entre orchestrator/manager via `heph-contracts` (Quitado: Wave 1 `RD-010`).
  - [x] Despacho íntegro de `control_package_ref` no `dispatch_next` (Quitado: Wave 2 `RD-020`).
  - Spec: `tasks/specs/orchestrator-modularization.md`.
- **Packaging & Reuso:**
  - Assinar `build_package_diffusion` com `fingerprint` para permitir reuso de pacotes em treinos de difusão.
  - Marcar `dataset_versions` como `complete` apenas pós-upload confirmado para evitar reuso de versões órfãs.
- **Roteamento Dinâmico por VRAM:**
  - Implementar fila por VRAM livre dinâmica no manager (suporte a 2+ jobs por nó, preempção de runner, `max_parallel_trainers`).
- **Segurança & Credenciais de Nós:**
  - Adicionar credenciais dedicadas por nó (`heph_o_*`), rotação de tokens e rate-limit de pareamento.

---

## 3. Storage, Dados & Curadoria (Prioridade Média)

- **Lixeira & Garbage Collection com TTL:**
  - Implementar cron/purge agendado para exclusão física no S3 e sweep de registros expirados (`images.deleted_at`, `generations.deleted_at`, `generation_inputs/`).
- **Upload Chunked de Modelos & Conexão BFF:**
  - [x] TTL/GC de sessões de upload chunked órfãs em memória e disco temporário (`sweep_expired_upload_sessions` via GC periódico).
  - [x] Retries com backoff exponencial no cliente HTTP do manager para `create_job` e `abort_job` (Quitado: Wave 3 `RD-032`).
  - Tornar `complete_upload` não-destrutivo em falhas transitórias do S3/manager.
- **Curadoria Human-in-the-Loop (AutoLabel):**
  - Permitir inspeção, edição e aprovação individual ou em lote das legendas geradas antes de aplicar ao dataset.
- **Reconciliação Storage:**
  - Auditoria periódica de divergências bucket S3 vs Postgres (`datasets.size_bytes` vs `ListObjectsV2`).

---

## 4. Frontend & Interface (Prioridade Média-Baixa)

- **Acessibilidade & Modais:**
  - Focus-trap e foco inicial nos modais base (`Modal.tsx`, `ImportDatasetModal`).
  - Ajuste de hit-area mínima em `SegmentedControl` (WCAG 2.5.8).
- **Refinamento de Estado:**
  - Eliminar duplicação da lógica `canTrain`/`trainDisabledReason` na galeria de datasets.
  - Desativar polling de telemetria da sidebar quando o drawer estiver fechado.
  - Push-down de paginação/filtro de quantização no BFF/manager para evitar degradação em memória.
