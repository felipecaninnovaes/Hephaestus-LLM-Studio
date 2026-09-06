# Coordenação — estado do plano (memória do coordenador)

Arquivo de trabalho do agente coordenador: registra **onde estamos** e **qual o
próximo passo na ordem**, para sobreviver a restart de sessão. Não duplica
docs — referencia por seção. Atualizar: ao abrir fatia, ao fechar fatia, e ao
ser interrompido no meio de uma.

## Protocolo de retomada (início de sessão)

1. Ler este arquivo → seção "Plano em andamento".
2. `git status` + `git log --oneline -5` para conferir se o disco bate com o
   registrado (branch aberta, commits pendentes de push).
3. `graft check` se for mexer em código indexado (refresh: `graft build`).
4. Fontes de verdade para a fatia: `IDEIA.md`, `docs/backend.md` §9/§10,
   `docs/frontend.md` §10, `docs/repo-estrutura.md` (ordem de fatias),
   `docs/dividas.md` (dívidas a honrar no nascedouro) e os ADRs em
   `docs/adr/` — a 3b tem especificação própria e completa em
   **`docs/adr/0003-object-storage-s3.md`** (decisões D0–D10, delta de contrato,
   contorno da migration 0003, plano de commits 3b.0–3b.8); não reinvente nada que já
   está lá, e não aplique os deltas de `backend.md`/`frontend.md` antes do commit 3b.8.

## Estado atual — 2026-09-06 (sessão 5: fatia 3d — galeria `/datasets/[id]` + editor BBox)

- **Branch aberta: `feat/datasets-gallery`** (de `main` `69cdd47`; CI verde na main, run 3 confirmado success via API de Actions). Plano de commits: **3d.1** backend T7 (`autoTracked` derivado de `boxes.origin='autotracker'` via `EXISTS` no SQL — `DatasetRow`+`COLS`+queries em `models.rs`/`handlers.rs`; campo JÁ existe no wire, sem mudança de contrato; subquery no `RETURNING` do INSERT é válida — dataset novo não tem imagens → false) → **3d.2** galeria (types/api lib client, página real substitui placeholder, upload multipart `files` com toasts por `status`/`reason`, grade de thumbs `<img src={item.url}>` h-36, chip `split`, load-more, botões AutoLabel/AutoTracker/Exportar/Treinar no layout do protótipo ~L1659-1791 — AutoLabel/AutoTracker/Exportar **disabled com `title` de fatia futura**, Treinar segue padrão do DatasetCard) → **3d.3** editor BBox rota `annotate/[imageId]` (sidebar w-72 protótipo ~L2264-2351: ferramentas B/V/H, classes com cor hex inline do backend, coordenadas norm. 6 casas, Salvar → PUT boxes payload total `{boxes:[{classId,x,y,w,h}]}`) → **3d.4** interações (desenhar/mover/resize alça se-resize/clamp 0..1/atalhos B,V,H,[1-9],Delete,Esc/zoom toolbar 50-250/autosave debounced ~800ms + beforeunload) → **3d.5** docs-sync (frontend.md L152/L161) → review → verificação final.
- **Decisões de desenho da 3d (coordenador, sem ADR — sem novo contrato, sem schema)**:
  - `ImageResponse` NÃO tem `labeled` no wire (confirmado em models.rs:280-291): tile NÃO mostra selo por-imagem de rotulada (evita N+1); filtro `labeled` entra como chip de listagem usando o query param que a API já suporta; contagem rotulada vem do `Dataset.labeledCount` na faixa resumo.
  - Editor BBox só para `category==='yolo'` (§5.2); clique em tile difusão/clip → toast honesto. `PUT caption` NÃO ganha client na 3d (YAGNI).
  - Moldura do canvas: base 600px de largura, altura derivada do aspect ratio real (`width/height` do backend), não 600×450 fixo do mock.
  - Caixa usa `style` inline com `class.color` hex (paleta FECHADA não tem as classes tailwind do protótipo; dot/borda/chip via hex + alpha `26`).
- **Ambiente de pé reconfirmado**: db/seaweedfs(healthy)/manager/principal + dev server :3000 (login 200).

### Sessão 4 — ADR-0004 busca semântica + CI (contexto)

