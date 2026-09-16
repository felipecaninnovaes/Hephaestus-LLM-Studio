# Hephaestus LLM Studio — Esboço do Backend

> Origem: `IDEIA.md` + `docs/frontend.md` + decisões da revisão (auth single-user, SQLite só transporte, playground multi-motor, preempção de runner por VRAM, Postgres no principal, manager local).
> Componentes: **backend principal (Rust + Postgres)** → **manager local (Rust)** → **orquestrador(es) (Rust)** → **motores (Python)**. Tudo em Docker, execução local ou VPS/RunPod.

## 0. Escopo e base (resposta à revisão)

- **Nome vs. escopo:** `Hephaestus-LLM-Studio` é legado; o escopo implementado é **visão computacional/multimodal** (YOLO, Difusão LoRA, OpenCLIP). LLM textual (SFT/DPO/vLLM) é futuro, não entra no MVP.
- **Base presente no repo:** `IDEIA.md`, `arquitetura_studio_modular.png`. Conteúdo da IDEIA incorporado aqui + `frontend.md`.

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
- **Embedder local (exceção consciente à topologia até a fatia 4 — ADR-0004 D1, Fatia 3f):** serviço `embedder` no compose (build `engines/trainer-clip`, modo `serve`) que o principal chama via HTTP (`EMBEDDER_URL`) para embeddings texto/imagem da busca semântica; inferência leve (mock por default, CLIP real só `@gpu` manual fora do compose). Unificação runner na fatia 4. O principal continua sem GPU.
- Manager mora no ambiente local, **dono da fila central e da política VRAM**; sincroniza ambientes e despacha jobs conforme capacidade reportada.
- Orquestrador (local ou remoto) é **stateless e executor**: valida md5, faz build do dataset, sobe trainers/runners, reporta VRAM/logs e devolve artefatos. Sem Postgres próprio, só workspace efêmero + volumes de cache.
- Python só treina/infere e devolve artefatos + métricas + samples.
- **Principal↔Manager (interno):** HTTP em rede docker (`manager:8081`), auth por segredo pré-compartilhado (`MANAGER_TOKEN`, `Authorization: Bearer`); front nunca fala com o manager direto.
- **Postgres compartilhado:** um único Postgres do stack local; principal é dono de `datasets/images/auth/settings`, manager é dono de `jobs/runners/orchestrators/queue`. Sem transações cruzadas via API — cada um escreve só nas suas tabelas.
- **Resiliência:** fila e reservas de VRAM são **reconstruídas do Postgres** no boot do manager (`jobs` em `queued/dispatched/preparing` voltam a `queued` com `queue_reason=recovered`); nada crítico só em memória.
- **Direção de rede (NAT): outbound-first.** Modelo primário é **reverso**: orquestrador remoto abre WS persistente com o manager (`/orch/channel`, Bearer `heph_o_*` + pin) e recebe despachos por ele; artefatos voltam via `POST` do orquestrador para o manager/principal. Conexão inbound direta (manager→pod) é opcional, só quando há IP:porta alcançável. **Emenda G.7 (ADR-0010 D1):** a sessão GPU (TrueNAS) é exatamente este caso — manager despacha para `http://10.15.1.2:8082` (LAN, branch inbound direta); orquestrador remoto envia report/heartbeat para `http://10.15.10.3:8081` (dev host).
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
- **Build no orquestrador:** ele pode materializar um `build.sqlite` **interno/temporário** ou montar direto via `JSON/YAML` — o que for melhor por engine — e então **reconstrói a árvore exata do motor** (`labels/*.txt`, `data.yaml` com paths remapeados). Python nunca lê SQLite do transporte; SQLite é detalhe interno e descartável do orquestrador. **Emenda G.7 (ADR-0010):** o builder/export Rust continua emitindo `train: [images/...]` no `dataset.yaml` (contrato 3e intocado). É o caminho REAL do trainer que adapta dentro do container: `_prepare_dataset_yaml` reescreve `path:` para absoluto, converte listas para `.txt` com caminhos absolutos (ultralytics 8.3.253 abre listas diretas como UTF-8 e explode com `UnicodeDecodeError`), e val vazio aponta para `train.txt`.

## 4. Jobs, trainers sob demanda e cache

- Tipos: `yolo_train | difusao_train | clip_train | autolabel | autotracker | download_model | playground | diffusion_generate`.
- **Emenda Fatia J (ADR-0013):** a inferência YOLO real é implementada como **job na fila** (`kind='yolo_predict'`, `engine='yolo'`, `mode='predict'`), NÃO como o runner quente do §5. O módulo "Playground" da Sidebar ficou habilitado (badge Roadmap removido); `playground`/runners quentes continuam dívida (ADR-0013 D0/D7).
- Ciclo: `queued → dispatched → preparing(env+dataset) → running → paused? → done|failed|cancelled`, com `POST /api/jobs/:id/{pause,abort}` + `POST /api/jobs/:id/resume`. **Fila central no manager** (posição + motivo `waiting_vram|waiting_slot` visíveis no front); orquestrador só executa o que recebe e reporta `vram_used/total` + heartbeat.
- **Pause = checkpoint + libera VRAM** (não `docker pause`): `pause` pede `save_checkpoint`, derruba o trainer e mantém `last.ckpt`; `resume` recria do checkpoint. Sem checkpoint do engine, pause é recusado (`409 checkpoint_unsupported`) e só `abort` vale.
- Orquestrador sobe **um container `trainer-<engine>-<jobid>` por job** a partir de imagens por engine (isola deps: ultralytics vs. diffusers/kohya vs. open_clip). **Exceção:** modo daemon de difusão (`DIFFUSION_DAEMON_ENABLED=1`) sobe 1 container `trainer-difusao-daemon` persistente por nó em vez de 1 por job; o daemon processa jobs `generate` sequencialmente via lock local (ADR-0023 D1). Ao destruir o container, **cache persiste fora**: volumes `models/`, `datasets-cache/`, `outputs/` mapeados no host/remoto.
- **Imagens (decisão): uma por engine** (yolo, difusão, clip, autolabel/tracker, runner), **base no estável mais recente testado** (não pinar no 12.4/2.4.1 do protótipo; registrar a versão validada em `engines.yaml`), **build local no compose** (sem registry externo por enquanto; tags `hephaestus/trainer-<engine>:local`).
  - **Emenda G.7 (ADR-0010 D5):** para treino real @gpu, existe **`hephaestus/trainer-yolo:gpu`** — imagem separada (`engines/trainer-yolo/Dockerfile.gpu`) com base PyTorch 2.6.0+CUDA 12.4+cuDNN 9, ultralytics==8.3.253 pinado, `ENV ENGINE_MOCK=0` baked, e peso base `yolo11n.pt` baixado no build (~5MB). Construída no TrueNAS via `docker compose -p gpu --profile build build trainer-gpu`. A imagem `:local` continua mock stdlib pura e não é afetada.
  - **Emenda — AutoTracker v1 (ADR-0008 D2/D6):** o autotracker v1 (mock) reutiliza a imagem `trainer-yolo:local` via subcomando `autotrack` (`python -m trainer_yolo autotrack --config <config.yaml> --output <output_path>`), sem modelo real. Runner/imagem próprios (florence-2/qwen-vl, ADR-0008 D6) são fatia futura.
  - **Fatia 9 — AutoTracker real (ADR-0014 D0/D3):** o autotracker real usa o **mesmo** subcomando `autotrack` com modelo open-set `yolov8x-worldv2.pt` do ultralytics (~1.3 GB); `model.set_classes(classes do dataset)` mapeia 1:1 (prompts = nomes das classes); `boxes.json` MESMO shape do mock (`seed: 0` sentinela, sem `metrics.jsonl`). Não ganhou runner/imagem próprios — usa a mesma `trainer-yolo:gpu`. florence-2/qwen continuam alternativas futuras (imagem própria).
- Paralelismo por VRAM, não fixo: com folga (ex. RunPod 80 GB) roda 2+ trainers e enfileira o resto (FIFO + cancel manual). Fila visível no front com posição e motivo (`waiting_vram`).
- Entrada do trainer: zip do §3 + `config.yaml` gerado pelo principal (hiperparams do front) + mounts de modelos solicitados.
- **trainer-clip ganhou modo `serve` (Fatia 3f; `engines/trainer-clip/src/trainer_clip/serve.py`, extras `[serve]` no pyproject: `open_clip_torch/torch/pillow`):** mock via `ENGINE_MOCK=1` (vetores hash determinísticos, stdlib puro, sem torch — default do compose); modo real = OpenCLIP `ViT-B-32` (`laion2b_s34b_b79k`, lazy, GPU se disponível) só `@gpu` manual fora do compose. (Nota: não existe seção de engines neste arquivo — o modo serve vive aqui no §4.)

## 5. Playground / runners (decisão: todos os motores, preemptível)

