# Consolidação Transversal, Auditoria de Contratos e Roadmap Unificado
**Projeto:** Hephaestus LLM Studio  
**Data:** 19 de Setembro de 2026  
**Escopo:** Transversal Monorepo (`apps/web`, `services/*`, `engines/*`, `infra/*`, `packages/*`)  
**Status:** Concluído (Auditoria Estritamente Somente Leitura)

---

## 1. Resumo Executivo da Arquitetura Holística

O **Hephaestus LLM Studio** é uma plataforma local-first para curadoria de datasets, treinamento supervisionado (YOLOv11, FLUX.2 Klein, SDXL, SD 1.5) e inferência em tempo real. A arquitetura monorepo é estruturada em torno de quatro pilares de código (`apps/web`, `services/`, `engines/`, `infra/`) e um pilar de contratos (`packages/`).

A auditoria transversal cruzou as análises setoriais (`tasks/web-modularizacao-auditoria.md`, `tasks/specs/orchestrator-modularization.md`, `tasks/specs/backend-autonomia.md`, `tasks/specs/infra-autonomia.md`) com a inspeção estrita dos pontos de encontro em `packages/` e das quatro fronteiras de comunicação intercamadas.

### Diagnóstico Macro (Estado Atual vs Arquitetura Alvo)

```
ESTADO ATUAL (Fragmentado & Acoplado):
[Web UI] ──(Tipos manuais TS + camel/snake mistos)──> [api-principal :8080]
                                                            │ (Body JSON dinâmico; drop de campos)
                                                            ▼
                                                     [manager :8081]
                                                            │ (DTOs duplicados; perda de control_package_ref)
                                                            ▼
                                                   [orchestrator :8082]
                                                            │ (Dualidade metrics.jsonl vs telemetry.jsonl)
                                                            ▼
                                                   [engines Python] (Root container; HEPHMOCK isolado)

ARQUITETURA ALVO (Orientada a Contratos & Autônoma):
[Web UI] ◄──(Tipos gerados via openapi-typescript)──► [api-principal :8080]
                                                            │ (heph-contracts: DTOs compartilhados)
                                                            ▼
                                                     [manager :8081]
                                                            │ (Protocolo interno tipado e auditado)
                                                            ▼
                                                   [orchestrator :8082]
                                                            │ (telemetry.jsonl canônico; non-root UID)
                                                            ▼
                                                   [engines Python] (engine-kit centralizado; paridade bitwise)
```

| Dimensão | Estado Atual (Auditoria) | Arquitetura Alvo (Padrão Inegociável) |
|---|---|---|
| **Contratos HTTP (`packages/contracts`)** | OpenAPI 0.29.0 validado no BFF via teste de inventário, porém com schemas incompletos (`Job` omite `totalSteps` e `totalEpochs`, `JobParams` deixado como `object` genérico). | OpenAPI 3.1.0 canônico e estrito, tipando integralmente todos os campos retornados e parâmetros por engine. |
| **Políticas de VRAM (`packages/policies`)** | `vram-table.yaml` com duplicações sintáticas (`diffusion` vs `difusao`, `flux-2-klein-4b` vs `flux2-klein-4b`), gerando bypass silencioso para fallback permissivo no manager. | Tabela normalizada em inglês, tipada, espelhando 100% os enums do OpenAPI e os checks de hardware. |
| **DTOs Inter-Serviços (`services/`)** | Tipos duplicados manualmente entre `api-principal`, `manager` e `orchestrator`. Perda silenciosa de campos (`control_package_ref` gerado no BFF mas ignorado no dispatch do manager). | Crate compartilhada interna (`heph-contracts` / `heph-dto`) garantindo sincronia em tempo de compilação. |
| **Telemetria de Engines (`engines/`)** | Bifurcação legada: `orchestrator` lê `metrics.jsonl` em jobs one-shot e `telemetry.jsonl` no daemon. `HEPHMOCK` restrito ao YOLO. | `telemetry.jsonl` unificado gerado via `TelemetryEmitter` (`engine-kit`) e consumido universalmente. |
| **Segurança e Rede (`infra/`)** | Rede bridge única `infra_default`. Portas internas publicadas no host em modo dev; Caddy sem flushing explícito de SSE (`flush_interval -1`). | Segmentação em zonas (`frontend_net`, `backend_net`, `engine_net`), zero portas internas abertas e SSE desbufferizado. |

---

## 2. Auditoria Direta dos Pacotes Compartilhados (`packages/`)

### 2.1 `packages/contracts/openapi.yaml`

A especificação OpenAPI 3.1.0 (`version: 0.29.0`, 4.982 linhas) foi auditada sintática e semanticamente:
1. **Validade e Integridade Referencial:**
   - Sintaxe YAML e integridade de `$ref`: **100% íntegra** (313 referências internas analisadas; 0 `$ref` quebrados).
   - Wire camelCase: **100% em conformidade** nas propriedades de `components.schemas` (0 violações detectadas pelo analisador AST).