- **Contratação do `@infra-dev` (2026-09-05, pedido do usuário)** — dono mecânico de infra: `infra/` (compose), Dockerfiles, `scripts/` de verificação, CI quando spec pedir, `.env.example`. **Não decide arquitetura** (ADR vem do fluxo normal); migrations seguem com `@rust-dev`; não toca código de negócio. Permissões negadas: commits/push/merge/rebase (padrão) + `compose down`/`prune`/`rm` de volume/network/container (proteção do ambiente de dev de pé — a lição do `fix/infra-env` virou política). `variant: medium` (decisão do coordenador: blast radius de ambiente inteiro + falha silenciosa — mesmo rationale do `@fixer`; usuário pode rebaixar para `low`). **Efetivo na próxima sessão** (config não retroage em sessão viva — roster do Task tool é fixado no boot). Gap que motivou: sem CI (`.github/workflows` não existe), dívida de digests pendente, e a 3f adiciona trabalho de infra (imagem pgvector, serviço embedder). Charter em `.opencode/agent/infra-dev.md`.
- **Decisões de gestão do usuário (2026-09-05, fechamento da sessão 4)**:
  1. `chore/infra-agent` MERGEADA pelo usuário (`662fb4d`) — `@infra-dev` efetivo na
     próxima sessão.
  2. **Digests: APROVADO e FEITO** — commit `4b8c7e4` em `chore/pin-digests`
     (despacho `@rust-dev`; dívida QUITADA em `dividas.md`; aguardando merge). Nota
     nova registrada: manager/orchestrator ainda em `bookworm-slim` — migrar para
     `trixie-slim` quando o orquestrador ganhar cliente S3 (fatia 4, R10).
  3. **CI = Gitea Actions (CORREÇÃO — registro anterior errado dizia "descartado")**:     o usuário se auto-hospeda em `git.felipecncloud.com` (origin) e estava
     configurando o **gitea-runner** quando perguntei; workflows em `.gitea/workflows/`
     (formato GitHub-compatível do act_runner). Desenho v1 acordado verbalmente (sem
     arquivo ainda): job rust (`cargo fmt --all --check`, `cargo check --workspace`,
     `cargo test -p api-principal` — sem banco), job web (`npm ci` + build), job compose
     (`config -q` — não precisa de daemon). V2: testes de db com `services: postgres`;
     storage tests só com docker-in-docker (adiar); engines Python entram no CI na 3f.3.
     **Imagens + digests para o runner mirar (resolvidos 2026-09-05, registry oficial;
     digests preservam-se ao copiar para o registry do Gitea)**:
     `rust:1.97.1-slim@sha256:8e8cf8f7fd54a2d23d5a743b3a03f56e26b6c774276c33fa0595111704ebb15c`,
     `node:20-slim@sha256:2cf067cfed83d5ea958367df9f966191a942351a2df77d6f0193e162b5febfc0`,
     `docker:29-cli@sha256:eccaacfeed644c7de222ff047483568cb988dde95476fbaaf10ea2d04921bb66` (29 = major do docker do host 29.7.2),
     `postgres:16@sha256:f1c3376c26f2609ab9f29f71f824103fe2fcd8ee0346485cb6122a4f93df6f94` (v2),
     `python:3.12-slim@sha256:78387bc3881b8273120a12ebe6c1ab22b018ccc2c9adf565ae1ac9b536e184ea` (3f.3).
     Pendências para escrever o `ci.yml`: label do runner (`runs-on:`) e se ele alcança
     o Docker Hub (senão, mirar via registry do Gitea).
     **Estado da iteração de CI (2026-09-06, monitorada pela API de Actions com token
     read-only do usuário em `~/.config/hep-ci/token` — FORA do repo)**: run 1
     (`079eb88`) provou digests ✓ + healthcheck do service ✓ + download de action ✓ e
     derrubou `actions/checkout` (act_runner v3.3.2 NÃO injeta node em actions JS —
     exit 127 em imagens sem node). Fix (`22c6f76`, mergeado): checkout manual
     `git clone` + token automático via header basic `x-access-token` (esquema do
     actions/checkout). Run 2 (`d05d7b9`): job `compose` VERDE de ponta a ponta
     (prova do caminho inteiro); `rust` falhou por `rustfmt` ausente no
     `rust:1.97.1-slim` (perfil mínimo do rustup) e `web` por git ausente no
     `node:20-slim` (base debian-slim puro, NÃO scm-slim — errata do coordenador).
     Fix no run 3: `chore/ci-round3` (`c224ca6`) — `rustup component add rustfmt` +
     `apt-get install git` no web. **Regra de processo (pedida pelo usuário, vale
     para sempre): NUNCA mergear na main para testar CI — o workflow dispara em
     `on: push` de QUALQUER branch; pusha a branch, acompanha via API, mergeia só
     quando verde. Main deve estar sempre verde. Recomendação ao usuário: ativar
     branch protection em `main` (Settings → Branch → Enable Status Check exigindo
     `rust`/`web`/`compose`) para o gate virar mecânico.** Acesso do coordenador à
     API: `GET /api/v1/repos/Felipe/Hephaestus-LLM-Studio/actions/runs` e
     `…/actions/jobs/{id}/logs` (o endpoint `…/tasks/{id}` individual não existe no
     1.27.1 — usar o `jobs` do run).
  4. **Push do tronco: feito pelo usuário** (após o merge do infra-agent; o ADR-0004
     e o pin-digests/housekeeping ainda não estão no origin).
  5. **`cargo fmt`: APROVADO e FEITO** — commit `8947fec` em `chore/housekeeping`
     (`cargo fmt --all`, 14 arquivos; `cargo check --workspace` + `cargo test -p
     api-principal` verdes; dívida do fmt quitada com o merge). Lição operacional:
     o type-enum do commitlint é `[feat, fix, docs, refactor, test, chore]` — `style`
     NÃO existe; usar `chore(fmt)`.
  6. **3d na próxima sessão**: usuário abrirá sessão nova e fará levantamento leve
     se a 3f afeta o que a 3d vai fazer (resposta: a 3f não muda o desenho da 3d —
     só consome a galeria que ela cria; ver ADR-0004 D0).
