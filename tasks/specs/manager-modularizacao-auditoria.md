# Auditoria Técnica e Plano de Modularização: `services/manager`

**Projeto:** Hephaestus LLM Studio
**Data:** 20 de Setembro de 2026
**Escopo:** `services/manager` (:8081) — fila/estado de jobs, nós GPU, telemetria, watchdogs, dispatch. Somente leitura; precedentes: `tasks/specs/api-principal-modularizacao-auditoria.md` e `tasks/specs/orchestrator-modularization.md`.
**Método:** 6 varreduras paralelas (M1 ciclo de vida de jobs, M2 nós/VRAM, M3 transporte/contrato, M4 bootstrap/async, M5 banco/migrations, M6 testes/dup/segurança) + validação por amostragem pelo coordenador (toda linha citada abaixo foi aberta; divergências entre varreduras resolvidas contra o código).

---

## 1. Resumo Executivo & Métricas

O manager é o serviço mais **funcionalmente rico** e o mais **estruturalmente atrasado** dos três: contém a máquina de estados real dos jobs, os watchdogs anti-zumbi, o scheduler com VRAM e a defesa `no_artifacts` — decisões maduras e testadas — mas tudo dentro de **3 arquivos**: `src/lib.rs` (4.625 linhas, módulo plano único), `src/main.rs` (1.185) e `tests/manager_db.rs` (7.473). É o único serviço que **não** seguiu a refatoração em camadas que o orchestrator já demonstra (`ports/adapters/app/domain`) nem a organização por feature do api-principal.
Os riscos concentrados não são de estilo, são de **concorrência e consistência**: o dispatch commita `dispatched` antes do POST ao nó e, se a rede cair, reverte em spin-loop de 2s sem backoff nem contador (pode travar um nó para sempre); `abort_job` lê o status sem lock (job cancelado pode continuar queimando GPU); um job que entra em `cancelling` sem resposta do nó **trava o nó permanentemente** (a query de elegibilidade exclui nós com jobs `cancelling`); `get_telemetry` segura o read-lock do cache através de `.await` de SQL (latência do Postgres bloqueia heartbeats de todo o cluster em cascata); e o `/internal/adopt` envia o **`MANAGER_TOKEN` mestre** para qualquer URL com prefixo `http://` (SSRF com vazamento de credencial).

Saúde quantitativa: `cargo check` limpo; `cargo clippy` 14 warnings; **2** `unwrap/expect` fora de teste em `lib.rs` e **7** em `main.rs` (todos no boot, fail-fast — aceitáveis, mas desestruturados); 0 `unsafe`; 0 TODO/FIXME; 91+2 queries sqlx **todas** runtime-checked (zero macros `query!`); ~83% dos structs de wire entre manager e api-principal duplicados (10 de 12); **zero rotas do manager no `openapi.yaml`** — ao contrário do que a doc afirma.

### Métricas por arquivo

| Arquivo | Linhas | Problema central |
|---|---|---|
| `src/lib.rs` | 4.625 | DTOs + domínio + SQL + watchdogs + scheduler + 35 testes num único módulo plano; 4 funções >250 linhas |
| `src/main.rs` | 1.185 | 24 handlers + middleware + bootstrap + 7 testes; `main()` com 130 linhas |
| `tests/manager_db.rs` | 7.473 | 143 testes de integração serializados por lock global; maior teste com 224 linhas |

### Top 12 funções >80 linhas (produção)

| # | Função | Local | Linhas | Responsabilidades misturadas |
|---|---|---|---|---|
| 1 | `report_job` | `lib.rs:1583-2133` | 550 | guarda de transição + UPDATE status + métricas append/dedup + artefatos (2 queries por item) + hook `models` + hook `generations` + merge JSONB de erro + tx só no `done` |
| 2 | `create_job` | `lib.rs:493-956` | 463 | validação de payload + resolução weights/LoRA (N+1) + checkpoint/text-encoder/init-image + derivação de nome + mutação `generation_inputs` + INSERT sem tx + posição de fila |
| 3 | `dispatch_next` | `lib.rs:3761-4021` | 260 | tx + SKIP LOCKED + VRAM + hint com fallback + UPDATE `dispatched` + **commit antes do HTTP** + parsing manual de ~8 campos JSONB + payload `json!` + compensação |
| 4 | `get_telemetry` | `lib.rs:2224-2360` | 137 | read-lock vivo sobre 3 fallbacks SQL + agregação manual $O(N^2)$ de GPUs + `env::var` em loop |
| 5 | `main` | `main.rs:876-1005` | 130 | tracing + 9 env com expect + retry loop do pool + adopt + recovery + leitura síncrona da VRAM table + spawn sem handle + bind + serve |
| 6 | `list_jobs` | `lib.rs:963-1079` | 117 | varredura completa da fila p/ posições + SQL dinâmico com `bind_idx` manual + `fetch_all` **sem LIMIT** |
| 7 | `watchdog_tick` | `lib.rs:3530-3608` | 86 | degraded + cancel de `cancelling` + offline+requeue + prepare-timeout + **GC de 7 dias a cada 2s** |
| 8 | `adopt_internal` | `lib.rs:3628-3711` | 84 | validação + HTTP com token mestre (SSRF) + INSERT ON CONFLICT sem RETURNING + SELECT de id redundante + DTO com campos fake |
| 9 | `abort_job` | `lib.rs:1190-1272` | 83 | leitura de status **sem lock** + UPDATE sem guarda no WHERE + retry de notificação só para `running` |
| 10 | `list_generations` | `lib.rs:3381-3465` | 80 | SQL dinâmico com índices interpolados `${limit_idx}` |
| 11 | `receive_heartbeat` | `lib.rs:2136-2214` | 79 | fronteira; lock write só no fim (correto) |
| — | `dispatch_next` args | `lib.rs:3761-3768` | 6 args | violação do teto de 5; callers: `main.rs:474` **dentro de handler HTTP** e `main.rs:981` |

### Contagens (verificadas)

| Métrica | lib.rs | main.rs | manager_db.rs |
|---|---|---|---|
| `unwrap()/expect()` fora de teste | 2 (`:300`, `:2251`) | 7 (`:887,892,900,937,943,1002,1004`) | — (231/413 em testes) |
| `panic!` fora de teste | 0 | 2 (`:937,943`) | 2 |
| `unsafe` / TODO/FIXME | 0 / 0 | 0 / 0 | 0 / 0 |
| Queries sqlx runtime | 91 | 2 | 153 |
| Macros `query!` | **0** | **0** | **0** |
| `tokio::spawn` | 0 | 1 (`:971`, handle descartado) | 0 |
| Transações (`pool.begin`) | **4** (`:1692,:2920,:3010,:3770`) | 0 | — |
| Escrita de `status` de job (SQL) | **18 sítios** (`:1208,1217,1226,1335,1388,1415,1454,1640,2072,2097,2116,3357,3366,3561,3578,3879,3890,4012`) | 0 | — |
| Testes | 35 | 7 (2 de rota, ambos `#[ignore]`) | 143 |

