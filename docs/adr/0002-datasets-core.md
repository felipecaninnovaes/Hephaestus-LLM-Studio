# ADR-0002 — Datasets núcleo (Fatia 3a)

- **Status:** Aceito
- **Data:** 2026-09-05
- **Componentes:** `services/api-principal` (`src/datasets/`, `src/state.rs`, `src/error.rs`, `src/auth/routes.rs`), Postgres (`db`), `packages/contracts`
- **Fontes:** `docs/backend.md` §2 (gate), §9 (1ª linha do grupo datasets), §10 (schema), §11 (transporte);
  `docs/frontend.md` §4.3 (modal), §5.1 (lista), §8 (shape), §10 (contratos); `docs/repo-estrutura.md` (fatia < 400 linhas, snapshot OpenAPI)
- **Contrato:** `packages/contracts/openapi.yaml` (v0.1.0 → 0.2.0 nesta fatia: `not_found`, `slug_conflict`, grupo `datasets`)

## Contexto

Primeira rota de negócio do `api-principal`: `GET/POST /api/datasets` + `GET/DELETE /api/datasets/:id`
(1ª linha do grupo datasets do §9). Junto dela entra o gate `route_layer` adiado pelo
ADR-0001 D9 e precisa ser resolvida a tensão T3 da 0001 (política global de casing), que a
0001 exigiu "antes da Fatia 3". Escopo fechado: metadados + classes, sem filesystem, sem
paginação. Upload/imagens/boxes/captions/videos/package/UI ficam para 3b+.

- Firmado (código): migration `migrations/0002_datasets.sql` (`b5d3c33`); modelos puros
  `src/datasets/models.rs` (derivação, slug, classes, `DatasetResponse`); handlers
  `src/datasets/handlers.rs` + gate plugado em `src/auth/routes.rs` + OpenAPI 0.2.0 (`1e5cbe6`).
- 27 units + 6 contract verdes sem banco; boot com Postgres validado manualmente (migração aplica, sem panic de `route_layer`).

## Decisões

### D1 — Casing global: camelCase em todas as chaves de body/query/response de `/api/*`
Colunas SQL permanecem snake_case (`size_bytes`, `images_count`), valores de enum permanecem
snake_case (`yolo_bbox`, `needs_labeling`), `Error.code` permanece snake_case (`slug_conflict`),
e artefatos de transporte permanecem snake_case (`manifest.json`, `config.yaml`, SQLite do
orquestrador). Razões: custo zero (auth já shipa camelCase com `userId`/`loggedAt` e tem teste
guardando), a UI (`frontend.md` §8) pede camelCase, evita superfície two-style para o codegen
futuro de `packages/contracts`.
**Descartado:** snake_case fora de auth — churn em auth shipped + two-style permanente.
**Enforcement não é prosa:** `tests/contract.rs::json_property_names_are_camel_case` rejeita
qualquer propriedade de schema fora de `^[a-z][A-Za-z0-9]*$`. O ponto de conversão é único:
`#[serde(rename_all = "camelCase")]` em `DatasetResponse` sobre campos snake_case
(`models.rs`), nunca `AS "camelCase"` em SQL. Quita a pendência do ADR-0001 T3.

### D2 — Fronteira da migration: `0002_datasets.sql` cria só `datasets` + `classes`
`images`/`boxes`/`captions`/`videos`/`dataset_versions` entram em `0003` com a 3b, porque
(i) nenhuma rota da 3a os lê/escreve (schema morto = código não verificável),
(ii) a regra do §10 "contadores via trigger a partir de images/boxes/captions" é inescrevível
sem `images`, (iii) a FK com `ON DELETE CASCADE` nasce correta junto da tabela filha.
**Descartado:** criar as 7 tabelas de uma vez — inflaria a migration com DDL não exercitado
por nenhum handler nem teste com banco.

