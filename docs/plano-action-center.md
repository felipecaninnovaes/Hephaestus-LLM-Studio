# Plano — Action Center (melhorias AC-001..AC-006)

Data: 2026-09-16 · Estado: **PLANEJADO, NADA IMPLEMENTADO**
Origem: levantamento do usuário (6 pontos). Causas-raiz provadas por leitura do
código do tronco (`main`); file:line citados em cada item.

Escala: é uma fatia de UX/observabilidade com 1 componente de backend novo
(limpeza de jobs) e 1 decisão de contrato (status vs. métricas). Sem migrations
de schema salvo decisão em AC-006-A (coluna `status` no job — ver alternativa).

---

## AC-001 — Galeria de amostras: ordem, volume e colapso

**Sintoma**: amostras fora de ordem de época; acumula centenas de imagens em
treinos longos; sem colapso.

**Causa provada**:
- `get_job_artifacts` (manager) não tem `ORDER BY` → ordem arbitrária do
  Postgres: `services/manager/src/lib.rs:878-879`.
- `JobSamplesGallery` renderiza o array como chega: sem ordenar, sem agrupar,
  sem teto/colapso: `apps/web/components/studio/JobSamplesGallery.tsx:24-35`
  (filtro) e grid `:55-60`. Já existe `formatSampleLabel` que extrai
  `epoch_(\d+)` (`:38-46`) mas só para rótulo, nunca para ordenar.
- Usada em 2 lugares: ActionCenter (`ActionCenter.tsx:1179`) e página
  `/jobs` (`apps/web/app/(studio)/jobs/page.tsx:878`) — a correção vale para as duas.

**Plano**:
1. Manager: `ORDER BY path, id` na query de artifacts (determinismo barato; os
   nomes são `sample_epoch_%NNN` zero-padded, então ordem lexicográfica ≈
   cronológica). `services/manager/src/lib.rs::get_job_artifacts`.
2. `JobSamplesGallery`:
   - ordenar numericamente pelo epoch parseado (fallback: ordem do servidor;
     baseline "Época 0" sempre primeiro);
   - separar "Baseline (pré-treino)" e "Amostras por época";
   - **colapsada por padrão** mostrando as últimas N (sugestão: 6) + "Mostrar
     todas (K)" / "Recolher"; cabeçalho com contador `(K amostras)`;
   - manter zoom modal existente.
3. Sem mudança de contrato/API além do ORDER BY.

**Verificação**: unit do gallery com array embaralhado (ordenar + colapsar);
smoke Chrome em job de difusão real.

---

## AC-002 — Painel modular e adaptável por tipo de job

**Sintoma**: funções sem métrica de treino (AutoLabel) exibem gráficos/chips de
métricas que não existem; o painel não é modular.

**Causa provada**:
- ActionCenter renderiza sempre: chips de "Métricas" para qualquer
  `latestMetric` (`ActionCenter.tsx:1109-1175`) — para AutoLabel cai no ramo
  YOLO e mostra **zeros fabricados** (mAP50 0.0%, Box Loss 0.000), porque o
  engine AutoLabel emite por imagem `box_loss:0.0,...,mAP50:0.0`
  (`engines/trainer-yolo/src/trainer_yolo/autolabel.py:443-452`).
- `/jobs`: `ConvergenceChart` renderiza se `kind===yolo_train ||
  metrics.length>0` (`jobs/page.tsx:777-786`) — AutoLabel tem metrics →
  gráfico de curvas zeradas. AutoTracker real não emite metrics.jsonl
  (`autotrack.py:369`), mas o mock emite 1 linha (`:264-281`).
- Não há registro de capacidades por kind; a lógica está espalhada em ternários
  de `job.kind/job.engine`.

**Plano**:
1. Criar `apps/web/lib/jobCapabilities.ts` — mapa kind/engine → painéis:
   | capability | yolo_train | diffusion_train | autolabel | autotracker | yolo_predict | diffusion_generate |
   |---|---|---|---|---|---|---|
   | progressLive | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
   | metricChips  | mAP/loss | loss/lr/step/época | ✗ (só "N imagens") | ✗ | ✗ | ✗ |
   | convergenceChart | ✓ | ✓ | ✗ | ✗ | ✗ | ✗ |
   | samplesGallery | ✗ | ✓ | ✗ | ✗ | ✓ (predições) | ✓ |
   | applyAction | ✗ | ✗ | ✓ | ✓ | ✗ | ✗ |
   | rerun | ✓ | ✓ | ✓ | ✓ | ✗ | ✗ |
   (a matrix final é decisão do usuário; acima é o padrão derivado do que os
   engines realmente emitem.)