2. **Drifts e Omissões Críticas:**
   - **Campos Omitidos no Schema `Job` (`packages/contracts/openapi.yaml:4428-4518`):** O backend `api-principal` serializa ativamente `totalSteps` e `totalEpochs` em `JobResponse` (`services/api-principal/src/jobs/handlers.rs:218-221`), e o frontend os consome em `apps/web/types/jobs.ts:159-160`. No entanto, o schema `Job` na OpenAPI omite ambas as propriedades e declara `additionalProperties: false`. O teste de contrato em `services/api-principal/tests/contract.rs` não detectou essa quebra porque valida chaves detalhadas apenas para `DatasetResponse`, não para `JobResponse`.
   - **`params` como Objeto Genérico Opaque (`packages/contracts/openapi.yaml:4515-4517`):** Declarado como `type: [object, "null"]` sem schemas filhos. Isso forçou o frontend a tolerar misturas de `camelCase` e `snake_case` (`baseModel` / `base_model`, `batchSize` / `batch_size`, `triggerWord` / `trigger_word`) em `apps/web/types/jobs.ts:107-127`.
   - **Assimetria no enum `orchestratorKind` (`packages/contracts/openapi.yaml:4493`):** Declara `enum: [docker, slurm, "null"]`, enquanto o frontend suporta `"local" | "remoto"` em `apps/web/types/jobs.ts:165`.
   - **Nulabilidade de `datasetId` em `Job` (`packages/contracts/openapi.yaml:4448`):** OpenAPI e BFF definem `datasetId` como `[string, "null"]` (pois um dataset pode ser excluído mantendo histórico de jobs), enquanto `apps/web/types/jobs.ts:152` declara incorretamente como não-nulo (`datasetId: string`), arriscando exceções em runtime no cliente.

### 2.2 `packages/policies/vram-table.yaml`

O arquivo `packages/policies/vram-table.yaml` (28 linhas) define os requisitos mínimos de memória e políticas de escalonamento.
1. **Divergência Crítica de Nomenclatura e Bypass de VRAM:**
   - No arquivo `vram-table.yaml`:
     - Linhas 7-10 usam `engine: diffusion`, `model: flux2-klein-4b`, `mode: train`.
     - Linhas 11-14 usam `engine: difusao` (português!), `model: flux2-klein-4b`, `mode: train`.
     - Linha 16 usa `engine: diffusion`, `model: flux-2-klein-4b` (com hífen entre `flux` e `2`), `mode: generate`.
   - No código do `manager` (`services/manager/src/lib.rs:454-459`):
     ```rust
     pub fn resolve_required_gb(&self, engine: &str, model: &str, mode: &str) -> Option<i32> {
         self.entries
             .iter()
             .find(|e| e.engine == engine && e.model == model && e.mode == mode)
             .map(|e| e.vram_min_gb + self.defaults.headroom_gb)
     }
     ```
   - No `api-principal` (`services/api-principal/src/jobs/handlers.rs:1706-1708`):
     O submit de treino despacha `"engine": "diffusion"`, `"model": "flux-2-klein-4b"`, `"mode": "train"`.
   - **Impacto:** A busca em `vram-table.yaml` falha (linha 8 tem `flux2-klein-4b` sem hífen; linha 11 tem engine `difusao`). `resolve_required_gb` retorna silenciosamente `None` (modo permissivo). O job de treino de FLUX-2 Klein 4B tem a validação estática de VRAM completamente burlada por uma discrepância de string.
2. **Ausência de Modos Auxiliares:**
   - Jobs de `autolabel`, `autotracker` e `yolo_predict` despachados pelo BFF utilizam engines `autolabel`, `autotracker` e `yolo` com modos `autolabel`, `autotrack` e `predict` (`handlers.rs:1080, 1346, 2109`). Nenhum desses modos possui entrada em `vram-table.yaml`, operando sempre em fallback permissivo.

### 2.3 `packages/policies/engines.yaml`

O arquivo `packages/policies/engines.yaml` (8 linhas) registra as imagens canônicas e toolchains validadas.
- **Inconsistência de Identificadores:** Declara `engine: difusao` (linha 3), enquanto `vram-table.yaml` e os serviços Axum utilizam `diffusion`.
- **Falta de Versionamento Semântico:** As imagens utilizam tags genéricas `:local` e `:gpu` sem digests SHA-256 fixados, impossibilitando verificações herméticas de integridade entre nós heterogêneos.

---

## 3. Matriz das 4 Fronteiras Transversais

### Fronteira 1: Web (`apps/web`) ↔ BFF (`api-principal :8080`)

| Aspecto Auditado | Evidência Web | Evidência Backend | Diagnóstico & Risco |
|---|---|---|---|
| **Tipos e Codegen** | `apps/web/package.json:1-28`<br>`apps/web/types/jobs.ts:1-202` | `packages/contracts/openapi.yaml:4428`<br>`services/api-principal/src/jobs/handlers.rs:188` | **Ausência de gerador de tipos.** Tipos TypeScript são mantidos manualmente. Criação de campos fantasmas (`orchestratorKind: "local" \| "remoto"`) e omissão de propriedades no OpenAPI (`totalSteps`, `totalEpochs`). |
| **Envelope de Erro** | `apps/web/lib/api.ts:57-66`<br>`envelope = { code, message }` | `services/api-principal/src/error.rs:20-22`<br>`ErrorBody { code, message }` | **Alinhamento perfeito.** O formato JSON `{ code, message }` com status HTTP canônico é respeitado por ambos os lados da fronteira. |
| **Streaming SSE** | `apps/web/hooks/useJobTelemetry.ts:159`<br>`new EventSource("/api/jobs/${id}/events")` | `services/api-principal/src/jobs/handlers.rs:551-662`<br>`stream_job_events` (Axum SSE) | **Risco de buffering reverso.** Dev usa rewrite no Next (`next.config.ts:23-25`). Em prod, Caddy (`infra/Caddyfile:4,8`) aplica `encode zstd gzip` sem `flush_interval -1` no proxy do BFF, podendo reter eventos SSE em lote. |
| **Upload Chunked** | `apps/web/components/studio/ModelUploadModal.tsx:148-192` | `services/api-principal/src/models/chunk.rs:33-60`<br>`services/api-principal/src/storage/gc.rs:16` | **Alinhado no wire, mitigado no GC.** Partes cruas ≤ 96 MiB via `PUT`, finalizadas com `POST complete`. Sweeper implementado (`sweep_expired_upload_sessions`) rodando a cada 1h no BFF contra vazamento de disco temporário. |