### D3 — Derivação server-side: `type` é o único input taxonômico
`type` é enum fechado de 4 códigos de máquina (`yolo_bbox`, `yolo_seg`, `difusao_lora`,
`clip_image_text`) — as 4 options do modal `create-dataset-modal` (`frontend.md` §4.3).
`category`/`task`/`format`/`status` são função de `type` (`datasets::models::derive`, tabela
única: `YoloBbox → (yolo, detect_track, yolo_txt)`, `YoloSeg → (yolo, segment, yolo_txt)`,
`DifusaoLora → (difusao, caption, captions)`, `ClipImageText → (openclip, embedding, pairs)`)
e `status = 'needs_labeling'` fixo no create. Rótulos pt-BR são exclusividade da UI.
Input do cliente só aceita `title`, `type`, `classes` (`CreateDatasetRequest` com
`deny_unknown_fields`). **Descartado:** aceitar `category`/`task`/`format` do cliente —
triplicaria a superfície de inconsistência (`type=yolo_bbox` + `category=difusao`) sem nenhum
caso de uso na 3a.

### D4 — `datasets.updated_at` + trigger `tg_set_updated_at`
Coluna **ausente no §10**, adicionada porque `frontend.md` §5.1 pede `lastModified` e mapear
`created_at` mentiria a partir da 1ª edição em 3b/3c. Função plpgsql `tg_set_updated_at()`
(`NEW.updated_at := now()`), trigger `BEFORE UPDATE` por tabela, reutilizável pelas tabelas
da 3b. A lista ordena por `updated_at DESC` (a UI filtra/busca e calcula as pills no cliente).
No wire sai como `lastModified` (`last_modified` + `rename_all = "camelCase"`).
**Descartado:** omitir a coluna e servir `created_at` como `lastModified` — dívida silenciosa
que apodreceria na primeira edição de galeria.

### D5 — `slug` gerado no servidor, colisão → 409 `slug_conflict` sem auto-sufixo
Normalização determinística em `models::slugify`: minúsculas, mapa Latin-1 explícito por
`match` sem crate nova (`á→a`, `ç→c`, `æ→ae`, `ß→ss`, …), runs de não-`[a-z0-9]` → `-`,
sem `-` nas pontas, 96 máx com trim de `-` final; resultado vazio ⇒ 400. Inserção via
`INSERT ... ON CONFLICT (slug) DO NOTHING RETURNING` — sem linha retornada ⇒ 409.
Auto-sufixo (`pcb-defeitos-2`) foi **descartado**: o nome é exibido na UI e será nome de
diretório em disco na 3b, então colisão é mostrada ao usuário e não disfarçada.

### D6 — Nunca-aceitos-do-cliente
`id`, `slug`, `category`, `task`, `format`, `status`, `source`, `sizeBytes`, `imagesCount`,
`labeledCount`, `autoTracked`, `createdAt`, `lastModified`, `classes[].idx/color`.
`deny_unknown_fields` no struct + `additionalProperties: false` na spec, provado por teste
(`POST {"title":"x","type":"yolo_bbox","status":"ready"}` → 400). Título vazio, título que
não produz slug, `type` fora do enum, classe fora de `^[A-Za-z0-9_]{1,64}$` ou > 200 classes
⇒ 400 `invalid_request`. **Descartado:** ignorar chaves desconhecidas (tolerância que
esconderia cliente enviando `status` achando que controla o ciclo de vida).

### D7 — `autoTracked: false` constante na 3a
Mantém o shape de `frontend.md` §8 estável para a 3c tipar; fonte real = existe
`boxes.origin = 'autotracker'` no dataset, derivação na 3d. **Descartado omitir o campo:**
a tag do card sumiria sem decisão registrada e a 3c teria de adivinhar o nome/tipo.
Prazo na T7: se a 3d não entregar, o campo vira mentira útil.

### D8 — `id` não-UUID → 404 `not_found`, nunca 400
`models::parse_id` (`raw.parse().ok()`) antes de qualquer query; `None` ⇒ 404 com envelope.
Não distingue "id inválido" de "inexistente" e mantém o inventário de status fechado
(200/201/204 + 400/401/404/409 + 500/405 globais). **Descartado:** 400/422 para id malformado —
vazaria distinção sem valor operacional e abriria um status a mais no contrato.

