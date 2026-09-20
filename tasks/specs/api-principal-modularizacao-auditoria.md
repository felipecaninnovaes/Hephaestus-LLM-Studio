# Auditoria Técnica e Plano de Modularização: `services/api-principal`

**Projeto:** Hephaestus LLM Studio
**Data:** 20 de Setembro de 2026
**Escopo:** BFF `api-principal` (:8080) — Rust/Axum 0.7/SQLx 0.8; contrato público `packages/contracts/openapi.yaml` (63 paths), protocolo interno `crates/heph-contracts` + `/internal/*` do manager, Postgres compartilhado com manager
**Status:** Concluído (Auditoria Estritamente Somente Leitura — 10 varreduras paralelas + validação por amostragem)

---

## 1. Resumo Executivo & Métricas

`api-principal` é o serviço mais saudável dos três em Rust: zero `unsafe`, zero TODO/FIXME, zero `unwrap/expect/panic` em caminho de produção (todos os ~500 contados estão em `#[cfg(test)]`, exceto 2 `expect` no `MockStorage::snapshot/ops`), envelope de erro D6 central, camelCase travado por teste de contrato, ports com mocks (`StoragePort`/`EmbeddingPort`/`ManagerPort`), SSRF fail-closed em 3 camadas, zip-slip defendido, e os 4 invariantes auditados **respeitados no código**.

O problema não é higiene — é **forma estrutural**: 12 arquivos >500 linhas concentrando transporte + regra de negócio + SQL + I/O; **~150 queries SQL inline, zero camada de repositório**; o crate compartilhado `heph-contracts` é declarado dependência mas **usado 0 vezes** (o protocolo interno do BFF é 9 structs `Internal*` duplicadas à mão); status de job é `String` com literais espalhados; e 6,75% das linhas são clones (jscpd, 148 clones). O risco operacional real mais agudo é externo ao código-fonte: **o diretório de migrations do api-principal é o schema efetivo do manager** (sem CI que o prove) e **não há graceful shutdown** (SIGTERM mata pacotes/export de minutos no meio).

### Métricas por módulo

| Métrica | Valor | Nota |
|---|---|---|
| LOC total `src/` | **27.073** em 37 arquivos | + 9.912 em `tests/` + 560 em `examples/` |
| Arquivos >500 linhas | **16** (12 >800) | `jobs/handlers.rs` 5472 é o pior do monorepo |
| Funções de transporte >80 linhas | **18** | top: `apply_autotracker_boxes` 402, `submit_diffusion_job` 387, `import_dataset` 490, `upload` 329 |
| `sqlx::query*` inline | **~150 sítios / 0 repositórios** | `query!` macro: 0 ocorrências (schema só confere em runtime) |
| `unwrap/expect/panic` fora de teste | **2** (ambos no mock, `storage/mock.rs:71,81`) | produção: 0 — régua a proteger com lint |
| Duplicação (jscpd `--min-tokens 70`) | **6,75% linhas / 8,39% tokens, 148 clones** | padrões submit/preview-apply, `test_state`, remaps camelCase |
| `ManagerPort` | **24 métodos**, 9 DTOs `Internal*` locais | `heph_contracts` = 0 usos no src (grep confirmado) |
| `tokio::spawn` prod | 7 + 4 `spawn_blocking` | 2 loops de boot e 3 indexadores sem handle/`catch_unwind` |
| Compilação | `cargo check -p api-principal --locked --tests`: **ok ~40s**, exit 0 | verificado nesta auditoria (`CARGO_TARGET_DIR=/tmp/heph-audit-target`) |
| Testes | 25 módulos inline (~500 testes) + `contract.rs` 15 ativos + `datasets_db.rs` ~92 `--ignored` | gap: `submit_diffusion_job` e SSE sem teste ativo |
| Tooling | sem `clippy.toml`/`rustfmt.toml`/`deny.toml`; sem machete/audit/deny | recomendação §8-F7 |

### Top 10 arquivos

| # | Arquivo | Linhas | Diagnóstico |
|---|---|---|---|
| 1 | `src/jobs/handlers.rs` | 5472 | 3302 prod + 2170 testes inline; 11 handlers >80 linhas |
| 2 | `src/jobs/models.rs` | 3827 | 1842 prod: DTOs de 6 tipos de job + validação + gerador de YAML + VRAM |
| 3 | `src/datasets/handlers.rs` | 1947 | 7 handlers >80, ~45 sítios SQL, zero `#[cfg(test)]` |
| 4 | `src/jobs/manager_client.rs` | 1598 | protocolo interno copiado, boilerplate ×24 |
| 5 | `src/datasets/package.rs` | 1436 | dois `build_package_*` de ~300 linhas quase clones |
| 6 | `src/models/handlers.rs` | 1431 | upload+download+catálogo+finalize num arquivo só |
| 7 | `src/datasets/models.rs` | 1341 | misto: regras puras (ótimas) + DTO wire |
| 8 | `src/datasets/export.rs` | 1152 | terceiro clonador do pipeline zip/S3 |
| 9 | `src/jobs/prepare.rs` | 1128 | bem subdividido; maior função 124 linhas — ok |
| 10 | `src/generations/handlers.rs` | 961 | export ZIP com N+1 serial e `TempDir` em stream |

---

## 2. Divergências entre Documentação e Código Real

`docs/services/api-principal.md` tratado como hipótese. Cada linha foi confrontada com o código:

