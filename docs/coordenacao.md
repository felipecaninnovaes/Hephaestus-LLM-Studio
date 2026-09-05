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

## Decisão em aberto — storage da 3b (bloqueia abrir a fatia)

O usuário propôs usar **MinIO** (como ele já opera no VisionLens,
`/home/felipecn/DEV/VisionLens`) para imagens canônicas e manter labels/coordenadas no
banco. Direção **aprovada em princípio por mim** e desenhada pelo `@architect` em
**`docs/adr/0003-object-storage-s3.md` (status PROPOSTO)**: bucket S3 = blob canônico,
Postgres = verdade relacional, upload **via principal com spool+PUT de length exato**,
leitura com presigned quando alcançável + fallback `/data`, `StoragePort` + mock.

**MinIO está descartado por evidência** (não por gosto): o repo `minio/minio` foi
arquivado pelo dono em **25/04/2026** ("NO LONGER MAINTAINED", distribuição só de fonte),
e a GHSA-9c4q-hq6p-c237 (bypass de assinatura na trilha `STREAMING-*-TRAILER`) não tem
patch no OSS. Também não dá para streamar corpo de tamanho desconhecido — é a trilha das
issues abertas #21611/#21303 (esta última é o SDK Rust com `ByteStream::from_path`).

**Pendente do usuário: escolher o servidor S3 da D0.** Recomendação do coordenador:
**SeaweedFS** (Apache-2.0, desde 2012, escolhido pelo Kubeflow no lugar do MinIO, bucket
pré-criado no boot → mata o init-container), **Garage** como plano B (binário único +
TOML, mas AGPL e sem console rico), **RustFS descartado como default** (pré-1.0).
Qualquer um dos três é config atrás da `StoragePort`, não mudança de código.

Depois da escolha: **spike `3b.0`** (7 critérios binários na ADR; o critério 4 — presigned
GET funcionando no browser — é o que decide D0 na prática) e então 3b.1..3b.8.
Nenhum doc de `backend.md`/`frontend.md` foi alterado ainda: a lista de linhas que ficam
falsas está no fim da ADR-0003, pronta para o `@docs-sync` quando a decisão for aceita.

## Dívidas registradas que as próximas fatias precisam honrar

- **3b (upload/imagens)** — **revisada pela ADR-0003 (proposta)**: morrem o volume
  `datasets`/`DATASETS_DIR` no principal e o "cleanup de `<DATASETS_DIR>/<slug>`" (viram
  sweep de prefixo `datasets/<id>/` no bucket, pós-commit); **permanece** o
  `DefaultBodyLimit` **dedicado** de 200 MB em `POST /:id/upload` com envelope de erro
  (não herdar os 2 MiB do axum — ADR-0002 T10, quitado na letra); `images.path` vira
  `object_key`; migration `0003` com `images/boxes/captions/videos` + **contadores
  recalculados por função única** (nunca `+=`) — o invariante
  `labeled_count <= images_count` saiu do `CHECK` porque CHECK não é deferrável e o
  `DELETE FROM images` de imagem rotulada passa por estado intermediário (T2); status
  `needs_labeling → in_progress → ready` passa a ser derivado por trigger; export/import/
  package sobem para **3e** (backup interim = console + `mc mirror`); bloco `--datasets`
  no `scripts/e2e-smoke.sh`.
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

## Plano em andamento — próximo passo: fechar a D0 e rodar o spike `3b.0`

1. **3b — storage + imagens** (ADR-0003 PROPOSTA, `@architect` já rodou): escolha do
   servidor S3 (D0) → spike `3b.0` com 7 critérios binários → `feat/datasets-storage` com
   3b.1..3b.8 (migration 0003 → porta+mock → upload → S3+compose → leitura → boxes/caption
   → sweep do DELETE → docs-sync). `@reviewer` ao fim de 3b.3 e 3b.6.
2. **3c — UI `/datasets` (lista)**: `@frontend-dev` vs protótipo (mock dos 5
   datasets de `frontend.md` §5.1 até a API fechar) → `@ui-designer` audita
   screenshot-vs-screenshot → `@reviewer`.
3. **3d — `/datasets/[id]` galeria + annotate**: maior; abrir sub-fatias ao chegar.
4. **3e — export/import/package** (subiu de 3b pela ADR-0003 D9; é onde o requisito
   "usuário obtém os arquivos para backup" vira produto, não `mc mirror`).

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