2. Refatorar o bloco expandido do ActionCenter e o painel de detalhe do `/jobs`
   para consumirem o registry (um único helper compartilhado — evita drift).
3. Chips de AutoLabel/AutoTracker: "Imagens processadas N/M" derivado de
   `job.progress/job.step`, não do array metrics.
4. Backend inalterado (dados já vêm com `kind`/`engine`).

**Verificação**: unit do registry por kind; smoke Chrome com um job de cada tipo.

---

## AC-003 — Limpar jobs antigos

**Sintoma**: não há como remover jobs do histórico.

**Causa provada**: não existe DELETE em lugar nenhum — rotas de jobs são 7 de
leitura + submits + abort (`services/api-principal/src/auth/routes.rs:352-409`);
manager não tem delete de job (`services/manager/src/main.rs:547-581` só tem o
DELETE interno de `job_artifacts` na resubmissão de done). FK
`job_artifacts.job_id ON DELETE CASCADE` já existe
(`services/api-principal/migrations/0006_jobs.sql`), e os objetos S3 vivem sob
`artifacts/{job_id}/...` (convenção D8; orchestrator
`services/orchestrator/src/lib.rs:1597,1650`).

**Plano (fatia de 3 commits, na ordem de dependência)**:
1. **manager**: `DELETE /internal/jobs/:id` — guarda: só estados terminais
   (`done|failed|cancelled`), senão `409 job_not_terminal`; retorna a linha
   deletada (paths dos artifacts) para o sweep. E `POST /internal/jobs/cleanup`
   com corpo `{ olderThanDays?, statuses? }` → apaga em lote e devolve
   `{ deleted: n, jobIds: [...], objectKeys: [...] }`.
2. **principal** (BFF, padrão existente): `DELETE /api/jobs/:id` e
   `POST /api/jobs/cleanup` → chamam manager e depois fazem **sweep best-effort
   do prefixo S3 `artifacts/{jobId}/`** pós-commit (mesma ordem
   objeto→linha→compensação da ADR-0003 D6/D7 invertida: linha primeiro pois o
   manager é dono, objeto depois; falha de sweep logada, não 500). Erros novos:
   nenhum (reusa `not_found`, 409 `job_not_terminal`, `storage_unavailable`
   apenas no caminho de sweep se decidir propagar — recomendação: não
   propagar). bump openapi 0.26.0 → minor.
3. **web**: lixeira por job na lista do ActionCenter e do `/jobs` (visível só
   em terminais, com ConfirmDialog) + botão "Limpar antigos…" (diálogo com
   critérios: concluídos/falhos/cancelados há mais de N dias) + toast com
   contagem. 
4. docs-sync: backend.md §9 (rotas), frontend.md §10, contracts.

**Decisões abertas (confirmar com o usuário)**:
- D-a: cleanup expurga artifacts S3 junto? (recomendado: sim — senão vaza
  storage; modelos `done` podem ser re-treinados, mas são reprodutíveis).
- D-b: precisa preservar "favoritos/fixados"? (recomendado: não na v1).
- D-c: auto-retenção periódica (cron no manager) — **fora de escopo v1**.

**Verificação**: unit manager (guard de estado), contract principal (401/404/409),
teste db com sweep em MockStorage, smoke E2E com cleanup em job de teste.

---

## AC-004 — "Tela cheia" para acompanhar um único job + persistir no refresh

**Sintoma**: usuário quer focar num job; ao atualizar a página perde o contexto
e "caça o job de novo".

**Causa provada**:
- `/jobs` lê `?job=` na entrada (`jobs/page.tsx:102-108`) mas **nunca escreve**
  a seleção de volta na URL (`setSelectedJobId` local, `:90,112` do lado da
  lista) — refresh sem seleção cai no fallback `activeJobs[0] ??
  terminalJobs[0]`.
- Bug adicional: o ActionCenter navega para `/jobs?selected=${job.id}`
  (`ActionCenter.tsx:498,605,614,623`) mas a página só lê `job` — o deep link
  está **morto hoje**.

