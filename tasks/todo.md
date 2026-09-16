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
- **Follow-ups desta fatia (não bloqueiam):**
  1. Spinner `aria-hidden` explícito + `aria-busy` no Button (a11y).
  2. `--text-2xs/3xs/4xs` sem line-height própria (herda do contexto) —
     fixar token se introduzirmos `leading-*` nesses tamanhos.
  3. **Biome toolado; dívida de lint a zerar (fatia follow-up):** 116 errors
     manuais catalogados — `useExhaustiveDependencies` 32 (12 arqs; pior:
     `datasets/[id]` 8, `GenerationPanel` 8, `AutoLabelModal` 7 — risco
     comportamental, revisar 1 por 1), `noLabelWithoutControl` 23,
     `useKeyWithClickEvents` 16, `noStaticElementInteractions` 15,
     `useSemanticElements` 9, `noArrayIndexKey` 8, `noSvgWithoutTitle` 7
     (→ `biome-ignore` justificado p/ decorativos), `noAutofocus` 3,
     `Select.tsx` a11y 3. Warnings: `noExplicitAny` 90, `noUnusedImports` 46,
     `noNonNullAssertion` 22, `noImgElement` 19. Também pendente: gate de
     format/assist Biome (touch ~108 arqs — slice dedicada), alinhar
     `import React`→type-only em Button, pin $schema vs `^` dep.
     Comando canônico: `npm run lint --workspace=web` (raiz). **rtk NÃO serve
     p/ lint** (corrompe saída) — chamar npm direto.
  4. Paleta de séries do ConvergenceChart (1 consumidor) — avaliar tokenização.
  5. Divergências cosméticas canonicadas pelo reviewer: raio CTA /jobs
     (lg→md), ghost DatasetTable (zinc-400→300), tom spinner brand (500→400).
- **Pendência:** merge/push desta branch só com ordem explícita do usuário.

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
