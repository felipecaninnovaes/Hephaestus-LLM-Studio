# ADR-0006 — Export/Import de dataset (backup estruturado) (Fatia 3e)

- **Status:** **ACEITA** pelo usuário (2026-09-07, após a emenda da substituição
  consentida). Nada implementado; este documento é o passo **3e.0** do
  `docs/plano-3e-export-import.md` §5 e agora é a **especificação executável** da fatia;
  os deltas de contrato abaixo são aplicados nos commits 3e.1/3e.2
  (openapi junto do código) e 3e.5 (docs de texto), nunca antes.
- **Data:** 2026-09-07
- **Emenda 2026-09-07 (auditoria do usuário):** P3 revisada — o 409 vira protocolo de
  substituição consentida (campo `replace`); ver "Decisões fechadas pelo usuário" e
  D3/D5/D6/R10. O usuário **ACEITOU** a ADR emendada (2026-09-07).
- **Componentes:** `services/api-principal` (`src/datasets/{export,import}.rs` novos,
  `src/datasets/handlers.rs`, `src/datasets/models.rs`, `src/storage/{port,mock,s3}.rs`,
  `src/auth/routes.rs`), `packages/contracts` (spec 0.5.0 → **0.6.0**), `apps/web`
  (galeria, `DatasetMenu`, `lib/backup.ts` novo). Postgres: **nenhuma migration** (P6).
- **Fontes:** `IDEIA.md` §1 (backup estruturado na galeria); `docs/PRODUCT.md` :28,41;
  `docs/backend.md` §9/:131-133, §10/:248-249, §11/:254; `docs/frontend.md` §5.2/:96,
  §10/:188, §13/:226; `docs/adr/0003-object-storage-s3.md` D1/D2/D6/D7/D9/D10;
  `docs/adr/0005-datasets-gestao-amostras-classes.md` D3/D9; `docs/plano-3e-export-import.md`
  (brief completo); código: `src/auth/routes.rs` (`UPLOAD_BODY_LIMIT_BYTES` + envelope),
  `src/datasets/handlers.rs::upload` (spool+sniff+hash, ordem objeto→linha→compensação),
  `src/storage/sniff.rs`, `src/storage/keys.rs`, `src/storage/port.rs`.
- **Sequência:** 3d → 3g → 3f → **3e** → 4 (3e é a próxima fatia; 3f/3g já mergeadas).

## Contexto

A IDEIA pede "Exportar o dataset e importar (uma forma de backup estruturado)" como
funcionalidade da galeria. A ADR-0003 D9 adiou `POST /:id/export`, `POST /datasets/import`
e `POST /:id/package` da 3b para a 3e; o schema já está pronto para isso —
`boxes.origin`/`captions.origin` têm CHECK `manual|autotracker|import` desde a migration
0003 (o valor `import` existe e nunca foi usado), e toda a infra de ingest da 3b
(spool tempfile, sniff por magic bytes, `put_object` de length exato, ordem
objeto→linha→compensação, sweep de prefixo pós-commit, limite de corpo multipart
dedicado em `src/auth/routes.rs`) é reutilizável. Doutrina fixa (backend.md :249): a
materialização YOLO nasce **do banco em tempdir** ("O package sempre gera do banco"),
nunca de disco/bucket. Entre principal e cliente não há chunking (ADR-0003 D9).

## Decisões fechadas pelo usuário (2026-09-07)

Resposta literal do usuário às perguntas do plano §7: **"tudo como você recomenda"**.
Registradas como fechadas — não re-abrir:
Emenda (2026-09-07, auditoria da proposta): o usuário REJEITOU o P3 original (409-seco
"renomeie antes") e decidiu a semântica de substituição consentida (P3 abaixo, reescrito).
Nada mais foi impugnado — P1/P2/P4/P5/P6 e as decisões extras (title, dedupe→400
`import_invalid`, manifest como fonte da verdade, `origin` preservado literal) permanecem.

- **P1 — `POST /:id/package` sai da 3e, vai para a fatia 4.** O consumidor (orquestrador
  + transporte remoto 8 MB) só nasce lá; incluir manifest+md5 para orquestrador na 3e
  seria código sem consumidor. A D0 abaixo registra a revisão consciente do ADR-0003 D9.
- **P2 — Entrada de Importar na UI: SÓ na galeria.** O botão "Importar" desabilitado do
  header de `/datasets` (frontend.md :86) é **removido**; importar recria dataset novo,
  e o fluxo é galeria→importar→abrir o dataset importado.
