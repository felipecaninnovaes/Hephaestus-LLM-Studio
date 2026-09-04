# Estrutura do repositório (monorepo)

Decisões: trunk + `feat/*`, commits convencionais rígidos, integração em compose + mock, dev CPU-only.

## Layout

```
apps/web/                    # Next.js — abas, galeria, editor BBox, playground, /login
services/api-principal/      # Rust :8080 — auth, datasets, Postgres, BFF do front
services/manager/            # Rust :8081 — fila central, VRAM, orquestradores
services/orchestrator/       # Rust — build dataset, trainers/runners (docker|subprocess)
engines/{trainer-yolo,trainer-difusao,trainer-clip,runner-*}/  # Python (mock CPU no dev)
packages/contracts/          # OpenAPI + tipos gerados (fonte: backend.md §9)
packages/policies/           # vram-table.yaml, engines.yaml
infra/{compose.yaml,compose.integ.yaml,Dockerfiles}/
docs/{frontend.md,backend.md,repo-estrutura.md}
scripts/{dev.sh,reset-password.sh,e2e-smoke.sh}
```

## Regras

- PR < 400 linhas de diff, 1 slice vertical por vez (contrato → migration → endpoint → manager → engine mock → UI → teste).
- `main` protegida, squash merge, `feat/<slice>` curta.
- Commitlint + lefthook bloqueiam fora de `feat|fix|docs|refactor|test|chore(scope): msg`.
- Testes: unit por serviço → contrato (snapshot OpenAPI) → `compose.integ` com engine-mock → smoke e2e. GPU real só manual (`@gpu`).
- Mínimos dev: CPU, 16 GB RAM, Docker 24+, Node 20, Rust stable, Python 3.11 + uv; Postgres via compose.

## Ordem de construção

1. Este scaffold (pastas + compose + lint + policies). ← agora
2. Slice 1: `GET /health` nos 3 serviços + smoke do compose.
3. Slice 2: auth single-user (`users`, `STUDIO_PASSWORD`, login/logout).
4. Slice 3+: datasets → package → jobs mock → UI.