| # | Alegação documental | Realidade no código | Evidência | Impacto |
|---|---|---|---|---|
| D-01 | SSE em `GET /api/jobs/:id/telemetry` | A rota real é `GET /api/jobs/:id/events` | `src/auth/routes.rs:439`, `jobs/handlers.rs:552`, `openapi.yaml:2132` | Média — doc erra o path exato |
| D-02 | Domínios: auth/datasets/models/jobs/search/generations | **Não documentados**: `/api/orchestrators/adopt|revoke`, `/api/environments/*`, `/api/telemetry`, `/api/jobs/cleanup`, `GET /metrics`, `GET /ready`, e `monitoring.rs` inteiro como camada de proxy | `src/monitoring.rs:129-351`, `auth/routes.rs:347-593`, `openapi.yaml:227-304,2247` | Alta — 1/4 da superfície ausente do doc; `monitoring.rs` tem nome enganoso (não é monitoramento, são proxies) |
| D-03 | Credenciais `STUDIO_PASSWORD`, `STUDIO_MASTER_KEY` | `STUDIO_MASTER_KEY` **não existe** no código; segredo JWT tem 3 fontes (`AUTH_SECRET` env > tabela `auth_state` > gerar) | `main.rs:161`, `auth/secret.rs:28-72` | Média |
| D-04 | Cookie `Secure` "em conexões TLS" | `SECURE_COOKIE=="true"` **OU** header `x-forwarded-proto: https` — forjável contra :8080 direto | `auth/handlers.rs:109-113` | Média (ver S-06) |
| D-05 | "`normalize.rs` sanitiza metadados/nomes de classes/tags" | `normalize.rs` normaliza **pixels** (decode→WebP→hash); sanitização de nomes vive em `datasets/models.rs` e `storage/keys.rs` | `datasets/normalize.rs:41-78`, `datasets/models.rs` (`is_valid_class_name`) | Baixa |
| D-06 | Upload chunked "com sweep de sessões abandonadas" | Confirmado — mas o **próprio header do módulo** (`chunk.rs:13-15`) ainda diz "Dívida conhecida: sem TTL/GC" | `chunk.rs:63-77`, `storage/gc.rs:16`, `main.rs:260-268` | Baixa (doc interno obsoleto; o serviço está certo) |
| D-07 | `docs/REPO_MAP.md:81-82` "multipart único ≤96 MiB na prática" | Teto real do upload único é **8 GiB**; 96 MiB é só o tamanho da parte chunked | `models/validate.rs:40`, `models/handlers.rs:677-694` (comentário diz "cap 2 GiB" — também stale) | Média |
| D-08 | openapi: "todas as partes exceto a última têm exatamente `partSize`" | `put_part` aceita parte curta fora de ordem; só a soma é checada no `complete` | `openapi.yaml:4929-4931` × `chunk.rs:214-300` | Média — spec stricter que o código |
| D-09 | `migrations/0006:13` "dataset_versions Dono: principal" | manager **também escreve** (touch `created_at`) e roda GC próprio com predicado diferente | `manager/lib.rs:1361,1477-1486` × `storage/gc.rs:45-54` | Alta — risco de dupla poda |
| D-10 | `migrations/0017:7` generation_inputs "sem GC, linhas permanecem p/ auditoria" | `storage/gc.rs:84-88` apaga linhas >7 dias | — | Média |
| D-11 | Doc não descreve o fluxo `preparing`/202 assíncrono | `accept_job_preparing`, dedupe 30min, fingerprint, recovery no boot, watchdog 60s | `jobs/prepare.rs:225-441,811-880`, `main.rs:236-258` | Alta — submit ≠ treino imediato é invisível no doc |
| D-12 | Doc sugere push no SSE | Implementação é **polling `get_job` a cada 300ms por conexão** com change-filter | `jobs/handlers.rs:616-617` | Média — mecanismo não documentado (amplificação de carga, ver T-06) |
| D-13 | `packages/policies/vram-table.yaml` "fonte única de VRAM" | BFF hardcode `sd15→8/flux→10/`default→12` e 4 submits enviam `null`; manager grava `vram_min_gb` mas o **ignora na fila** (usa a tabela); valores hardcoded divergem da tabela | `jobs/handlers.rs:1697-1701` × `vram-table.yaml` × `manager/lib.rs:491,3797` | Média — campo de exibição divergente, não risco de scheduler |
| D-14 | `manager_client.rs:300-304,853-856` "rota `/internal/generations/:id` não existe" | Existe desde `manager/main.rs:824` | comentário stale | Baixa |

**Divergência mais grave de todas é a D-09/DB-01 combinadas com o fato estrutural:** `services/api-principal/migrations/` é o único diretório de migrations do monorepo e o manager (sem migrations próprias) lê/escreve 8 dessas tabelas. Ver §5 INV-4 e tarefa TASK-API-002.

---

## 3. Arquitetura-Alvo

Parte do que existe (feature-modules + ports já provados), não impõe Clean Architecture completa. A camada que falta é **repositório** e **caso de uso**; a que não deve ser criada é "domínio puro compartilhado entre serviços" — os serviços permanecem independentes por contrato.

### 3.1 Dentro de `services/api-principal/src/<feature>/` (padrão por fatia)

```
jobs/
├── mod.rs                  # re-export enxuto
├── routes.rs               # (hoje em auth/routes.rs → migrar para a feature) registro + DefaultBodyLimit por grupo
├── handlers.rs             # TRANSPORTE: extrai → valida DTO → chama caso de uso → mapeia erro→HTTP. Sem SQL.
├── usecases.rs             # APLICAÇÃO: submit_diffusion, apply_autotracker (resolve→decide→emit)
├── model.rs                # DOMÍNIO: JobStatus enum, DTOs wire camelCase, geradores de YAML, consts (96MiB, limites)
├── repository.rs           # INFRA: todo sqlx do domínio jobs
└── prepare.rs              # (mantém) worker de preparação — já bem desenhado
```

Regras: handler ≤60 linhas sem SQL; função de caso de uso ≤120; arquivo ≤600 linhas; transição de estado de job só via `JobStatus` (`is_terminal()` único); todo `sqlx` fora de `repository.rs` é violação.

### 3.2 Bootstrap e estado

- `main.rs` (355 → ~120): extrair `boot/config.rs` (struct tipada `Config::from_env()` — hoje são 12+ `std::env::var` soltos em main), `boot/tracing.rs`, `boot/workers.rs` (GC 600s, recovery 60s, sweeps — com `CancellationToken` e `JoinSet`), servidor com `with_graceful_shutdown`.
- `AppState` (11 campos): fatiar por handler — `State<Pool> + State<ManagerArc>` onde possível, ou sub-structs (`Db`, `Sec`, `Ports`); hoje `monitoring::get_orchestrators` recebe pool+storage+embedder que nunca usa.

### 3.3 Crates compartilhados candidatos

| Crate | Conteúdo | O que migra | Prioridade |
|---|---|---|---|
| `crates/heph-contracts` (**já existe, subusado**) | DTOs do protocolo interno | api-principal passa a **usar**: `JobRow`, `ArtifactRow`, `TelemetryResponse`, `OrchestratorItem`, `ModelItem`, `GenerationRow`, `PackageRef` unificado (com `version_id: Option`); deletar as 9 structs `Internal*` + `MetricsItem`/`JobTelemetryEvent` duplicados (`jobs/handlers.rs:51,83` vs `telemetry.rs:6,29`) | **P1 — primeiro passo** |
| candidato `heph-manager-client` (ou módulo do contrato) | `get/post/delete_json` tipados com Bearer `MANAGER_TOKEN`, timeouts, retry por classe, `ManagerError` | boilerplate ×24 de `manager_client.rs:393-978` | P2 |
| `packages/contracts/openapi.yaml` | erro `{code,message}` | listar os `code` como `components.schemas` — hoje o envelope existe (`error.rs`) mas os códigos estão só em comentários; testar erro×contrato | P2 |
| `crates/heph-config` / `heph-telemetry` | env tipado + tracing json + request_id | só quando um 2º serviço precisar; não criar antes | P3 |

### 3.4 Camada de banco

`repository.rs` por feature (datasets/jobs/search/generations) com as queries existentes movidas **sem reescrita**; adotar padrão `unnest` em lote (régua: `datasets/handlers.rs:200-203`) para matar N+1 (`jobs/handlers.rs:2484-2528`, `search/indexer.rs:147-151`); `list_jobs` paginado antes da fila crescer (`manager/lib.rs:986-1022` sem LIMIT). **Não** trocar PgPool por trait agora (custo alto, TT-06): validar parse/validação antes de qualquer query (padrão `contract.rs:413-570`) mantém ramos 400/404 sem DB.

---

## 4. Matriz de Duplicação

| # | O que | Onde (2+ lugares) | Para onde | Tarefa |
|---|---|---|---|---|
| X-01 | Protocolo interno (dispatch/report/telemetry/jobs) | `heph-contracts` (crate) × `manager_client.rs:49-215` (9 structs `Internal*`) × `jobs/handlers.rs:51,83` (`MetricsItem`/`JobTelemetryEvent` idênticos a `telemetry.rs:6,29`) | `heph-contracts` (usar, não recriar) | TASK-API-001 |
| X-02 | Esqueleto de `submit_*` (Bytes→validate→404→readiness→fingerprint→accept) | 6× em `jobs/handlers.rs:820-2141` | um guard/extractor de submit + tabela de readiness | TASK-API-004 |
| X-03 | Pipeline preview/apply (gate done→dataset→artifacts→get→md5→parse) | 4× (`jobs/handlers.rs:2318/2728/2898/3077`) | `load_artifact_bytes(job, kind)` compartilhado | TASK-API-004 |
| X-04 | Pipeline package (SQL→materializar→zip→PUT→INSERT→compensar) | 3× (`package.rs:275-579` vs `:583-853` × `export.rs`) com compensação `delete_prefix` copiada (481-561 vs 794-834) | pipeline único parametrizado por layout (yolo_tree/diffusion_txt) | TASK-API-005 |
| X-05 | Validações UUID/conf/XOR-weights por tipo de job | `jobs/models.rs:160-1003` (~12 sítios) | helpers `must_uuid`/`must_conf`/XOR parametrizado | TASK-API-004 |
| X-06 | SELECT derivado dataset+auto_tracked+trash | `datasets/handlers.rs:55,171,246` + `import.rs:761` | 1 função de repositório | TASK-API-003 |
| X-07 | `ManagerError→Response` mapeado à mão | ~40 sítios (`jobs/handlers.rs`, `monitoring.rs:130-351`, `models/handlers.rs:294`, `generations:240`) | `impl From<ManagerError> for Response` único | TASK-API-004 |
| X-08 | `internal()`/`is_too_large`/consts de colunas | copiados em 4+ arquivos de datasets | `crate::error` / `datasets::common` | TASK-API-007 |
| X-09 | `test_state()` com `connect_lazy`+Mocks idêntico | 6+ módulos de teste (`handlers.rs:4683`, `contract.rs:33`, `datasets_db.rs:53`) | `tests/common/mod.rs` | TASK-API-008 |
| X-10 | Política VRAM | `vram-table.yaml` (canônica) × hardcode `jobs/handlers.rs:1697` × fn `models.rs:1704` | BFF lê a tabela (embed build-time) **ou** remove o campo wire `vram_min_gb` (hoje no-op no manager — `manager/lib.rs:491`) | TASK-API-006 |
| X-11 | Filename canônico | `storage/keys.rs` × reconstruído inline em `generations/inputs.rs:203-207` | `keys::canonical_filename` | TASK-API-007 |

Duplicação estimada: **6,75% das linhas** (jscpd 148 clones; padrão de match-arm e test_state responde pela maioria).

---

## 5. Verificação dos Invariantes

| Invariante | Veredito | Evidência |
|---|---|---|
| **INV-1** Upload chunked: streaming RAM O(1), partes ≤96 MiB, TempDir drop-clean, sweep de abandonadas | **RESPEITADO** (sub-lacuna: sem teto de sessões concorrentes) | streaming `chunk.rs:250-290` (`into_data_stream`+`write_all` por frame); teto duplo `chunk.rs:258` (96MiB→413) + `routes.rs:535` (`DefaultBodyLimit` 104MiB), provado por teste com 97×1MiB (`chunk.rs:595-625`); `.tmp`+rename atômico `chunk.rs:293-300`; sessão removida antes de validar `chunk.rs:330`; sweep TTL 1h/10min `gc.rs:16` + `main.rs:260-268`. Lacuna: `init_upload` (135-211) sem cap de sessões → N inits retêm N TempDirs por 1h (T-14) |
| **INV-2** SSE só emite em mudança real; encerra em terminal | **RESPEITADO** (com ressalvas de mecanismo) | change-filter `jobs/handlers.rs:630-637`; `snapshot` imediato 599-612; `finished`+`terminal_sent→None` 646-649,595-597. Ressalvas: é polling de `get_job` a cada 300ms **por conexão** (616-619) — doc sugere push (D-12); erro de manager `continue` infinito sem teto (621-623); sem idle-timeout (T-06) |
| **INV-3** Máquina de estados queued→preparing→running→done/failed/cancelled | **PARCIALMENTE VIOLADO** | A sequência é a operada, mas **não existe tipo**: `InternalJob.status: String` (`manager_client.rs:56`); literais dos 6 estados em ~14 sítios no BFF (`jobs/handlers.rs:135-148,437-447,578,628` + `prepare.rs:296,314-335,696`); 3 vocabulários paralelos (DB CHECK `0006:22-24` / `report.rs:7` `String` / wire `done→completed`, `failed→error`) com mapeamento **duplicado** em `from_job_response`×`to_job_response` (divergência futura garantida). Transição validada só no manager. Tarefa TASK-API-003 |
| **INV-4** Manager só acessível via BFF + `MANAGER_TOKEN` | **RESPEITADO no BFF** | `manager: Arc<dyn ManagerPort>` único (`state.rs:22`); nenhum outro `reqwest` a `:8081` (grep `8081` só no default URL `main.rs:217`); browser headers/token nunca repassados (`manager_client.rs:393-395` server-side); todas as rotas `/api/*` sob `route_layer(require_auth)` (`routes.rs:578-581`), inventário + teste `contract.rs:350-390`. **Viés externo ao escopo desta fatia**: compose dev publica `manager 8081:8081` no host + token default `changeme` (`compose.yaml:46,167`; análise completa em `tasks/backend-autonomia.md`/fatia infra) |
| **INV-extra** Watchdogs/reconciliação do lado BFF (preparing >10min, attempts<3) | **RESPEITADO** | `prepare.rs:811-880` + worker 60s `main.rs:248-258`; `catch_unwind→fail_prepare` `prepare.rs:452-480` |

Sem proposta de mudança em nenhum invariante — apenas proteção e fechamento das sub-lacunas (T-06, T-14, TASK-API-003).

---

## 6. Tabela Geral de Achados

Impacto A/M/B · Esforço P/M/G · Pri P0–P3. Origem = fatia de varredura (A1 transporte-jobs, A2 domínio-jobs, A3 cliente-manager, A4 models/upload, A5 datasets/storage, A6 auth/search/mon, B banco, C async, D testes, E segurança).

| ID | Título | Evidência-chave | Imp | Eff | Risco regressão | Pri | Tipo | Quebra contrato? | Deploy coord.? | Origem |
|---|---|---|---|---|---|---|---|---|---|---|
| **T-01** | Defaults `changeme` de `STUDIO_PASSWORD`/`MANAGER_TOKEN` neutralizam bootstrap fail-closed do dia 1 | `infra/compose.yaml:44-46`; `main.rs:161-185` (env presente ⇒ nunca gera senha) | A | P | baixo | **P0** | quick-win | não | sim (operador) | E |
| **T-02** | Migrations do BFF são o schema do manager; nenhuma trava de CI; dupla escrita/GC em `dataset_versions` | `main.rs:145-148`; manager sem `migrate!`; `manager/lib.rs:1361,1477` × `storage/gc.rs:45-54` | A | M | alto | **P0** | estrutural | sim (schema) | sim | B |
| **T-03** | Sem graceful shutdown: SIGTERM mata package/export/upload em voo | `main.rs:270-279` (`axum::serve` nu); zero `signal/CancellationToken` | A | M | alto | **P1** | estrutural | não | sim | C |
| **T-04** | `heph-contracts` dependido, 0 vezes usado; protocolo interno duplicado em 12+ structs | Cargo.toml:36; `manager_client.rs:49-215`; `jobs/handlers.rs:51,83` × `telemetry.rs:6,29`; dispatch manager via `json!` (`manager/lib.rs:3958-4005`) | A | M | médio | **P1** | estrutural | não (wire igual) | sim (3 serviços) | A3 |
| **T-05** | `status` de job = `String` em 3 vocabulários; mapeamento duplicado | `manager_client.rs:56`; `0006:22-24`; `report.rs:7`; `jobs/handlers.rs:135-148 vs 437-447` | A | M | médio | **P1** | estrutural | só se serializar diferente | sim | A1/A2/B |
| **T-06** | SSE = polling 300ms/conexão, erro de manager engolido `continue` (loop sem teto), magic numbers | `jobs/handlers.rs:616-624` | M | P | baixo | **P1** | quick-win | não | não | A1/C |
| **T-07** | Busca vetorial: `LIMIT` no SQL **antes** dos pós-filtros Rust → resultados perdidos silenciosamente | `search/handlers.rs:234-280` | M | M | médio | **P1** | estrutural (correção) | não | não | A6 |
| **T-08** | Login sem rate-limit + argon2 síncrono no executor (brute-force + DoS de event loop) | `auth/handlers.rs:145-167`; `password.rs:30-48`; sem `governor` no Cargo | A | P | baixo | **P1** | quick-win | 429 novo (aditivo) | não | E/A6 |
| **T-09** | JWT não revogável: logout só limpa cookie, token vale até `exp` 7d | `auth/session.rs:12,54-58`; `handlers.rs:199-215` | M | M | médio | **P1** | estrutural | não | sim (web) | A6/E |
| **T-10** | Monólitos de transporte: 11 handlers de jobs >80 linhas (402/387/265), ~29 SQL inline; 7 handlers de datasets >80 (329/210), ~45 SQL | `jobs/handlers.rs:2318,1402,1128`; `datasets/handlers.rs:618-910` | A | G | médio | **P1** | estrutural | não | não | A1/A5 |
| **T-11** | Import de dataset: zip/descompressão **síncronos no executor** (segundos de bloqueio) + não atômico (falha apaga dataset e re-varre S3) | `datasets/import.rs:448,472,208-303`; tx só header `:558-636` + `cleanup_failed_import:62-71` | A | M | médio | **P1** | estrutural | não | não | C/B |
| **T-12** | Bloqueio CPU em async: argon2 verify/hash, `normalize_image` em loop de upload | `auth/handlers.rs:166`; `datasets/handlers.rs:762` | A | P | baixo | **P1** | quick-win | não | não | C |
| **T-13** | VRAM: BFF hardcode divergente da tabela canônica (campo de exibição; manager no-op `vram_min_gb`) | `jobs/handlers.rs:1697-1701` × `vram-table.yaml`; `manager/lib.rs:491` | M | M | baixo | **P1** | estrutural | não | não | A2 |
| **T-14** | Upload chunked sem teto de sessões concorrentes (amplificação de disco até o sweep de 1h) | `chunk.rs:135-211` | M | P | baixo | **P2** | quick-win | 429 novo | não | A4 |
| **T-15** | `/metrics` montada fora do inventário D8 (`PUBLIC_ROUTES`/`routes_all`) → furo silencioso no teste de contrato; `/health` revela `setup_required` | `auth/routes.rs:585 vs 206-212`; `routes.rs:244-253` | M | P | baixo | **P2** | quick-win | documentar | não | A6/E |
| **T-16** | Boilerplate do cliente manager: 24 métodos ×auth header+match+json repetidos; retry só em 3 mutações; query sem percent-encode | `manager_client.rs:393-978,439-447` | A | M | médio | **P2** | estrutural | não | não | A3 |
| **T-17** | `PackageRef` duplo incompatível (manager interno × crate); `InternalModelResponse.url` campo morto | `manager/lib.rs:71-76` × `dispatch.rs:7-11`; `manager_client.rs:182-201` | A | M | médio | **P2** | estrutural | sim (internamente) | sim | A3 |
| **T-18** | `list_queue` transfere tabela inteira p/ derivar fila; `list_jobs` sem LIMIT | `manager_client.rs:469-489`; `manager/lib.rs:986-1022` | M | P | baixo | **P2** | quick-win | aditivo | sim (manager 1º) | A3/B |
| **T-19** | Pares preview/apply e submit_* ~60% idênticos; `ManagerError→Response` ×40 | §4 X-02/03/07 | M | M | baixo | **P2** | estrutural | não | não | A1 |
| **T-20** | `dataset_versions`/CHECKs de vocabulário como tripla fonte (DDL × manager × BFF) | `0008/0010/0011/0014/0018` × `manager/lib.rs:650-746` × `jobs/*` | M | M | médio | **P1** | estrutural | sim | sim | B |
| **T-21** | apply_autotracker: 1 tx **por imagem** (parcial sem cursor) + N+1 de classes (3 round-trips/nome) | `jobs/handlers.rs:2599-2660, 2484-2528` | M | M | médio | **P2** | estrutural | não | não | B |
| **T-22** | `dataset_versions` GC duplo com predicados diferentes (possível delete divergente) | `storage/gc.rs:45-54` × `manager/lib.rs:1477-1486` | A | M | alto | **P1** | estrutural | não | sim | B |
| **T-23** | Testes: `submit_diffusion_job` e SSE sem cobertura ativa (2 testes `#[ignore]`); `datasets/handlers.rs` zero unidade (só PG) | `jobs/handlers.rs:5284-5359`; `datasets/handlers.rs` sem `#[cfg(test)]` | A | P-M | baixo | **P1** | quick-win | não | não | D |
| **T-24** | `datasets_db.rs` 8257 linhas + helpers duplicados (`test_state` ×6, `call/json`) | `tests/datasets_db.rs:1-8257`; `contract.rs:33-120` | M | M | médio | **P2** | estrutural | não | não | D |
| **T-25** | SSRF download: IPv4-mapped `::ffff:127.0.0.1` escapa; falta `0/8`,`100.64/10`; DNS-fail fail-open por hop; wildcard casa apex | `models/handlers.rs:131-149`; `validate.rs:144-210` | M | P-M | baixo | **P2** | quick-win | não | não | E/A4 |
| **T-26** | Senha de bootstrap em campo estruturado de log JSON (persistido por shipper) | `main.rs:174-176` | M | P | baixo | **P1** | quick-win | não | sim (runbook) | E/A6 |
| **T-27** | Bomba de descompressão de imagem (decode sem pixel-limit) + filename externo em `join` no export + `eprintln` espalhado | `normalize.rs:41-79`, `generations/handlers.rs:270-282,437,448` | M | P | baixo | **P2** | quick-win | não | não | E/A5 |
| **T-28** | Loops de boot sem `catch_unwind`/handle; indexer fire-and-forget; sem observabilidade de falha | `main.rs:250,262`; `search/handlers.rs:88`, `indexer.rs:36-107` (advisory lock sem guarda RAII; `acquire_owned` ignorado `:107`) | M | P | baixo | **P2** | quick-win | não | não | C/A6 |
| **T-29** | `err()` só aceita `&'static str`: erros dinâmicos colapsam; ~36 SQL-fail → 500 `internal` genérico | `error.rs:20`; `jobs/handlers.rs:855-3231` | M | M | baixo | **P2** | estrutural | não | não | A1/A6 |
| **T-30** | Limpeza: `#[allow(dead_code)]` ×2, `expect` no `MockStorage::snapshot`, spikes "NÃO mergear" commitados, comentários stale (cap 2GiB=8GiB; rota existe; chunk header) | `models/handlers.rs:1017`, `jobs/handlers.rs:4320`, `mock.rs:71,81`, `examples/storage_spike*.rs`, `UP-01/07/09` | B | P | nenhum | **P3** | quick-win | não | não | A4/D |
| **T-31** | Tooling ausente: `clippy.toml` (p/ `unwrap_used`+`await_holding_lock`), `rustfmt.toml`, `deny.toml`, machete/audit; `serde_yaml` deprecated; hash triplo md-5/sha1/sha2 0.10+0.11 no grafo | glob (0 configs); `Cargo.lock:2033,2985,3588` | B | P | nenhum | **P3** | quick-win | não | não | D |
| **T-32** | `monitoring.rs` nome enganoso (proxies), `get_models` de models lá dentro; PATCH `update_model` fura `sanitize_model_name` | `monitoring.rs:1-20,163`; `models/handlers.rs:900-960` × `validate.rs:82-130` | B | P | baixo | **P2** | quick-win | só se PATCH apertar (400 novo) | não | A4/A6 |

### Detalhamento — tarefas estruturais (as demais são auto-explicativas na tabela)

#### [TASK-API-001] Consumir `heph-contracts` e deletar DTOs duplicados (T-04, T-17)
- **Evidência:** `services/api-principal/Cargo.toml:36` vs 0 `use heph_contracts` no src; `manager_client.rs:49-215` (`Internal*`×9); `jobs/handlers.rs:51,83` ≡ `heph-contracts/telemetry.rs:6,29`; dois `PackageRef` (`manager/lib.rs:71-76` × `dispatch.rs:7-11`).
- **Problema:** o protocolo interno vive em 3 representações (crate, manager local, BFF local); drift é garantido, hoje campo-a-campo compatível só por disciplina.
- **Proposta:** (1) unificar `PackageRef` no crate com `version_id: Option` + aliases serde; (2) api-principal desserializa direto os tipos do crate em `manager_client`; (3) wire público continua mapeado no BFF (snake→camel) — openapi intocado; (4) manager passa a montar dispatch com `DispatchRequest` tipado em vez de `json!` (elimina T-17c).
- **Aceite:** `rg 'struct Internal' services/api-principal/src` = 0; `cargo check --workspace --locked` e `cargo clippy --locked` limpos; testes de contrato (`contract.rs`) verdes sem nenhuma mudança em `openapi.yaml`; payloads de `tests/datasets_db.rs --ignored` inalterados byte-a-byte nos snapshots JSON.
- **Esforço:** M | **Risco:** médio (bump da crate nos 3 serviços — rollout: crate→manager→principal na mesma janela; wire idêntico torna ordem irrelevante) | **Dep:** nenhuma. **PR1 do roadmap.**

#### [TASK-API-002] Migrations como contrato versionado com CI de compatibilidade (T-02)
- **Evidência:** único `migrations/` no BFF (`main.rs:145-148`); manager lê/escreve `jobs/models/orchestrators/job_artifacts/generations/dataset_versions/…` sem `migrate!` nem diretório próprio; `0016:12` já prova que sqlx rejeita migration mutada (append-only de facto).
- **Proposta:** manter diretório único (dividir agora = custo sem ganho no estágio atual do projeto); **registrar a posse em `docs/`**; CI: (a) job que roda as migrations sobre dump do schema + suite `--ignored` do **manager** contra a migration nova antes do merge; (b) gate que bloqueia `DROP/ALTER TYPE/rename` sem ack; (c) `sqlx` `query!` não adotado em produção (offline build) — opcional.
- **Aceite:** doc de posse por tabela (matriz do relatório da fatia B) em `docs/REPO_MAP.md`; pipeline verde com migration "break-the-manager" simulada rejeitada.
- **Esforço:** M | **Risco:** nenhum (só adiciona trava) | **Dep:** nenhuma. **PR2.**

#### [TASK-API-003] `JobStatus` enum central no BFF (T-05)
- **Evidência:** `manager_client.rs:56` `pub status: String`; `jobs/handlers.rs:135-148` vs `:437-447` (mapeamentos `done→completed` duplicados com fallbacks diferentes); literais em `prepare.rs:296,314-335,696`, SSE `578,628`, guards `2361,2751,2922,3111`.
- **Proposta:** `enum JobStatus {Queued,Preparing,Dispatched,Running,Done,Failed,Cancelled,Cancelling}` em `heph-contracts` (serde lowercase, `#[serde(other)] Unknown` p/ compat), `is_terminal()`, `to_wire()` único; serialização **idêntica** ao wire atual.
- **Aceite:** `rg '"(queued|preparing|running|done|failed|cancelled)"' services/api-principal/src` só em `cfg(test)`/serde defs; teste de round-trip enum×strings do `0006` CHECK; contrato inalterado.
- **Esforço:** M | **Risco:** médio (fuso com `0013` phase/message; manter strings no wire) | **Dep:** TASK-API-001 (mesmo crate). **PR3.**

#### [TASK-API-004] Camada de aplicação em `jobs/`: decompor os 6 `submit_*` e os pares preview/apply (T-10, T-19)
- **Fases:** (a) extrair `jobs/repository.rs` (~29 SQL) e `generations/datasets` idem — movidos, não reescritos; (b) guard único de submit (Bytes→DTO→404→readiness→fingerprint) e `load_artifact_bytes()` compartilhado; (c) handlers com ≤60 linhas.
- **Aceite:** nenhum handler de `jobs/handlers.rs` >100 linhas; `sqlx` restrito a `*/repository.rs`; todos os ~104 testes inline de `jobs/handlers.rs` continuam verdes; contrato e snapshots wire inalterados.
- **Esforço:** G (3 PRs encadeáveis) | **Risco:** médio | **Dep:** TASK-API-001 (DTOs estáveis primeiro).

#### [TASK-API-005] Pipeline de package/export unificado (T-10 datasets, X-04)
- **Proposta:** `datasets::packaging` com `PackageLayout {YoloTree, DiffusionTxt, ExportFull}`; `materialize + zip(spawn_blocking) + PUT + INSERT + delete_prefix-compensation` uma vez; `import_dataset` passa por `spawn_blocking` (fecha T-11 parte 1) e ganha tx fim-a-fim ou staging-swap (parte 2).
- **Aceite:** `build_package_filtered`+`build_package_diffusion`+export share um pipeline (jscpd: clones triplos resolvem); import de zip de teste 0 bloqueio do executor (assert com poll de latência); falha injetada no meio do import → dataset anterior intacto **ou** novo completo, nunca "vazio + 503".
- **Esforço:** M-G | **Risco:** médio (regras de roundtrip package→import mudam comportamento — ver DS-03 `snapshot_to_export_manifest` degrada campos) | **Dep:** nenhuma; coordenar com TASK-API-003 (mesmos arquivos `prepare` só tocado de leve).

#### [TASK-API-006] VRAM de uma fonte só (T-13)
- **Proposta:** embed do `vram-table.yaml` em build-time (`include_str!` + parse no boot com validação; o serde_yaml já é dep) **ou** remover `vram_min_gb` do wire BFF→manager (manager já o ignora — `lib.rs:491`) e manter a tabela só no manager. Escolher **embed + consumir no `vram_min_gb` de exibição**, preservando números do wire para o front.
- **Aceite:** `rg 'sd15|flux' services/api-principal/src -i` sem números de VRAM hardcoded; teste compara tabela×valores wire.
- **Esforço:** M | **Risco:** baixo | **Dep:** TASK-API-001 (mesmo PR de crate).

#### [TASK-API-007] Convenções de erro/config/estado (T-29, T-30, X-08)
- `impl From<ManagerError> for Response` + `impl From<StorageError>`, `internal()` com detalhe logado (campo `tracing`, nunca no wire); pool-fail → 503 com código existente (`queue_unavailable`/`storage_unavailable`) e 500 reservado a invariante; `err()` aceita `Cow<'static,str>`; consts nomeadas (`CACHE_IMMUTABLE_MAX_AGE`, `CAPTION_MAX_CHARS`, `ARTIFACT_KEY_FMT`); `is_too_large`/`internal` para `crate::error`; `keys::canonical_filename` no generations.
- **Aceite:** 0 `eprintln!` em `src/` (troca por `tracing::warn!`); `rg 'INTERNAL_SERVER_ERROR' src/jobs` −36 sítios; testes adaptados; wire de status idêntico (400/404/409/503 mantidos — **não** trocar 503→404 sem decisão registrada: `get_artifact_data` mapeia `NotFound→503` hoje e o front pode depender).