- **P3 — Conflito de nome no import: protocolo de substituição consentida em dois passos
  (REVISTA em 2026-09-07 — o usuário auditou a proposta e rejeitou o 409-seco "renomeie
  antes").** Decisão literal do usuário: "Se o dataset que for importado já existir, pode
  ser duas coisas: o usuário clicou errado ou é uma atualização do dataset — então deve
  ser substituído, não duplicado, como uma caixa de mensagem informando que a ação não tem
  reversão." Formalização: 409 `slug_conflict` **continua como detecção** — o servidor
  NUNCA substitui por conta própria; `POST /api/datasets/import` ganha campo multipart
  **`replace`** (booleano, default false). Fluxo: (1) sem `replace` + slug existente →
  409; (2) a UI mostra caixa de confirmação de irreversibilidade e re-envia com
  `replace=true` → substituição (teardown + ingest, 201). Substituição = **teardown +
  ingest com dataset_id novo** (não reconciliação in-place); **validação completa do
  pacote ANTES de qualquer teardown** (zip corrompido não destrói o existente);
  `replace=true` com slug inexistente importa normalmente (o consentimento é para o caso
  de existir). O campo `title` PERMANECE (importar como dataset novo com outro nome segue
  sendo caso de uso; o clique-errado cancela, a atualização confirma).
- **P4 — Limite do zip de import: 200 MiB + 8 MiB de envelope** (mesmo padrão do upload
  3b, `UPLOAD_BODY_LIMIT_BYTES` em `src/auth/routes.rs`).
- **P5 — Export inclui somente imagens ativas** (`deleted_at IS NULL`); lixeira não vai
  no backup.
- **P6 — Sem migration.** O schema já suporta o roundtrip (`origin` inclui `import` desde
  a 0003). Se no desenho/implementação surgir necessidade inesperada de DDL, PARAR e
  reportar — não inventar migration.

## Decisões

### D0 — Escopo da fatia: **export + import apenas**; `POST /:id/package` → fatia 4 (revisão consciente do ADR-0003 D9)

A 3e entrega `POST /:id/export` e `POST /datasets/import` (contrato 0.6.0). O
`POST /:id/package` (zip manifest+md5 para o orquestrador) fica na fatia 4, onde nasce o
consumidor real. A ADR-0003 D9 lista o package na 3e — esta ADR **revisa** essa parte:
só o export/import fica na 3e. A fatia 4 também absorve o `dataset_versions` (T4 das
dívidas) — a 3e NÃO cria snapshot de versão (import cria dataset novo; versão é conceito
de treino/materialização, não de backup). O manifest §11 de backend.md (:254, transporte
orquestrador, `{dataset_id, slug, category, engine, files, md5_zip, bytes, chunks}`) é
artefato da fatia 4 e **não** se confunde com o `manifest.json` de backup da 3e (D1).

**Descartado:** fazer o package na 3e "de carona" (código sem consumidor; o plano §4 D9
recomendou mover e o usuário aceitou — P1).

### D1 — Layout do ZIP: `manifest.json` é a **fonte da verdade do roundtrip**; `dataset.yaml`/`labels/*.txt`/`captions.jsonl` são **materializações derivadas**

Proposta do plano §4 D1 aceita com uma decisão que a fecha: **as boxes COMPLETAS vivem
no `manifest.json`** (array `boxes[]` por imagem com `origin`/`conf`/`track_id`), e o
`labels/*.txt` é **só materialização YOLO para consumo externo** (formato YOLO perde
origin/conf/track_id por construção — 6 decimais, sem conf). A fonte da verdade do
roundtrip é o manifest; o import lê manifest + `images/*` e **ignora** os derivados
(edições manuais em `labels/*.txt`/`captions.jsonl`/`dataset.yaml` dentro do zip NÃO
sobrevivem a um re-import — comportamento documentado, ver R8).

Estrutura do pacote (determinística — o import é o espelho):

```
<slug>.zip
├── manifest.json      # snake_case (artefato de transporte — ADR-0002 D1); FONTE DA VERDADE
├── dataset.yaml       # DERIVADO: estilo YOLO (path/train/val lists/names); só format yolo_txt
├── labels/<stem>.txt  # DERIVADO: uma linha `cls cx cy w h` por box (6 decimais); por imagem com ≥1 box
├── captions.jsonl     # DERIVADO: `{"image": "<filename>", "caption": "..."}` por linha; por imagem com caption
└── images/<filename>  # binários; <filename> = nome canônico do banco (stem sanitizado + ext do sniff)
```

Regras de conteúdo:

- **`manifest.json`** (schema_version 1): carrega `dataset{name(slug), title, type,
  format, counts{images,labeled,classes,size_bytes}}`, `classes[{idx,name,color}]`,
  `images[{filename,split,width,height,bytes,sha256,media_type,boxes[],caption?}]`.
  `boxes[]` = `{class_idx,x,y,w,h,conf?,origin,track_id?}` com **origem original
  preservada** (`manual`|`autotracker`) — ver D6. `caption` = `{text,origin,model?}`.
  `class_idx` referencia a posição em `classes` (índices são imutáveis e estáveis entre
  export/import; ids UUID não sobrevivem roundtrip — o import gera ids novos).
  `counts` é informativo (os contadores são recalculados por trigger no import).
- **`dataset.yaml`** só para `format == yolo_txt`: `path: .`, `train:`/`val:` como
  **listas de caminhos** `images/<filename>` relativos ao yaml (split por imagem vem do
  manifest), `names: {idx: name}`. Formato válido para o ultralytics (resolução de
  labels por `img2label_paths` — `images/`→`labels/`).
- **`labels/<stem>.txt`**: por imagem com ≥1 box, independente do format (é o único
  formato de box existente). `stem` = filename sem extensão.
- **`captions.jsonl`**: por imagem com caption, independente do format. Exemplo da casa
  (frontend.md §7.2): `{"image": "img_0042.jpg", "caption": "..."}`.
- `images/<filename>`: nome canônico do banco; extensão veio do sniff no upload e é
  revalidada no import (D4).
- Export é byte-determinístico por construção exceto `exported_at` (RFC 3339) — a
  estrutura é o que importa, não o byte a byte (o teste de fidelidade compara por API).

Exemplo concreto (`inspecao-pcb-defeitos-v2`, yolo_bbox, 2 imagens, 1 classe):

```json
{
  "schema_version": 1,
  "exported_at": "2026-09-07T12:00:00Z",
  "dataset": {
    "name": "inspecao-pcb-defeitos-v2", "title": "Inspeção PCB v2",
    "type": "yolo_bbox", "format": "yolo_txt",
    "counts": { "images": 2, "labeled": 1, "classes": 1, "size_bytes": 48213 }
  },
  "classes": [ { "idx": 0, "name": "solda_fria", "color": "#10b981" } ],
  "images": [
    {
      "filename": "img_0001.jpg", "split": "train", "width": 1280, "height": 720,
      "bytes": 31234, "sha256": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
      "media_type": "jpeg",
      "boxes": [ { "class_idx": 0, "x": 0.5, "y": 0.5, "w": 0.2, "h": 0.3, "conf": 0.96, "origin": "autotracker", "track_id": 7 } ],
      "caption": null
    },
    { "filename": "img_0002.jpg", "split": "val", "width": 1280, "height": 720,
      "bytes": 16979, "sha256": "1b16b1df538c12b4a8c1d7b2e9f4a5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b",
      "media_type": "jpeg", "boxes": [], "caption": null }
  ]
}
```

```yaml
# dataset.yaml (derivado)
path: .
train: [images/img_0001.jpg]
val: [images/img_0002.jpg]
names:
  0: solda_fria
```

```text
# labels/img_0001.txt (derivado; 6 decimais; sem conf — YOLO txt não carrega)
0 0.500000 0.500000 0.200000 0.300000
```

**Descartado:** boxes completas só em `labels/*.txt` (perde origin/conf/trackId — o plano
exige fidelidade deles); espelhar `labels` como objetos no bucket (duas verdades —
ADR-0003 D6); layout com árvores `images/train/`+`images/val/` duplicando o conceito de
split (o manifest já carrega split por imagem; a lista do yaml resolve o consumo YOLO).

### D2 — Transporte do export: spool tempdir → **`zip` sync em `spawn_blocking`** → stream do arquivo

`POST /api/datasets/:id/export` responde **o zip em stream** (`Content-Type:
application/zip`, `Content-Disposition: attachment; filename="{slug}.zip"` — slug é
kebab-case ASCII por construção, seguro no header), `Content-Length` do arquivo spoolado.
O zip em si **não toca o bucket** (evita re-upload + presigned de algo que o principal
já tem em mãos). Pipeline em 3 fases dentro do handler:

1. **Fase A (async, sem zip):** `tempfile::TempDir` criado; lê do banco dataset +
   classes + imagens ativas + boxes + captions; `get_to_file(object_key)` de cada imagem
   para o tempdir (novo método da `StoragePort` — espelho de `put`, ver D9); escreve
   `manifest.json`/`dataset.yaml`/`labels/*.txt`/`captions.jsonl` como arquivos no
   tempdir. Bytes das imagens = `get_object` por `object_key`.
2. **Fase B (`tokio::task::spawn_blocking`):** `zip::ZipWriter` sync sobre
   `std::fs::File` no tempdir; `Stored` para imagens (jpeg/png/webp já são comprimidos —
   Deflate desperdiça CPU e pode até inflar) e `Deflated` para os artefatos de texto.
   Saída = arquivo zip no mesmo tempdir.
3. **Fase C:** `ReaderStream` do arquivo → body; tempdir cai no `drop` (disco só
   efêmero, D1 — mesmo padrão do spool da 3b, nunca stream de tamanho desconhecido).

Crate: **`zip` 2.x (sync)** — API estável e madura, suporta Zip64 (sem teto de 4 GiB);
`async_zip` não traz benefício aqui porque o zip é **inteiramente spoolado antes da
resposta** (não há streaming incremental do zip para o cliente durante a montagem), e o
bloqueio de CPU/IO sync é o caso canônico de `spawn_blocking`. Fase A e B separadas
porque o handler é async (pool + storage) e o zip é sync: coletar dados (async) →
zipar (blocking). Memória constante; disco ~2× o tamanho do dataset no pico (spools de
imagem + zip) — aceitável single-user local (R6).

Comportamentos: dataset inexistente/id não-UUID → 404 `not_found` (D8); bucket fora →
503 `storage_unavailable`; imagem com linha mas **sem objeto** (estado ruim aceitável da
3b) → **skip + `eprintln`**, export segue com as demais (falhar o backup inteiro por um
órfão derrota o propósito; os `counts` do manifest refletem o que foi exportado — R4);
dataset vazio → 200 com zip só-manifest (`counts.images=0` — espelha o create de dataset
vazio).

**Descartado:** `get(key) -> Vec<u8>` em RAM por imagem (200 MiB por imagem na heap —
`get_to_file` mantém a memória constante); `async_zip` (maturidade menor, sem ganho real
com spool completo); stream-on-the-fly bucket→zip sem spool (exigiria streaming na porta
de storage e quebra o "o package sempre gera do banco em tempdir").

### D3 — Transporte do import: multipart `file` + **`title`** + **`replace`**; validação completa **antes** de qualquer write/teardown

`POST /api/datasets/import` multipart com campos **`file`** (obrigatório, binário `.zip`),
**`title`** (opcional, ≤96 chars — override do nome para importar como dataset novo) e
**`replace`** (booleano, default `false` — consentimento para SUBSTITUIR o dataset de
mesmo slug; protocolo P3/D5). `replace` é form field booleano no wire (string
`"true"`/`"false"`, ausente = `false`; valor inválido → 400 `invalid_request`). O campo
`title` é decisão desta ADR (não estava no plano): sem ele, o P3 original (409-seco)
seria beco sem saída — não existe rota de renomear dataset (ADR-0005 D9 deixou fora);
com a revisão do P3, ele permanece como o caminho "importar como dataset novo com outro
nome". Fluxo do handler (servidor **SEM estado** entre requests — os dois passos do
protocolo de substituição são dirigidos pelo cliente, que guarda o arquivo e re-envia):

