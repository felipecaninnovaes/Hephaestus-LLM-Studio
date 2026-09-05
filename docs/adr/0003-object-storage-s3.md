# ADR-0003 — Storage de objetos S3 como blob canônico (Fatia 3b)

- **Status:** PROPOSTO — aguarda escolha do servidor S3 padrão (§D0) e aprovação do
  usuário. Nenhum código escrito; nenhuma linha de `backend.md`/`frontend.md` alterada
  ainda. Se rejeitado, este arquivo morre aqui sem custar migração.
- **Data:** 2026-09-04
- **Anexa/substitui parcial:** `docs/backend.md` §1/:15, §1/:26, §3/:49, §3/:50, §10/:148,
  §10/:160-161, §10/:165, §10/:183, §10/:188, §11/:195; `docs/adr/0002-datasets-core.md`
  T3 e o `DATASETS_DIR` das Consequências.
- **Motivação:** proposta do usuário — usar MinIO (como ele já opera no VisionLens) para
  imagens e guardar labels/coordenadas no banco. A 3b ainda não tinha uma linha escrita,
  então adoitar agora custa zero migração; adoitar depois custaria reescrever a 3b +
  migrar datasets + refazer a coluna `source`.

## Contexto

`backend.md` §3 mandava manter **blobs canônicos em disco no formato por engine**
(YOLO `images/ + labels/*.txt + data.yaml`). Isso cria duas verdades (banco × disco) e o
debounce de materialização do §10/:183 existe só para costurá-las. O §3 também especifica
um transporte chunkado de 8 MB com `chunk_bitmap`/resume para levar dataset ao
orquestrador.

Fatos verificados nesta decisão (fonte primária, consultada em 2026-09-04):

- **MinIO OSS está morto como produto**: `github.com/minio/minio` arquivado pelo dono em
  **25/04/2026**, read-only, README em caixa alta "THIS REPOSITORY IS NO LONGER
  MAINTAINED", distribuição da edição comunidade **só de código-fonte** (sem binários/
  imagens atualizadas). Última release `RELEASE.2025-10-15T17-29-55Z`.
  https://github.com/minio/minio
- **GHSA-9c4q-hq6p-c237** (CVE-2026-40344, High 8.8): write não autenticado via
  `STREAMING-UNSIGNED-PAYLOAD-TRAILER` no handler Snowball; advisory lista
  **"Patched versions: None"** para o OSS (conserto apenas em AIStor).
  https://github.com/minio/minio/security/advisories/GHSA-9c4q-hq6p-c237
- **A trilha de streaming é onde os SDKs e o MinIO brigam**: `minio/minio#21611`
  (aws-chunked > 16 MiB → `chunk too big`, aberto desde 2025) e **`minio/minio#21303`,
  que é o SDK Rust com `ByteStream::from_path`** falhando no mesmo ponto.
  https://github.com/minio/minio/issues/21611 · https://github.com/minio/minio/issues/21303
- Alternativas mantidas: **SeaweedFS** (Apache-2.0, desde 2012, backend default que o
  Kubeflow Pipelines adotou no lugar do MinIO, ~36 releases em 2026, `weed server -s3`
  expõe `:8333` com bucket pré-criado e credenciais), **Garage** (Rust, binário único +
  TOML, roda em 2 GB RAM, suporta presigned URL e multipart, **sem** versioning/object
  lock, AGPL-3.0, repositório canônico em Forgejo próprio), **RustFS** (Apache-2.0,
  reimplementação drop-in do MinIO, portas 9000/9001 + console, porém `1.0.0-rc.3` em
  ago/2026 — **sem 1.0 estável**). Fontes: READMEs e comparações de 2026
  (elest.io, ossalt.com, shipgarden.com, lowcloud.io) — **SEARCH-ONLY**, não verificadas
  item a item em docs oficiais dos projetos.

Evidência do VisionLens (`/home/felipecn/DEV/VisionLens`, operado pelo usuário):
`apps/api/src/infra/storage/base.py` (porta `StorageClient` com 8 métodos) +
`s3_storage.py` + `mock_storage.py`; `s3_storage.py:42-57` resolve o gotcha de que **o
header `Host` entra na assinatura SigV4, então o presigned tem de ser assinado com o
endpoint público**; `models_ml/yolo_trainer.py` confirma que o ultralytics **exige**
árvore de arquivos (tempdir com `images/train` + `labels/train/*.txt` + `data.yaml`, e
`shutil.rmtree` no `finally`); upload presigned dele chega ao banco por **notificação de
bucket** (`handlers/s3_notification.py` + `workers/event_consumer_worker.py` + coluna
`phash`), ou seja: 3 componentes para o servidor descobrir o que fez com os bytes.