#### [TASK-API-008] Suíte de testes (T-23, T-24)
- (a) ativar os 2 `#[ignore]` de `submit_diffusion` via `MockManager` + teste SSE com stream finita; (b) extrair `tests/common/mod.rs` e fatiar `datasets_db.rs` por domínio; (c) `clippy.toml` com `unwrap_used`+`expect_used` (`cfg(test)` allow) + `await_holding_lock`, `rustfmt.toml`, `cargo clippy -D warnings` no CI.
- **Aceite:** `submit_diffusion_job` e `stream_job_events` com teste ativo; nenhum `#[cfg(test)]` >1500 linhas em arquivo único; CI clippy verde.

---

## 7. Segurança e Robustez (achados mapeados, não explorados)

**Fluxo de auth:** login→argon2→JWT HS256 (alg pinning, `exp` 7d, `iss`, leeway 30s — `session.rs:54-65`)→cookie `heph_session` (HttpOnly/Lax/Path=/; Secure condicional D-04). Gate: `route_layer` único + fallback 401/404 + inventário com teste de contrato (régua sólida). Sem comparação manual de segredo no BFF (nenhum `==` sobre tokens); o manager compara `Bearer` com `==` não-constant-time (`manager/main.rs:164-176`) — teórico, rede interna, registrado para a fatia manager.

