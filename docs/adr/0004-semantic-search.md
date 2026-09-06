# ADR-0004 — Busca semântica sobre datasets com embeddings OpenCLIP (Fatia 3f)

- **Status:** Aceito (2026-09-05, usuário)
- **Data:** 2026-09-05
- **Componentes:** `services/api-principal` (novo `src/search/`), `engines/trainer-clip` (modo `serve`), `infra/compose.yaml` (serviço `embedder` + troca da imagem do `db`), Postgres (migration `0004`), `packages/contracts` (0.3.0 → 0.4.0), `apps/web` (painel na galeria — depende da 3d).
- **Fontes:** `IDEIA.md` (categoria OpenCLIP = descrição/embedding de imagem; ferramentas de preparo); `docs/backend.md` §1 (topologia, principal sem GPU), §5 (runners CLIP — futuro), §9 (grupo datasets), §10 (schema), §11 (compose); `docs/frontend.md` §5.2 (galeria), §10 (contratos); `docs/adr/0002` (D1 casing, D8 404, D10 sem macros); `docs/adr/0003` (D1 bucket canônico, D8 só o principal fala S3, formato de ADR); `docs/dividas.md` (fixar digests, T4 jobs); `docs/coordenacao.md` (roadmap: 3d → 3f → 3e → 4).

## Contexto

O usuário aprovou a direção do produto (2026-09-05): busca semântica sobre imagens de datasets ("RAG sobre datasets" — índice de embeddings + busca por texto e por imagem similar). Fatos verificados no código na sessão de design:

- **Manager e orquestrador são skeletons** (`services/manager/src/main.rs` e `services/orchestrator/src/main.rs`: só `/health`) — fila, jobs, VRAM, cliente S3 no orquestrador **não existem**; tudo é fatia 4. Estender o orquestrador para inferência agora custaria o esqueleto da fila = pagar a fatia 4 duas vezes.
- **O principal não toca GPU** (backend.md §1) e é o único com cliente S3 (ADR-0003 D8).
- **Postgres do compose é `postgres:16`** (compose.yaml) — imagem oficial que **não** inclui a extensão `vector`; a imagem `pgvector/pgvector:pg16` (oficial do projeto) adiciona pgvector ao Postgres.
- **Crate Rust `pgvector`** suporta sqlx ≥0.8 via feature `sqlx` (`Vector::from`, `bind`, `row.try_get`, `ORDER BY embedding <=> $1`) — compatível com a regra ADR-0002 D10 (SQL runtime, sem `query!`).
- **`open_clip_torch`**: `create_model_and_transforms('ViT-B-32', pretrained='laion2b_s34b_b79k')` + `encode_image`/`encode_text`; normalização L2 obrigatória antes do cosseno; ~245 ms 1ª inferência em CPU, ~50–80 ms depois; ViT-B-32 = 512 dims, ~2 GB VRAM. Pacote legado `openai/clip` está morto desde 2023.
- **10k–100k imagens**: 512 floats × 4 B = ~2 KB/linha → 20–200 MB de embeddings por dataset — cabe confortavelmente no Postgres local.

## Decisões

### D0 — A fatia é a **3f "busca semântica"**, posicionada **3d → 3f → 3e → 4**

Dependência única e explícita: a UI da busca vive **na galeria** (`/datasets/[id]`, criada pela 3d), então 3f depende da **3d**. 3f **não** depende da 3e (export/import) nem da 4 (jobs) — a D4 resolve o assíncrono sem fila. Os commits 3f.1–3f.5 são disjuntos de 3e e 4; só 3f.6 (UI) espera a galeria.
**Descartado:** fatia 5 pós-jobs — a indexação assíncrona é resolvível sem a máquina de jobs (D4), e adiar para depois da 4 (a maior fatia do roadmap) seria o único motivo não-técnico.

### D1 — Inferência via `EmbeddingPort` no principal; real = serviço Python `embedder` local; **remoto só na fatia 4**