### Fronteira 2: Manager (`:8081`) ↔ Orchestrator (`:8082`)

| Aspecto Auditado | Evidência Manager | Evidência Orchestrator | Diagnóstico & Risco |
|---|---|---|---|
| **Payload de Dispatch (`control_package_ref`)** | `services/manager/src/lib.rs:3966-4008`<br>`dispatch_next` monta `dispatch_body` | `services/orchestrator/src/domain/models.rs:81`<br>`control_package_ref: Option<PackageRef>` | **Bug Crítico de Comunicação.** O BFF gera `control_package_ref` para datasets de controle de difusão (`handlers.rs:1742`). O manager salva em `jobs.params`, mas o loop `dispatch_next` omite a extração desse campo, enviando-o vazio. O nó executor nunca recebe o dataset de controle. |
| **Ciclo de Vida e Watchdog** | `services/manager/src/lib.rs:3544-3551`<br>`degraded_s = 15s`, `offline_s = 60s` | `services/orchestrator/src/main.rs:231-234`<br>Loop de heartbeat a cada 2s | **Alinhamento temporal correto.** Heartbeat de 2s cumpre com ampla folga a janela de degradação de 15s e expiração de 60s. |
| **Fluxo de Cancelamento (Abort)** | `services/manager/src/lib.rs:1264-1293`<br>Status vira `cancelling`, retry até 3x | `services/orchestrator/src/server/handlers.rs:170-203`<br>`abort_handler` busca `active_jobs` | **Assimetria semântica no fallback.** Se o orquestrador não encontra o container ativo no momento do abort, reporta `failed` com erro `"job not found or already finished"` em vez de transitar para `cancelled`. |
| **Segurança e Tokens** | `services/manager/src/main.rs:848-871`<br>`resolve_manager_token` fail-fast | `services/orchestrator/src/main.rs:74-77`<br>`HttpHeartbeatClient` com Bearer | **Segurança parcial.** Em produção, o manager recusa tokens padrão. No entanto, o canal HTTP interno trafega em texto claro se executado em rede local sem TLS/WireGuard. |

### Fronteira 3: Orchestrator (`:8082`) ↔ Engines Python (`engines/*`)

| Aspecto Auditado | Evidência Orchestrator | Evidência Engine Python | Diagnóstico & Risco |
|---|---|---|---|
| **Invocação e Volumes** | `services/orchestrator/src/adapters/executor_docker.rs:20-58`<br>`build_docker_run_args` | `engines/trainer-yolo/src/trainer_yolo/train.py`<br>`engines/trainer-difusao/src/trainer_difusao/train.py` | **Permissões de root (UID 0).** Orchestrator injeta `--network` e volumes `/outputs/<job_id>`. Porém, sem flag de usuário (`--user`), containers escrevem artefatos como root no filesystem compartilhado do host. |
| **Coleta de Telemetria** | `services/orchestrator/src/app/mod.rs:866` (lê `metrics.jsonl`)<br>`app/mod.rs:630` (lê `telemetry.jsonl`) | `engines/engine-kit/src/engine_kit/telemetry.py:20-92`<br>`TelemetryEmitter` gera ambos | **Dívida de espelho legado.** Duplicidade de I/O em disco. Se uma engine desativar o espelho legado (`legacy_filename=None`), o coletor de jobs one-shot do orquestrador falha em reportar métricas em tempo real. |
| **Paridade de Mock Determinístico** | `services/api-principal/src/search/embed.rs:61-78`<br>`MockEmbedder` (Rust f32 raw) | `engines/engine-kit/src/engine_kit/mock.py:26-42`<br>`mock_vector` (Python float `round(x, 6)`) | **Divergência de Precisão Bitwise.** O mock Python aplica arredondamento em 6 casas decimais (`round(x / norm, 6)`), enquanto o mock Rust divide em `f64` e converte diretamente para `f32`. Ausência de teste de contrato automatizado entre os dois. |
| **Assinatura Mágica de Artefato** | `docs/adr/0007-jobs-v1.md:320`<br>Referência a `HEPHMOCK` (110B) | `engines/trainer-yolo/src/trainer_yolo/deterministic.py:12`<br>`MOCK_MAGIC = b"HEPHMOCK"` | **Fragmentação de padrão.** `HEPHMOCK` existe apenas no `trainer-yolo`. Não foi padronizado em `engine-kit` nem adotado pelos trainers de difusão ou CLIP. |