| Área | Estado | Riscos mapeados |
|---|---|---|
| Autorização de rotas | ✅ gate cobre `/api/*`; inventário testado | `/metrics` fora de ambas listas (T-15); `/health` vaza janela de bootstrap (T-15/SEC-05); sem CSRF em cookies Lax + POST (SEC-06: top-level POST porta cookie — `SameSite=Lax` mitiga GET-cross-site, não POST form; avaliar Origin check no gate) |
| Limites de corpo | ✅ por rota: upload 208/8GiB/104/28MiB verificados (`routes.rs:217-231,359,428,521,535,556`) | sessões de upload sem cap (T-14); imagem sem pixel-limit (T-27) |
| Path traversal | ✅ `uploadId`→UUID, `part-{n:06}`; `sanitize_filename` central; zip-slip `classify_entry` + whitelist de raízes + tetos (réguas) | filename do DB em `join` no export (T-27); `validate_artifact_path` não barra `\\`/NUL internos (SEC-14) |
| SSRF | ✅ allow-list fail-closed (vazia=403), política por hop, recheck pós-redirect, MAX_REDIRECTS=5 | IPv4-mapped v6 escapa; `0.0.0.0/8`/CGNAT ausentes; DNS-fail fail-open; TOCTOU resolve→connect (T-25) |
| Injeção SQL | ✅ 100% bind; `format!` só em consts `{COLS}`; `QueryBuilder::push_bind` | nenhum sítio vulnerável encontrado |
| Injeção em args Docker | n/a neste serviço (orquestador) | — |
| Segredos | ✅ nunca em log (`load_storage` mensagens estáticas; `DATABASE_URL` não logada); sem `.env` lido pela auditoria | senha bootstrap em campo JSON (T-26); defaults `changeme` (T-01); `MANAGER_TOKEN` default (T-01) |
| Robustez async | ✅ zip/export/package em `spawn_blocking`; locks sem await sob guard; S3 com timeouts+retry-budget; upload atômico `.tmp`+rename | import síncrono no executor (T-11); argon2/normalize no executor (T-12); sem graceful shutdown (T-03); loops sem `catch_unwind` (T-28); advisory lock sem RAII (`indexer.rs:36-50`); `let _permit` ignora falha de acquire (`indexer.rs:107`); SSE sem teto (T-06); `RetryConfig::disabled` no S3 por decisão documentada (manter, avaliar jitter em GETs) |
| Estado em memória | ✅ por desenho (documentado) | sessões chunk single-instance; índice é durável no Postgres (multi-instance ok); **sem rate-limit nenhum no app** — só Caddy ingress |

