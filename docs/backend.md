# Hephaestus LLM Studio — Esboço do Backend

> Origem: `IDEIA.md` + `docs/frontend.md` + decisões da revisão (auth single-user, SQLite só transporte, playground multi-motor, preempção de runner por VRAM, Postgres no principal, manager local).
> Componentes: **backend principal (Rust + Postgres)** → **manager local (Rust)** → **orquestrador(es) (Rust)** → **motores (Python)**. Tudo em Docker, execução local ou VPS/RunPod.

## 0. Escopo e base (resposta à revisão)

- **Nome vs. escopo:** `Hephaestus-LLM-Studio` é legado; o escopo implementado é **visão computacional/multimodal** (YOLO, Difusão LoRA, OpenCLIP). LLM textual (SFT/DPO/vLLM) é futuro, não entra no MVP.
- **Base presente no repo:** `IDEIA.md`, `arquitetura_studio_modular.png` e `ai-vision-training-studio.html` (verificado em disco). Conteúdo da IDEIA incorporado aqui + `frontend.md`.

## 1. Topologia e responsabilidades

```
[Next.js] ──HTTP/WS──> [backend principal Rust :8080 + Postgres]
  auth single-user · datasets (metadados no Postgres, blobs em disco) ·
  uploads 200MB · configs JSON/YAML · jobs API · chaves HF/Civitai
        ──HTTP──> [manager local]
          inventário de orquestradores · regras/VRAM policy · sync de ambientes ·
          roteamento de jobs (local vs remoto) · adopção (auto local / por chave remota)
            ├──> [orquestrador local (compose, auto-adotado via rede docker)]
            └──> [orquestrador remoto VPS/RunPod (IP público, adotado por chave)]
                    fila local + bootstrap de containers + downloads + cache
                      ──exec──> [trainer-yolo | trainer-difusao | trainer-clip | runner-* (Python)]
```

- Principal nunca toca GPU pesada; dono da verdade (Postgres + arquivos canônicos) e única superfície do front.
- Manager mora no ambiente local, **dono da fila central e da política VRAM**; sincroniza ambientes e despacha jobs conforme capacidade reportada.
- Orquestrador (local ou remoto) é **stateless e executor**: valida md5, faz build do dataset, sobe trainers/runners, reporta VRAM/logs e devolve artefatos. Sem Postgres próprio, só workspace efêmero + volumes de cache.
- Python só treina/infere e devolve artefatos + métricas + samples.
- **Principal↔Manager (interno):** HTTP em rede docker (`manager:8081`), auth por segredo pré-compartilhado (`MANAGER_TOKEN`, `Authorization: Bearer`); front nunca fala com o manager direto.
- **Postgres compartilhado:** um único Postgres do stack local; principal é dono de `datasets/images/auth/settings`, manager é dono de `jobs/runners/orchestrators/queue`. Sem transações cruzadas via API — cada um escreve só nas suas tabelas.
- **Resiliência:** fila e reservas de VRAM são **reconstruídas do Postgres** no boot do manager (`jobs` em `queued/dispatched/preparing` voltam a `queued` com `queue_reason=recovered`); nada crítico só em memória.
- **Direção de rede (NAT): outbound-first.** Modelo primário é **reverso**: orquestrador remoto abre WS persistente com o manager (`/orch/channel`, Bearer `heph_o_*` + pin) e recebe despachos por ele; artefatos voltam via `POST` do orquestrador para o manager/principal. Conexão inbound direta (manager→pod) é opcional, só quando há IP:porta alcançável.
- **Buffer de logs:** orquestrador mantém ring 2000 linhas em disco por job; principal cacheia últimas 1000; WS aceita `?since_seq=` para retomar após oscilação.

## 2. Auth (decisão: single-user local) — IMPLEMENTADO (Fatia 2)

> Implementado: contrato em `packages/contracts/openapi.yaml`, decisões em `docs/adr/0001-auth-single-user.md` (D1–D9). O que segue espelha o código; o ADR é a referência de rationale.