**Duplicação estimada:** ~83% dos wire structs manager↔BFF duplicados (§4); ~600-700 linhas de código do manager têm gêmeo funcional em outro serviço (DTOs, `slugify`, normalização de arch, envelope de erro, GC, harness de teste) ≈ **5-6%** das 13,3k linhas. `jscpd` **não instalado** no ambiente (instalar é proibido pela regra do skill) — roda-lo no CI fica como recomendação R-01.

---

## 2. Divergências entre Documentação e Código Real

`docs/services/manager.md` tratado como hipótese; `tasks/backend-autonomia.md` e `docs/PITFALLS.md` confrontados quando relevantes:

| # | Doc afirma | Código real | Evidência |
|---|---|---|---|
| D-01 | Ciclo `[queued] → [preparing]` | **Invertido**: job entra direto em `preparing` na criação e vai a `queued` no prepare-complete; nunca há `queued→preparing` | `lib.rs:910-916` ("package presente sempre vence → queued"), `lib.rs:1334-1337` |
| D-02 | "Rotas internas e modelos de dados estão referenciados em `packages/contracts/openapi.yaml`" | **FALSO**: zero paths `/internal/*` no yaml (só 2 menções em prosa de descrição); as 24 rotas existem apenas no código | `packages/contracts/openapi.yaml:2803,2872` (comentários); router em `main.rs:783-842` |
| D-03 | "Nós sem heartbeat recente (> 10s) são considerados stale e **não recebem novos dispatches**" | **VIOLADO**: dispatch só exige `status='online'` no banco, que degrada aos 15s via watchdog e não consulta `last_heartbeat`; janela de 5-15s despacha para nó morto pela telemetria | `lib.rs:3804,3822` (WHERE `o.status='online'`), `lib.rs:3542-3544`, `lib.rs:3534` (15s) |
| D-04 | "armazena o status em cache de telemetria" | Cache é **puramente em memória** e não é reconstruído no boot; `list_orchestrators` nem seleciona a coluna `gpus` do banco → nós zeram telemetria visível até novo heartbeat; entradas de nós revogados nunca saem do mapa | `lib.rs:383,2436-2506,3718-3729` |
| D-05 | prepare_timeout "60 minutos" | Verdade, mas **hardcoded no SQL** (`interval '60 minutes'`), âncora é `created_at` e não `updated_at` (o `docs/PITFALLS.md:44` alega `updated_at` — doc de armadilha também divergente) | `lib.rs:1454-1457` |
| D-06 | "O job é protegido contra execução prematura antes da finalização do pacote" | Respeitado no dispatch (só pega `queued`); mas `abort_job` pode cancelar um job já `dispatched` **sem notificar o nó** (execução prematura do outro lado: zumbi pós-cancelamento) | `lib.rs:1207-1215` |
| D-07 | Manager "não é exposto à internet pública" | Verdade em prod (`compose.prod.yaml:74` `ports: !override []`, token obrigatório `:?`, `ENVIRONMENT=production`); em dev publica `127.0.0.1:8081` por default, mas o fluxo de nó GPU (TrueNAS→`10.15.10.3:8081`) **exige** `MANAGER_PUBLISH=0.0.0.0` → o manager fica acessível na LAN inteira protegido por `==` e default `changeme` | `infra/compose.yaml:162,167`; `infra/compose.gpu.yaml:7,32` |
| D-08 | Watchdogs de reconciliação cobrem falhas parciais | Há **3 estados de reconciliação não cobertos**: (a) `cancelling` sem resposta do nó nunca expira (e trava o nó para dispatch), (b) `dispatched` sem report (falha entre commit e POST) só sai no próximo boot via `recover_jobs`, (c) falha da query de compensação pós-HTTP deixa `dispatched` órfão com nó online | `lib.rs:4011-4019`, `lib.rs:3354-3374` (só no boot), `lib.rs:3815,3838` (filtro `cancelling`) |

---

## 3. Arquitetura-Alvo

Partindo do que já existe — o repo **já provou** dois padrões: o das camadas `ports/adapters/app/domain` do orchestrator (`services/orchestrator/src/lib.rs:9-17`) e o das feature-slices com ports do api-principal. O manager adota o segundo (é um serviço de estado com banco, como o BFF), com o primeiro como régua de disciplina de porta.

### 3.1 Dentro de `services/manager/src/` (padrão por fatia)

```
main.rs                  (~60 linhas: boot::run().await)
boot/    config.rs ManagerConfig tipado (14 envs, lidos 1x) · tracing.rs · router.rs · workers.rs
                         (dispatch-loop, watchdog, gc em tickers próprios com CancellationToken + JoinSet,
                          with_graceful_shutdown)
jobs/    state.rs JobStatus enum + apply_transition(tx, id, from→to) — ÚNICO dono de UPDATE de status
         create.rs · report.rs (guardas | métricas | artefatos | hooks models/generations) · queue.rs
         lifecycle.rs (abort/delete/cleanup com UPDATE guardado) · repository.rs (sqlx da fatia)
nodes/   heartbeat.rs · telemetry.rs (aggregate_telemetry() pura — mata o clone de teste M2-09) · adopt.rs
         (SSRF-safe, sem token no verify) · watchdog.rs (transições de nó + timeout de cancelling)
scheduling/ scheduler.rs (dispatch_next ≤100 linhas, monta heph_contracts::DispatchRequest tipado,
              attempt+backoff, compensação em tx) · vram.rs (tabela + fail-safe default_train_gb) · images.rs
models/  · generations/   CRUD + repository por fatia
http/    error.rs impl IntoResponse para ManagerError (mapeamento único) · extract.rs AppJson<T>
         routes jobs.rs/nodes.rs/models.rs/generations.rs (≤60 linhas/handler, zero SQL)
```

### 3.2 Máquina de estados (o coração)

`JobStatus` (8 estados do CHECK `0006_jobs.sql:32-34`) vira enum **em `heph-contracts`** com `is_terminal()`, `can_transition_to()` e serialização idêntica ao wire atual (`serde rename` = strings do banco). Hoje são **18 sítios de escrita** com string literal, guards reimplementados (`lib.rs:1598,1609,1629` no report; `lib.rs:1334-1416` no prepare — estes, bem feitos: UPDATE com guarda no WHERE = compare-and-set atômico) e um abort **sem** guarda (`lib.rs:1208-1215`). A régua interna a generalizar é o próprio `prepare_complete`: `UPDATE ... WHERE id=$1 AND status='preparing'` + `rows_affected` → `Conflict`.

### 3.3 Camada de dados