### Fronteira 4: Topologia de Rede, Infraestrutura e Segredos

| Aspecto Auditado | Evidência Infra/Compose | Evidência Código/Runtime | Diagnóstico & Risco |
|---|---|---|---|
| **Segredos em Produção** | `infra/compose.prod.yaml:1-36`<br>`infra/compose.yaml:38-46` | `services/api-principal/src/main.rs:161-179`<br>Bootstrap com fallback randômico | **Falta de Fail-Fast no Compose Prod.** `compose.prod.yaml` não anula os defaults de interpolação (`${VAR:-default}`) do `compose.yaml`. Sem `.env` configurado, serviços sobem com senhas fracas conhecidas (`changeme`, `studio`). |
| **Variáveis Fantasma** | `infra/compose.yaml:39`<br>`STUDIO_MASTER_KEY: ${STUDIO_MASTER_KEY:-changeme}` | `services/api-principal/src/` (zero referências a `STUDIO_MASTER_KEY`) | **Variável Morta.** `STUDIO_MASTER_KEY` é definida no compose mas não é consumida por nenhum componente do backend. |
| **Flags do Cookie de Sessão** | `infra/compose.prod.yaml:9-25`<br>Caddy escuta `:80` e `:443` | `services/api-principal/src/main.rs:194-196`<br>`SECURE_COOKIE` booleano estático | **Falta de inspeção dinâmica de HTTPS.** O BFF emite `Secure` no cookie apenas se `SECURE_COOKIE=true` for passado via env. Ele não lê o header `X-Forwarded-Proto: https` enviado pelo Caddy. |
| **Exposição LAN para Nós GPU** | `docs/infra/gpu-nodes.md:27-48`<br>`MANAGER_PUBLISH=0.0.0.0`, `SEAWEED_PUBLISH=0.0.0.0` | `infra/compose.yaml:17`<br>`ports: ${DB_PUBLISH:-127.0.0.1}:5432:5432` | **Vetor de sniffing LAN.** Tráfego HTTP do Manager e S3 corre sem cifra TLS na rede local. `compose.prod.yaml` fecha portas de `web` e `principal`, mas mantém `manager` e `db` abertos se variáveis forem alteradas. |

---

## 4. Grafo e Matriz de Dependência Intercamadas

A evolução arquitetural do monorepo não pode ocorrer de forma caótica ou com refatorações simultâneas que quebrem contratos mútuos. A ordem de execução obedece rigorosamente às seguintes restrições de causalidade:

```
                  ┌──────────────────────────────┐
                  │ Wave 0: Guardrails & Specs   │
                  │ (OpenAPI, Policies, Secrets) │
                  └──────────────┬───────────────┘
                                 │
                  ┌──────────────▼──────────────┐
                  │ Wave 1: Shared Core & DTOs  │
                  │ (heph-contracts, engine-kit)│
                  └──────────────┬───────────────┘
                                 │
                  ┌──────────────▼──────────────┐
                  │ Wave 2: Execution & Engine  │
                  │ (Orchestrator, Nodes, VRAM) │
                  └──────────────┬───────────────┘
                                 │
                  ┌──────────────▼──────────────┐
                  │ Wave 3: BFF, DB & Streaming │
                  │ (api-principal, SSE, S3)    │
                  └──────────────┬───────────────┘
                                 │
                  ┌──────────────▼──────────────┐
                  │ Wave 4: Frontend Web        │
                  │ (Types codegen, Pages, A11y)│
                  └──────────────┬───────────────┘
                                 │
                  ┌──────────────▼──────────────┐
                  │ Wave 5: Hermetic E2E & Gate │
                  │ (Cross-tests, Sweeper, CI)  │
                  └─────────────────────────────┘
```

### Matriz de Impacto Cruzado e Breaking Changes

| Iniciativa de Mudança | Camadas Impactadas | Potencial de Breaking Change | Ação Preventiva / Salvaguarda |
|---|---|:---:|---|
| **Alinhamento de `Job` e `JobParams` na OpenAPI** | Contracts, Services, Apps | **Alto** (Pode quebrar tipagem TypeScript do Studio) | Adicionar `totalSteps` e `totalEpochs` como opcionais; tipar `params` mantendo backward compatibility antes do codegen na Web. |
| **Normalização de `vram-table.yaml` (`diffusion` e hífen)** | Contracts, Services (Manager) | **Médio** (Afeta cálculo de elegibilidade na fila) | Corrigir chaves no YAML simultaneamente à atualização da leitura no `manager/src/lib.rs`. |
| **Criação de Crate Compartilhada `heph-contracts`** | Services (`api-principal`, `manager`, `orchestrator`) | **Baixo** (Refatoração puramente interna Rust) | Extração gradual mantendo aliases nos módulos legados até migração completa. |
| **Unificação `telemetry.jsonl` (Fim do `metrics.jsonl`)** | Engines, Services (Orchestrator) | **Alto** (Orquestrador pode parar de coletar métricas) | Orquestrador deve priorizar `telemetry.jsonl` com fallback automático para `metrics.jsonl` durante o período de transição. |
| **Isolamento de Redes no Compose e Non-Root Containers** | Infra, Engines, Services | **Alto** (Risco de erro `EACCES` em volumes de artefatos) | Ajustar permissões com `chown` no entrypoint ou padronizar UID:GID `1000:1000` em todos os Dockerfiles. |
| **Inspeção de `X-Forwarded-Proto` no Cookie de Sessão** | Services (BFF), Infra (Caddy) | **Baixo** (Melhoria transparente de segurança) | Ativar verificação condicional: se `X-Forwarded-Proto == https` OU `SECURE_COOKIE == true`, injetar `; Secure`. |