- Um `runner-<engine>` por motor ativo (difusão gera imagem, YOLO infere, CLIP busca), mesmo design das demais abas.
- Lifecycle: sobe no primeiro uso → fica warm → desliga por (a) botão "Matar runner", (b) idle timeout configurável, (c) **preempção quando treino precisa de VRAM mínima e não há folga**. Front mostra `runner ativo · VRAM X GB · [Matar]` + toast quando preemptado.
- **TTL (decisão): padrão 15 min** (`difusao: 10 min`, `yolo: 30 min`, `clip: 20 min`, sobrescrevível por ambiente). Contagem só com fila vazia e sem inferência ativa; front mostra countdown + aviso 2 min antes; qualquer uso reseta. Com pressão de VRAM o idle é morto na hora sem esperar o TTL.
- Nunca dividir container trainer/runner: evita contaminação de deps e permite matar inferência sem tocar no treino.
- **Emenda ADR-0023 (D1 — daemon quente de difusão):** o orquestrador mantém 1 daemon HTTP de inferência por nó para jobs `generate` (subcomando `serve` do trainer-difusao). O daemon sobe no 1º job `generate` despachado para o nó, mantém 1 pipeline carregado, e é matado por idle TTL (`DIFFUSION_DAEMON_IDLE_TTL_S`, default 600s) ou preempção (treino precisa de VRAM → mata daemon idle primeiro). `DIFFUSION_DAEMON_ENABLED=0` (default no compose mock) desabilita o daemon e cai no caminho one-shot existente (`docker run --rm`). **Parcialmente quitada:** a dívida dos runners quentes é quitada para difusão (daemon interno do orquestrador); a API pública `/runners/*` permanece dívida (§9 runners).

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
- Medição: `nvidia-smi` 2s no orquestrador + `torch.cuda.max_memory_allocated` reportado pelo motor no fim do warmup; manager aplica `medido * 1.25` e sugere atualizar o yaml (`POST /api/settings/vram-table/propose`). **Emenda H.7 (ADR-0011 D3):** roteamento estático entra — `vram-table.yaml` é carregada pelo manager no boot (`VRAM_TABLE_PATH`, default compilado via `include_str!`); para cada job `queued`, `required_gb = entries[engine][model][mode].vram_min_gb + headroom_gb`; entrada faltante ⇒ requisito `NULL` (permissivo); `queue_reason='waiting_vram'` quando há nó online mas nenhum com capacidade. Nenhum nó elegível com requisito presente → `waiting_vram`; sem requisito e sem nó online → `waiting_slot`.
- **Roteamento com seleção de nó e fallback honesto — Fatia N (ADR-0015):**
  - O manager (`POST /internal/jobs`) e os 3 submits públicos do `api-principal` (`POST /api/jobs/yolo`, `/autotracker`, `/predict`) aceitam `orchestrator_hint` / `orchestratorId` (UUID opcional). Ausente ou null = roteamento automático por capacidade.
  - Validação fail-fast no submit: string não-UUID ⇒ 400 `invalid_request`; inexistente ⇒ 404 `not_found`; nó não-online ⇒ 400 `invalid_request`.
  - Despacho em 1º nível: o escalonador tenta alocar no nó indicado. Se ocupado por > 120s (`ORCHESTRATOR_HINT_TIMEOUT_SECS`), ocorre fallback automático para o nó com maior capacidade disponível, marcando honestamente `params.orchestrator_fallback = true` (se honrado, remove a flag).
  - Wire de jobs (`list_jobs` e `get_job`): enriquece a resposta com `orchestratorName`, `orchestratorKind` e `orchestratorFallback` via `LEFT JOIN orchestrators`.
- Regra: se `livre >= min` → sobe em paralelo; senão treino entra em `queued(waiting_vram)` e, se o bloqueio for um runner, o runner é drenado/morto primeiro. **Treino nunca é morto por falta de VRAM, só enfileirado.**
- **Policy VRAM aplicada completa (fila por VRAM livre dinâmica, paralelismo 2+ jobs por nó, preempção de runner, `max_parallel_trainers`) permanece DÍVIDA** — o que entra é o roteamento estático (capacidade declarada vs requisito).**
- Config por ambiente (via manager): `max_parallel_trainers` (teto, padrão 2; efetivo = `min(teto, floor((vram_total - headroom) / vram_min_do_job))`), `vram_headroom_gb`, `runner_idle_ttl_s`, `vram-table` por modelo.
- **Preempção (decisão):** runner idle → mata direto + toast com motivo; runner com inferência ativa → modal "Treino X precisa de N GB. Matar runner?" com countdown 30s (expirar = mata). Log de auditoria `runner_preempted {by_job}`. **Sem usuário (expirado/madrugada):** mata ao expirar, a inferência em voo retorna `409 runner_preempted {job_id}` e o playground mostra toast + estado vazio (sem corromper resposta parcial).
- **Multi-GPU (MVP): 1 job = 1 GPU** (`CUDA_VISIBLE_DEVICES=gpu_index` escolhido pelo orquestrador; `gpus:[{index, vram_total, vram_used}]`, sem DDP). DDP/Accelerate multi-GPU para um único treino fica fora do MVP.

## 7. Samples por ciclo (health visual do treino)

- Motores salvam a cada ciclo (época YOLO/CLIP, N steps difusão): `samples/cycle_{n}/{img, meta.json}` com métricas do ciclo.
- Orquestrador coleta e o principal serve: `GET /api/jobs/:id/samples?cycle=N&limit=8` + `GET /api/jobs/:id/metrics` (loss, mAP, recall, step/epoch). Front renderiza a grade "Samples do ciclo N" ao lado das curvas.

## 8. Downloads HF/Civitai + bootstrap e adoção de orquestradores

- `POST /api/models/download {url, engine, name?}` → **implementado (Fatia I)**: server-side no **principal** (reqwest stream → PUT S3 `models/<engine>/<id>/<name>`), síncrono, allow-list fail-closed via `MODEL_DOWNLOAD_ALLOWED_HOSTS` (env ausente/vazia = 403 `model_download_disabled`; feature nasce desligada); deny de ranges privados/metadata; redirects re-validados a cada hop (máx 5); cap 2 GiB, timeouts 30s/120s; falha → 502 `model_download_failed`. **Divergência consciente** da visão §9/:104 original (manager→orquestrador+volume+WS): o download v1 é síncrono no principal sem WS, sem volume novo; a visão original é arquitetura futura (ADR-0012 D4).
- Bootstrap remoto ao subir container: orquestrador mapeia `datasets-cache/<jobid>/` (build do §3), `models/<engine>/` solicitados e `outputs/<jobid>/`; na conclusão faz o caminho inverso (`.safetensors`, pesos YOLO/CLIP, imagens processadas, logs, samples) de volta ao principal com md5.
- **Adoção (decisão: token colado):** orquestrador remoto (VPS/RunPod com IP público) ao subir **gera pairing token + fingerprint**; o usuário cola no front (`Conectar Pod`) e o manager adota (`POST /api/orchestrators/adopt {endpoint, key}`), passa a health-checkar e sincronizar regras. **Local:** o compose sobe `principal + manager + orquestrador-local` juntos e o manager **auto-adota via rede docker**, sem chave.
  - **Implementado v1 (Fatia H, ADR-0011 D5):** pairing code `heph_p_*` (formato encorajado, não enforceado) verificado no orquestrador via `POST /internal/pairing/verify` (single-use em memória); upsert no manager via `POST /internal/adopt`; `POST /api/orchestrators/adopt` (BFF) e `POST /api/orchestrators/:id/revoke` (tombstone `revoked`, não DELETE). Alias `/api/environments*` implementado (módulo da UI habilitado). `AUTO_ADOPT_LOCAL` permanece (default `1`) com guarda "não ressuscita `revoked`".
  - **Pendente (divida):** `heph_o_*`/TLS-pin/rotação (colunas `token_hash`/`fingerprint` existem e ficam NULL); sem rate-limit de 5 tentativas; health-check 15s/2-5 falhas do §8/:111 substituído pelo watchdog sobre heartbeat (15s/60s).
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
health:   GET /health → {status, service, auth: ready|setup_required, version}  → implementado (campo `auth` novo ADR-0001 D3; campo `version` novo ADR-0009 D5; spec 0.9.0)
          GET /ready → {status: ok|unavailable, reason?}  → implementado (Fatia 4; D11 ADR-0007; liveness + readiness)
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
            GET /api/datasets/:id/search?q&k&classId&split, POST /api/datasets/:id/search/by-image,
            GET /api/datasets/:id/search/status, POST /api/datasets/:id/search/index
               → implementado (Fatia 3f; ADR-0004/openapi 0.5.0)
            POST /api/datasets/:id/export, POST /api/datasets/import
               → implementado (Fatia 3e; ADR-0006/openapi 0.6.0 — ver Nota Fatia 3e abaixo)
            POST /api/datasets/:id/package
               → implementado (Fatia 4; ADR-0007 D1 — congela `dataset_versions`, gera zip, PUT `packages/<version_id>/`)
models:   GET /api/models (pesos da tabela canônica `models`, ordered by created_at DESC)  → implementado (Fatia I; ADR-0012 D2 — fonte trocada de derived para tabela)
          POST /api/models/upload  → implementado (Fatia I; ADR-0012 D3 — multipart file+engine+name?, magic PK\x03\x04 para .pt ou header JSON para .safetensors, engines yolo/world/diffusion/clip, teto 2 GiB, md5)
          POST /api/models/download  → implementado (Fatia I; ADR-0012 D4/E1 — server-side no principal, allow-list fail-closed MODEL_DOWNLOAD_ALLOWED_HOSTS, engines yolo/world/diffusion/clip, extensões .pt e .safetensors, 502 model_download_failed)
          DELETE /api/models/:id  → implementado (Fatia Gestão de Modelos; 204 No Content, 401, 404; remove S3 se upload/download e desvincula no manager)
          PATCH /api/models/:id  → implementado (ADR-0022 D2; 200 OK com Model, 400, 401, 404; renomeia modelo no catálogo)
          Nota Fatia 9: upload/download aceitam `engine='world'` (migration 0008 — ADR-0014 D1; validação `.pt`+magic PK idêntica ao yolo)
