# Backlog & Dívidas Técnicas — Hephaestus LLM Studio

Consolidação única de pendências e melhorias prioritárias. Rotas canônicas em `packages/contracts/openapi.yaml`, limites de VRAM em `packages/policies/vram-table.yaml`.

---

## 1. Motores & Treino (Prioridade Alta)

- [x] **QLoRA Difusão Canônico:** quantização 4-bit NF4 na UNet/Transformer + Paged Optimizers + gradient checkpointing nativo diffusers (Quitado 2026-09-20; spec arquivada em `docs/archive/specs/qlora-difusao.md`).
- **Validação @gpu Difusão & Img2Img:** Validar carregamento com pesos reais (`ENGINE_MOCK=0`) para SDXL/Flux-2-Klein, multi-LoRA PEFT e geração sequencial em daemon quente (ADR-0023).
- **AutoLabel v2:** Evoluir motor para modelos VLM reais (Florence-2 / Qwen-VL) com aceleração GPU (v1 atual é determinística mock).

---

## 2. Orquestração & Backend (Prioridade Média-Alta)

- **Pipelines & Abort Races:**
  - [x] Tratar abort durante `preparing` e `dispatched` para evitar término incorreto em `failed` em vez de `cancelled` (Quitado: Wave 2 `RD-021`).
  - Ancorar watchdog de `preparing` no `updated_at` (heartbeat do worker) para builds longos (>60min).
  - Spec: `tasks/specs/backend-autonomia.md`.
- **Modularização e Autonomia do Orchestrator (P0–P3):**
  - [x] Decomposição do monólito `lib.rs` (7.9k LOC) em Clean Architecture (`domain/`, `ports/`, `adapters/`, `app/`, `server/`, `daemon/`, `testkit/`). (Quitado: fatia `refactor/orchestrator-modularization`).
  - [x] Autonomia do nó: spool outbox local durável para reports (P0-2), heartbeat com backoff/jitter (P2-1), reaper periódico de containers órfãos (P2-2), admissão atômica sem TOCTOU (P0-3) e graceful shutdown coordenado (P2-5). (Quitado: fatia `feat/orchestrator-autonomia`).
  - [x] Eliminação de duplicidades de DTOs espelho entre orchestrator/manager via `heph-contracts` (Quitado: Wave 1 `RD-010`).
  - [x] Despacho íntegro de `control_package_ref` no `dispatch_next` (Quitado: Wave 2 `RD-020`).
  - Specs: `tasks/specs/orchestrator-modularization.md` e `tasks/specs/backend-autonomia.md`.
- **Dívidas Técnicas & Modularização `api-principal`:**
  - [x] Consumir `heph-contracts` no BFF `api-principal`, deletar DTOs `Internal*` duplicados e decompor monólito `jobs/handlers.rs` em submódulos coesos preservando contrato OpenAPI wire `camelCase` (TASK-API-001 / fatia `feat/api-principal-contracts-modularizacao` quitada 2026-09-25; spec `tasks/specs/api-principal-modularizacao-auditoria.md`).
- **Packaging & Reuso:**
  - Assinar `build_package_diffusion` com `fingerprint` para permitir reuso de pacotes em treinos de difusão.
  - Marcar `dataset_versions` como `complete` apenas pós-upload confirmado para evitar reuso de versões órfãs.
- **Roteamento Dinâmico por VRAM:**
  - Implementar fila por VRAM livre dinâmica no manager (suporte a 2+ jobs por nó, preempção de runner, `max_parallel_trainers`).
- **Segurança & Credenciais de Nós:**
  - Adicionar credenciais dedicadas por nó (`heph_o_*`), rotação de tokens e rate-limit de pareamento.

- **Sensores Avançados de GPU & Seleção Multi-GPU (Prioridade Média-Alta):**
  - Coleta granular de sensores por GPU física: potência (W), utilização (%) e temperatura (°C) via `nvidia-smi` com fallback resiliente para ambientes sem suporte.
  - Seleção de GPU alvo (`gpuDevice` / `gpu_device`) nos fluxos de submissão de jobs (`/treino`, `/difusao`, autolabel, autotracker, geração img2img).
  - Mitigação definitiva do **Pitfall D9** (swap de índice pós-reboot) via identificação por GPU UUID de hardware no Docker executor (`--gpus "device=GPU-..."`).
  - Coexistência de isolamento do daemon de difusão (`DIFFUSION_DAEMON_GPU_DEVICE`) com treinos efêmeros em nós multi-GPU.
  - Componentes de UI: `MultiGpuRack`, alertas térmicos escalonados e `GpuDeviceSelect` integrado às telas do Studio.
  - Spec completa: `tasks/specs/multi-gpu-sensores-selecao.md`.