- `repository.rs` por fatia com as queries **movidas sem reescrita**; structs `#[derive(FromRow)]` (`JobDbRow` etc.) matam as tuplas de 12 campos (clippy type_complexity ×7) e o mapeamento manual de 22 `r.get()` em `get_job` (`lib.rs:1095-1109`).
- Macros `query!` **não** adotadas (build offline sem DATABASE_URL no CI — decisão já registrada no auditoria-irmã T-02; manter).
- Transações: hoje só 4 pontos (`:1692,:2920,:3010,:3770`). Envolver: `create_job` inteiro (INSERT job + `generation_inputs.used_at` — `lib.rs:823,918`), os ramos `failed/cancelled` do report (2 queries soltas `:2088-2123`), e cada par CTE do watchdog que muta nós+jobs (statements separados hoje).

### 3.4 Crates compartilhadas (candidatos reais, sem inventar crate nova)

| Crate | Conteúdo a mover/de criar | O que migra do manager |
|---|---|---|
| `heph-contracts` (existe) | `JobStatus`; `PackageRef` unificado (`version_id: Option`); wire responses `JobRow/TelemetryResponse/OrchestratorItem/ModelItem/CreateJobResponse/AbortResponse/StorageUsageResponse`; consts de paginação; `DispatchRequest` **já existe e é ignorado** | `lib.rs:54-272,2411-2531,3322-3326`; payload `json!` de `lib.rs:3956-3983` |
| (sem crate nova) envelope de erro | copiar o shape do `api-principal/src/error.rs:14-22` (ErrorBody tipado, códigos em inglês) | `main.rs:71-118` |
| `heph-contracts` utils (opcional) | `slugify`, `DiffusionArch`+aliases | `lib.rs:3063-3080,3101-3112` vs `datasets/models.rs:49-65`, `models/validate.rs:223`, `jobs/models.rs:732` |

**Não** criar: crate de "domínio compartilhado" com lógica de fila, abstração multi-backend de storage, ou `JobRepository` como trait dinâmico (a única porta que vale hoje é a que já existe — `OrchestratorClient` trait, `lib.rs:279-286`).

---

## 4. Matriz de Duplicação entre Serviços

| # | O que | Onde (2+ lugares) | Para onde | Tarefa |
|---|---|---|---|---|
| X-01 | Protocolo dispatch (payload completo) | manager monta `json!` solto (`lib.rs:3956-3983`) × `heph_contracts::DispatchRequest` (`dispatch.rs:24-44`) × orchestrator **usa o tipo do crate** (`domain/models.rs:3-5`) | manager passa a instanciar `DispatchRequest` | TASK-MGR-001 |
| X-02 | `PackageRef` | `lib.rs:79-84` (com `version_id`) × `dispatch.rs:4-9` (sem) — o dispatch serialize um e o outro serviço desserializa outro; drift só não mordeu porque o wire é `json!` | crate unifica com `version_id: Option` | TASK-MGR-001 |
| X-03 | `WeightsRef` | `lib.rs:54-57` ≡ `dispatch.rs:12-16` (idêntico, vivo 2×) | crate | TASK-MGR-001 |
| X-04 | `JobRow` ↔ `InternalJob` | `lib.rs:126-158` (24 campos) ≡ `api-principal/src/jobs/manager_client.rs:49-84` (23-24 campos) | `heph-contracts::jobs` (1 definição) | TASK-MGR-001 |
| X-05 | `TelemetryResponse` ×3 | `lib.rs:180-189` × `manager_client.rs:103-116` × `api-principal/jobs/handlers.rs:265-280` + cópia manual campo-a-campo em `handlers.rs:795-805` | crate + `#[serde(rename_all="camelCase")]` | TASK-MGR-001 |
| X-06 | `OrchestratorItem/ModelItem/StorageUsageResponse/CreateJobResponse/AbortResponse/ArtifactRow` | `lib.rs:2411-2531,3322-3326,119-123,174-177,160-167` × `manager_client.rs:94-210` | crate | TASK-MGR-001 |
| X-07 | GC de `dataset_versions` (mesma DELETE pesada) | `lib.rs:1477-1489` × `api-principal/src/storage/gc.rs:45-54` — **donos concorrentes apagando a mesma tabela** | dono único: api-principal (a migration `0006` anota `-- Dono: principal`); apagar do manager | TASK-MGR-008 |
| X-08 | `slugify` | `lib.rs:3063-3080` × `datasets/models.rs:49-65` (+ testes idênticos) | crate compartilhada (função pura) | TASK-MGR-010 |
| X-09 | Aliases/normalização de arch difusão | `lib.rs:3101-3112` × `models/validate.rs:223` × `jobs/models.rs:732` | enum `DiffusionArch` na crate | TASK-MGR-010 |
| X-10 | Envelope de erro `{code,message}` | `main.rs:71-118` (json! dinâmico, mensagens PT) × `api-principal/error.rs:14-22` (struct, EN) × `orchestrator/server/middleware.rs:13` | formato canônico + ErrorBody tipado | TASK-MGR-005 |
| X-11 | Harness anti-footgun de banco de teste (`assert_test_db_url` + guarda) | `main.rs:1055-1062` ≡ `tests/manager_db.rs:25-46` (+ padrão similar no BFF) | `tests/common/mod.rs` | TASK-MGR-010 |
| X-12 | Defaults de env duplicados manager×compose | `/data`, `docker`, `:local`, `10s/15s/60s` em `main.rs:893-898`, `lib.rs:2220,3534,3538` × `infra/compose.yaml:162-170` | config tipada + compose como único default (deixar env explícito) | TASK-MGR-006 |
| X-13 | Clamp de paginação `limit.clamp(1,200)` | `main.rs:730-734` × `manager_client.rs:831-832` | consts na crate | TASK-MGR-007 |

Re-export parcial atual do manager: só `ArtifactItem`, `HeartbeatBody`, `ReportBody` (`lib.rs:170-172`) — 3 de ~15 tipos de fronteira.

---

## 5. Verificação dos Invariantes

