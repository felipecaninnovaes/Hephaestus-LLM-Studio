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
  credencial escopada por prefixo. (O `POST /:id/package` também é da
  fatia 4 — revisão D0 da ADR-0006 sobre o ADR-0003 D9.)
- **M2 — lacunas de teste do export/import (re-auditoria do reviewer da 3e,
  sem fatia marcada)**: (a) nenhum teste abre o zip gerado PELO HANDLER (o
  unit cobre `build_label_entries` puro; o loop real com `a.jpg`+`a.png` +
  zip final sem teste); (b) skip de órfão sem teste-db; (c) fixture de
  dedupe pós-sniff e teto de reader contado em zip real, sem fixture.
- **Borda composta em `label_arcname_for` (re-auditoria do reviewer da 3e,
  sem fatia marcada)** — `src/datasets/export.rs:292`: `used.insert` ignora
  o retorno — o caso de 3 vias (`a.jpg`+`a_png.jpg`+`a.png` na ordem criada)
  ainda produz arcname duplicado; correção = loop de sufixo + teste 3-vias.
  Raríssimo; roundtrip interno não afetado (import ignora labels).
- **SELECTs pós-commit do import sem cleanup (re-auditoria do reviewer da
  3e, sem fatia marcada)** — `src/datasets/import.rs:637-645` (mapa
  class_idx→id) e `:761-784` (SELECT final do dataset + classes): falha de
  leitura aí deixa o dataset novo commitado (vazio/completo) com 500;
  all-or-nothing estrito chamaria `cleanup_failed_import` nesses retornos
  também.
- **NIT a11y do ImportDatasetModal (re-auditoria do reviewer da 3e, sem
  fatia marcada)**: o foco não migra para o botão "Substituir" na fase
  `confirm` (padrão do ConfirmDialog — consistente, registrável).
- **NIT RAM do import (re-auditoria do reviewer da 3e, sem fatia
  marcada)**: `extract_images` lê a imagem inteira em memória para
  sniff/decode (teto 200 MiB/imagem limita; a 3b spoola sem acumular).

### Sem fatia marcada

**Backend**