1. **Spool** do `file` em tempfile com teto 200 MiB (excesso → 413 `invalid_request`
   no envelope, mesma família do upload); corpo total limitado por
   `IMPORT_BODY_LIMIT_BYTES = 200 MiB + 8 MiB` de envelope **dedicado na rota nova** em
   `src/auth/routes.rs` (mesmo padrão da 3b — o limite mora na camada de roteamento,
   nunca no handler; P4). `title` fora do range → 400 `invalid_request`; vazio/ausente →
   usa o `title` do manifest. `replace` ausente → `false`; valor inválido → 400
   `invalid_request`.
2. **Validação completa do pacote ANTES de qualquer write/teardown** (D3 do plano):
   pré-scan do central directory (D4) → extrai `manifest.json` → valida manifest
   (schema_version, domínios, referências) → extrai e valida **cada** imagem
   (sniff, hash, dimensões, limites) → deriva `slug = slugify(title ?? manifest.title)` →
   consulta `EXISTS(slug)`. Nada vai ao banco nem ao bucket com pacote inválido.
3. **Decisão de fluxo pelo estado do slug** (D5/D6): slug existe + `replace` ausente/
   false → 409 `slug_conflict` (o pacote JÁ foi validado — o 409 é detecção, não beco
   sem saída); slug existe + `replace=true` → **teardown + ingest** (D6); slug inexistente
   → ingest normal (D6; `replace=true` com slug inexistente importa normalmente — o
   consentimento é para o caso de existir).

