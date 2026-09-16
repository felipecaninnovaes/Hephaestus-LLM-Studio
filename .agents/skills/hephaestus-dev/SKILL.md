---
name: hephaestus-dev
description: Hephaestus monorepo workflow — use when implementing slices, committing, branching feat/*, verifying builds, or enforcing conventional commits and small PRs in this repo.
---

# Fluxo de desenvolvimento Hephaestus

Convenções para toda tarefa de implementação. Seguir sem ser lembrado.

## Fontes de verdade

- `tasks/todo.md` — memória ativa do coordenador: estado da sessão e próximo passo (ler no início de sessão; NUNCA o histórico em `docs/archive/`).
- `AGENTS.md` (L0) e `docs/REPO_MAP.md` (L1) — mapa e topologia canônicos.
- `docs/adr/` — ADRs aceitas são a especificação da fatia (ex.: 0024 = telemetria status×métrica).
- `docs/backend.md` — topologia, contratos (§9), schema (§10), políticas.
- `docs/frontend.md` — contratos de UI (§10), design system, rotas.
- `docs/repo-estrutura.md` — layout do monorepo, ordem de fatias.
- `IDEIA.md` — intenção do produto. Nunca contradizer em silêncio; expor conflitos.
- Contexto de código: graft ANTES de grep/leitura (`graft ask --source`, `graft grep`, `graft callers`); após mudanças grandes, `graft build`.

## Quem faz o quê no git

- **Implementadores (rust-dev, frontend-dev, python-engines, fixer, docs-sync) NÃO commiteiam** — a permissão `git commit/push/merge/rebase` está negada em runtime, não é só texto de prompt.
- **Coordenador (hephaestus) stageia e commiteia**, sempre fora de `main`/`develop` direto; branches `feat/…`, `fix/…`, `chore/…` abertas de `develop` (exceções autorizadas: `tasks/todo.md` e ADRs).
- **Usuário decide merge/push.** Nunca fazer nenhum dos dois sem pedido explícito.

### Checklist do coordenador ANTES de todo commit

1. Branch atual confere com a fatia do plano? (`git branch --show-current`)
2. Staging arquivo-a-arquivo; **nunca `git add -A`**; conferir `git diff --cached --stat`.
3. Diff de código ≤ ~400 linhas? Passou, é para quebrar — anti-exemplo histórico: `1c1f72f` com 3.219 linhas em 1 commit virou cirurgia de rebase + reword + sincronização de hashes em série.
4. Mensagem pronta no padrão `type(scope): subject` pt-BR, minúscula após os dois-pontos; validar **antes**: `echo 'type(scope): subject' | npx --no-install commitlint`. Gate lefthook ativo (`commit-msg` + `pre-commit`; setup: `npm install && npx lefthook install`).
5. Verificação da seção "Checks" executada e verde.

## Despacho para subagentes

- **Um dispatch = um commit** (no máximo um passo numerado do plano da ADR). Nunca pedir "implemente a fatia inteira": modelo barato cumpre literalmente e produz monocommit.
- Prompt com contexto COMPLETO: arquivos-alvo, trecho da ADR/contrato, convenções, comando de verificação, critério de pronto, resultado do graft já colado (para não caçarem contexto).
- **Todo por passo**: cada todowrite espelha um dispatch/commit, com `in_progress` único e atualização imediata após a verificação do commit — nunca batch de conclusão, nunca todo-pai "implementar fatia X" vivo por horas.
- **Spikes ficam com o coordenador**: spike é pesquisa que produz especificação (charter do implementador é executar spec completa, não decidir). Rodar iteração de build/prova em **script bash único** que executa o ciclo e imprime a matriz de resultados.
- Erro de build/teste → `@fixer` (máx. 2 tentativas — Regra das Duas Correções); reincidiu, escalar a `@architect` ou assumir.

## Verificação (antes de chamar qualquer coisa "pronto")

- Rust: `cargo check --workspace` (workspace raiz, lock único `Cargo.lock`)
- Testes Rust: `cargo test -p api-principal` (units+contract sem banco); integração Postgres: `bash scripts/test-db.sh`; storage (quando existir): `bash scripts/test-storage.sh`
- Compose: `docker compose -f infra/compose.yaml -f infra/compose.integ.yaml config -q` (+ `infra/compose.spike.yaml` se ramo spike)
- Web: `npm run build --workspace=web`; `node --check` em configs alteradas
- Smoke: `bash scripts/e2e-smoke.sh` quando a rota existir na superfície
- Nunca afirmar "done" sem executar o check relevante e colar a evidência.

## Regras de implementação

- Dev é CPU-only; caminho GPU sempre atrás de mock (`ENGINE_MOCK=1`). Testes `@gpu` são manuais — não rodar.
- Boundaries: principal é a única superfície do front; manager possui fila/VRAM; orchestrators são stateless. Postgres: schema único com migrations em `services/api-principal/migrations/`; posse lógica — principal: `users`/`auth_state`, `datasets`, `images`, `videos`, `boxes`, `classes`, `captions`, `image_embeddings`, `models`, `generations`, `dataset_versions`; manager: `jobs`, `job_artifacts`, `orchestrators`. Estado reconstruído do banco no boot, nunca só memória.
- **Casing (ADR-0002 D1)**: wire de `/api/*` 100% camelCase; SQL, enums, `Error.code` e artefatos de transporte em snake_case. Enforcement por teste; não regridar.
- Rota nova só entra em `docs/backend.md` §9 / `frontend.md` §10 quando implementada — `@docs-sync` no commit de sync da fatia, nunca antes (docs descrevem o que existe, não o que foi aprovado).
- Commits das fatias seguem o plano numerado da ADR quando ela existir.
- Usar `todowrite` em todo trabalho multi-passos; marcar concluído em tempo real.
- Responder em português. Resumo curto: o que mudou, onde, o que foi verificado.
