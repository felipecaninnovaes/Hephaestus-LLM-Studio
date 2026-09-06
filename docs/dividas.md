# Dívidas técnicas e pendências — registro da casa

Registro **permanente** de dívidas técnicas e pendências que as próximas fatias
precisam honrar. Inspirado no `tech-debt-tracker.md` do artigo "Harness
Engineering" (OpenAI, 2026-02-11): dívida como registro de primeira classe do
repo, legível por qualquer agente sem contexto externo — paga continuamente em
pequenas parcelas, não em rajadas.

Separado de propósito: o estado de sessão/plano vive em `docs/coordenacao.md`
(volátil, reescrito por sessão); este arquivo é sistema de registro (permanente,
versionado). O `coordenacao.md` referencia este arquivo e não o duplica.

**Como atualizar**: ao REGISTRAR (revisor, spike, ADR, lição de ambiente), ao
MARCAR a fatia que honra, e ao QUITAR (mover para "Quitadas" com o commit que
fechou). Enquanto em aberto, uma dívida NÃO pode ser violada por uma fatia nova
— honrar no nascedouro.

## Em aberto

### Com fatia marcada

- **T4 — `jobs.dataset_id` + `dataset_versions` (fatia 4 — jobs/package/
  materialização)**: `jobs.dataset_id UUID NULL REFERENCES datasets(id) ON
  DELETE SET NULL` + snapshot `dataset_versions` (nunca `RESTRICT`) —
  ADR-0002 T4. Nessa mesma fatia o orquestrador ganha cliente S3 com
  credencial escopada por prefixo.

### Sem fatia marcada

**Backend**

- **GC automático da lixeira (TTL) — ABERTA 2026-09-06** (fatia futura, fora da
  3g por D9 da ADR-0005): hoje só purge manual via UI (`DELETE /:id/trash`);
  `images.deleted_at` já dá o dado (idade do soft delete); falta TTL +
  purge agendado (job/cron que DELETE físico + sweep das vencidas).
- **Ordem estável de `boxes` entre saves (nota menor da 3d)** — o PUT boxes é
  `DELETE`+`INSERT` com `RETURNING`: os ids nascem novos a cada save e
  `GET detail` não garante ordem (visto no smoke 3d: `[autotracker, manual]`
  no PUT, `[manual, autotracker]` no GET seguinte). A UI correlaciona
  seleção por índice de payload pós-save (correto), mas os chips `#N` no
  canvas podem reordenar entre loads. Correção real = coluna de ordenação/
  `ORDER BY` determinístico no backend — só valerá a pena quando o
  autotracker consertar caixas existentes (fatia 4), hoje os efeitos são
  cosméticos.
- **Logging server-side (fatia nomeada, sem número)** — revisores 3b.3/3b.6:
  `Err(_) => internal()` engole detalhes (banco vira 500 mudo, sem log nenhum)
  e o `map_err` do SDK descarta `code()`; sweep do DELETE usa `eprintln` como
  mínimo honesto até a fatia chegar. Escopo: log server-side (nunca no
  response) antes/depois da fatia de jobs. Desenho-alvo a validar ao abrir a
  fatia (mesma fonte): logs estruturados JSON numa pilha local consultável
  pelo agente — transforma metas tipo "inicialização < 800 ms" em tarefas
  verificáveis.
- **CLI `studio reset-password`** (ADR-0001 T4).
- **Gate aceita `sub` órfão** (ADR-0002 T8): cookie assinado com segredo
  antigo sobrevive a reset de `users` e passa a ler/deletar datasets;
  mitigação = `SELECT EXISTS` no gate ou rotacionar segredo no reset.

**Verificação / toolchain**

- **`cargo fmt -p api-principal`** viola o padrão (hunks pré-existentes da
  Fatia 2 + acréscimo da 3b); nenhum CI de fmt — decisão de quando formatar é
  do usuário.
- **`e2e-smoke.sh`**: confirmar cobertura do bloco `--datasets`.

**Infra / ambiente**

- ~~**Fixar digests de imagem no compose/Dockerfiles**~~ **QUITADA 2026-09-05**
  (decisão do usuário; commit `4b8c7e4` em `chore/pin-digests` — aguardando merge;
  despacho `@rust-dev` com digests resolvidos no registry pelo coordenador; 11/11
  referências pinadas, `compose config -q` verde nos dois arquivos). Padrão
  `tag@sha256:` mantém a tag legível e o digest como trava.
- **Nota para a fatia 4 (nova)**: `manager`/`orchestrator` rodam runtime
  `debian:bookworm-slim` (GLIBC 2.36) — quando o orquestrador ganhar cliente S3
  (aws-lc-sys, GLIBC_2.38), migrar para `trixie-slim` como o principal (R10 da
  ADR-0003). Os 3 Dockerfiles Rust seguem compartilhando o digest do builder
  `rust:1.97.1-slim`.

**Frontend**

- **Testes de UI** (backlog §12 do `frontend.md`) — cobrir
  criar→listar→excluir quando o e2e for ampliado.
- ~~**Sincronizar `docs/frontend.md` linha 3** — ainda descreve o protótipo como
  "~2910 linhas"; o do tronco é a regeneração OpenDesign (3641 linhas, com
  LoginPage).~~ **QUITADA 2026-09-06** (commit 3g.6 docs-sync: linha 3 agora
  diz "3641 linhas", conferido com `wc -l ai-vision-training-studio.html`).

## Quitadas

- **T7 — `autoTracked` derivado — QUITADA 2026-09-06** (`61dc75b` em
  `feat/datasets-gallery`, 3d.1): campo do wire `Dataset` agora deriva de
  `EXISTS(boxes.origin='autotracker')` nas 3 queries de `handlers.rs`
  (`DatasetRow.auto_tracked` + teste de integração com 3 casos: autotracker→
  true, manual→false, vazio→false). Sem mudança de contrato (campo já existia; 
  descrição do openapi atualizada).

- **3b (upload/imagens) — QUITADA 2026-09-05** (`feat/datasets-storage`,
  `f6c6ff5`..`393163c`; docs no 3b.8). Entregue: bucket S3/SeaweedFS como blob
  canônico (volume `datasets`/`DATASETS_DIR` do principal mortos; sweep de
  prefixo `datasets/<id>/` pós-commit best-effort), `DefaultBodyLimit`
  dedicado (200 MiB total + 8 MiB + teto por arquivo, envelope — ADR-0002
  T10), `images.object_key` + `sha256`/`media_type`, `videos` sem rota de
  escrita, `heph_refresh_dataset_counters` (nunca `+=`, status derivado),
  `source` derivado `s3://{bucket}/datasets/{id}/`, `classes` como
  `{id,name,idx,color}`. Sobrou como PLANO (não dívida): export/import/package
  → 3e (backup interim = console + `mc mirror`); UI `/datasets` (3c, feita);
  galeria/anotação (3d, próxima).