- **ADR-0004 ACEITA pelo usuário (2026-09-05): busca semântica sobre datasets com embeddings OpenCLIP = fatia 3f**, especificação completa em **`docs/adr/0004-semantic-search.md`** (D0–D8, migration `0004`, spike `3f.0` com 5 critérios binários, plano de commits 3f.0–3f.7). Resumo das decisões: pgvector no Postgres existente (compose troca `postgres:16` → `pgvector/pgvector:pg16` com digest pinado — spike prova upgrade sem dump/restore, R1); embedder = `trainer-clip` em modo `serve` como serviço compose (fora do orquestrador até a fatia 4 — exceção consciente à topologia, com caminho de unificação); indexação assíncrona SEM fila (estado derivado `indexedCount` vs `imagesCount` + advisory lock — não depende da fatia 4); 4 rotas novas, spec 0.4.0, erros novos `index_not_ready` (409) e `embedding_unavailable` (503); EmbeddingPort com `MockEmbedder` default (`EMBEDDING_BACKEND=mock`). Dedup e AutoLabel assistido fora de escopo v1 (schema não fecha portas). **Nada implementado** — docs de contrato só mudam no commit 3f.7 (lista de linhas que ficam falsas está no fim da ADR).
- **Sequência do roadmap atualizada**: 3d → **3f** → 3e → 4. A 3f depende apenas da 3d (a busca mora na galeria); 3f.1–3f.5 são disjuntos de 3e/4 e podem ser despachados em paralelo ao fim da 3d; só 3f.6 (UI) espera a galeria.

### Sessão 3 — alinhamento de design + fixes de ambiente (contexto)

- **Problema reportado pelo usuário**: implementadores frontend desviando do estilo de layout (impeccable/OpenDesign como referência; troca de modelo do dev + protótipo regenerado com login no OpenDesign como teste). Fechado em 3 frentes:
  1. **Referência formal** — `docs/design-system.md` MESCLADO (base OpenDesign: frontmatter YAML navegável + Do's/Don'ts + regras nomeadas; seções exclusivas da versão impeccable reincorporadas: iconografia, anatomia de componentes, a11y, avaliação crítica; token Runtime Python `#eab308` recuperado com prova v1:864). **Descoberta do reviewer**: o `ai-vision-training-studio.html` do tronco JÁ É a regeneração OpenDesign (3641 linhas, `LoginPage` ~329) desde o merge `chore/opendesign` (`39a1410`) — o `ai-vision-training-studio-v2.html` que o coordenador importou do OpenDesign era byte-idêntico e foi REMOVIDO (`1a00b22`); referência única = protótipo da raiz.
  2. **Auditoria+correção visual** (`@ui-designer` qwen3.7-plus, Chrome flatpak :9222 + skill chrome-mcp): `/login` (blobs de luz zenital, meta v1.3, placeholder, autoFocus, focus ring emerald, footer "Single-User Mode" SEM botão demo — instrumentação rejeitada); DatasetCard (violet/sky → `text-zinc-300` PROVADO contra v1:1528; p-5; tiles com borda; chips estilo v1; "Treinar →" text-link); TabsBar (aba ativa `text-white` sem underline); Topbar (inline style → classes; superfície `zinc-950/80` MEDIDA E PROVADA igual à v1 — suspeita inicial do coordenador era falsa); modal `max-w-lg`. Pendências fechadas via `@frontend-dev`: `IconLock` (padrão Base, paths do protótipo) + ícones no DatasetMenu (adaptações declaradas: Eye→IconLayers, Play→IconTarget). **Smoke do login com senha dev `changeme`: 4/4 PASS** (autoFocus; POST 200 + cookie; redirect; 401 → "Senha incorreta."; regression redirect; console limpo).
  3. **Causa-raiz** — charters de `@frontend-dev`/`@ui-designer` agora apontam `docs/design-system.md` como fonte de ESTILO (paleta FECHADA, regras nomeadas: One CTA/Monospace Truth/Refractive Edge/Class Palette Integrity; cores fora da paleta proibidas) e o protótipo como fonte de LAYOUT. É o mecanismo anti-improviso para os implementadores low.