| Invariante | Veredito | Evidência |
|---|---|---|
| **INV-1** Máquina de estados queued→preparing→running→done/failed/cancelled | **PARCIALMENTE VIOLADO** | Transições guardadas onde importa: terminais imutáveis (`lib.rs:1598-1606`), `cancelling` só aceita_done/failed/cancelled (`:1609-1619`), sem regressão de ciclo (`:1629-1637`), prepare_* com compare-and-set (`:1334,1388,1415`) — **mas**: sem tipo (String em 18 sítios de escrita), `abort_job` sem lock nem guarda no UPDATE (`:1196-1215` → GPU fantasma), `dispatched` cancelado não notifica o nó (`:1207-1215`), hotfix 4bfb450 (`cancelled` no report, `:2116-2121`, testado em `manager_db.rs:812+`) foi pontual e não centralizada. Doc com seta invertida (D-01). TASK-MGR-003/004 |
| **INV-2** Watchdogs: heartbeat stale >10s, prepare_timeout 60min, reconciliação de órfãos, tempos p/ transferências pesadas | **PARCIALMENTE VIOLADO** | Os 3 watchdogs existem: `receive_heartbeat`+`node_stale_timeout_secs` (`:2136,2216`), `watchdog_prepare_timeout` 60min (`:1452-1463`), `watchdog_tick` degraded→offline→requeue com `queue_reason='recovered'` (`:3541-3591`), `recover_jobs` no boot (`:3354-3374`, `main.rs:927`). Violações: dispatch ignora frescor e usa régua de 15s (D-03); `cancelling` nunca expira e trava nó (D-08a); `dispatched` órfão só resolvido em restart (D-08b); GC 7d correndo no tick de 2s (`:3600-3605`); 60min hardcoded na string SQL (D-05); CTEs de duas mutações sem tx entre statements (`:3555-3585`) |
| **INV-3** Manager não exposto publicamente; só BFF + nós via MANAGER_TOKEN | **RESPEITADO EM PROD / FRÁGIL FORA DELA** | Entrada: `auth_middleware` cobre todo `/internal/*`, só `/health|/ready|/metrics` públicos (`main.rs:783-842`); prod fecha ports e exige token (`compose.prod.yaml:71-74`). Fragilidades: bind `0.0.0.0` fixo (`main.rs:1000`); fluxo GPU publica na LAN (D-07); comparação `==` não-constant-time com `format!` por request (`main.rs:175`); default dev `manager-dev-token`/`changeme` (`main.rs:863-871`, `compose.yaml:167`); **saída**: adopt envia o token mestre a URL arbitrária (`lib.rs:3639` valida só prefixo; `lib.rs:328-330` anexa `Bearer`). TASK-MGR-009 |
| **INV-4** Orchestrator stateless / kill_on_drop / TTL daemon | **N/A (lado do nó)** | Coberto em `tasks/specs/orchestrator-modularization.md`; o que cabe ao manager é manter o protocolo stateless ao reiniciar: **respeitado** (recupera via `recover_jobs` no boot `main.rs:927`) |
| **INV-5** Upload chunked / SSE | **N/A** | Serviços BFF; ver auditoria-irmã §5 |

Nenhum invariante merece mudança de intenção — todos os achados são de **fechamento** da implementação atual.

---

## 6. Tabela Geral de Achados

Impacto A/M/B · Esforço P/M/G · Pri P0–P3. Origem = fatia (M1=coordenador).

