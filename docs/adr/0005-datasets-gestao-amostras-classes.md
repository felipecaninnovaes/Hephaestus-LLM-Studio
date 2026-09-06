# ADR-0005 — Gestão de amostras e classes em datasets (Fatia 3g)

- **Status:** ACEITA (2026-09-06)
- **Data:** 2026-09-06
- **Componentes:** `services/api-principal` (`src/datasets/handlers.rs`, `src/datasets/models.rs`, `src/storage/port.rs`, `src/storage/s3.rs`, `src/storage/mock.rs`, `src/auth/routes.rs`), Postgres (migration `0005_image_soft_delete.sql`), `packages/contracts` (spec 0.3.0 → **0.4.0**), `apps/web` (galeria, `ClassesModal`, `lib/classes.ts`, `lib/images.ts`, `components/studio/Toast.tsx`).
- **Fontes:** gaps reportados pelo usuário (classes pós-criação impossíveis; imagem sem exclusão) + pedido de restauração na exclusão; `docs/backend.md` §9/§10; `docs/frontend.md` §5.2/§5.3/§10; `docs/adr/0004` D5 (recalibrada: 3f → 0.5.0); commits `d3f3d21`, `3240c39`, `a13a2e1`, `bd60033`, `f45a2e0`, `17f460e` (+ este, docs-sync).
- **Sequência:** 3d → **3g** → 3f → 3e → 4.

## Contexto

Dois gaps bloqueavam o uso real da galeria: (a) classes só existiam na criação — renomear/adicionar/remover depois era impossível; (b) imagem não tinha exclusão, e quando o usuário pediu exclusão, pediu também restauração (exclusão destrutiva direta era inaceitável como UX). A armadilha central da D2: o FK `boxes.class_id ON DELETE CASCADE` convida a `DELETE`+`INSERT` de classes, o que regeneraria ids e orfanaria todas as caixas — a reconciliação por id + guard 409 existe por causa dela.

## Decisões

### D0 — A fatia é a **3g "gestão de amostras e classes"**, sequência 3d → **3g** → 3f → 3e → 4

Origem dupla: gaps reportados pelo usuário (classes pós-criação impossíveis; imagem sem exclusão) + pedido explícito de restauração na exclusão. A 3g landou antes da 3f e tomou a spec **0.4.0** (regra "versão = ordem de landing"); a ADR-0004 D5 já foi recalibrada (3f → 0.5.0).
**Descartado:** embutir na 3e (export) ou na 4 (jobs) — gestão de amostras é pré-requisito de uso da galeria, não de export nem de treino.

### D1 — Spec 0.3.0 → **0.4.0**; ADR-0004 D5 recalibrada (3f → 0.5.0)

A 3g adiciona 4 rotas + `trashCount` no wire, logo a spec anda. A nota de recalibração já está aplicada na 0004 ("Nota de delta (3g.3)"), então esta D1 só registra o fato consumado.
**Descartado:** minor sem bump — o wire mudou (`trashCount`, 4 rotas, 1 código novo), consumidores gerados precisam do número.

### D2 — `PUT /api/datasets/:id/classes`: substituição total com **reconciliação por id**

Semântica (código: `handlers.rs::put_classes`): id presente = rename preservando id (caixas intocadas); ausente = cria; ordem do array = idx 0..n-1; cor rederivada da paleta server-side (nunca do cliente). Validação pura em `models.rs` (regex única reutilizada, ≤200 itens, nomes/ids únicos, dedupe silencioso proibido). **409 `classes_in_use`** bloqueia remoção de classe com caixas — guard DENTRO da transação, antes de qualquer write (o CASCADE do FK é a armadilha: `DELETE`+`INSERT` regeneraria ids e orfanaria todas as caixas; o guard conta caixas de imagens ativas **e** da lixeira — conservador: as caixas voltam no restore). Dance de `UNIQUE(name/idx)` na transação com ordem OBRIGATÓRIA: guard → fase 1 (tmp via `id.simple()` por causa do `CHECK` de name, `idx+1000000`) → **DELETE das removidas** → fase 2 (finais) → INSERT das novas. O DELETE entre as fases é obrigatório: sem ele, remover classe com `idx` original menor que o destino renumerado de um mantido viola `UNIQUE(dataset_id, idx)` (descoberto no review da fatia com probe empírico; testes db cobrem remover 1ª/do meio/última).
**Descartado:** PATCH por classe (N round-trips, reconciliação no cliente); aceitar cor do cliente (paleta deixa de ser invariante); deletar em cascata silenciosa (perda de anotação sem consentimento).

### D3 — Exclusão de imagem **restaurável** (lixeira)

`DELETE /:id/images/:imageId` = soft delete (204, sem sweep, objeto intocado; 404 se inexistente/já deletada/UUID inválido). `POST /:id/images/:imageId/restore` (204 sem conflito; conflito de filename → rename `{stem}_restaurado{ext}` com desambiguação + **copy_object server-side** + delete da key antiga best-effort pós-commit → 200 `{filename}`; copy falha → 503 `storage_unavailable`, nada parcial). `DELETE /:id/trash` = purge REAL (CASCADE + sweep best-effort por prefixo de imagem pós-commit, 204 idempotente). `GET /:id/images?deleted=true` lista a lixeira. `Dataset.trashCount` derivado (badge).
**Descartado:** hard delete direto (pedido do usuário era restauração); mover objeto no bucket no soft delete (custo sem benefício — a linha some das queries, o objeto fica); sweep síncrono falhando a rota (família best-effort, ADR-0003 D1).

### D4 — Migration `0005_image_soft_delete.sql`: `images.deleted_at`, unique parcial, índice da lixeira, trigger filtrando deleted; **sem** spike