4. **Organização — dívidas viraram registro próprio (inspiração: artigo "Harness
   Engineering" da OpenAI, 2026-02-11)**: seção de dívidas extraída deste arquivo
   para **`docs/dividas.md`** (registro permanente: em aberto/quitado, fatia marcada,
   como atualizar). Este arquivo referencia e não duplica; pendências do Fecho
   também migraram para lá.
- **Fixes de ambiente (desbloqueio do dev; branch `fix/infra-env` `c09569a`)**: healthcheck do SeaweedFS dependia de GNU wget (exit 8 em 403); a tag **mutável** `4.45_full` trocou o wget para BusyBox (exit 1) → container unhealthy permanente → novo probe portável (aceita QUALQUER resposta HTTP: `wget -S … | grep -q HTTP/1.1`). Runtime do principal `bookworm`(glibc 2.36) → `trixie-slim` (builder rust:slim é trixie/2.41; `aws-lc-sys` exige GLIBC_2.38 — **R10 da ADR-0003 materializado por tag mutável**). **Lição: tags de imagem mutáveis quebram builds verificados; recomendação PENDENTE ao usuário: fixar digests no compose/Dockerfiles.**
- **Review da `fix/web-design-alignment`: APROVA** — 1 menor corrigido (v2 duplicado removido) e 1 menor REFUTADO com prova empírica: botão "Treinar" disabled — a regra global `button:disabled` do globals.css JÁ aplica opacity .55 + not-allowed (getComputedStyle confirmado) e o hit-test resolve no próprio botão (sem click-through ao Link) — falso positivo duplo do reviewer, registrado como lição (provar antes de corrigir).
- **Branches aguardando MERGE do usuário (ordem importa)**: ~~todas~~ **MERGEADAS em 2026-09-05**: `fix/web-3c-review` (`0554d3f`, pelo usuário), `fix/web-design-alignment` (`a19d835` — conflito em TabsBar resolvido pelo coordenador: 6 abas da emenda + decisão visual da auditoria na aba ativa `text-white` sem underline; DatasetCard/Modal auto-mergeados, tag AutoTracker preservada, `max-w-lg` combinado), `fix/infra-env` (`fdfaff6`), `chore/agent-design-ref` (`11ebdfb`). **Verificação pós-merge**: build web verde (rota `ƒ /datasets/[id]` viva), compose config OK, smoke visual no Chrome: 6 abas + badge de contagem, aba ativa `rgb(255,255,255)` sem underline, card p-5/16px com Refractive Edge provado (borda topo `0.13` vs laterais `0.07`), botão Treinar com affordance global (opacity .55 + not-allowed). Branches de fatia ainda não apagadas — decisão do usuário. `main` ~6 à frente do origin (push pendente).
- **Nota docs**: `docs/frontend.md` linha 3 ainda descreve o protótipo como "~2910 linhas" — o do tronco é a regeneração (3641, com LoginPage); sincronizar no próximo docs-sync.
- **Ambiente de dev de pé** (não desligado): compose (db, seaweedfs healthy, manager, principal :8080 — senha dev `changeme`), dev server Next :3000, Chrome :9222 (flatpak).

### Sessão 2 — 3b mergeada, 3c revisada e emendada (contexto)

- **Fatia 3b MERGEADA no tronco pelo usuário** (`04b8987 Merge branch 'feat/datasets-storage'`) — storage S3/SeaweedFS fechado, spec 0.3.0.
- **Fatia 3c (UI `/datasets`) NO TRONCO via `1f182ed`** — 3 commits (`cf2b978` fundação do shell: tipos, api lib, format, icons, Topbar/TabsBar/Toast; `f8bca9e` lista grade+tabela com filtros e empty states; `ce29fa4` criar/excluir com modal, confirmação, menu de contexto e toasts). O mesmo merge trouxe `d6e4e1d` (troca de modelos dos agentes — config puro, conferido pelo coordenador).
- **Revisão da 3c: CONDICIONAL** (`@reviewer`, despacho único): contrato/casing/fetch 1:1 com openapi 0.3.0, zero críticos. Condições F1–F6 (link de galeria → 404; só 1 das 6 abas do shell, sem badge; code `"validation"` morto fora do enum; coluna Ações ausente na tabela; `trainTabFor` dead export; tag AutoTracker ausente). **Emenda implementada via `@frontend-dev` e verificada pelo coordenador** (build web limpo com rota `ƒ /datasets/[id]`; greps `validation`/`trainTabFor` zerados) em **branch `fix/web-3c-review`** (`f7889b2` fix(web) + `41739c2` chore gitignore) — **aguardando merge do usuário**. F7 (sem teste de UI) não bloqueia = backlog §12 do frontend.md.
- **`main` está 5 commits à frente de `origin/main`** e `fix/web-3c-review` soma 2 — push/merge = decisão do usuário.

### Histórico das sessões anteriores (contexto)

- **Fatia 3b LANDED na branch `feat/datasets-storage` (3b.0–3b.7:
  `f6c6ff5`..`393163c`) + docs sincronizados (3b.8, working tree desta sessão, sem
  commit — o coordenador commiteia). Branch à frente de `main`; merge = decisão do
  usuário. Entregue: migration 0003 + `StoragePort`/`MockStorage`/`S3Storage` + 6 rotas
  (upload, images, detail, `/data`, boxes, caption) + sweep pós-commit + `source`
  derivado + `classes{id}`; spec 0.3.0; revisões 3b.3/3b.6 feitas. Dívida 3b QUITADA
   (ver `docs/dividas.md`); sobraram: logging server-side (fatia nomeada), gate `sub` órfão,
  `cargo fmt`. A/B 3b.6 registrado no bullet do experimento — **encerrado pelo usuário:
  sem swap; `@reviewer` é o despacho único, max só como escalada** (`a343a7c` em
  `chore/reviewer-escalacao`).

- **Spike 3b.0 EXECUTADO e PASSOU (7/7).** Rodado no ramo **descartável**
  `spike/storage-seaweedfs` (commit `09a517d`; **fundido pelo usuário em `main` (`52da6f9`)**
  — matriz `spike/STORAGE-SPIKE.md` e harnesses vivem no tronco). Consequência: **D4 (crate) e D3 (presigned)
  ficam aprovadas, sem inversão**; R2 e R3 desriscados no ferro. Os achados que **corrigem o
  rascunho da ADR** (identidade via `-s3.config` JSON e não env vars; bucket auto-cria sem
  init-container; healthcheck exige `-ip.bind=0.0.0.0`; nomes reais da API do SDK; novo risco R10
  = build do `aws-lc-sys` no `rust:slim`) estão appêndados na **ADR-0003**, seção "Resultados do
  spike 3b.0" — **ler antes de codar a 3b.4**. `main` limpa de worktree; **1 commit à
  frente do `origin/main`** (`f4d1551`), push pendente = decisão do usuário.
- **Branch de trabalho: `main`.** `feat/datasets-core` foi **mergeada pelo usuário**
  (`e724436 Merge branch 'feat/datasets-core'`) e as branches de fatia foram apagadas,
  incluindo a de segurança `backup/pre-reword-3a` (confirmado antes de apagar: árvores de
  código byte-idênticas aos commits que entraram; o único resíduo era o hash pré-reword de
  um commit cujo conteúdo é o mesmo). Situação de push: ver bullet acima (`main` à frente
  do origin em `f4d1551`).
- Roadmap `docs/repo-estrutura.md` §Ordem: Slice 1 ✅, Slice 2 ✅, **Slice 3a ✅ (no
  tronco)**, 3b é o próximo passo.
- **Slice 3a no tronco**: `GET/POST /api/datasets` + `GET/DELETE /api/datasets/:id` com
  migration `0002` (`datasets`+`classes`), primeira rota de negócio → gate
  `route_layer(require_auth)` plugado (dívida do ADR-0001 D9 quitada), OpenAPI
  0.2.0, ADR-0002 escrita. Verificação: `cargo check --workspace` limpo,
  `cargo test -p api-principal` = 27 units + 7 contract verdes sem banco,
  `bash scripts/test-db.sh` = 7 integration verdes com Postgres do compose,
  `compose -f compose.yaml -f compose.integ.yaml config -q` OK.
- **ADR-0003 (storage de objetos) ACEITA pelo usuário, servidor = SeaweedFS.** Nada
  implementado ainda; é a próxima fatia. Ver "Próximo passo" abaixo e
  `docs/adr/0003-object-storage-s3.md`.
- **Decisão estrutural da 3a (ADR-0002 D1)**: casing no wire é **camelCase em
  `/api/*` inteiro**; colunas SQL, valores de enum, `Error.code` e artefatos de
  transporte (`manifest.json`, `config.yaml`, SQLite) ficam **snake_case**.
  Enforcement por teste (`json_property_names_are_camel_case`, walk recursivo).
  Isso altera o que os docs de settings exemplificavam → `hf_token` virou
  `hfToken` no wire em `backend.md` §9 e `frontend.md` §10 (rota ainda não
  existe). Não regrida isso por acaso.
- `apps/web` continua com só `/` e `/login`; `/datasets` (3c) ainda não existe.
- Ferramental: `@ui-designer` despacha (exige dev server + Chrome :9222).
  Grafo graft em dia (`graft/` é git-ignored — não se commite); 2 nós de
  `layout.tsx` seguem pendentes no meaning tier (modelo local falha lá, cosmético).
- **Cadeia operacional atualizada (2026-09-05, `chore/agent-team`, pendente de merge):**
  gate de commit vivo (`lefthook.yml` → commitlint no `commit-msg` + aviso de
  staging >400 linhas e bloqueio de segredos/`target/` no `pre-commit`; setup novo:
  `npm install && npx lefthook install`); subagentes com `git commit/push/merge/rebase`
  **negados em runtime** (só o coordenador commiteia); `graft` mandatório em
  `fixer`/`reviewer`/`docs-sync`; entregável do `architect` = formato ADR + plano de
  commits numerado; checklist do `reviewer` com invariantes da casa (casing D1,
  body-limit, ordem objeto→linha→compensação, contadores por função única);
  `ui-designer`/`frontend-dev` alinhados ao Tailwind v4 com desempate de posse;
  skill `hephaestus-dev` regrava (um dispatch = um commit, todo por passo com
  atualização em tempo real, spikes = coordenador com loop de build em script único
  — lição do 3b.0). Anti-exemplo registrado na skill: `1c1f72f` (3.219 linhas em 1
  commit) que virou a cirurgia de reword da 3a.
- **Experimento A/B de revisor (3b) — ENCERRADO pelo usuário em 2026-09-05** (custo de
  tokens; decisão registrada ao fim da fatia). `@reviewer-max` (qwen3.8-max, `variant: high`,
  corpo idêntico ao titular) despachou no MESMO diff que o `@reviewer` nos marcos
  3b.3 e 3b.6. **Veredito do usuário: sem swap — `@reviewer` (flash/high) é o despacho
  único de marco; `@reviewer-max` fica no time como ESCALADA** (só quando o titular não
  resolver, travar no mesmo ponto, ou risco alto pedir auditoria independente — charter
  reescrito em `a343a7c`/`chore/reviewer-escalacao`). *Dia 1 (smoke em `86fb0eb`)*: ambos
  BLOQUEIA no mesmo defeito real (gate de segredos × `!.env.example` do gitignore —
  comprovado por matriz de 4 casos antes do fix); o titular ainda cruzou com a ADR-0003
  (`.env.example` é entregável prometido da 3b.4) — 1 ponto pro flash. *Marco 3b.3
  (difícil, teste real)*: ambos **BLOQUEIA** pelo MESMO crítico comprovado por sonda
  própria (livelock multipart pós-`LengthLimit` — axum embrulha corpo todo, multer nunca
  fuseja; os dois citaram a fonte e reproduziram), **zero falso positivo nos dois lados**.
  Titular achou a mais: duplicate falso por stem sem extensão (F4) e a janela de boot mock
  (F6, decisão do coordenador); sombra achou a mais: doc de `sanitize_filename` mentindo +
  branch morto (F7) e o staleness latente do `COALESCE(NEW,OLD)` em UPDATE de
  reparentização (registro: inofensivo até existir rota de UPDATE de `image_id`/`dataset_id`).
  Contagem: 2 titular × 2 sombra — empate técnico. *Marco 3b.6*: titular CONDICIONAL com
  **1 falso positivo** (emenda vista só no trunk — o diff do marco não continha o fix) e 2
  únicos (erros por-chave do `delete_prefix`, TTL não wired no compose); sombra PASSA com 2
  únicos (órfão `infra_pgdata` no runner, upsert com `RETURNING`) e 0 falsos. **Placar
  final: 3b.3 empate 2×2; 3b.6 2×2 com vantagem da sombra só em falsos positivos.**
  Conclusão operacional: achados convergentes nos dois marcos — **o segundo despacho nunca
  mudou um desfecho que o titular + coordenador não tivessem alcançado; o duplo despacho
  não se paga.** Hipótese "medium no fixer reduz escaladas" segue válida (é outra linha do
  experimento, sem custo de modelo caro).
- **Esforço de razonamento fixado por agente** (`variant:` na frontmatter, validado
  no provider): **coordenador `@hephaestus` sobe para qwen3.8-max/`medium`** (decisão do
  usuário 2026-09-05, informed pelo A/B: o loop redundante de decisão em flash/high custou
  mais que o differential do modelo — max decide certo com cadeia menor); `high` em
  architect/reviewer; escalada `@reviewer-max` só quando o titular travar; `medium` em
  fixer e ui-designer; `low` nos implementadores e explore. **Novo @visao**
  (flash/`low`, permissões edit/bash/web negadas): proxy de visão do coordenador —
  transcreve screenshots/PNGs de forma fática quando o usuário anexa imagem; NÃO audita
  tela (isso segue sendo do `@ui-designer` com DevTools: DOM+computed styles+edição, que
  "descrever pixels" não substitui). Fallbacks de uma linha: se max/medium mostrar
  verbosidade ou loop novo no coordenador, testar `low`, e rebaixar para flash/high é o
  último passo; a medição natural é a sessão da 3c. Hipótese "medium no fixer reduz
  escaladas" segue válida. Vale a partir da próxima sessão (config não retroage em sessão
  viva; em `chore/reviewer-escalacao` `bc4d5db`).

