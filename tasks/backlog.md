# Backlog & Dívidas Técnicas — Hephaestus LLM Studio

Consolidação única de pendências e melhorias prioritárias. Rotas canônicas em `packages/contracts/openapi.yaml`, limites de VRAM em `packages/policies/vram-table.yaml`.

---

## 1. Motores & Treino (Prioridade Alta)

- [x] **QLoRA Difusão Canônico:** quantização 4-bit NF4 na UNet/Transformer + Paged Optimizers + gradient checkpointing nativo diffusers (Quitado 2026-09-20; spec arquivada em `docs/archive/specs/qlora-difusao.md`).
- **Validação @gpu Difusão & Img2Img:** Validar carregamento com pesos reais (`ENGINE_MOCK=0`) para SDXL/Flux-2-Klein, multi-LoRA PEFT e geração sequencial em daemon quente (ADR-0023).
- [x] **Unificação dos 4 trainers de difusão (Template Method):** Fases A (sd15+sdxl) e B (flux.1+flux.2-klein) unificadas via `TrainingLoopRunner`/`ModelAdapter` em `models/loop.py` (-1.876 linhas líquidas em `sd15.py`/`sdxl.py`/`flux.py`); Fase C reduzida por decisão explícita (risco vs ganho reavaliado após 2 bugs críticos achados na Fase B) a só migrar `prompt_cache` de RAM para `TextEmbedsCache` em disco em `qwen_image.py` (item #12), sem forçar o Template Method completo no modelo flagship. Fecha #7/#8/#12 de `engines-auditoria-global.md`. As 3 fases com smoke real (SD15/SDXL/FLUX.2-Klein/Qwen-Image-2.1 reais, pesos HF baixados on-the-fly) no nó GPU e gate `@reviewer` aprovado (Quitado 2026-09-26; commits d062c93, f94541c, a61344b; spec arquivada em `docs/archive/specs/trainer-difusao-unificacao-modelos.md`).
- **AutoLabel v2:** Evoluir motor para modelos VLM reais (Florence-2 / Qwen-VL) com aceleração GPU (v1 atual é determinística mock).
- [x] **AutoLabel — telemetria sem progresso por imagem** (reportado pelo usuário 2026-10-03): telemetria mostrava só o carregamento do dataset (job `bc54d01c`, 986 imagens, `openai`, parado em `[PREPARING] (5%)` por >3h). Corrigido em `5f29b0b` + `d407269` (engine emite `phase=labeling` por imagem com `step/totalSteps`, progresso 0.05→0.99, ETA por EMA; removidas as chaves YOLO fabricadas do `metrics.jsonl`) e `aa1e102` (UI: "Anotando Imagens", tag `AUTOLABEL`). Smoke 2026-10-03: job mock de 25 imagens no docker-04 com 25 eventos `labeling` no `telemetry.jsonl` servidos por `GET /api/jobs/:id/logs?source=telemetry` e zero `box_loss` no `metrics.jsonl`; progresso ao vivo na UI não observado (mock/florence-2 terminam em < 10 s) (Quitado 2026-10-03).
- **Daemon de geração Qwen-Image-2.1 nativo (gap achado pelo @reviewer):** o
  treino nativo (`models/qwen_image.py`) está completo/verificado
  (`fix/qwen-image-2-1-native`, 234 pytest, smoke GPU real com
  `epoch_complete loss=0.026513001415878534 step=8`, checkpoint LoRA-only
  256 tensores válido). O caminho quente de geração do daemon
  (`generation/runner.py`, branch `elif base_model == "qwen-image-2.1":`)
  chamava `QwenImage21Pipeline.from_pretrained(...)`, que não existe — e
  mesmo que existisse, o resto do `runner.py` downstream para esse `pipe`
  exige uma interface completa equivalente a
  `diffusers.DiffusionPipeline` (`.load_lora_weights()`/`.set_adapters()`
  com hot-swap multi-adapter nomeado/escalado, dict `.components` usado
  para reconstruir uma variante img2img via `_I2I(**pipe.components)`,
  `.scheduler.config`, `pipe(**kwargs).images[0]` chamável) que o
  `QwenImage21Pipeline` vendorizado (um sampler de preview de treino de
  ~30 linhas em `qwen_pkg/sample.py`/`qwen_image_2/`) não fornece. Nesta
  fatia o branch foi substituído por um erro explícito
  (`_die(...)`) em vez de deixar o caminho quebrado silenciosamente.
  Escopo desta fatia futura: implementar (ou adaptar) uma classe de
  pipeline nativa Qwen-Image-2.1 com a interface completa acima, ou
  reescrever o trecho relevante de `generation/runner.py` para não
  depender dela. Spec de origem: `tasks/specs/qwen-image-2.1-native-rewrite.md`.

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
  - [x] Assinar `build_package_diffusion` com `fingerprint` para permitir reuso de pacotes em treinos de difusão (Quitado: fatia `feat/no-gpu-reuso-dataset-embeds`, Pilar A gap de diffusion).
  - Marcar `dataset_versions` como `complete` apenas pós-upload confirmado para evitar reuso de versões órfãs.
- **Roteamento Dinâmico por VRAM:**
  - Implementar fila por VRAM livre dinâmica no manager (suporte a 2+ jobs por nó, preempção de runner, `max_parallel_trainers`).
- **Segurança & Credenciais de Nós:**
  - Adicionar credenciais dedicadas por nó (`heph_o_*`), rotação de tokens e rate-limit de pareamento.
- **Recovery do manager reconciliado:** `recover_jobs` no boot (`services/manager/src/watchdog/recovery.rs:27`) deve reconciliar com o orchestrator (jobs ativos reportados por heartbeat/endpoint) em vez de requeue cego de `dispatched|running` (incidente 2026-10-03, job `bc54d01c`; ver PITFALLS § Orquestração).
- **Daemon de difusão frio derruba a 1ª geração:** com o daemon recém-ligado, a 1ª geração falha com "error sending request"/broken pipe porque o orchestrator desiste enquanto o daemon ainda baixa/carrega o modelo (sd15); as seguintes, quentes, saem em ~3 s. Pré-existente, achado no smoke multi-GPU de 2026-10-03 no docker-04 (`DIFFUSION_DAEMON_ENABLED=1`). Orchestrator deve aguardar o daemon ficar pronto (readiness/timeout de carga) antes de enviar a geração.
- **Imagens/tags de rollback sumiram do docker-04:** por volta de 2026-10-03T00:50Z `hephaestus/trainer-difusao:gpu` e todas as tags de rollback (`:pre-3a`, `:pre-metrickeys`, `:pre-telemetria`, `:gpu-pre-resume-fix`, `gpu-orchestrator-gpu:pre-multigpu-b1`) desapareceram; `trainer-yolo:gpu` foi recriada às 00:50:45Z. Sem cron/timer/Komodo periphery identificado (buffer do `docker events` já sobrescrito); suspeita de prune externo (Komodo Core/manual). `trainer-difusao:gpu` rebuildada do build cache; um job de smoke falhou com exit 125. Investigar a causa e proteger imagens/tags de rollback no nó.

- **Observabilidade & Reprodutibilidade do Treino (fatia `fix/treino-observabilidade` — código completo 2026-09-20; deploy orquestrator/manager/BFF/engines na próxima janela segura):**
  - [x] C2b (regressão, P0): `parse_metrics_line` do orchestrator não achata `v["metrics"]` do `telemetry.jsonl` (coletor prioriza esse arquivo desde RD-022/ADR-0023) → `loss`/`lr` nunca persistam em `jobs.metrics` → gráfico de loss vazio em runtime e pós-refresh. (Quitado 2026-09-20 `fix/treino-observabilidade` commit fbb58e8; deploy na próxima janela segura do orchestrator.)
  - [x] C1: "Repetir Treino"/"Retomar" do ActionCenter gravam chaves sessionStorage órfãs (`heph_resume_job`/`heph_rerun_yolo` — zero consumidores); `training_config.json` (snake_case aninhado em `lora:`) é incompatível com o importador de preset da Forja. (Quitado 2026-09-20 commit 615ed65: mapper único `lib/paramsToPreset.ts`, consumidor YOLO real em `/treino`, import snake→preset, fallback de download rotulado `_source`.)
  - [x] C2a/C2c: logs do job não são persistidos em lugar nenhum (`JobLogViewer` sintetiza linhas de estado React + SSE transitório → refresh apaga tudo); fases de split/cache latent emitidas só como `print` no daemon. (C2c quitado commit 38be5d9 — sem split train/val no repo, `preparing_dataset` cobre scan/bucketing; C2a quitado commits 7c89958/9863021 — snapshot incremental `logs/telemetry.jsonl` no orquestrador + `GET /api/jobs/:id/logs` no BFF + histórico paginado no viewer.)
  - [x] F1: ETA de treino a partir dos deltas step/timestamp do SSE (depende de C2b; só web). (Quitado 2026-09-20 commit b8b3844 — mediana P50, janela 30, stalls>120s descartados.)
  - [x] F2: ZIP de artefatos pós-treino — streaming no BFF (zip stored) + rota nova + bump de openapi. (Quitado 2026-09-20 commits 7366c9b/9863021 — `GET /api/jobs/:id/artifacts/zip` no padrão ADR-0006 + botões em /jobs e ActionCenter.)
  - Spec completa com linhas exatas e sequência de execução arquivada em: `docs/archive/specs/treino-observabilidade.md`.

- [x] **Telemetria, Logs & Observabilidade (escopo completo, ondas 0–5, 17 fatias):** séries em `job_metric_points` + pub-sub `pg_notify`/SSE delta, captura `run.log`, propagação `x-request-id`/`traceparent` + OTel + Loki/Tempo/Grafana (porta 4000), sensores/diagnóstico de difusão, alertas só UI (`nan_detected`/`telemetry_stale`/`disk_high`/`vram_high`), uPlot/comparação, galeria/linhagem/export (Quitado 2026-10-03; 3a-GPU e `vram_high` pela B3 `e05cecc`; spec arquivada em `docs/archive/specs/telemetria-observabilidade.md`). Dívidas residuais:
  - Drop da coluna legada `jobs.metrics` (congelada; gate manual de contagem legado == `job_metric_points`, depois de remover o pivot legado de `GET /api/jobs/:id[/metrics]`). Evidência (2026-10-03): para jobs de autolabel, `GET /api/jobs/:id` ainda devolve `metrics: [{boxLoss:0, clsLoss:0, dflLoss:0, map50:0, map5095:0, ...}]` (chaves YOLO fabricadas pelo pivot legado).
  - Export OTel do nó GPU (hoje o `orchestrator-gpu` não exporta; sem rota ao collector).
  - Alertas via webhook (ntfy/Discord) — hoje alertas só na UI.
  - Prova formal no nó GPU de `systemMetrics` em ≥70% das linhas de step do `telemetry.jsonl` (aceite 3a não registrado; pontos `sys.*` já observados nos smokes).
- [x] **Sensores de GPU & Seleção Multi-GPU:** sensores por GPU física (VRAM/W/%/°C) no heartbeat, `/api/orchestrators` e telemetria; `gpuDevice` (UUID ou índice → UUID) nos 5 submits com eleição por VRAM da placa (automático = menor que cabe com headroom; manual sem headroom); daemon fixado por `DIFFUSION_DAEMON_GPU_DEVICE` e preempção só na mesma GPU; `sys.gpu.*` por job e alerta `vram_high`; UI `MultiGpuRack` + `GpuDeviceSelect`; Pitfall D9 mitigado por UUID (Quitado 2026-10-03; commits `519cd9e`, `f83ddb1`, `e05cecc`, `95da793`, `691468d`, `2cc03b8` + fix `047a827`; smoke real no docker-04; spec arquivada em `docs/archive/specs/multi-gpu-sensores-selecao.md`).

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
- [x] **Reaproveitar dataset e text-embeds entre jobs do mesmo dataset no nó GPU** (Média; donos `@backend` orchestrator + `@engines` text cache + `@infra` volumes; medido 2026-10-01 em `docker-04`):
  - Pacote S3 já é reusado por fingerprint (`try_reuse_package`, `services/api-principal/src/jobs/prepare.rs:578`); sem re-upload ao SeaweedFS. O desperdício está no nó.
  - (1) `run_job_inner` (`services/orchestrator/src/app/mod.rs`) baixa `dataset.zip` (3.7 GB) num `temp_dir` por job e extrai em `datasets/datasets-cache/<job_id>/`; sem cache por `md5_zip` (diferente de `storage/cache.rs`, MD5 dedupe + hardlink); só `sweep_orphan_workdirs` limpa após 24h.
  - (2) `outputs/<job_id>/` nunca é limpo (só `temp_dir`, `mod.rs:1414`): 39 GB em `/data/outputs` com 12 jobs; `0962380c` = 9.2 GB, sendo **7.3 GB de `text_embeds_cache`**; resume cancelado `a65f012e` +8.7 GB (7.3 GB de text_embeds idênticos).
  - (3) No resume o `text_embeds_cache` é recalculado do zero (mesmo dataset + encoder): custo de tempo e disco.
  - Proposta (especificar como fatia): (a) cache local de dataset por `md5_zip` no padrão de `storage/cache.rs` (promoção atômica + hardlink/bind no job) com retenção LRU/por tamanho; (b) `text_embeds_cache` compartilhado por (fingerprint do dataset, encoder, quantização) fora de `outputs/<job>`; (c) retenção de `outputs/<job>` após confirmar upload dos artefatos ao S3 (purgar `text_embeds_cache` e checkpoints locais).
  - Spec: `docs/archive/specs/no-gpu-reuso-dataset-embeds.md` (fatia `feat/no-gpu-reuso-dataset-embeds`, Quitado 2026-10-01).

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
- **VRAM exibida incorreta em nó heterogêneo (multi-GPU):** opção de nó no `NodeSelect` mostra VRAM somada ("18GB VRAM"); painel "VRAM estimada" do treino de difusão mostra "Dispositivo: NVIDIA GeForce RTX 3060 (24 G…" e alerta de OOM para ~8 GB numa placa de 12 GB — conferir a fonte desses números.

---

## 5. Resíduos de Specs (verificação item-a-item 2026-09-20)

- **`backend-autonomia.md` (FICAR):** cache/ETag ou pub-sub no SSE (1.10 — absorvido pela fatia 1b de `docs/archive/specs/telemetria-observabilidade.md`); streaming no `get_artifact_data` (1.11); watchdog por job `running` sem telemetria (5.3 — absorvido pela regra `telemetry_stale` da fatia 3c da mesma spec); agendar `cleanup_jobs` no worker do manager (5.5); forward de `x-request-id`/`traceparent` na malha (4.2 — absorvido pela fatia 2b da mesma spec); `/ready` com checagens profundas (4.3); mascarar senha de bootstrap no log (4.4). Já tem dono acima: 1.12 (`updated_at`), 5.7 (Reconciliação Storage).
- **`orchestrator-modularization.md` (FICAR):** §5 Autonomia 0/5 — além da linha 23, faltam admissão atômica no dispatch (P0-3, `server/handlers.rs:106-123`), `testkit/`+`tests/` fora do `src` (P1-6), `/ready` com Docker/disco (P2-3), docker_args unificado executor×daemon (P0-5), bypass de auth sem token (P3-1) e SIGTERM/graceful shutdown.
- **`consolidacao-auditoria-roadmap.md` (FICAR):** api-principal consumir `heph-contracts` + dispatch tipado no manager (RD-010 — 0× no BFF, `manager/lib.rs:3958`); eliminar duplicatas `flux2-klein-4b`/linha `difusao` em `packages/policies/{vram-table,engines}.yaml` (RD-002); `--user` default no `docker run` sem opt-in via env (RD-023); E2E hermético mock no CI (RD-050).
- **`web-modularizacao-auditoria.md` (FICAR):** `ui/Select.tsx` 643L (extrair `useFloatingPosition`/`useListboxNavigation` e consumir `FormField` — TASK-WEB-009); `jobs/page.tsx` 1114L; `datasets/[id]/page.tsx` 392L > teto (busca CLIP não extraída); eliminar ~40 `as any` de fallback snake_case (RD-040) e boundaries per-rota (015).
- **`infra-auditoria.md` (FICAR):** gerar identidade S3 do SeaweedFS a partir de env/template fora do versionamento (INFRA-03 — `infra/seaweedfs-s3.json` commitado); CI com `compose up` de integração real (INFRA-20); `USER` no Dockerfile do orchestrator (INFRA-05); CSP no Caddyfile + digest da imagem caddy (INFRA-01); `set -euo pipefail` no `doctor.sh` (INFRA-18).
- **`infra-autonomia.md` (FICAR):** redes segmentadas frontend/backend/engine (quitado: 1.3 na fatia `feat/infra-redes-segmentadas`); `HEALTHCHECK` nos Dockerfiles (2.5); cargo-chef (2.4); lifecycle/expiração de artefatos não consolidados no S3 (3.3); remover IPs literais de `infra/env.gpu.example` e defaults de `start-truenas.sh` (4.1); converter scripts legados (`start-host*`, `build-*`) em delegações do `heph.sh` (6.1).