**Descartado:** import em dois passos com preview server-side / estado no servidor
(segundo endpoint + estado; o servidor não guarda nada do 409 — o protocolo de
substituição é dirigido pelo cliente, que guarda o arquivo e re-envia com `replace=true`;
backup é raro e single-user); sem `title` (beco sem saída do P3 original, acima);
`replace` default `true` (substituição nunca é default — consentimento explícito, P3).

### D4 — Segurança do ingest (zip-slip, zip bomb, sniff, sanitize, dedupe)

- **Zip-slip:** entradas com caminho contendo `..`, barra inicial, `\` ou não-relativo →
  400 `import_invalid`. Permitidas só as raízes `manifest.json`, `dataset.yaml`,
  `captions.jsonl`, `labels/`, `images/`; `__MACOSX/` e `.DS_Store` são **ignoradas**
  (lixo comum de mac); qualquer outra entrada → 400 `import_invalid` (layout
  determinístico, "import é espelho").
- **Zip bomb:** pré-scan do central directory (soma dos `size()` declarados) com teto
  total de descompressão de **8 GiB** e teto de **100.000 entradas**; durante a extração,
  reader de cada entrada embrulhado com `take()` — teto por entrada de **200 MiB**
  (espelho de `MAX_FILE_BYTES`) para imagens e 10 MiB para artefatos de texto, mais
  contador global de bytes descomprimidos com o mesmo teto de 8 GiB (declarações mentem;
  o reader contado é a defesa real). Excedeu → aborta → 400 `import_invalid`.
- **Sniff de magic bytes de CADA imagem** (reuso de `src/storage/sniff.rs`): `media_type`
  derivado do conteúdo e **exigido igual** ao `manifest.media_type` → senão 400
  `import_invalid`. Extensão canônica vem do sniff, nunca do nome da entrada
  (doutrina 3b, `keys::canonical_filename`).
- **Filename sanitizado** igual ao upload: nome final = `canonical_filename` (stem
  sanitizado + ext do sniff); o `manifest.filename` serve só para casar a entrada
  `images/<filename>` com o índice do manifest (entrada sem correspondência no manifest,
  ou manifest apontando entrada ausente → 400 `import_invalid`).
- **Dedupe de filename** DENTRO do zip: sobre o **canônico pós-sniff** (semântica do
  upload — dois nomes podem colidir após o sniff: `a.jpg` com conteúdo PNG e `a.png`
  com conteúdo PNG → ambos `a.png`) → 400 `import_invalid` (ver D5).
- **Integridade:** `sha256` de cada imagem conferido contra o `manifest.sha256` → 400
  `import_invalid` (o hash já é calculado para o INSERT — a verificação é de graça;
  pega zip corrompido/adulterado no download).

**Descartado:** confiar no `manifest.filename` como nome final (extensão viria de fonte
não-confiável — viola D2/D5 da 3b); sem pré-scan de bomb (a defesa real é o reader
contado; o pré-scan é o filtro barato).

### D5 — Conflito e substituição: 409 como **protocolo de detecção**; `replace=true` → teardown + ingest (201); dedupe intra-zip ⇒ 400 `import_invalid`

Refutação parcial da proposta do plano §4 D5: o plano dizia "dedupe de filename dentro
do próprio zip → erro `duplicate` (semântica do upload)". A **detecção** segue a
semântica do upload (dedupe por filename canônico — D4), mas o código HTTP `duplicate`
**não existe**: o import é **atômico** (ou cria o dataset inteiro, ou nada), não tem
vocabulário por-item como o upload. Filename duplicado dentro de um único zip = pacote
malformado → **400 `import_invalid`**.

O conflito de slug (dataset com o mesmo slug já existente) deixa de ser beco sem saída e
vira **protocolo de substituição consentida em dois passos** (P3 revisto):

1. **Detecção:** o servidor deriva `slug` (a validação completa já foi feita — D3) e
   consulta `EXISTS(slug)`; se existe e `replace` não é `true` → **409 `slug_conflict`**
   (erro existente, mensagem estática "dataset slug already exists" — sem detalhe no
   envelope, política de casa; a UI decide o copy de confirmação por `code`, nunca por
   `message`). O servidor **NUNCA substitui por conta própria** — sem `replace=true`
   explícito, o 409 é a única resposta possível.
2. **Consentimento:** a UI mostra a caixa de mensagem de irreversibilidade ("substituir
   apaga o dataset atual e recria a partir do backup; a ação não tem reversão") e
   re-envia com `replace=true`.
3. **Substituição:** teardown + ingest com **dataset_id novo** (D6) — não reconciliação
   in-place; **validação completa do pacote ANTES de qualquer teardown** (um zip
   corrompido retorna 400 e o dataset existente permanece intacto); sucesso → 201 com o
   novo `Dataset`. `replace=true` com slug inexistente importa normalmente (o
   consentimento é para o caso de existir).

**Descartado:** auto-sufixo de slug (P3 — backup não é cópia silenciosa; o usuário
decide entre renomear via `title` ou substituir via `replace`); `duplicate` como código
novo de 400/200 (import é atômico, ver acima); 422 (409 é o conflito honesto — padrão
ADR-0005 D8); substituição automática sem consentimento (decisão do usuário — o servidor
nunca decide por si); reconciliação in-place com ids preservados (teardown + ingest é o
domínio do delete já provado — D6 — e o roundtrip gera ids novos de qualquer forma).

### D6 — Ordem de escrita do import: **teardown condicional** + ingest (espelho da D6 da 3b); falha fatal → delete row + sweep

Ordem obrigatória: **validação completa do pacote (D3/D4) → teardown (só quando
`replace=true`) → ingest** — um zip corrompido nunca destrói o dataset existente.

1. **Criação da linha nova + classes** (uma transação; precisa do id): `title` resolvido,
   `slug = slugify(title)`, `type` do manifest (slug vazio → 400 `import_invalid`);
   classes ordenadas por `idx` do manifest, cores rederivadas da paleta por `idx` — nunca
   do cliente (ADR-0005 D2; `UNIQUE(dataset_id, idx)` garantido pela ordem).
   **No caminho de substituição (D5), esta transação funde o teardown do antigo**:
   `DELETE` da linha do dataset antigo (`RETURNING id`; CASCADE derruba
   `images`/`boxes`/`captions`/`classes`/`image_embeddings` — os dois FKs da 0004 são
   `ON DELETE CASCADE`) + INSERT da linha nova com o MESMO slug (o UNIQUE não conflita:
   o DELETE libera o slug dentro da mesma transação) + classes. Commit. Se o INSERT novo
   falhar (ex.: corrida no slug), o rollback devolve o dataset antigo **intacto** — a
   janela destrutiva começa só no commit. Pós-commit, no caminho de substituição: **sweep
   do prefixo antigo** `datasets/{old_id}/` best-effort (padrão do `DELETE /:id` — commit
   CASCADE + `delete_prefix`; falha loga `eprintln` e o prefixo fica reapável, ADR-0003
   D7). O banco transita old→new atomicamente — nunca existe a janela "nenhum dataset com
   o slug" (R10).
2. **Por imagem** (ordem do manifest): `put_object(key, spool)` (key = chave canônica com
   `image_id` novo) → INSERT em `images` (filename canônico, sha256/md5 do spool,
   dimensões, `split` do manifest) → INSERT das `boxes` (map `class_idx` → id da classe
   nova; **origin do manifest preservado literal** — `manual`/`autotracker`/`import` como
   veio, validado contra o CHECK) → upsert do `caption` (text/origin/model do manifest).
   Compensação por imagem (espelho D7 da 3b): falha no INSERT → DELETE best-effort do
   objeto recém-postado + rollback da linha (CASCADE de boxes/captions); falha dura →
   500. O estado ruim aceitável continua sendo **objeto sem linha**; nunca linha sem
   objeto.
3. **Falha fatal** (ex.: bucket cai no meio): 503 `storage_unavailable` + **delete da
   linha do dataset NOVO** (CASCADE remove imagens/boxes/captions/classes) + **sweep do
   prefixo** `datasets/{new_id}/` pós-commit best-effort (ListObjectsV2 paginado +
   DeleteObjects de 1000 — delete-after-commit/sweep-after-commit, ADR-0003 D7). No
   caminho de substituição o dataset antigo já foi: estado de falha = **dataset ausente +
   zip intacto + possível resíduo de objeto nos prefixos antigo/novo** (R10). O import é
   all-or-nothing por compensação, sem transação longa aberta durante N PUTs.
4. Sucesso → 201 com o dataset criado + **indexação fire-and-forget** (espelho do upload:
   `index_dataset_images` spawnado, best-effort com `eprintln` — o embedder é mock em
   dev, `ENGINE_MOCK=1`; sem GPU, CPU-only). No caminho de substituição, as embeddings
   antigas morreram no CASCADE do passo 1 e o re-import re-indexa do zero.

**Descartado:** transação única abrangendo todos os PUTs (transação longa aberta durante
N chamadas ao bucket — anti-padrão; a compensação por imagem é o padrão 3b provado);
escrever boxes com `origin='import'` forçado (perderia a origem real e falsificaria o
`autoTracked` derivado — a coluna `import` existe para outros consumidores futuros, não
para apagar a história); sweep do prefixo antigo ANTES do commit do teardown (violaria o
delete-after-commit — apagaria objetos ainda referenciados se a transação fizesse
rollback); teardown em transação separada do INSERT novo (criaria a janela "nenhum
dataset com o slug" no banco — DELETE+INSERT fundidos a eliminam, R10).

### D7 — Erros novos e spec: `import_invalid` (400); spec 0.5.0 → **0.6.0**

Um código novo: **`import_invalid`** (400, mensagem estática "invalid import package") —
manifest ausente/incompatível/corrompido, zip malformado, zip-slip, zip bomb, dedupe,
sha256/media_type divergentes, domínios inválidos (D3/D4/D5). Reuso: `slug_conflict`
(409), `storage_unavailable` (503), `not_found` (404), `invalid_request` (400 de form:
sem campo `file`, `title` inválido), `unauthorized` (401), `internal` (500 global), 413
global. Spec 0.5.0 → 0.6.0 (regra "versão = ordem de landing", ADR-0005 D1). Rotas novas
entram em `PROTECTED_ROUTES` com os status exatos da tabela do delta.

**Descartado:** 422 para erros de domínio do manifest (a casa usa 400 `invalid_request`
para validação; `import_invalid` é o 400 especializado); código novo para export
(404/503 existentes cobrem).

### D8 — Export só imagens ativas (P5)

Queries de export filtram `deleted_at IS NULL` (imagens, boxes via JOIN, captions). A
lixeira é estado de trabalho, não de backup; o import cria linhas novas (nunca
`deleted_at`). `counts.images` = ativas; `dataset` re-importado nasce com
`trashCount=0` (derivado). **Descartado:** incluir lixeira no backup (P5 — restauraria
sujeira; o GC da lixeira é dívida da 3g/4, não desta fatia).

### D9 — Dependências e boundary: `zip` 2.x, `tokio-util` (io), `serde_yaml` promovido; `get_to_file` na `StoragePort`; **só o principal** fala S3/Postgres

- Cargo: `zip = "2"` (novo), `tokio-util = { version = "0.7", features = ["io"] }`
  (novo — `ReaderStream` da Fase C; dep direto, nunca transitivo), `serde_yaml = "0.9"`
  **promovido** de dev-deps para deps (geração do `dataset.yaml` no export).
- `StoragePort` ganha **`get_to_file(&self, key: &str, path: &Path) ->
  Result<(), StorageError>`** — espelho de `put`, streama bucket→disco sem RAM (S3:
  `get_object().body.into_async_read()` → copy para arquivo; mock: bytes da HashMap →
  arquivo). Sem novo env, sem compose, sem migration.
- Boundary: export/import vivem **só no principal** (único com cliente S3 e Postgres —
  ADR-0003 D8); manager/orquestrador/engines: **zero mudança**; `ENGINE_MOCK=1` dev
  CPU-only intocado (roundtrip roda sobre `MockStorage` em RAM, sem GPU).
- Módulos novos `src/datasets/export.rs` e `src/datasets/import.rs` (handlers.rs já tem
  1743 linhas); validações puras do manifest em `models.rs` (unit-testáveis sem banco,
  padrão da casa).
- Handlers novos nascem com `eprintln` mínimo honesto (padrão dos existentes — a fatia
  de logging não é antecipada).

**Descartado:** `async_zip` (D2); crate compartilhado `heph-storage` (ADR-0003 D8 —
abstração sem segundo consumidor); método `get` em RAM no export (D2).

## Formato do pacote (D1) — resumo de contrato do arquivo

Ver D1. Pontos que o teste de fidelidade do roundtrip (R3) precisa garantir: `origin`,
`conf`, `track_id` das boxes, `origin`/`model` do caption, `split` por imagem, `idx`/
`name` das classes, counts — **idênticos por API após re-import** (não a olho).

## Contrato desta fase (delta OpenAPI — 0.6.0)

Rotas (entram em `PROTECTED_ROUTES` com estes status exatos; 413/405/500 seguem na
convenção global, como hoje):

```
POST /api/datasets/:id/export   200 401 404 503
POST /api/datasets/import       201 400 401 409 503
```

Detalhes do wire (camelCase global — ADR-0002 D1; `deny_unknown_fields` não se aplica ao
manifest — ver nota):

- **`POST /api/datasets/:id/export`** — sem body/query; 200 = `application/zip` binário
  com `Content-Disposition: attachment; filename="{slug}.zip"` e `Content-Length`;
  404 `not_found` (id não-UUID/inexistente); 503 `storage_unavailable` (bucket fora).
  `id` não-UUID → 404 (D8).
- **`POST /api/datasets/import`** — multipart `file` (obrigatório, binário) + `title`
  (opcional, ≤96, override do nome — importar como dataset novo) + `replace` (opcional,
  booleano no form: string `"true"`/`"false"`, ausente = `false`, valor inválido →
  400 `invalid_request`; consentimento para SUBSTITUIR o dataset de mesmo slug — D5);
  201 = `Dataset` (schema existente — `DatasetResponse` com
  `imagesCount/labeledCount/classes` recalculados por trigger; nenhum schema novo de
  resposta); 400 `invalid_request` (form: sem `file`, `title`/`replace` inválidos) |
  `import_invalid` (pacote); **409 `slug_conflict` = protocolo de detecção da
  substituição** (P3/D5: slug existente + `replace` ausente/false; a UI confirma a
  irreversibilidade e re-envia com `replace=true`; o servidor nunca substitui por conta
  própria); 503 `storage_unavailable`; 413 global (corpo > 200 MiB + 8 MiB envelope, P4).
- **Erro novo:** `import_invalid` entra no enum de `Error.code` (descrição atualizada no
  spec: "na 0.6.0, fatia 3e: `import_invalid`").
- **Nota de contrato:** o `manifest.json` é artefato de transporte (snake_case, fora do
  `/api/*` — a fronteira HTTP não contamina o transporte, ADR-0002 D1); o manifest
  **permite campos desconhecidos** (forward-compat de arquivo de backup — o gate é o
  `schema_version`, não `deny_unknown_fields`; divergência consciente da casa, ver D1).

## Testes

| Camada | Infra | Prova |
|---|---|---|
| `cargo test -p api-principal` | `MockStorage` + pool lazy | units puras do manifest (build export / validate import: domínios, class_idx, sha256, dedupe pós-sniff, origem preservada); fixtures de zip malicioso (slip `../`, bomb declarada vs reader contado, entrada desconhecida, `__MACOSX/` ignorado, filename duplicado pós-canônico); inventário de rotas ≡ spec 0.6.0 (contrato) com as 2 rotas novas; 401 nas 2; export handler: zip tem entradas esperadas + `Content-Disposition` + skip de órfão logado; `get_to_file` no mock observável |
| `bash scripts/test-db.sh` (estendido) | Postgres do compose + `MockStorage` | **roundtrip de fidelidade**: dataset com boxes manual+autotracker (conf/track_id) + caption (origin/model) + classes + split → export → import → compara por API: counts, classes idx/name, split por imagem, boxes (coords/origin/conf/track_id), caption (text/origin/model), `autoTracked` preservado; **substituição**: import `replace=true` sobre dataset existente → linha antiga sumiu (CASCADE) + sweep do prefixo antigo registrado no mock + dataset novo com dados do zip (counts/classes/boxes/caption corretos) + `image_embeddings` antigas ausentes (CASCADE da 0004) + re-indexação fire-and-forget; `replace=false` sobre existente → 409; **zip inválido + `replace=true` → 400 e dataset existente INTACTO** (prova da ordem validação→teardown); 400 manifest corrompido; falha injetada no PUT no meio do import → linha do dataset removida + sweep do prefixo registrado no mock |
| E2E smoke (stack `ENGINE_MOCK=1`, Chrome) | `docker compose` completo | galeria → Exportar baixa `.zip` válido (download real no browser) → Importar modal → dataset novo com contagens → compara por API; console limpo |

## Spike obrigatório? — **não**

As premissas externas desta fatia são bibliotecas Rust estáveis e bem documentadas
(`zip` 2.x com Zip64, `tokio-util::io::ReaderStream`, `serde_yaml`), não servidores de
terceiros com flags indocumentadas (o caso do spike 3b.0 do SeaweedFS). Os critérios
binários de inversão ficam **embutidos no commit 3e.1**: o unit test de roundtrip
write→read do próprio crate e o smoke E2E do download no browser. Se `zip` falhar um
critério (ex.: comportamento Zip64 > 4 GiB ou desempenho do Deflate), a inversão é
registrada e o fallback é `async_zip` atrás do mesmo layout de pacote (D1 não muda — o
formato do arquivo independe do crate).

## Riscos

- **R1 — Import atômico vs. falha parcial:** compensação por imagem + delete da linha do
  dataset + sweep do prefixo em falha fatal. Testar com falha injetada no PUT (test-db);
  risco residual: sweep best-effort pode falhar → prefixo órfão reapável por script
  (stance da 3b, R5/0003).
- **R2 — Zip bomb/slip:** tetos por entrada e global + reader contado (declarações do
  central directory mentem); sanitize e whitelist de raízes. Fixtures maliciosas nos
  units.
- **R3 — Fidelidade do roundtrip:** origem/conf/trackId/model/split são os campos que um
  import ingênuo perderia; o teste de fidelidade compara **por API** (plano §6 item 7) —
  sem ele, a fatia não fecha.
- **R4 — Órfãos no export (linha sem objeto):** skip + `eprintln`; `counts` refletem o
  exportado; manifest nunca promete imagem que não está no zip. Comportamento documentado
  no backend.md na sync.
- **R5 — Conflito de slug sem rota de rename:** dois caminhos de resolução (P3 revisto):
  (a) importar como dataset novo → campo `title` do modal (D3); (b) atualizar o existente
  → caixa de confirmação de irreversibilidade re-envia com `replace=true` (D5). O servidor
  nunca decide por si; o 409 só dispara após validação completa (precedência de erro
  determinística: pacote ruim sempre 400, mesmo com slug existente).
- **R6 — Disco no export (~2× o dataset):** spools de imagem + zip no tempdir; single-user
  local, limpo no drop; pico = imagens spooladas + zip acumulado. Aceito.
- **R7 — Deflate de mídia:** `Stored` para imagens (já comprimidas), `Deflated` só para
  texto; evita CPU inútil e zip maior que o dataset.
- **R8 — Edições manuais em `labels/*.txt`/`captions.jsonl`/`dataset.yaml` não
  sobrevivem ao re-import:** o manifest é a verdade; os derivados são materialização de
  exportação. Comportamento documentado (não é bug — é a doutrina "o package gera do
  banco" aplicada ao roundtrip).
- **R9 — Indexação fire-and-forget pós-import:** espelho do upload; dataset grande →
  embedder trabalha em background; mock em dev; `eprintln` honesto.
- **R10 — Janela de substituição (teardown→ingest):** entre o commit da transação que
  funde DELETE antigo + INSERT novo (D6) e o fim do ingest, o dataset antigo **não existe
  mais** e o ingest pode falhar fatalmente (ex.: bucket cai no meio). Estado de falha:
  dataset ausente + **zip intacto** (o usuário importa DE um zip que permanece com ele —
  re-import é sempre possível, inclusive com `replace=true` para limpar resíduos) +
  possível resíduo de objeto nos prefixos antigo e/ou novo (sweeps best-effort, reapável
  por script — stance da 3b). Janela minimizada estruturalmente: (a) teardown o mais
  tarde possível — só após a validação completa do pacote (D3/D4), zip corrompido nunca
  destrói o existente; (b) ingest imediatamente após o teardown; (c) o banco transita
  old→new **atomicamente** (DELETE+INSERT na mesma transação — não existe a janela
  "nenhum dataset com o slug" no banco; se o INSERT novo falhar, rollback devolve o
  antigo intacto). **Embeddings:** `image_embeddings` (0004) tem `ON DELETE CASCADE` nos
  dois FKs (`image_id` e `dataset_id`) — o CASCADE do DELETE antigo as apaga; o re-import
  re-indexa fire-and-forget (D6). Teste: falha injetada no PUT no meio do ingest com
  `replace=true` → dataset antigo ausente, novo ausente, zip intacto, sweep dos prefixos
  registrado no mock.

## O que fica falso nos docs (lista pronta para o `@docs-sync`, commit 3e.5)

**Não aplicar agora** — docs descrevem o que existe, não o que foi aprovado.

- `backend.md` :131-133 — "`POST /:id/export`, `POST /datasets/import` … adiados para a
  fatia 3e" → implementados (nota nova com a tabela de rotas + `import_invalid` +
  **semântica de substituição**: multipart `file`/`title`/`replace`, 409 como protocolo de
  detecção, teardown + ingest com slug preservado); a linha do `POST /:id/package`
  permanece adiada para a **fatia 4** (revisão D0).
- `backend.md` :254 (§11 `manifest.json` "PROJETO para 3e/4") — **desambiguar**: o
  manifest do §11 é o transporte orquestrador (fatia 4, `{dataset_id, slug, category,
  engine, files, md5_zip, bytes, chunks}`); o `manifest.json` de backup da 3e é artefato
  distinto (D1, schema_version/classes/images/boxes). O §11 continua projeto da 4.
- `frontend.md` :86 (§5.1 header "Importar (Backup)") — **removido** por P2 (import só na
  galeria); o botão desabilitado sai do header de `/datasets`.
- `frontend.md` :96 (§5.2 galeria) — "Exportar (`.zip + dataset.yaml + anotações`)" →
  implementado; ganha Importar (modal com `title` + **diálogo de substituição no 409**) e
  o copy do pacote (manifest + derivados).
- `frontend.md` :180 (fluxo 5 "Backup") — implementado.
- `frontend.md` :188 (§10) — "Export/import/package seguem pendentes (3e)" → export/import
  implementados (rotas + `lib/backup.ts` + limite 200 MiB + 8 MiB + **fluxo de
  substituição em dois passos: 409 → diálogo de irreversibilidade → `replace=true`**);
  package segue pendente (fatia 4).
- `frontend.md` :216 (backlog "import/export `.zip` validado") — item marcado feito (só
  o "progresso/cancel do upload" permanece).
- `frontend.md` §13 :226 (limites) — mencionar o limite do zip de import
  (200 MiB + envelope) junto do limite de upload.
- `adr/0003` D9 (:204-213) — **banner de emenda** (padrão do banner da 0002, sem
  reescrever o corpo): "revisado pela ADR-0006 — export/import na 3e; package na fatia 4".
- `plano-3e-export-import.md` — status PLANEJADO → ACEITA (após o aceite desta ADR) e,
  ao final, EXECUTADA.
- `coordenacao.md` — bloco da fatia 3e reescrito a cada commit; `dividas.md` :122-123
  (export/import → 3e) vira "implementado"; T4 (`dataset_versions`) segue na fatia 4.

## Plano de commits (3e.0–3e.5; branch `feat/datasets-export-import` de `main`)

Um dispatch = um commit; produção < 400 linhas por commit (contrato/testes fora da
conta, exceção da casa). **3e.1 e 3e.2 são sequenciais** (mesmos módulos) — nunca
paralelos; 3e.3 pode ser preparado em paralelo SÓ se restrito a `apps/web/**`.

| # | Dono | Conteúdo | Critério de pronto |
|---|---|---|---|
| **3e.0** | @architect + coordenador | ADR-0006 (este arquivo) → aceite do usuário | ADR ACEITA; delta §9/§10 listado (aplicar só no 3e.5) |
| **3e.1** | @rust-dev | **Export**: Cargo (`zip` 2, `tokio-util` io, `serde_yaml` → deps); `StoragePort.get_to_file` (port+mock+s3); `src/datasets/export.rs` (builder: manifest do banco, materialização YOLO/captions em tempdir, zip em `spawn_blocking`, stream com Content-Disposition/Content-Length); `src/auth/routes.rs` (rota + `PROTECTED_ROUTES` `[200,401,404,503]`); openapi 0.6.0 (versão + rota export) **no mesmo commit**; units + contrato | `cargo test -p api-principal` verde; `cargo fmt --all --check` limpo; zip baixa com conteúdo correto (smoke E2E parcial); unit de roundtrip write→read do `zip` |
| **3e.2** | @rust-dev | **Import**: `src/datasets/import.rs` (spool + pré-scan zip-slip/bomb + extração + validação completa do manifest + **teardown condicional `replace=true`: tx DELETE antigo + INSERT novo + classes, sweep do prefixo antigo pós-commit** + escrita imagem objeto→linha→compensação→boxes/caption + falha fatal = delete row novo + sweep + indexação fire-and-forget); `src/datasets/models.rs` (structs/validações puras do manifest); `src/auth/routes.rs` (rota + `DefaultBodyLimit::max(IMPORT_BODY_LIMIT_BYTES)` + `[201,400,401,409,503]`); `error.rs` (`MSG_IMPORT_INVALID`); openapi (rota import + `import_invalid` no enum) + `tests/datasets_db.rs` estendido (roundtrip de fidelidade + substituição + 409 + 400 + falha injetada) | `bash scripts/test-db.sh` verde; roundtrip local provado (fidelidade por API: counts/classes/split/boxes-origin-conf-trackId/caption/autoTracked) + substituição (`replace=true`: antigo some + embeddings CASCADE + sweep no mock; `replace=false`: 409; zip inválido + `replace=true`: 400 e antigo intacto) |
| **3e.3** | @frontend-dev | `apps/web/lib/backup.ts` (exportDataset → fetch raw → blob + `a[download]`; importDataset → FormData → apiFetch\<Dataset\>); galeria `[id]/page.tsx`: habilita Exportar (toast 404/503), botão Importar + `components/studio/ImportDatasetModal.tsx` (file picker `.zip`, campo `title`, **diálogo de confirmação de irreversibilidade no 409 — caixa de mensagem "substituir apaga o dataset atual; a ação não tem reversão" com Cancelar/Substituir → re-envio com `replace=true`**; resultado com contagens + "Abrir dataset importado", toasts 400/503/413 — design system; o 409 deixa de ser toast e vira o diálogo); `/datasets/page.tsx`: **remove** botão Importar do header (P2); `DatasetMenu.tsx`: habilita item Exportar | `npm run build --workspace=web` verde; smoke visual no Chrome (download real + modal + diálogo de substituição + toasts; console limpo) |
| **3e.4** | @reviewer | Revisão do diff completo da fatia | invariantes da casa: camelCase (D1/0002), ordem objeto→linha→compensação, contract ≡ router a cada commit, ci/compose coerentes, nenhuma migration |
| **3e.5** | @docs-sync | Lista "O que fica falso nos docs" acima + `dividas.md` | docs descrevem o que existe |

**Notas de processo (lições já pagas):** `cargo fmt --all` antes de reportar; rotas novas
com os status exatos da ADR em `PROTECTED_ROUTES`; contract test exige spec ≡ router **a
cada commit** (portanto o delta OpenAPI é incremental: 3e.1 declara só export; 3e.2
adiciona import sob a mesma 0.6.0); nenhuma migration esperada — se surgir, PARAR e
reportar (P6); se mexer no compose/CI, fazer no MESMO commit (não é o caso esperado).