- **Abort races (review F4.8):** abort durante `preparing` pode ser engolido (estado substituído pelo progress report do orquestrador; `is_cancelled` não é consultado no pipeline — dead code); abort em `dispatched` não notifica o orquestrador (o dispatch já foi feito mas o orquestrador pode estar starting); abort em voo termina `failed` (nunca `cancelled` — o orquestrador reporta `failed` com erro "job not found or already finished" quando o container é stopado). Janela de segundos em mock local; conserto exige testes de abort-em-voo.
- **Watchdog de orquestrador (review F4.8):** orquestrador morto nunca fica `offline` (dispatch tenta para sempre em single-orchestrator — o health-check 15s/5 falhas da ADR-0007 D3 não está implementado no manager v1; o `status` de `orchestrators` permanece `online` mesmo sem heartbeat).
- **CI: pytest do trainer-yolo (review F4.8):** contrato das 6 keys do `metrics.jsonl` (`epoch, box_loss, cls_loss, dfl_loss, mAP50, mAP50-95`) com o parser do orquestrador (`parse_metrics_line`) não roda em CI — job Python ausente em `.gitea/workflows/ci.yml`. Validado manualmente no E2E.
- **Mock: best.pt e last.pt idênticos (review F4.8):** artefatos deterministas (bytes de cabeçalho, mesmos 32 bytes) — diferenciar por mAP50 se a UI consumir histograma ou comparação entre best/last.
- **Reports de métrica best-effort (review F4.8):** `let _ =` sem retry no orquestrador (`lib.rs` — report de progresso/métricas); terminal de erro agora tem log (`tracing::warn`), mas falha do manager no momento da falha prende o job até recovery no boot.
- **Defaults de token inconsistentes (review F4.8):** manager aceita `manager-dev-token` como default (`MANAGER_TOKEN` env); orquestrador bypassa auth sem `MANAGER_TOKEN` (Bearer vazio aceito na v1 local); principal fail-fast sem `MANAGER_TOKEN`. Sem fail-fast uniforme entre os 3.
- **`unzip_safe` não rejeita separador `\` (review F4.8):** divergência com `validate_artifact_path` do principal (que aceita `\`); inócuo em Linux mas viola defesa em profundidade.
- **NITs frontend (review F4.8):**
  - ~~Highlight de módulo ativo da Sidebar hardcoded em "Dados & Anotação" (sem `usePathname` — o módulo ativo NÃO muda quando o usuário navega para `/jobs`).~~ **QUITADA 2026-09-08** (Fatia 5, commit `61492ea`: `usePathname` destaca módulo ativo via `pathname` no `Sidebar.tsx`).
  - Duplicação tripla de `canTrain`/`trainDisabledReason` (lógica de condição de treino repetida no botão da galeria, no `DatasetMenu` e na galeria de datasets).
  - Toast "Job cancelado." no 200 quando o estado real é `cancelling` (o handler retorna `{"status":"cancelling"}` mas a UI pode exibir mensagem genérica).
  - Telemetria pollada a cada 3s mesmo com drawer da Sidebar fechado (consumo de requests desnecessário quando o card não está visível).
- **SubprocessExecutor = stub honesto (review F4.8):** `EXEC_MODE=subprocess` implementado mas é stub (só `subprocess.run`, sem progress streaming). RunPod = fatia futura. Se já não estiver em dividas.md, registrado aqui.
- **GC automático da lixeira (TTL) — ABERTA 2026-09-06** (fatia futura, fora da
  3g por D9 da ADR-0005): hoje só purge manual via UI (`DELETE /:id/trash`);
  `images.deleted_at` já dá o dado (idade do soft delete); falta TTL +
  purge agendado (job/cron que DELETE físico + sweep das vencidas).
- **Logging server-side (Fatia 4 parcial — review F4.8):** `tracing` + `tracing-subscriber` com formatter JSON + `x-request-id` + `/ready` entraram nos 3 serviços Rust (D11 ADR-0007); **pendente:** buffer de logs 1000/2000 linhas, `?since_seq`, `WS` de logs (fatia de logs/WS).
- **CLI `studio reset-password`** (ADR-0001 T4).
- **Gate aceita `sub` órfão** (ADR-0002 T8): cookie assinado com segredo
  antigo sobrevive a reset de `users` e passa a ler/deletar datasets;
  mitigação = `SELECT EXISTS` no gate ou rotacionar segredo no reset.
- **Ordem estável de `boxes` entre saves (nota menor da 3d)** — o PUT boxes é
  `DELETE`+`INSERT` com `RETURNING`: os ids nascem novos a cada save e
  `GET detail` não garante ordem. A UI correlaciona seleção por índice de
  payload pós-save (correto), mas os chips `#N` no canvas podem reordenar
  entre loads. Correção real = coluna de ordenação/`ORDER BY` determinístico
  no backend — só valerá a pena quando o autotracker consertar caixas
  existentes, hoje os efeitos são cosméticos. **Reafirmação (Fatia 5):** o apply do autotracker não piora a ordem semanticamente porque o merge por origem é atômico por imagem — mas a dívida permanece aberta para quando o autotracker consertar caixas existentes.
- **R1 drift de classes snapshot→apply (curta, Fatia 5):** o snapshot (`SnapshotClass{index,name}`, sem id) congela classes do pacote; no apply, o principal resolve por `name` no dataset atual. Se uma classe foi renomeada/deletada entre job e apply → box skippada (contagem honesta). Aceito na v1 mock; alternativa futura: snapshot com id de classe + decisão de re-mapear.
- **AutoTracker de vídeo — ABERTA 2026-09-08 (ADR-0008 D5):** extração de frames + tracking por `track_id` (`boxes.track_id` já existe desde 0003) + rota de escrita de `videos` (tabela existe sem rota de escrita desde 3b). IDEIA.md §3/:42 pede "Video e Imagem"; a v1 é imagem apenas.
- **AutoTracker real — ABERTA 2026-09-08 (ADR-0008 D6):** modelo local (`florence-2-large`, `yolov8x-world`, `qwen2-vl-7b` em frontend.md §7.1) + upload de modelo + imagem `runner-autotracker` própria (honra backend.md §4 uma-imagem-por-engine) + classe open-set mapeada para classes existentes/adicionadas. O v1 usa mock determinístico (`ENGINE_MOCK=1`) sem modelo real.

**Verificação / toolchain**

- **test-db.sh apaga estado de produto quando a stack está de pé — ABERTA 2026-09-08**
  (causa-raiz achada na fatia 5): o script roda os testes do manager
  (`manager_db`) no MESMO banco `studio` do compose (localhost:5432), e os
  fixtures fazem `DELETE FROM orchestrators` (e limpeza de jobs) — cada
  `bash scripts/test-db.sh` com a stack de pé invalida a auto-adoção e
  deixa dispatches presos em `waiting_slot` até um restart do manager
  (que re-adota no boot). Sintoma: `orchestrators` vazio + jobs em
  `waiting_slot` sem despacho. Conserto certo: isolar os testes-db num
  banco efêmero próprio (ex.: `studio_test` criado + migrations aplicadas
  + drop no fim), nunca o banco do produto. Mitigação de curto prazo:
  restart do manager após rodar test-db.