preview:  POST /api/preview/{autolabel,autotracker,generate,search} (job efêmero ou runner quente, sem fila de treino)
jobs:     POST /api/jobs/yolo  → implementado (Fatia 4; ADR-0007 D7 — spec 0.7.0)
           POST /api/jobs/autotracker  → implementado (Fatia 5/9; ADR-0008 D0/D3, ADR-0014 — spec 0.13.0)
             body `{datasetId, model?, conf?, modelId?}` → 202 `{jobId,status:"queued",queuePosition?}`
             `modelId?`: presente → job REAL (pesos world); ausente → mock (comportamento atual)
             erros: 400 `invalid_request` (model∉{mock} | conf fora 0..1 | modelId não-UUID |
             row engine≠world — via manager), 404 `not_found` (dataset não-UUID/inexistente |
             modelId inexistente — via manager), 409 `dataset_not_ready` (category≠yolo, 0 classes, 0 imagens),
             503 `queue_unavailable`; compensação do package em TODOS os erros
          POST /api/jobs/predict  → implementado (Fatia J; ADR-0013 D8 — spec 0.12.0)
            body `{modelId, datasetId, conf?}` (conf default 0.65) → 202 `{jobId,status:"queued",queuePosition?}`
            erros: 400 `invalid_request` (modelId não-UUID, conf fora 0..1, body malformado, engine≠yolo da row de models — via manager),
            404 `not_found` (datasetId não-UUID/inexistente, modelId inexistente — via manager),
            409 `dataset_not_ready` (category≠yolo, 0 imagens ativas), 503 `queue_unavailable` (manager fora — compensação do package)
            Nota R6: handler mapeia `NotFound→404`, `InvalidRequest→400` (diferente do submit_yolo_job que mapeia Err(_)→503)
          POST /api/jobs/:id/autotracker/apply  → implementado (Fatia 5; ADR-0008 D1/D1a — spec 0.8.0)
            body `{overwrite?, imageId?}` → 200 `{applied, skipped, images}`
            erros: 400 `invalid_request` (imageId não-UUID), 404 `not_found`, 409 `job_not_done`,
            503 `queue_unavailable`/`storage_unavailable`
          POST /api/jobs/autolabel  → implementado (Fatia AutoLabel v1; ADR-0016 D0 — spec 0.15.0)
            body `{datasetId, model?, prompt?, orchestratorId?}` (model default "mock") → 202 `{jobId,status:"queued",queuePosition?}`
            erros: 400 `invalid_request` (datasetId não-UUID, model≠"mock", orchestratorId não-UUID), 404 `not_found` (dataset inexistente), 409 `dataset_not_ready` (0 imagens ativas), 503 `queue_unavailable` (manager fora — compensação do package)
          POST /api/jobs/:id/autolabel/apply  → implementado (Fatia AutoLabel v1; ADR-0016 D1 — spec 0.15.0)
            body `{datasetId?, overwrite?}` → 200 `{applied, skipped, images}`
            erros: 400 `invalid_request` (datasetId divergente ou malformado), 404 `not_found` (job inexistente | engine≠autolabel | captions.jsonl ausente), 409 `job_not_done` (status≠done), 503 `queue_unavailable`/`storage_unavailable`
          POST /api/jobs/diffusion  → implementado (Fatia Difusão LoRA; ADR-0018 — spec 0.17.0)
            body `{datasetId, baseModel, triggerWord?, epochs?, batchSize?, learningRate?, rank?, alpha?, weights?, orchestratorId?}` → 202 `{jobId,status:"queued",queuePosition?}`
            erros: 400 `invalid_request` (baseModel∉{sdxl,flux,sd15}, hiperparâmetros fora de domínio), 404 `not_found`, 409 `dataset_not_ready` (0 imagens), 503 `queue_unavailable` (manager fora — compensação do package)
          GET /api/jobs         → implementado (Fatia 4; lista `{items,total}`)
          GET /api/jobs/queue   → implementado (Fatia 4; fila `{items:[{jobId,position,queueReason}]}`)
          GET /api/jobs/:id     → implementado (Fatia 4; detalhe do job)
          POST /api/jobs/:id/abort  → implementado (Fatia 4; 200 `{"status":"cancelling"|"cancelled"}` | 409 `job_not_abortable`)
           DELETE /api/jobs/:id  → implementado (AC-003; 200 `JobDeletedResponse` | 401 | 404 `not_found` | 409 `job_not_terminal` | 503 `queue_unavailable`)
           POST /api/jobs/cleanup  → implementado (AC-003; 200 `JobCleanupResponse` | 400 `invalid_request` | 401 | 503 `queue_unavailable`)
           GET /api/jobs/:id/metrics  → implementado (Fatia 4; `{items:[{epoch,boxLoss,clsLoss,dflLoss,map50,map5095}]}`) — nota AC-006-A: só pontos de treino
           GET /api/jobs/:id/artifacts  → implementado (Fatia 4; `{items:[{id,kind,path,md5,bytes}]}`)
          GET /api/jobs/:id/artifacts/:artifactId/data  → implementado (Fatia 4; proxy do objeto via StoragePort)
          # Adiados para fatias futuras: pause/resume, samples, WS, runners, clip, autolabel v2 (modelos reais/VLM)
runners:  POST /api/runners/{difusao,yolo,clip}/up, POST /api/runners/:id/kill, GET /api/runners
          POST /api/runners/:id/infer {prompt|image|query} (inferência interativa; 409 se preemptado)
orchestrators (via manager): GET /api/orchestrators  → implementado (F6.1 + Fatia H; leitura da tabela do manager com telemetria por nó; ADR-0009 D1 + ADR-0011 D2)
          POST /api/orchestrators/adopt  → implementado (Fatia H; ADR-0011 D5 — 200 upsert / 400 / 409 `pairing_invalid` / 503; spec 0.10.0)
          POST /api/orchestrators/:id/revoke  → implementado (Fatia H; ADR-0011 D5 — 204 tombstone `revoked` / 404 / 503)
          POST /api/orchestrators/:id/{enable,disable}  → pendente (futuro)
          GET /api/orchestrators/:id/health  → pendente (watchdog dá o status; health-check ativo redundante na v1)
          alias UI: /api/environments* responde o mesmo que /api/orchestrators* (front usa "Ambientes")  → implementado (Fatia H; ADR-0011 D5/D7)
          → manager auto-adota `orchestrator-local` no boot (Fatia 4; ADR-0007 D3); auto-adoção não ressuscita `revoked` (Fatia H; ADR-0011 D5.7)
telemetry: GET /api/telemetry  → implementado (Fatia 4 + Fatia H; proxy do cache do manager: {measured,cpu,ram,ramTotal,vramUsed,vramTotal,gpus,jobsActive}; ramTotal = ADR-0009 D4, bytes)
           ramTotal: aditivo Option<i64> (bytes; ADR-0009 D4; spec 0.9.0)
           **Emenda H.7 (ADR-0011):** agregação definida — 0 nós → fallback atual (`measured:false`); 1 nó → idêntico ao de hoje; >1 nós → `vramUsed/vramTotal` = soma dos Some, `gpus` = união (ordem por nó), `jobsActive` = soma, `cpu/ram/ramTotal` = **`null`** (não agregáveis de forma honesta), `measured:true` se ≥1 nó fresco. `measured` por nó = heartbeat ≤ 10s. `gpus[]`/`vramUsed`/`vramTotal` carregam valores reais quando um orquestrador com GPU heartbeats (contrato inalterado).
monitoring: GET /api/orchestrators  → implementado (F6.1 + Fatia H; leitura via manager com telemetria por nó; status 200/401/503; ADR-0009 D1 + ADR-0011 D2)
            GET /api/models         → implementado (Fatia I; tabela canônica `models`, shape `Model` com source/md5/url/model nullable; ADR-0012 D2/D6 — fonte trocada de derived para tabela)
            GET /api/storage/usage  → implementado (Fatia I; soma SQL: datasetsBytes + artifactsBytes (exclui kind='model') + modelsBytes; ADR-0009 D3 + ADR-0012 D8)