- `POST /api/auth/login {password}` → JWT HttpOnly + `GET /api/auth/me` + `POST /api/auth/logout`. Middleware Rust em tudo exceto `/api/auth/*` e `/health`.
- Tela `/login` no front (fora do protótipo) → implementada na Fatia 2 (ver `frontend.md` §10). Sem RBAC por enquanto; senha via env `STUDIO_PASSWORD` no primeiro boot.
- Chaves HF/Civitai: `PUT /api/settings/keys {hf_token, civitai_key}` (máscara no GET), fallback para env. Front nunca loga valores.
- Firmado (código): cookie `heph_session` `HttpOnly; SameSite=Lax; Path=/; Max-Age=604800` (7d; `Secure` só via env `SECURE_COOKIE=true`); hash Argon2id (`Params::default()`); JWT HS256 TTL 7d sem refresh (`iss/sub/iat/exp/jti`, leeway 30s); segredo de 32 B em tabela `auth_state` (override `AUTH_SECRET` hex 64 chars); bootstrap `STUDIO_PASSWORD` só no 1º boot (depois ignorado); modo setup fail-closed (`/health` 200 `auth:"setup_required"`, login 503 `setup_required`, protegidas 401); envelope de erro `{code,message}`; sem rate-limit na v1 (429 reservado `x-reserved`).
- `/api/auth/me` está fora do gate por prefixo (`/api/auth/*` isento, letra do §2) e **auto-valida o próprio cookie** no handler — é o gate daquela rota, não brecha (D9).
- `route_layer(require_auth)` **adiado p/ Fatia 3** (axum 0.7 dá panic com `route_layer` em router vazio, ver `routes.rs`); fail-closed desde já via `.fallback()` — sem cookie válido → 401 `unauthorized` (mesmo em rota inexistente); com sessão válida → 404 sem body (fora do envelope e da OpenAPI) — D9.

## 3. Dataset e banco — Postgres canônico, SQLite só no orquestrador

- **Postgres (principal + web):** metadados (`datasets, images, annotations, captions, classes, jobs, runners, orchestrators, settings/keys ref`). Blobs canônicos em disco no formato por engine: YOLO `images/ + labels/*.txt + data.yaml`, Difusão `img + .txt/.json + captions.jsonl`, CLIP `pares .parquet/.jsonl`.
- **Chunks (decisão): 8 MB** (faixa configurável 4–16 MB). Protocolo: `POST package/init {md5_zip, bytes, chunks}` → `PUT package/:id/chunk/:i` (md5 por chunk, retry individual, 4 em paralelo) → `POST package/:id/complete` (verifica md5 fim-a-fim, só então build). Local via volume dispensa chunking; remoto sempre chunkado com resume por `chunk_bitmap`.
- **Build no orquestrador:** ele pode materializar um `build.sqlite` **interno/temporário** ou montar direto via `JSON/YAML` — o que for melhor por engine — e então **reconstrói a árvore exata do motor** (`labels/*.txt`, `data.yaml` com paths remapeados). Python nunca lê SQLite do transporte; SQLite é detalhe interno e descartável do orquestrador.

## 4. Jobs, trainers sob demanda e cache

- Tipos: `yolo_train | difusao_train | clip_train | autolabel | autotracker | download_model | playground`.
- Ciclo: `queued → dispatched → preparing(env+dataset) → running → paused? → done|failed|cancelled`, com `POST /api/jobs/:id/{pause,abort}` + `POST /api/jobs/:id/resume`. **Fila central no manager** (posição + motivo `waiting_vram|waiting_slot` visíveis no front); orquestrador só executa o que recebe e reporta `vram_used/total` + heartbeat.
- **Pause = checkpoint + libera VRAM** (não `docker pause`): `pause` pede `save_checkpoint`, derruba o trainer e mantém `last.ckpt`; `resume` recria do checkpoint. Sem checkpoint do engine, pause é recusado (`409 checkpoint_unsupported`) e só `abort` vale.
- Orquestrador sobe **um container `trainer-<engine>-<jobid>` por job** a partir de imagens por engine (isola deps: ultralytics vs. diffusers/kohya vs. open_clip). Ao destruir o container, **cache persiste fora**: volumes `models/`, `datasets-cache/`, `outputs/` mapeados no host/remoto.
- **Imagens (decisão): uma por engine** (yolo, difusão, clip, autolabel/tracker, runner), **base no estável mais recente testado** (não pinar no 12.4/2.4.1 do protótipo; registrar a versão validada em `engines.yaml`), **build local no compose** (sem registry externo por enquanto; tags `hephaestus/trainer-<engine>:local`).
- Paralelismo por VRAM, não fixo: com folga (ex. RunPod 80 GB) roda 2+ trainers e enfileira o resto (FIFO + cancel manual). Fila visível no front com posição e motivo (`waiting_vram`).
- Entrada do trainer: zip do §3 + `config.yaml` gerado pelo principal (hiperparams do front) + mounts de modelos solicitados.