**Plano**:
1. Corrigir `?selected=` → `?job=` (todos os 4 links).
2. Ao selecionar job, `router.replace(`/jobs?job=${id}`)` (e `?job=&` limpo ao
   "voltar ao ativo") — seleção sobrevive a refresh e o link vira
   compartilhável.
3. Modo **foco** (`/jobs?job=ID&focus=1`): esconde a coluna-lista, painel de
   detalhe ocupa a largura, com botão "Sair do foco"; mantém o polling/SSE
   existentes (`useJobTelemetry`, `JobProgressLive` já ativos por job
   selecionado). Botão "Acompanhar / Tela cheia" no cabeçalho do job expandido
   do ActionCenter leva a essa URL.
4. Alternativa descartada: rota dedicada `/jobs/[id]` — duplicaria a lógica de
   polling/métricos da página atual sem ganho; URL-query resolve o caso e custa
   ~3× menos.

**Verificação**: unit/route test da sincronização URL↔estado; smoke Chrome:
abrir do drawer → refresh → continua no mesmo job em foco.

---

## AC-005 — Mockups/fabricações ainda no drawer do Action Center

**Sintoma**: "ficou muitos mockups" na lateral do action center.

**Auditoria do que é fabricado hoje (ActionCenter + JobLogViewer)**:
1. Notificação fixa "Orquestrador Local Ativo — Nó Hephaestus operacional…"
   despejada a cada poll com timestamp `new Date()`
   (`ActionCenter.tsx:473-482`).
2. Notificação ficcional "Armazenamento & Datasets — Volumes … sincronizados no
   cache NVMe local" com timestamp falso de −30min (`:503-512`) — nenhuma
   sincronização de dataset existe.
3. Rodapé `Nó Local: Operacional` quando `telemetry === null` — afirma saúde
   sem dado (`:666-675`).
4. Empty state promete "sincronizações de datasets e alertas de telemetria dos
   nós" (`:761-763`).