Nota de correção da auditoria (validação por amostragem): o `unwrap()` de `batch_update_boxes` (`datasets/handlers.rs:1874,1889`) **não é alcançável** — `validate_batch_boxes_update` (`models.rs:513-536`) rejeita `remap` sem `target_class_id` antes; o panic-hook `__test_panic__` (`prepare.rs:689`) é alcançável só por escrita direta no DB e é contido por `catch_unwind` (P3, higiene). O "use-after-drop no export ZIP" reportado pela varredura **não ocorre no Linux** (o `File`/fd é aberto antes do drop do `TempDir` — `generations/handlers.rs:507`; unlink mantém dados no fd); ainda assim, vazar o `TempDir` até o fim do stream é a correção correta do padrão frágil (P3), e o N+1 serial do export (`:406-416`) é real (P2).

---

## 8. Roadmap em Fases (cada fase = 1 PR independente e verificável)

| Fase | PR | Conteúdo | Dep |
|---|---|---|---|
| **F0 — quick-wins de segurança** | PR-0a | T-01 (fail-closed compose+boot, rotação com runbook), T-08 (rate-limit login + `spawn_blocking` argon2), T-26 (senha fora do log JSON), T-15 (`/metrics` no inventário ou atrás do gate; tirar `auth` do `/health`) | nenhuma |
| **F1 — contrato interno** | PR-1 | TASK-API-001+002: crate consumida, `PackageRef` unificado, migrations com CI | 002 pode ser PR-1b |
| **F2 — domínio de estado** | PR-2 | TASK-API-003 `JobStatus` + T-06 SSE (consts, erro com teto) + T-13/TASK-API-006 VRAM | PR-1 |
| **F3 — repositórios + aplicação** | PR-3a/b/c | `repository.rs` jobs→datasets→search/generations; depois TASK-API-004 guards; T-21 batch tx/N+1; T-22 dono único do GC de `dataset_versions` (com manager) | PR-2 |
| **F4 — quebrar monólitos** | PR-4a/b | datasets (T-10, TASK-API-005, T-11 atomicidade, `spawn_blocking`); jobs (T-19, T-29); models (`upload/download/catalog/finalize`, T-32 renomear `monitoring.rs`→`proxies` + mover `get_models`) | PR-3 |
| **F5 — robustez runtime** | PR-5 | T-03 graceful shutdown + `JoinSet`/`CancellationToken` (T-28), T-12, AU-09 embedder retry, T-25 SSRF ranges+pin, T-27 pixel-limit/sanitização export, `clippy.toml`/`rustfmt.toml`+CI (T-31) | PR-4 |
| **F6 — sessão/JWT** | PR-6 | T-09 (TTL curto + refresh ou denylist `jti`) + CSRF/Origin (SEC-06) — exige web junto | F0 |
| **F7 — testes & limpeza** | PR-7 | TASK-API-008, T-24, T-30, spikes `examples/`→`docs/`, `deny.toml`+machete/audit em CI | contínuo |