Três camadas, uma porta (`src/search/embed.rs`, análoga a `StoragePort`):
- `MockEmbedder` — **default dev** (`EMBEDDING_BACKEND=mock`, sem rede, sem serviço): vetor 512d determinístico = hash do payload (imagem: sha256 dos bytes; texto: sha256 dos bytes UTF-8), normalizado L2. Permite testar ranking: `by-image` com a própria imagem retorna score 1.0 e top-1 = ela mesma.
- `HttpEmbedder` — caminho real: POST `http://<EMBEDDER_URL>/embed` `{model, items:[{id,b64}]}` (batch ≤ 32, base64) → `{items:[{id,vector,dim}]}` e `POST /embed-text` `{model, texts:[...]}`. **O embedder é o `trainer-clip` estendido com modo `serve`** (mesmo runtime Python, novo `serve.py` + entrypoint; `ENGINE_MOCK=1` → responde vetores hash sem carregar torch; sem flag → `open_clip_torch` ViT-B-32, GPU se `cuda` disponível, normalização L2 no servidor).
- **Local na v1**: serviço novo `embedder` no compose (container dedicado, tratado como infra local — como o SeaweedFS, não como job). **Remoto na fatia 4**: o mesmo container vira o runner-CLIP do §5 (orquestrador sobe, fila/VRAM); o principal só troca `EMBEDDER_URL` — a porta não muda. AutoLabel futuro (D7) reusa o mesmo serviço.

**Descartado:** estender orquestrador nesta fatia (manager/orquestrador são skeletons — duplicaria a fatia 4); worker indexador solto fora do orquestrador (viola a topologia §1); inferência no principal (proibido §1); `openai/clip` legado (morto).

### D2 — Índice = **pgvector no Postgres existente** (compose troca `postgres:16` → `pgvector/pgvector:pg16`, digest pinado)

`CREATE EXTENSION vector` + tabela `image_embeddings` + índice HNSW (`vector_cosine_ops`). Filtros por dataset/model viram SQL; busca = `ORDER BY embedding <=> $1 LIMIT k`. Não há serviço novo de busca; o Postgres já é dono de tudo (está no compose, backup com pg_dump cobre vetores).
**Descartado:** (b) faiss/npz no bucket — leitura do arquivo inteiro a cada busca, segundo runtime de índice, faiss é overkill para 10k–100k; (c) distância em app — 20–200 MB na RAM do principal por dataset, sem filtros SQL. HNSW global (pgvector não suporta filtro no índice): a busca filtra `dataset_id` no WHERE e o re-check acontece sobre o gráfico; aceitável para single-user com poucos datasets (benchmark no spike, R2).

### D3 — Embeddings **só de imagem** no índice; query por texto usa o text encoder na hora

CLIP é um espaço conjunto: texto de busca → `encode_text` (1 chamada) → `<=>` contra os image embeddings indexados. Indexar captions dobraria custo de indexação (2× GPU) e tamanho do índice para ganho marginal. Porta aberta sem migração: coluna `model` na linha + padrão de tabela por modalidade permite `caption_embeddings` análoga no futuro.

### D4 — Indexação **assíncrona best-effort, sem fila e sem jobs**: disparada por upload + rebuild manual; estado **derivado**

- Disparo 1: fim do `POST /upload` → `tokio::spawn` indexa os itens `stored` (semáforo de 4; por item: `StoragePort.get` → `HttpEmbedder`/mock → `INSERT ... ON CONFLICT (image_id) DO UPDATE`).
- Disparo 2: `POST /api/datasets/:id/search/index` → rebuild (mesmo indexador, varre imagens sem embedding do modelo ativo).
- **Estado derivado, sem tabela de fila**: `indexedCount = count(image_embeddings WHERE dataset_id=$1 AND model=$ativo)`; `GET /search/status` compara com `images_count`. Crash no meio = embeddings parciais = status `indexing` + reparação natural no próximo gatilho. Concorrência: `pg_advisory_lock(hashtext('heph_index:'||dataset_id))` em sessão, unlock no `finally` (lotes commitam em blocos de 100 — lock de sessão, não transacional).
- **Não depende da fatia 4**: a "fila" é a diferença entre `images` e `image_embeddings`; nenhuma tabela de jobs, nenhum toque no manager.

**Descartado:** síncrono no upload (10k–100k imagens × ~80–245 ms CPU = bloqueio inviável); fila própria no Postgres do principal (máquina de estado que a fatia 4 substituiria); lazy no primeiro uso (primeira busca travada, pior UX).

### D5 — Contrato: 4 rotas novas, spec **0.3.0 → 0.4.0**, camelCase, 2 códigos de erro novos