5. `JobLogViewer`: linhas de boot fabricadas ("Container montado:
   /outputs/…", "YOLO Engine inicializado…") com timestamps fictícios
   (`fmtTime(0/1/2)` e `+2s` por métrica — hora inventada):
   `apps/web/components/studio/JobLogViewer.tsx:56-105,117`.

**Plano**:
1. Remover (1) e (2) por completo — o bloco de notificações fica apenas com
   derivadas reais: alertas VRAM/CPU (reais, derivados de telemetria), jobs
   falhados, e futuramente eventos de sistema (when exists).
2. Rodapé: quando sem telemetria → `—`/`offline`, nunca "Operacional".
3. Copy do empty state: descrever só o que existe (treinos, tarefas de
   anotação/rastreamento, alertas de recursos do nó).
4. JobLogViewer: manter a síntese **derivada de dados reais** (fase/mensagem,
   métricas) mas **sem relógio inventado** — esconder a coluna de timestamp
   quando a origem é sintetizada (ou exibir hora real apenas para as linhas
   que a têm). Revisar as 3 linhas de boot: remover ou reduzir a 1 linha
   honesta ("Job submetido · kind/engine/nó" — dados do próprio job).
5. Nota: a sidebar global (`Sidebar.tsx`) já está honesta (itens inexistentes
   com `isAvailable:false` + badge "Roadmap" + aria-disabled, `:233,275,288,296`)
   — fora do escopo deste item, nada a limpar.

**Verificação**: greps `Hephaestus operacional`/`NVMe` zerados; smoke Chrome do
drawer vazio e com job.

---

## AC-006 — Gráfico de steps/curvas contando mensagens de status como steps

**Sintoma**: no start o gráfico já tem ~10 "steps" (mensagens de carregamento
de modelo/dataset, amostra, início de treino) e depois zera e recomeça.

**Cadeia da causa (provada)**:
1. O engine difusão escreve **mensagens de status no mesmo `metrics.jsonl`**
   com `phase`+`message` e `epoch=0, step=1..10` sequencial
   (`engines/trainer-difusao/src/trainer_difusao/common.py:68-98` +
   `models/flux.py:391,473,502,549,568,597,639,668,710,761,782,804,827,836` —
   idem `sd15.py`/`sdxl.py`/`mock.py`). São ~11–14 linhas de boot.
2. O collector do orquestrador lê o arquivo incrementalmente e **reporta cada
   linha como métrica** ao manager (`services/orchestrator/src/lib.rs:1677-1724`);
   `parse_metrics_line` tolera linhas sem loss (epoch defaulta 0 quando há
   phase/progress, `:399-405`).
3. `upsert_metrics` do manager persiste tudo no JSONB `jobs.metrics`, dedup por
   `(epoch,step)` — steps 1..10 do boot são únicos e ficam
   (`services/manager/src/lib.rs:983-1024`). **Risco colateral provado**: a
   linha `training_started` tem `(epoch_offset, 10)`; se `epoch_offset=0` e o
   loop global_step passar por `(0,10)`, há sobrescrita silenciosa.
4. O frontend plota **todas** as entradas: `loss ?? 0` (`ConvergenceChart.tsx:160-174`),
   x por índice (`:143-153`), e o header conta `(N checkpoints)` (`:261-263`).
   O chip "Step" do ActionCenter usa `latestMetric.step`
   (`ActionCenter.tsx:1134-1138`) — por isso o contador vai a ~10 no boot e
   "volta para 0" quando o loop real reinicia a numeração do step.
   O que o usuário vê como "[DIFFUSION]" são exatamente essas linhas: o
   JobLogViewer renderiza cada uma com a tag `DIFFUSION`
   (`JobLogViewer.tsx:121-140`).

**Decisão de arquitetura (a ser travada em ADR curta ou nota de produto — 2 opções)**:

- **AC-006-A (definitiva, backend)**: separar **eventos de status** de
  **métricas de treino** na borda do orquestrador/manager:
  - Orquestrador: linha com `phase != null && sem campo de valor` (loss/box_loss/
    mAP) → vira **evento de status** (report separada: `progress`, `phase`,
    `message`, `epoch`, `step`) e **não** entra no array de métricas.
  - Manager: persistir o último status no job (coluna nova `status_event JSONB`
    ou reaproveitar `params` — **recomendação: coluna `phase`/`phase_message`
    derivadas no report**, espelhando o que a SSE `JobTelemetryEvent` já
    expõe); eventos históricos ficam fora de escopo v1.
  - Contrato wire: `/api/jobs/:id/metrics` passa a devolver **só pontos de
    dados**; `phase/phaseMessage` continuam no job/SSE. É mudança de
    comportamento de endpoint já especificado → bump minor openapi + nota no
    backend.md §9. Testes de contrato: linha phase some do array.
  - AutoLabel: as linhas por-imagem (zeros + `progress`) viram eventos de
    progresso → `jobs.metrics` fica vazio para autolabel (a UI do AC-002 já não
    mostraria gráfico de qualquer forma; contagem de imagens vem de
    `progress`/`step` do job).
- **AC-006-B (interim, só frontend — entra já, sobrevive ao A)**: derivar no
  selector compartilhado: `chartPoints = metrics.filter(m => m.loss != null ||
  (m.map50 > 0) || (m.boxLoss > 0) || (m.clsLoss > 0))`; contagem de
  checkpoints, chips e curva usam o filtro; linhas `phase` continuam
  exclusivamente no JobLogViewer (onde são desejadas). Barato, sem contrato,
  mas deixa o dado poluído no banco e o risco de colisão (epoch,step) do item 3.

**Recomendação do coordenador**: B imediato (AC-1) + A como slice própria (AC-4)
com ADR de meia página (contrato metrics.jsonl do engine não muda — é o
transporte; quem separa é o orquestrador).

**Verificação**: unit do filtro; unit do orquestrador (classificação
status×métrica); test-db do manager (métricas persistidas sem phase); E2E: job
diffusion mock → gráfico começa em 0 pontos e cresce só com épocas reais.

---

## Sequenciamento proposto em fatias (commits pequenos, 1 dispatch cada)

| Slice | Conteúdo | Dono | Dependências |
|---|---|---|---|
| **AC-1** (web) | AC-005 (limpar mockups), AC-006-B (filtro), AC-004 (URL + focus + fix `?selected=`), AC-001 item 2 (ordenação/colapso client) | @frontend-dev + @ui-designer | nenhuma |
| **AC-2** (web) | AC-002 (registry de capacidades; remove chips zerados de autolabel) | @frontend-dev | AC-1 (mesmos arquivos, sequencial) |
| **AC-3** (backend+web) | AC-003 cleanup (manager → principal+sweep → UI) | @rust-dev → @frontend-dev | nenhuma; openapi minor |
| **AC-4** (backend) | AC-006-A (sepiação status/métrica no orquestrador+manager; SSE já consome) | @rust-dev + nota de ADR | AC-2 preferencial antes (UI já não depende de phase-lines) |
| **AC-5** (backend+web) | AC-007 (progresso de staging no canal de status + cache conteudo-endereçado no nó + UI de sync) | @rust-dev → @frontend-dev | **AC-4** (usa o canal status criado lá); AC-3 divide manager/principal — sequencial |
| docs-sync | backend.md §9/§10, frontend.md §10, dividas.md | @docs-sync | fim de cada slice |

Regras da casa: branch `feat/action-center-polish` para AC-1/AC-2; AC-3 e AC-4
em branches próprias (`feat/jobs-cleanup`, `feat/jobs-status-metrics-split`) —
são domínios diferentes (manager/principal/orchestrator). `cargo fmt --all
--check` em todo commit rust. CI dispara no push da branch.

**Atenção**: se o branch de redesign do front (Sessão 2026-09-08) ainda estiver
vivo e mexer em `ActionCenter.tsx`/`jobs/page.tsx`, AC-1/AC-2 rebasem nele —
confirmar com o usuário antes de abrir as fatias.

**Decisões FECHADAS pelo usuário (2026-09-16 — todas "Sim" para o recomendado)**:
1. AC-002: matriz de capacidades aprovada como rascunhada (AutoTracker/Predict
   com galeria de previews onde houver artefato de imagem).
2. AC-003: D-a sim (expurgo S3 junto com a linha); D-b sim (sem favoritos na
   v1); auto-retenção periódica fora da v1.
3. AC-004: sim ao modo foco por query (`/jobs?job=ID&focus=1`); sem rota
   `/jobs/[id]`.
4. AC-006: sim ao B agora + A depois (filtro interim no frontend + separação
   status×métrica como slice própria).
5. AC-005: registrado como — linhas de boot reduzidas a **1 linha honesta**
   derivada dos dados reais do job (submetido · kind/engine/nó) e timestamps
   sintetizados **ocultos** (coluna "—" quando a origem é derivada), não
   removida a coluna inteira. Corrigir se o usuário quis outra coisa.

---

## AC-007 — Progresso da sincronização de conteúdo com o nó (dataset/pesos) + cache

**Pedido do usuário**: exibir o progresso de envio/materialização de conteúdo
no nó (dataset, custom models), hoje invisível. Pergunta correlata: *custom
models são enviados a todo momento ou ficam em cache no nó?*

**Resposta apurada (estado atual, provado)**:
- **Upload browser→servidor**: 1× só — o modelo custom mora no S3 sob
  `models/...` (migration `0007_models.sql`/`0010_models_engines.sql`).
- **Nó→S3 (materialização do job)**: **re-baixado a TODO job, sem cache entre
  jobs** — dataset zip (`services/orchestrator/src/lib.rs:1102-1123`), pesos
  fine-tune (`:1128-1171`), LoRAs multi-ref (`:1182-1208`) e custom checkpoint
  (`:1213-1242`), todos `s3.get_to_file` + MD5 staging por
  `outputs/{job_id}/weights/` e `datasets-cache/{job_id}/`. O mesmo dataset em
  5 treinos = 5 downloads integrais. Os arquivos ficam no volume após o job e
  **nunca são reaproveitados nem apagados** (comentário
  `lib.rs:2128` "datasets-cache e outputs persistem") — logo, o AC-003 (limpar
  jobs) também deve alcançar o cache local do nó.
- **Único cache real hoje**: modelos-base HuggingFace (FLUX/SDXL do hub) em
  volume persistente `/outputs/.cache/huggingface` (`lib.rs:1502-1522`) — por
  isso o 1º uso de um base model é lento e os seguintes não.
- **Progresso durante staging**: inexistente — o orquestrador reporta só
  `status:"preparing"` (`lib.rs:1076-1092`) e fica mudo durante todo o
  download (minutos em dataset GB). No front o status nem é tipado:
  `JobStatus` não inclui `dispatched`/`preparing`
  (`apps/web/types/studio.ts:185-191`) e o teste `isActive` dos dois painéis
  (`running|queued|cancelling`) os ignora → nenhum ProgressBar aparece na
  janela inteira de sincronização. O `PackageRef.bytes` já existe
  (`lib.rs:76-80`); `WeightRef` **não tem** `bytes` (`:43-48`).
- **Build do pacote no submit** (principal → S3) é síncrono ao POST
  (`handlers.rs:840,1005,1281,1435,1783` + `datasets/package.rs::build_package`)
  — dataset grande = botão de submit travado sem feedback (v1: ao menos
  indicador no dialog de submit; build assíncrono é backlog).

**Plano (2 commits + 1 de UI)**:
1. **Contrato/orquestrador — progresso de staging no canal de status**
   (consumidor nº 1 da separação status×métrica decidida no AC-006):
   - `WeightRef` ganha `bytes: Option<i64>` (o manager já sabe o tamanho pelo
     registro `models`/`job_artifacts` no dispatch);
   - `S3Port::get_to_file` vira streaming com callback de bytes (ou task de
     polling do tamanho em disco); relatório periódico (~2s) com
     `phase = staging_dataset|staging_weights|staging_loras|staging_custom`,
     `progress = bytesDone/bytesTotal` (total = package + Σ refs) e
     `message` legível ("Baixando dataset.zip · 1,2/3,5 GB");
   - manager aceita report de progresso em status `preparing` (hoje a UPDATE
     com progress só roda no caminho running —
     `services/manager/src/lib.rs:1071`).
2. **Cache conteudo-endereçado no nó — DECIDIDO pelo usuário (2026-09-16)**:
   "mapeamento fixo, não efêmero; verifica o hash e reaproveita; só re-baixa se
   houver qualquer alteração". Desenho:
   - Chaves: `datasets-cache/md5/{md5_zip}/` (árvore descompactada) e
     `weights-cache/md5/{md5}/<arquivo>` (pesos, LoRAs, custom checkpoint) —
     vive no volume persistente do nó, sobrevive a restart e a jobs.
   - **Integridade barata + self-healing**: download sempre para
     `{key}.part` → verifica MD5 → rename atômico + marker `.verified`
     (md5, bytes, last_access). Reuso = checagem de marker (O(1), sem
     re-hash de GBs a cada job); marker ausente/corrompido → re-hash do
     arquivo local → só re-baixa em mismatch. Contente mudou ⇒ md5 mudou ⇒
     chave nova ⇒ re-baixa automaticamente; a entrada antiga vira órfã e é
     expurgada pela eviction.
   - Staging do job não copia bytes: hardlink/symlink da entrada canônica do
     cache para `datasets-cache/{job_id}`/`outputs/{job_id}/weights/` (mesmo
     volume; fallback copy se cross-device) — o path esperado pelo trainer
     (`/datasets/datasets-cache/{job_id}`, §config) não muda.
   - **Eviction (necessária — substitui o vazamento atual)**: varredura LRU
     por `last_access` com teto de disco configurável no nó
     (ex.: ORCH_CACHE_MAX_GB; default conservador) + expurgo imediato quando
     o AC-003 apagar o job que era o último usuário da chave — sem eviction,
     o volume cresce para sempre (problema que já existe hoje, `lib.rs:2128`).
   - Geração de pacote 3e: md5_zip já muda com qualquer edição do dataset
     (build no submit) → invalidação cai de podre, sem chave extra.
   - Report de staging ganha `phase=cache_hit` (message "Dataset pronto em
     cache (md5 …) · 0 bytes baixados") para o chip da UI.
3. **Web**: tipar `dispatched`/`preparing` em `JobStatus` + `STATUS_CONFIG`;
   incluí-los no `isActive` (ActionCenter e `/jobs`); render do
   `JobProgressLive`/ProgressBar durante staging com o phase/progress do
   commit 1; chip "(cache hit)" quando o nó reportar staging pulado
   (message própria no evento).

**Alternativas descartadas**: proxy de download via manager (não resolve — o
gargalo é nó→S3); presigned browser→node (R3, fora da topologia).

**Verificação**: unit do orquestrador (parser de fases de staging + skip por
marker); test-db do manager (progress persistido em preparing); E2E mock:
job de dataset conhecido → UI mostra "Sincronizando conteúdo · N%" antes de
running; 2º job com o mesmo dataset → cache hit (log do nó reporta skip, 0
bytes baixados).