### D9 — Gate da 1ª rota de negócio (quita a dívida do ADR-0001 D9)
`route_layer(middleware::from_fn_with_state(state, gate::require_auth))` aplicado **depois**
dos `.route()` no sub-router `protected` — com router vazio o axum 0.7.9 dá panic no boot
(`path_router.rs`, `routes.is_empty()`). `.fallback(gate_fallback)` da raiz permanece só para
caminho **não roteado**; daí duas camadas de 404 distintas: roteado+inexistente = envelope
`not_found` com corpo (handler); não roteado = 404 **sem corpo** (fora da OpenAPI, sonda
autenticada em `/api/nada` prova). `.layer()` foi **descartado**: envolveria a raiz inteira
e engoliria `/health` e `/api/auth/*`. `PROTECTED_ROUTES` continua o cinto estrutural
(esquecer a rota aqui = contrato vermelho) e `security_class_matches_openapi` amarra a classe
da rota à spec. Quita o "adiado p/ Fatia 3" do ADR-0001 D9.

### D10 — SQL runtime, sem macros
`sqlx::query`/`query_as` com string; `query!`/`query_as!` **proibidos** nesta fatia (exigiriam
`DATABASE_URL`/offline no build, e o `cargo check --workspace` do CI não tem banco). Lista usa
**duas queries** (`SELECT ... FROM datasets ORDER BY updated_at DESC` + `SELECT dataset_id,
name FROM classes ORDER BY dataset_id, idx`) com agrupamento em `HashMap` (sem N+1) em vez de
`json_agg` — escolha de não tocar `Cargo.toml` na 3a. Bulk de classes num único
`INSERT ... SELECT ... FROM unnest($2::text[], $3::int[], $4::text[])`.
**Descartado:** `json_agg` na query da lista — economizaria Rust ao custo de SQL menos
portátil e acoplamento de agregação JSON no driver.

### D11 — Erros: novos `Error.code` `not_found` e `slug_conflict`
Enum cresce **por aditivo** (`info.version` 0.1.0 → 0.2.0); a UI DEVE ter ramo default
(`frontend.md` §10 já especifica). Sem `details`/`field` no envelope: o modal valida no
cliente e `message` continua estático por code (`src/error.rs`: `"dataset not found"`,
`"dataset slug already exists"`, `"invalid request"`), sem valor de usuário, path ou token.
**Descartado:** payload de validação por campo — sem consumidor na 3a e expandiria o envelope
fechado da 0001 D6.

### D12 — `Error`/`AppState` saíram de `auth/`
`crate::error::err` é o construtor único do envelope D6 para todo domínio;
`crate::state::AppState` é o estado do boot D3 (`pool`, `jwt_secret`, `secure_cookie`,
`setup_required`). `auth::AppState` continua existindo como **re-export** para não quebrar
`tests/contract.rs`/`main.rs` (paths de teste travados); `auth::handlers` mantém suas próprias
`MSG_*` (já no contrato da 0001). **Descartado:** mover os handlers auth junto — churn em
código shipped sem ganho e estouraria o teto de 400 linhas da fatia.

## Consequências

- **Novas dependências (Cargo.toml): nenhuma.** A 3a não toca `Cargo.toml` (D10); sqlx/uuid/chrono/serde já vinham da Fatia 2.
- **Env vars:** nenhuma nova. `storage_dir`/`DATASETS_DIR` chegam na 3b com o upload.
- **Migration `0002`:** só `datasets` + `classes` + `tg_set_updated_at` (D2/D4); `sqlx::migrate!`
  continua sem `DATABASE_URL` no build. `UNIQUE(slug)` já indexa o lookup de colisão; índice
  explícito só em `classes(dataset_id, idx)` para o JOIN da lista.
- **Contrato 0.2.0:** grupo `datasets` (4 operações), schemas `CreateDatasetRequest`/`Dataset`,
  enum `Error.code` +2. Regra de casing vira texto normativo na `description` raiz da spec.
- **Front:** nenhum código TS neste slice. `Dataset` da spec já é o shape que a tela de datasets
  vai consumir (ver anotações em `frontend.md` §8); a dropzone do modal continua sem rota até a 3b.

## Tensões nos docs (expostas, não resolvidas em silêncio)

- **T1 — CHECKs de domínio e `UNIQUE(dataset_id, idx)` não constam do §10.** Aperto de
  semântica, não tabela nova (`CHECK` em `category/type/task/format/status`, `CHECK` de
  `size_bytes/images_count/labeled_count`, regex de `classes.name`/`color`). §10 foi editado
  nesta fatia marcando "ADR-0002".