## 5. Playground / runners (decisão: todos os motores, preemptível)

- Um `runner-<engine>` por motor ativo (difusão gera imagem, YOLO infere, CLIP busca), mesmo design das demais abas.
- Lifecycle: sobe no primeiro uso → fica warm → desliga por (a) botão "Matar runner", (b) idle timeout configurável, (c) **preempção quando treino precisa de VRAM mínima e não há folga**. Front mostra `runner ativo · VRAM X GB · [Matar]` + toast quando preemptado.
- **TTL (decisão): padrão 15 min** (`difusao: 10 min`, `yolo: 30 min`, `clip: 20 min`, sobrescrevível por ambiente). Contagem só com fila vazia e sem inferência ativa; front mostra countdown + aviso 2 min antes; qualquer uso reseta. Com pressão de VRAM o idle é morto na hora sem esperar o TTL.
- Nunca dividir container trainer/runner: evita contaminação de deps e permite matar inferência sem tocar no treino.

## 6. Gerência de VRAM — mínima por serviço/modelo (não global)

- Fonte: `nvidia-smi` (orquestrador, polling ~2s) + `torch.cuda` nos motores como fallback; exposto em `WS /ws/telemetry {vram_used, vram_total, jobs_ativos}`.
- `vram_min` é **por serviço + modelo + modo**: ex. inferência Flux2-Klein 9B 8-bit ≈ **10–16 GB**, treino do mesmo base bem acima; YOLO-nano bem abaixo. Cada `POST /api/jobs` informa `engine + model_id + modo (train/infer)` e o manager resolve `vram_min` via tabela (`policy/vram-table.yaml`, sobrescrevível por ambiente) + headroom.
- Tabela inicial (`policy/vram-table.yaml`):
  ```yaml
  defaults: { headroom_gb: 2, measure_margin: 1.25 }
  entries:
    - { engine: difusao, model: flux2-klein-9b-8bit, mode: infer, vram_min_gb: 16 }
    - { engine: difusao, model: flux2-klein-9b,      mode: train, vram_min_gb: 28 }  # medir; treino >> infer
    - { engine: difusao, model: sdxl-1.0,            mode: train, vram_min_gb: 20 }
    - { engine: yolo,    model: yolo11n,             mode: train, vram_min_gb: 6 }
    - { engine: yolo,    model: yolo11m,             mode: train, vram_min_gb: 10 }
    - { engine: clip,    model: ViT-B-32,            mode: train, vram_min_gb: 10 }
  ```
  Sem entrada = `default_train_gb: 16` + aviso `unmeasured` no front.
- Medição: `nvidia-smi` 2s no orquestrador + `torch.cuda.max_memory_allocated` reportado pelo motor no fim do warmup; manager aplica `medido * 1.25` e sugere atualizar o yaml (`POST /api/settings/vram-table/propose`).
- Regra: se `livre >= min` → sobe em paralelo; senão treino entra em `queued(waiting_vram)` e, se o bloqueio for um runner, o runner é drenado/morto primeiro. **Treino nunca é morto por falta de VRAM, só enfileirado.**
- Config por ambiente (via manager): `max_parallel_trainers` (teto, padrão 2; efetivo = `min(teto, floor((vram_total - headroom) / vram_min_do_job))`), `vram_headroom_gb`, `runner_idle_ttl_s`, `vram-table` por modelo.
- **Preempção (decisão):** runner idle → mata direto + toast com motivo; runner com inferência ativa → modal "Treino X precisa de N GB. Matar runner?" com countdown 30s (expirar = mata). Log de auditoria `runner_preempted {by_job}`. **Sem usuário (expirado/madrugada):** mata ao expirar, a inferência em voo retorna `409 runner_preempted {job_id}` e o playground mostra toast + estado vazio (sem corromper resposta parcial).
- **Multi-GPU (MVP): 1 job = 1 GPU** (`CUDA_VISIBLE_DEVICES=gpu_index` escolhido pelo orquestrador; `gpus:[{index, vram_total, vram_used}]`, sem DDP). DDP/Accelerate multi-GPU para um único treino fica fora do MVP.