| ID | Título | Evidência-chave | Imp | Eff | Risco | Pri | Tipo | Quebra contrato? | Deploy coord.? | Orig. |
|---|---|---|---|---|---|---|---|---|---|---|
| **T-M01** | Dispatch: commit `dispatched` antes do HTTP + spin-loop de 2s sem backoff/contador; falha da compensação deixa job órfão **e nó online nunca mais é elegível** | `lib.rs:3901` (commit) → `:4008` (POST) → `:4011-4019` (revert best-effort); filtro `:3815,3838` | A | M | alto (correção) | **P0** | estrutural | não | não | M4/M5 |
| **T-M02** | `abort_job` TOCTOU: lê status sem `FOR UPDATE`, UPDATE sem guarda no WHERE → job `cancelled` no Studio com GPU treinando; `dispatched` cancelado nem notifica o nó | `lib.rs:1196-1215` | A | P | médio | **P0** | estrutural | não | sim (orch idempotência abort) | M5/M1 |
| **T-M03** | `cancelling` sem timeout: 3 tentativas de abort falham → `Ok("cancelling")` eterno → nó bloqueado p/ sempre no elegível | `lib.rs:1255-1263`, `:3815,3838` | A | P | baixo | **P0** | quick-win | não | não | M4 |
| **T-M04** | `/internal/adopt` faz SSRF e **vaza o MANAGER_TOKEN mestre** para endpoint arbitrário | `lib.rs:3639` (só prefixo http), `:3646` (`/internal/pairing/verify`), `:328-330` (header sempre) | A | P | baixo | **P0** | quick-win (seg.) | não | não | M2 |
| **T-M05** | `get_telemetry` segura read-lock do cache através de `.await` de SQL (3 fallbacks) → latência do PG engasga heartbeats do cluster inteiro | `lib.rs:2225` guard vivo + `:2229-2234,2256-2260,2323-2327` fetch_one | A | P | baixo | **P1** | quick-win | não | não | M2/M4 |
| **T-M06** | Máquina de estados sem tipo: String em 18 sítios de escrita + guards duplicados; doc do ciclo invertida | §5 INV-1; D-01 | A | M | médio | **P1** | estrutural | não (wire idêntico) | sim (crate→3 serviços) | M1/M5 |
| **T-M07** | `heph-contracts` usada em 3/15 tipos; dispatch via `json!`; `PackageRef` divergente; 10 structs `Internal*` no BFF são cópias do manager | X-01..X-06; `manager/lib.rs:170-172` | A | M | médio | **P1** | estrutural | não (byte a byte igual) | sim (mesma janela) | M3/M6 |
| **T-M08** | **Zero rotas do manager no openapi.yaml** (doc diz o contrário) e zero testes HTTP sem `#[ignore]` — contrato manager↔BFF protegido só por disciplina | D-02; `main.rs:1073-1135` (2 testes, ambos ignore) | A | M | baixo | **P1** | estrutural | especifica o existente | não | M3 |
| **T-M09** | `dispatch_next` consulta só `status='online'` (régua 15s) e não o frescor de 10s → despacha p/ nó que a telemetria já dá como morto | D-03; `lib.rs:3804,3822` × `:2248,2304` | A | P | baixo | **P1** | quick-win | não | não | M2 |
| **T-M10** | VRAM permissiva: sem entrada na tabela → passa sem checagem; `OR vram_total_gb IS NULL` → job de 28GB cai em nó CPU; campos do yaml `measure_margin`/`default_train_gb` mortos; `jobs.vram_min_gb` gravado e ignorado | `lib.rs:3808,3828`; `vram-table.yaml:2-4` × `lib.rs:400-402`; `lib.rs:491,930` | A | M | médio | **P1** | estrutural | não | não | M2 |
| **T-M11** | Sem graceful shutdown: SIGTERM mata no meio do commit/POST (janela do T-M01); worker `tokio::spawn` com handle descartado, sem CancellationToken | `main.rs:971,1004` | A | M | médio | **P1** | estrutural | não | não | M4 |
| **T-M12** | `list_jobs` carrega tabela inteira (sem LIMIT/OFFSET) e todo `get_job` varre a fila inteira p/ posição | `lib.rs:987-1030`, `:970,1085` | A | M | médio (consumidor BFF) | **P1** | estrutural | aditivo (params) | sim (BFF 1º ou default=∞ transitório) | M3/M5 |
| **T-M13** | Duplo GC concorrente em `dataset_versions` (domínio do BFF) rodando a cada 2s no manager | X-07; `lib.rs:1477-1489` + `:3600-3605` × `storage/gc.rs:45-54` | A | P | médio | **P1** | estrutural | não | sim (dono único) | M5/M4 |
| **T-M14** | `create_job` não-atômico: `generation_inputs.used_at=now()` ANTES do INSERT; INSERT do job fora de tx; report failed/cancelled em 2 queries sem tx | `lib.rs:823,918-934`; `:2088-2123` | M | P | baixo | **P1** | quick-win | não | não | M5 |
| **T-M15** | Monólito: lib.rs 4.625 linhas plano; `report_job` 550 / `create_job` 463 / `dispatch_next` 260; `main()` 130 | §1 | A | G | médio | **P1** | estrutural | não | não | todas |
| **T-M16** | Erro→HTTP ×23 `match` manual; negócio virou string de erro (`Internal("model_exists")` → 409 via `==`); report com status inválido vira **500**; detalhe de SQL do PG vai no corpo do 500 | `main.rs:270-776`; `main.rs:665`; `lib.rs:2125-2128`+`main.rs:438`; `lib.rs:945` etc. | M | M | baixo | **P2** | estrutural | só mensagens (codes estáveis) | não | M3 |
| **T-M17** | Config: 14 envs ad-hoc, 3 relidos **a cada tick/loop/dispatch**; endpoint `orchestrator-local:8082` e bind `0.0.0.0` hardcoded; pool sem `max_connections/acquire_timeout` | `main.rs:887-1000`; `lib.rs:2217,3531,3535,3749`; `lib.rs:2395`; `main.rs:904,1000` | M | M | baixo | **P2** | estrutural | não | sim (envs novos→compose) | M4/M2 |
| **T-M18** | Auth: `==` não-constant-time + alocação por request; sem body-limit/timeout layer (heartbeat 2s aceita 2MB); token inválido citado por valor no erro de boot | `main.rs:175`; `Cargo.toml:16`+`main.rs:833-841`; `main.rs:860` | M | P | baixo | **P2** | quick-win (seg.) | não | não | M3/M6 |
| **T-M19** | TelemetryCache: esvazia no boot (nós zeram `gpus`/`ram`), não expurga revogados/offline, `list_orchestrators` não faz fallback do banco (coluna `gpus` nem selecionada) | `lib.rs:383,2180-2212,2436-2475,3718` | M | M | baixo | **P2** | estrutural | não | não | M2 |
| **T-M20** | Testes: unidade de agregação testa **cópia colada** da lógica, não a função; 147 testes serializados por `SERIAL`; `set_var/remove_var` em teste (corrida UB-emergente com leitores); `manager_db.rs` 7.473 linhas sem divisão por domínio | `lib.rs:4181-4315` (clones); `manager_db.rs:22`; `lib.rs:4565-4606` | M | M | baixo | **P2** | estrutural | não | não | M2/M4/M6 |
| **T-M21** | 91 queries raw + tuplas anônimas (12 campos), SQL dinâmico com `bind_idx` manual e `${limit_idx}` interpolado; 22 `r.get()` em `get_job` | `lib.rs:3398-3435,995-1003,1095-1109,2548` | M | M | baixo | **P2** | estrutural | não | não | M5 |
| **T-M22** | `prepare_complete_handler` dispara `dispatch_next` (6 args) dentro do request HTTP → latência do nó segura a conexão do BFF; acoplado ao T-M01 (spin em hot-path) | `main.rs:468-490` | M | P | baixo | **P2** | estrutural | não | não | M3 |
| **T-M23** | Bytes→serde manual clonado em 10 handlers (`is_empty`+`from_slice`+msg); clamp de paginação duplicado manager×BFF | `main.rs:254-748` (10×); X-13 | M | P | baixo | **P2** | quick-win | não | não | M3 |
| **T-M24** | `span.enter()` vivo através de `next.run().await` no middleware (corrupção da árvore de spans); `x-request-id` ausente vira literal `"unknown"` ecoado | `main.rs:139-154` | M | P | baixo | **P2** | quick-win | não | não | M3 |
| **T-M25** | Paths de artefato (`art.path`) e `s3_key` de modelo aceitos sem validação de traversal; chaves S3 montadas por `format!` dependem disso | `lib.rs:1680-1700,2630-2670,539,1867,1979,2888` | M | P | médio | **P2** | quick-win (seg.) | não | não | M6 |
| **T-M26** | Cold boot sem validação de schema: manager não roda migrations nem checa `_sqlx_migrations`; dev compose segura (`service_healthy` do principal) mas qualquer deploy fora do compose do dev falha com `relation "jobs" does not exist` | sem `migrate!` no manager; `api-principal/main.rs:146`; `compose.yaml:171-175`; `main.rs:927` | M | P | baixo | **P2** | quick-win | não | sim (schema compartilhado) | M5 |
| **T-M27** | `adopt_internal`: INSERT sem `RETURNING` + SELECT id redundante; resposta com `status="online"`, `gpus=[]`, `vram=None` hardcoded mesmo em re-adoção | `lib.rs:3665-3711` | B | P | baixo | **P3** | quick-win | resposta richer (aditiva) | não | M2 |
| **T-M28** | Duplicação de utilitários puros (`slugify`, aliases de arch) e harness de teste | X-08/09/11 | B | P | nenhum | **P3** | limpeza | não | não | M6 |
| **T-M29** | Higiene/tooling: 14 warnings de clippy (`derivable_impls` `:366`, 7× type_complexity…); sem `clippy.toml`/`rustfmt.toml`/`deny.toml`; `serde_yaml` deprecated; Dockerfile compila 2× com src dummy; `impl Default` manual; doc do PITFALLS desatualizada (D-05) | `cargo clippy` (14); glob de configs; `Dockerfile:11-22` | B | P | nenhum | **P3** | limpeza | não | não | M4/M6/coord. |

### Detalhamento — tarefas estruturais

