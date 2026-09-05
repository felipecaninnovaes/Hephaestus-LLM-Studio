# Coordenação — estado do plano (memória do coordenador)

Arquivo de trabalho do agente coordenador: registra **onde estamos** e **qual o
próximo passo na ordem**, para sobreviver a restart de sessão. Não duplica
docs — referencia por seção. Atualizar: ao abrir fatia, ao fechar fatia, e ao
ser interrompido no meio de uma.

## Protocolo de retomada (início de sessão)

1. Ler este arquivo → seção "Plano em andamento".
2. `git status` + `git log --oneline -5` para conferir se o disco bate com o
   registrado (branch aberta, commits pendentes de push).
3. `graft check` se for mexer em código indexado (refresh: `graft build`).
4. Fontes de verdade para a fatia: `IDEIA.md`, `docs/backend.md` §9/§10,
   `docs/frontend.md` §10, `docs/repo-estrutura.md` (ordem de fatias).

## Estado atual — 2026-09-04

- Branch de trabalho: **`feat/datasets-core`** (Slice 3a) — 7 commits, pronta para
  revisão/merge do usuário. **Não foi feita push nem merge** (pedir explicitamente).
- `main` local: **5 commits à frente de `origin/main`** — push pendente de pedido.
- Roadmap `docs/repo-estrutura.md` §Ordem: Slice 1 ✅, Slice 2 ✅, **Slice 3a ✅
  (aguarda merge)**, 3b/3c/3d no backlog.
- **Fatia 3a fechou**: `GET/POST /api/datasets` + `GET/DELETE /api/datasets/:id` com
  migration `0002` (`datasets`+`classes`), primeira rota de negócio → gate
  `route_layer(require_auth)` plugado (dívida do ADR-0001 D9 quitada), OpenAPI
  0.2.0, ADR-0002 escrita. Verificação: `cargo check --workspace` limpo,
  `cargo test -p api-principal` = 27 units + 7 contract verdes sem banco,
  `bash scripts/test-db.sh` = 7 integration verdes com Postgres do compose,
  `compose -f compose.yaml -f compose.integ.yaml config -q` OK.
- **Decisão estrutural nova (ADR-0002 D1)**: casing no wire é **camelCase em
  `/api/*` inteiro**; colunas SQL, valores de enum, `Error.code` e artefatos de
  transporte (`manifest.json`, `config.yaml`, SQLite) ficam **snake_case**.
  Enforcement por teste (`json_property_names_are_camel_case`, walk recursivo).
  Isso altera o que os docs de settings exemplificavam → `hf_token` virou
  `hfToken` no wire em `backend.md` §9 e `frontend.md` §10 (rota ainda não
  existe). Não regrida isso por acaso.
- `apps/web` continua com só `/` e `/login`; `/datasets` (3c) ainda não existe.
- Ferramental: `@ui-designer` despacha (exige dev server + Chrome :9222).
  Grafo graft em dia (`graft/` é git-ignored — não se commite); 2 nós de
  `layout.tsx` seguem pendentes no meaning tier (modelo local falha lá, cosmético).

## Dívidas registradas que as próximas fatias precisam honrar

- **3b (upload/imagens)** — pré-condições de aceite: volume `datasets` +
  `DATASETS_DIR` no serviço `principal` (hoje só o orquestrador monta) e campo
  `storage_dir` no `AppState`; `POST /:id/upload` com `DefaultBodyLimit` **dedicado**
  de 200 MB repetindo o envelope de erro (não herdar os 2 MiB do axum); `source`
  preenchido e **cleanup de `<DATASETS_DIR>/<slug>` no DELETE, delete-after-commit**
  (ADR-0002 T3); migration `0003` com `images/boxes/captions` + **triggers de
  contadores recalculando os dois numa função só ou `CONSTRAINT TRIGGER DEFERRABLE`**
  — o invariante `labeled_count <= images_count` saiu do `CHECK` justamente porque
  CHECK não é deferrável (T2); status `needs_labeling → in_progress → ready` passa a
  ser derivado; bloco `--datasets` no `scripts/e2e-smoke.sh`.
- **Jobs (fatia 4)** — `jobs.dataset_id UUID NULL REFERENCES datasets(id)
  ON DELETE SET NULL` + snapshot `dataset_versions` (nunca `RESTRICT`) — ADR-0002 T4.
- **3d** — derivar `autoTracked` de `boxes.origin='autotracker'` (T7); hoje é
  constante `false`.
- **Hardening (sem fatia marcada)** — gate aceita `sub` órfão: cookie assinado com
  segredo antigo sobrevive a reset de `users` e passa a ler/deletar datasets (T8;
  mitigação = `SELECT EXISTS` no gate ou rotacionar segredo no reset). Erro de
  banco hoje vira 500 **sem log nenhum** — antes da fatia de jobs adicionar log
  server-side (nunca no response). `cargo fmt -p api-principal` tem 10 hunks
  violando padrão, **todos pré-existentes** (Fatia 2); nenhum CI de fmt — decisão
  de quando formatar é do usuário.

## Plano em andamento — próximo passo: Slice 3b (upload/imagens/storage)

1. **3b — storage + imagens**: contrato de `POST /:id/upload` (multipart 200 MB) e
   `GET /:id/images?limit&offset` → `@architect` (envolve boundary de disco + novo
   env `DATASETS_DIR`, logo é decisão de arquitetura) → migration `0003` →
   endpoints + triggers → testes (contract + `--ignored`) → `@reviewer`.
2. **3c — UI `/datasets` (lista)**: `@frontend-dev` vs protótipo (mock dos 5
   datasets de `frontend.md` §5.1 até a API fechar) → `@ui-designer` audita
   screenshot-vs-screenshot → `@reviewer`.
3. **3d — `/datasets/[id]` galeria + annotate**: maior; abrir sub-fatias ao chegar.

Cada fatia: branch `feat/<slice>` de `main` atualizada, commit `type(scope):
subject`, verificação do coordenador (`cargo check --workspace`, `cargo test -p
api-principal`, `bash scripts/test-db.sh` quando houver teste de banco, `npm run
build --workspace=web` quando houver UI, `compose config -q`), sem push/merge sem
pedido.

## Fecho

- [ ] Merge de `feat/datasets-core` em `main` — aguardando revisão do usuário.
- [ ] Push dos 5 commits de `main` + dos commits da branch — aguardando pedido.
- [ ] Abrir 3b (`@architect` primeiro: `DATASETS_DIR` cruza para o serviço
      `principal` no compose).