## 7. Samples por ciclo (health visual do treino)

- Motores salvam a cada ciclo (época YOLO/CLIP, N steps difusão): `samples/cycle_{n}/{img, meta.json}` com métricas do ciclo.
- Orquestrador coleta e o principal serve: `GET /api/jobs/:id/samples?cycle=N&limit=8` + `GET /api/jobs/:id/metrics` (loss, mAP, recall, step/epoch). Front renderiza a grade "Samples do ciclo N" ao lado das curvas.

## 8. Downloads HF/Civitai + bootstrap e adoção de orquestradores

- `POST /api/models/download {url, engine, dest}` → manager roteia ao orquestrador do ambiente ativo, que baixa com token da settings/env, verifica hash, salva em `models/<engine>/` (volume persistente, fora dos trainers) e notifica via WS.
- Bootstrap remoto ao subir container: orquestrador mapeia `datasets-cache/<jobid>/` (build do §3), `models/<engine>/` solicitados e `outputs/<jobid>/`; na conclusão faz o caminho inverso (`.safetensors`, pesos YOLO/CLIP, imagens processadas, logs, samples) de volta ao principal com md5.
- **Adoção (decisão: token colado):** orquestrador remoto (VPS/RunPod com IP público) ao subir **gera pairing token + fingerprint**; o usuário cola no front (`Conectar Pod`) e o manager adota (`POST /api/orchestrators/adopt {endpoint, key}`), passa a health-checkar e sincronizar regras. **Local:** o compose sobe `principal + manager + orquestrador-local` juntos e o manager **auto-adota via rede docker**, sem chave.
- **Formato do token (proposta):**
  - Pairing (uso único, curta duração): `heph_p_<32 chars base32 sem ambíguos, em grupos 4-4-4>` ex. `heph_p_7KQ2-9MZX-4TWD-8FHA`. Validade 15 min, single-use, rate-limit 5 tentativas. Exibido uma vez no log/boot do orquestrador.
  - Credencial longa (pós-adoção): `heph_o_<64 hex>` (256 bits), guardada no Postgres **só como hash SHA-256**, exibida nunca mais. Autentica manager→orquestrador via `Authorization: Bearer` + TLS com pin do fingerprint.
  - Rotação: `POST /api/orchestrators/:id/rotate` gera nova `heph_o_*` e invalida a anterior (overlap de 5 min); `POST .../revoke` corta na hora; novo pareamento exige gerar outro `heph_p_*` no orquestrador (`POST /orch/pairing-code` local ao remoto ou var de boot). Local usa segredo pré-compartilhado do compose, sem pairing.
- **TLS (decisão: híbrido):** padrão self-signed + pin SPKI (TOFU no adopt, funciona com IP puro de VPS/RunPod); se o remoto tiver domínio, usa Let's Encrypt. Manager guarda `fingerprint + endpoint + credencial hash`; falha de pin = recusa + alerta no front.
- **Retry (decisão: backoff simples):** adopt com 3 tentativas (1s/3s/10s) + erro visível com "Tentar de novo"; health-check 15s com 2 falhas seguidas = `degraded`, 5 = `offline` (jobs ficam `queued`, sem migração automática).
- **Postgres (decisão: só local):** apenas no stack do principal/manager. Orquestradores remotos são stateless (workspace + cache em volumes); todo estado durável volta via retorno de artefatos/métricas para o Postgres local.
- Novo serviço local **`manager` (Rust):** inventário `orquestradores {id, endpoint, tipo local|remoto, gpus, vram_total, status}`, distribuição de jobs, sync de `vram-table`/filas/creds ref, e roteamento de downloads/datasets. Principal continua sendo o único BFF do front.

## 9. API mínima a implementar (principal expõe, orquestrador executa)

