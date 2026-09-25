# Levantamento de Modularização por Domínio: `services/manager`

**Projeto:** Hephaestus LLM Studio
**Data:** 2026-09-23
**Escopo:** `services/manager` (:8081) — modularização e eliminação de duplicação. Somente leitura; nenhuma mudança de código proposta para execução imediata.
**Método:** 6 varreduras paralelas por domínio funcional (state/lifecycle, dispatch/scheduling, nodes/heartbeats, reporting, watchdogs, auth/bootstrap) + validação por amostragem do coordenador (contagens divergentes resolvidas contra o código; `prepare_cancel` e heartbeat verificados linha a linha).
**Relação com auditorias existentes:** complementa `tasks/specs/manager-modularizacao-auditoria.md` (2026-09-20, TASK-MGR-001…010, foco em robustez/concorrência/segurança). Este documento organiza as mesmas 5.830 linhas por **domínio funcional** com propostas de módulo e matriz Impacto×Esforço. Achados de robustez (SSRF no adopt, commit-antes-do-POST, abort sem lock) NÃO são repetidos aqui — ver a auditoria-irmã. O bug MM-01 abaixo é **novo** (não consta na auditoria de 2026-09-20).

---

## 1. Resumo Executivo

O manager concentra 100% da sua lógica em `src/lib.rs` (4.645 linhas) e `src/main.rs` (1.185 linhas), sem um único módulo interno. As funções-monstro por domínio: `report_job` 552 linhas (8 responsabilidades), `create_job` 472 (contém ~370 linhas de resolução de pesos que pertencem a outro domínio), `dispatch_next` 260 (transação SQL + política + payload + HTTP + compensação no mesmo bloco).

Padrões transversais de duplicação (contagens verificadas):

| Padrão | Ocorrências | Evidência |
|---|---|---|
| `UPDATE jobs SET status` ad hoc, sem função central | 18 em lib.rs | `lib.rs:1221,1230,1239,1348,1401,1428,1467,1653,2085,2110,2129,3378,3387,3582,3599,3899,3910,4032` |
| `ManagerError::Internal(format!("…: {e}"))` boilerplate | ~100 em lib.rs | todas as funções de banco |
| Guard `parse::<uuid::Uuid>()` em handler | 12 em main.rs | `main.rs:305,320,335,357,416,452,510,545,624,686,704,767` |
| Guard `body.is_empty()` + parse JSON | 10 (+1 variante cleanup) em main.rs | `main.rs:255,384,421,457,515,567,605,648,708,749` |
| `match ManagerError` → error_response manual | 24 handlers; catch-all `internal_error(&e.to_string())` ×22 | `main.rs:270-777` |
| Predicado de fila `status='queued' ORDER BY created_at` | 11 em lib.rs (2 byte-a-byte idênticos) | `lib.rs:953,983,1098,3805,3883,3901,3912,4032,1348,3387,3599` |
| Mapeamento ManagerError→HTTP divergente por handler | abort converte variantes impossíveis em 500 | `main.rs:342-350` |

**Bug factual encontrado (novo):** `prepare_cancel` (`lib.rs:1427-1431`) executa `UPDATE … WHERE id = $1 …` sem `.bind(id)` → erro de bind em runtime a cada chamada de `POST /internal/jobs/:id/prepare-cancel` (hoje sempre 500). Contraste com `prepare_fail` (`lib.rs:1400-1408`), que faz bind. O fallback do handler (`rows_affected == 0` → checagens) nunca é alcançado pelo caminho feliz porque a query inteira falha antes. Correção de 1 linha + teste.

**Estados como String:** a máquina de estados não tem enum; `status` trafega como `String` com literais espalhados (`match status.as_str()` em `lib.rs:1220`; `TERMINAL_STATUSES` em `lib.rs:2848-2849` convive com literais duplicados em `lib.rs:1504`).

---

## 2. Mapa por Domínio (tamanho e donos de linha)

