# tasks/todo.md — memória ativa do coordenador

Substitui `docs/coordenacao.md` como estado de sessão (padrão "Documentar e
Limpar"). Histórico fóssil arquivado em
`docs/archive/coordenacao-historico-2026-09.md` — **nunca** carregar o
arquivo de arquivamento no início de sessão; consultar pontualmente se um
registro antigo for necessário.

Atualizar: ao abrir fatia, ao fechar fatia, e ao ser interrompido no meio de
uma. Manter < 100 linhas; não duplicar docs — referenciar por seção.

## Protocolo de retomada (início de sessão)

1. Ler este arquivo (seção "Plano em andamento").
2. `git status` + `git log --oneline -5` para conferir se o disco bate com o
   registrado (branch aberta, commits pendentes de push/merge).
3. `graft check` se for mexer em código indexado (refresh: `graft build`).
4. Fontes de verdade: `IDEIA.md`, `docs/REPO_MAP.md` (topologia L1),
   `docs/backend.md` §9/§10, `docs/frontend.md` §10, `docs/repo-estrutura.md`
   (ordem de fatias), `docs/dividas.md` e os ADRs em `docs/adr/`.

## Estado atual — 2026-09-16 (fatia viva: design system web)

- **Fatias anteriores fechadas:** harness restructure mergeado; as 3 branches
  de UI (jobs-cleanup → action-center-polish → jobs-status-metrics-split) e
  `develop` entraram na `main` via PR #31.
- **Branch de trabalho:** `feat/web-design-tokens` (carrega `chore(agents):
  atualiza impeccable` na história via `chore/fix-web`).
- **Fatia em andamento:** extração de design system (`impeccable extract`):
  tokens `status-*` + micro-tipografia (`text-2xs/3xs/4xs`) em `globals.css`,
  componente `Spinner.tsx`, migração de ~503 ocorrências em 59 arquivos,
  docs `DESIGN.md` + `ui/README.md` v2.2.
  - [x] @reviewer: FECHAR (APROVA COM NITS); nit 4 (docs Spinner) fechado
  - [x] `npm run build` verde (gate final pós-correção docs)
  - [x] 4 commits atômicos (fundação → ui primitives → studio A-L → studio C-Y → páginas+consolidações) — fechados
  - [x] Fatia Biome: config + scripts + 11 autofixes + nits a11y/line-height + docs honestos (@reviewer CORRIGIR-ANTES→fixer→ok; lint 116E/204W = débito da follow-up 3)
  - [x] Slice dívida a11y: 66 errors→0 + follow-ups reviewer (StatCard props, Esc lightbox)
  - [x] Slice dívida leve: noArrayIndexKey 8 + noSvgWithoutTitle 7 + noAutofocus 3 (suprimido justificado) → restam 32E
  - [x] Slice dívida hooks: useExhaustiveDependencies 32 → 0 (FECHAR; reviewer aprovou, 2 achas corrigidas antes do commit)
  - [ ] Decisão UX: noAutofocus — RESOLVIDA: manter comportamento (login/dialogs), suprimido com justificativa
- **Follow-ups desta fatia (não bloqueiam):**
  1. Spinner `aria-hidden` explícito + `aria-busy` no Button (a11y).
  2. `--text-2xs/3xs/4xs` sem line-height própria (herda do contexto) —
     fixar token se introduzirmos `leading-*` nesses tamanhos.
  3. **Biome toolado; dívida de lint a zerar:** ~~66 a11y~~ FECHADO (slice
     a11y + follow-ups M1/M2 do reviewer). Restavam 50 → ZERO errors (204 warnings, 3 infos — gate 0-errors agora real):
     `useExhaustiveDependencies` 32 (risco comportamental — um commit por
     módulo, @reviewer em cada), `noArrayIndexKey` 8 + `noSvgWithoutTitle` 7
     (slice em andamento), `noAutofocus` 3 (decisão UX: login/busca querem
     autofocus? → suprimir justificado ou remover). Warnings: `noExplicitAny`
     90, `noUnusedImports` 46, `noNonNullAssertion` 22, `noImgElement` 19.
     Pendente pós-0-errors: gate de format/assist (~108 arqs), `import React`
     →type-only em Button, pin $schema vs `^`.
     Comando canônico: `npm run lint --workspace=web` (raiz). **rtk NÃO serve
     p/ lint** (corrompe saída) — chamar npm direto. Body de commit ≤100 col.
  4. Paleta de séries do ConvergenceChart (1 consumidor) — avaliar tokenização.
  5. Divergências cosméticas canonicadas pelo reviewer: raio CTA /jobs
     (lg→md), ghost DatasetTable (zinc-400→300), tom spinner brand (500→400).
- **Pendência:** merge/push desta branch só com ordem explícita do usuário.

## INCIDENTE (2026-09-17 01:09, causado por workers desta branch) — dev DB `studio` limpo
Um dispatch de teste (janela P4a/P4b) rodou a suíte `datasets_db.rs` com `DATABASE_URL`
apontando para o DB de dev `studio`; o setup da suíte faz DELETE limpeza e allow-list
antigo aceitava `db == "studio"` (datasets_db.rs:L114) → **dataset do usuário
`boys_big_dataset` (d67a0a52, 860 imgs, 3GB) perdido no DB**. S3/SeaweedFS INTACTO
(volumes heph-data* ok). usuário tem backup → re-importar. Correção de segurança em
andamento: harnesses de teste devem PANICAR se db != studio_test (guardia-mecanica).
Branch segue: commits ee5c1f4..ecda78b+6. E2E pesado adiado p/ dataset re-importado ou
sintético.

## Achado 2 FECHADO (2026-09-17) — import de zip >2GB quebra no INSPECTOR (pré-existente)
Sintoma: zip de 3GB no modal → DOMException "requested file could not be read..."
(1.3GB ok). Causa: `dataset-inspector.ts` lia o zip INTEIRO (`file.arrayBuffer()`) —
Chromium limita leitura de Blob a 2GB. O upload (FormData) nunca foi o gargalo.
Fix 140aa66: parser por faixas (cauda≤64KB p/ EOCD, CD fatiado, extração só do range
do entry) + ZIP64 completo (locator/record/extra-field u64). Validação: usuário testou
import real de 3GB → OK; ZIP64 validado com zip sintético 4,1GB (CD além de 4GB) via
port Node da mesma lógica. ⚠ Nota de harness: `chrome-devtools upload_file`
(CDP setFileInputFiles) entrega File FANTOMA (size 0) — "EOCD não encontrado" em teste
automatizado NÃO indica parser quebrado; usar seleção real do usuário.

## Fatia FECHADA (2026-09-17) — feat/jobs-async-submit: submit assíncrono (ADR-0025)
12 commits (ee5c1f4..cab2cf5) sobre develop@4026fe3. **E2E ao vivo aprovado**: dataset
sintético 865 imgs/4,6GB → submit via proxy :3000 = 202 `preparing` em 7-10ms (antes:
500@30002ms); packaging em background com progresso `packaging_dataset`; reuso por
fingerprint (<10s, dataset_versions sem duplicata); dedupe retorna mesmo jobId; apply de
865 captions ok. Gates: cargo test --workspace 619p + test-db.sh verde; build/lint web
0 errors novos; reviewer FECHAR em 3 rounds (bloqueantes manager+B1-B4 e dedupe/panic
P4a todos resolvidos).
- **Bugs extras caçados no teste pesado**: (1) `operation_timeout` S3 de 5s matava PUT
  multi-GB (fix bd99e9c: 60min; fail-fast real = connect 2s + read 30s); (2) `infra/.env`
  apontava `TRAINER_IMAGE=:gpu` em máquina CPU-only (corrigido local p/ `:local`, não
  versionado); (3) migration 0015 mutada in-place quebrou checksum dev → índice virou
  0016 (91b7e11).
- **INCIDENTE**: worker rodou harness com DATABASE_URL do dev `studio` e limpou o dataset
  do usuário `boys_big_dataset` (860 imgs — S3 intacto, usuário tem backup → RE-IMPORTAR
  via UI). Guarda mecânica aplicada (b83d874: harnesses só aceitam `studio_test*`).
- **SEGURANÇA**: token HuggingFace real (`hf_Gvud…`) exposto em env do container
  orchestrator-local (origem: infra/.env, git-ignored). Recomendar rotação + mover p/
  secret store se o host sair da LAN. NÃO commitado em lugar versionado (verificado).
- **Pendente do usuário**: merge/push da branch (NUNCA sem ordem explícita); re-import do
  backup; dataset `bigload_e2e` (c2aea0a4, 865 imgs, 4,6GB) deixado no dev p/ smoke —
  apagar quando quiser.
- **Dívidas registradas** (docs/dividas.md): diffusion sem fingerprint no build; watchdog
  preparando ancorado em created_at; versão órfã pós-pânico reusável; progresso por
  marcos sem % real; GC de prefixos S3 sem linha (>48h) é sweep manual.
- **Escala p/ GPU real**: prep de 4,6GB levou ~4min serial→paralelo; com engine real o
  gargalo vira o download do nó — AC-007 (`feat/node-content-cache`) já alinhado.

## Achado ORIGINAL (2026-09-16, teste manual pesado) — empacotamento síncrono no request path
Sintoma: dataset `boys_big_dataset` (d67a0a52, 860 imagens, ~3GB) → `POST /api/jobs/autolabel`
via proxy Next retorna **500 em exatos 30002ms** e o job nunca inicia visível na UI.
Causa-raiz (evidência em código + logs):
- `submit_autolabel_job` (services/api-principal/src/jobs/handlers.rs:L1297) chama
  `build_package_filtered` **dentro do request**: baixa as 860 imagens do SeaweedFS em loop
  SERIAL (package.rs:L913), zipa ~3GB, lê o zip INTEIRO em RAM (`tokio::fs::read`, L441),
  md5, e faz PUT de 3GB de volta ao S3 → minutos; o proxy do Next corta em 30s → 500.
- MESMO padrão em TODOS os submits: L856, L1021 (yolo), L1451 (diffusion), L1799 — bug
  geral de datasets grandes, não só autolabel.
- Evidência colateral: `dataset_versions` com 0 linhas no DB `studio` (pacotes nunca
  persistem/sobrevivem) e sqlx `slow statement` >1s por INSERT de image durante upload.
- Drones: job manager 2206ec8e (autolabel, 00:38:37) está `done` sem linha em
  dataset_versions → forte indício de compensation (`compensate_package` apaga a linha
  L42) apagando pacote de job já aceito, ou dispatch sem versão. Investigar no round.
Plano: executado e FECHADO — ver seção "Fatia FECHADA" acima (ADR-0025, 12 commits).

## Plano encerrado — 2026-09-16: feat/geracao-galeria-fixes (de develop)
Correções Geração/Galeria 001–009. TODAS as fatias implementadas, @reviewer em cada round,
gates verdes (cargo test 581p, pytest 101p, build+lint web 0E/204W baseline, compose ok):
- [x] F1 (001+003): aba + configs do form persistidas (localStorage versionado) + "Restaurar padrões"
- [x] E1 (006): PNG iTXt `hephaestus.generation` (mock+real) + testes round-trip
- [x] B1 (009): hook pós-treino manager registra kind/arch + upsert COALESCE + migration 0014 backfill
- [x] F2 (002): marker `geracao:lastCompletedAt` + listener `storage` cross-tab + focus/visibility refetch
- [x] F3 (008): selecionar todas (paginado, teto honesto) + Shift-faixa com âncora por id + delete em lotes 100
- [x] F4 (007): copiar configs/prompt (clipboard) + "Usar estas configs" (resíduos LoRA/custom avisados, nunca silenciosos)
- [x] F5+B2 (004): telemetria fina por step do sampler (throttle, fallback TypeError filtrado) + UI "Imagem i/N" honesta + reset de snapshot por jobId
- [x] docs(repos): REPO_MAP 0001..0014 + backend §9/§10 + frontend Model{kind,arch}
- 005: SEM código (decisão de produto) — daemon quente já existe (`DIFFUSION_DAEMON_ENABLED`,
  default 0, TTL 600s, cache 1 pipeline por spec) + cache de pesos AC-007 planejado
  (`docs/plano-action-center.md` §AC-007 → branch `feat/node-content-cache`).
  ⚠ Dependência descoberta: path do DAEMON NÃO faz tail de telemetry.jsonl
  (`manager/src/lib.rs` daemon reporta 0.0 até done, ~L1336-1517) → habilitar daemon sem
  isso REGREDI o 004. Registrar como pré-requisito de 005.
- Follow-ups desta branch (não bloqueiam):
  1. manager/engine persistirem `modelId` (UUID) nos `generations.params` de LoRA/custom →
     "Usar estas configs" reaplicaria de fato (hoje: aviso honesto + `lorasRaw` no JSON).
  2. `JobResponse` não expõe `totalSteps` → contador "Imagem i/N" só via SSE; campo no
     contrato/openapi daria paridade no polling.
  3. Galeria: shift-faixa por teclado (Shift+Space/Arrow); propagar filtros
     `baseModel`/`quantization` ao refresh (TODO no código; paginação com filtro quant já é
     inconsistente upstream — `effective_total` em memória).
  4. `JobProgressLive.totalSteps` = IMAGENS do batch (documentado); gate por kind se o
     Action Center passar a repassar telemetry de treino no mesmo componente.

## Estado do produto (paralelo, NÃO bloqueado por esta fatia)
- **Pendências do produto:**
  1. **AC-007 NADA implementado** — staging progress (canal da ADR-0024) +
     cache MD5 conteudo-endereçado no nó; plano pronto em
     `docs/plano-action-center.md` §AC-007 (`WeightRef.bytes`, eviction LRU
     `ORCH_CACHE_MAX_GB`, cache_hit, UI preparing/dispatched) → nova branch
     `feat/node-content-cache`.
  2. Smoke E2E Chrome pós-rebuild das imagens (lixeira/cleanup reais, fase
     terminal no drawer, chips autolabel).
   3. Push de `feat/web-design-tokens` ao origin ao fechar a fatia (remoto já
      tem `main` com PR #31; confirmar com usuário).
   4. Dívida registrada: ramo `cancelled` de telemetria não existe (Abort
      races — ver ADR-0024 / backend.md).
   5. (16/09, runtime) DB recriado vazio → login 500 no principal nativo
      (:8080, pid novo, log `/tmp/api-principal.log`); reiniciado com
      `MANAGER_URL=http://localhost:8081`, migrations rodaram, bootstrap
      `STUDIO_PASSWORD=changeme`. **FIX COMMITADO `6130f57`** na branch
      `chore/infra-native-runner` (worktree
      `~/.cache/tmp/opencode/heph-chore-infra`, base origin/main): db
      healthcheck + principal `service_healthy` + `scripts/run-native.sh`
      (STUDIO_PASSWORD obrigatória, env de host, preflights, masking).
      Re-adotar pós-wipe: endpoint deve ser DNS-de-container
      (`http://orchestrator-local:8082`), nunca `localhost` (manager não
      resolve host dentro do container); pairing code é single-use (linha
      "pairing code gerado" no log do orquestrador). Local re-adotado e
      online; remoto pendente (usuário fornece novo code + endpoint
      alcançável do manager).
   6. (16/09, runtime) Geração sem imagens: `down -v` matou bucket
      `heph-data` (autoCreateBucket só vale p/ admin; orquestrador usa
      credencial escopada) → jobs Truenas "done" com ZERO artefatos. Bucket
      recriado à mão + **FIX COMMITADO `a167e2b`** (`chore/infra-native-runner`,
      worktree tmp): s3-init no boot + ensure-bucket.sh SigV4 + preflight
      WARN no run-native. @reviewer FECHAR COM NITS (nits 1-4 aplicados).
      **FOLLOW-UP B: FECHADO pela fatia S2 abaixo (item 7).**
   7. (16/09, AUTONOMIA day-one — `fix/infra-autonomia-day-one`, 4 commits
      `5a5ed5e`→`b2d54a9`): S1 principal autônomo (retry Postgres, senha de
      bootstrap gerada+logada 1x em campo `bootstrap_password`, ensure_bucket
      no boot); S2 done mentiroso morto (put_with_retry nos 6 uploads +
      gate `no_artifacts` no manager; RED do incidente→GREEN; workspace
      597/0); S3 `scripts/reset-dev.sh` = PROVA day-one automatizada
      (down -v→up→asserts; --yes obrigatório); +paridade storage
      nativo↔compose no run-native (5ª mina: mock não servia objetos do S3).
      Deployado: compose backend inteiro nas imagens novas + principal
      CONTAINER dono do :8080 (nativo parado; run-native p/ iteração).
      E2E real provado: generate→Truenas→done→gallery→PNG 1024² 1.2MB.
      PENDENTES: (a) TrueNAS ainda tem código velho do orquestrador — o
      guard do manager já cobre a mentira, mas sync do repo no nó exige
      push/merge (aguardando ordem do usuário); (b) corrida real do
      reset-dev.sh (destrutivo — dia-one ainda NÃO provado de fato);
      (c) nits reviewer S2: swallows residuais metrics-live/md5/scoped_key,
      2 UPDATEs não-atômicos, teste integração yolo_train-done-vazio,
      teste daemon upload_fail, retry-count assert.

## Invariantes & lições da casa

- **Regra das Duas Correções:** 2 falhas do mesmo erro = contexto contaminado;
  parar, registrar aprendizado aqui e escalar (@architect) ou propor reset.
- **Nenhum fix mecânico pelo coordenador:** toda correção vira spec despachada
  (@fixer e afins); coordenador decide e integra, não edita às cegas.
- **File Ownership:** paralelismo só com conjuntos de arquivos disjuntos;
  contratos (openapi, migrations, policies) editados sequencialmente antes.
- **Dev é CPU-only** (`ENGINE_MOCK=1`); GPU real sob demanda explícita.
- **Custo (lição 16/09):** sessão queimou ~45M tokens lendo `coordenacao.md`
  inteiro + arquivos completos. Usar funil L0→L3: `AGENTS.md` →
  `docs/REPO_MAP.md` → `graft ask --source` → leitura delimitada por offset.