**Ordem de dependência real:** contrato → estado → repositório → monólitos → robustez → sessão → limpeza. F0 e F1 são independentes entre si e podem correr em paralelo.

### O que NÃO mudar
- Os 5 invariantes do skill (verificados §5) — nada de trocar SSE por push "para modernizar" sem resolver a amplificação primeiro; manter polling com teto.
- `AppState`→trait de repositório: custo alto, ganho marginal sem a camada F3; decisão adiada conscientemente (T-06 teste).
- Subir axum 0.8 / sqlx 0.9 / dividir `migrations/` por serviço: churn quebra `pgvector=0.4.1` pin e o CI do manager; sem demanda.
- `tokio full`, hash triplo, `serde_yaml`: só limpeza F7, não risco ativo.
- `RetryConfig::disabled` do S3 e tetos 8GiB: decisões com incidente documentado (`s3.rs:52-62`).
- `deny_unknown_fields` + camelCase + 404-para-não-UUID: contratos comportamentais travados por testes — qualquer relaxamento vira quebra de contrato com a web.
- Padrão de compensação objeto→linha→delete (D7) e guarda `studio_test` dos testes: são as melhores ideias do serviço.

---

## 9. Proposta de texto para `AGENTS.md` — "Convenções de Backend" (não aplicar; só proposta)