| Domínio | lib.rs | main.rs | Funções âncora |
|---|---|---|---|
| state/lifecycle | ~1.040 | ~305 + 11 rotas | `create_job` 472l, `list_jobs` 118l, `abort_job` 83l, `delete_job`/`cleanup_jobs` 114l |
| dispatch/scheduling | ~846 | ~222 | `dispatch_next` 260l, resolução de pesos 370l (dentro de create_job), `VramTable` 48l |
| reporting (+models/generations) | ~1.360 | ~248 | `report_job` 552l, hooks de catalogação 153l, `compute_model_name` 84l |
| nodes/heartbeats | ~500 | ~80 | `receive_heartbeat` 79l, `get_telemetry` 137l, `adopt_internal` 84l |
| watchdogs | ~168 | ~34 | `watchdog_tick` 79l, `recover_jobs` 22l |
| auth/bootstrap | 32 | ~332 | `auth_middleware` 19l, `main()` 130l, `build_router` 60l |

Sobreposição esperada: a resolução de pesos (dispatch) vive fisicamente dentro de `create_job` (lifecycle); os hooks de catalogação (reporting) vivem fisicamente dentro de `report_job`. A extração por domínio separa esses enxertos.

---

## 3. Achados por Domínio

### 3.1 state/lifecycle

**Localização:** `create_job` (`lib.rs:503-974`), `list_jobs` (:976-1093), `get_job` (:1095-1165), `get_job_artifacts` (:1167-1201), `abort_job` (:1203-1285), `prepare_complete` (:1304-1382), `prepare_fail` (:1384-1424), `prepare_cancel` (:1426-1463, **bug MM-01**), `delete_job` (:2933-2974), `normalize_cleanup_statuses` (:2976-3003), `cleanup_jobs` (:3005-3076), `recover_jobs` (:3375-3396); handlers `main.rs:254-568`.

**Duplicação:**
- 18 `UPDATE jobs SET status` sem função central; guardas inconsistentes (as de `report_job:1653` e `dispatch_next:4032` não têm `WHERE status = …`; as terminais de `report_job:2085,2110,2129` idem). Side effects heterogêneos (`queue_reason`, `finished_at`, `params||`, `jsonb_set`) colados no SQL literal.
- Predicado de fila + montagem de `pos_map` copiados verbatim entre `list_jobs:983-989` e `get_job:1098-1105` (~15 linhas); `create_job:953` repete o COUNT do mesmo predicado.
- Mapeamento tupla→`JobRow` de ~22 campos duplicado entre `list_jobs:1040-1068` e `get_job:1130-1158`.
- Conjunto de estados terminais duplicado como literal: `TERMINAL_STATUSES` (`:2849`), `gc_dataset_versions:1504`, regra reencenada em `abort_job:1220` e no doc do erro `main.rs:86-89`.
- `cleanup_jobs:3038-3051` repete dentro de um laço o par `plan_job_sweep`+`DELETE` de `delete_job:2952-2963`.

**Mistura de responsabilidades:** `abort_job` faz match manual sobre `status.as_str()` + 3 UPDATEs inline + retry HTTP de abort ao nó (3 tentativas, backoff) no mesmo bloco. `create_job` mistura validação, resolução de pesos (§3.2), decisão de estado inicial e INSERT. Handlers fazem guards de parse + mapeamento de erro à mão.

**Proposta:** `src/jobs/{lifecycle,abort,delete,repo}.rs`. `repo.rs` abriga o predicado de fila, `row_to_job_row` e um helper `transition(conn, id, from, to, side_effects)`; `abort` usa `OrchestratorClient` por trait. **Riscos:** unificar guardas de UPDATE altera semântica de rows_affected dos caminhos best-effort (`report_job` retorna `Ok(())` silencioso em guarda não-erro por design, `lib.rs:1633-1638` — manter); `recover_jobs` é `pub` e chamado do boot (`main.rs:927`) — re-export necessário.

### 3.2 dispatch/scheduling

**Localização:** `dispatch_next` (`lib.rs:3782-4041`), `resolve_diffusion_image` (:3769-3780), `VramTable`+impl (:389-436), `OrchestratorClient`+`HttpOrchestratorClient` (:274-345), resolução de pesos DENTRO de create_job (:520-600 weights, :602-625 hint, :627-889 lora/checkpoint/encoder/img2img), `slugify` (:3078-3107); boot do worker `main.rs:964-995`, load da vram-table `main.rs:933-949`.