ws:       /ws/jobs/:id/logs?since_seq=, /ws/telemetry
```

- Nota Fatia 2 (D9): `/api/auth/me` valida o próprio cookie (isento do gate por prefixo, §2); `route_layer` plugado na Fatia 3a (ADR-0002 D9) — fallback fail-closed (sem sessão → 401 mesmo em rota inexistente; com sessão → 404 sem body, fora da OpenAPI).
- Nota Fatia 4 (ADR-0007 D7/D9 — spec 0.7.0, `packages/contracts/openapi.yaml`):
  - **`queue_unavailable` (503) em TODAS as rotas de jobs + telemetry:** o `handlers.rs` do principal mapeia `ManagerError::Unavailable` (e qualquer outro erro do manager client) para 503 `queue_unavailable` em **todas** as 9 rotas que falam com o manager (`list_jobs`, `list_queue`, `get_job`, `get_job_metrics`, `list_artifacts`, `get_artifact_data`, `submit_yolo_job`, `abort_job`, `get_telemetry`). A ADR-0007 D7 previa `queue_unavailable` só no `POST /api/jobs/yolo` e na rota `data` — o código generalizou (decisão do coordenador na F4.2a: se o manager está inalcançável, qualquer leitura de estado de jobs não tem fonte fiável).
  - **`GET /api/jobs` SEMPRE `{items,total}`:** o shape é camelCase `JobList` (não há "dual-shape"); a fila (`GET /api/jobs/queue`) é um payload separado `{items:[{jobId,position,queueReason}]}` derivado do mesmo `queue_position`/`queue_reason` do banco.
  - **`GET /api/jobs/:id` devolve o payload interno do manager (snake_case verbatim):** o principal é BFF puro — ecoa o JSON do manager sem mapear `queue_reason`/`queue_position` (campos snake_case no wire). Cuidado: o front precisa ler snake_case nests se consumir esses campos. O `metrics` JSONB do banco é snake_case (`mAP50-95`) e o principal re-mapeia `map5095` no response de `/metrics`.
  - **Artefatos:** `{id,kind,path,md5,bytes}` — `path` é relativo ao prefixo `artifacts/<job_id>/`.
  - **`POST /api/jobs/:id/abort`:** 200 `{"status":"cancelling"}` (job em `preparing`/`running`) ou `{"status":"cancelled"}` (job em `queued`/`dispatched`); 409 `job_not_abortable` em estado terminal (`done`/`failed`/`cancelled`). Código: `manager::abort_job` (`lib.rs:532-585`) consulta status antes de escrever.
  - **Telemetria:** `{measured:bool, cpu:float|null, ram:i64|null, ramTotal:i64|null, vramUsed:i64|null, vramTotal:i64|null, gpus:string[], jobsActive:i32}`. `ramTotal` = bytes (lido de MemTotal do /proc/meminfo; ADR-0009 D4; aditivo, Option). CPU/RAM reais (leitura `/proc` do container orquestrador via heartbeat ~2s). `measured:true` = heartbeat recebido nos últimos 10s (get_telemetry L832-843). Com GPU ausente: `gpus:[]`, `vramUsed/vramTotal:null`, mas CPU/RAM **reais** — `measured:true` quando o heartbeat é fresco. O texto "sem GPU (mock)" é renderizado pelo front quando `vramTotal==null || gpus.length===0`. **Nota R5:** a semântica real (código) é: `measured:true` = heartbeat ≤ 10s; GPU ausente = `gpus:[]`/`vram_*:null` com CPU/RAM reais. `measured:false` só ocorre quando o heartbeat está ausente (>10s) — nesse caso CPU/RAM também são null. O mock SEMPRE reporta heartbeat (~2s), logo `measured:true` com `gpus:[]` (não `measured:false`).
  - **Erros novos na v1:** `queue_unavailable` (503, todas as rotas jobs/telemetry), `dataset_not_ready` (409, `POST /api/jobs/yolo` quando category≠yolo ou 0 classes/imagens), `job_not_abortable` (409, `POST /:id/abort` em estado terminal), `job_not_terminal` (409, `DELETE /api/jobs/:id` em estado não-terminal — AC-003), `engine_unsupported` (400, `POST /:id/package` quando engine≠yolo).
  - **`POST /api/datasets/:id/package`:** body `{engine:"yolo"}` → 200 `PackageResponse{versionId,key,bytes,md5Zip,files}`; 400 `engine_unsupported` (engine≠yolo na v1) | 404 dataset | 503 storage/queue. Congela `dataset_versions{manifest}` (snapshot JSONB, T4 ADR-0002). `config.yaml` gerado pelo principal com placeholders `{dataset_path}`/`{output_path}` substituídos pelo orquestrador no spawn do container. Trainer via `docker run` com volumes nomeados (`datasets-cache/<jobid>/`, `models/`, `outputs/`).
- Nota Fatia 5 (ADR-0008, spec 0.8.0, `packages/contracts/openapi.yaml`):
  - **`POST /api/jobs/autotracker`** — body `{datasetId, model?, conf?}` → 202 `SubmitJobResponse{jobId,status:"queued",queuePosition?}`. Validação: `model` ∈ `{mock}` apenas (default `mock`), `conf` em `0..=1` (default `0.65`). Erros: 400 `invalid_request` (model∉{mock} | conf fora de domínio), 404 `not_found` (dataset não-UUID/inexistente), 409 `dataset_not_ready` (category≠yolo, 0 classes, 0 imagens ativas), 503 `queue_unavailable`. Job: `kind='autotracker'`, `engine='autotracker'`, `mode='autotrack'` (TEXT livre, sem migration).
  - **`POST /api/jobs/:id/autotracker/apply`** — body `{overwrite?: bool, imageId?: string}` → 200 `AutotrackerApplyResponse{applied, skipped, images}` (`applied` = boxes gravadas, `skipped` = boxes ignoradas (classe/imagem inexistente ou cap), `images` = imagens que receberam ≥1 box). Validação `imageId`: não-UUID ⇒ 400 `invalid_request`; UUID fora do dataset ⇒ 404 `not_found`. Fluxo: job via manager (engine='autotracker', status='done', dataset_id presente) → artefato `boxes.json` via `list_artifacts` (kind='boxes') + path/md5 → `StoragePort.get` → parse → resolve filename→image_id + class name→class_id → transação por imagem DELETE+INSERT. Merge por origem (D1a): `overwrite=false` → DELETE só `origin='autotracker'` (preserva manual/import); `overwrite=true` → DELETE total. Imagem presente no artefato com boxes emitidas (mesmo todas skippadas) → DELETE executado (last-write-wins por origem, código `handlers.rs:1049-1053`). Erros: 400 `invalid_request`, 404 `not_found`, 409 `job_not_done` (job não está `done`), 409 `dataset_not_ready` (dataset_id null/deletado), 503 `queue_unavailable`/`storage_unavailable`.
  - **`queue_unavailable` estendido:** as rotas novas também retornam 503 quando o manager está inalcançável (mesmo mapeamento do Fatia 4).
  - **`boxes.json` (snake_case, transporte):** `{engine, model, seed, conf, images:[{filename, boxes:[{class, x, y, w, h, conf}]}]}`. Keyado por filename (engine não conhece image UUID) e class name (robusto a reordenação).
- Nota Fatia AutoLabel v1/v2 (ADR-0016 / ADR-0019, spec 0.18.0, `packages/contracts/openapi.yaml`):
  - **`POST /api/jobs/autolabel`** — body `{datasetId, model?, prompt?, apiKey?, apiBase?, openaiModel?, orchestratorId?}` (model default "mock", aceita `mock`, `florence-2`, `qwen2-vl`, `openai`) → 202 `SubmitJobResponse{jobId,status:"queued",queuePosition?}`. Validação: `model` ∈ `{mock, florence-2, qwen2-vl, openai}`, `apiBase` iniciando com `http://` ou `https://`. Erros: 400 `invalid_request`, 404 `not_found`, 409 `dataset_not_ready` (0 imagens ativas), 503 `queue_unavailable` (manager fora — compensação do package). Job: `kind='autolabel'`, `engine='autolabel'`, `mode='autolabel'`.
  - **`POST /api/jobs/:id/autolabel/apply`** — body `{datasetId?, overwrite?}` → 200 `AutolabelApplyResponse{applied, skipped, images}` (`applied` = legendas gravadas, `skipped` = legendas ignoradas por preservação/formato, `images` = imagens distintas atualizadas). Merge por origem (D1): `overwrite=false` → preserva `manual`/`import` (aplica apenas em imagens sem caption ou origin='autolabel'); `overwrite=true` → sobrescreve inclusive manuais.
  - **Engine AutoLabel (`autolabel.py`)**: modo `openai` faz chamadas HTTP para `/chat/completions` com vision payload base64; modos `florence-2` e `qwen2-vl` executam legendagem contextual densa e analítica; modo `mock` opera determinístico.
  - **`captions.jsonl` (transporte):** linhas JSON `{"filename": "...", "caption": "..."}`. Coleta automática pelo orquestrador como artefato `kind='captions'`.
  - **Migration 0009 (`0009_captions_autolabel.sql`):** amplia constraint de `captions.origin` para aceitar `'autolabel'`.
- Nota Fatia Difusão LoRA (ADR-0018, spec 0.17.0, `packages/contracts/openapi.yaml`):
  - **`POST /api/jobs/diffusion`** — body `{datasetId, baseModel, triggerWord?, epochs?, batchSize?, learningRate?, rank?, alpha?, weights?, orchestratorId?}` → 202 `SubmitJobResponse{jobId,status:"queued",queuePosition?}`. Validação: `baseModel` ∈ `{sdxl, flux, sd15}`, epochs 1..100, batchSize ∈ {1, 2, 4, 8}, lr 1e-6..0.01, rank/alpha 4..128. Erros: 400 `invalid_request`, 404 `not_found`, 409 `dataset_not_ready` (0 imagens ativas), 503 `queue_unavailable`. Job: `kind='diffusion_train'`, `engine='diffusion'`, `mode='train'`.
  - **Empacotamento `engine="diffusion"`:** gera pares `{stem}.webp` + `{stem}.txt`. Cada arquivo `.txt` contém a legenda da imagem consultada na tabela `captions`, opcionalmente prefixada por `triggerWord`. Se não houver caption, contém apenas o triggerWord ou vazio.
  - **Orquestrador e Manager:** orquestrador despacha subcomando `train --config --output` e coleta `adapter.safetensors` (`kind='model'`) e `metrics.jsonl` (`kind='metrics'`). O manager registra automaticamente o artefato `.safetensors` no catálogo canônico `models` com `engine='diffusion'`.