## Decisões

### D0 — Servidor S3 padrão: **em aberto** (decisão do usuário)

O que não está em discussão: falamos **S3 API por uma porta** (`StoragePort`, D8), então a
escolha do servidor é config, não código. O que está:

| | SeaweedFS | Garage | RustFS |
|---|---|---|---|
| Licença | Apache-2.0 | AGPL-3.0 | Apache-2.0 |
| Maturidade | alta (2012, Kubeflow) | média (2020, core S3) | **pré-1.0** |
| Ops local | master+volume+filer (1 cmd `weed server -s3`) | 1 binário + TOML | 1 binário, igual MinIO |
| Bucket no boot | **pré-criado** (mata o init-container) | precisa criar | precisa criar |
| Console p/ backup | sim | web UI limitada | sim (9001) |
| Presigned/multipart | a **verificar no spike** | suportado | suportado |

Recomendação do coordenador: **SeaweedFS** (maturidade manda quando o bucket é a única
cópia das imagens do usuário; bucket pré-criado simplifica o compose; Apache-2.0) com
critério de presigned validado no spike; **Garage** como plano B se a simplicidade de um
binário pesar mais; **RustFS descartado para o default** (pré-1.0 guardando o dado mais
valioso do produto). **MinIO descartado** por evidência, não por gosto.

### D1 — Bucket é o único armazém de blobs canônicos; disco local só efêmero

Imagens e vídeos viram objetos. Qualquer árvore de arquivos por engine nasce em tempdir
no orquestrador e morre no `finally` — padrão que o próprio VisionLens valida. Postgres
continua a única verdade **relacional**; o bucket vira a única verdade **binária**, com as
linhas de `images` como índice dele. Descartado: disco+S3 híbrido (duas verdades) e FS
puro atrás de porta (manteria o volume no principal e a migração quando o remoto chegar).

### D2 — Upload passa pelo principal, com **spool em tempfile + PUT de length exato**

`POST /api/datasets/:id/upload` (multipart, N arquivos) → spool por arquivo → `md5` +
`sha256` + `w/h` + `media_type` por **magic bytes** → `put_object` com content-length
exato (sem `aws-chunked`, sem trailer) → `INSERT`. Justificativa dupla:

1. **Corretude de dados:** só assim a linha de `images` nasce com hash/dimensões
   sincronamente. Upload presigned browser→bucket exigiria `complete` + `HEAD` + sonda de
   imagem no servidor, ou a máquina assíncrona do VisionLens (notificação de bucket →
   webhook → worker + estado "pendente") — 3 componentes novos para resolver um problema
   que não existe: upload não é hot path.
2. **Segurança/compatibilidade:** é o único modo de PUT que todo servidor S3-compatível
   trata como rotineiro; streaming com trailer é justamente a trilha com issue aberta no
   MinIO e com CVE não corrigida (contexto acima).

Limite `DefaultBodyLimit` **dedicado de 200 MB nesta rota** (+ margem de envelope
multipart), `LengthLimitError` → 413 `invalid_request` no envelope (padrão ADR-0002 T10).
Resposta **por item**: `stored|duplicate|rejected|failed` com `reason` ∈
`duplicate_filename|unsupported_media|too_large|storage_error`. `filename` do form nunca
chaveia nada (não confiável). Credencial S3 jamais chega ao browser; CORS não existe.

### D3 — Leitura híbrida por flag `S3_PUBLIC_ENDPOINT_URL` (assinada no host público)

`GET /:id/images` devolve `url` por imagem: com a flag → presigned GET (TTL
`S3_URL_TTL_SECS`, default 3600) **assinado com o endpoint público** (gotcha SigV4
aprendido com o VisionLens); sem a flag → `/api/datasets/:id/images/:imageId/data`, que o
rewrite Next já roteia ao principal com cookie same-origin. A rota `/data` existe
**incondicionalmente**. Descartado: always-proxy (principal na hot path da grade) e
always-presigned (quebra "funciona sem configurar nada" e deixa o fallback intestável).