---

## 5. Roadmap Único Unificado em Ondas Sequenciais (Waves)

---

### Wave 0 — Fundação, Contratos Canônicos e Guardrails (Sem quebra de runtime)
*Meta:* Blindar os contratos de dados e tabelas de políticas antes de qualquer edição em serviços ou interfaces.

#### [RD-001] Padronização e Correção de Esquemas em `packages/contracts/openapi.yaml`
- **Camadas:** `Contracts`
- **Origem:** Fronteira 1 (Achado 2.1)
- **Pré-requisitos:** Nenhum
- **Proposta:**
  1. Incluir propriedades `totalSteps` (integer, null) e `totalEpochs` (integer, null) no schema `components.schemas.Job`.
  2. Tipar schemas dedicados para `DiffusionJobParams` e `YoloJobParams` sob `components.schemas`, referenciados em `Job.properties.params` via `anyOf`.
  3. Atualizar `orchestratorKind` enum para `[docker, slurm, local, remoto, "null"]`.
  4. Corrigir nulabilidade de `datasetId` em `Job` para `[string, "null"]`.
- **Critério de Aceite:** `cargo test -p api-principal --test contract` passa 100% verde; validação sintática e de `$ref` permanece sem erros.
- **Risco & Rollout:** Baixo (mudança puramente aditiva/corretiva no contrato).
- **Esforço & Subagente:** `P` — `@architect`

#### [RD-002] Normalização de Nomes e Entradas em `packages/policies/vram-table.yaml` e `engines.yaml`
- **Camadas:** `Contracts`, `Services`
- **Origem:** Achado 2.2 e 2.3
- **Pré-requisitos:** Nenhum
- **Proposta:**
  1. Substituir todas as ocorrências de `engine: difusao` por `engine: diffusion` em `vram-table.yaml` e `engines.yaml`.
  2. Padronizar o identificador do modelo FLUX-2 Klein 4B estritamente como `model: flux-2-klein-4b` em todas as entradas (`train` e `generate`).
  3. Adicionar entradas para modos auxiliares (`autolabel`, `autotracker`, `yolo:predict`).
  4. Eliminar entradas duplicadas (linhas 11 e 14 de `vram-table.yaml`).
- **Critério de Aceite:** Teste unitário de parse em `services/manager/src/lib.rs` valida todas as entradas sem nenhuma chave retornando `None` para os modelos oficiais.
- **Risco & Rollout:** Baixo.
- **Esforço & Subagente:** `P` — `@architect`

#### [RD-003] Fail-Fast de Segredos em Produção e Limpeza de Variáveis Fantasma
- **Camadas:** `Infra`
- **Origem:** Fronteira 4 (`tasks/specs/infra-autonomia.md:1.1, 1.3`)
- **Pré-requisitos:** Nenhum
- **Proposta:**
  1. Remover a variável morta `STUDIO_MASTER_KEY` do `infra/compose.yaml`.
  2. Em `infra/compose.prod.yaml`, utilizar sintaxe de interpolação obrigatória sem fallback default (ex.: `${STUDIO_PASSWORD:?Defina STUDIO_PASSWORD em producao}`) para variáveis críticas (`STUDIO_PASSWORD`, `AUTH_SECRET`, `POSTGRES_PASSWORD`, `MANAGER_TOKEN`, `S3_SECRET_KEY`).
  3. Garantir fechamento de portas de `db` e `manager` em `compose.prod.yaml` (`ports: !override []`).
- **Critério de Aceite:** Execução de `docker compose -f infra/compose.yaml -f infra/compose.prod.yaml config` sem `.env` falha imediatamente apontando as variáveis ausentes.
- **Risco & Rollout:** Médio (impede subida insegura; requer documentação explícita).
- **Esforço & Subagente:** `P` — `@infra-dev`

---

### Wave 1 — Pacotes e Bibliotecas Compartilhadas (Fundação de Código)
*Meta:* Estabelecer os blocos de construção comuns em Rust, Python e TypeScript para eliminar clones e divergências.

#### [RD-010] Extração da Crate Compartilhada de Contratos e DTOs (`packages/contracts-rs`)
- **Camadas:** `Services`, `Contracts`
- **Origem:** Fronteira 2 (`tasks/specs/backend-autonomia.md:1.8`)
- **Pré-requisitos:** `RD-001`
- **Proposta:**
  1. Criar crate interna no workspace Rust (`crates/heph-contracts` ou `packages/contracts-rs`).
  2. Centralizar structs wire compartilhadas entre BFF, Manager e Orchestrator: `PackageRef`, `MetricsItem`, `ReportBody`, `HeartbeatBody`, `JobDispatchPayload`, `JobTelemetryEvent`.
  3. Configurar serde com `rename_all = "camelCase"` para wire público e `snake_case` para protocolo interno entre serviços.
- **Critério de Aceite:** Workspace compila sem warnings com `cargo check --workspace`; eliminação de structs clonadas nos 3 serviços.
- **Risco & Rollout:** Baixo (refatoração interna com re-exports retrocompatíveis).
- **Esforço & Subagente:** `M` — `@rust-dev`