## Storage da 3b — decisão TOMADA (2026-09-04): bucket S3/SeaweedFS

Origem: o usuário propôs **MinIO** (como ele já opera no VisionLens,
`/home/felipecn/DEV/VisionLens`) para imagens canônicas e labels/coordenadas no banco.
O `@architect` desenhou e eu aprovei a direção; **o usuário aceitou a ADR-0003 e escolheu
SeaweedFS** como servidor. Tudo está em
**`docs/adr/0003-object-storage-s3.md`** — ela é a especificação da 3b, leia antes de
codar. Resumo do que já está fechado lá:

- **D1** bucket = único blob canônico; disco local só efêmero (árvore YOLO/`.txt`/
  `data.yaml` nasce em tempdir **no orquestrador** e morre no `finally` — padrão validado
  no `yolo_trainer.py` do VisionLens).
- **D2** upload **via principal** com **spool em tempfile + `put_object` com
  content-length exato.** Nunca stream de tamanho desconhecido (trilha
  `aws-chunked`/`STREAMING-*-TRAILER`), nunca presigned browser→bucket (exigiria
  `complete`+`HEAD`+sonda ou a máquina de notificação de bucket + estado "pendente").
- **D3** leitura híbrida por `S3_PUBLIC_ENDPOINT_URL` (presigned **assinado no host
  público** — gotcha SigV4 do header `Host`, lição do VisionLens) com fallback
  `GET …/images/:imageId/data` **sempre disponível**.