#### [TASK-MGR-001] Contrato único real: crate no dispatch, DTOs e `JobStatus` (T-M06/T-M07)
- **Evidência:** §4 X-01..X-06; `lib.rs:170-172` (re-export parcial); `lib.rs:3956-3983` (json!).
- **Problema:** o protocolo vive em 3 representações (crate, manager local/`json!`, BFF `Internal*`); o orchestrator já é cliente exemplar do crate (`domain/models.rs:2-7`) — manager e BFF são os outliers.
- **Proposta:** (1) na crate: unificar `PackageRef` (`version_id: Option<String>` com `serde(alias)`/skip), adicionar `JobStatus` (8 estados do `0006:32-34`, serde lowercase idêntico ao wire atual, `is_terminal()`, `can_transition_to()`), mover DTOs de resposta (§4); (2) manager: montar `DispatchRequest` tipado e apagar structs locais; (3) api-principal: consumir os tipos da crate em `manager_client` (elimina 10 `Internal*` — espelho do TASK-API-001 da auditoria-irmã, fazer **no mesmo PR de crate** para os 3 serviços).
- **Aceite:** `rg 'struct (Internal|PackageRef|WeightsRef|JobRow|TelemetryResponse)' services/manager/src services/api-principal/src` = 0 (exceto crate); payload de dispatch do orchestrator desserializa no tipo do crate em teste de contrato; wire byte-a-byte idêntico aos snapshots atuais de `manager_db.rs`. `cargo check --workspace --locked` + clippy limpos.
- **Esforço:** M | **Risco:** médio (bump da crate nos 3 serviços; ordem manager→BFF segura porque o wire não muda) | **Dep:** nenhuma. **PR1.**

#### [TASK-MGR-002] Contrato interno especificado + testes HTTP sem Postgres (T-M08)
- **Evidência:** D-02; `main.rs:1073-1135` (2 testes `#[ignore]`); nenhum teste de 401 (`M6-08`).
- **Proposta:** `packages/contracts/openapi-manager.yaml` com as 24 rotas (`/internal/*`) gerado **a partir do código** (orval/manual-travado-por-teste — decidir com o dono do contracts) + teste de inventário no manager (mesma régua do `contract.rs` do BFF); suíte de handlers com `tower::ServiceExt::oneshot` e estado mock (sem PG) cobrindo 401/400/404/409 de cada rota.
- **Aceite:** doc `manager.md` corrigida (D-01/D-02); `cargo test -p manager` roda rotas HTTP sem Postgres; teste de inventário falha se rota nova nascer fora do yaml.
- **Esforço:** M | **Risco:** baixo | **Dep:** TASK-MGR-001 (schemas). **PR2.**

#### [TASK-MGR-003] Dispatch resiliente: tentativa+backoff, janela `dispatched` vigiada, elegível c/ frescor (T-M01/T-M09)
- **Evidência:** `lib.rs:3901→4008→4011-4019`; `:3804,3822`; `main.rs:971-995`.
- **Proposta:** sem mudar o CHECK do schema: contadores em `params` jsonb (`dispatch_attempts`, `last_dispatch_attempt`), backoff com teto de tentativas → `failed` auditável; **compensação em transação única** com o registro da tentativa; watchdog ganha a regra "`dispatched` sem report por N min com nó online → requeue/`failed`" e o `WHERE` do elegível passa a incluir `last_heartbeat >= now() - stale_secs`; considerar POST **dentro** da tx (commit só após 200 OK, com `FOR UPDATE` no nó segurando o slot — o nó já é travado por `FOR UPDATE OF o`) ou, se travar conexão demais, o estado intermediário fica em `queue_reason='dispatching'` (sem migração).
- **Aceite:** teste injetando falha de rede no `OrchestratorClient` mock: job volta a `queued` com `dispatch_attempts` incrementado e **não** é re-tentado antes do backoff; SIGKILL entre commit e POST não deixa nó permanentemente indisponível (assert com watchdog); elegível nunca seleciona nó stale-por-telemetria.
- **Esforço:** M | **Risco:** médio (mexer na ordem commit/HTTP pode introduzir duplo-dispatch se feito às pressas — cobrir com teste de corrida real, cf. TASK-MGR-010) | **Dep:** TASK-MGR-001 (JobStatus) desejável. **PR3.**

#### [TASK-MGR-004] Abort atômico com notificação total (T-M02/T-M03)
- **Evidência:** `lib.rs:1196-1215` (leitura sem lock, UPDATE sem guarda, dispatched silencioso); `:1255-1263`; `:3815,3838`.
- **Proposta:** `UPDATE jobs SET status=... WHERE id=$1 AND status=$2 RETURNING` (compare-and-set, padrão `prepare_complete` já no arquivo); no ramo `dispatched`, notificar `/internal/abort` igual ao `running` (orchestrator é tolerante a job desconhecido? **verificar antes** — se não, PR no orchestrator primeiro); watchdog faz `cancelling` > `abort_timeout` (ex.: 5min) → `cancelled` com `queue_reason='abort_timeout'`, liberando o nó.
- **Aceite:** teste de corrida abort×dispatch (dois calls concorrentes) termina sempre em par consistente (banco e nó); `cancelling` órfão expira; nó volta a ser elegível sozinho.
- **Esforço:** P-M | **Risco:** médio | **Dep:** orquestrador (idempotência do abort). **PR3b.**