**Duplicação:**
- Query de elegibilidade de nó duplicada em 2 variantes quase-idênticas (~18 linhas cada; diferem só no bind do hint e no ORDER BY): `lib.rs:3830-3845` vs `3853-3869`.
- Padrão "query models → valida kind → grava snake_case em params" repetido 3× (loras `:660`, custom checkpoint `:703`, text encoder `:755`) + 5× o padrão `to_value`+`params[x]=v` (`:687,739,775,843,877`); 6× `Uuid::parse_str(...).map_err(InvalidRequest)` (`:604,648,698,752,819,857`).
- Extração de 7 refs + injeção no payload por copia-e-cola: `lib.rs:3926-3971` e `3988-4025`.
- 9× `map_err(Internal(format!))` dentro da própria `dispatch_next` (`:3793,3809,3841,3862,3888,3891,3917,3923,4037`).
- vram-table definida em 3 lugares: YAML canônico, `include_str!` no binário (`main.rs:939`) e fixture reescrita à mão nos testes (`tests/manager_db.rs:234-244`, `main.rs:1035-1037`) — risco de drift.

**Mistura de responsabilidades:** `dispatch_next` = transação SQL (SKIP LOCKED no job, FOR UPDATE OF o nó) + política de eleição embutida no ORDER BY do SQL (`lib.rs:3861-3864`) + decisão waiting_vram/waiting_slot + montagem de payload de ~100 linhas + POST + compensação best-effort FORA da transação. `resolve_diffusion_image` lê `std::env` dentro de função pura de string.

**Proposta:** `src/dispatch/{mod,election,payload}.rs` + `src/policy/vram.rs` (loader sai do main) + `src/jobs/resolve.rs` (dedup `resolve_model_ref(pool, id, expected_kind)`) + `src/orchestrator/client.rs` (trait, módulo leaf a extrair primeiro). **Riscos (altos):** atomicidade SKIP LOCKED protege contra duplo-dispatch entre o loop de 2s e o disparo best-effort do handler `prepare_complete` (`main.rs:474-487`) — qualquer extração que quebre begin→select→update→commit pode despachar 2×; payload tem contrato snake_case byte-a-byte com o orchestrator; janela commit→POST é comportamento atual (não "corrigir" na migração).

### 3.3 nodes/heartbeats

**Localização:** `TelemetryState`/`new_telemetry_cache` (`lib.rs:352-387`), `receive_heartbeat` (:2149-2236), `get_telemetry` (:2237-2373), `list_orchestrators` (:2449-2520), `adopt_orchestrator` (:2405-2418), `adopt_internal` (:3649-3732), `revoke_orchestrator` (:3739-3750); handlers `main.rs:566-636`.

**Duplicação:** critério de stale em 3 regimes: helper com env `NODE_STALE_TIMEOUT_SECS` (`lib.rs:2229-2234`, default 10) usado só em `get_telemetry`; `list_orchestrators:2482` e testes usam literal 10 ignorando o env; watchdog usa `ORCH_WATCHDOG_DEGRADED_S=15`/`OFFLINE_S=60` (`:3552-3559`). `adopt_internal` repete validação+HTTP+SQL+DTO num bloco de 84 linhas.

**Mistura:** Postgres (`orchestrators.last_heartbeat/status`) é a fonte operacional — `dispatch_next` e `watchdog_tick` ignoram o cache; o `TelemetryCache` RwLock em memória serve só observabilidade, diverge em janela/clock/volatilidade e não é expurgado no revoke. `receive_heartbeat` verificado: fluxo correto (write-lock só no fim, guard de revoked em `rows_affected`), sem duplicação interna.

**Proposta:** `src/nodes/{cache,heartbeat,telemetry,registry}.rs`; unificar frescor de nó numa única fn (elimina os 3 regimes); considerar mover `adopt_internal` para registry com `OrchestratorClient` injetado (ver SSRF na auditoria-irmã). **Riscos:** baixos — fronteiras já são limpas; cuidado com re-exports (`main.rs:590-600` chama `manager::get_telemetry`/`list_orchestrators`).