- **T2 — contadores sem `images`.** `images_count = 0 ∧ labeled_count = 0 ∧ size_bytes = 0` é
  **teorema** da 3a (nenhum caminho de escrita), não estimativa. A revisão derrubou a ideia de
  pré-comprometer `% Rotuladas` (`labeled_count <= images_count`) com `CHECK`: `CHECK` não é
  deferrável no Postgres e o `DELETE FROM images` de uma imagem rotulada passa por estado
  intermediário (trigger de `images` vs. cascata de `boxes`, ordem de RI vs. usuário) que o
  violaria. O CHECK foi **removido** do `0002` (ficou só `>= 0`); o invariante passa a ser
  obrigação declarada do trigger da 3b (função única de recálculo ou `CONSTRAINT TRIGGER
  DEFERRABLE`). Registrado em `backend.md` §10 "Regra".
- **T3 — `DELETE` da 3a não apaga disco.** Em 3a não há artefato; **pré-condição de aceite da
  3b** é `datasets::handlers::delete` ganhar cleanup de `<DATASETS_DIR>/<slug>`
  (delete-after-commit, nunca apagar disco antes do commit) — senão o DELETE passa a vazar GBs.
- **T4 — FK de `jobs.dataset_id` vai travar o `DELETE`.** `jobs` não é filho do dataset, é
  consumidor. Quando a fatia de jobs chegar: `jobs.dataset_id UUID NULL REFERENCES
  datasets(id) ON DELETE SET NULL` + snapshot `dataset_versions` (§10), **não** `RESTRICT`
  (deixaria o usuário sem limpar datasets). Decidir agora evita `ALTER TABLE` com dados.
- **T5 — `Error.code` cresceu.** Breaking para cliente que trate enum como fechado; mitigado
  por ramo default obrigatório + bump de versão + texto na própria spec.
- **T6 — `type`/`task`/`format` redundantes** (as três são função de `type`). Dívida assumida do
  §10, que manda as colunas existirem e a UI (§5.1 "Formato-Tarefa") lê as três; derivação numa
  `derive()` única deixa o custo próximo de zero.
- **T7 — `autoTracked` constante tem prazo.** Se a 3d não entregar a derivação
  (`boxes.origin = 'autotracker'`), o campo vira mentira útil; deadline fixado na descrição
  da propriedade na spec.
- **T8 — gate aceita `sub` órfão (pré-existente, alcance ampliado).** `require_auth` valida
  assinatura, não existência do usuário; se `users` for resetado sem trocar o segredo, um cookie
  antigo passa a ler dados de dataset (antes só lia `/me`). Mitigação barata (1 query
  `SELECT EXISTS` no gate) fica para a fatia de hardening.
- **T9 — normalização `{id}`↔`:id` é ponto cego do teste.** A spec declara **somente** `{...}`
  e o teste assere isso (`inventory_matches_openapi` rejeita `:` na spec); a normalização
  acontece só no lado da comparação.
