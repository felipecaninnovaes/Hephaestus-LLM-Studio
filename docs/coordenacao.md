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
   `docs/frontend.md` §10, `docs/repo-estrutura.md` (ordem de fatias).

## Estado atual — 2026-06/09-04

- Branch: `main`. **4 commits à frente de `origin/main` — push pendente**
  (merge `feat/auth-single-user`, `feat/design-tailwind`: tailwind real nas
  telas, agente `ui-designer`, skill+MCP `chrome-devtools`).
- Roadmap `docs/repo-estrutura.md` §Ordem: Slice 1 (health/smoke) ✅, Slice 2
  (auth single-user) ✅, **Slice 3+ (datasets → package → jobs mock → UI) —
  PRÓXIMA**.
- Fatias de suporte fora de ordem já fechadas: bootstrap web + design
  tailwind/protótipo (`/` e `/login` na linha do HTML de referência).
- `apps/web` só tem rotas `/` e `/login`; `/datasets` etc. ainda não existem
  (especificadas em `docs/frontend.md` §5 e §10, protótipo
  `ai-vision-training-studio.html`).
- `api-principal`: rotas de negócio vazias de propósito (`auth/routes.rs`
  `build()` — gate plugado junto da 1ª rota de negócio na Slice 3+).
- Ferramental: `@ui-designer` despachável (audita telas vs protótipo com
  Chrome DevTools MCP; exige dev server + Chrome :9222, nunca committa).
  Grafo graft: em dia, meaning tier 98% (2 nós de `layout.tsx` pendentes —
  modelo local falha nesse arquivo, cosmético).

## Plano em andamento — Slice 3: datasets (backlog, nada aberto)

Decomposição em fatias verticais (< ~400 linhas cada), na ordem:

1. **3a — datasets backend núcleo**: conferir contrato `docs/backend.md` §9/§10
   (tabela `datasets`, rotas `GET/POST /api/datasets`, `GET/DELETE /:id`) →
   `@architect` valida contra schema atual → migration `0002_datasets` →
   endpoints em `api-principal` (+ primeira rota de negócio: plugar
   `gate::require_auth` no `protected` router) → teste contract → `@reviewer`.
2. **3b — upload/imagens (storage)**: `POST /:id/upload`,
   `GET /:id/images`, paths do storage local → migration das imagens →
   endpoints → testes.
3. **3c — UI `/datasets` (lista)**: `@frontend-dev` implementa vs protótipo
   (mock inicial dos 5 datasets de `docs/frontend.md` §5.1 até 3a fechar de
   verdade) → `@ui-designer` audita screenshot-vs-screenshot → `@reviewer`.
4. **3d — `/datasets/[id]` galeria + annotate**: maior; abrir sub-fatias ao
   chegar (drag/resize de boxes, autosave `PUT .../boxes`).

Cada fatia: branch `feat/datasets-*` de `main` atualizada, commit
`type(scope): subject`, verificação do coordenador (`cargo check --workspace`,
`npm run build --workspace=web`, compose config), sem push/merge sem pedido.

## Fecho

- [ ] Push dos 4 commits de `main` — aguardando pedido do usuário.
- [ ] Abrir 3a quando o usuário der o go.
