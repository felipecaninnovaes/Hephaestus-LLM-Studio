# SPIKE 3f.0 — matriz de resultados (pgvector × sqlx × HNSW × embedder mock)

> **Ramo descartável `spike/search-pgvector`.** Saída exigida pela ADR-0004
> ("Spike obrigatório 3f.0", 5 critérios binários). Data: 2026-09-07.
> Ambiente: host linux, cargo/rustc 1.97.1, docker 29.7, sqlx **0.8.6** (o do
> workspace), crate **pgvector =0.4.1**, imagem **`pgvector/pgvector:pg16-trixie`**
> (digest pinado), `postgres:16@sha256:f1c3376…` (o do compose, PG 16.15 pgdg13).
> Harness: `spike/search/` (crate Rust + `mockserve/` stdlib + `run-spike.sh`
> orquestrador — script único que imprime a matriz; containers/volumes `spike-*`
> temporários, o volume REAL do dev só é **copiado**, nunca tocado).

## Veredito: **5/5 critérios PASSAM.** D1 (porta/mock), D2 (pgvector no Postgres) e D4 (índice assíncrono) ficam como aprovadas, com **3 correções na ADR** (abaixo).

| # | Critério (ADR-0004) | Resultado | Evidência |
|---|---|---|---|
| 1 | `pgvector/pgvector:pg16` sobe com o volume `pgdata` do `postgres:16` atual **sem dump/restore**; `CREATE EXTENSION vector` OK | **PASS — com correção de tag** | Com a tag `pg16` (bookworm) o PGDATA **abre** (9 tabelas do dev intactas, WAL redo ~0.001 s), mas **collation version mismatch** (glibc 2.41→2.36): `CREATE DATABASE` falha (template1) e índices btree de texto ficam suspeitos. Com **`pg16-trixie`** (mesma base do `postgres:16` oficial atual — PG 16.15-1.pgdg13+2 idêntico): boot com **0 fatais**, `CREATE EXTENSION vector` = 1 **no banco real migrado** (studio), 9 tabelas do dev, e `CREATE DATABASE spikedb` limpo. Cópia do volume feita com `cp -a` (o real só lê). |
| 2 | Crate `pgvector` (feature `sqlx`) com `sqlx::query` runtime: INSERT `Vector` 512d → `ORDER BY embedding <=> $1` ordena certo | **PASS — com correção de versão** | **`pgvector` 0.4.2 (2026-05) exige sqlx-core 0.9** — dois sqlx no grafo (0.8.6 + 0.9.0) → trait `Type` incompatível, 14 erros E0277/E0433. **`pgvector =0.4.1` (2025-05) casa com sqlx 0.8.6** (um único sqlx-core no grafo, `cargo tree` provado). Com ele: INSERT de 100 vetores via `.bind(Vector::from(...))`, query `SELECT id, (v <=> $1)::float8 … ORDER BY v <=> $1 LIMIT 5` → `top1_id=8 self_dist=0 monotonic=true roundtrip_exact=true` (round-trip por bits, `row.try_get::<_, Vector>`). |
| 3 | HNSW 512d com 10k vetores: build < 30 s; busca k=10 < 50 ms; top-1 ≥ 90% vs brute force | **PASS com folga** | Clusters determinísticos (100 centros × 100 membros + ruído ±0.03, LCG semeado). `bulk_insert_10k=0.28 s` (QueryBuilder, 1000 binds/stmt). **`hnsw_build=1.76 s`** (limite 30). 20 queries com `SET enable_seqscan=off`: **p50=0.317 ms, p95=1.497 ms** (limite 50). **`top1_match=20/20, overlap10=1.00`** contra brute force seq-scan (rodado antes do `CREATE INDEX`). `SET hnsw.ef_search=40` (default). |
| 4 | Container `embedder` com mock: `/embed` (batch 32) e `/embed-text` < 500 ms; paridade com o mock Rust | **PASS** | `mockserve/serve.py` (stdlib puro, **sem torch**) em `python:3.12-slim` (digest pinado), porta 8090: `/health` ok; **POST /embed batch 32 (~2 KB/item) = 17.9 ms; POST /embed-text (3 textos) = 2.6 ms**; resposta `{items:[{id,vector,dim:512}]}` completa. Paridade tripla do vetor mock (payload canônico × 64, 2048 B): **rust×python maxdiff=1.39e-17; serve.py×python maxdiff=0.00** — contrato do mock fixado e idêntico nas duas linguagens. Teste GPU manual (`@gpu`) fica para a 3f.3 (peso ~600 MB). |
| 5 | `pg_dump` com vetores → restore preserva o tipo | **PASS** | `pg_dump -Fc` do banco com 10k embeddings + índice HNSW → `pg_restore` num container pgvector com banco limpo: **0 erros**, `rows=10000`, `ext_vector=1`, `v<=>v=0`, e **`md5(v::text)` idêntico origem↔restore** (todos os 512 valores preservados). |

