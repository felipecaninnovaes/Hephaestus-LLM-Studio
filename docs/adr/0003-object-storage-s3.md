# ADR-0003 — Storage de objetos S3 como blob canônico (Fatia 3b)

- **Status:** IMPLEMENTADA na branch `feat/datasets-storage` (3b.0–3b.7:
  `f6c6ff5`, `656a474`, `c012be5`, `a21345b`, `507637e`, `94fc0db`, `8451fa0`,
  `3b6b46e`, `393163c`), landagem em **2026-09-05**. Direção + D0 = **SeaweedFS**
  aceitas pelo usuário em 2026-09-04. **Spike 3b.0 executado em 2026-09-05: 7/7
  critérios PASS** (ver "Resultados do spike 3b.0" abaixo) — D4 (crate) e D3
  (presigned) confirmadas, sem inversão; R2/R3 desriscados. Os deltas de
  `backend.md`/`frontend.md` da lista ao fim desta ADR foram aplicados no commit 3b.8
  (docs agora descrevem o que existe).
- **Data:** 2026-09-04 (aceite); implementação 2026-09-05
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

### D0 — Servidor S3 padrão: **SeaweedFS** (decisão do usuário, 2026-09-04)

Escolhido **SeaweedFS**; **Garage** registrado como plano B; **RustFS descartado como
default** (pré-1.0: `1.0.0-rc.3` em ago/2026, sem 1.0 estável, guardando a única cópia das
imagens de treino do usuário); **MinIO descartado por evidência** (arquivado + CVE sem
patch no OSS — contexto acima). O que não estava em discussão e continua sendo o ponto
que torna a escolha barata: falamos **S3 API por uma porta** (`StoragePort`, D8), então o
servidor é config, não código, e a troca posterior é um swap de endpoint+credenciais — não
um refactor.

Por que SeaweedFS ganhou: licença **Apache-2.0** (sem AGPL, e o produto é proprietário),
maturidade (desde 2012; backend default que o **Kubeflow Pipelines** adotou no lugar do
MinIO), e detalhe operacional decisivo: `weed server -s3` sobe endpoint em `:8333` **com o
bucket já pré-criado e credenciais**, o que elimina o init-container `minio-init-bucket`
que a proposta original tinha. `aws-sdk-s3` fala com ele sem mudança de código.