- Nota Fatia Geração (ADR-0023, spec 0.25.0, `packages/contracts/openapi.yaml`):
  - **`POST /api/jobs/diffusion/generate`** — body `{baseModel?|customModelId?, prompt, negativePrompt?, width?, height?, steps?, guidanceScale?, seed?, quantization?, distilled?, batchSize?, loras?, orchestratorId?}` → 202 `SubmitJobResponse{jobId,status:"queued",queuePosition?}`. `baseModel` e `customModelId` são **XOR** (exatamente um). `loras`: array ≤4 de `{modelId: uuid, scale: number 0..2}`. `weights`/`loraScale` marcados deprecated (400 se coexistirem com `loras`). `batchSize`: 1..8 (default 1). Validação: prompt ≤ 4000 chars, dimensões 256..2048, `customModelId` válido (row engine='diffusion', kind='checkpoint', arch ∈ {sdxl, sd15}). VRAM mínima por arch+quant: sd15→6; sdxl/flux: 4bit→8, 8bit→12, none→16. Job: `kind='diffusion_generate'`, `engine='diffusion'`, `mode='generate'`.
  - Erros: 400 `invalid_request` (body malformado, XOR violado, `weights`+`loras` coexistentes, dimensões fora de domínio), 400 `unsupported_architecture` (custom arch fora de {sdxl, sd15}), 404 `not_found` (customModelId inexistente), 503 `queue_unavailable`.
  - **Orquestrador e Manager (daemon):** modo daemon (`DIFFUSION_DAEMON_ENABLED=1`, default 0) sobe 1 HTTP daemon por nó no 1º job `generate`; idle TTL 600s; lock 1 job por vez; preempção mata daemon idle antes de treino. Fallback one-shot (`docker run --rm`) quando daemon desabilitado ou falho. Artefatos glob: `generated_*.png` (kind `generated`), `thumb_*.jpg` (kind `generated_thumb`), `generation_meta.json` (kind `generated_meta`).
  - **Hook generations:** manager insere na tabela `generations` por imagem quando job `diffusion_generate` termina `done` com artefato `generated_meta` (idempotente via `ON CONFLICT (s3_key)`).
  - **`GET /api/generations?limit(1..200,50)&offset&baseModel&quantization&deleted`** → 200 `GenerationList{items,total}`; `url`/`thumbUrl` presigned quando `S3_PUBLIC_ENDPOINT_URL`; 503 `queue_unavailable`.
  - **`GET /api/generations/:id/data`** → 200 imagem (proxy via StoragePort); 404; 503.
  - **`POST /api/generations/delete`** (body `GenerationIdsRequest{ids: uuid[], 1..100}`) → 204 soft-delete; 400; 503.
  - **`POST /api/generations/export`** (body `GenerationIdsRequest`) → 200 `application/zip` stream; 400; 503.
  - **Schemas novos:** `Generation{id,jobId,filename,url,thumbUrl,width,height,seed,prompt,negativePrompt?,params,createdAt}`, `GenerationList{items,total}`, `GenerationIdsRequest{ids}`, `LoraRef{modelId,scale}`.
  - **`Model` (aditivo):** `kind: 'lora'|'checkpoint'|null`, `arch: 'flux-2-klein-4b'|'sdxl'|'sd15'|null` (migration 0011).
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
  `PUT /:id/classes` = substituição total com reconciliação por id (`{classes:[{id?,name}]}`): id presente = rename preservando id (caixas intocadas); ausente = cria; ordem do array = idx 0..n-1; cor rederivada da paleta server-side (nunca do cliente); validação pura em `models.rs` (regex única reutilizada, ≤200, nomes/ids únicos, dedupe silencioso proibido); remoção de classe com caixas ⇒ 409 `classes_in_use` (guard — que conta caixas de imagens ativas **e** da lixeira, conservador; o CASCADE do FK regeneraria ids e orfanaria as caixas); dance de `UNIQUE(name/idx)` na transação com ordem OBRIGATÓRIA guard → fase 1 (tmp via `id.simple()` por causa do `CHECK` de name, `idx+1000000`) → **DELETE das removidas** → fase 2 (finais) → INSERT novas (o DELETE entre as fases libera os slots de `idx` — sem ele, remover classe com `idx` menor que o destino renumerado de um mantido viola `UNIQUE(dataset_id, idx)` e vira 500; coberto por teste db). Lixeira: `DELETE /:id/images/:imageId` = soft delete (204, sem sweep, objeto intocado; 404 se inexistente/já deletada/UUID inválido); `POST /:id/images/:imageId/restore` (204 sem conflito; conflito de filename → rename `{stem}_restaurado{ext}` com desambiguação + `copy_object` server-side + delete da key antiga best-effort pós-commit → 200 `{filename}`; copy falha → 503 `storage_unavailable`, nada parcial); `DELETE /:id/trash` = purge REAL (CASCADE + sweep best-effort por prefixo de imagem pós-commit, 204 idempotente);    `GET /:id/images?deleted=true` lista a lixeira; `Dataset.trashCount` derivado (badge). O parágrafo "PUT boxes … classId tem de pertencer ao dataset senão 400 seco" continua verdadeiro.
- Nota Fatia 3f (ADR-0004, spec 0.5.0 — shapes reais em `services/api-principal/src/search/handlers.rs`, tabela de rotas ≡ `PROTECTED_ROUTES` em `src/auth/routes.rs`):
  ```
   POST /api/datasets/:id/search/index    202 401 404
   GET  /api/datasets/:id/search/status   200 401 404
   GET  /api/datasets/:id/search          200 400 401 404 409 503
   POST /api/datasets/:id/search/by-image 200 400 401 404 409
   ```
  `POST /:id/search/index` = rebuild fire-and-forget (202 sempre: dataset sem imagens ⇒ `{status:not_indexed}` sem trabalhar; senão spawna o indexador sob advisory lock de sessão `heph_index:{dataset_id}` e responde `{status:indexing}`). `GET /:id/search/status` ⇒ `SearchStatus{status,imagesCount,indexedCount,model,dim}` (D5 literal: 0 embeddings do modelo ativo ⇒ `not_indexed`; 0 < idx < images(ativas) ⇒ `indexing`; iguais ⇒ `ready`; `stale` nunca emitido na v1; `indexedCount` conta com JOIN em `images` ativas — `deleted_at IS NULL`, lixeira invisível). `GET /:id/search?q&k&classId&split` (`q` 1..500 chars obrigatório, `k` default 20 1..100, `classId` opcional — não-UUID ⇒ 400, divergência CONSCIENTE do D8, `split` train|val; validação pura 400 precede o 404 do dataset): embeda `q` e busca cosseno via HNSW global (`LIMIT k*4`, cap 400) com pós-filtro em Rust; `indexedCount == 0` ⇒ 409 `index_not_ready`; embedder fora ⇒ 503 `embedding_unavailable`; `score` = cosseno RAW (-1..1), desc.    `POST /:id/search/by-image` (`{imageId,k?,threshold?}`, `deny_unknown_fields`; `k` default 20 1..100, `threshold` -1..1): vetor de query = embedding da própria imagem no modelo ativo (NUNCA chama o embedder — sem 503); `imageId` não-UUID ou fora do dataset ⇒ 404; sem embedding ⇒ 409 `index_not_ready`; a própria imagem aparece com score ~1.0 (não excluída). `id` não-UUID → 404 `not_found` (D8). Erros novos: `index_not_ready` (409) e `embedding_unavailable` (503, família do `storage_unavailable` — ADR-0003 D10). Env do principal: `EMBEDDING_BACKEND=mock|http` (default `mock`), `EMBEDDER_URL` (default `http://embedder:8090`), `EMBEDDING_MODEL` (default `ViT-B-32`). Cargo: `pgvector = "=0.4.1"` (pin exato — 0.4.2 exige sqlx-core 0.9), reqwest 0.12 (rustls), base64 movido a deps. Wire camelCase global.
- Nota Fatia 3e (ADR-0006, spec 0.6.0 — shapes reais em `services/api-principal/src/datasets/{export,import}.rs` + validações puras em `models.rs`, tabela de rotas ≡ `PROTECTED_ROUTES` em `src/auth/routes.rs`):
  ```
   POST /api/datasets/:id/export   200 401 404 503
   POST /api/datasets/import      201 400 401 409 503
   ```
  `POST /:id/export` (sem body/query): 200 = `application/zip` em stream com `Content-Disposition: attachment; filename="{slug}.zip"` e `Content-Length` do spool; 404 `not_found` (id não-UUID/inexistente); 503 `storage_unavailable` (bucket fora). Pipeline em 3 fases: coleta async do banco + `get_to_file` por imagem para tempdir (novo método da `StoragePort`) → zip sync em `spawn_blocking` (`Stored` p/ imagens, `Deflated` p/ texto) → `ReaderStream` do arquivo. Só imagens ativas (`deleted_at IS NULL`); imagem com linha mas sem objeto ⇒ skip + `eprintln` (os `counts` do manifest refletem o exportado); dataset vazio ⇒ 200 com zip só-manifest. `POST /datasets/import` (multipart `file` obrigatório + `title` opcional ≤96 chars + `replace` opcional `"true"`/`"false"`, ausente = `false`, valor inválido ⇒ 400 `invalid_request`): 201 = `Dataset` existente (nenhum schema novo de resposta); 400 `invalid_request` (form: sem `file`, `title`/`replace` inválidos) | **`import_invalid`** (erro novo, "invalid import package": manifest ausente/incompatível/corrompido, zip malformado, zip-slip, zip bomb, dedupe intra-zip pós-sniff, sha256/media_type divergentes, domínios inválidos); 409 `slug_conflict` = protocolo de detecção da substituição consentida (slug existente + sem `replace=true`; o servidor NUNCA substitui sozinho — a UI confirma a irreversibilidade e re-envia com `replace=true` ⇒ teardown + ingest com dataset_id novo, 201; validação completa do pacote ANTES de qualquer teardown, zip corrompido nunca destrói o existente; `replace=true` com slug inexistente importa normalmente). Limites: corpo total `IMPORT_BODY_LIMIT_BYTES` = 200 MiB + 8 MiB de envelope na rota (mesmo padrão do upload 3b; excesso ⇒ 413 `invalid_request` no envelope).    `POST /:id/package` segue pendente (fatia 4, revisão D0 da ADR-0006). Sem migration (schema já tinha `origin` com `import` desde a 0003).