- **D4** `aws-sdk-s3` **sem** `aws-config`, `force_path_style(true)`,
  `request_checksum_calculation(WhenRequired)`. Nenhum `reqwest` na 3b.
- **D5** chaves legíveis `datasets/{dataset_id}/images/{image_id}/{filename}`;
  `images.path`→`object_key`; **`datasets.source` sai do banco** (DROP na 0003) e vira
  derivado no wire `s3://{bucket}/datasets/{id}/` quando `images_count > 0`.
- **D6/D7** só mídia vira objeto; ordem **objeto→linha→compensação** e sweep de prefixo
  **pós-commit** no `DELETE /:id`.
- **D8** `src/storage/{port,mock,s3,keys,sniff}.rs` + `MockStorage` (testes sem rede);
  **manager e orquestrador não têm cliente S3 na 3b**.
- **D9** `export`/`import`/`package` **sairam da 3b e viraram fatia 3e** (backup interim =
  UI do servidor + `mc mirror`); o chunking de 8 MB sobrevive só no transporte
  principal→orquestrador **remoto**.
- **D10** um erro novo só: `storage_unavailable` (503). Spec 0.2.0 → **0.3.0**.

Por que não MinIO (e-a-confirma-na-fonte, não é palpite): repo `minio/minio` **arquivado
pelo dono em 25/04/2026** ("THIS REPOSITORY IS NO LONGER MAINTAINED", só código-fonte,
sem binário de comunidade; última release out/2025) e a **GHSA-9c4q-hq6p-c237 /
CVE-2026-40344** (bypass de assinatura na trilha `STREAMING-UNSIGNED-PAYLOAD-TRAILER`) com
**"Patched versions: None"** no OSS. As issues #21611 e **#21303 (esta é o SDK Rust com
`ByteStream::from_path`)** documentam a trilha de streaming quebrada que a D2 evita.