`deleted_at TIMESTAMPTZ NULL`; `UNIQUE(dataset_id, filename) WHERE deleted_at IS NULL` (re-upload de filename na lixeira nasce linha nova, sem conflito); índice parcial da lixeira `(dataset_id) WHERE deleted_at IS NOT NULL`; `heph_refresh_dataset_counters` filtra `deleted_at IS NULL` (trigger `AFTER UPDATE` recalcula de graça — soft delete/restore movem contadores sem código extra). Sem spike: DDL trivial sobre padrão 0003 já provado.
**Descartado:** flag booleana (perde "quando deletou", dado do futuro GC); renomear filename no delete (sujaria restore + re-upload).

### D5 — Invisibilidade (inventário fechado D13)

Detail/data/boxes/caption → 404 em deletadas; `EXISTS auto_tracked` nas 3 queries de dataset ignora deletadas; `derived_source` deriva de contador já filtrado (dataset com só-lixeira volta a `source: null`). Cobertura: teste de inventário `t0003_trash_invisivel_e_trash_count` (13 leituras: list/get/detail/data/boxes/caption × estados).
**Descartado:** 410 Gone (D8 manda 404 para id inacessível); filtrar só no list e deixar detail aberto (lixeira vazaria por URL direta).

### D6 — UI: `ClassesModal`, copy honesta, hover trash + toast Desfazer, pills, purge único ponto destrutivo

`ClassesModal` (galeria + editor; 409 mantém modal aberto com estado, sem fechar na cara do erro). Copy honesta ("renomear preserva caixas; remover com caixas é bloqueado"). Hover trash por thumb + toast com **Desfazer** (`Toast` ganha `action`, 6s). Pills `Ativas | Lixeira (n)`; `Restaurar` por item; `Esvaziar` = única exclusão permanente (`ConfirmDialog`). Editor SEM exclusão (um lugar só para destruir).
**Descartado:** botão deletar no editor (dois lugares destrutivos = erro); purge por item (varredura parcial confunde contagem); toast sem ação (restauração escondida em outra aba).

### D7 — Cross-tab aceito v1

Autosave do editor com `classId` removido em outra aba → 400 (guard existente, sem código novo) → toast + reload. Caixas órfãs renderizam cinza (fallback `#71717a`/"classe") em vez de quebrar o canvas.
**Descartado:** lock otimista com versão de classes (custo de protocolo para conflito raro); refetch silencioso sem toast (usuário não entenderia o sumiço da caixa).

### D8 — Erros: 1 código novo `classes_in_use` (409); 503 reusa `storage_unavailable`; resto padrão

`classes_in_use` (409) é o único código novo — remoção bloqueada com N caixas. Restore com copy falho reusa `storage_unavailable` (503, ADR-0003 D10). 404 padrão D8 (UUID inválido/inexistente/já-deletada). `deny_unknown_fields` nos requests novos.
**Descartado:** 422 para 409 semântico (409 é o conflito honesto); código novo para copy (é indisponibilidade de storage, não erro de domínio).

### D9 — Fora de escopo v1

GC automático da lixeira (dívida registrada em `docs/dividas.md` — `deleted_at` já dá o dado; só falta o TTL + purge agendado), renomear dataset, re-upload/substituir imagem, split manual, cor custom, lixeira/undo de dataset, exclusão no editor, contagem de uso por classe, lote (multi-restore/multi-purge).
**Descartado:** incluir qualquer um deles "de carona" — cada um tem semântica própria e inflaria a transação da 3g.

### D10 — Boundary: só o principal; zero DDL além da 0005; manager/orquestrador/engines intocados

`copy_object` entra na `StoragePort` (mock observável + s3 `CopyObject` com encode de `copy_source` — gotcha do SDK: `{bucket}/{key}` com key URL-encoded, barras preservadas). Manager/orquestrador/engines: zero mudança (topologia §1 intacta).
**Descartado:** cliente S3 no orquestrador para copiar (ADR-0003 D8: só o principal fala S3).

## Consequências

- **Dependências novas:** nenhuma (s3 `CopyObject` já estava no SDK; mock ganha `copied` observável).
- **Commits:** 7 (`d3f3d21` migration, `3240c39` PUT classes, `a13a2e1` lixeira API, `bd60033` spec 0.4.0, `f45a2e0` ClassesModal, `17f460e` lixeira UI + este docs-sync).
- **Riscos fechados:** R5 (dance de UNIQUE testado sob transação), R6 (inventário de invisibilidade testado), R7 (GC vira dívida explícita, não esquecimento).
- **Env/storage:** nenhum env novo; `copy_object` comporta-se como `delete_prefix` (best-effort onde pós-commit, hard onde pré-commit).

## Migration `0005_image_soft_delete.sql` (contorno)

```sql
ALTER TABLE images ADD COLUMN deleted_at TIMESTAMPTZ NULL;
DROP INDEX IF EXISTS images_dataset_filename_idx;
CREATE UNIQUE INDEX images_dataset_filename_active_idx
  ON images (dataset_id, filename) WHERE deleted_at IS NULL;
CREATE INDEX images_trash_idx ON images (dataset_id) WHERE deleted_at IS NOT NULL;
-- heph_refresh_dataset_counters: todos os COUNT/SUM ganham
--   AND deleted_at IS NULL; trigger AFTER UPDATE recalcula de graça.
```

Invariantes: re-upload de filename na lixeira = linha nova (unique parcial não conflita); contadores/status/`source`/`autoTracked`/`trashCount` derivam do conjunto ativo; purge é o único `DELETE` físico de imagem fora do `DELETE dataset`.