### 3.4 reporting

**Localização:** `report_job` (`lib.rs:1596-2147`, 552 linhas; bloco `done` sozinho ~354 linhas :1705-2060), métricas (:1513-1593), `done_artifacts_violation` (:463-501), classificação difusão (:3116-3268), models (:2547-2805, :3272-3341), generations (:3402-3543); handlers `main.rs:411-445, 637-777`.

**Duplicação:**
- Blocos failed/cancelled de `report_job` cópia verbatim (8 linhas): `lib.rs:2101-2108` vs `2120-2127`; UPDATE terminal ×4 quase-idênticos (`:2043,2110,2128,1734`).
- Validação de artefato (md5+bytes) ×3 com 3 semânticas de erro distintas — skip silencioso (`:1674`), `Internal` (`:1755`), `InvalidRequest` (`:2627`).
- `INSERT INTO job_artifacts` ×2 quase verbatim com EXISTS prévio (`:1684-1699` running vs `:1771-1783` done; erro engolido no primeiro, propagado no segundo).
- Mapeamento tupla-12→`ModelItem` ×4 (`:2562,2750,2792,3307`); closure row→`GenerationRow` idêntica ×2 (`:3460,3509`).
- Lista canônica de archs em 3 fontes independentes (`:3117,2663,3219`) — adicionar arch exige 3 edits + migration.
- Pattern warn-and-continue colado 5× inline no `report_job` (`:1814,1832,1899,1936,2000`).

**Mistura:** `report_job` orquestra 8 responsabilidades; o bloco `done` roda em UMA transação (`:1707`→`:2060`) com hooks de catalogação e generations dentro — atomicidade do done é invariante. `params->>'error'` é convenção só de comentário (`:1293-1294`), escrita em 3 pontos e lida em 2, sem enum de código.

**Proposta:** `src/reporting/{mod,metrics,artifacts,models,generations}.rs` — hooks viram fns que recebem `&mut PgConnection` (padrão já existente em `upsert_metrics_conn`), NÃO pool, para preservar a transação única do done. **Riscos (altos):** semântica best-effort dos hooks (warn-and-continue por design — converter em Err quebra o comportamento do incidente galeria-vazia); NENHUM teste de integração de `report_job` existe hoje (só 21 testes de fns puras) — mover 552 linhas de SQL sem rede de proteção exige antes MM-16.

### 3.5 watchdogs

**Localização:** `watchdog_prepare_timeout` (`lib.rs:1465-1477`), `gc_dataset_versions` (:1489-1505), `recover_jobs` (:3375-3396), `watchdog_tick` (:3551-3629); boot `main.rs:927-934, 971-996` (um único `tokio::spawn`, interval 2s hard-coded, watchdog→dispatch sequenciais no mesmo loop).

**Duplicação:** `recover_jobs` ↔ `watchdog_tick` são espelhos admitidos em comentário (`lib.rs:3572`); o par requeue é idêntico (`:3387-3394` vs `:3599-3600`), mas o par cancelling **diverge de fato**: recover grava `queue_reason='recovered_cancel'` sem limpar `orchestrator_id` (:3379-3385); watchdog limpa `orchestrator_id` sem queue_reason (:3577-3584) — provável inconsistência não escolhida. 60min e 7 dias são literais SQL sem env (`:1469,1494`); o domínio tem 3 regimes de config (literal SQL, env, literal Rust).

**Mistura:** `watchdog_tick` = leitura de env inline + 3 statements SQL + política de log + chaining de 2 sub-watchdogs, sem transação (5 statements autocommit). Compartilha `jobs.status`/`orchestrators.status` com dispatch via string literals, sem interface — salvaguardas atuais são incidentais (execução sequencial no mesmo task + SKIP LOCKED + revalidação READ COMMITTED).