- **Observabilidade & Reprodutibilidade do Treino (fatia `fix/treino-observabilidade` — código completo 2026-09-20; deploy orquestrator/manager/BFF/engines na próxima janela segura):**
  - [x] C2b (regressão, P0): `parse_metrics_line` do orchestrator não achata `v["metrics"]` do `telemetry.jsonl` (coletor prioriza esse arquivo desde RD-022/ADR-0023) → `loss`/`lr` nunca persistam em `jobs.metrics` → gráfico de loss vazio em runtime e pós-refresh. (Quitado 2026-09-20 `fix/treino-observabilidade` commit fbb58e8; deploy na próxima janela segura do orchestrator.)
  - [x] C1: "Repetir Treino"/"Retomar" do ActionCenter gravam chaves sessionStorage órfãs (`heph_resume_job`/`heph_rerun_yolo` — zero consumidores); `training_config.json` (snake_case aninhado em `lora:`) é incompatível com o importador de preset da Forja. (Quitado 2026-09-20 commit 615ed65: mapper único `lib/paramsToPreset.ts`, consumidor YOLO real em `/treino`, import snake→preset, fallback de download rotulado `_source`.)
  - [x] C2a/C2c: logs do job não são persistidos em lugar nenhum (`JobLogViewer` sintetiza linhas de estado React + SSE transitório → refresh apaga tudo); fases de split/cache latent emitidas só como `print` no daemon. (C2c quitado commit 38be5d9 — sem split train/val no repo, `preparing_dataset` cobre scan/bucketing; C2a quitado commits 7c89958/9863021 — snapshot incremental `logs/telemetry.jsonl` no orquestrador + `GET /api/jobs/:id/logs` no BFF + histórico paginado no viewer.)
  - [x] F1: ETA de treino a partir dos deltas step/timestamp do SSE (depende de C2b; só web). (Quitado 2026-09-20 commit b8b3844 — mediana P50, janela 30, stalls>120s descartados.)
  - [x] F2: ZIP de artefatos pós-treino — streaming no BFF (zip stored) + rota nova + bump de openapi. (Quitado 2026-09-20 commits 7366c9b/9863021 — `GET /api/jobs/:id/artifacts/zip` no padrão ADR-0006 + botões em /jobs e ActionCenter.)
  - Spec completa com linhas exatas e sequência de execução arquivada em: `docs/archive/specs/treino-observabilidade.md`.

---

## 3. Storage, Dados & Curadoria (Prioridade Média)

- **Lixeira & Garbage Collection com TTL:**
  - [x] TTL/purge agendado para exclusão física no S3 (`sweep_expired_trash` >30d + `generation_inputs/` >7d em `storage/gc.rs`, GC 10min). Falta ainda purge físico de `generations.deleted_at`.
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
  - [x] Focus-trap, scroll lock e restauração de foco nos diálogos base (`useFocusTrap`/`useBodyScrollLock` em `Modal`/`Drawer`/`ConfirmDialog`; shims antigos removidos).
- **Acessibilidade — hit-area:**
  - [x] Ajuste de hit-area mínima em `SegmentedControl` (WCAG 2.5.8) (Quitado: fatia `feat/web-ui-modularizacao-a11y`).
- **Refinamento de Estado:**
  - [x] Eliminar duplicação da lógica `canTrain`/`trainDisabledReason` na galeria de datasets (Quitado: unificado em `lib/datasets.ts`).
  - [x] Desativar polling de telemetria da sidebar quando o drawer estiver fechado ou aba oculta (Quitado: listener de visibilidade e estado do drawer em `Sidebar.tsx`).
  - Push-down de paginação/filtro de quantização no BFF/manager para evitar degradação em memória.

---

## 5. Resíduos de Specs (verificação item-a-item 2026-09-20)

- **`backend-autonomia.md` (FICAR):** cache/ETag ou pub-sub no SSE (1.10); streaming no `get_artifact_data` (1.11); watchdog por job `running` sem telemetria (5.3); agendar `cleanup_jobs` no worker do manager (5.5); forward de `x-request-id`/`traceparent` na malha (4.2); `/ready` com checagens profundas (4.3); mascarar senha de bootstrap no log (4.4). Já tem dono acima: 1.12 (`updated_at`), 5.7 (Reconciliação Storage).
- **`orchestrator-modularization.md` (FICAR):** §5 Autonomia 0/5 — além da linha 23, faltam admissão atômica no dispatch (P0-3, `server/handlers.rs:106-123`), `testkit/`+`tests/` fora do `src` (P1-6), `/ready` com Docker/disco (P2-3), docker_args unificado executor×daemon (P0-5), bypass de auth sem token (P3-1) e SIGTERM/graceful shutdown.
- **`consolidacao-auditoria-roadmap.md` (FICAR):** api-principal consumir `heph-contracts` + dispatch tipado no manager (RD-010 — 0× no BFF, `manager/lib.rs:3958`); eliminar duplicatas `flux2-klein-4b`/linha `difusao` em `packages/policies/{vram-table,engines}.yaml` (RD-002); `--user` default no `docker run` sem opt-in via env (RD-023); E2E hermético mock no CI (RD-050).
- **`web-modularizacao-auditoria.md` (FICAR):** `ui/Select.tsx` 643L (extrair `useFloatingPosition`/`useListboxNavigation` e consumir `FormField` — TASK-WEB-009); `jobs/page.tsx` 1114L; `datasets/[id]/page.tsx` 392L > teto (busca CLIP não extraída); eliminar ~40 `as any` de fallback snake_case (RD-040) e boundaries per-rota (015).
- **`infra-auditoria.md` (FICAR):** gerar identidade S3 do SeaweedFS a partir de env/template fora do versionamento (INFRA-03 — `infra/seaweedfs-s3.json` commitado); CI com `compose up` de integração real (INFRA-20); `USER` no Dockerfile do orchestrator (INFRA-05); CSP no Caddyfile + digest da imagem caddy (INFRA-01); `set -euo pipefail` no `doctor.sh` (INFRA-18).
- **`infra-autonomia.md` (FICAR):** redes segmentadas frontend/backend/engine (quitado: 1.3 na fatia `feat/infra-redes-segmentadas`); `HEALTHCHECK` nos Dockerfiles (2.5); cargo-chef (2.4); lifecycle/expiração de artefatos não consolidados no S3 (3.3); remover IPs literais de `infra/env.gpu.example` e defaults de `start-truenas.sh` (4.1); converter scripts legados (`start-host*`, `build-*`) em delegações do `heph.sh` (6.1).