```markdown
## Convenções de Backend (Rust)
1. Camadas por feature: `routes/handlers` (transporte: ≤60 linhas, zero SQL) →
   `usecases` (orquestração ≤120 linhas/função) → `repository` (único dono de
   `sqlx` na feature) → `model` (tipos/validação puros, camelCase wire).
   Arquivo >600 linhas = fatiar antes de acrescentar.
2. Protocolo interno só por `crates/heph-contracts`: proibido redefinir struct
   local para payload que já existe no crate. Mudar o crate exige bump + PR nos
   3 serviços na mesma janela (wire deve permanecer idêntico).
3. `migrations/` do api-principal é o schema compartilhado e é APPEND-ONLY:
   migration aplicada nunca se edita; DROP/RENAME exige PR com mudança no
   manager no mesmo PR (CI roda o suite do manager sobre a migration).
4. Estados de job: usar `JobStatus` (heph-contracts). Proibido literal de
   status em string fora de serde/enum; terminalidade só via `is_terminal()`.
5. Erros: `crate::error` → envelope `{code,message}` com `code` do catálogo do
   openapi; detalhe dinâmico vai para `tracing`, nunca para o wire. Sem
   `unwrap()/expect()` fora de `#[cfg(test)]` (clippy `unwrap_used` = deny).