## Achados que **corrigem a ADR-0004** (aplicar nos commits 3f.1–3f.5)

1. **Tag da imagem: `pgvector/pgvector:pg16-trixie@sha256:c8483555ce48101872f888c1df8a895ff689d6c7c7a5f7ac266475f9dfe89e0b`** (pgvector **0.8.6**, PG **16.15-1.pgdg13+2** — mesma build exata do `postgres:16` pinado no compose). A ADR D2 mandava "pgvector/pgvector:pg16 (digest pinado)" — a tag `pg16` é **bookworm/glibc 2.36** e conflita com o volume criado pelo `postgres:16` oficial atual (**trixie/glibc 2.41**): abre, mas com collation version mismatch que quebra `CREATE DATABASE` (erro em template1) e invalida teoricamente índices btree de texto. **Nunca misturar bases glibc entre postgres ↔ pgvector.** Se a imagem oficial `postgres` subir de base no futuro, re-alinhar a tag pgvector no mesmo commit.
2. **Crate: `pgvector = { version = "=0.4.1", features = ["sqlx"] }`** no `api-principal`. O 0.4.2 (mai/2026) subiu para sqlx-core 0.9 → dois sqlx no grafo com o workspace (0.8.6) = 14 erros de trait. Pinnar `=0.4.1` (ou migrar o workspace para sqlx 0.9 — fora de escopo da 3f). O crate é SQL runtime puro: D10 (veto de `query!`) segue válido.
3. **Volume copiado quente entra em crash recovery** ("database system was interrupted → automatic recovery", redo ~0.001 s) — FATALs transitórios `the database system is starting up` durante o startup são benignos. Na troca real do compose (3f.1): `docker compose down` → trocar imagem → `up` faz o mesmo recovery; sem dump/restore **confirmado**.
4. **Planner com 10k linhas preferiu seq scan** (`planner_sem_seqscanoff_uses_hnsw=false`): com `LIMIT k` pequeno e poucas linhas, o custo do seq scan vence o HNSW. Não é bloqueio (10k×512 seq = ~1 ms; R2 da ADR); em 100k+ o planner troca sozinho. Para a 3f.5: **não** fazer `SET enable_seqscan=off` em produção (foi recurso do benchmark) — deixar o planner decidir; o pós-filtro em Rust sobre `k*4` candidatos continua igual.
5. **Subscripting `v[i]` não existe no pgvector 0.8.6** (`cannot subscript type vector`) — comparação de valores via `v::text`/`md5`. Sem impacto no produto (o crate lê o vetor inteiro), só para testes.
6. **`EXPLAIN (FORMAT JSON)` devolve coluna JSON** (sqlx exigiria feature `json`) — usar FORMAT TEXT no ferramental de diagnóstico.
7. **Contrato do mock fixado (idêntico Rust × Python)**: `h = sha256(payload); repetir h = sha256(h) e extrair 4 f64 por bloco (u64 LE → (q/2^53)*2−1) até 512` → normalização L2. `/embed-text` ecoa `id = índice do array`. Serve Python: `POST /embed {model, items:[{id,b64}]}` → `{items:[{id,vector,dim}]}`; `POST /embed-text {model, texts:[…]}`; `GET /health`.
8. Números de referência para a 3f.4 (indexação): bulk insert 10k = **0.28 s**; com mock ~80–245 ms/imagens o gargalo é o embedder, não o Postgres.

## Fallbacks da ADR que NÃO foram acionados

- F-1 (imagem custom + initdb compilando extensão) — desnecessário.
- F-2 (encode/decode manual via `ToSql`/`FromSql`) — desnecessário (crate 0.4.1 binda).
- F-3 (ivfflat/busca exata) — desnecessário (HNSW 1.76 s / 0.32 ms / 100% top-1).
- F-4 (embedder mock-only até a fatia 4) — desnecessário (container mock provado < 500 ms).

## O que este spike NÃO prova (residual, honesto)

- Qualidade semântica real do CLIP (mock é hash — R8; teste `@gpu` manual na 3f.3, peso ~600 MB).
- Comportamento com 100k+ vetores (dev não justifica; o desenho D2 tolera — re-benchmark só se aparecer dataset real grande).
- Concorrência real de indexação (semáforo 4 + advisory lock — coberto por testes de integração da 3f.4, não por este spike).
