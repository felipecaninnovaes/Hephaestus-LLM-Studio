# Memória Ativa — Hephaestus LLM Studio

- **Branch atual:** `fix/trainer-difusao-qlora-flux2-checkpointing`
- **Fatia em andamento:** Correção de crash QLoRA PEFT no FLUX.2 Klein / SD 1.5 / SDXL (`Flux2Transformer2DModel has no attribute get_input_embeddings`).
---

## Checklist Imediato da Sessão Ativa

- [x] Fase 1: Motor de Treino Difusão (optimizers.py, sd15.py, sdxl.py, flux.py, mock.py, testes)
- [x] Fase 2: Contratos OpenAPI e Validação no api-principal Rust
- [x] Fase 3: Políticas de VRAM (vram-table.yaml)
- [x] Fase 4: Interface Web Studio (types/studio.ts, jobs.ts e ForjaDifusaoSetup.tsx)
- [x] Fase 5: Validação completa (Python pytest 160/160, Cargo test 708/708, Web build, Docker config)
---

## Levantamento Arquitetural Recente

- [x] **Levantamento e Auditoria de Modularização do Orchestrator:**
  - Autópsia completa de `services/orchestrator/` (`lib.rs` 7.9k LOC, `daemon.rs` 1.1k LOC, `main.rs` 726 LOC).
  - Mapeamento de duplicidades com `manager` e `api-principal`.
  - Desenho da nova arquitetura Clean/Hexagonal (`domain/`, `ports/`, `adapters/`, `app/`, `server/`, `daemon/`, `testkit/`).
  - Plano de autonomia do nó (outbox durável, heartbeat backoff, reaper periódico, GC de disco).
  - Documento mestre de especificação gerado em `tasks/specs/orchestrator-modularization.md`.

- [x] **Fatia 1: Extração da Camada de Configuração (`config/`):**
  - Módulo `services/orchestrator/src/config/mod.rs` criado com `OrchestratorConfig` e `DaemonConfig`.
  - `main.rs` enxugado com eliminação de leituras manuais dispersas de envs.
  - 165 testes passando (14 novos testes de validação fail-fast e fallbacks de config).
- [x] **Fatia 2: Extração de Modelos de Domínio e Portas (`domain/` e `ports/`):**
  - Modelos puros e erros migrados para `src/domain/` (`models.rs`, `errors.rs`).
  - Traits abstratas migradas para `src/ports/` (`storage.rs`, `executor.rs`, `reporter.rs`, `heartbeat.rs`).
  - `lib.rs` enxugado em quase 300 linhas com re-exports transparentes; 165 testes passando.
- [x] **Fatia 3: Extração da Camada HTTP (`server/`):**
  - Handlers, middlewares, router e `AppState` migrados para `src/server/`.
  - `main.rs` encolhido em 365 linhas (de ~667 para ~308 linhas); 165 testes passando.
- [x] **Fatia 4: Modularização do Subsistema Daemon (`daemon/`):**
  - Arquivo monolítico `daemon.rs` (1.140 linhas) decomposto no diretório `src/daemon/` (`types`, `client`, `launcher`, `state`, `lifecycle`, `tests`).
  - Eliminado warning pré-existente de método morto `inspect_container_ip`.
  - 165 testes passando verdes.
- [x] **Fatia 5: Extração da Camada de Storage e Cache de Pesos (`storage/`):**
  - Criado `src/storage/` (`scope.rs`, `archive.rs`, `s3.rs`, `cache.rs`, `mod.rs`).
  - Mais de 400 linhas monolíticas de I/O de storage removidas de `lib.rs`.
  - 165 testes passando verdes.
- [x] **Fatia 6: Fatiamento do Pipeline e Unificação de Coleta de Artefatos (`app/` e `stages/`):**
  - Monólito `run_job_inner` (1.826 linhas) decomposto em `src/app/` (`mod.rs`, `stages/{collector, weights, config, execute}.rs`).
  - Unificada coleta de artefatos (`collect_diffusion_artifacts`), eliminando duplicação entre daemon e one-shot (P0-4).
  - Unificado staging de pesos com hash MD5 em `resolve_and_stage_weight` (P1-3).
  - Mais de 1.800 linhas removidas de `lib.rs`; 165 testes passando verdes.
- [x] **Fatia 7: Extração de Adaptadores, Telemetria e Segurança (`adapters/`, `telemetry/`, `security/`):**
  - Criados módulos `src/telemetry/` (`host`, `gpu`, `metrics`), `src/adapters/` (`docker`, `subprocess`, `sweeper`, `http`) e `src/security/` (`pairing`).
  - Mais de 750 linhas removidas de `lib.rs`; toda lógica de produção de `lib.rs` foi 100% modularizada.
  - `lib.rs` opera como fachada pura de declaração e re-exports; 165 testes passando verdes.
- [x] **Fatia 8: Extração da Suíte de Testes Inline (`tests.rs`):**
  - Suíte de 4.683 linhas migrada para `src/tests.rs` (`#[cfg(test)] mod tests;`).
  - `lib.rs` reduzido de 7.975 linhas para **43 linhas** (fachada canônica e limpa).
  - 165 testes passando verdes.
## Protocolo de Retomada (3 Passos)

1. **Conferir Branch e Active:** Confirmar git branch atual (`git status`) e ler `tasks/active.md` para situar a fatia e checklist em andamento.
2. **Checar Integridade com Graft:** Executar `mcp__graft_check_freshness` para validar que o grafo de símbolos e dependências está sincronizado.
3. **Consultar REPO_MAP e Contratos:** Ler `docs/REPO_MAP.md` e `packages/contracts/openapi.yaml` antes de planejar alterações de código ou novos endpoints.