### D4 — Crate: `aws-sdk-s3` sem `aws-config`

`config::Builder` + `StaticCredentialsProvider` (creds de env própria; cortar a cadeia de
providers IMDS/SSO/profile é o grosso do peso do meta-crate), `force_path_style(true)`
(obrigatório p/ S3-compatível em docker), `request_checksum_calculation(WhenRequired)` —
que é o que impede o SDK de entrar sozinho na trilha trailer (lição de #21611/#21303).
Entrega `put_object`/`get_object`/`ListObjectsV2`/`DeleteObjects`/`presigner` e
multipart-copy futuro. Descartado: `rusty-s3`+`reqwest` (reimplementar XML de
List/Delete/retries é dívida criada para economizar compilação — e o swap continua
possível atrás da porta D8, então é decisão reversível); `rust-s3`/`minio-rs`; SigV4 à mão
(assinar à mão é o erro que o VisionLens carrega cicatrizado). Nenhum `reqwest` na 3b.

### D5 — Chaves legíveis, `object_key` canônico; coluna `datasets.source` morre e vira derivado

`datasets/{dataset_id}/images/{image_id}/{filename_sanitizado}` (vídeos idem sob
`videos/`). Extensão vem do sniffing, não do nome do form. IDs são imutáveis → a chave
nunca muda (renomear slug não toca storage); a pasta por imagem já reserva espaço para um
`thumb.webp` futuro sem migração. **Dedupe não é função do storage:** `md5`/`sha256` são
colunas e `UNIQUE(dataset_id, filename)` resolve o caso real ("mandei o mesmo lote
duas vezes" → item `duplicate`, zero PUT). Conteúdo-endereçado (`<sha[0:2]>/<sha>`)
descartado: sem requisito de dedupe inter-histórico (single-user), torna o backup
ilegível e exigiria rename no delete.

`images.path` do §10 → **`images.object_key`** (tabela nasce agora, sem dado a migrar).
`datasets.source`: **`ALTER TABLE datasets DROP COLUMN source`** na 0003 (era "caminho em
disco", conceito morto); no wire `Dataset.source` **permanece** (contrato 0.2.0 não quebra)
com valor derivado server-side — `s3://{bucket}/datasets/{id}/` quando `images_count > 0`,
`null` enquanto vazio. Bucket no valor é config, não dado.

### D6 — Só mídia vira objeto; anotação é linha no banco; artefatos por engine são derivados

`boxes` (0-1, 6 decimais no build) e `captions` (PK=`image_id`) são a verdade; `data.yaml`,
`labels/*.txt`, `captions.jsonl` e pares `.parquet` do CLIP são materializados no tempdir
do orquestrador a partir do Postgres. Consequência: **a linha do §10/:183 sobre
"materializar o `.txt` com debounce (~2s)" morre** — não há mais o que materializar.
Descartado: espelhar `.txt`/`captions.jsonl` como objetos (duas verdades por anotação).

### D7 — Ordem de escrita objeto→linha→compensação; varredura de prefixo pós-commit

PUT do objeto (chave já conhecida, `image_id` gerado antes) → `INSERT ... ON CONFLICT
(dataset_id, filename) DO NOTHING`; sem linha ⇒ `duplicate` + **DELETE do objeto recém-
postado**; erro duro ⇒ DELETE de melhor esforço + 500. O estado ruim aceitável é **objeto
sem linha** (invisível, reapável); nunca linha sem objeto (UI quebrada).
`DELETE /api/datasets/:id`: commit do banco primeiro (CASCADE em
`images/boxes/captions/videos`), **depois** varredura do prefixo (`ListObjectsV2` paginado
+ `DeleteObjects` de 1000); falha da varredura loga e **não** transforma o 204 em erro —
o prefixo fica reapável. Mantém o stance delete-after-commit da ADR-0002 T3, agora
sweep-after-commit. GC de órfãos fora do prefixo de dataset vivo: **deliberadamente
fora** da 3b (script `mc find` × banco na fatia de manutenção).

### D8 — `StoragePort` + `MockStorage`; na 3b **somente o principal** fala S3

`put/get/presign_get/delete/delete_prefix` — os 9 métodos do VisionLens aparados para 5 (sem
`get_file_metadata` porque o spool já tem tudo em mãos; sem `file_exists` porque a chave é
gerada, não consultada). `AppState` ganha `storage: Arc<dyn StoragePort>` +
`StorageConfig { bucket, public_endpoint, url_ttl_secs }`; boot fail-fast se faltar `S3_*`
(mesma política de `DATABASE_URL`, sem ecoar credencial na mensagem). Testes de contrato
usam `MockStorage` (HashMap em `RwLock`) e rodam **sem rede**. Manager e orquestrador não
têm cliente S3 na 3b (zero uso); quando jobs chegar, o orquestrador recebe **manifest com
`key`** (snake_case, §11) e usa credencial de serviço escopada por prefixo no compose
local; para o remoto, presigned-assinado-no-endpoint-alcançável entra junto do TLS+pin
`heph_o_*` da ADR-0001. Descartado: crate compartilhado `heph-storage` agora (abstração
sem segundo consumidor — extrai-se trivialmente quando o orquestrador precisar; a
reversibilidade de D4 é o propósito da porta).

### D9 — Export/import/package sobem para 3e; chunking de 8 MB sobrevive só no transporte remoto

`POST /:id/export`, `POST /datasets/import`, `POST /:id/package` ficam fora da 3b: nenhuma
UI da 3c/3d os consome antes da galeria existir, e cada um arrasta dependente (zip
server-side, ingest `.zip`→objetos, manifest+md5 pro orquestrador). O chunking de 8 MB
renasce na fatia de orquestrador remoto como transporte **exclusivamente**
principal→orquestrador remoto; entre principal e bucket local não há chunk nenhum. Ordem
revisada: **3b storage+imagens+anotação → 3c UI lista → 3d UI galeria/anotação → 3e
export/import → 4 jobs/package/materialização**. Requisito de backup do usuário no
intervalo: console do servidor + `mc mirror` (funciona sem código).

### D10 — Um erro novo: `storage_unavailable` (503); spec 0.2.0 → 0.3.0

Bucket inalcançável em PUT/GET/sweep → 503 `{code:"storage_unavailable"}` (message
estático, sem endpoint no corpo). Nenhum 415 dedicado: arquivo não-imagem num lote é
resultado por item; lote inteiro indecodável → 400 `invalid_request`. 413 já é convenção
global da spec.

## Contrato desta fase (delta OpenAPI — 0.3.0)

Rotas (as 6 novas entram em `PROTECTED_ROUTES` com estes status exatos; 413/405/500
seguem na convenção global, como hoje):

```
POST /api/datasets/:id/upload                  200 400 401 404 503
GET  /api/datasets/:id/images                  200 401 404
GET  /api/datasets/:id/images/:imageId         200 401 404
GET  /api/datasets/:id/images/:imageId/data    200 401 503 404
PUT  /api/datasets/:id/images/:imageId/boxes   200 400 401 404
PUT  /api/datasets/:id/images/:imageId/caption 200 400 401 404
```

Schemas novos: `UploadResult{items[]}`, `UploadItem{imageId,filename,status,reason,bytes,
width,height}`, `ImagePage{items,total,limit,offset}`, `Image{...,objectKey,mediaType,
split,url}`, `ImageDetail`(=Image+`boxes[]`+`caption`), `Box`, `PutBoxesRequest/Response`,
`PutCaptionRequest`, `CaptionResponse`. `limit` default 50, máx 200; filtros `split`
(`train|val`) e `labeled`. Wire camelCase (ADR-0002 D1, guardado por teste); `id`/
`imageId` não-UUID → 404 `not_found` (ADR-0002 D8 replicado).

## Migration `0003_images.sql` (contorno)

`ALTER TABLE datasets DROP COLUMN source;` + `images`(id, dataset_id FK CASCADE, filename,
`object_key UNIQUE`, bytes, width>0, height>0, md5 `^[0-9a-f]{32}$`, **sha256**
`^[0-9a-f]{64}$`, media_type ∈ jpeg|png|webp, split default train, created_at,
UNIQUE(dataset_id, filename), índices `(dataset_id)` e `(dataset_id, split)`), `boxes`
(FKs CASCADE em image_id e class_id, x/y/w/h `CHECK 0..1`, conf NULL, origin ∈
manual|autotracker|import, track_id, índice `(image_id)` e `(class_id)`), `captions`
(PK=image_id, text 1..8000, origin, model, `updated_at` pelo trigger `tg_set_updated_at`
da 0002), `videos` (idem objeto; **sem rota de escrita na 3b**, nasce agora porque DDL é
estático e a FK CASCADE vem junto das irmãs).

**Contadores (fecha ADR-0002 T2):** função única `heph_refresh_dataset_counters(uuid)` que
**recalcula** `images_count`/`labeled_count`/`size_bytes` e deriva `status`
(`needs_labeling`/`in_progress`/`ready`) a partir das tabelas-fato — nunca `+=` — disparada
por triggers em `images` e `boxes`/`captions`. Consequências: drift impossível; a ordem
trigger-usuário × cascata-RJ do `DELETE FROM images` deixa de importar (o último disparo
vê o estado final) — que é exatamente o motivo pelo qual o CHECK da 0002 foi removido;
`labeled` = imagem com ≥1 box (format `yolo_txt`) **ou** linha em `captions` (demais
formats), respeitando a taxonomia D3/0002; guarda `IS DISTINCT FROM` evita churn de
`updated_at`.

## Testes

| Camada | Infra | Prova |
|---|---|---|
| `cargo test -p api-principal` | `MockStorage` + pool `connect_lazy` | inventário das 6 rotas ≡ spec, camelCase guard, `deny_unknown_fields`, sniff/keys units (path traversal virado `-`, extensão do sniff e não do nome), 401 nas 6, 404 id não-UUID, itens `rejected`/`duplicate` |
| `scripts/test-db.sh` (estendido) | Postgres do compose + `MockStorage` | 0003 aplica; **triggers**: upload→contadores→status; `PUT boxes` `needs_labeling→in_progress→ready`; caption `''` → unlabeled; **`DELETE FROM images` de imagem rotulada não viola o invariante (o cenário exato da T2)**; DELETE dataset → CASCADE nas 4 + `delete_prefix` registrado no mock |
| `scripts/test-storage.sh` (novo) | servidor S3 do compose | `#[ignore]`: PUT length-exato, GET byte-idêntico, presigned consumido por `curl` no host, `delete_prefix` de 1500 objetos, servidor morto → 503, e **nenhum** request com `x-amz-content-sha256` iniciando com `STREAMING-` |
| `scripts/e2e-smoke.sh --datasets` | stack completo `ENGINE_MOCK=1` | create→upload→list→data→boxes→status ready→delete→prefixo vazio |

## Spike obrigatório antes de código (`3b.0`, ~150 linhas descartáveis)

Time-box meio dia, branch `spike/storage-minio`→`spike/storage-s3`, contra **o servidor
escolhido na D0**. Critérios binários:

1. `cargo check --workspace` verde com `aws-sdk-s3` sem `aws-config` (wall-clock
   registrado, sem threshold).
2. PUT de 1 KiB **e 512 MiB** ok, e o trace do servidor não mostra nenhuma requisição
   `STREAMING-*`/`aws-chunked`. Se falhar mesmo com `WhenRequired` → fallback:
   `UNSIGNED-PAYLOAD` explícito, ou trocar de crate.
3. Round-trip byte-idêntico (sha256) e `head_object.bytes` igual.
4. **Presigned GET funciona no browser** (`<img>` em página de `localhost:3000`, sem erro
   de console) e `curl -f` do host = 200. ← este é o critério que decide D0 entre
   SeaweedFS e Garage.
5. 1500 objetos sob um prefixo: `list_objects_v2` paginado + `delete_objects` limpa em
   ≤3 chamadas.
6. Boot com as 6 rotas novas em axum 0.7.9/matchit sem panic; sonda autenticada em
   `/…/data` rota para o handler (404 com envelope, não o 404 bodyless do fallback).
7. Servidor morto → `StorageError::Unavailable` em ≤5 s → handler 503.

Falha em 2/3/5 → troca de crate atrás da mesma porta (D4 invertida, registrado); falha em
4 → modo proxy vira default absoluto e a flag passa a ser opt-in futuro (D2 intacta);
falha em 6 → re-particionar paths; falha em 7 → timeout/retry da porta. Se tudo passar, o
spike morre e os testes viram o núcleo de `tests/storage_s3.rs` da 3b.4.

## Riscos

- **R1 — servidor arquivado/não corrigido:** MinIO fora por evidência (arquivo + CVE sem
  patch no OSS). Em qualquer escolha da D0: **bind loopback no compose**
  (`127.0.0.1:9000:9000`/`:9001`) — local-first significa browser na mesma máquina, o
  presigned `localhost` continua funcionando e uma LAN nunca toca o bucket — e **pin por
  digest**, nunca `:latest`.
- **R2 — default de checksum do SDK entra na trilha trailer:** mitigado por
  `WhenRequired` (D4) e provado pelo critério 2 do spike. O comportamento do **SDK Rust**
  não foi verificado em doc oficial: **NÃO-CHECKED**, por isso é critério de spike e não
  premissa. O precedente Go/S3-compatível (>16 MiB, aberto) é CHECKED.
- **R3 — presigned no browser falha (CORS em `fetch`/canvas; mixed-content sob TLS):**
  `<img src>` sem `crossOrigin` não exige CORS; a flag só liga quando o endpoint público é
  alcançável; o fallback `/data` existe sempre (D3). Fontes sobre o comportamento do
  MinIO: **SEARCH-ONLY**.
- **R4 — spool de 200 MB em disco:** tempdir local, unlink por item; falha do spool → item
  `failed`, nunca 500 parcial.
- **R5/R6 — órfãos:** compensação pode falhar (PUT ok, INSERT fail, bucket cai no DELETE) →
  órfão preso sob o prefixo de um dataset já deletado, reapável por script; crash entre
  commit do DELETE e sweep é idempotente por construção.
- **R7 — galeria lê o original inteiro por thumb** (sem pipeline de thumbs na 3b): a chave
  por pasta de imagem já reservou espaço para `thumb.webp` futuro sem migração.
- **R8 — export adiado (D9)** vs. requisito da IDEIA: resposta interim é o console +
  `mc mirror`; a fatia 3e fica nomeada em `coordenacao.md` para não evaporar.
- **R9 — `labeled` depender de `format`** (dataset `yolo_txt` com caption solto conta como
  não-rotulado): intencional, comentado no DDL, coberto por teste.

## O que fica falso nos docs quando isto for aprovado (lista para o `@docs-sync`)

`backend.md` :15 (blobs em disco → bucket), :26 (verdade canônica), :49 (formatos por
engine deixam de ser canônicos → artefatos de build), :50 (chunking só no transporte
remoto), :148 (`source` sai do banco), :160-161 (`images.path`→`object_key`, +sha256/
media_type), :165 (`videos` idem), :183 (materialização com debounce morre), :188
(`manifest.files[].path` → `{key,filename,md5,bytes}`), :195 (volumes: `minio_data`, sem
`datasets` no orquestrador, serviço novo). `frontend.md` :36 (progresso resumível →
por item), :89 (exemplos de `source`), :159 (null → derivado), :180 (3b entrega o quê;
export→3e), :217 (cleanup de diretório → sweep de prefixo). `adr/0002` :136 e T3
(`DATASETS_DIR` substituído) — com **banner** na 0002, não reescrevendo o corpo; **T10 e
D6 da 0002 permanecem válidos** (o limite dedicado de 200 MB foi quitado literalmente).
`coordenacao.md` :45-53 e :70-72/:88-89 (bloco de dívida da 3b reescrito).

## Plano de commits da 3b (produção <400 linhas cada; contrato/testes fora da conta, exceção da 0002)

| # | Commit | Prod. |
|---|---|---|
| 3b.0 | `spike(storage): matriz aws-sdk-s3 × <servidor D0>` | ~150 descartáveis |
| 3b.1 | `feat(datasets): migration 0003 — images/boxes/captions/videos + gatilhos` | ~200 |
| 3b.2 | `feat(storage): porta + mock + AppState + env de boot` | ~280 |
| 3b.3 | `feat(datasets): POST upload (spool+sniff+hash) + GET images` | ~380 |
| 3b.4 | `feat(storage): S3Storage + compose (serviço, bucket, bind loopback, digest pin) + .env.example + runner` | ~240 |
| 3b.5 | `feat(datasets): leitura — GET detail + proxy /data + url híbrida por flag` | ~200 |
| 3b.6 | `feat(datasets): PUT boxes + PUT caption + validações de anotação` | ~300 |
| 3b.7 | `feat(datasets): DELETE varre prefixo pós-commit + `source` derivado no wire` | ~130 |
| 3b.8 | `docs(adr): ADR-0003 aceita + sincroniza docs` | 0 `.rs` |

Total ~1.250 linhas de produção em 9 commits. `apps/web`, `services/manager`,
`services/orchestrator`, `engines`: **zero mudança** (D8/D9).