6. Nunca bloquear o executor: CPU-bound (hash de senha, decode/encode de
   imagem, zip) e I/O síncrono (zip de import) via `spawn_blocking` ou
   `tokio::fs`. `Mutex`/`RwLock` std jamais segurado através de `.await`.
7. Chamada externa (manager, embedder, S3, download) nasce com timeout no
   builder e retry só onde há política declarada. Nenhum `tokio::spawn` sem
   `JoinSet`/`CancellationToken` ou `catch_unwind`+recovery explícito.
8. Config: todo env lido na `boot/config` tipada; proibido `std::env::var` em
   handler/usecase. Literais de política (96 MiB, TTLs, tetos) viram `const`
   nomeada ou campo de config — nunca número solto.
9. Segurança de ingestão: arquivo externo passa por `keys::sanitize_filename`
   + sniff; zip por `classify_entry`; download por allow-list + recheck pós-hop
   (padrões: `storage/keys.rs`, `datasets/import.rs`, `models/validate.rs`).
10. Todo endpoint novo entra no inventário de `auth/routes.rs`
    (`PUBLIC_ROUTES`/`PROTECTED_ROUTES`) no mesmo PR — o teste de contrato
    (`tests/contract.rs`) é a trava; `/metrics` nunca fora dele.
```

🌱 graft saved ~42 tokens this turn (file_api do `jobs/handlers.rs` economizados em leitura integral; subagentes reportaram ~1,24M próprios).