- Nota Fatia AC-003 (ADR-0024 D3/D4, migration 0012+0013, spec 0.28.0, `packages/contracts/openapi.yaml`):
  - **`DELETE /api/jobs/:id`** — exclui job terminal via manager. Sweep best-effort das `object_keys` retornadas (lista EXATA de chaves, não prefixo — preserva galeria `generations` cujos bytes vivem sob `artifacts/{job_id}/`). Status: 200 `JobDeletedResponse` | 401 | 404 `not_found` (UUID inválido ou inexistente) | 409 `job_not_terminal` (não-terminal) | 503 `queue_unavailable`.
    - `JobDeletedResponse`: `{id, status, artifacts: string[], objectKeys: string[], modelsDeleted: int, generationsPreserved: int}` (camelCase). `modelsDeleted` = linhas do catálogo `models` derivadas do job e expurgadas; `generationsPreserved` = gerações preservadas (FK `SET NULL`, migration 0012).
    - Manager: `delete_job` (`lib.rs:2211`) — lock `FOR UPDATE`, guarda `TERMINAL_STATUSES`, `plan_job_sweep` (chaves exatas sem gerações + DELETE `models` WHERE `job_id`), `DELETE FROM jobs`, commit. Não-terminal ⇒ `NotDeletable` (409); inexistente ⇒ `NotFound` (404).
    - Rotas internas manager: `DELETE /internal/jobs/:id` (handler `delete_job_handler`, `main.rs:305`), `POST /internal/jobs/cleanup` (handler `cleanup_jobs_handler`, `main.rs:332`).
  - **`POST /api/jobs/cleanup`** — limpeza em lote de jobs terminais. Body: `{olderThanDays?: int ≥ 0, statuses?: ("done"|"failed"|"cancelled")[]}` (ambos opcionais; exige pelo menos um critério — ausência ⇒ 400 `invalid_request`). Status: 200 `JobCleanupResponse` | 400 `invalid_request` (body inválido ou sem critério) | 401 | 503 `queue_unavailable`.
    - `JobCleanupResponse`: `{deleted: int, jobs: JobDeletedResponse[], objectKeys: string[]}` (camelCase). Cada job na lista segue o mesmo shape de `DELETE /:id`. `objectKeys` é a união das chaves de todos os jobs apagados.
    - Manager: `cleanup_jobs` (`lib.rs:2283`) — `FOR UPDATE SKIP LOCKED`, seleciona terminais com filtro de idade (`COALESCE(finished_at, created_at) < NOW() - (N days)`), faz `plan_job_sweep` + `DELETE FROM jobs` por job, commit único. Status não-terminal no filtro ⇒ 400 `invalid_request` (não aceita não-terminais).
  - **Comportamento sweep S3:** lista EXATA de chaves (artifacts + models, sem generations preservadas) varrida item por item no principal pós-commit (`sweep_object_keys`, `handlers.rs:1908`). Cada `delete` é best-effort: `NotFound` ignorado, erro logado (`eprintln`) sem transformar 200 em erro. **Decisão:** lista exata em vez de `DeleteObjects` em lote — volumes baixos na v1, preserva galeria (`ADR-0024 D3/D4`; migration 0012 altera FK `generations.job_id` para `ON DELETE SET NULL`).
  - **Wire camelCase** (`job_deleted_to_wire` / `cleanup_result_to_wire`): manager retorna `object_keys`/`models_deleted`/`generations_preserved` (snake_case); principal remapeia para `objectKeys`/`modelsDeleted`/`generationsPreserved` antes de retornar ao client.
  - **Erros novos na enum:** `job_not_terminal` (409, `DELETE /:id` em estado não-terminal).
- Nota AC-006-A (ADR-0024, migration 0013):
  - **`GET /api/jobs/:id/metrics`** — o array `items` passa a conter **só pontos de dados de treino** (linhas com ao menos um valor numérico: `loss`, `lr`, `box_loss`, `cls_loss`, `dfl_loss`, `mAP50`, `mAP50-95`). Linhas de status/fase do engine (boot, `training_started`, progresso por imagem) não entram mais no array — classificação feita no orquestrador (`is_training_metric`, `orchestrator/src/lib.rs:357-365`, D1 da ADR-0024). Jobs antigos (pré-migration 0013) mantêm linhas de status dentro de `jobs.metrics` — o filtro B do front (`lib/jobMetrics.ts`) cobre a leitura; sem backfill.
  - **`Job.phase` / `phaseMessage`:** agora têm origem direta nas colunas `jobs.phase` / `jobs.message` (migration `0013_job_status.sql`: `phase TEXT`, `message TEXT`). O manager persiste o ÚLTIMO status reportado (`report_job` grava `COALESCE($n, phase)` — D3 da ADR-0024). `to_job_response` lê essas colunas com fallback de status-para-fase quando `phase` é None; a derivação a partir do último item do array metrics é REMOVIDA (D4 da ADR-0024). `vramUsedGb` permanece derivado da última métrica.
  - Spec OpenAPI: 0.28.0 (`packages/contracts/openapi.yaml`).

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
captions(image_id UUID PK FK CASCADE, text TEXT CHECK 1..8000, origin TEXT CHECK manual|autolabel|autotracker|import,
  model TEXT NULL, updated_at TIMESTAMPTZ + trigger tg_set_updated_at da 0002);
image_embeddings(image_id UUID PK FK images(id) CASCADE, dataset_id UUID FK datasets(id) CASCADE,
  model TEXT CHECK (model IN ('ViT-B-32')), embedding vector(512) NOT NULL, created_at TIMESTAMPTZ);
  -- IMPLEMENTADO (Fatia 3f; `migrations/0004_search.sql` à letra): `dataset_id`
  -- denormalizado de propósito (filtro da busca + CASCADE duplo; consistência é
  -- obrigação do indexador); índices `image_embeddings(dataset_id, model)` e
  -- HNSW `USING hnsw (embedding vector_cosine_ops) WITH (m = 16, ef_construction = 64)`.
videos(id UUID PK, dataset_id UUID FK CASCADE, filename TEXT CHECK 1..255, object_key TEXT UNIQUE,
  fps DOUBLE NULL, frames INT NULL CHECK >= 0, md5 TEXT CHECK ^[0-9a-f]{32}$,
  bytes BIGINT CHECK >= 0, created_at TIMESTAMPTZ, UNIQUE(dataset_id, filename));
  -- IMPLEMENTADO (Fatia 3b): idem objeto, índice `videos(dataset_id)`; SEM rota de escrita
  -- na 3b (nasce agora porque DDL é estático e a FK CASCADE vem junto das irmãs — ADR-0003 D5).
dataset_versions(id UUID PK, dataset_id UUID FK, manifest JSONB, created_at TIMESTAMPTZ);
  -- IMPLEMENTADO (Fatia 4; `migrations/0006_jobs.sql`): snapshot JSONB do dataset no despacho do job (T4 ADR-0002).
  -- `dataset_id` FK ON DELETE CASCADE (a versão morre com o dataset).
  -- Dono: principal (domínio de dataset); manifest snake_case: {dataset{id,slug,category,engine},classes[],images[{filename,split,width,height,boxes[],caption?}],counts}.
  -- "Trava lógica por versão" — edição posterior do dataset não afeta a row (§10/:258).
  -- Índice: `dataset_versions(dataset_id, created_at)`.
orchestrators(id UUID PK, name TEXT, endpoint TEXT UNIQUE, kind TEXT,     -- local|remoto
  fingerprint TEXT, token_hash TEXT, gpus JSONB, vram_total_gb INT,
  status TEXT, last_heartbeat TIMESTAMPTZ);
  -- IMPLEMENTADO (Fatia 4; `migrations/0006_jobs.sql`): manager auto-adota `orchestrator-local` no boot (ADR-0007 D3).
  -- Índice: `orchestrators(status)`.
  -- Fatia H (ADR-0011): heartbeat identificado (`endpoint` no `HeartbeatBody`) grava `gpus`/`vram_total_gb` dinamicamente
  -- (round(MiB/1024), GiB). Watchdog: `online → degraded` (15s) → `offline` (60s); re-queue dos jobs do nó morto.
  -- Adopt por token (pairing code single-use, upsert); revoke = tombstone `revoked` (não DELETE).
models(id UUID PK, engine TEXT NOT NULL CHECK (engine IN ('yolo','world','diffusion','clip')),
  name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 255),
  model TEXT,                                              -- variante conhecida (treino); NULL p/ upload/download
  kind TEXT NULL CHECK (kind IS NULL OR kind IN ('lora','checkpoint')),  -- só semântica para engine='diffusion' (ADR-0023 D4)
  arch TEXT NULL CHECK (arch IS NULL OR arch IN ('flux-2-klein-4b','sdxl','sd15')),  -- checkpoint: arquitetura (ADR-0023 D4)
  s3_key TEXT NOT NULL UNIQUE,                             -- 'models/<engine>/<id>/<name>' | 'artifacts/<job_id>/<path>'
  source TEXT NOT NULL CHECK (source IN ('train','upload','download')),
  url TEXT,                                                -- fonte original do download; NULL p/ upload/train (transporte interno)
  hash TEXT NOT NULL CHECK (hash ~ '^[0-9a-f]{32}$'),      -- md5 (padrão da casa)
  bytes BIGINT NOT NULL CHECK (bytes >= 0),
  job_id UUID REFERENCES jobs(id) ON DELETE SET NULL,      -- origem do treino; upload/download NULL
  created_at TIMESTAMPTZ NOT NULL DEFAULT now());
  -- IMPLEMENTADO (Fatia I; `migrations/0007_models.sql`): tabela canônica de pesos (catálogo; ADR-0012 D1/D2).
  -- Dono: manager. Leitura: `GET /internal/models` (rota interna existente — troca a fonte de derived SQL para SELECT da tabela).
  -- Escrita: (a) hook no `report_job` do manager — job `done` com artefato `kind='model'` e `path` contendo `best` → INSERT ON CONFLICT (s3_key) DO NOTHING;
  --          (b) `POST /internal/models` — cria row a partir de upload/download do principal (compensação delete se INSERT falhar).
  --          (c) `DELETE /internal/models/:id` — remove row do banco (204 No Content, 404).
  -- Backfill na migration: INSERT..SELECT dos artefatos `kind='model' AND path LIKE '%best%'` de jobs `done` (idempotente via ON CONFLICT).
  -- CHECK engine ampliado para `('yolo','world')` na migration `0008_world_models.sql` (ADR-0014 D1)
  -- e para `('yolo','world','diffusion','clip')` na migration `0010_models_engines.sql` (Fatia Gestão de Modelos).
  -- Colunas `kind`/`arch` adicionadas na migration `0011_generations.sql` (ADR-0023 D4): distingue LoRA de checkpoint
  -- e arquitetura (sdxl/sd15/flux-2-klein-4b); backfill: diffusion existentes → kind='lora'.
  -- Índices: `models(engine)`, `models(created_at DESC)`.
  -- NOTA: checkpoint de treino vive em `artifacts/<job_id>/` (morre com o job via CASCADE; FK ON DELETE SET NULL no models.job_id preserva o modelo).
  --       Exclusão pública via `DELETE /api/models/:id`: remove S3 se upload/download e deleta row no manager.