**Nenhum doc de `backend.md`/`frontend.md` foi alterado** — a lista de linhas que ficam
falsas está no fim da ADR-0003, marcada para o commit `3b.8` (docs descrevem o que existe,
não o que foi aprovado).

## Dívidas técnicas

As dívidas técnicas e pendências vivem em **`docs/dividas.md`** — registro
permanente de primeira classe (em aberto / quitado, com fatia marcada quando
aplicável; inspirado no `tech-debt-tracker` do artigo "Harness Engineering"
da OpenAI). Este arquivo não as duplica: ao registrar, marcar fatia ou quitar
uma dívida, atualize o `dividas.md`. Fatias novas DEVEM ler o `dividas.md`
(item 4 do protocolo de retomada) e honrar as dívidas relevantes no
nascedouro.

## Plano em andamento — PRÓXIMO PASSO EXATO: fatia 3d (galeria `/datasets/[id]` + editor BBox)

**Todos os merges da sessão 3 fechados no tronco (ver "Estado atual"). Sequência daqui (atualizada pela ADR-0004):**
**3d galeria `/datasets/[id]` + editor BBox** (o placeholder honesto criado pela emenda é substituído pelo conteúdo real; upload UI entra aqui; `autoTracked` derivado de `boxes.origin='autotracker'` — dívida T7) → **3f busca semântica (ADR-0004)** → **3e export/import** → **4 jobs/package/materialização** (orquestrador ganha cliente S3 com credencial escopada por prefixo e absorve o embedder como runner-CLIP — mesma interface HTTP da 3f; dívida T4 da ADR-0002 — `jobs.dataset_id ON DELETE SET NULL` + `dataset_versions` — honrada no nascedouro).