```
GET  /api/datasets/{id}/search?q&k&classId&split   200 400 401 404 409 503
POST /api/datasets/{id}/search/by-image            200 400 401 404 409 503
GET  /api/datasets/{id}/search/status              200 401 404
POST /api/datasets/{id}/search/index               202 401 404
```
- `GET /search`: `q` (1..500 chars, obrigatório), `k` (1..100, default 20), `classId`/`split` opcionais → **pós-filtro em Rust** sobre `k*4` candidatos (cap 400; HNSW não filtra no índice — R2). Resposta `SearchResponse{items:[{image: Image, score}]}` — `score` = cosseno **raw** (faixa -1..1, documentada; CLIP normaliza L2 no embedder). `classId` não-UUID → 400 (é filtro opcional, não id de recurso — divergência consciente do D8, documentada na spec).
- `POST /search/by-image` `{imageId, k?, threshold?}` — `imageId` de uma imagem **do dataset** (a UI: clique na imagem → "buscar similares"); `threshold` opcional filtra score (o "80% do dedup" da D6). Upload de imagem arbitrária para buscar = v2 (mesmo transporte do indexador, só falta a rota).
- `GET /search/status` → `SearchStatus{status: not_indexed|indexing|ready|stale, imagesCount, indexedCount, model, dim}`. Derivação: 0 embeddings → `not_indexed`; `0 < indexed < images` → `indexing`; igual → `ready`; `stale` reservado (v1 nunca emite — existe para a futura troca de modelo; o front DEVE ter ramo default).
- `POST /search/index` → 202 sempre `{status: indexing|not_indexed}` (dataset sem imagens não trabalha; advisory lock serializa segundos disparos). Erros novos no enum `Error.code`: **`index_not_ready` (409)** — busca em dataset sem embeddings do modelo ativo; **`embedding_unavailable` (503)** — embedder inalcançável (família do `storage_unavailable`, ADR-0003 D10). Wire camelCase (guardado por teste, ADR-0002 D1); `id`/`imageId` não-UUID → 404 (D8 replicado); 413/405/500 seguem a convenção global.

### D6 — Dedup: **fora desta fatia**; `threshold` do by-image já cobre 80%

O schema não fecha portas (busca por imagem + threshold é o pré-dedup natural; marcar/agrupar duplicatas na UI é fatia futura, independente de schema).

### D7 — AutoLabel assistido: **fora de escopo**, design não fecha portas

O serviço `embedder` é o mesmo runtime que o AutoLabel usará (engine clip serve); busca por texto sobre imagens é a base de "auto-rotular por query". Nada no schema impede.

### D8 — Casing/erros/limites seguem as ADRs anteriores (não é decisão nova)

camelCase no wire, `deny_unknown_fields`, SQL/`Error.code` snake_case (ADR-0002 D1/D6); `id` não-UUID → 404 (D8); SQL runtime sem `query!` (D10) — o crate `pgvector` binda `Vector` em `sqlx::query` runtime, sem macro.

## Consequências

- **Dependências novas**: `pgvector` (crate, feature `sqlx`) no `api-principal`; `open_clip_torch` + `torch` + `PIL` no `trainer-clip` (só modo serve; mock não importa torch). Nenhuma no manager/orchestrator.
- **Env vars (principal)**: `EMBEDDING_BACKEND=mock|http` (default `mock`), `EMBEDDER_URL` (default `http://embedder:8090`), `EMBEDDING_MODEL` (default `ViT-B-32`). Sem segredo (embedder interno, rede docker; hardening futuro). Só `ViT-B-32`/512d é validado — outro modelo quebra o CHECK do banco honestamente (v1 não suporta troca de modelo; ALTER futuro).
- **compose.yaml**: serviço novo `embedder` (build `engines/trainer-clip`, `ENGINE_MOCK=1` default, porta interna 8090, cache do peso em volume `models`); `db` troca `postgres:16` → `pgvector/pgvector:pg16` **com digest pinado** (dívida "fixar digests" honrada no nascedouro — lição `fix/infra-env`).
- **Front**: painel de busca na galeria (3d) — barra de texto, "buscar similares" por imagem, grade de resultados com score, badge de status do índice ("Indexar agora" quando `not_indexed`; "Indexando X/N" quando `indexing`).

## Migration `0004_search.sql` (contorno)