**Proposta:** `src/watchdog/{mod,offline,janitor,recovery}.rs`; extrair `requeue_orphan_jobs(pool, escopo)` unificando os espelhos (após decidir a divergência cancelling); knobs em `WatchdogConfig` com binds `make_interval` mantendo defaults idênticos. **Riscos:** separar watchdog e dispatch em tasks distintas ANTES de fechar a janela commit→POST do dispatch aumenta a corrida já mapeada pela auditoria-irmã (T-M01) — extrair módulos sem separar loops. Zero testes dedicados hoje.

### 3.6 auth interna & bootstrap

**Localização:** `auth_middleware` (`main.rs:164-182`), `request_id_middleware` (:124-158), `resolve_manager_token` (:851-874), `build_router` (:783-842), `main()` (:876-1005), `AppState` (:30-40), error helpers (:71-118); isolamento: `infra/compose.yaml:157-170` (publish em loopback por default, `MANAGER_URL` interno na :45) e `infra/compose.prod.yaml:68-74` (`ports: !override []`, token obrigatório).

**Verificações:** 21 rotas `/internal/*` sob a layer de auth (`:822-825`); `/health`,`/ready`,`/metrics` públicas de propósito (probes/Prometheus). Comparação de token via `==` de String (`:176`) — não constant-time; mitigada por loopback/rede interna, correção barata (ver TASK-MGR-009). Config stringly-typed: 9 env vars inline com 2 `.expect` (`main.rs:884-905`); orchestrator já demonstra o padrão tipado (`services/orchestrator/src/config/mod.rs:38-129`).

**Duplicação:** o mapeamento `ManagerError`→HTTP repetido em 24 handlers (§1) é o maior volume de duplicação de main.rs; catch-all `internal_error(&e.to_string())` ×22 faz variantes novas caírem em 500 silenciosamente.

**Proposta:** `src/http/{mod,auth,error}.rs` + `src/config.rs` (`ManagerConfig::from_env`, espelhando orchestrator); `impl IntoResponse for ManagerError` com campo `detail` por contexto para preservar mensagens por rota. **Riscos:** extractor `Path<Uuid>` muda 404→400/422 para UUID inválido (9 handlers esperam `not_found`, 3 `bad_request` — decidir semântica única e atualizar testes `main.rs:1075-1092`); mover mensagens humanas por rota exige campo de contexto no erro.

---

## 4. Matriz Impacto × Esforço

Impacto = ganho de manutenibilidade + redução de duplicação. Esforço: P (<0,5 dia), M (1-3 dias), G (>3 dias). Risco = chance de regressão de comportamento.

| ID | Melhoria | Domínio | Impacto | Esforço | Risco | Dependências |
|---|---|---|---|---|---|---|
| MM-01 | Fix `prepare_cancel`: adicionar `.bind(id)` + teste de regressão | lifecycle | A (bug em produção) | P | baixo | — |
| MM-02 | `src/error.rs`: `impl IntoResponse for ManagerError` (+detail), elimina 24 matches e 22 catch-alls (~150-180 linhas) | transversal | A | P-M | médio (mensagens por rota) | — |
| MM-03 | Unificar guard de UUID (semântica 404 vs 400 decidida) — mata 12 guards | transversal | M | P | baixo | MM-02 (testes de rota) |
| MM-04 | Extractor de body JSON (empty+parse) — mata 10-11 guards | transversal | M | P | baixo | MM-02 |
| MM-05 | `src/config.rs` `ManagerConfig::from_env` + `WatchdogConfig` + knobs nomeados (60min/7d como binds com defaults) | transversal | M | P-M | baixo | — |
| MM-15 | Constantes de domínio: `TERMINAL_STATUSES` única, lista de archs única (3 cópias), enum de códigos `params.error` | transversal | M | P | baixo | — |
| MM-07 | `src/orchestrator/client.rs` (trait + HTTP client) — módulo leaf | dispatch/nodes | M | P | baixo | — |
| MM-08 | `src/policy/vram.rs` (loader + fixtures de teste unificados) — mata drift de 3 cópias do YAML | dispatch | M | P | baixo | — |
| MM-06 | `src/http/` (build_router, AppState, middlewares); `main.rs` → ~120 linhas | bootstrap | M | M | baixo | MM-02/03/04/05 |
| MM-09 | `src/watchdog/` + `requeue_orphan_jobs` unificado (decidir divergência cancelling) | watchdogs | M | M | médio (decisão de negócio) | MM-05 |
| MM-10 | `src/nodes/` (cache/heartbeat/telemetry/registry) + frescor de nó único | nodes | M | M | baixo | MM-07 |
| MM-12 | `src/jobs/resolve.rs`: `resolve_model_ref` dedup ×3 + extração dos 4 blocos de create_job (~370 linhas) | dispatch/lifecycle | A | M | médio | MM-07, MM-15 |
| MM-13 | `src/jobs/` lifecycle+abort+delete com `repo.rs` (predicado de fila, row→JobRow, helper de transição) | lifecycle | A | M | médio | MM-02, MM-12 |
| MM-16 | Testes de integração para `report_job`/`dispatch_next` antes do split (sqlx::test / rede mock) | transversal | A (habilita) | M | baixo | — |
| MM-11 | Split de `report_job` em `src/reporting/` preservando tx única do done e warn-and-continue | reporting | A | G | alto sem MM-16 | MM-15, MM-16 |
| MM-14 | `src/dispatch/` (dispatcher/election/payload) preservando SKIP LOCKED, commit-antes-POST e payload snake_case | dispatch | A | G | alto | MM-07, MM-08, MM-12, MM-16 |

