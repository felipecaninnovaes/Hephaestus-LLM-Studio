# Hephaestus LLM Studio — Esboço do Backend

> Origem: `IDEIA.md` + `docs/frontend.md` + decisões da revisão (auth single-user, SQLite só transporte, playground multi-motor, preempção de runner por VRAM, Postgres no principal, manager local).
> Componentes: **backend principal (Rust + Postgres)** → **manager local (Rust)** → **orquestrador(es) (Rust)** → **motores (Python)**. Tudo em Docker, execução local ou VPS/RunPod.

## 0. Escopo e base (resposta à revisão)

- **Nome vs. escopo:** `Hephaestus-LLM-Studio` é legado; o escopo implementado é **visão computacional/multimodal** (YOLO, Difusão LoRA, OpenCLIP). LLM textual (SFT/DPO/vLLM) é futuro, não entra no MVP.
- **Base presente no repo:** `IDEIA.md`, `arquitetura_studio_modular.png` e `ai-vision-training-studio.html` (verificado em disco). Conteúdo da IDEIA incorporado aqui + `frontend.md`.

## 1. Topologia e responsabilidades

```
[Next.js] ──HTTP/WS──> [backend principal Rust :8080 + Postgres]
   auth single-user · datasets (metadados no Postgres, blobs canônicos no bucket S3/SeaweedFS — ADR-0003 D1) ·
   uploads 200MB · configs JSON/YAML · jobs API · chaves HF/Civitai
        ──HTTP──> [manager local]
          inventário de orquestradores · regras/VRAM policy · sync de ambientes ·
          roteamento de jobs (local vs remoto) · adopção (auto local / por chave remota)
            ├──> [orquestrador local (compose, auto-adotado via rede docker)]
            └──> [orquestrador remoto VPS/RunPod (IP público, adotado por chave)]
                    fila local + bootstrap de containers + downloads + cache
                      ──exec──> [trainer-yolo | trainer-difusao | trainer-clip | runner-* (Python)]
```

- Principal nunca toca GPU pesada; dono da verdade (Postgres como verdade relacional + bucket S3 como verdade binária, com as linhas de `images` como índice — ADR-0003 D1) e única superfície do front. Disco local no principal é só efêmero (spool de upload em tempfile).
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
- `route_layer(require_auth)` **plugado na Fatia 3a** (ADR-0002 D9; quita a divergência D9 anotada em `auth/routes.rs`): sub-router `protected` com `route_layer` depois dos `.route()`; `.fallback(gate_fallback)` da raiz cobre só caminho **não roteado**. Coexistem dois 404: rota roteada + id inexistente → 404 `not_found` **com** envelope (handler); caminho não roteado + sessão válida → 404 **sem body** (fora do envelope e da OpenAPI).

## 3. Dataset e banco — Postgres canônico, SQLite só no orquestrador

- **Postgres (principal + web):** metadados (`datasets, images, boxes, captions, classes, videos, jobs, runners, orchestrators, settings/keys ref`). Só mídia vira objeto no bucket; anotação é linha no banco (`boxes`, `captions` — ADR-0003 D6). Os formatos por engine (YOLO `images/ + labels/*.txt + data.yaml`, Difusão `img + .txt/.json + captions.jsonl`, CLIP `pares .parquet/.jsonl`) **não são canônicos**: são artefatos de build materializados no tempdir do orquestrador a partir do Postgres e mortos no `finally` (ADR-0003 D1/D6).
- **Chunks (decisão): 8 MB** (faixa configurável 4–16 MB) **só no transporte principal→orquestrador REMOTO** (ADR-0003 D9: `POST package/init {md5_zip, bytes, chunks}` → `PUT package/:id/chunk/:i` → `POST package/:id/complete`; projeto da fatia 3e/4, ainda não implementado). Entre principal e bucket local não há chunk nenhum: upload é multipart com spool em tempfile + `put_object` de length exato.
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
settings: PUT/GET /api/settings/keys {hfToken, civitaiKey, openaiKey, anthropicKey, vllmEndpoint}, GET /api/settings/vram-policy
datasets: GET/POST /api/datasets, GET/DELETE /api/datasets/:id  → implementado (Fatia 3a; ADR-0002/openapi)
           POST /api/datasets/:id/upload, GET /api/datasets/:id/images,
           GET  /api/datasets/:id/images/:imageId, GET /api/datasets/:id/images/:imageId/data,
           PUT  /api/datasets/:id/images/:imageId/boxes, PUT /api/datasets/:id/images/:imageId/caption
              → implementado (Fatia 3b; ADR-0003/openapi 0.3.0)
           PUT  /api/datasets/:id/classes,
           DELETE /api/datasets/:id/images/:imageId, POST /api/datasets/:id/images/:imageId/restore,
           DELETE /api/datasets/:id/trash
              → implementado (Fatia 3g; ADR-0005/openapi 0.4.0)
           POST /api/datasets/:id/export, POST /api/datasets/import
           POST /api/datasets/:id/package (gera zip manifest+md5 p/ orquestrador)
             → adiados para a fatia 3e (ADR-0003 D9; backup interim = console + `mc mirror`)
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