```sql
CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE image_embeddings (
    image_id    UUID PRIMARY KEY REFERENCES images(id) ON DELETE CASCADE,
    dataset_id  UUID NOT NULL REFERENCES datasets(id) ON DELETE CASCADE,
    model       TEXT NOT NULL CHECK (model IN ('ViT-B-32')),   -- v1: enum fechado, cresce por fatia
    embedding   vector(512) NOT NULL,                          -- dim fixa exigida pelo HNSW
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- dataset_id denormalizado de propósito: filtro da busca + CASCADE duplo
-- (consistência é obrigação do indexador, não há CHECK cross-tabela).
CREATE INDEX image_embeddings_dataset_model_idx ON image_embeddings (dataset_id, model);
CREATE INDEX image_embeddings_embedding_hnsw ON image_embeddings
    USING hnsw (embedding vector_cosine_ops) WITH (m = 16, ef_construction = 64);
```

Invariantes: `embedding` 512d imposto pelo tipo; `model` enum fechado v1; contadores do dataset **não mudam** (embeddings não entram em `size_bytes`/`labeled`); `image_embeddings` é filha — `DELETE /datasets/:id` já cascateia (sem mudança no sweep). Exercitada por: migration aplica em banco limpo + teste de integração (INSERT, `<=>` ordena, CASCADE no DELETE dataset).

## Decisões de boundary (quem é dono de quê)

- **Principal**: dono do índice (escreve `image_embeddings`), do gatilho de indexação, das 4 rotas, do `EmbeddingPort` (mock in-process). Nunca toca GPU.
- **Embedder (serviço Python)**: dono da inferência (image + text encoding). Stateless: modelo vem no request, não em config própria.
- **Bucket**: continua verdade binária; o embedder **não** fala S3 (ADR-0003 D8) — o principal baixa o objeto (`StoragePort.get`) e manda bytes.
- **Manager/orquestrador**: zero mudança nesta fatia (D0/D1). A unificação embedder→runner acontece na fatia 4.
- **Postgres**: extensão `vector` habilitada no `db` do compose (boundary de Postgres compartilhado preservado — tabela é do principal, como `images`).

## Plano de commits da 3f (produção < 400 linhas cada; contrato/testes fora da conta, exceção registrada como na 3a)

| # | Commit | O que | Critério de aceite |
|---|---|---|---|
| 3f.0 | `spike(search): pgvector+HNSW+sqlx+crate pgvector+embedder mock` | harness descartável + matriz `spike/SEARCH-SPIKE.md` | 5/5 critérios binários (ver "Spike") |
| 3f.1 | `feat(search): migration 0004 — extensão vector + image_embeddings + HNSW` | DDL + testes db (aplica; INSERT/`<=>`; CASCADE no DELETE dataset) + compose `db` → imagem pgvector (digest) | `bash scripts/test-db.sh` verde; `compose config -q` OK |
| 3f.2 | `feat(search): EmbeddingPort + Mock + Http + AppState + env de boot` | `src/search/embed.rs` + state/main + env | `cargo test -p api-principal` (units do mock: determinismo, normalização, ranking by-image top-1) |
| 3f.3 | `feat(search): engine trainer-clip modo serve + serviço compose embedder` | `serve.py` + entrypoint + Dockerfile + compose | container mock responde `/embed` e `/embed-text`; `ENGINE_MOCK=1` sem torch importado |
| 3f.4 | `feat(search): indexação — upload dispara background + POST /index + status derivado` | spawn+semáforo, advisory lock, upsert, `GET /search/status` | integração: upload → `indexedCount` sobe; rebuild idempotente; lock serializa 2 disparos |
| 3f.5 | `feat(search): rotas de busca + OpenAPI 0.4.0 + contract` | `GET /search`, `POST /by-image`, pós-filtro classe/split, 2 erros novos, spec + `PROTECTED_ROUTES` | contract verde (inventário spec≡router, camelCase, 401 nas 5, 404 D8, 409/503 novos) |
| 3f.6 | `feat(web): painel de busca na galeria` | SearchBar + resultados (thumb+score) + "buscar similares" + badge de status | `npm run build --workspace=web`; smoke Chrome com mock |
| 3f.7 | `docs(adr): ADR-0004 aceita + sincroniza docs` | docs-sync + banner | 0 `.rs` |

Dependência de sequência: **3f.6 requer a 3d landada** (galeria existe); 3f.1–3f.5 não dependem de 3d/3e/4. Ordem no roadmap: 3d → 3f (1–5 podem ser despachadas em paralelo ao fim da 3d) → 3e → 4.