jobs(id UUID PK, kind TEXT, dataset_id UUID NULL FK, engine TEXT, model TEXT, mode TEXT,
  params JSONB, config_yaml TEXT, status TEXT, queue_reason TEXT NULL,
  orchestrator_id UUID NULL FK, vram_min_gb INT, progress FLOAT,
  epoch INT, step INT, metrics JSONB, created_at TIMESTAMPTZ, finished_at TIMESTAMPTZ NULL,
  phase TEXT, message TEXT);
  -- IMPLEMENTADO (Fatia 4; `migrations/0006_jobs.sql`): ciclo `queued→dispatched→preparing→running→done|failed|cancelled`.
  -- CHECK de status inclui `cancelling` (janela transitória entre abort aceito e confirmação do orquestrador — ADR-0007 D3/D7).
  -- `dataset_id` FK ON DELETE SET NULL (T4); `orchestrator_id` FK ON DELETE SET NULL.
  -- `params` JSONB inclui `package_ref` (snake_case): `{version_id,key,md5_zip,bytes}` (D1b ADR-0007).
  -- `config.yaml` gerado pelo principal com placeholders `{dataset_path}`/`{output_path}` substituídos pelo orquestrador no spawn.
  -- `metrics` JSONB é snake_case (transporte) — principal re-mapeia para camelCase no response de `/api/jobs/:id/metrics` (mAP50-95 → map5095).
  -- IMPLEMENTADO (AC-006-A; `migrations/0013_job_status.sql`): `phase TEXT` e `message TEXT` — snapshot do último status reportado (ADR-0024 D3).
  -- `report_job` grava `COALESCE($n, phase)`; o array `metrics` passa a conter só pontos de treino (linhas de status/classificação ficam fora — D1).
  -- Índices: `jobs(status)`, `jobs(dataset_id)`, `jobs(created_at)`.
job_artifacts(id UUID PK, job_id UUID FK, kind TEXT, path TEXT, md5 TEXT, bytes BIGINT);
  -- IMPLEMENTADO (Fatia 4; `migrations/0006_jobs.sql`): artefatos retornados pelo orquestrador (ADR-0007 D8).
  -- `job_id` FK ON DELETE CASCADE; `md5` CHECK hex 32; `bytes` CHECK >= 0.
  -- Grava kind/path/md5/bytes — `path` é relativo ao prefixo `artifacts/<job_id>/`.
  -- Escrita: manager insere a partir do report `done` do orquestrador (orquestrador não toca Postgres — stateless).
  -- NOTA (Fatia I): `kind='model'` continua existindo (registro do job) mas o catálogo canônico de modelos
  -- agora é a tabela `models` (hook do manager registra `best.pt` por job `done` via INSERT na tabela models).
  -- `artifactsBytes` no storage/usage EXCLUI `kind='model'` (sem dupla contagem — ADR-0012 D8).
  -- Índice: `job_artifacts(job_id)`.
job_samples(job_id UUID FK, cycle INT, idx INT, image_path TEXT, meta JSONB, PRIMARY KEY(job_id, cycle, idx));
runners(id UUID PK, engine TEXT, model TEXT, orchestrator_id UUID FK,
  status TEXT, vram_gb INT, last_used TIMESTAMPTZ);
generations(id UUID PK, job_id UUID NULL FK jobs ON DELETE SET NULL,
  s3_key TEXT NOT NULL UNIQUE, thumb_s3_key TEXT,
  filename TEXT NOT NULL CHECK (char_length(filename) BETWEEN 1 AND 255),
  seed BIGINT NOT NULL CHECK (seed >= 0),
  prompt TEXT NOT NULL CHECK (char_length(prompt) BETWEEN 1 AND 4000),
  negative_prompt TEXT CHECK (negative_prompt IS NULL OR char_length(negative_prompt) <= 4000),
  width INT NOT NULL CHECK (width BETWEEN 256 AND 2048),
  height INT NOT NULL CHECK (height BETWEEN 256 AND 2048),
  params JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  deleted_at TIMESTAMPTZ);
  -- IMPLEMENTADO (Fatia Geração; ADR-0023 D5; migration 0011_generations.sql): galeria persistente de imagens geradas.
  -- Dono: manager (hook no report_job — job diffusion_generate done com artefato generated_meta → INSERT por imagem).
  -- Soft-delete: deleted_at (objeto S3 intocado; sweep é dívida); índice parcial WHERE deleted_at IS NOT NULL.
  -- IMPLEMENTADO (AC-003; `migrations/0012_generations_job_optional.sql`): FK `job_id` alterada para `NULL + ON DELETE SET NULL`
  -- (apagar um job não apaga a galeria — os bytes vivem sob `artifacts/{job_id}/`, as linhas `generations` preservam `s3_key` válido).
  -- Índices: `generations(created_at DESC)`, `generations(job_id)`, `generations(deleted_at) WHERE deleted_at IS NOT NULL`.