#### [RD-011] Consolidação e Centralização do `engine-kit` em Python
- **Camadas:** `Engines`
- **Origem:** Fronteira 3 (`tasks/specs/backend-autonomia.md:3.1`)
- **Pré-requisitos:** `RD-002`
- **Proposta:**
  1. Centralizar a constante mágica `MOCK_MAGIC = b"HEPHMOCK"` dentro de `engine_kit.mock`.
  2. Alinhar a precisão de `engine_kit.mock.mock_vector` eliminando o `round(..., 6)` para bater com a saída float `f32` exata de `services/api-principal/src/search/embed.rs`.
  3. Criar teste de ouro (golden test) determinístico para validar paridade bitwise entre Rust e Python.
  4. Padronizar `TelemetryEmitter` como emissor primário de `telemetry.jsonl`, mantendo espelhamento em `metrics.jsonl` apenas como fallback transitório com flag deprecada.
- **Critério de Aceite:** Suíte `pytest engines/engine-kit` passando com 100% de cobertura nos mocks e telemetria.
- **Risco & Rollout:** Médio (sensível a determinismo de embeddings).
- **Esforço & Subagente:** `M` — `@python-engines`

#### [RD-012] Configuração de Geração Automatizada de Tipos na Web (`openapi-typescript`)
- **Camadas:** `Apps`
- **Origem:** Fronteira 1 (`tasks/web-modularizacao-auditoria.md:TASK-WEB-014`)
- **Pré-requisitos:** `RD-001`
- **Proposta:**
  1. Adicionar `openapi-typescript` às devDependencies de `apps/web/package.json`.
  2. Criar script `npm run codegen:contracts` na raiz e no workspace `web` lendo `packages/contracts/openapi.yaml` e gerando `apps/web/types/api-generated.ts`.
  3. Fazer com que `apps/web/types/index.ts` estenda e valide os tipos manuais contra as interfaces geradas.
- **Critério de Aceite:** Execução de `npm run codegen:contracts` gera tipos idênticos à OpenAPI; `tsc --noEmit` em `apps/web` sem erros.
- **Risco & Rollout:** Baixo (não altera código em produção).
- **Esforço & Subagente:** `P` — `@frontend-dev`

---

### Wave 2 — Camada Interna de Execução e Orquestração (Nós e Infraestrutura)
*Meta:* Garantir despacho sem perdas, orquestração resiliente e execução segura em containers GPU.

#### [RD-020] Correção do Repasse de `control_package_ref` no Dispatch do Manager
- **Camadas:** `Services`
- **Origem:** Fronteira 2 (Achado da Auditoria)
- **Pré-requisitos:** `RD-010`
- **Proposta:**
  1. No loop `dispatch_next` em `services/manager/src/lib.rs:3948-4008`, extrair `control_package_ref` de `params` (mesmo padrão adotado para `custom_checkpoint` e `text_encoder_ref`).
  2. Injetar `dispatch_body["control_package_ref"] = cp` quando presente.
  3. Adicionar teste de integração no manager validando que jobs com dataset de controle geram payload de dispatch com a chave `control_package_ref` preenchida.
- **Critério de Aceite:** Teste unitário e de integração em `manager` comprovando despacho íntegro de `control_package_ref`.
- **Risco & Rollout:** Baixo (corrige funcionalidade quebrada).
- **Esforço & Subagente:** `P` — `@rust-dev`

#### [RD-021] Ajuste Semântico de Abort e Máquina de Estados no Orchestrator
- **Camadas:** `Services`
- **Origem:** Fronteira 2 (`tasks/specs/backend-autonomia.md:1.1, 1.7`)
- **Pré-requisitos:** `RD-010`
- **Proposta:**
  1. No `abort_handler` em `services/orchestrator/src/server/handlers.rs:179-203`, quando um job solicitado para abort não for encontrado em `active_jobs`, verificar se já houve report prévio. Se inexistente ou pendente, reportar status `cancelled` (e não `failed`).
  2. Implementar callback `POST /internal/jobs/:id/prepare-cancel` no BFF e Manager para cancelar graciosamente tarefas abortadas na fase `preparing`.
- **Critério de Aceite:** Testes em `orchestrator` e `manager` cobrindo aborts em `preparing`, `running` e corrida com job já finalizado.
- **Risco & Rollout:** Baixo.
- **Esforço & Subagente:** `M` — `@rust-dev`

#### [RD-022] Unificação de Coleta de Telemetria no Orchestrator (Migração para `telemetry.jsonl`)
- **Camadas:** `Services`, `Engines`
- **Origem:** Fronteira 3 (`tasks/specs/backend-autonomia.md:3.2`)
- **Pré-requisitos:** `RD-011`
- **Proposta:**
  1. No coletor de métricas do orchestrator (`src/app/stages/collector.rs` e `src/app/mod.rs:866`), passar a monitorar primordialmente `telemetry.jsonl`.
  2. Manter fallback transparente: se `telemetry.jsonl` não existir após 5s de execução, ler `metrics.jsonl`.
  3. Unificar o parser de eventos JSONL para utilizar o mesmo modelo em jobs one-shot e no daemon de difusão.