## Spike obrigatório — `3f.0` (~250 linhas descartáveis, branch `spike/search-pgvector`)

Premissas externas não verificadas: comportamento da imagem `pgvector/pgvector` com volume `pgdata` existente, crate `pgvector` × sqlx 0.8 no ferro, recall do HNSW 512d, e o container embedder. Critérios binários:

1. `pgvector/pgvector:pg16` (digest pinado) sobe com o volume `pgdata` do `postgres:16` atual **sem dump/restore**; `CREATE EXTENSION vector` OK.
2. Crate `pgvector` (feature `sqlx`) com `sqlx::query` runtime: INSERT de `Vector` 512d → SELECT com `ORDER BY embedding <=> $1` ordena certo (round-trip, `row.try_get`).
3. HNSW 512d com 10k vetores: build < 30 s; busca k=10 < 50 ms; top-1 bate com brute force em ≥ 90% dos casos.
4. Container `embedder` com `ENGINE_MOCK=1`: `/embed` (batch 32) e `/embed-text` respondem < 500 ms cada; teste manual GPU (opcional, `@gpu`): `open_clip_torch` ViT-B-32 `laion2b_s34b_b79k` carrega e infere 1 imagem (sem threshold de tempo).
5. `pg_dump` do banco com vetores → restore preserva o tipo.

Falha em 1 → imagem oficial + `docker-entrypoint-initdb.d` compilando a extensão (mais caro, D2 fica, custo sobe); 2 → encode/decode manual via `ToSql`/`FromSql` do sqlx (sem crate); 3 → `ivfflat lists=100` ou busca exata (10k é pequeno — HNSW vira otimização, não requisito); 4 → embedder fica mock-only até a fatia 4 (HttpEmbedder testado contra servidor axum fake in-process).

## Riscos e o que testar

- **R1 — upgrade de imagem postgres com `pgdata` existente** (tag mutável → digest; dívida honrada): critério 1 do spike; se falhar, dump/restore manual documentado.
- **R2 — HNSW global + filtro `dataset_id`**: re-check pós-gráfico; aceito v1 (single-user, poucos datasets); benchmark 10k no spike; mitigação futura = particionamento por dataset (PG15+), fora de v1.
- **R3 — embedder sem auth** (rede docker interna, bind não exposto): hardening futuro; nada exposto além de loopback no compose.
- **R4 — crash no meio da indexação**: embeddings parciais; estado derivado mostra `indexing`; reparação natural no próximo gatilho; e2e cobre "upload → status → busca".
- **R5 — download do peso ViT-B-32 (~600 MB)** no primeiro boot real: só no caminho sem mock; cache em volume `models`.
- **R6 — API do `open_clip_torch`**: `openai/clip` legado morto (2023); usar ≥2.20 com `pretrained='laion2b_s34b_b79k'`; normalização L2 no servidor. Coberto pelo critério 4 do spike.
- **R7 — custo CPU real** (dev sem GPU): 10k imagens ≈ 20–40 min em background; status mostra progresso; GPU corta para minutos.
- **R8 — mock ≠ semântica real**: testes de ranking usam `by-image` com a própria imagem (prova o transporte, não a qualidade); qualidade semântica só com GPU manual (`@gpu`), registrado como teste manual.
- **R9 — veto de `query!`**: crate `pgvector` binda em runtime; adicionar macro no futuro quebraria `cargo check --workspace` sem banco — não fazer.
- **Testar**: contract (inventário/casing/401/404/409/503), units (mock determinístico, pós-filtro classe/split), integração db (migration, ordenação `<=>`, CASCADE, advisory lock, status derivado), e2e smoke estendido (create→upload→index→search text→by-image→status ready→delete→tabela vazia).

## O que fica falso nos docs quando a 3f landar (lista para o `@docs-sync`, commit 3f.7)

`backend.md` §11 (compose: `postgres:16` → `pgvector/pgvector:pg16` pinado; serviço novo `embedder`; volume `models` ganha cache de peso), §10 (tabela `image_embeddings` + índices na lista), §9 (grupo datasets +4 rotas), §1 (topologia: nota do embedder como runner-especializado sem fila na v1, unificado na fatia 4). `frontend.md` §5.2 (galeria ganha painel de busca), §10 (contratos novos). `docs/dividas.md` (dívida digests: item postgres pagável). `coordenacao.md` (roadmap: 3f inserida). **Não aplicar antes da fatia** — docs descrevem o que existe.
