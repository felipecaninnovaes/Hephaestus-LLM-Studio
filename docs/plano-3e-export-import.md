# Plano da Fatia 3e — Export/Import (backup estruturado de dataset)

**Status:** ACEITO — a ADR-0006 (`docs/adr/0006-export-import.md`) foi escrita pelo
`@architect`, auditada, emendada (substituição consentida: 409 → diálogo de
irreversibilidade → `replace=true`) e **ACEITA pelo usuário em 2026-09-07**. A ADR é a
especificação executável; este arquivo permanece como plano de coordenação da fatia
(tabela de commits, donos, verificação). Implementação em curso a partir do 3e.1.

**Criado:** 2026-09-07 (sessão 9, antecipação pedida pelo usuário — "planejar e
gravar em arquivo, sem implementar").
**Ordem na sequência:** 3e é a PRÓXIMA fatia (3d ✅, 3f ✅ mergeada; sequência
informada pelo usuário na sessão 8: 3e → 3f → 3h, com 3f feita fora de ordem).

---

## 1. Escopo — o que é a 3e

Backup estruturado de dataset: o usuário **exporta** um dataset (imagens +
anotações + classes + split) como um pacote portátil e **importa** o mesmo
pacote de volta, recriando o dataset com fidelidade.

Fontes de verdade (não re-inventar):

- **IDEIA.md §1**: "Suporte a Exportar o dataset e importar (uma forma de backup
  estruturado)" — funcionalidade da galeria, por dataset.
- **docs/PRODUCT.md :28,41**: "exportação/importação estruturada de pacotes de
  dataset (ZIP contendo imagens, anotações, `dataset.yaml` e labels)"; fluxos de
  backup concentram-se **no contexto da galeria**.
- **docs/adr/0003-object-storage-s3.md D9 (:204)**: `POST /:id/export`,
  `POST /datasets/import`, `POST /:id/package` saíram da 3b e viraram a 3e;
  backup interim = console + `mc mirror`. Chunking 8 MB sobrevive **só** no
  transporte principal→orquestrador remoto (não nesta fatia).
- **docs/backend.md :51 e :249**: chunking é "projeto da fatia 3e/4"; a
  materialização YOLO (`.txt`, `data.yaml`, `captions.jsonl`) nasce **do banco
  em tempdir** — "O package sempre gera do banco" (doutrina já fixada).
- **docs/frontend.md :96, :180**: Exportar (`.zip + dataset.yaml + anotações +
  captions.jsonl`) / Importar (mesmo pacote); menu de contexto `import_dataset`
  = "Selecionar backup `.zip/.json`".

## 2. O que a fatia herda (estado atual do código)