- Nota Fatia 2 (D9): `/api/auth/me` valida o próprio cookie (isento do gate por prefixo, §2); `route_layer` plugado na Fatia 3a (ADR-0002 D9) — fallback fail-closed (sem sessão → 401 mesmo em rota inexistente; com sessão → 404 sem body, fora da OpenAPI).
- Nota Fatia 3a (ADR-0002 D1, casing): TODAS as chaves de body/query/response de `/api/*` são camelCase (o teste `json_property_names_are_camel_case` rejeita o resto). **Os nomes listados no §9 são colunas (§10) ou campos de transporte, não chaves JSON** — ex.: settings `{hfToken, …}` no wire vs colunas `hf_token` em `settings`; datasets `sizeBytes/imagesCount/lastModified` no wire vs colunas `size_bytes/images_count/updated_at`. A rota `PUT/GET /api/settings/keys` ainda **não está implementada**; as colunas de `settings` permanecem snake_case.
- Nota Fatia 3b (ADR-0003, spec 0.3.0 — shapes reais em `services/api-principal/src/datasets/models.rs`, tabela de rotas ≡ `PROTECTED_ROUTES` em `src/auth/routes.rs`):
  ```
   POST /api/datasets/:id/upload                  200 400 401 404 503
   GET  /api/datasets/:id/images                  200 400 401 404
   GET  /api/datasets/:id/images/:imageId         200 401 404
   GET  /api/datasets/:id/images/:imageId/data    200 401 404 503
   PUT  /api/datasets/:id/images/:imageId/boxes   200 400 401 404
   PUT  /api/datasets/:id/images/:imageId/caption 200 400 401 404
   PUT  /api/datasets/:id/classes                 200 400 401 404 409
   DELETE /api/datasets/:id/images/:imageId       204 401 404
   POST /api/datasets/:id/images/:imageId/restore 200 204 401 404 503
   DELETE /api/datasets/:id/trash                 204 401 404
   ```
  `POST /:id/upload` (multipart campo `files`): resposta por item `{imageId,filename,status,reason,bytes,width,height}`, `status ∈ stored|duplicate|rejected|failed`, `reason ∈ duplicate_filename|unsupported_media|too_large|storage_error` (`src/datasets/handlers.rs::MAX_FILE_BYTES` = 200 MiB por arquivo; corpo TOTAL limitado por `UPLOAD_BODY_LIMIT_BYTES` = 200 MiB + 8 MiB de envelope em `src/auth/routes.rs`, excesso → 413 `invalid_request` no envelope; lote todo indecodível → 400; bucket fora → 503 `storage_unavailable`). `filename` do wire = nome canônico server-side (stem sanitizado + extensão do sniff por magic bytes), nunca o nome do form. `GET /:id/images?limit&offset&split&labeled&deleted` → `ImagePage{items,total,limit,offset}` (`limit` default 50, máx 200, `offset` default 0; `split=train|val`, `labeled=true|false`, inválido → 400; `labeled` respeita a taxonomia do trigger: `yolo_txt` ⇒ boxes, demais ⇒ captions; `deleted=true|false` (default false — `true` lista a lixeira). `GET` detail → `ImageDetail` flat (= `Image` + `boxes[]` + `caption|null`); `url` por imagem é híbrida D3: com `S3_PUBLIC_ENDPOINT_URL` → presigned GET (TTL `S3_URL_TTL_SECS`, default 3600), sem ela → `/api/datasets/:id/images/:imageId/data`. `GET /data` existe **incondicionalmente** (proxy do objeto, `Cache-Control: private, max-age=31536000, immutable`; objeto ausente ⇒ 404, bucket fora ⇒ 503). `PUT boxes` = substituição total transacional (`DELETE` + `INSERT` com `RETURNING` numa transação; erro ⇒ rollback + 500): corpo `{boxes:[{classId,x,y,w,h,conf?,origin?,trackId?}]}` (cap 1000, `x/y/w/h` e `conf` em `0..=1`, `origin ∈ manual|autotracker|import` default `manual`, `classId` tem de pertencer ao dataset senão 400 seco); `PUT caption` = upsert de statement único com `RETURNING` (`{text,origin?,model?}`, `text` 1..8000 chars, `model` ≤ 255 chars). `id`/`imageId` não-UUID → 404 `not_found` (ADR-0002 D8 replicado); erro novo `storage_unavailable` (503, ADR-0003 D10); wire camelCase, `deny_unknown_fields` nos inputs.
- Nota Fatia 3b — storage/env (código: `src/main.rs::load_storage`, `infra/compose.yaml`): `STORAGE_BACKEND=mock|s3` (default `mock` nos testes, `s3` no compose); modo `s3` exige `S3_ENDPOINT_URL` + `S3_ACCESS_KEY` + `S3_SECRET_KEY` (fail-fast no boot sem ecoar valor); `S3_BUCKET` (default `heph-data`), `S3_PUBLIC_ENDPOINT_URL` (default `http://localhost:8333`; ausente ⇒ `url` vira fallback `/data`), `S3_URL_TTL_SECS` (default 3600, validado no boot em `1..=604800`, máx SigV4 de 7 dias). `Dataset.source` no wire **permanece** (contrato não quebra) mas é derivado server-side (`src/datasets/models.rs::derived_source`): `s3://{bucket}/datasets/{id}/` quando `images_count > 0`, `null` em dataset vazio. `Dataset.classes` é objeto completo `{id,name,idx,color}` (o `id` alimenta o `classId` do PUT boxes — gap fechado na 3b.7).
- Nota Fatia 3g (ADR-0005, spec 0.4.0 — shapes reais em `services/api-principal/src/datasets/models.rs`, tabela de rotas ≡ `PROTECTED_ROUTES` em `src/auth/routes.rs`):
  `PUT /:id/classes` = substituição total com reconciliação por id (`{classes:[{id?,name}]}`): id presente = rename preservando id (caixas intocadas); ausente = cria; ordem do array = idx 0..n-1; cor rederivada da paleta server-side (nunca do cliente); validação pura em `models.rs` (regex única reutilizada, ≤200, nomes/ids únicos, dedupe silencioso proibido); remoção de classe com caixas ⇒ 409 `classes_in_use` (guard — que conta caixas de imagens ativas **e** da lixeira, conservador; o CASCADE do FK regeneraria ids e orfanaria as caixas); dance de `UNIQUE(name/idx)` na transação com ordem OBRIGATÓRIA guard → fase 1 (tmp via `id.simple()` por causa do `CHECK` de name, `idx+1000000`) → **DELETE das removidas** → fase 2 (finais) → INSERT novas (o DELETE entre as fases libera os slots de `idx` — sem ele, remover classe com `idx` menor que o destino renumerado de um mantido viola `UNIQUE(dataset_id, idx)` e vira 500; coberto por teste db). Lixeira: `DELETE /:id/images/:imageId` = soft delete (204, sem sweep, objeto intocado; 404 se inexistente/já deletada/UUID inválido); `POST /:id/images/:imageId/restore` (204 sem conflito; conflito de filename → rename `{stem}_restaurado{ext}` com desambiguação + `copy_object` server-side + delete da key antiga best-effort pós-commit → 200 `{filename}`; copy falha → 503 `storage_unavailable`, nada parcial); `DELETE /:id/trash` = purge REAL (CASCADE + sweep best-effort por prefixo de imagem pós-commit, 204 idempotente); `GET /:id/images?deleted=true` lista a lixeira; `Dataset.trashCount` derivado (badge). O parágrafo "PUT boxes … classId tem de pertencer ao dataset senão 400 seco" continua verdadeiro.

## 10. Schema Postgres (só local — principal/manager)

```sql
users(id UUID PK, password_hash TEXT NOT NULL, created_at TIMESTAMPTZ);  -- single-user; 1 linha
auth_state(id SMALLINT PK CHECK(id=1), jwt_secret BYTEA CHECK(octet_length=32), created_at TIMESTAMPTZ);  -- segredo HS256, linha única (ADR-0001 D2/D4; resolve T1)
settings(key TEXT PK, value_enc TEXT, updated_at TIMESTAMPTZ);            -- hf_token, civitai_key (cifrado app-level), vram-table ref
orchestrators(id UUID PK, name TEXT, endpoint TEXT UNIQUE, kind TEXT,     -- local|remoto
  fingerprint TEXT, token_hash TEXT, gpus JSONB, vram_total_gb INT,
  status TEXT, last_heartbeat TIMESTAMPTZ);
datasets(id UUID PK, slug TEXT UNIQUE, title TEXT, category TEXT,          -- difusao|openclip|yolo
  type TEXT, task TEXT, format TEXT, status TEXT, size_bytes BIGINT,
  images_count INT DEFAULT 0, labeled_count INT DEFAULT 0, created_at TIMESTAMPTZ);
  -- IMPLEMENTADO (Fatia 3b; `migrations/0003_images.sql`): a coluna `source` SAIU do banco
  -- (`ALTER TABLE datasets DROP COLUMN source` — era "caminho em disco", conceito morto,
  -- ADR-0003 D5); no wire `Dataset.source` permanece como derivado server-side
  -- (`s3://{bucket}/datasets/{id}/` quando images_count > 0, null enquanto vazio).
  -- IMPLEMENTADO (Fatia 3a; ADR-0002/openapi): colunas NOT NULL/DEFAULT conforme `0002_datasets.sql`;
  -- CHECKs de domínio em category/type/task/format/status (valores do §10 à letra) + CHECKs
  -- size_bytes/images_count/labeled_count >= 0; o invariante `labeled_count <= images_count`
  -- NÃO é CHECK (não deferrável) e sim obrigação do trigger da 3b (ver §10 "Regra" abaixo);
  -- adição ADR-0002 D4: updated_at TIMESTAMPTZ NOT NULL DEFAULT now() + trigger
  -- tg_set_updated_at (BEFORE UPDATE), exposto no wire como lastModified.
classes(id UUID PK, dataset_id UUID FK, name TEXT, idx INT, color TEXT, UNIQUE(dataset_id, name));
  -- IMPLEMENTADO (Fatia 3a; ADR-0002/openapi): name CHECK ^[A-Za-z0-9_]{1,64}$, idx CHECK >= 0,
  -- color CHECK ^#[0-9a-f]{6}$ (paleta §3 front), UNIQUE(dataset_id, name) + UNIQUE(dataset_id, idx),
  -- FK ON DELETE CASCADE; índice classes(dataset_id, idx).
images(id UUID PK, dataset_id UUID FK CASCADE, filename TEXT CHECK 1..255, object_key TEXT UNIQUE,
  bytes BIGINT CHECK >= 0, width INT CHECK > 0, height INT CHECK > 0,
  md5 TEXT CHECK ^[0-9a-f]{32}$, sha256 TEXT CHECK ^[0-9a-f]{64}$,
  media_type TEXT CHECK jpeg|png|webp, split TEXT DEFAULT 'train' CHECK train|val,
  created_at TIMESTAMPTZ, UNIQUE(dataset_id, filename));
  -- IMPLEMENTADO (Fatia 3b; `migrations/0003_images.sql` à letra): `object_key` no lugar de
  -- `path` (chave legível `datasets/{dataset_id}/images/{image_id}/{filename_sanitizado}`);
  -- +`sha256`/`media_type`; índices `images(dataset_id)` e `images(dataset_id, split)`.
  -- IMPLEMENTADO (Fatia 3g; `migrations/0005_image_soft_delete.sql`): `deleted_at TIMESTAMPTZ NULL`
  -- (soft delete — linha some das queries, objeto intocado); unique parcial
  -- `(dataset_id, filename) WHERE deleted_at IS NULL` (re-upload de filename na lixeira
  -- nasce linha nova); índice parcial da lixeira `(dataset_id) WHERE deleted_at IS NOT NULL`;
  -- `heph_refresh_dataset_counters` filtra `deleted_at IS NULL` (trigger AFTER UPDATE
  -- recalcula de graça no soft delete/restore).
boxes(id UUID PK, image_id UUID FK CASCADE, class_id UUID FK CASCADE,
  x/y/w/h DOUBLE CHECK 0..1, conf DOUBLE NULL, origin TEXT CHECK manual|autotracker|import,
  track_id INT NULL);
  -- ÍNDICES: `boxes(image_id)`, `boxes(class_id)`.
captions(image_id UUID PK FK CASCADE, text TEXT CHECK 1..8000, origin TEXT CHECK manual|autotracker|import,
  model TEXT NULL, updated_at TIMESTAMPTZ + trigger tg_set_updated_at da 0002);
videos(id UUID PK, dataset_id UUID FK CASCADE, filename TEXT CHECK 1..255, object_key TEXT UNIQUE,
  fps DOUBLE NULL, frames INT NULL CHECK >= 0, md5 TEXT CHECK ^[0-9a-f]{32}$,
  bytes BIGINT CHECK >= 0, created_at TIMESTAMPTZ, UNIQUE(dataset_id, filename));
  -- IMPLEMENTADO (Fatia 3b): idem objeto, índice `videos(dataset_id)`; SEM rota de escrita
  -- na 3b (nasce agora porque DDL é estático e a FK CASCADE vem junto das irmãs — ADR-0003 D5).
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

- Índices: `images(dataset_id)`, `images(dataset_id, split)`, `images(dataset_id, filename) parcial ativa + images(dataset_id) parcial lixeira (0005)`, `boxes(image_id)`, `boxes(class_id)`, `videos(dataset_id)`, `classes(dataset_id, idx)`, `jobs(status)`, `job_artifacts(job_id)`.
- Nota (ADR-0002 D1, casing — resolvido; era ADR-0001 T3 "a definir antes da Fatia 3"): wire camelCase em `/api/*` (`userId`, `sizeBytes`, `lastModified`, settings `hfToken`…); colunas SQL snake_case; valores de enum, `Error.code` e artefatos de transporte (`manifest.json`, `config.yaml`, SQLite do orquestrador) snake_case.
- Regra: contadores do dataset recalculados por função única `heph_refresh_dataset_counters(uuid)` — IMPLEMENTADO (Fatia 3b; `migrations/0003_images.sql`, fecha ADR-0002 T2): recalcula `images_count`/`labeled_count`/`size_bytes` e deriva `status` (`needs_labeling`/`in_progress`/`ready`) a partir das tabelas-fato, nunca `+=` (drift impossível; `UPDATE` com guarda `IS DISTINCT FROM` evita churn de `updated_at`); disparada por triggers `AFTER INSERT OR UPDATE OR DELETE` em `images`, `videos` e `boxes`/`captions` (via lookup de `dataset_id`); a ordem trigger-usuário × cascata-RJ do `DELETE FROM images` deixa de importar (o último disparo vê o estado final); `labeled` = imagem com ≥1 box (format `yolo_txt`) **ou** linha em `captions` (demais formats) — taxonomia R9; `size_bytes` soma `images` + `videos`. Chaves externas com `ON DELETE CASCADE` de dataset→filhos. O invariante `labeled_count <= images_count` continua sem `CHECK` (não deferrável) — é obrigação do trigger.
- **Split:** coluna `images.split (train|val)`; padrão 80/20 estratificado no package com override manual na galeria (seletor train/val por imagem).
- **Consistência bucket×banco (ADR-0003 D1/D6/D7):** Postgres é a verdade relacional, o bucket é a verdade binária (linhas de `images` como índice). `PUT boxes/caption` atualiza SÓ o banco — a materialização do `.txt` com debounce (~2s) MORREU (não há mais o que materializar; `data.yaml`/`labels/*.txt`/`captions.jsonl`/`.parquet` nascem no tempdir do orquestrador a partir do Postgres). Ordem de escrita objeto→linha→compensação: PUT do objeto → `INSERT ... ON CONFLICT (dataset_id, filename) DO NOTHING` (sem linha ⇒ `duplicate` + DELETE best-effort do objeto); `DELETE /api/datasets/:id` commita o banco primeiro (CASCADE) e depois varre o prefixo `datasets/{id}/` (`ListObjectsV2` paginado + `DeleteObjects` de 1000) best-effort — falha do sweep loga (`eprintln`) e não transforma o 204 em erro. O package sempre gera do banco.
- **Snapshot:** ao despachar job, congela `dataset_versions{manifest}` e o zip aponta para a versão; edição posterior não afeta treino em voo (trava lógica por versão, não por dataset).

## 11. Schemas, retenção e setup inicial

- `manifest.json` (transporte — PROJETO para 3e/4, ainda não implementado; ADR-0003 D9): `{dataset_id, slug, category, engine, files:[{key,filename,md5,bytes}], md5_zip, bytes, chunks, created_at}` (`files[].path` → `{key,filename,md5,bytes}`; snake_case, orquestrador recebe o manifest com `key` quando ganhar cliente S3).
- `config.yaml` por job: comum `{job_id, engine, model, mode, dataset_path, output_path, seed}` + específico:
  - yolo: `{model, epochs, batch, imgsz, lr0, optimizer, augment:{mosaic, mixup_flip}}`;
  - difusao: `{base_model, trigger_word, rank, alpha, optimizer, steps, lr, cfg}`;
  - clip: `{backbone, embed_dim, loss, lr, warmup, batch, epochs}`.
- `engines.yaml`: `{engine, image, cuda, torch, validated_at}` — ex. `trainer-difusao: hephaestus/trainer-difusao:local`.
- Retenção: samples últimos 5 ciclos ou 500 MB/job; logs 10 MB + 30 dias; artifacts guarda `best + last`, resto com GC manual (`DELETE /api/jobs/:id/artifacts?keep=best,last`).
- Setup: `STUDIO_PASSWORD` no primeiro boot (hash Argon2 em `users`); troca via CLI `studio reset-password` (sem expor rota); chaves cifradas app-level com `STUDIO_MASTER_KEY` (nunca em log); `infra/compose.yaml` sobe `db (postgres:16) + principal (:8080) + manager + orquestrador-local (socket docker) + web (next) + seaweedfs (:8333 S3, :9333 master UI)` com volumes `pgdata, seaweed_data, datasets, models, outputs`. Serviço novo `seaweedfs` (`chrislusf/seaweedfs:4.45_full` pinado, `server -s3 -ip.bind=0.0.0.0 -s3.port=8333 -s3.config=/etc/seaweedfs/s3.json`, identidade em `infra/seaweedfs-s3.json`, bind loopback `127.0.0.1:8333:8333` + `127.0.0.1:9333:9333`, healthcheck por `wget` com `403 = no ar`); principal com `STORAGE_BACKEND=s3`, `S3_ENDPOINT_URL=http://seaweedfs:8333`, `S3_BUCKET=heph-data`, `S3_PUBLIC_ENDPOINT_URL=http://localhost:8333`, `S3_URL_TTL_SECS=3600`. O volume `datasets` do orquestrador-local SOBREVIVE (para `models`/`outputs`/cache de build); o volume `datasets` do principal morreu (blobs no bucket).
- **Execução sem DinD (RunPod padrão):** orquestrador opera em 2 modos — `docker` (socket disponível) ou `subprocess` (venv/python direto no mesmo host). Pods sem socket usam modo subprocess; template com DinD é opcional, não requisito.
