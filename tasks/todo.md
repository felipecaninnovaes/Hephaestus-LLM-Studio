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

## Estado atual — 2026-09-16 (fatia viva: harness restructure)

- **Branch de trabalho:** `chore/harness-restructure` (off `develop`, 16/09).
- **Fatia em andamento:** reestruturação do harness de agentes a partir da
  proposta auditada em `./tmp` (README §6 corrigido — ver plano abaixo).
  - [x] Branch aberta de `develop`
  - [x] Migrar estado ativo + arquivar `coordenacao.md`/`plano-3e`
  - [x] `AGENTS.md` L0 + `docs/REPO_MAP.md` corrigidos (portas vs compose,
        comandos uv, convenção de branch, ponteiro graft)
  - [x] `.agents/rules/` + `opencode.json` saneado (MCPs preservados; nits do
        reviewer: denies docker restaurados, escalada reviewer-max no prompt)
  - [x] Dedup de skills (hephaestus-dev reconciliado, `.agent/` órfão removido)
  - [x] Saneamento de disco (`apps/web/graft`, `design-system.md`,
        `.impeccable/critique`, `.ignore` ancorado em `/graft/` — nota: o
        runtime do graft pode rescrever `!graft/` não-ancorado em refresh;
        não travar briga, a árvore aninhada já não existe)
  - [x] @reviewer no diff (APROVA COM NITS — nits P2 fechados: ponteiro ativo
        em `docs/dividas.md` → `tasks/todo.md`)
- **Pendência desta fatia:** mesclar em `develop` só com ordem do usuário
  (revisão final humana dos 7 commits).

## Estado do produto (paralelo, NÃO bloqueado por esta fatia)

- **`develop`** contém as 3 branches revisadas/aprovadas e mergeadas em 16/09:
  `feat/jobs-cleanup` → `feat/action-center-polish` → `feat/jobs-status-metrics-split`
  (ordem respeitada; UI AC-003 canônica da polish). Branches locais deletadas
  pós-merge. `develop` está **44 commits à frente de `main`** — merge na main
  aguarda ordem explícita do usuário.
- **Pendências do produto:**
  1. **AC-007 NADA implementado** — staging progress (canal da ADR-0024) +
     cache MD5 conteudo-endereçado no nó; plano pronto em
     `docs/plano-action-center.md` §AC-007 (`WeightRef.bytes`, eviction LRU
     `ORCH_CACHE_MAX_GB`, cache_hit, UI preparing/dispatched) → nova branch
     `feat/node-content-cache`.
  2. Smoke E2E Chrome pós-rebuild das imagens (lixeira/cleanup reais, fase
     terminal no drawer, chips autolabel).
  3. Push de `develop` ao origin (confirmar com usuário; remoto só tem
     `feat/enable-bucket` fora).
  4. Dívida registrada: ramo `cancelled` de telemetria não existe (Abort
     races — ver ADR-0024 / backend.md).

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