- **Critério de Aceite:** Jobs one-shot de treino (YOLO e difusão) transmitem telemetria contínua consumindo `telemetry.jsonl`.
- **Risco & Rollout:** Baixo.
- **Esforço & Subagente:** `M` — `@rust-dev`

#### [RD-023] Execução de Containers como Usuário Não-Privilegiado (Non-Root)
- **Camadas:** `Engines`, `Infra`, `Services`
- **Origem:** Fronteira 3 (`tasks/specs/infra-autonomia.md:2.2`)
- **Pré-requisitos:** `RD-011`
- **Proposta:**
  1. Atualizar Dockerfiles das engines (`engines/*/Dockerfile*`) criando usuário e grupo de sistema `studio:studio` (UID 1000, GID 1000).
  2. No orchestrator (`services/orchestrator/src/adapters/executor_docker.rs`), injetar `--user 1000:1000` na construção dos argumentos de `docker run`.
  3. Ajustar scripts de inicialização de volumes para assegurar propriedade correta de diretórios em `/data`.
- **Critério de Aceite:** `docker run` executa com sucesso; arquivos gravados em `/data/outputs` pertencem ao UID 1000 e não a `root`.
- **Risco & Rollout:** Médio (pode exigir ajustes em permissões de volumes existentes em ambientes dev).
- **Esforço & Subagente:** `M` — `@infra-dev`

---

### Wave 3 — Borda e Serviços de Aplicação (BFF e Domínio)
*Meta:* Estabilizar endpoints, persistência relacional, fluxo de streaming SSE e proxies reversos.

#### [RD-030] Otimização de SSE e Flushing Imediato no Ingress Caddy
- **Camadas:** `Infra`
- **Origem:** Fronteira 1 (Achado da Auditoria)
- **Pré-requisitos:** `RD-003`
- **Proposta:**
  1. Em `infra/Caddyfile`, configurar flush imediato para rotas de eventos e telemetria:
     ```caddy
     handle /api/jobs/*/events {
         reverse_proxy principal:8080 {
             flush_interval -1
         }
     }
     handle /api/* {
         reverse_proxy principal:8080
     }
     ```
  2. Assegurar que `encode zstd gzip` não comprima fluxos com `Content-Type: text/event-stream`.
- **Critério de Aceite:** EventSource no frontend recebe chunks de telemetria em tempo real atrás do Caddy sem retenção de buffer.
- **Risco & Rollout:** Baixo.
- **Esforço & Subagente:** `P` — `@infra-dev`

#### [RD-031] Detecção Dinâmica de HTTPS para Cookie de Sessão no BFF
- **Camadas:** `Services`
- **Origem:** Fronteira 4 (Achado da Auditoria)
- **Pré-requisitos:** Nenhum
- **Proposta:**
  1. Em `services/api-principal/src/auth/handlers.rs`, verificar se o header `X-Forwarded-Proto` possui valor `https`.
  2. Emitir o atributo `; Secure` no cookie `heph_session` se `state.secure_cookie == true` OU se a requisição chegar via HTTPS / proxy TLS.
- **Critério de Aceite:** Requisições via Caddy TLS recebem cookie com flag `Secure` automaticamente, mesmo sem `SECURE_COOKIE=true` explícito.
- **Risco & Rollout:** Baixo.
- **Esforço & Subagente:** `P` — `@rust-dev`

#### [RD-032] Robustez do Ciclo Assíncrono de Preparação (`job_prepares` e Retries)
- **Camadas:** `Services`
- **Origem:** `tasks/specs/backend-autonomia.md:1.2, 1.4`
- **Pré-requisitos:** `RD-010`
- **Proposta:**
  1. Adicionar rotina periódica em background no `api-principal` para executar `recover_stale_prepares` a cada 60s, evitando que falhas de processo tranquem a chave única de deduplicação (`fingerprint`).
  2. Implementar retries com backoff exponencial no cliente HTTP do BFF ao despachar chamadas `prepare_complete` e `prepare_fail` para o Manager.
- **Critério de Aceite:** Teste de simulação de queda do manager durante preparação não gera descarte indevido de pacotes construídos.
- **Risco & Rollout:** Médio.
- **Esforço & Subagente:** `M` — `@rust-dev`

---

### Wave 4 — Frontend e Interface do Usuário (Apps / Web)
*Meta:* Desacoplar estado de visualização, eliminar duplicidades de parâmetros e alinhar types ao OpenAPI gerado.

#### [RD-040] Limpeza de Parâmetros Duplicados e Tipagem Estrita em `apps/web`
- **Camadas:** `Apps`
- **Origem:** Fronteira 1 (`tasks/web-modularizacao-auditoria.md:TASK-WEB-014`)
- **Pré-requisitos:** `RD-012`
- **Proposta:**
  1. Eliminar a duplicidade de chaves em `DiffusionJobParams` (`apps/web/types/jobs.ts`), mantendo exclusivamente o formato canônico camelCase definido no contrato OpenAPI.
  2. Ajustar os componentes de forja e submissão (`ForjaDifusaoSetup.tsx`, `ForjaYoloSetup.tsx`) para enviar objetos em estrito camelCase, eliminando fallbacks em snake_case.
  3. Atualizar a tipagem de `datasetId` para `string | null` em componentes que manipulam o ciclo de vida do job.
