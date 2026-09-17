# REPO_MAP.md — Hephaestus LLM Studio (Nível 1: Meso L1)

Mapa topológico de serviços, portas, rotas e persistência (~1.300 tokens).
Fonte: `infra/compose.yaml` e código real (17/09/2026). Consulte para localizar
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

Migrations canônicas: `services/api-principal/migrations/0001..0018.sql`.
- **Domínio aplicação/dados (escrita: api-principal):** `users`, `auth_state`,
  `datasets`, `dataset_versions`, `job_prepares` (aceite assíncrono ADR-0025 —
  0015 tabela, 0016 índice único parcial `state='preparing'`), `images`,
  `videos`, `boxes`, `classes`, `captions`, `image_embeddings` (pgvector),
  `models` (`kind` += `text_encoder` — 0018, arch só `flux-2-klein-4b`),
  `generations`, `generation_inputs` (img2img — 0017 tabela efêmera
  de inputs avulsos, sem GC; `used_at` marca consumo, linhas permanecem p/ auditoria).
- **Domínio execução (escrita: manager/orchestrator):** `jobs` (status inclui
  `preparing`/`dispatched` + `phase`/`message` — ADR-0024/ADR-0025),
  `job_artifacts`, `orchestrators`.
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
  `/jobs/cleanup`, `/jobs/:id/abort` (abort em `preparing` ⇒ `cancelling`);
  `GET /jobs/:id[/artifacts|/metrics|/events]`; submits com dataset aceitam
  em <1s com 202 `{jobId,status: preparing|queued}` (ADR-0025, spec 0.29.0 —
  erro assíncrono `prepare_failed:<code>:<msg>` lido via `GET /jobs/:id`);
  previews `POST /jobs/:id/autolabel|autotracker/preview` + `/apply`;
  telemetria `GET /api/telemetry`; geração `POST /jobs/diffusion/generate`.
- **Modelos/pesos:** `GET /api/models[/:id]`, `POST /api/models/upload|download`.
- **Galeria de gerações:** `GET /api/generations`, `/:id/data`,
  `POST /api/generations/delete|export`; upload efêmero img2img
  `POST /api/generations/inputs` (multipart campo `file`, png/jpeg/webp por
  sniff, teto 20 MiB → 201 `{id,filename,mimeType,width,height}`; objeto em
  `generation_inputs/{id}/{canonical}`); geração img2img via
  `POST /jobs/diffusion/generate` com `initImageId` XOR `initGenerationId` +
  `initStrength` (0.05–0.95, default 0.6 aplicado no yaml e no engine; wire
  null quando ausente).
- **Nós/monitoramento:** `GET /api/environments`, `/api/orchestrators`,
  `POST /api/environments/adopt`, `/orchestrators/adopt`, `/:id/revoke`,
  `GET /api/storage/usage`.

Rotas internas manager (`:8081/internal/*`: dispatch loop, report, heartbeat,
telemetry, cleanup, artifacts, adopt/revoke, `POST /internal/jobs/:id/prepare-complete`
e `prepare-fail` — ciclo `preparing` ADR-0025, Bearer `MANAGER_TOKEN`, fora do
contrato público) e orchestrator
(`:8082/internal/dispatch|abort|pairing/verify`) nunca são chamadas pelo browser.

Cadeia img2img (nunca via browser): api-principal emite
`generate.init_image_path: "{init_image_path}"` + `init_strength` no
`config.yaml` (placeholder literal, id real nunca vaza) → manager resolve a
ref (`initImageId` ⇒ `SELECT s3_key,md5 FROM generation_inputs` + `used_at`;
`initGenerationId` ⇒ `SELECT s3_key FROM generations`, `md5: null`) e despacha
`init_image_ref {s3_key, md5|null}` → orchestrator baixa (escopo
`S3Scope::GenerationInputs` ou `artifacts/`, md5 obrigatório só p/ upload —
galeria só registra o calculado) e faz staging em
`outputs/<job>/inputs/init.<ext>` (ext sanitizada do s3_key, fallback `png`),
substituindo o placeholder → engine consome
`generate.init_image_path`/`init_strength` (flux-2-klein: `image=` nativo sem
`strength`; sd15/sdxl: variante `*Img2ImgPipeline(**pipe.components)` com
`strength`; cache/spec inalterados).

Cadeia pesos custom flux-2 (treino+geração, nunca via browser): wire
`customModelId` (treino: XOR `baseModel`, default `sdxl`; geração: XOR já
existia, arch += `flux-2-klein-4b`) + `textEncoderModelId` (treino+geração,
só arch `flux-2-klein-4b`, senão 400; kind≠alvo ⇒ 400, inexistente ⇒ 404) →
api-principal emite `config.yaml` só com placeholders literais (treino
root-level `custom_checkpoint_path`/`text_encoder_path`; geração dentro de
`generate:`; id/path real nunca vaza) → manager resolve por SQL
(`SELECT s3_key,hash,kind,arch FROM models`, grava `custom_checkpoint` /
`text_encoder_ref {s3_key,md5}` em `params` + `text_encoder {s3_key,md5}` no
dispatch) → orchestrator baixa (escopo `models/`|`artifacts/`, md5
obrigatório) e stageia em `outputs/<job>/weights/{custom.safetensors,
text_encoder.safetensors}`, substituindo os placeholders → engine carrega
(geração: `Flux2KleinPipeline` SEM `from_single_file` + transformer custom via
`Flux2Transformer2DModel.from_single_file` + encoder/tokenizer override;
treino: transformer via `load_state_dict` sobre repo; cache isolado por
checkpoint+encoder).

## 5. Módulos do Frontend (`apps/web/app/`)

- `(studio)/dashboard` — visão geral, métricas de hardware, atalhos.
- `(studio)/datasets` e `datasets/[id]` + `datasets/[id]/annotate` — galeria,
  classes, anotação manual/verificação de boxes.
- `(studio)/treino` — abas de setup de treino (YOLO, Difusão, CLIP).
- `(studio)/jobs` — Action Center (fila, telemetria, drawer, lixeira/cleanup).
- `(studio)/difusao` e `(studio)/geracao` — forja difusiva e galeria de gerações
  (img2img: dropzone + slider `initStrength` no `GenerationPanel`, ação
  "Usar como input" da galeria via localStorage `geracao:initSource`).
- `(studio)/playground` — inferência interativa.
- `(studio)/models` — upload/download de checkpoints (LoRA/pesos).
- `(studio)/environments` — nós executores, adoção e monitor de VRAM.
- `login` — sessão single-user.