#### [TASK-MGR-005] Fatiar o monólito + transporte limpo (T-M15/T-M16/T-M22/T-M23)
- **Proposta:** aplicar §3.1 em 3 PRs encadeáveis: (a) extrair `http/` + `boot/` (handlers ≤60 linhas, `AppJson<T>`, `impl IntoResponse for ManagerError` único, sem SQL de `ready`/`metrics` no main — hoje `main.rs:194,215` query inline em handler; `dispatch_next` sai do handler do prepare e vira comando no canal do worker (T-M22); (b) `jobs/` + `scheduling/` (split de `report_job`/`create_job` em guardas/persistência/hooks; extrair `aggregate_telemetry()` pura); (c) `nodes/` + `models/` + `generations/` + repositories com `FromRow` (T-M21).
- **Aceite:** nenhum arquivo >600 linhas; nenhum handler faz SQL; `sqlx` restrito a `*/repository.rs` + `boot/`; os ~185 testes continuam verdes **sem tocar em wire nem em SQL** (movidos); clippy `type_complexity` = 0.
- **Esforço:** G (3 PRs) | **Risco:** médio (movimentação mecânica; perigo real é "aproveitar e reescrever" — proibir no PR) | **Dep:** TASK-MGR-001. **PR4a/b/c.**

#### [TASK-MGR-006] Bootstrap tipado e gracioso (T-M11/T-M17/T-M26)
- **Proposta:** `boot/config.rs` com `ManagerConfig::from_env()` (14 vars + defaults nomeados, validação fail-fast **estruturada** — `main() -> Result`, sem `panic!`/`expect` pós-tracing); pool com `max_connections/acquire_timeout` da config; `bind` via `HOST` (default `127.0.0.1` — prod não usa o bind do container? usa: compose rede interna → default do container `0.0.0.0` mantém-se pela env do Dockerfile, host dev fica protegido); workers com `JoinSet` + `CancellationToken`; `axum::serve(...).with_graceful_shutdown(sigterm)`; ticker de GC separado (1h); verificação de schema no boot (`SELECT version FROM _sqlx_migrations ORDER BY version DESC LIMIT 1` com retry até timeout, depois `recover_jobs`); `read_to_string` → `tokio::fs`.
- **Aceite:** `rg 'std::env::var' services/manager/src` só em `boot/config.rs` (e `RUST_LOG`); SIGTERM durante dispatch não deixa órfão (teste com cancelamento); log de boot 100% JSON.
- **Esforço:** M | **Risco:** baixo | **Dep:** nenhuma (paralelo a TASK-MGR-003; o graceful shutdown reduz a janela do T-M01). **PR5.**

#### [TASK-MGR-007] Paginação de `list_jobs` + posições via window function (T-M12/T-M13)
- **Proposta:** `GET /internal/jobs?limit&offset` (default atual = sem teto é o risco: adotar default 100 com teto 500 e header/objeto `{items,total}` — **alinhado com o BFF primeiro**, que hoje chama sem params e deriva fila em `list_queue`); posições de fila: `row_number() OVER (ORDER BY created_at)` numa única query (mata a varredura extra de `:970,1085`).
- **Aceite:** `EXPLAIN` do `get_job` sem o `SELECT id FROM jobs WHERE status='queued'`; BFF com teste de contrato da listagem paginada; memória do manager independente do histórico.
- **Esforço:** M | **Risco:** médio (contrato do BFF com a web: `jobs/handlers.rs` do BFF assume lista completa em 2 lugares) | **Dep:** TASK-MGR-001 (DTOs). **PR6.**

#### [TASK-MGR-008] Dono único de GC e transações de escrita cruzada (T-M13/T-M14)
- **Proposta:** remover `gc_dataset_versions` do manager (deixar `storage/gc.rs` do BFF como dono, já coberto pelo T-22 da auditoria-irmã); envolver `create_job` inteiro em tx; ramos failed/cancelled do report em tx; CTEs do watchdog em tx única; `prepare_complete` já é CAS — manter.
- **Aceite:** `rg 'DELETE FROM dataset_versions' services/manager` = 0; teste de falha injetada no INSERT do `create_job` reverte `used_at`.
- **Esforço:** P | **Risco:** baixo | **Dep:** nenhuma. **PR7** (pode ir antes, quick win).

#### [TASK-MGR-009] Blindar auth e ingestão (T-M04/T-M18/T-M25)
- **Proposta:** precomputar `expected_auth: Bytes` no `AppState` + `constant_time_eq` (crate `subtle` ou loop XOR — 10 linhas, sem nova dep se preferir `ring::constant_time`); `DefaultBodyLimit::max` por rota (64KB geral / 1MB report) + `TimeoutLayer`; remover o valor do token da mensagem de erro (`main.rs:860`); no adopt: validar endpoint contra allow-list de subnets/regras (padrão SSRF do BFF: allow-list fail-closed, `models/validate.rs:144-210` como régua) e **usar cliente sem token** no `/internal/pairing/verify`; validar `art.path`/`s3_key` contra `..`/leading `/` (função compartilhada na crate, vinda do `keys::sanitize_filename` do BFF).
- **Aceite:** testes de 401 (header ausente/malformado/token errado) na suíte do TASK-MGR-002; teste de adopt com endpoint `http://169.254.169.254` e `http://attacker` → 400 e **zero** header Authorization na chamada de verify; payload >limite → 413.
- **Esforço:** P | **Risco:** baixo | **Dep:** TASK-MGR-002 (testes). **PR0 (pode ser F0, antes de tudo).**

#### [TASK-MGR-010] Suíte de testes confiável (T-M20/T-M28)
- **Proposta:** `sqlx::test` com bancos/schemas efêmeros por teste (mata `SERIAL` e permite **teste de corrida real** de dispatch×abort×watchdog); extrair `tests/common/mod.rs` (harness anti-footgun único — mover para `crates/heph-testkit` quando o BFF adotar o mesmo); apagar os 2 testes de agregação-cópia e testar `aggregate_telemetry()` pura; `resolve_diffusion_image` vira função pura com override injetado (sem `set_var`); mover `slugify`/`DiffusionArch` para a crate com testes únicos; instalar `jscpd` no CI (recomendação R-01) para o % de duplicação.
- **Aceite:** `cargo test -p manager` roda em paralelo sem lock global e sem Postgres compartilhado (integrais usam `sqlx::test` com `--ignored` só onde PG real é exigido); 0 `set_var` em testes; corrida abort×dispatch coberta (fecha T-M02).
- **Esforço:** M | **Risco:** baixo | **Dep:** TASK-MGR-005 (módulos puros p/ testar). **PR8.**

---

## 7. Segurança e Robustez (achados mapeados, não explorados)

**Fluxo de auth:** todo `/internal/*` atrás de `Bearer MANAGER_TOKEN` (`main.rs:164-182`); `/health`,`/ready`,`/metrics` públicos (`main.rs:783-842` — `/metrics` expõe contagens de jobs/nós: aceitável em rede interna, registrado). Prod: env obrigatória e porta fechada (`compose.prod.yaml:69-75`). Dev/GPU: ver D-07. **Comparação de segredo:** `==` em `main.rs:175` (não-constant-time) + alocação por request — o BFF não tem comparação manual (auditoria-irmã §7), o orchestrator usa HMAC/pairing separado (`security/pairing.rs`).

| Área | Estado | Riscos mapeados |
|---|---|---|
| Autenticação de entrada | ✅ middleware cobre tudo | `==` timing + alloc hot-path (T-M18); default dev inseguro documentado no boot com warn (`main.rs:863-871`) — aceitável, mas valor aparece no erro (`:860`) |
| SSRF saída | ❌ **sem defesa** | adopt valida só prefixo e envia token mestre (`lib.rs:3639,328-330`); BFF tem allow-list fail-closed como régua — o manager, que é o serviço mais fechado da topologia, não |
| Limites de corpo | ❌ nenhum layer | 2MB default do axum em rotas de 2s (heartbeat) e no report (T-M18) |
| Path traversal / S3 keys | parcial | `report_job` aceita `art.path` cru (`lib.rs:1680-1700`); `validate_create_model` não valida `s3_key` (`:2630-2670`); chaves por `format!` (`:539,1867,1979,2888`) |
| Injeção SQL | ✅ | todas as 93 queries usam bind posicional; `format!` só monta nomes de coluna/cláusulas e índices de bind (fragilidade, não injeção — `:995-1003,3398-3435`) |
| Injeção em args Docker | n/a aqui | manager monta imagem/env (`resolve_diffusion_image`, `exec_mode`) — a superfície `docker run` é do orchestrator (auditoria própria) |
| Segredos em log | ✅ (1 ponto) | nenhum `tracing::!` imprime token; ver `:860` acima |
| Robustez async | ❌ 4 frentes | lock-over-await (T-M05); `std::fs` no async do boot (T-M17/M4-10); spin-loop sem backoff (T-M01); span guard over await (T-M24); spawn sem handle (T-M11) |
| Estado só em memória | projetado, com furos | TelemetryCache não-durável (T-M19); sessões de upload chunked são do BFF, não daqui |
| Cancelamento | ❌ | `cancelling` eterno trava nó (T-M03); abort de `dispatched` silencioso (T-M02) |
| Graceful shutdown | ❌ ausente | T-M11; janela órfã entre commit e POST (T-M01) |

Nota de validação por amostragem (correções às varreduras): (a) o cold-boot não mata o compose de dev — `compose.yaml:171-175` usa `service_healthy` do principal (que roda `sqlx::migrate!` **antes** de abrir `/health`, `api-principal/main.rs:146-149`), ao contrário do alegado pela varredura M5; o risco sobrevive apenas fora do compose (T-M26). (b) `receive_heartbeat` adquire o write-lock **no fim** (`lib.rs:2199-2201`, verificado) — não é fonte de contenção; só `get_telemetry` é. (c) A régua "10s" do dispatch não é *inexistente*, é **diferente** (15s via status) — o achado está na janela e na dupla fonte, não em ausência total.

---

## 8. Roadmap em Fases (cada fase = 1 PR independente e verificável)

| Fase | PR | Conteúdo | Dep |
|---|---|---|---|
| **F0 — freios de segurança/robustez (sem wire)** | PR-0a | TASK-MGR-009 (constant-time, limits, SSRF adopt, sanitize paths) | nenhuma |
| | PR-0b | T-M03+T-M05+T-M24 (timeout de `cancelling` no watchdog; soltar lock no get_telemetry; `.instrument(span)`; GC fora do tick de 2s) | nenhuma |
| **F1 — contrato** | PR-1 | TASK-MGR-001 (crate única; `JobStatus`; dispatch tipado) | nenhuma |
| | PR-2 | TASK-MGR-002 (openapi-manager + testes HTTP oneshot) | PR-1 |
| **F2 — correção do dispatch/abort** | PR-3a/b | TASK-MGR-003, TASK-MGR-004 | PR-0b, PR-1 |
| **F3 — domínio e módulos** | PR-4a/b/c | TASK-MGR-005 (fatiamento integral; `aggregate_telemetry` pura; `apply_transition`) | PR-1 (PR-4c: PR-0a) |
| **F4 — runtime e config** | PR-5 | TASK-MGR-006 | PR-4a |
| **F5 — banco e escala** | PR-6/7 | TASK-MGR-007 (paginação, coord. BFF), TASK-MGR-008 (GC dono único, tx) | PR-1 |
| **F6 — testes e higiene** | PR-8 | TASK-MGR-010 + T-M29 (clippy -D no CI, rustfmt.toml, deny.toml/machete, Dockerfile 1 build) | PR-3..5 |

**Dependências reais:** F0 é independente e libera os riscos P0 sem esperar nada; contrato (F1) precede F2/F3 porque os módulos novos devem nascer com os tipos certos; paginação e GC exigem combinar com o api-principal na mesma janela (deploy coordenado); **nenhum passo exige mudança de schema** — as correções usam `params` jsonb + guards no WHERE, preservando o `0006` intacto (append-only).

### O que NÃO mudar
- As guardas de `report_job` (`lib.rs:1597-1637`), o compare-and-set de `prepare_*` (`:1334,1388,1415`), a defesa `done_artifacts_violation` (`:453-481`) e o `WHERE status <> 'revoked'` da auto-adoção (`:2396-2397`) — **são as referências de ouro do serviço** e as melhores ideias da classe em todo o repo; generalizá-las (não substituí-las).
- O `FOR UPDATE SKIP LOCKED` + `FOR UPDATE OF o` do dispatch (padrão correto; o bug é a ordem commit/HTTP, não o lock).
- A política de watchdog dimensionada (60min/7d/15s/60s) — números com incidente documentado; o achado é onde vivem (config), não o valor.
- O ciclo `preparing`-inicial (ADR-0025): o código está certo e a doc, errada — corrigir a doc.
- Os 143 testes de integração como rede de segurança durante o PR-4: mover junto, não reescrever; nenhuma mudança de wire em PR de movimentação.
- `sqlx` sem macros `query!` (build offline — decisão-irmã T-02); `OrchestratorClient` trait como única porta; compose prod fail-closed.
- Bind `0.0.0.0` **dentro do container** (necessário à rede docker) — a mudança é o default do host bind fora do container + env explícita.

---

## 9. Proposta de texto para `AGENTS.md` — "Convenções de Backend (manager)" (não aplicar; só proposta)

```markdown
1. Máquina de estados: job status só via `JobStatus` (heph-contracts) e
   `jobs::state::apply_transition()`; proibido `SET status` literal em SQL
   novo; transição incerta = compare-and-set no WHERE (padrão `prepare_*`).
2. Máquina de nó: elegibilidade de dispatch e frescor de telemetria têm de
   ler a MESMA régua (uma função `node_is_fresh`); proibido status='online'
   sem checagem de last_heartbeat no scheduler.
3. Protocolo interno: todo payload de fronteira (dispatch/report/heartbeat/
   respostas) vem de `heph-contracts`; proibido `json!` para wire e proibido
   struct local duplicando tipo do crate; mudança no crate = PR nos 3 serviços.
4. HTTP de controle: nenhuma rota `/internal/*` sem entrada no
   openapi-manager.yaml e no inventário testado; nenhum handler com SQL,
   `dispatch` ou chamada de rede no hot-path do request (comandos vão por
   canal para os workers).
5. Concorrência: nenhuma lock (std/tokio/tracing guard) viva através de
   `.await`; HTTP externo nasce com timeout+backoff+contador; operações
   multi-escrita em jobs/creation/report usam transação (padrões: a defesa
   `done_artifacts_violation`, o CAS de `prepare_complete`).
6. Config: `boot::ManagerConfig` tipado é o ÚNICO leitor de env; proibido
   `std::env::var` em lib/worker; tempos de watchdog são campos da config
   com defaults nomeados (nunca `interval '60 minutes'` no SQL).
7. Tokens: comparação sempre constant-time com header pré-computado; o
   MANAGER_TOKEN JAMAIS sai em request a endpoint não-autenticado (pairing
   usa cliente sem token); valor inválido nunca aparece em mensagem de erro.
8. Schema: as migrations vivem no api-principal e são append-only; o manager
   valida a versão esperada no boot ANTES de `recover_jobs` e não roda GC de
   tabelas de outro domínio (dataset_versions é do BFF).
```

**Recomendações de tooling (R-01):** `jscpd` em CI (gate de clone), `cargo-machete`/`cargo-deny`, `clippy.toml` com `unwrap_used`/`expect_used` deny + `await_holding_lock`/`disallowed_methods`, `rustfmt.toml`, `cargo clippy -D warnings` no pipeline, e avaliar migrar `serde_yaml` (deprecated) — hoje o único uso deyaml do manager é a VRAM table (`lib.rs:423`).