**3b.0–3b.8 FEITOS e MERGEADOS (`04b8987`); 3c FEITA, REVISADA e EMENDADA (ver "Estado atual").** A sequência:

1. ~~`spike/storage-seaweedfs`~~ ✅ **CONCLUÍDO 2026-09-05** (ramo `spike/storage-seaweedfs`,
   commit `09a517d`, matriz em `spike/STORAGE-SPIKE.md`; **fundido pelo usuário em `main` (`52da6f9`)** — harnesses e matriz vivem no tronco).
2. ~~**3b.1..3b.7**~~ ✅ **CONCLUÍDOS** em `feat/datasets-storage` (`f6c6ff5`..`393163c`);
   `@reviewer` ao fim de 3b.3 e 3b.6 (ver A/B no "Estado atual").
3. ~~**3b.8**~~ ✅ **CONCLUÍDO nesta sessão** (`@docs-sync`: deltas da ADR-0003 aplicados em
   `backend.md`/`frontend.md` + banner na ADR-0002 + ADR-0003 marcada IMPLEMENTADA).
4. ~~3c UI `/datasets`~~ ✅ **NO TRONCO** (`1f182ed`, 3 commits) + revisão CONDICIONAL
   fechada com emenda em **`fix/web-3c-review`** (`f7889b2`, `41739c2`). Próximo:
   **usuário mergeia `fix/web-3c-review`** → **3d galeria `/datasets/[id]` + editor
   BBox** (o placeholder honesto criado pela emenda é substituído pelo conteúdo real;
   upload UI entra aqui) → **3e export/import** → **4 jobs/package/materialização**
   (onde o orquestrador ganha cliente S3 com credencial escopada por prefixo e onde a
   dívida T4 da ADR-0002 — `jobs.dataset_id ON DELETE SET NULL` + `dataset_versions` —
   precisa ser honrada no nascedouro).

Cada fatia: branch `feat/<slice>` de `main` atualizada, commit `type(scope):
subject`, verificação do coordenador (`cargo check --workspace`, `cargo test -p
api-principal`, `bash scripts/test-db.sh` quando houver teste de banco, `bash
scripts/test-storage.sh` quando houver storage, `npm run build --workspace=web` quando
houver UI, `compose config -q`), sem push/merge sem pedido. Commits **fora de
`main`** (a regra da casa; `docs/coordenacao.md` e ADRs são as exceções que o usuário já
autorizou a landing direto no tronco).

## Fecho

- [x] Fatia 3a mergeada em `main` pelo usuário (`e724436`) e branches de fatia apagadas.
- [x] ADR-0003 aceita, D0 = SeaweedFS.
- [x] Spike `spike/storage-seaweedfs` rodado (7/7 PASS, commit `09a517d`; **fundido pelo
      usuário em `main` `52da6f9`**); achados appêndados na ADR-0003 → "Resultados do spike 3b.0".
- [x] **3b.7** ✅ (sweep + `source` derivado + classes com `id`, `393163c`) e **3b.8** ✅
      (docs sincronizados nesta sessão).
- [x] **3b mergeada** (`04b8987`); **3c no tronco** (`1f182ed`), revisada (CONDICIONAL,
      F1–F6) e emendada em `fix/web-3c-review` — build web limpo, greps zerados.
- [x] Próximo passo = ~~4 merges~~ ✅ **MERGEADOS** (3c-review pelo usuário `0554d3f`; design-alignment `a19d835` com conflito de TabsBar resolvido; infra-env `fdfaff6`; agent-design-ref `11ebdfb`) → **3d é a próxima fatia**.
- [ ] Pendências e dívidas que continuam valendo: ver **`docs/dividas.md`**
      (registro permanente — inclui CLI `reset-password`, `cargo fmt`,
      logging server-side, gate `sub` órfão, digests de imagem, testes de UI,
      sync `docs/frontend.md` linha 3).
      ~~`lefthook install`~~ ✅ quitado em `chore/agent-team` (gate ativo: hooks
      instalados + commitlint real + deny de commit nos subagentes).