---

## 5. Ordem de Execução Recomendada

**Fase 0 — hotfix (imediatamente, PR isolado):** MM-01. Uma linha + teste de regressão em `tests/manager_db.rs`. Independente de tudo.

**Fase 1 — base transversal (quick wins, ~40% de redução do main.rs):** MM-02 → MM-03 → MM-04 (transporte), MM-05 e MM-15 em paralelo (arquivos disjuntos). Nenhum movimento de domínio; wire e SQL intocados.

**Fase 2 — módulos leaf e casca:** MM-07, MM-08, depois MM-06. Re-exports em `lib.rs` preservam a API `manager::*` usada pelo binário.

**Fase 3 — domínios de baixo acoplamento (podem ser PRs paralelos por donos diferentes):** MM-09 (watchdog: só PgPool), MM-10 (nodes), MM-12 (resolve.rs: maior dedup por linha movida — mata o padrão triplo de query-models).

**Fase 4 — core, somente após MM-16:** MM-13 (jobs) → MM-11 (reporting) → MM-14 (dispatch, por último: maior risco de locking e contrato de payload). Cada extração é PR mecânico — proibido "aproveitar e reescrever" (mesma disciplina de TASK-MGR-005 da auditoria-irmã).

**Mapeamento com TASK-MGR (auditoria 2026-09-20):** MM-02/03/04/06 ≈ TASK-MGR-005(a); MM-11/13 ≈ 005(b); MM-09/10 ≈ 005(c); MM-05 ≈ TASK-MGR-006; MM-14 ≈ 005(c)+003; MM-16 ≈ TASK-MGR-010; MM-01 é novo e deve entrar como PR0 ao lado de TASK-MGR-009.

## 6. O que NÃO mudar na extração (invariantes)

1. Atomicidade do bloco `done` do `report_job` (transação única `lib.rs:1707-2060`) — hooks recebem `&mut PgConnection`, nunca pool.
2. Semântica best-effort warn-and-continue dos hooks de models/generations.
3. Guardas não-erro silenciosas de `report_job` (`Ok(())` em estado não esperado, `lib.rs:1633-1638`).
4. `FOR UPDATE SKIP LOCKED` + `FOR UPDATE OF o` e a ordem commit→POST→compensação de `dispatch_next`.
5. Payload de dispatch byte-a-byte (snake_case interno; md5 nullable em `init_image_ref`).
6. Defaults de timing (60min, 7d, 2s, 15s, 60s, 10s) — só permitir override por env, nunca alterar valor em deploy existente.
7. Divergência recover↔watchdog no par cancelling só após decisão explícita de qual comportamento é o correto (consumidores de `queue_reason`).