```

- Índices: `images(dataset_id)`, `images(dataset_id, split)`, `images(dataset_id, filename) parcial ativa + images(dataset_id) parcial lixeira (0005)`, `boxes(image_id)`, `boxes(class_id)`, `videos(dataset_id)`, `classes(dataset_id, idx)`, `image_embeddings(dataset_id, model)` + HNSW do embedding (0004), `orchestrators(status)`, `jobs(status)`, `jobs(dataset_id)`, `jobs(created_at)`, `job_artifacts(job_id)`, `dataset_versions(dataset_id, created_at)`, `generations(created_at DESC)`, `generations(job_id)`, `generations(deleted_at) WHERE deleted_at IS NOT NULL`.
- Nota (ADR-0002 D1, casing — resolvido; era ADR-0001 T3 "a definir antes da Fatia 3"): wire camelCase em `/api/*` (`userId`, `sizeBytes`, `lastModified`, settings `hfToken`…); colunas SQL snake_case; valores de enum, `Error.code` e artefatos de transporte (`manifest.json`, `config.yaml`, SQLite do orquestrador) snake_case.
- Regra: contadores do dataset recalculados por função única `heph_refresh_dataset_counters(uuid)` — IMPLEMENTADO (Fatia 3b; `migrations/0003_images.sql`, fecha ADR-0002 T2): recalcula `images_count`/`labeled_count`/`size_bytes` e deriva `status` (`needs_labeling`/`in_progress`/`ready`) a partir das tabelas-fato, nunca `+=` (drift impossível; `UPDATE` com guarda `IS DISTINCT FROM` evita churn de `updated_at`); disparada por triggers `AFTER INSERT OR UPDATE OR DELETE` em `images`, `videos` e `boxes`/`captions` (via lookup de `dataset_id`); a ordem trigger-usuário × cascata-RJ do `DELETE FROM images` deixa de importar (o último disparo vê o estado final); `labeled` = imagem com ≥1 box (format `yolo_txt`) **ou** linha em `captions` (demais formats) — taxonomia R9; `size_bytes` soma `images` + `videos`. Chaves externas com `ON DELETE CASCADE` de dataset→filhos. O invariante `labeled_count <= images_count` continua sem `CHECK` (não deferrável) — é obrigação do trigger.
- **Split:** coluna `images.split (train|val)`; padrão 80/20 estratificado no package com override manual na galeria (seletor train/val por imagem).
- **Materialização YOLO no export (Fatia 3e):** nasce do banco em tempdir (a doutrina acima é mantida — "O package sempre gera do banco"). Desambiguação de labels de mesmo stem: a primeira imagem com um dado stem fica com `labels/{stem}.txt` e a colisão (ex.: `a.jpg` + `a.png`) vira `labels/{stem}_{ext}.txt` (código: `src/datasets/export.rs::label_arcname_for`). **Limitação YOLO documentada:** um consumidor YOLO externo resolve as imagens de mesmo stem para o primeiro `labels/{stem}.txt` (o roundtrip interno não é afetado — o import lê o `manifest.json` e ignora os `labels/*.txt`).
- **Consistência bucket×banco (ADR-0003 D1/D6/D7):** Postgres é a verdade relacional, o bucket é a verdade binária (linhas de `images` como índice). `PUT boxes/caption` atualiza SÓ o banco — a materialização do `.txt` com debounce (~2s) MORREU (não há mais o que materializar; `data.yaml`/`labels/*.txt`/`captions.jsonl`/`.parquet` nascem no tempdir do orquestrador a partir do Postgres). Ordem de escrita objeto→linha→compensação: PUT do objeto → `INSERT ... ON CONFLICT (dataset_id, filename) DO NOTHING` (sem linha ⇒ `duplicate` + DELETE best-effort do objeto); `DELETE /api/datasets/:id` commita o banco primeiro (CASCADE) e depois varre o prefixo `datasets/{id}/` (`ListObjectsV2` paginado + `DeleteObjects` de 1000) best-effort — falha do sweep loga (`eprintln`) e não transforma o 204 em erro. O package sempre gera do banco.
- **Snapshot:** ao despachar job, congela `dataset_versions{manifest}` e o zip aponta para a versão; edição posterior não afeta treino em voo (trava lógica por versão, não por dataset).

## 11. Schemas, retenção e setup inicial

- `manifest.json` (transporte orquestrador — IMPLEMENTADO Fatia 4; ADR-0007 D1): `{dataset_id, slug, category, engine, files:[{filename,md5,bytes}], md5_zip, bytes, chunks:null, created_at}`. **Na v1 local:** `files[].key` ausente (não há leitura por objeto — o orquestrador baixa o zip inteiro); `chunks: null` (não há transporte chunked — D9 da ADR-0003, adiado para orquestrador remoto). Snake_case (transporte, fora de `/api/*`). **Não confundir** com o `manifest.json` de backup da Fatia 3e (artefato distinto, D1 da ADR-0006: `schema_version/classes/images/boxes` — fonte da verdade do roundtrip export/import, com `dataset.yaml`/`labels/*.txt`/`captions.jsonl` como derivados ignorados no re-import).
- `config.yaml` por job — IMPLEMENTADO (Fatia 4; ADR-0007 D6): comum `{job_id, engine, model, mode, dataset_path, output_path, seed}` + específico:
  - yolo: `{model, epochs, batch, imgsz, lr0, optimizer, augment:{mosaic, mixup_flip}}`.
  - difusao train: `{base_model, trigger_word, rank, alpha, optimizer, steps, lr, cfg}` (adiado).
  - difusao generate (ADR-0023): `{model, generate:{prompt, negative_prompt, seed, base_model?, width, height, steps, guidance_scale, quantization, distilled, batch_size, loras:[{path, scale}], custom_checkpoint_path?, arch?}}`. Retrocompat: `weights_path` root-level quando modo legado (sem loras, sem custom).
  - clip: `{backbone, embed_dim, loss, lr, warmup, batch, epochs}` (adiado).
  - `dataset_path`/`output_path` são **placeholders** (`{dataset_path}`/`{output_path}`) substituídos pelo orquestrador no momento do spawn do container trainer com os mounts reais. O principal é agnóstico de paths locais.
- `engines.yaml`: `{engine, image, cuda, torch, validated_at}` — ex. `trainer-difusao: hephaestus/trainer-difusao:local`.
- Retenção: samples últimos 5 ciclos ou 500 MB/job; logs 10 MB + 30 dias; artifacts guarda `best + last`, resto com GC manual (`DELETE /api/jobs/:id/artifacts?keep=best,last`).
- Setup: `STUDIO_PASSWORD` no primeiro boot (hash Argon2 em `users`); troca via CLI `studio reset-password` (sem expor rota); chaves cifradas app-level com `STUDIO_MASTER_KEY` (nunca em log); `infra/compose.yaml` sobe `db (pgvector/pgvector:pg16-trixie@sha256:c8483555ce48101872f888c1df8a895ff689d6c7c7a5f7ac266475f9dfe89e0b) + principal (:8080) + manager + orquestrador-local (socket docker) + web (next) + seaweedfs (:8333 S3, :9333 master UI) + embedder (127.0.0.1:8090)` com volumes `pgdata, seaweed_data, datasets, models, outputs`. Serviço novo `seaweedfs` (`chrislusf/seaweedfs:4.45_full` pinado, `server -s3 -ip.bind=0.0.0.0 -s3.port=8333 -s3.config=/etc/seaweedfs/s3.json`, identidade em `infra/seaweedfs-s3.json`, bind loopback `127.0.0.1:8333:8333` + `127.0.0.1:9333:9333`, healthcheck por `wget` com `403 = no ar`); principal com `STORAGE_BACKEND=s3`, `S3_ENDPOINT_URL=http://seaweedfs:8333`, `S3_BUCKET=heph-data`, `S3_PUBLIC_ENDPOINT_URL=http://localhost:8333`, `S3_URL_TTL_SECS=3600`, mais `EMBEDDING_BACKEND=mock` (`mock|http`), `EMBEDDER_URL=http://embedder:8090`, `EMBEDDING_MODEL=ViT-B-32` (Fatia 3f). Serviço novo `embedder` (Fatia 3f, ADR-0004 D1; build `engines/trainer-clip`, `ENGINE_MOCK=1`, porta `127.0.0.1:8090`, healthcheck stdlib via `/health`, volume `models:/data/models`); o volume `models` é reusado como cache do peso (~600 MB) no caminho real. O volume `datasets` do orquestrador-local SOBREVIVE (para `models`/`outputs`/cache de build); o volume `datasets` do principal morreu (blobs no bucket).
- **Execução sem DinD (RunPod padrão):** orquestrador opera em 2 modos — `docker` (socket disponível) ou `subprocess` (venv/python direto no mesmo host). Pods sem socket usam modo subprocess; template com DinD é opcional, não requisito.
- Nota Fatia I (ADR-0012, spec 0.11.0):
  - **`GET /api/models`**: fonte trocada de `job_artifacts` (derived `DISTINCT ON`) para tabela canônica `models` (D2). Contrato preservado com shape aditivo: `Model{id,name,engine,model?,source,bytes,md5,url?,jobId?,createdAt}` (D6). A dedupe por `(engine,model)` acabou: cada treino `done` produz 1 linha (o melhor checkpoint); 2 treinos do mesmo yolo11m = 2 modelos na lista (mudança de semântica do StatCard — checkpoints, não modelos distintos).
  - **`POST /api/models/upload`**: multipart `file`+`engine`+`name?`, `DefaultBodyLimit` 2 GiB + 8 MiB envelope (413), magic `PK\x03\x04` (400 se divergir), sanitização de nome, md5 (hex 32), spool em tempdir + `put()` streama do disco (sem RAM). 201 `Model`. Compensação `delete` do objeto se o INSERT no manager falhar.
  - **`POST /api/models/download`**: body `{url, engine, name?}` (camelCase, `deny_unknown_fields`). Server-side no **principal** (reqwest stream → PUT S3). Allow-list fail-closed via `MODEL_DOWNLOAD_ALLOWED_HOSTS` (env ausente/vazia = 403 `model_download_disabled`); deny de ranges privados/metadata (127/8, 10/8, 172.16/12, 192.168/16, ::1, fc00::/7, fe80::/10, 169.254/16); redirects re-validados a cada hop (máx 5); cap 2 GiB, timeouts 30s/120s; falha de rede/timeout/tamanho/hash → 502 `model_download_failed`. Síncrono (sem fila, sem WS). Divergência consciente da visão §9/:104 original (ADR-0012 D4).
  - **`POST /api/jobs/yolo` ganha `weights?: string`** (UUID de `models`). Validação pura: não-UUID → 400 `invalid_request`; manager resolve no `create_job`: inexistente → 404 `not_found`, engine≠yolo → 400 `invalid_request`; grava `params.weights_ref = {s3_key, md5}` (JSONB); dispatch com `weights_ref` → orquestrador stagia em `outputs/<job_id>/weights/` e substitui `{weights_path}` no config.yaml.
  - **`GET /api/storage/usage`**: `StorageUsage` ganha `modelsBytes` (aditivo). `artifactsBytes` exclui `kind='model'` (sem dupla contagem). `totalBytes` = datasets + artifacts + models.
  - **Erros novos**: `model_download_failed` (502, download por URL falhou), `model_download_disabled` (403, allow-list ausente/vazia). 413 continua `invalid_request`. Wire: `{code,message}` estáticos.
  - **Wire `Model` (camelCase)**: `{id,name,engine,model?,source,bytes,md5,url?,jobId?,createdAt}`. `model`/`jobId`/`url` nullable (upload/download não têm jobId nem variante; sem `S3_PUBLIC_ENDPOINT_URL` → `url:null`). `ModelWeight` → `Model` (aditivo — campos existentes preservados).
  - **Spec OpenAPI**: 0.10.0 → **0.11.0** (`packages/contracts/openapi.yaml`).
  - **Erros novos na enum**: `model_download_failed`, `model_download_disabled` (ADR-0012 D6/E1).
- Nota Fatia J (ADR-0013, spec 0.12.0):
  - **`POST /api/jobs/predict`**: body `{modelId: uuid, datasetId: uuid, conf?: number 0..1 default 0.65}` (camelCase, `deny_unknown_fields`). Validação pura: modelId não-UUID → 400; conf fora de 0..1 → 400. Readiness: dataset category=yolo + ≥1 imagem ativa (classes NÃO obrigatórias — divergência do autotracker). Build package via `build_package()` compartilhado (D2). Manager body: `kind='yolo_predict'`, `engine='yolo'`, `mode='predict'`, `model='predict'` placeholder, `weights_id=modelId`, `vram_min_gb=null`. **Mapeamento R6**: `NotFound→404`, `InvalidRequest→400`, `Unavailable→503` + compensação do package em TODOS os erros (diferente do submit_yolo_job que mapeia `Err(_)→503`).
  - **Manager**: `dispatch_body` ganha `"mode"` (já trazia do SELECT — L1842); `create_job` com `model=="predict"` + variante NOT NULL na row de `models` → `jobs.model=variante` (display honesto no `/jobs`).
  - **Orquestrador**: `DispatchRequest.mode: String` (`#[serde(default)]` — tolerância forward-compat); matriz `(engine, mode)` → subcomando/artefatos: `(yolo,predict)` → subcomando `predict` + `[predictions.json predictions]` (sem metrics.jsonl — progresso binário honesto).
  - **Engine**: subcomando `predict` no trainer-yolo (`predict.py`); mock determinístico reusando geradores do autotrack; real `YOLO(weights_path).predict(source=images/, conf, imgsz=640, device=0)` → `predictions.json` com `engine: "yolo"`.
  - **Spec OpenAPI**: 0.11.0 → **0.12.0** (`packages/contracts/openapi.yaml`). Sem erro novo na enum (reuso de `invalid_request`/`not_found`/`dataset_not_ready`/`queue_unavailable`). Sem migration (`jobs.mode='predict'` nasce na coluna TEXT existente).