- **Spec OpenAPI em 0.5.0** (3f). Delta esperado da 3e: **0.6.0**.
- **Ganchos de UI prontos e desabilitados**: `DatasetMenu.tsx` :86-95 — item
  "Exportar" com `title="Export chega na fatia 3e"`; galeria 3d tem as ações
  desabilitadas com titles honestos (Exportar=3e). **Importar não tem gancho
  nenhum** — o protótipo o desenha como menu `import_dataset` ("Selecionar
  backup .zip/.json").
- **Schema já suporta import**: `boxes.origin` e `captions.origin` têm CHECK
  `manual|autotracker|import` — o valor `import` existe no banco desde a 3b e
  nunca foi usado. **Sem migration esperada** (confirmar no 3e.0).
- **Infra reutilizável**: spool em tempfile + `put_object` (ADR-0003 D2), sniff
  de magic bytes (`src/storage/sniff.rs`), sweep de prefixo pós-commit (D7),
  ordem objeto→linha→compensação (D6), limite de corpo multipart dedicado
  registrado em `src/auth/routes.rs` (200 MiB + 8 MiB envelope — convenção 3b).
- **Cuidado de boundary**: o limite de corpo do upload mora em
  `src/auth/routes.rs` (camada de roteamento), não no handler de datasets — o
  import multipart precisará do MESMO tratamento na rota nova.

## 3. Decisões JÁ travadas por docs existentes (não re-decidir)

1. Formato do pacote = **ZIP** com `dataset.yaml` + anotações + imagens
   (PRODUCT.md, frontend.md).
2. Export gera **do banco + bucket**, não de disco — materialização é efêmera
   em tempdir (backend.md :249; ADR-0003 D1).
3. Entre principal e cliente não há chunking — chunking é transporte
   principal→orquestrador **remoto** e não entra aqui (ADR-0003 D9).
4. Backup interim até a fatia: console + `mc mirror` (não implementar nada
   disso; é a resposta de operação manual).
5. UI de export/import vive **na galeria** (IDEIA §1, PRODUCT.md :41,
   frontend.md :24).
6. Wire camelCase em `/api/*`; erros `{code,message}`; convenção 413/405/500
   global (ADR-0002 D1 e convenções de casa).

## 4. Decisões ABERTAS → ADR-0006 (desenho do @architect, aprovação do usuário)

A ADR deve fechar, no mínimo:

- **D1 — Layout do ZIP (contrato do arquivo)**: proposta do coordenador —
  `manifest.json` (schemaVersion, exportedAt, dataset{name,format,counts},
  classes[{idx,name,color}], images[{filename,split,width,height,sha256,
  mediaType}]), `dataset.yaml` (estilo YOLO: names/train/val), `labels/*.txt`
  materializados das boxes (formato `yolo_txt`), `captions.jsonl`,
  `images/<filename>` binários. Estrutura determinística → import é espelho.
  **Fidelidade**: manifest deve carregar `origin`/`conf`/`trackId` das boxes
  (colunas existem; perda deles no roundtrip seria defeito).
- **D2 — Transporte do export**: proposta — `POST /api/datasets/:id/export`
  responde **o zip em stream** (spool tempdir → body com
  `Content-Disposition: attachment`), sem tocar o bucket para o zip em si
  (evita re-upload + presigned de algo que o principal já tem). Fonte dos
  bytes das imagens = `get_object` por `object_key`. Decidir crate (`zip` vs
  `async_zip`) e se o spool é em disco ou stream-on-the-fly.
- **D3 — Transporte do import**: `POST /api/datasets/import` multipart (campo
  `file`), spool tempfile, validação de manifest **antes** de escrever
  qualquer coisa. **Limite de corpo dedicado** (proposta: mesmos 200 MiB + 8
  MiB envelope do upload — confirmar se basta para o caso de uso).
- **D4 — Segurança do ingest**: zip-slip (caminhos `../` nas entradas) →
  rejeitar; zip bomb (teto de descompressão total); sniff de magic bytes de
  cada imagem reutilizado; filename sanitizado igual ao upload.
- **D5 — Semântica de conflito no import**: dataset com nome/slug já
  existente → proposta: **409 `slug_conflict`** (erro existente, sem
  auto-suffix — backup não é cópia silenciosa; o usuário renomeia antes).
  Dedupe de filename dentro do próprio zip → `duplicate` (semântica do upload).
- **D6 — Ordem de escrita do import** (espelho da D6 da 3b): proposta —
  dataset row primeiro (precisa do id) → por imagem: put_object → INSERT →
  compensação na falha; falha fatal → sweep do prefixo do dataset novo.
- **D7 — Erros novos**: proposta — `import_invalid` (400: manifest ausente/
  incompatível/corrompido), `storage_unavailable` (503, existe), 409
  `slug_conflict` (reuso). Spec 0.5.0 → 0.6.0.
- **D8 — Export e a lixeira**: proposta — export inclui **somente imagens
  ativas** (`deleted_at IS NULL`); lixeira é estado de trabalho, não de backup.
- **D9 — `POST /:id/package` na 3e ou na 4**: **recomendo mover para a fatia
  4** — o consumidor (orquestrador + transporte remoto 8 MB) só nasce lá;
  incluir manifest+md5 para orquestrador na 3e seria código sem consumidor.
  ADR-0003 D9 o lista na 3e — a ADR-0006 registra a revisão consciente.
  **[PERGUNTA AO USUÁRIO — ver §7]**

## 5. Esqueleto de plano de commits (um dispatch = um commit)

Branch: `feat/datasets-export-import`, a partir de `main` atualizada.

| Passo | Dono | Conteúdo | Critério de pronto |
|---|---|---|---|
| **3e.0** | @architect + eu | ADR-0006 (decisões D1–D9 acima) → aprovação do usuário | ADR ACEITA; delta §9/§10 listado (aplicar só no 3e.5) |
| **3e.1** | @rust-dev | **Export**: `POST /:id/export` — builder de manifest do banco, materialização YOLO em tempdir, zip + stream; openapi 0.6.0 **no mesmo commit**; testes unit + contrato | `cargo test -p api-principal` verde; `cargo fmt --all --check` limpo; zip baixa com conteúdo correto |
| **3e.2** | @rust-dev | **Import**: `POST /api/datasets/import` multipart — validação de manifest, ingest objeto→linha→compensação, limite de corpo na rota, erros novos; openapi + testes db | `bash scripts/test-db.sh` verde; roundtrip local provado |
| **3e.3** | @frontend-dev | **UI**: habilita "Exportar" (DatasetMenu + galeria) → download; modal Importar (file picker `.zip`, resultado com contagens, toasts 400/409/503/413); design-system obrigatório | `npm run build --workspace=web` verde; smoke visual no Chrome |
| **3e.4** | @reviewer | Revisão do diff completo da fatia | invariantes da casa (casing D1, ordem objeto→linha→compensação, contract≡router, ci/compose coerentes) |
| **3e.5** | @docs-sync | backend.md §9/§10 (rotas + erro novo), frontend.md §5.2/§10, dividas.md | docs descrevem o que existe |

**Ownership/serialização**: 3e.1 e 3e.2 tocam o MESMO módulo (`src/datasets/`,
`src/auth/routes.rs`, openapi) — **sequenciais, nunca paralelos**. 3e.3 pode
ser preparado em paralelo com 3e.1/3e.2 SOMENTE se restrito a
`apps/web/**` (disjunto real); contratos (openapi) ficam sempre sequenciais.

**Notas de processo para os prompts de despacho** (lições já pagas):

- rust-dev: `cargo fmt --all` antes de reportar (lição 3g/CI run 19).
- Rotas novas entram em `PROTECTED_ROUTES` com os status exatos da ADR.
- Contract test exige spec ≡ router **a cada commit** (lição 3g.1).
- Se (inesperadamente) houver migration, o service do `.gitea/workflows/ci.yml`
  é atualizado NO MESMO commit (lição 3f/run 22) — mas a 3e não deveria ter.
- Multipart/limite de corpo: seguir o padrão do upload 3b registrado em
  `src/auth/routes.rs` (`UPLOAD_BODY_LIMIT_BYTES` + envelope).

## 6. Verificação da fatia (eu rodo antes de declarar pronta)

1. `cargo check --workspace`
2. `cargo fmt --all --check`
3. `cargo test -p api-principal`
4. `bash scripts/test-db.sh` (compose de pé)
5. `npm run build --workspace=web`
6. `docker compose -f infra/compose.yaml config -q`
7. **Smoke E2E roundtrip (critério de aceitação da fatia)**: dataset com
   imagens + boxes (manual e autotracker) + caption + classes → export baixa
   `.zip` válido → import do mesmo zip → dataset novo com **counts, classes,
   split, boxes (coords/origin/conf/trackId) e caption idênticos** — comparado
   por API, não a olho. Console limpo no Chrome.

## 7. Perguntas ao usuário (fechar ANTES do 3e.0/ADR)

1. **`POST /:id/package`**: aceita mover para a fatia 4 (recomendo — ver D9)?
   Ou quer o manifest-para-orquestrador já na 3e?
2. **Entrada do Importar na UI**: só na galeria (leitura atual da IDEIA/PRODUCT)
   ou também no header de `/datasets` como no protótipo (frontend.md :86)?
3. **Conflito de nome no import**: 409 seco (recomendo) ou auto-sufixo de slug?
4. **Limite do zip de import**: 200 MiB (igual upload) basta? (imagens + zip
   sem compressão extra de mídia já são ~o mesmo volume do dataset)
5. **Export inclui lixeira?** (recomendo não — ver D8)
6. **Confirma a ausência de migration** (nenhuma mudança de schema esperada na
   3e) — a ADR-0006 fecha isso explicitamente.

## 8. Dívidas e pendências relacionadas (de `docs/dividas.md`)

- Nenhuma dívida em aberto BLOQUEIA a 3e. T4 (`jobs.dataset_id` +
  `dataset_versions`) continua marcada para a **fatia 4** — a 3e NÃO cria
  `dataset_versions` (import cria dataset novo; snapshot de versão é conceito
  de treino/materialização, não de backup).
- Logging server-side (fatia nomeada, sem número): os handlers novos da 3e
  nascem com `eprintln` mínimo honesto como os existentes — não antecipar a
  fatia de logging.
- `e2e-smoke.sh` (dívida "confirmar cobertura --datasets"): o roundtrip da §6
  item 7 é prova manual/E2E desta fatia; não confundir com o script.

## 9. Checklist de retomada (quando abrir a sessão da 3e)

1. Protocolo de retomada padrão (`docs/coordenacao.md` → topo).
2. Ler este arquivo inteiro + `docs/adr/0003-object-storage-s3.md` D9 e
   `docs/PRODUCT.md`.
3. Fechar as perguntas da §7 com o usuário (5 minutos).
4. Despachar **3e.0** ao `@architect` com as decisões abertas da §4 como brief
   (entregável: ADR-0006 no formato da casa + delta §9/§10 + plano de commits).
5. Aprovação do usuário na ADR → abrir branch `feat/datasets-export-import`
   → seguir a tabela da §5, um dispatch por commit, revisão no 3e.4.