```
auth:     POST /api/auth/login, GET /api/auth/me, POST /api/auth/logout  → implementado (ADR-0001/openapi)
health:   GET /health → {status, service, auth: ready|setup_required}  → implementado (campo `auth` novo, ADR-0001 D3)
settings: PUT/GET /api/settings/keys {hf_token, civitai_key, openai_key, anthropic_key, vllm_endpoint}, GET /api/settings/vram-policy
datasets: GET/POST /api/datasets, GET/DELETE /api/datasets/:id
          POST /api/datasets/:id/upload (200MB), GET /api/datasets/:id/images
          PUT  /api/datasets/:id/images/:img/{boxes,caption}
          POST /api/datasets/:id/export, POST /api/datasets/import
          POST /api/datasets/:id/package (gera zip manifest+md5 p/ orquestrador)
models:   GET /api/models (lista pesos em disco/banco p/ dropdowns), POST /api/models/upload, POST /api/models/download
preview:  POST /api/preview/{autolabel,autotracker,generate,search} (job efêmero ou runner quente, sem fila de treino)
jobs:     POST /api/jobs/{yolo,difusao,clip,autolabel,autotracker,playground}
          GET /api/jobs/:id, POST /api/jobs/:id/{pause,abort,resume}, GET /api/jobs/queue
          GET /api/jobs/:id/metrics, GET /api/jobs/:id/samples, GET /api/jobs/:id/artifacts
runners:  POST /api/runners/{difusao,yolo,clip}/up, POST /api/runners/:id/kill, GET /api/runners
          POST /api/runners/:id/infer {prompt|image|query} (inferência interativa; 409 se preemptado)
orchestrators (via manager): GET /api/orchestrators, POST /api/orchestrators/adopt {endpoint,key},
          POST /api/orchestrators/:id/{enable,disable,remove}, GET /api/orchestrators/:id/health
          # alias UI: /api/environments* responde o mesmo que /api/orchestrators* (front usa "Ambientes")
ws:       /ws/jobs/:id/logs?since_seq=, /ws/telemetry
```

- Nota Fatia 2 (D9): `/api/auth/me` valida o próprio cookie (isento do gate por prefixo, §2); `route_layer` adiado p/ Fatia 3 — fallback fail-closed (sem sessão → 401 mesmo em rota inexistente; com sessão → 404 sem body, fora da OpenAPI).

## 10. Schema Postgres (só local — principal/manager)

```sql
users(id UUID PK, password_hash TEXT NOT NULL, created_at TIMESTAMPTZ);  -- single-user; 1 linha
auth_state(id SMALLINT PK CHECK(id=1), jwt_secret BYTEA CHECK(octet_length=32), created_at TIMESTAMPTZ);  -- segredo HS256, linha única (ADR-0001 D2/D4; resolve T1)
settings(key TEXT PK, value_enc TEXT, updated_at TIMESTAMPTZ);            -- hf_token, civitai_key (cifrado app-level), vram-table ref
orchestrators(id UUID PK, name TEXT, endpoint TEXT UNIQUE, kind TEXT,     -- local|remoto
  fingerprint TEXT, token_hash TEXT, gpus JSONB, vram_total_gb INT,
  status TEXT, last_heartbeat TIMESTAMPTZ);
datasets(id UUID PK, slug TEXT UNIQUE, title TEXT, category TEXT,          -- difusao|openclip|yolo
  type TEXT, task TEXT, format TEXT, status TEXT, source TEXT, size_bytes BIGINT,
  images_count INT DEFAULT 0, labeled_count INT DEFAULT 0, created_at TIMESTAMPTZ);
classes(id UUID PK, dataset_id UUID FK, name TEXT, idx INT, color TEXT, UNIQUE(dataset_id, name));
images(id UUID PK, dataset_id UUID FK, filename TEXT, path TEXT, w INT, h INT,
  md5 TEXT, bytes BIGINT, split TEXT DEFAULT 'train', created_at TIMESTAMPTZ, UNIQUE(dataset_id, filename));
boxes(id UUID PK, image_id UUID FK, class_id UUID FK,
  x FLOAT, y FLOAT, w FLOAT, h FLOAT, conf FLOAT NULL, origin TEXT, track_id INT NULL);
captions(image_id UUID PK FK, text TEXT, origin TEXT, model TEXT, updated_at TIMESTAMPTZ);
videos(id UUID PK, dataset_id UUID FK, filename TEXT, path TEXT, fps FLOAT, frames INT, md5 TEXT);
dataset_versions(id UUID PK, dataset_id UUID FK, manifest JSONB, created_at TIMESTAMPTZ);
models(id UUID PK, engine TEXT, name TEXT, path TEXT, source TEXT,         -- hf|civitai|upload
  url TEXT NULL, hash TEXT NULL, bytes BIGINT, created_at TIMESTAMPTZ);
jobs(id UUID PK, kind TEXT, dataset_id UUID NULL FK, engine TEXT, model TEXT, mode TEXT,
  params JSONB, config_yaml TEXT, status TEXT, queue_reason TEXT NULL,
  orchestrator_id UUID NULL FK, vram_min_gb INT, progress FLOAT,
  epoch INT, step INT, metrics JSONB, created_at TIMESTAMPTZ, finished_at TIMESTAMPTZ NULL);
job_artifacts(id UUID PK, job_id UUID FK, kind TEXT, path TEXT, md5 TEXT, bytes BIGINT);
job_samples(job_id UUID FK, cycle INT, idx INT, image_path TEXT, meta JSONB, PRIMARY KEY(job_id, cycle, idx));
runners(id UUID PK, engine TEXT, model TEXT, orchestrator_id UUID FK,
  status TEXT, vram_gb INT, last_used TIMESTAMPTZ);
```