- **Critério de Aceite:** `npm run build --workspace=web` verde; eliminação dos casts `as any` na montagem de payloads de jobs.
- **Risco & Rollout:** Médio (exige conferência de todas as chamadas de jobs no frontend).
- **Esforço & Subagente:** `M` — `@frontend-dev`

#### [RD-041] Quebra e Desacoplamento dos Componentes Monolíticos Restantes
- **Camadas:** `Apps`
- **Origem:** `tasks/web-modularizacao-auditoria.md` (Fases 4 e 5)
- **Pré-requisitos:** `RD-040`
- **Proposta:**
  1. Modularizar `app/(studio)/jobs/page.tsx` (1.598 linhas) extraindo subcomponentes de tabela, paginação e drawer de logs.
  2. Modularizar `components/studio/GenerationGallery.tsx` (1.017 linhas) em componentes dedicados de grid, visualizador e ações em lote.
  3. Incorporar acessibilidade estrita (focus trap e scroll lock) em todos os novos diálogos e modais.
- **Critério de Aceite:** Nenhum arquivo TSX em `apps/web` excede 600 linhas; conformidade estrita com Biome e Next.js 16 App Router.
- **Risco & Rollout:** Baixo (refatoração interna de UI mantendo layout visual do Design System).
- **Esforço & Subagente:** `G` — `@frontend-dev`

---

### Wave 5 — Observabilidade, E2E e Limpeza Final
*Meta:* Blindar o monorepo com suíte ponta a ponta hermética e remover código morto e shims transitórios.

#### [RD-050] Teste de Integração Hermético Cruzado (Golden Parity & Mock Pipeline)
- **Camadas:** `Services`, `Engines`, `Contracts`
- **Origem:** Fronteiras 2 e 3
- **Pré-requisitos:** `RD-011`, `RD-020`, `RD-022`
- **Proposta:**
  1. Criar teste de integração hermético executado no CI que valida:
     - Criação de dataset sintético no BFF.
     - Submissão de job YOLO e Difusão em modo mock (`ENGINE_MOCK=1`).
     - Preparação assíncrona, reporte de fase e despacho para orquestrador mock.
     - Emissão de telemetria e leitura até transição para `done`.
     - Coleta e validação de artefatos assinados com `HEPHMOCK`.
  2. Implementar teste de paridade automatizado comparando a saída vetorial de `MockEmbedder` (Rust) contra `mock_vector` (Python) para uma lista de 50 strings e payloads binários determinísticos.
- **Critério de Aceite:** Teste executa em ambiente CPU-only e passa 100% verde sem requerer serviços externos de GPU.
- **Risco & Rollout:** Baixo.
- **Esforço & Subagente:** `M` — `@rust-dev`

#### [RD-051] Limpeza de Código Morto, Shims Transitórios e Sincronização Final de Docs
- **Camadas:** `Apps`, `Services`, `Engines`, `Infra`, `Contracts`
- **Origem:** Todas as auditorias
- **Pré-requisitos:** Todas as tarefas anteriores concluídas
- **Proposta:**
  1. Remover arquivos legados e shims marcados como obsoletos em todas as camadas.
  2. Atualizar a documentação modular em `docs/` (`docs/REPO_MAP.md`, `docs/services/`, `docs/engines/`, `docs/web/`, `docs/infra/`) espelhando as novas crates e fluxos reais.
  3. Registrar o encerramento do roadmap no `tasks/backlog.md` e na memória ativa `tasks/active.md`.
- **Critério de Aceite:** `graft check_freshness` sincronizado; zero referências quebradas ou código zumbi no repositório.
- **Risco & Rollout:** Baixo.
- **Esforço & Subagente:** `P` — `@docs-sync`

---

## 6. Guia de Implementação e Guardrails para o Coordenador (`AGENTS.md`)

O coordenador do Hephaestus deve observar os seguintes guardrails ao despachar fatias deste roadmap:

1. **Inviolabilidade da Sequência de Ondas:** Nenhuma tarefa de uma onda superior pode ser iniciada antes que todas as dependências duras da onda anterior estejam mescladas na branch `develop` ou validadas verdes.
2. **Propriedade Disjunta de Arquivos:** Ao despachar implementadores em paralelo (ex.: `@rust-dev` no backend e `@frontend-dev` na web), certificar-se de que os conjuntos de arquivos editados sejam estritamente disjuntos. Modificações em `packages/contracts/` ou migrations SQL devem ser commitadas previamente de forma sequencial.
3. **Regra das Duas Correções:** Se um subagente falhar duas vezes consecutivas no mesmo erro de build, teste ou lint, interromper imediatamente a execução, registrar o bloqueio na memória ativa e assumir a intervenção cirúrgica com `@fixer`.
4. **Validação Contínua com Graft:** Após a conclusão de cada Wave, executar `graft build` para reindexar o grafo semântico de símbolos e verificar drift arquitetural antes de liberar a próxima fase.
5. **Critério Binário de Conclusão de Onda:** Uma onda só é considerada fechada quando os comandos de verificação de todas as camadas impactadas passarem integralmente:
   ```bash
   cargo check --workspace && cargo fmt --all -- --check && cargo test --workspace
   npm run build --workspace=web && npm run lint --workspace=web
   python -m compileall engines/*/src
   cd engines/<engine> && uv run pytest
   docker compose -f infra/compose.yaml config -q
   ```