- **T10 — `DefaultBodyLimit` do axum (2 MiB) e o 413.** A premissa original ("o cap de 200
  classes evita o 413 sem envelope") foi **derrubada na revisão**: o extractor de body estoura o
  limite antes de qualquer validação, então junk > 2 MiB daria 413 `text/plain` do axum, fora do
  envelope D6 e fora dos status declarados. Resolução aplicada: `create` recebe
  `Result<Bytes, BytesRejection>` e mapeia `LengthLimitError` → 413 `invalid_request` **no
  envelope**; o `413` entra na convenção global da spec (ao lado de 500/405), não por operação.
  O cap de 200 classes continua valendo como `maxItems` da spec sobre o **input** (DC1). O
  `POST /:id/upload` da 3b **não** pode herdar este limite: precisa de `DefaultBodyLimit`
  dedicado de 200 MB repetindo o mesmo padrão de envelope.

## Plano de fatia (para despachar; cada passo é um commit < 400 linhas)

| # | Commit | O que | Critério de aceite |
|---|---|---|---|
| 3a.2 | `b5d3c33` `feat(datasets): migration 0002 + estado e modelos puros` | `migrations/0002_datasets.sql` (D2/D4) + `src/state.rs` + `src/error.rs` + `src/datasets/models.rs` (D1/D3/D5/D8) | `cargo test -p api-principal` (units de slug/derive/classes); `sqlx migrate run` aplica |
| 3a.3 | `1e5cbe6` `feat(datasets): rotas CRUD núcleo + gate require_auth plugado` | `src/datasets/handlers.rs` (D6/D8/D10) + `src/auth/routes.rs` (D9) + `openapi.yaml` 0.2.0 (D11) + `tests/contract.rs` (D1/D6/D9) | `cargo test -p api-principal` verde; remover rota do código quebra o contrato |
| 3a.4 | `38f8591` `test(datasets): integração postgres gateada por --ignored + runner` | `tests/datasets_db.rs` (7 testes pelas rotas HTTP sobre Postgres) + `scripts/test-db.sh` (sobe `db`, poll `pg_isready`, derrube só o que criou) | `bash scripts/test-db.sh` → 7 passed |
| 3a.5 | `83c266e` `fix(datasets): alinhamento a contrato nos pontos da revisao` | DC1 cap de input, DC2 413 no envelope, DC3 walker camelCase recursivo, DF1 remove o CHECK, DF4b keys serializadas ≡ spec, N1/N3/N2 | `cargo test -p api-principal` (7 contract) verde; migration reaplicada em banco limpo |
| 3a.6 | *(este commit)* `docs(adr): ADR-0002 datasets núcleo + política de casing` | este ADR + anotações em `backend.md`/`frontend.md` + T3/D9 da 0001 riscados | docs espelham código/OpenAPI; nenhum `.rs` tocado |

**Definition of Done da Fatia 3a:** `cargo test -p api-principal` (27 units + 7 contract) e boot
com Postgres (migração + ausência de panic de `route_layer`) verdes; nenhuma tabela além de
`datasets`/`classes` tocada; sem mudança em manager/orchestrator/engines. **Nota de tamanho de
commit:** o teto de ~400 linhas mede **código de produção**, não o total do commit. `1e5cbe6` tem
657 insertions porque ~269 são o YAML de contrato (`packages/contracts/openapi.yaml`) e 139 são
testes mecânicos de contrato — partir rotas+gate de contrato+teste deixaria
`inventory_matches_openapi` vermelho no commit intermediário e quebraria o bisect (o teste é
justamente o cinto que amarra os dois). Conta como exceção deliberada, registrada aqui.

## Riscos e o que testar

- **Coberto sem banco (7 tests em `tests/contract.rs` + 27 units):** inventário spec≡router;
  classe pública/protegida da spec ≡ `PUBLIC_/PROTECTED_ROUTES`; fail-closed das 4 rotas sem
  cookie (401); probes autenticados sem banco (`GET :id` não-UUID → 404 com corpo; `POST {}`
  → 400; `POST` com `"status":"ready"` → 400; `type` fora do enum → 400); invariante camelCase
  por walk recursivo da YAML (propriedades aninhadas + `parameters[].name`); chaves
  serializadas de `DatasetResponse` ≡ `Dataset.properties` da spec;
  `GET /api/nada` sem cookie → 401 vs com cookie → 404 sem corpo;
  units de `slugify`/`derive`/`normalize_classes` (cap de input, não de output)/`parse_id`/`color_for`.
- **Coberto com banco (`38f8591`, `--ignored`, runner `scripts/test-db.sh`):** fluxo
  create→list→get→delete; 409 de slug duplicado sem linha pela metade; `classes` ordenadas por
  `idx` com cores da paleta (`idx % 6`); cascade `datasets→classes` no DELETE; trigger
  `updated_at` (update move `lastModified`, `created_at` fica); campo extra / `type` inválido /
  201 classes → 400 sem escrever nada; 401 nas 4 rotas com pool real. Rodam **serializados**
  por um mutex (`SERIAL`) porque compartilham o banco com setup destrutivo — se a 3b abrir mais
  tabelas no setup, considerar schema-temp-por-teste em vez de enlarguecer o lock.
- **Teste manual já executado:** boot com Postgres aplica `0002`, `route_layer` não panica com
  sub-router não-vazio, `/health` segue público.