- **Teste `@gpu` manual do CLIP real (fatia futura, sem número) — ABERTA 2026-09-06**
  (spike/ADR-0004): o modo real do embedder (`ENGINE_MOCK` desligado,
  `open_clip_torch` ViT-B-32, peso ~600 MB fora do compose) nunca rodou em GPU;
  manual, não bloqueia fatias.
- **Planner do pgvector prefere seq scan com ~10k linhas (sem fatia) — ABERTA
  2026-09-06** (spike): custo do LIMIT pequeno faz o planner ignorar o HNSW;
  NÃO forçar `enable_seqscan=off`; re-avaliar com datasets reais grandes.

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
  - **Parte `db` re-honrada na 3f (2026-09-06, `feat/semantic-search`):** a imagem
    do `db` trocou `postgres:16` → `pgvector/pgvector:pg16-trixie@sha256:c8483555…`
    (spike provou upgrade sem dump/restore; `pg16` sem sufixo é bookworm →
    collation mismatch). Só a parte postgres/db está paga aqui; o item como um
    todo segue quitado acima.
- **Nota (atualizada Fatia 4):** o **orquestrador** migrou para `trixie-slim`
  (`rust:1.97.1-slim` builder, `debian:trixie-slim` runtime — GLIBC 2.41 para
  aws-lc-sys); o **manager** mantém `bookworm-slim` (sem S3 na v1). Os 3
  Dockerfiles Rust seguem compartilhando o digest do builder.

**Frontend**

- **Paleta de classes no backend desalinhada do v2 — ABERTA 2026-09-07**
  (origem: fatia redesign UI v2): cores de classes nascem no Postgres com a
  paleta da v1 (`services/api-principal/src/datasets/models.rs:11-18`
  `CLASS_PALETTE`, cabeça `#10b981` emerald + `#f59e0b` amber) e o editor
  renderiza `cls.color` como veio do banco. É dado, não estilo — realinhar a
  rotação de cores no backend numa fatia futura (decidir: migration dos
  valores existentes + enum novo).
- **Canvas do editor com navy v1 hardcoded — ABERTA 2026-09-07**
  (origem: fatia redesign UI v2): `apps/web/app/(studio)/datasets/[id]/
  annotate/[imageId]/page.tsx:670` usa `bg-[#0b0f17]` pré-existente (fora do
  token v2 `zinc-950`/`--bg`). Limpeza trivial em fatia de manutenção.
- ~~**Telemetria real da sidebar — ABERTA 2026-09-07~~ **QUITADA 2026-09-08**
  (Fatia 4, `feat/jobs-v1`): card TELEMETRIA DO NÓ agora faz polling
  `getTelemetry()` a cada 3s (`Sidebar.tsx:30-48`), preenche VRAM/CPU/RAM
  (barras `brand`); VRAM `"sem GPU (mock)"` quando `!measured`; remove
  `title="Telemetria chega na fatia 4"`.
- **Contraste do CTA documentado (NÃO é dívida — decisão da fatia redesign
  UI v2)**: CTA em `brand-500` (`#8350f2`) + `text-white` ≈ 4.78:1 — passa
  WCAG AA (texto normal, 4.5:1); piso mínimo, ver nota em
  `docs/design-system.md` (Components → Buttons). Registrado aqui só como
  referência contra re-litígios de contraste.
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

- **3e (export/import de dataset) — QUITADA 2026-09-07**
  (`feat/datasets-export-import`: export `21b95b9`, import `f37d76f`, UI
  `44cf475`, fixes `71b5587`/`23e0f3e`/`b07c215`/`096eb95`; review final
  APROVA COM NITS). Entregue: `POST /:id/export` (200 401 404 503, zip em
  stream do banco em tempdir) + `POST /datasets/import` (201 400 401 409
  503, multipart `file`+`title`≤96+`replace`, substituição consentida
  409→`replace=true`, erro novo `import_invalid`, limite 200 MiB + 8 MiB
  envelope); UI na galeria (`lib/backup.ts`, `ImportDatasetModal`,
  Importar removido do header). `POST /:id/package` + `dataset_versions`
  (T4) ficam na fatia 4. Sem migration (schema já tinha `origin` com
  `import` desde a 0003 — §10 intocado).
  - Nota: a ordem de boxes no wire segue não-determinística (`ORDER BY id`
    UUID — dívida "Ordem estável de `boxes`" acima, pré-existente e
    reafirmada, não duplicada; o teste da 3e compara por identidade).

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