- Índices: `images(dataset_id)`, `boxes(image_id)`, `jobs(status)`, `job_artifacts(job_id)`.
- Nota (ADR-0001 T3, casing): domínio auth trafega em camelCase (`userId`, `loggedAt`); demais bodies do §9 usam snake_case (ex. settings `hf_token`). Política global de casing a definir antes da Fatia 3.
- Regra: contadores do dataset via trigger/view a partir de `images/boxes/captions`; chaves externas com `ON DELETE CASCADE` de dataset→filhos.
- **Split:** coluna `images.split (train|val)`; padrão 80/20 estratificado no package com override manual na galeria (seletor train/val por imagem).
- **Consistência disco×banco:** Postgres é a verdade; `PUT boxes/caption` atualiza o banco e materializa o `.txt` em disco com debounce (~2s). O package sempre gera do banco.
- **Snapshot:** ao despachar job, congela `dataset_versions{manifest}` e o zip aponta para a versão; edição posterior não afeta treino em voo (trava lógica por versão, não por dataset).

## 11. Schemas, retenção e setup inicial

- `manifest.json` (transporte): `{dataset_id, slug, category, engine, files:[{path, md5, bytes}], md5_zip, bytes, chunks, created_at}`.
- `config.yaml` por job: comum `{job_id, engine, model, mode, dataset_path, output_path, seed}` + específico:
  - yolo: `{model, epochs, batch, imgsz, lr0, optimizer, augment:{mosaic, mixup_flip}}`;
  - difusao: `{base_model, trigger_word, rank, alpha, optimizer, steps, lr, cfg}`;
  - clip: `{backbone, embed_dim, loss, lr, warmup, batch, epochs}`.
- `engines.yaml`: `{engine, image, cuda, torch, validated_at}` — ex. `trainer-difusao: hephaestus/trainer-difusao:local`.
- Retenção: samples últimos 5 ciclos ou 500 MB/job; logs 10 MB + 30 dias; artifacts guarda `best + last`, resto com GC manual (`DELETE /api/jobs/:id/artifacts?keep=best,last`).
- Setup: `STUDIO_PASSWORD` no primeiro boot (hash Argon2 em `users`); troca via CLI `studio reset-password` (sem expor rota); chaves cifradas app-level com `STUDIO_MASTER_KEY` (nunca em log); `compose.yaml` sobe `db (postgres:16) + principal (:8080) + manager + orquestrador-local (socket docker) + web (next)` com volumes `pgdata, datasets, models, outputs`.
- **Execução sem DinD (RunPod padrão):** orquestrador opera em 2 modos — `docker` (socket disponível) ou `subprocess` (venv/python direto no mesmo host). Pods sem socket usam modo subprocess; template com DinD é opcional, não requisito.
