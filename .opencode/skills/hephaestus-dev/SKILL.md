---
name: hephaestus-dev
description: Hephaestus monorepo workflow — use when implementing slices, committing, branching feat/*, verifying builds, or enforcing conventional commits and small PRs in this repo.
---

# Hephaestus Dev Workflow

Conventions for every implementation task in this monorepo. Follow without being reminded.

## Sources of truth

- `docs/backend.md` — topology, API contracts (§9), Postgres schema (§10), policies.
- `docs/frontend.md` — UI contracts (§10), design system, routes.
- `docs/repo-estrutura.md` — monorepo layout, slice order.
- `IDEIA.md` — original product intent. Never contradict it silently; surface conflicts.

## Git workflow (agent commits, user merges)

1. Work always on `feat/<slice>` branched from updated `main`. Never commit to `main`.
2. The agent stages and commits; the user reviews and merges. Do not push or merge unless explicitly asked.
3. Before committing: `git status`, `git diff --cached --stat`; stage only intended files. Never commit `target/`, `node_modules/`, `.env`, secrets, or `*.lock` beyond Rust `Cargo.lock`.
4. Message format (commitlint enforced): `type(scope): subject` — types `feat|fix|docs|refactor|test|chore`, scope required, subject in Portuguese, concise.
5. One vertical slice per PR, diff < ~400 lines: contract → migration → endpoint → manager → engine mock → UI → test. Each slice must build and test green in isolation.

## Verification (always run before committing)

- Rust: `cargo check --workspace` (workspace raiz, lock único em `Cargo.lock`)
- Compose: `docker compose -f infra/compose.yaml -f infra/compose.integ.yaml config -q`
- JS: `node --check` on changed configs; `npm run build` in `apps/web` when UI changes.
- Never claim "done" without executing the relevant check.

## Implementation rules

- Dev is CPU-only; GPU paths stay behind mocks (`ENGINE_MOCK=1`, `compose.integ.yaml`). Mark real-GPU tests `@gpu`, manual only.
- Principal is the only frontend surface; manager owns queue/VRAM; orchestrators are stateless executors. Keep these boundaries.
- New API routes must appear in `docs/backend.md` §9 and `docs/frontend.md` §10 in the same slice.
- Postgres: principal owns datasets/auth/settings tables; manager owns jobs/runners/orchestrators. Rebuild in-memory state from DB on boot, never rely on memory alone.
- Use `todowrite` for multi-step work; mark todos completed as you go.
- Respond in Portuguese. Keep summaries short: what changed, where, what was verified.
