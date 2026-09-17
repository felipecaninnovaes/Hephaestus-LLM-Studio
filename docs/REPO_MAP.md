# REPO_MAP.md — Hephaestus LLM Studio (Nível 1: Meso L1)

Mapa topológico de serviços, portas, rotas e persistência (~1.300 tokens).
Fonte: `infra/compose.yaml` e código real (16/09/2026). Consulte para localizar
responsabilidades sem abrir código; aprofunde com `graft ask --source`.

---

## 1. Topologia de Rede & Serviços

```text
[Browser :3000] → apps/web (Next.js 16) → proxy/rewrites /api/* →
[services/api-principal :8080] ⇄ Postgres (pgvector) + SeaweedFS S3 (embedder :8090 p/ vetores)
        ↓ HTTP interno
[services/manager :8081] — fila de jobs, VRAM (packages/policies/vram-table.yaml), heartbeat
        ↓ dispatch HTTP interno
[services/orchestrator :8082] — Docker API / subprocess; daemon difusão :8766 (cache VRAM quente)
        ↓
[engines/*] trainer-yolo · trainer-difusao · trainer-clip (Python 3.11+, uv; dev com ENGINE_MOCK=1)
```

**Sem redes Docker customizadas** (`internal-net`/`frontend-net` não existem):
todos os serviços compartilham a rede default do compose. O isolamento de
engines é feito por **não mapear `ports:` para o host** — embedder e S3 têm
bind loopback (`127.0.0.1:`) em dev.

## 2. Inventário de Portas (dev — `infra/compose.yaml`)

| Serviço | Porta no host | Papel |
| :--- | :--- | :--- |
| `web` (apps/web) | `3000:3000` | UI do Studio; rewrites `/api/*` → principal |
| `principal` (api-principal) | `8080:8080` | Único gateway/BFF público; auth, datasets, storage S3, busca pgvector, proxy p/ manager |
| `manager` | `8081:8081` | Fila de jobs, VRAM, heartbeat (uso interno; Bearer token) |
| `orchestrator-local` | `8082:8082` | Executor de engines no nó; telemetria `metrics.jsonl` |
| `db` (postgres+pgvector) | `5432:5432` | Banco relacional |
| `seaweedfs` (S3) | `127.0.0.1:8333` | Objetos (datasets/checkpoints); presigned URLs |
| `embedder` (trainer-clip) | `127.0.0.1:8090` | Vetores OpenCLIP 512d |
| daemon difusão | `:8766` (interno) | Geração quente LoRA (Flux/SDXL/SD1.5), só via orchestrator |
| `trainer-yolo` / `trainer-difusao` | nenhuma | Jobs efêmeros disparados pelo orchestrator |

GPU real (TrueNAS): `infra/compose.gpu.yaml` / runbook `infra/README-gpu.md`.

## 3. Posse de Dados (Postgres único, schema compartilhado)

Migrations canônicas: `services/api-principal/migrations/0001..0014.sql`.
- **Domínio aplicação/dados (escrita: api-principal):** `users`, `auth_state`,
  `datasets`, `dataset_versions`, `images`, `videos`, `boxes`, `classes`,
  `captions`, `image_embeddings` (pgvector), `models`, `generations`.
- **Domínio execução (escrita: manager/orchestrator):** `jobs` (inclui
  `phase`/`message` — ADR-0024), `job_artifacts`, `orchestrators`.
- Políticas de hardware/engines: `packages/policies/vram-table.yaml`,
  `packages/policies/engines.yaml`. Contrato HTTP: `packages/contracts/openapi.yaml`.

## 4. Rotas públicas da API (`api-principal :8080`, wire `camelCase`)

Fonte: tabela de contrato em `services/api-principal/src/auth/routes.rs`
(validada por teste de contrato). Grupos:
- **Auth:** `POST /api/auth/login|logout`, `GET /api/auth/me`; `GET /health|/ready`.
- **Datasets:** CRUD `/api/datasets[/:id]`; upload `POST /:id/upload`;
  galeria `GET /:id/images` (+ `/data`, `/boxes`, `DELETE /:id/images/:imageId`,
  `/restore`, trash `/:id/trash`); `PUT /:id/images/:imageId/boxes`,
  `POST /:id/boxes/batch`, captions `/:id/images/:imageId/caption`,
  classes `/:id/classes`; export/import `POST /:id/export`, `/:id/package`,
  `POST /api/datasets/import`.
- **Busca semântica:** `POST /:id/search/index`, `GET /:id/search/status`,
  `GET /:id/search`, `POST /:id/search/by-image`.
- **Jobs/treino:** `GET /api/jobs`, `/jobs/queue`; `POST /jobs/yolo`,
  `/jobs/diffusion`, `/jobs/predict`, `/jobs/autolabel`, `/jobs/autotracker`,
  `/jobs/cleanup`, `/jobs/:id/abort`; `GET /jobs/:id[/artifacts|/metrics|/events]`;
  previews `POST /jobs/:id/autolabel|autotracker/preview` + `/apply`;
  telemetria `GET /api/telemetry`; geração `POST /jobs/diffusion/generate`.
- **Modelos/pesos:** `GET /api/models[/:id]`, `POST /api/models/upload|download`.
- **Galeria de gerações:** `GET /api/generations`, `/:id/data`,
  `POST /api/generations/delete|export`.
- **Nós/monitoramento:** `GET /api/environments`, `/api/orchestrators`,
  `POST /api/environments/adopt`, `/orchestrators/adopt`, `/:id/revoke`,
  `GET /api/storage/usage`.

Rotas internas manager (`:8081/internal/*`: dispatch loop, report, heartbeat,
telemetry, cleanup, artifacts, adopt/revoke) e orchestrator
(`:8082/internal/dispatch|abort|pairing/verify`) nunca são chamadas pelo browser.

## 5. Módulos do Frontend (`apps/web/app/`)

- `(studio)/dashboard` — visão geral, métricas de hardware, atalhos.
- `(studio)/datasets` e `datasets/[id]` + `datasets/[id]/annotate` — galeria,
  classes, anotação manual/verificação de boxes.
- `(studio)/treino` — abas de setup de treino (YOLO, Difusão, CLIP).
- `(studio)/jobs` — Action Center (fila, telemetria, drawer, lixeira/cleanup).
- `(studio)/difusao` e `(studio)/geracao` — forja difusiva e galeria de gerações.
- `(studio)/playground` — inferência interativa.
- `(studio)/models` — upload/download de checkpoints (LoRA/pesos).
- `(studio)/environments` — nós executores, adoção e monitor de VRAM.
- `login` — sessão single-user.