Fatos a honrar no spike (fontem **CHECKED** hoje, https://github.com/seaweedfs/seaweedfs):

- Presigned **SigV4 é suportado**: `weed/s3api/s3api_auth.go` define
  `isRequestPresignedSignatureV4` (`X-Amz-Credential` na query) → `authTypePresigned`.
- **Histórico de dor exatamente no nosso critério 4**: issue **#6761** — presigned s3v4
  passou a falhar com `SignatureDoesNotMatch` **atrás de reverse proxy HTTPS→HTTP**, por
  causa de `weed/s3api/auth_signature_v4.go:734` usar o scheme da requisição em vez de
  `X-Forwarded-Proto` (o host assinado virava `host:443`); relatado como resolvido pelo
  PR **#6884** (comentário de `chrislusf` fechando a issue em 2025-05-30). Consequência
  para nós: em dev/compose **não há proxy** (`localhost:8333` direto), logo o cenário não
  se aplica; mas **pin por versão explícita ≥ a correção** (nunca `:latest`/`:dev`), e ao
  ligar um TLS proxy remoto o critério 4 precisa ser reexecutado. O mesmo mecanismo de
  "host entra na assinatura" é a razão de ser do `S3_PUBLIC_ENDPOINT_URL` da D3 — a
  lição do VisionLens vale para os dois servidores.
- CORS: o filer/S3 API lê `cors.allowed_origins.values` (default `*`) e responde
  `OPTIONS` — ou seja, se um dia o browser precisar de CORS para ler pixels, é config.
- Porta S3 = **8333** (não 9000). Toda a documentação/env da 3b deve usar 8333; o
  `S3_PUBLIC_ENDPOINT_URL` de dev vira `http://localhost:8333`.


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

> **Banner pós-3e (2026-09-07):** revisado pela ADR-0006 (3e) — export/import
> implementados na 3e (`POST /:id/export`, `POST /datasets/import`, spec 0.6.0);
> `POST /:id/package` movido para a fatia 4. Corpo abaixo preservado como
> registro da 3b — não reescrevê-lo.

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
GET  /api/datasets/:id/images                  200 400 401 404
GET  /api/datasets/:id/images/:imageId         200 401 404
GET  /api/datasets/:id/images/:imageId/data    200 401 503 404
PUT  /api/datasets/:id/images/:imageId/boxes   200 400 401 404
PUT  /api/datasets/:id/images/:imageId/caption 200 400 401 404
```

> **Emenda 2026-09-05 (revisão 3b.3)**: `GET /:id/images` ganhou `400` do ferro —
> validação dos filtros `split|labeled|limit|offset` fora do domínio merece envelope
> `invalid_request` (mesma família da linha upload). Código e OpenAPI já batiam; a
> tabela é que ficou para trás. Também da revisão: o `DefaultBodyLimit` da D2 é do
> **corpo total do lote** (axum 0.7.9 embrulha a stream inteira — não existe limite
> por field), com teto **por arquivo de 200 MiB contabilizado no spool** (reason
> `too_large` por item) e margem de envelope multipart de 8 MiB no limite do request.

Schemas novos: `UploadResult{items[]}`, `UploadItem{imageId,filename,status,reason,bytes,
width,height}`, `ImagePage{items,total,limit,offset}`, `Image{...,objectKey,mediaType,
split,url}`, `ImageDetail`(=Image+`boxes[]`+`caption`), `Box`, `PutBoxesRequest/Response`,
`PutCaptionRequest`, `CaptionResponse`. `limit` default 50, máx 200; filtros `split`
(`train|val`) e `labeled`. Wire camelCase (ADR-0002 D1, guardado por teste); `id`/
`imageId` não-UUID → 404 `not_found` (ADR-0002 D8 replicado).

## Emendas da vida real (3b.0–3b.7, já no código — 2026-09-05)

- **Tabela de rotas:** `GET /:id/images` declara **400** (`PROTECTED_ROUTES` em
  `src/auth/routes.rs`: `&[200, 400, 401, 404]` — query `limit/offset/split/labeled`
  inválida responde 400 no envelope; o rascunho do contrato previa só 200/401/404).
- **Limites de corpo:** teto **TOTAL 200 MiB + 8 MiB** de envelope
  (`UPLOAD_BODY_LIMIT_BYTES` em `src/auth/routes.rs`) **e** teto **POR ARQUIVO 200 MiB**
  no spool (`MAX_FILE_BYTES` em `src/datasets/handlers.rs`; ao exceder, drena a stream
  até EOF e marca o item `rejected/too_large`).
- **NOVO — `Dataset.classes` expõe `id`** (gap do classId, 3b.7 `393163c`):
  `DatasetClassResponse{id,name,idx,color}`; a resposta de classes é canônica e o `id`
  alimenta o `classId` do PUT boxes.
- **NOVO — TTL de presign validado no boot** (`src/main.rs::load_storage`):
  `S3_URL_TTL_SECS` em `1..=604800` (máx SigV4 de 7 dias), fail-fast fora do range.
- **NOVO — sweep do DELETE é best-effort com `eprintln`** até a fatia de logging
  (`src/datasets/handlers.rs::delete`): falha do `delete_prefix` loga e não transforma
  o 204 em erro; prefixo fica reapável.

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

Time-box meio dia, branch **`spike/storage-seaweedfs`** de `main` atualizada (a 3a já está
no tronco), contra **SeaweedFS** (D0 decidida). Critérios binários:

1. `cargo check --workspace` verde com `aws-sdk-s3` sem `aws-config` (wall-clock
   registrado, sem threshold).
2. PUT de 1 KiB **e 512 MiB** ok, e o log/trace do servidor não mostra nenhuma requisição
   `STREAMING-*`/`aws-chunked`. Se falhar mesmo com `WhenRequired` → fallback:
   `UNSIGNED-PAYLOAD` explícito, ou trocar de crate (D4 invertida, registrado).
3. Round-trip byte-idêntico (sha256) e `head_object.bytes` igual.
4. **Presigned GET funciona no browser**: `<img src>` de página em `http://localhost:3000`
   apontando para `http://localhost:8333/...` **sem erro de console no Chrome**, e
   `curl -f` do host = 200. Rodar **sem proxy** (é o caso dev) e anotar que o cenário
   HTTPS→HTTP atrás de reverse proxy tem histórico de `SignatureDoesNotMatch` no
   SeaweedFS (#6761, corrigido pelo #6884) → pin de versão ≥ a correção é obrigatório.
5. 1500 objetos sob um prefixo: `list_objects_v2` paginado + `delete_objects` limpa em
   ≤3 chamadas.
6. Boot com as 6 rotas novas em axum 0.7.9/matchit sem panic; sonda autenticada em
   `/…/data` rota para o handler (404 com envelope, não o 404 bodyless do fallback).
7. Servidor morto (`docker compose stop seaweedfs`) → `put_object` mapeado a
   `StorageError::Unavailable` em ≤5 s (timeout configurado) → handler responderia 503.

Rascunho do serviço a montar no spike (porta **8333**, bucket pré-criado, bind loopback):

```yaml
  seaweedfs:
    image: chrislusf/seaweedfs:4.44_full   # PIN explícito ≥ correção do #6884; nunca :latest/:dev
    command: "server -s3 -s3.port=8333 -dir=/data -master.volumeSizeLimitMB=1024"
    environment:
      S3_BUCKET_CREATE_OPTIONS: heph-data  # pré-cria o bucket no boot
      S3_ACCESS_KEY: ${S3_ACCESS_KEY:-heph}
      S3_SECRET_KEY: ${S3_SECRET_KEY:-heph-local-dev}
    ports:
      - '127.0.0.1:8333:8333'              # S3 API (browser dev alcança via localhost)
      - '127.0.0.1:9333:9333'              # master UI (opcional, p/ inspeção humana)
    volumes:
      - seaweed_data:/data
    healthcheck:
      test: ["CMD-SHELL", "wget -q -O /dev/null http://localhost:8333/ || exit 1"]
      interval: 5s
      timeout: 5s
      retries: 12
```

**Verificar os nomes exatos de flag/env no spike** (`-s3.port`, `S3_BUCKET_CREATE_OPTIONS`,
endpoint de health, se `weed` tem `wget` na imagem) — isto é rascunho do coordenador a
partir de docs de terceiros, **NÃO-CHECKED item a item**; o spike existe justamente para
transformar isso em fato. Se alguma flag não existir, o substituto é um init-container
`weed shell`/`curl` criando o bucket (o caminho que o MinIO exigia).


Falha em 2/3/5 → troca de crate atrás da mesma porta (D4 invertida, registrado); falha em
4 → modo proxy vira default absoluto e a flag passa a ser opt-in futuro (D2 intacta);
falha em 6 → re-particionar paths; falha em 7 → timeout/retry da porta. Se tudo passar, o
spike morre e os testes viram o núcleo de `tests/storage_s3.rs` da 3b.4.

## Resultados do spike 3b.0 (executado 2026-09-05 — **7/7 PASS**)

Rodado no ramo descartável `spike/storage-seaweedfs` (NÃO mergeado; a matriz completa com a
evidência de tcpdump/CDP está em `spike/STORAGE-SPIKE.md` naquele ramo). Ambiente: imagem
**`chrislusf/seaweedfs:4.45_full`** (pin subiu de 4.44→4.45; ≥ correção do #6884),
**`aws-sdk-s3 1.145.0`** sem `aws-config`, cargo/rustc 1.97.1. **Consequência: D4 (crate) e
D3 (leitura híbrida por presigned) ficam como aprovadas — nenhum critério forçou inversão.**

- **C2/R2 desriscado no wire:** com `RequestChecksumCalculation::WhenRequired`, o PUT de 1 KiB
  **e o de 512 MiB** saíram com `content-length` exato e `x-amz-content-sha256` = SHA real do
  payload; tcpdump no loopback mostrou **zero** `aws-chunked`/`STREAMING-*`/`transfer-encoding:
  chunked`. A premissa NÃO-CHECKED do SDK Rust virou fato.
- **C3:** round-trip byte-idêntico (sha256) + `head_object.content_length` batendo.
- **C4/R3 desriscado:** `<img src>` de página em `http://localhost:3000` para o presigned em
  `http://localhost:8333` carregou num Chrome real sem proxy: request `:8333` = `200 image/png`,
  `naturalWidth>0`, **zero** erros de console/`loadingFailed`; `curl -f` do host = 200
  byte-idêntico. Cross-origin sem CORS (img sem `crossOrigin`). **Proxy não precisa ser default.**
- **C5:** 1500 objetos sob prefixo → `list_objects_v2` em 2 páginas, `delete_objects` em **2**
  chamadas (≤3). **C6:** as 6 rotas novas coexistem com as 4 core no matchit sem panic; `/…/data`
  roteia ao handler (não ao fallback bodyless). **C7:** servidor morto → `put_object` falha em
  ~2.5 ms (`RetryConfig::disabled()` + `TimeoutConfig` 5 s), mapeável a `StorageError::Unavailable`.

**Achados que corrigem o rascunho do compose da seção "Spike obrigatório"** (para a 3b.4 não
recair neles):

1. **Identidade é arquivo JSON, não env.** `S3_ACCESS_KEY`/`S3_SECRET_KEY`/
   `S3_BUCKET_CREATE_OPTIONS` do rascunho **não existem** no SeaweedFS → `403 AccessDenied` em
   tudo. O caminho real é a flag **`-s3.config=<path>`** com
   `{"identities":[{"name":..,"credentials":[{"accessKey":..,"secretKey":..}],
   "actions":["Admin","Read","Write","List","Tagging","UserManagement"]}]}`. O aviso de boot
   `no signing key found for STS` é **irrelevante** (não usamos STS).
2. **Bucket auto-cria no 1º PUT** de identidade com ação `Admin` (`-s3.autoCreateBucket` default
   `true`) → **sem init-container** (mais forte que o D0 previa). Basta o `heph-data` existir via
   primeiro PUT, ou criar de forma idempotente no runner.
3. **Healthcheck exige `-ip.bind=0.0.0.0`.** Default do `weed` amarra o S3 só ao IP da interface
   (não ao loopback), então `wget localhost:8333` do healthcheck falha mesmo servindo 200 do host.
   Adotar `-ip.bind=0.0.0.0` + healthcheck em `127.0.0.1`; bind externo local-first vem do
   mapeamento `127.0.0.1:8333:8333` (R1 preservado).
4. **API do SDK sem `aws-config` (nomes reais p/ a 3b.4):** `Credentials::new(ak,sk,None,None,..)`
   (não `from_keys`, que é feature-gated) vai direto em `.credentials_provider` (Credentials já
   implementa `ProvideCredentials`; não há `StaticCredentialsProvider`). `RetryConfig`/
   `TimeoutConfig` vêm de `aws_smithy_types`; o setter é `.timeout_config(..)` (não
   `runtime_config`). Presign é `get_object()….presigned(PresigningConfig::expires_in(d)?)` →
   `.uri()` (não `client.presigner()`). Outputs do SDK têm campos privados → usar getters
   (`content_length()`, `contents()`, `next_continuation_token()`).
5. **Novo risco R10 — build do `aws-lc-sys` no Docker:** `aws-sdk-s3` → `rustls 0.23` → provedor
   default `aws-lc-rs` → **`aws-lc-sys`**, cuja `build.rs` compila C/asm (lento: rebuild isolado
   >10 min no host; buildou cc-only **sem** cmake aqui). O `Dockerfile` usa `rust:1.97.1-slim`:
   **confirmar na 3b.4** o link nesse image; se pedir cmake/nasm, `apt-get install -y cmake make`
   no estágio `build` (ou fixar provedor `ring` nas features do SDK). Registrar p/ não virar
   surpresa de build.

## Riscos

- **R1 — servidor sem manutenção:** MinIO fora por evidência (arquivado 25/04/2026 + CVE
  sem patch no OSS). Para o SeaweedFS escolhido, o risco simétrico é o outro extremo: o
  projeto **lança ~36 releases por ano** (4.43 e 4.44 com um dia de diferença em ago/2026),
  então é rapidíssimo a corrigir e ao mesmo tempo alvo móvel → **pin de versão explícita**
  (`:4.44_full`, nunca `:latest`/`:dev`), ler o changelog antes de subir, e **bind loopback
  no compose** (`127.0.0.1:8333:8333`, `127.0.0.1:9333:9333`) — local-first significa
  browser na mesma máquina, o presigned `localhost` continua funcionando, e uma LAN nunca
  toca no bucket.
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

## O que fica falso nos docs quando a 3b landar (lista pronta para o `@docs-sync`)

**Não aplicar agora** — os docs têm de descrever o que existe, não o que foi aprovado. Esta
lista entra no commit `3b.8`.

`backend.md` :15 (blobs em disco → bucket), :26 (verdade canônica), :49 (formatos por
engine deixam de ser canônicos → artefatos de build), :50 (chunking só no transporte
remoto), :148 (`source` sai do banco), :160-161 (`images.path`→`object_key`, +sha256/
media_type), :165 (`videos` idem), :183 (materialização com debounce morre), :188
(`manifest.files[].path` → `{key,filename,md5,bytes}`), :195 (volumes: `seaweed_data`, sem
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
