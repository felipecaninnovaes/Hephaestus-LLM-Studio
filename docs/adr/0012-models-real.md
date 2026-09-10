# ADR-0012 — Models real: tabela `models` + upload + download por URL + fine-tune (Fatia I)

- **Status:** **PROPOSTA** (aguarda aceite do usuário). Nada implementado. Este
  documento é a especificação executável da fatia "MODELS REAL"; os deltas de
  contrato abaixo são aplicados **apenas nos commits da fatia** (openapi junto
  do código, docs de texto no `docs-sync` do fim), nunca antes.
- **Data:** 2026-09-10
- **Componentes:** `services/manager` (tabela `models` dona do manager, hook de
  registro no report, `POST /internal/models`, resolução de pesos no
  `create_job`), `services/api-principal` (BFF: `POST /api/models/upload`,
  `POST /api/models/download`, `GET /api/models` com fonte trocada,
  `weights` no `POST /api/jobs/yolo`, `modelsBytes` no storage/usage),
  `services/orchestrator` (escopo S3 `models/*`, download+staging de pesos,
  placeholder `{weights_path}`), `infra/seaweedfs-s3.json` (credencial do
  orquestrador ganha `models/*`), `apps/web` (página `/models` + dropdown de
  pesos em `/treino` — via /impeccable), `packages/contracts` (spec 0.10.0 →
  **0.11.0**).
- **Fontes:** `IDEIA.md` §1/:14-16 (config de treino gerada pelo principal);
  `docs/backend.md` §9/:141-142 (`GET /api/models` implementado derivado;
  upload/download **pendentes** = fatia Roadmap "Modelos & Pesos"), §9/:104
  (visão futura do download via manager→orquestrador + volume `models/` + WS),
  §10/:298-302 (esboço da tabela `models` — **NÃO migrada**; nota "a tabela
  nasce quando a fatia Roadmap implementar upload/download"), §9/:175
  (`GET /api/storage/usage` = datasetsBytes + artifactsBytes); `docs/frontend.md`
  §10/:249 (`ModelWeight{id,name,engine,model,jobId,bytes,createdAt}` consumido
  pelo StatCard do dashboard), §10/:253 (upload/download pendentes), Sidebar
  `apps/web/components/studio/Sidebar.tsx` L246-253 (módulo "Modelos & Pesos" =
  div desabilitada honesta, badge Roadmap); `docs/adr/0009-web-integracao-
  monitoramento.md` D2 (lista v1 derivada de `job_artifacts.kind='model'`;
  **"quando a fatia real de modelos chegar (upload/volume/tabela), a rota troca
  a fonte, o contrato permanece"**), D3 (storage = soma SQL por dono, NÃO
  ListObjects), R2 (job_artifacts FK CASCADE do job); `docs/adr/0010-treino-
  real-gpu.md` D3 (manager **não tem** S3 — orquestrador baixa do bucket),
  D11 (mock default `ENGINE_MOCK=1`); `docs/adr/0003-object-storage-s3.md`
  (bucket `heph-data` canônico, presigned via `S3_PUBLIC_ENDPOINT_URL`); FATO
  spike F4.0 (path-scope do SeaweedFS legacy provado com 2 prefixos; action-
  type NÃO enforceado — residual aceito). Código (verificado por graft/grep
  nesta data): `services/manager/src/lib.rs` (`list_models` L1263-1291 — SQL
  `DISTINCT ON (j.engine, j.model)` derivado; `CreateJobRequest` L47-58;
  `dispatch_next` L1528-1620 — monta `dispatch_body` com package_ref/
  config_yaml), `services/manager/src/main.rs` L381-387 (`GET /internal/models`),
  `services/api-principal/src/jobs/manager_client.rs` (`ManagerPort::list_models`
  L187, `InternalModel` L124-132, `HttpManager::list_models` L402-409),
  `services/api-principal/src/monitoring.rs` (BFF `GET /api/models`),
  `services/api-principal/src/jobs/models.rs` (`generate_config_yaml` L330-364
  — placeholders `{dataset_path}`/`{output_path}`; `ALLOWED_MODELS` L10 =
  yolo11n/11m/11x/yolov9-c/yolo11-seg), `services/api-principal/src/datasets/
  handlers.rs` L547 (`MAX_FILE_BYTES` 200 MiB), `services/api-principal/src/
  auth/routes.rs` L151/L229 (`DefaultBodyLimit` 200 MiB + 8 MiB envelope),
  `services/api-principal/src/storage/s3.rs` (put/get/get_to_file/presign_get/
  delete/delete_prefix/copy_object — **sem** list público), `services/
  orchestrator/src/lib.rs` (`S3Scope` L215-226 — `Packages`/`Artifacts`;
  `scoped_key` L238-253 — 8 testes de recusa; `DispatchRequest` L20-29 sem
  `weights_ref`; `run_job_inner` L816-1135 — volumes `[(vol_datasets,
  /datasets), (vol_outputs, /outputs)]`, substitution de placeholders no
  config.yaml, `build_docker_run_args` L605-644), `engines/trainer-yolo/src/
  trainer_yolo/train.py` (`load_and_validate_config` L97-128 — required-keys,
  tolera extras; `_real_train` L286-341 — `YOLO(yolo_cfg["model"])`),
  `infra/seaweedfs-s3.json` (identidade `heph-orchestrator` = Read/Write/List
  em `packages/*`+`artifacts/*`), `infra/compose.yaml` L88-89/L132-135 (volume
  `models` montado em embedder e orchestrator-local), `packages/contracts/
  openapi.yaml` (version 0.10.0; `ModelWeight` L2903; `GET /api/models`
  L1812-1836).
- **Sequência:** Fatia H (mergeada) → **I (esta: models real)** → dívidas
  registradas.

## Contexto

O ciclo dados → treino → modelo → reuso está **aberto na ponta do modelo**: um
treino real na GPU produz `best.pt` real como artefato de job (5.429.331 bytes
provados na sessão GPU da ADR-0010), mas o produto não consegue (a) listar/usar
modelos fora da derivação de artefatos, (b) fazer upload de pesos externos,
(c) baixar um modelo por URL server-side, (d) re-treinar (fine-tune) a partir
de um `best.pt` existente, (e) ter a página "Modelos" real (hoje módulo Roadmap
desabilitado honesto — Sidebar L246-253). `GET /api/models` existe e é **derivado**
(`job_artifacts.kind='model'` de jobs `done`, `DISTINCT ON (engine, model)` —
ADR-0009 D2), mas a própria ADR-0009 profetizou: *"quando a fatia real de
modelos chegar (upload/volume/tabela), a rota troca a fonte, o contrato
permanece"*. Esta fatia executa a profecia: tabela `models` canônica + upload +
download por URL + fine-tune + página `/models` + dropdown de pesos em `/treino`.

Regras da casa aplicadas: wire camelCase em `/api/*`, transporte interno
snake_case (ADR-0002 D1); 404 `not_found` p/ id não-UUID em path (ADR-0002 D8);
erros `{code,message}` estáticos; PRs < ~400 linhas de produção; branch
`feat/models-real`; mock continua default (`ENGINE_MOCK=1`) — GPU real atrás
de `ENGINE_MOCK=0`; frontend SEMPRE via /impeccable com `docs/DESIGN.md` como
contrato, review por página.

## Decisões já travadas (base — citar, não redecidir)

- **`job_artifacts.kind='model'` identifica `best.pt`/`last.pt` dos jobs yolo
  `done`** (ADR-0007 D8; ADR-0008 D2) — é o que `GET /api/models` lista hoje
  (ADR-0009 D2; dedupe por `(engine, model)`, prefere `best`).
- **Postgres é a verdade relacional, o bucket é a verdade binária** (ADR-0003
  D1): bytes de imagem em `datasets/{id}/`, packages em `packages/<vid>/`,
  artefatos em `artifacts/<job_id>/`; leitura no browser por **presigned URL**
  (`S3_PUBLIC_ENDPOINT_URL`, TTL `S3_URL_TTL_SECS`; híbrido D3 — sem a env, o
  `url` vira `null` e o front usa proxy `/data`).
- **Manager dono de jobs/orchestrators/job_artifacts/dataset_versions; o
  orquestrador nunca toca Postgres; principal só fala com o manager via Bearer
  `MANAGER_TOKEN`** (ADR-0007 D3/D8; backend.md §1/:32). Migration 0006 mora em
  `services/api-principal/migrations/` (padrão da casa: as migrations dos dois
  donos vivem no mesmo diretório).
- **Manager não tem cliente S3** (ADR-0010 D3 — "manager não tem S3"; quem
  baixa do bucket é o orquestrador, com credencial escopada + invariante de
  prefixo no código). O principal TEM S3 interno + presign (storage/s3.rs).
- **Credencial do orquestrador = `heph-orchestrator` escopada a
  `packages/*`+`artifacts/*`** + 2ª barreira `scoped_key` no código (ADR-0007
  D2; spike F4.0 provou path-scope; **action-type NÃO é enforceado** no
  `-s3.config` legacy — residual aceito LOCAL-DEV).
- **O orquestrador baixa o package zip do bucket, substitui placeholders
  `{dataset_path}`/`{output_path}` no config.yaml e roda o trainer via
  `docker run` com volumes `(datasets→/datasets, outputs→/outputs)`**
  (ADR-0007 D4/D5/D6; `run_job_inner` L816-1135). O volume `models` do compose
  é montado no **orquestrador** e no **embedder** (cache do peso do CLIP ~600MB
  — compose L88-89/L132-135), NÃO no container do trainer.
- **`POST /api/jobs/yolo` gera o config.yaml no principal** (IDEIA §1/:14-16;
  `generate_config_yaml` L330-364) com `model` = variante (ALLOWED_MODELS L10)
  e placeholders literais; o trainer real faz `YOLO(yolo_cfg["model"])`
  (ultralytics baixa o peso pretrained se for nome de variante).
- **`queue_unavailable` (503) é o mapeamento padrão** das rotas que falam com o
  manager (backend.md :168); `storage_unavailable` (503) para bucket fora
  (ADR-0003 D10).
- **Storage medido = soma SQL por dono, NÃO ListObjects** (ADR-0009 D3):
  `datasetsBytes` no principal (`SUM(datasets.size_bytes)`), `artifactsBytes`
  no manager (`SUM(job_artifacts.bytes)`).
- **Spec OpenAPI atual: 0.10.0**; versão dos crates 0.1.0.
- **Frontend**: dashboard StatCard "Modelos & Pesos" consome `listModels()`
  (`apps/web/lib/monitoring.ts` L159-161; shape `ModelWeight` L89-97); módulo
  Sidebar "Modelos & Pesos" = desabilitado honesto (badge Roadmap, L246-253);
  `/treino` (ForjaYoloSetup) NÃO tem seletor de pesos; TrainYoloModal da galeria
  INTOCADO (file ownership); toda UI passa por /impeccable com DESIGN.md.

## Decisões

### D0 — Escopo v1: ciclo completo modelos (tabela+upload+download+fine-tune+UI); fora: delete/rotate, inferência, WS, multi-engine

**Decidido — entra na fatia (1 fatia backend I.1–I.4 + 2 fatias UI I.5/I.6):**
- **Migration 0007**: tabela `models` canônica + backfill dos `best.pt`
  existentes (D2).
- **Upload** `POST /api/models/upload` (multipart, D3) e **Download por URL**
  `POST /api/models/download` (server-side no principal, D4).
- **Fine-tune** `weights?: uuid` no `POST /api/jobs/yolo` (D5) — reuso do
  checkpoint como peso de partida.
- **`GET /api/models` troca a fonte** para a tabela (D2) — contrato preservado,
  shape cresce (D6).
- **Página `/models`** (Sidebar habilitada: lista GlassCard + modais Upload/
  Baixar por URL) e **dropdown de pesos em `/treino`** (D7).
- **`modelsBytes` no `GET /api/storage/usage`** (D8).
- **Spec 0.10.0 → 0.11.0** (D6); erro novo `model_download_failed` (502).

**Decidido — fora (dívida registrada, NÃO implementado agora):**
- **DELETE/rotate de modelos** — sem consumidor no v1 (sem rota de DELETE de
  job também; a tabela cresce, gestão é fatia futura; lição P1 ADR-0006: código
  sem consumidor não se escreve).
- **Playground/inferência com modelos** — módulo Playground é Roadmap; runners
  (backend.md §9/:161-162) continuam pendentes.
- **WS de progresso do download por URL** — o download v1 é síncrono (D4);
  WS é dívida existente (backend.md §9/:176).
- **Multi-engine além de yolo** — tabela com `CHECK (engine IN ('yolo'))`;
  difusão/CLIP entram com migration própria ampliando o CHECK (sem trigger de
  migração de dados — engine é coluna nova por row).
- **`GET /api/models/:id/data` (proxy de bytes)** — presigned URL cobre o
  download (mesma env do padrão 3b; sem `S3_PUBLIC_ENDPOINT_URL` → `url:null`
  e a UI desabilita o botão, sem rota proxy nova no v1).
- **Sniff de `.safetensors`/`.ckpt`** — difusão fora; o sniff v1 é só `.pt`
  (D3).
- **Dedupe por `(engine, model)` na listagem** — a tabela lista TODOS os
  checkpoints (D2; mudança de semântica do StatCard, documentada na sync).

**Descartado:** "mais uma rota derivada de artefatos" (o problema é a ausência
de catálogo — upload/download/fine-tune não têm onde morar); "fazer tudo numa
fatia única incluindo UI" (regra: backend primeiro, rebuild+recreate antes de
UI — F4.7; 1 página = 1 fatia = 1 review).

### D1 — Dono e boundary: tabela `models` é do MANAGER; bytes canônicos no S3 prefixo `models/` (volume docker fica só como cache de engine)

**Decidido (opção a):** a tabela `models` nasce em `services/api-principal/
migrations/0007_models.sql` mas é **dona do manager** (mesmo padrão da 0006:
jobs/orchestrators/job_artifacts/dataset_versions). Leitura: `GET
/internal/models` (rota existente — **troca a fonte** de derived SQL para
SELECT da tabela). Escrita: (a) **hook no `report_job`** do manager — job
`done` com artefato `kind='model'` e `path` contendo `best` → INSERT na tabela
(`source='train'`, idempotente via `UNIQUE(s3_key)` + `ON CONFLICT DO NOTHING`);
(b) **`POST /internal/models`** novo — cria row a partir do upload/download do
principal (o principal gera o UUID e o `s3_key`, PUTa o objeto e chama o
manager; falha do INSERT → compensação `delete` do objeto, padrão do package —
ADR-0007 D1). **Bytes canônicos no S3**: upload/download → `models/<engine>/
<id>/<name>` (novo prefixo); treino → `artifacts/<job_id>/<path>` (existente —
o hook NÃO copia bytes porque o manager não tem S3 — ADR-0010 D3; o artefato
continua sendo a fonte binária do checkpoint de treino). O **volume docker
`models`** (compose L88-89/L132-135) permanece **exclusivamente cache de engine**
(embedder CLIP); dados de usuário NUNCA vão para o volume.

*Por quê — manager dono:* o ciclo de vida dos pesos de treino nasce no report
do orquestrador (lado manager); a resolução de pesos para fine-tune (D5) lê a
tabela no `create_job` (lado manager) — dono único evita escrita cruzada
(principal escrever tabela do manager = violação do boundary "owner writes own
tables"). *Por quê — S3 e não volume:* (1) browser lê por presigned (mecanismo
provado 3b), sem rota proxy nova nem mount novo no principal; (2) orquestrador
REMOTO (TrueNAS, ADR-0010) tem volume próprio — S3 é o único lugar que os dois
nós enxergam; (3) storage/usage conta por SQL (ADR-0009 D3) — volume é invisível
ao Postgres; (4) doutrina ADR-0003 "bucket é a verdade binária". *Gotcha:* o
hook de treino deixa o checkpoint com `s3_key` em `artifacts/` — se um dia
existir DELETE de job, o modelo de treino morre junto com o artefato
(`job_id` FK `ON DELETE SET NULL`; mesma ressalva do R2 da ADR-0009 — documentar
na sync; sem rota de DELETE hoje). *Descartado:* (b) **volume `models/` como
home dos pesos de usuário** (não compartilhado com remoto, invisível ao
storage SQL, exige proxy novo no principal que não monta o volume hoje, mistura
cache descartável com dado durável); (c) **tabela dona do principal** (o hook
do manager escreveria tabela do principal = escrita cruzada, ou a lista viveria
como UNION derivada+tabela = duas fontes e o fine-tune precisaria de rota nova
de resolução no principal; dono único no manager fecha o ciclo com menos
superfície).

### D2 — Fonte de `GET /api/models`: tabela canônica (derived morre); migration COM backfill dos `best.pt` existentes

**Decidido:** `GET /api/models` (BFF → `GET /internal/models`) passa a ler a
**tabela `models`** (`SELECT ... FROM models ORDER BY created_at DESC`); o SQL
derivado `DISTINCT ON (j.engine, j.model)` **morre** (substituído, não
convivência). **Backfill SIM** na migration 0007: `INSERT ... SELECT` dos
artefatos `kind='model' AND path LIKE '%best%'` de jobs `done` (id = `ja.id` —
o wire `id` dos itens existentes permanece estável após a troca; `ON CONFLICT
(s3_key) DO NOTHING` = idempotente). Jobs futuros entram pelo hook do `report`
(D1). A dedupe por `(engine, model)` acaba: cada treino `done` produz **1 linha**
(o melhor checkpoint); 2 treinos do mesmo yolo11m = 2 modelos na lista.

*Por quê — backfill apesar de "banco local descartável":* (a) custo zero
(`INSERT..SELECT` numa migration); (b) sem ele, o StatCard do dashboard e a
página `/models` zerariam no merge e só voltariam a ter conteúdo no primeiro
treino novo — regressão visível num dado real existente (o `best.pt` de 5.4MB
da sessão GPU da ADR-0010); (c) a troca de fonte fica **invisível** para o
front (contrato preservado — profecia ADR-0009 D2 cumprida). *Gotcha:* só
`best.pt` vira modelo (`last.pt` continua artefato de job, baixável via
`/api/jobs/:id/artifacts`, mas não é "modelo"); a semântica do StatCard muda de
"modelos distintos treinados" para "checkpoints disponíveis" (documentar na
sync — a verificação F6.4.1 citava o dedupe; comportamento atualizado).
*Descartado:* (b) **sem backfill** (regressão visível do dashboard pós-merge);
(c) **UNION tabela + derived** (duas fontes, dedupe inconsistente, "canônico"
vira mentira; o derived morre limpo).

### D3 — Upload: `POST /api/models/upload` (multipart, padrão 3b; `.pt` + magic PK; teto 2 GiB; md5)

**Decidido:** multipart com **campo único `file`** + campos de form `engine`
(v1: `"yolo"` apenas — 400 `invalid_request` senão) e `name?` (opcional,
server-side sanitizado, ≤255 chars, deve terminar em `.pt` p/ yolo; ausente =
nome canônico do form). Padrão 3b (Fatia 3b): rota com
`DefaultBodyLimit::max(MODEL_UPLOAD_BODY_LIMIT_BYTES)` = **2 GiB + 8 MiB de
envelope** (excesso → 413 `invalid_request`); teto por arquivo
`MODEL_MAX_FILE_BYTES` = **2 GiB** (spool em tempdir + `put()` que streama do
disco — sem RAM; yolo real tem 5–40 MB, o teto folgado deixa difusão entrar sem
mudança de limite depois). Validação por engine (v1 yolo): extensão `.pt`
obrigatória + **magic bytes `PK\x03\x04`** (zip do `torch.save` — todo `.pt`
moderno é zip; 400 `invalid_request` se divergir). Hash: **md5** (hex 32,
padrão da casa — job_artifacts e package usam md5; CHECK no DDL). Fluxo:
valida → PUT `models/<engine>/<id>/<name>` (id = UUID gerado pelo principal) →
`POST /internal/models` (row) → compensação `delete` do objeto se o INSERT
falhar → **201 `Model`** (D6). *Por quê:* o padrão 3b é o único caminho de
upload provado da casa (multipart + sniff + limites + compensação); sniff
**enforced** porque upload de arquivo errado (ex. `.txt` renomeado) só falharia
no fine-tune — 400 imediato é mais honesto que `failed` tardio. *Gotcha:*
`.pt` antigos salvos com serialização não-zip (pickle puro) seriam recusados —
aceitável (torch ≥1.6 salva zip; documentar no sync). *Descartado:* campo
múltiplo `files` como datasets (modelo é artefato individual — 1 file = 1 row
= 1 card; resposta 201 simples em vez de array por item); sniff de
`.safetensors` (difusão fora do v1); sha256 em vez de md5 (house pattern é md5;
troca de hash seria ruptura de consistência com artefatos).

### D4 — Download por URL: server-side no PRINCIPAL (reqwest stream → PUT S3), síncrono, com SSRF fail-closed (allow-list via env)

**Decidido (opção a):** `POST /api/models/download` body
`{url, engine, name?}` (camelCase, `deny_unknown_fields`) → o **principal**
baixa: valida scheme `http|https` (400); **emenda E1 (aceite do usuário 2026-09-10, rejeição do SSRF mínimo): allow-list fail-closed via env `MODEL_DOWNLOAD_ALLOWED_HOSTS`** — lista separada por vírgulas de hosts autorizados. **Sem `*.` = host exato SOMENTE; subdomínios exigem entrada `*.<domínio>`** (ex.: `huggingface.co` aceita só `huggingface.co`; `*.huggingface.co` aceita `cdn-lfs.huggingface.co` e qualquer subdomínio). **Env ausente ou vazia ⇒ a rota responde 403 `model_download_disabled` (erro novo, D6) — a feature nasce DESLIGADA; nada é baixado por URL sem autorização explícita no `.env`** (`.env.example` ganha a linha comentada com exemplo no commit I.4a). Mantido sempre: deny de ranges privados/metadata (127/8, 10/8, 172.16/12, 192.168/16, ::1, fc00::/7, fe80::/10, 169.254/16 — 400) E **redirects (máx. 5) re-validados contra a allow-list + deny de privados a cada hop** (redirect não pode escapar da allow-list; DNS rebinding fica estrito: host autorizado que resolver IP privado = recusa); streama p/ tempdir com cap 2 GiB e timeout
(30s connect / 120s total); falha de rede/timeout/tamanho/hash → **502
`model_download_failed`** (erro novo, D6); sucesso → PUT `models/<engine>/<id>/
<name>` (nome = `name` do body ou basename sanitizado da URL, `.pt` obrigatório)
+ `POST /internal/models` + compensação → **201 `Model`** com `url` (a fonte
original vai para a coluna `models.url` — transporte interno, NÃO exposta no
wire v1). Síncrono (sem fila, sem WS — resposta em até ~2min; single-user).

*Por quê — baixar no principal:* o principal já tem reqwest + cliente S3 +
presign (storage/s3.rs); o orquestrador continuaria **stateless** (não ganha
endpoint de download nem volume novo); a visão do backend.md §9/:104
("manager roteia ao orquestrador, salva em `models/<engine>/` no volume, notifica
via WS") pressupõe WS + volume + rota interna de download — arquitetura de
futuro registrada como dívida, divergência consciente documentada na sync.
*Por quê — allow-list fail-closed e não deny-list sozinha:* o usuário rejeitou o mínimo (nega só ranges privados) — deny-list cobre o vetor óbvio da LAN mas deixa o principal baixando de QUALQUER host público; a allow-list torna a superfície de saída explícita e auditável, e nascer desligada (403 sem env) é a falha segura. Sufixo por domínio evita manter mirror por mirror (HF/Civitai/GitHub e mirrors). *Gotcha — DNS rebinding:* a re-validação a cada hop + resolução negando IP privado reduz a janela, mas a checagem resolve-antes-conecta-depois ainda admite rebinding; registrado como dívida (proxy de download dedicado com pin de IP resolvido se o produto sair da LAN). *Descartado:*
(b) **registrar-só-URL** (download preguiçoso no fine-tune: URL morta vira job
`failed` não-óbvio, SSRF aconteceria no dispatch — pior hora, e a "disponibilidade"
da lista seria mentira); (c) **rotear via manager→orquestrador** (backend.md
§9/:104: manager não tem S3 e orquestrador precisaria de endpoint novo +
progresso/WS + volume — peso morto para single-user; descartado por arquitetura,
não conveniência); (d) async 202 + polling (sem WS de progresso, polling de um
job de download seria fila nova sem consumidor — sincronia honesta com timeout
é suficiente no v1).

### D5 — Fine-tune: `weights?: uuid` no `POST /api/jobs/yolo`; resolução no manager; pesos stagados no volume outputs (zero mount novo); trainer `YOLO(path)`; mock intocado

**Decidido:** o body de `POST /api/jobs/yolo` ganha **`weights?: string`
(UUID de uma row de `models`)**. Fluxo:
1. **Principal** (`jobs/models.rs`): valida UUID (não-UUID → 400
   `invalid_request`); quando presente, `generate_config_yaml` emite a chave
   opcional `weights_path: "{weights_path}"` (placeholder literal, mesmo
   mecanismo dos existentes) — o `model` (variante) continua obrigatório e vai
   para `yolo.model` (metadados do job; com `weights`, a arquitetura vem do
   arquivo e o `model` é display-only); repassa `weights_id` no
   `POST /internal/jobs`.
2. **Manager** (`create_job`): resolve `SELECT s3_key, hash, engine FROM models
   WHERE id=$1` → inexistente → **404 `not_found`**; `engine != 'yolo'` → 400
   `invalid_request`; grava `params.weights_ref = {s3_key, md5}` (JSONB) — a
   resolução é no SUBMIT (fail-fast, sem fila gasta) e no lado dono da tabela.
3. **Dispatch** (`dispatch_next`): `dispatch_body` ganha `weights_ref`
   (contrato interno snake_case); `DispatchRequest` do orquestrador ganha
   `weights_ref: Option<WeightsRef{s3_key, md5}>`.
4. **Orquestrador** (`run_job_inner`, entre o unzip e o `docker run`): se
   `weights_ref` presente → `scoped_key` com escopo **inferido pelo prefixo**
   (`models/` → `S3Scope::Models` novo; `artifacts/` → `S3Scope::Artifacts`
   existente) → `get_to_file` → verifica md5 → **staging em
   `{workdir}/outputs/{job_id}/weights/<name>`** (o volume `outputs` JÁ é
   montado no trainer em `/outputs` — **zero mount novo**, zero env novo) →
   substitui `{weights_path}` no config.yaml por `/outputs/{job_id}/weights/
   <name>`.
5. **Trainer real** (`_real_train`, 1-2 linhas): `weights_path =
   cfg.get("weights_path")`; `model = YOLO(weights_path) if weights_path else
   YOLO(yolo_cfg["model"])` — `YOLO(caminho)` é a API canônica de fine-tune do
   ultralytics (mesma chamada já usada com variante). **Mock intocado**:
   `load_and_validate_config` valida required-keys e **tolera chaves extras**
   (L97-128); o mock ignora `weights_path` e segue determinístico.

*Por quê:* o fine-tune é o fecho do ciclo produto (reuso do `best.pt`); a
resolução no manager mantém o boundary (principal nunca lê tabela do manager);
staging no volume outputs evita o trap do volume `models` (que não é montado
no trainer) sem tocar compose; `weights_path` como chave separada (em vez de
reescrever `yolo.model`) preserva a semântica "model = variante de display" e
não exige condicional no principal. *Gotcha:* (a) peso de treino (`s3_key` em
`artifacts/`) usa escopo já existente; peso de upload/download (`models/`) usa
o escopo novo — os DOIS precisam estar na credencial (D1/I.1); (b) job **mock
com weights** baixa bytes reais do bucket e produz `best.pt` fake — desperdício
local aceito (é o cenário de teste E2E; documentar); (c) variante ≠ arquitetura
do arquivo → ultralytics falha no load → job `failed` honesto (não silencioso);
(d) fine-tune real @gpu é smoke MANUAL (fora do CI, padrão ADR-0010 G.6) — o
critério binário do smoke GPU valida `YOLO(path)` com pesos reais.
*Descartado:* `weights` como string livre (caminho/URL arbitrário do cliente —
SSRF/escopo fora do controle do manager; UUID referenciando a tabela é o único
ponteiro honesto); materializar pesos no volume `models` do orquestrador
(mount novo + env novo + sync de compose.gpu — staging em outputs é mais
simples e o peso é re-baixável do bucket); download do peso no `preparing`
via URL externa (rede externa no runtime — não-reprodutível; o peso vem do
bucket).

### D6 — Wire: `Model` público com `source`/`md5`/`url`/`model` nullable; `StorageUsage.modelsBytes`; erro novo `model_download_failed`; spec 0.10.0 → 0.11.0

**Decidido — shape público do `Model` (camelCase; campos existentes do
`ModelWeight` preservados, aditivo):**

```json
{ "id": "uuid", "name": "best.pt", "engine": "yolo", "model": "yolo11m" | null,
  "source": "train" | "upload" | "download", "bytes": 5429331,
  "md5": "d41d…", "url": "https://…presigned…" | null,
  "jobId": "uuid" | null, "createdAt": "2026-09-10T12:00:00Z" }
```

`url` = presigned `GET` do `s3_key` (TTL `S3_URL_TTL_SECS`, híbrido 3b — sem
`S3_PUBLIC_ENDPOINT_URL` → `null`); `jobId` = origem do treino (`null` p/
upload/download); `model` = variante conhecida (`null` p/ upload/download).
Interno (manager, snake_case): `{id, job_id, name, engine, model, s3_key,
source, hash, bytes, created_at}` — o BFF re-mapeia (padrão ADR-0002 D1).

**Erros e status exatos (novos na 0.11.0):**
```
GET  /api/models            200 401 503
POST /api/models/upload     201 400 401 413 503
POST /api/models/download   201 400 401 502 503
POST /api/jobs/yolo         (202 400 404 409 503) + 400 weights não-UUID + 404 weights inexistente
GET  /api/storage/usage     200 401 503    (shape + modelsBytes)
```
- Erro novo na enum: **`model_download_failed` (502)** — falha de rede/
  timeout/tamanho/hash no download por URL (é falha EXTERNA, não do manager —
  não mapeia para `queue_unavailable`).
- 413 → `invalid_request` (padrão 3b); 400 → `invalid_request` (engine≠yolo,
  extensão/magic/name, scheme/host negado, weights não-UUID); 404 →
  `not_found` (weights inexistente); 503 → `storage_unavailable`/`queue_
  unavailable`.
- `StorageUsage` ganha **`modelsBytes`** (aditivo); `totalBytes` =
  datasets + artifacts + models; **`artifactsBytes` passa a excluir
  `kind='model'`** (sem dupla contagem — D8).
- Rotas internas do manager (`POST /internal/models`, swap do
  `GET /internal/models`, `weights_ref` no dispatch) **não** entram na OpenAPI
  (fora do contrato público, padrão dos paths internos).

*Por quê:* contrato existente do `GET /api/models` é consumido pelo StatCard do
dashboard — shape aditivo preserva o front antigo (front novo contra backend
antigo vê `undefined` → UI trata como "—"); `source` é o que a página `/models`
precisa para o badge de origem honesto (treino/upload/URL); `model` nullable é
a verdade (upload não carrega variante). *Gotcha:* `name` no wire = basename do
`path` (como hoje — `best.pt`), NÃO o nome amigável; a coluna `models.url`
(fonte do download) é transporte interno e **não** vira `sourceUrl` no wire v1
(adicionar quando a UI precisar exibir a origem da URL).

### D7 — UI: página `/models` real (Sidebar habilitada) + dropdown de pesos em `/treino`; TrainYoloModal intocado

**Decidido — fatia I.5 (1 página = 1 fatia = 1 review, via /impeccable com
`docs/DESIGN.md`):**
- **Sidebar** (`Sidebar.tsx` L246-253): módulo "Modelos & Pesos" sai do Roadmap
  — `badge` removido, `isAvailable: true`, `href: /models` (padrão do módulo
  Orquestradores na Fatia H).
- **Página `/models`**: header "Modelos & Pesos" + CTAs "Upload" (modal com
  file picker + engine fixo yolo) e "Baixar por URL" (modal com `url`/`name`);
  lista de **GlassCards** (órfão adotado) com nome (mono), chip de engine,
  **badge de origem** (`source`: Treino/Upload/URL), bytes formatados, data,
  botão **Baixar** (usa `url` presigned; `url:null` → desabilitado com
  tooltip honesto); empty state ("Nenhum modelo ainda — treine um job YOLO ou
  importe pesos."); erros mapeados em pt-BR por `code` (padrão da casa).
  Consome `lib/models.ts` novo (uploadModel/downloadModel/listModels estendido)
  + `types/studio.ts` (Model).
- **`/treino` (fatia I.6)**: `ForjaYoloSetup` ganha seletor opcional **"Pesos
  iniciais (fine-tune)"** — `listModels()` filtrado por `engine==='yolo'`,
  rótulo `name · origem`; selecionado → envia `weights: id` no `startYoloJob`;
  a variante YOLO continua visível e obrigatória (metadados). **TrainYoloModal
  da galeria INTOCADO** (file ownership — segue sem pesos).

*Por quê:* o módulo Roadmap "Modelos & Pesos" é a promessa de UI que esta fatia
cumpre; badge de origem é a verdade honesta (treino vs importado — a ADR-0009
D2 proibia rotular "YOLOv11/Flux.1/SDXL" porque eram mocks; agora `source` é
real); dropdown em `/treino` é o consumidor do fine-tune (D5). *Gotcha:* o
dashboard StatCard continua consumindo `listModels()` sem mudança de código
(shape aditivo); a contagem muda de semântica (D2) — o subtexto do card pode
ganhar "checkpoints" na revisão de UI (decisão do /impeccable, dentro da página
dashboard NÃO entra nesta fatia — é outra página/fatia). *Descartado:* página
com tabela densa (GlassCards + badges é o contrato DESIGN.md); fazer upload/
download por URL direto na página sem modal (padrão da casa é modal —
CreateDatasetModal/ImportDatasetModal).

### D8 — Telemetria de storage: SIM — `modelsBytes` aditivo no `StorageUsage`; sem dupla contagem com `job_artifacts`

**Decidido:** `GET /api/storage/usage` ganha **`modelsBytes`**: no manager,
`artifactsBytes` = `SUM(job_artifacts.bytes WHERE kind != 'model')` e
`modelsBytes` = `SUM(models.bytes)`; `totalBytes` = datasets + artifacts +
models; `measured` permanece `true` quando 200 (soma SQL atômica — qualquer
fonte fora → 503, padrão ADR-0009 D3). O dashboard soma `totalBytes` (sem
mudança de código; o subtexto do card "imagens+vídeos ativos + artefatos de
jobs" ganha "+ modelos" na sync do front). *Por quê:* pesos são bytes reais e
devem contar; a exclusão de `kind='model'` de `artifactsBytes` evita a dupla
contagem (cada checkpoint de treino está nas DUAS tabelas — `job_artifacts`
(registro do job) e `models` (catálogo) — mas é o MESMO objeto binário).
*Gotcha:* o teste existente de storage do manager (soma de `job_artifacts`
incluindo best.pt de fixture) muda de expectativa — atualizado junto (I.2a);
`packages/*` órfãos continuam fora da conta (residual R3 da ADR-0009, dívida
existente de reconciliação). *Descartado:* não contar modelos (upload/download
sumiriam do card de storage — subnotificação); contar `models` e manter
`artifacts` integral (dupla contagem de todo checkpoint de treino).

## Migration — `0007_models.sql` (dono: manager; arquivo em `services/api-principal/migrations/`)

```sql
-- 0007_models.sql — tabela `models` (catálogo canônico de pesos; ADR-0012 D1/D2).
-- Dono: manager. Bytes: bucket (models/<engine>/<id>/<name> p/ upload/download;
-- artifacts/<job_id>/<path> p/ checkpoints de treino — o hook NÃO copia bytes).

CREATE TABLE models (
    id UUID PRIMARY KEY,
    engine TEXT NOT NULL CHECK (engine IN ('yolo')),        -- v1: só yolo; difusão amplia o CHECK em migration própria
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 255),
    model TEXT,                                             -- variante quando conhecida (treino); NULL p/ upload/download
    s3_key TEXT NOT NULL UNIQUE,                            -- 'models/<engine>/<id>/<name>' | 'artifacts/<job_id>/<path>'
    source TEXT NOT NULL CHECK (source IN ('train','upload','download')),
    url TEXT,                                               -- fonte original do download; NULL p/ upload/train (transporte interno)
    hash TEXT NOT NULL CHECK (hash ~ '^[0-9a-f]{32}$'),     -- md5 (padrão da casa: job_artifacts/package usam md5)
    bytes BIGINT NOT NULL CHECK (bytes >= 0),
    job_id UUID REFERENCES jobs(id) ON DELETE SET NULL,     -- origem do treino; upload/download NULL (R2 ADR-0009)
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX models_engine_idx ON models(engine);
CREATE INDEX models_created_at_idx ON models(created_at DESC);

-- Backfill (D2): checkpoints best.pt de treinos done existentes. Idempotente.
INSERT INTO models (id, engine, name, model, s3_key, source, hash, bytes, job_id, created_at)
SELECT ja.id, j.engine, split_part(ja.path, '/', -1), j.model,
       'artifacts/' || ja.job_id::text || '/' || ja.path, 'train',
       ja.md5, ja.bytes, ja.job_id, j.created_at
FROM job_artifacts ja
JOIN jobs j ON j.id = ja.job_id
WHERE ja.kind = 'model' AND j.status = 'done' AND ja.path LIKE '%best%'
ON CONFLICT (s3_key) DO NOTHING;
```

Invariantes: `s3_key UNIQUE` (barreira de idempotência do hook — ON CONFLICT);
CHECKs de `engine`/`source`/`hash`/`bytes` (simples, não-deferráveis — sem
invariante multi-tabela; o "contador" de storage é `SUM` na leitura, padrão
ADR-0009 D3 — sem trigger); `job_id` FK `ON DELETE SET NULL` (o modelo NÃO
morre com o job — o artefato binário em `artifacts/<job_id>/` morre, ressalva
documentada). Hook no `report_job` (código, ~15 linhas): job `done` com
artefato `kind='model' AND path LIKE '%best%'` → `INSERT ... ON CONFLICT
(s3_key) DO NOTHING` (id `gen_random_uuid()`, `engine`/`model`/`created_at` do
job, `hash`/`bytes` do artefato, `s3_key = 'artifacts/'||job_id||'/'||path`).

## Delta OpenAPI (0.11.0) — descrição na ADR

Rotas públicas (novas em negrito; existentes alteradas):
```
GET  /api/models                200 401 503              # fonte = tabela models (D2); shape Model (D6)
POST /api/models/upload         201 400 401 413 503       # multipart file+engine+name? → 201 Model
POST /api/models/download       201 400 401 502 503       # {url, engine, name?} → 201 Model
POST /api/jobs/yolo             202 400 404 409 503       # +weights?: uuid (400 não-UUID / 404 inexistente)
GET  /api/storage/usage         200 401 503               # +modelsBytes (aditivo)
```

Schemas:
- `ModelWeight` → **`Model`** (campos existentes preservados): `{id, name,
  engine, model: string|null, source: "train"|"upload"|"download", bytes,
  md5, url: string|null, jobId: string|null, createdAt}`.
- `StorageUsage` ganha `modelsBytes: integer` (aditivo; `totalBytes` =
  datasets + artifacts + models).
- `YoloJobRequest` ganha `weights: string|null` (UUID de `models`).
- Enum `Error.code` ganha **`model_download_failed`** (502).

**Erros novos: `model_download_failed` (502) e `model_download_disabled` (403 — download por URL com `MODEL_DOWNLOAD_ALLOWED_HOSTS` ausente/vazio).** (413 continua `invalid_request`.)

## Testes

| Camada | Infra | Prova |
|---|---|---|
| `cargo test -p manager` (+ `scripts/test-db.sh`) | Postgres do compose | **migration/backfill**: 2 jobs done com `best.pt`+`last.pt` → 2 rows `source='train'` (best apenas); job `failed` → 0; re-run da migration → sem duplicatas (ON CONFLICT); **hook**: report done com best.pt → 1 row; report 2× (idempotência) → 1 row; job sem `best` → 0; **list_models**: lê a tabela, `created_at DESC`, shape novo (s3_key/source/hash); **POST /internal/models**: 201 com id dado, 400 (engine/source/hash/bytes inválidos), 409 (s3_key duplicado); **create_job com weights_id**: 404 inexistente, 400 engine≠yolo, `params.weights_ref` gravado, dispatch_body com `weights_ref`; **storage**: `artifactsBytes` exclui `kind='model'`, `modelsBytes` soma, total certo (fixture antiga atualizada) |
| `cargo test -p orchestrator` | unit (sem S3) | `scoped_key` com `S3Scope::Models` (aceita `models/...`, recusa fora do prefixo, `..`/absoluto/vazio — extensão dos 8 testes); `DispatchRequest` com `weights_ref` (Some/None, serde); `run_job_inner` com weights_ref: escopo inferido por prefixo (models/→Models, artifacts/→Artifacts), download→staging em `outputs/<job_id>/weights/<name>`, md5 divergente → PipelineError, substituição de `{weights_path}` no config.yaml (presente só com weights_ref) |
| `cargo test -p api-principal` | MockStorage + pool lazy | **upload**: 201 shape camelCase, 400 (engine≠yolo, ext≠.pt, magic≠PK, name inválido), 413 envelope, compensação delete do objeto se INSERT falhar (MockManager fail); **download**: 400 (scheme não-http(s), host privado 127.0.0.1/10.x/192.168.x/169.254.169.254, name inválido), 502 `model_download_failed` (client falha/timeout), 201 com `url` presigned; **GET /api/models**: re-map camelCase, `model`/`jobId` nullable, `url` presigned ou null; **jobs/yolo**: weights não-UUID → 400, weights_id repassado no create_job; **storage**: modelsBytes; **contract**: spec 0.11.0 ≡ router (inventário de rotas + enum de erros) |
| `pytest engines/trainer-yolo` | unit (ultralytics mockado) | `_real_train` com `weights_path` → `YOLO(path)` chamado com o caminho (monkeypatch); sem weights_path → comportamento atual; **mock path intocado** (57 testes existentes verdes — chave extra tolerada pela validação de required-keys) |
| `scripts/test-db.sh` (extensão) | Postgres do compose | upload → list → job com weights → done → hook (2 rows) → storage totals (artifacts exclui models, models somado) ponta-a-ponta via manager |
| Smoke E2E (stack `ENGINE_MOCK=1`, `EXEC_MODE=docker`, Chrome) | compose completo | upload de `.pt` real pequeno → 201; `GET /api/models` ≥ 2 itens (backfill + upload) com `source` correto e `url` presigned baixável; job yolo com `weights` → `done` (mock, pipeline completa: download no bucket, staging, placeholder); `GET /api/storage/usage` com `modelsBytes` > 0 e `totalBytes` = soma; download por URL (servidor http local) → 201; UI Chrome: `/models` lista+badges+upload+download, `/treino` dropdown pesos, console limpo. **Fine-tune real @gpu = manual, fora do CI** (checklist README-gpu; critério binário: `YOLO(path)` carrega pesos reais e treina) |

O CI cobre as baterias locais (pytest + cargo dos 3 + test-db + contract); o
smoke @gpu continua manual (padrão ADR-0010).

## Spike obrigatório? — **NÃO**

As premissas externas são comportamento padrão ou já provado: (1) `YOLO(caminho
absoluto)` é a API canônica de fine-tune do ultralytics — a MESMA chamada que
`_real_train` já faz com variante (L300), só muda o argumento; (2) o path-scope
do SeaweedFS legacy foi provado no spike F4.0 com 2 prefixos — `models/*` é o
mesmo mecanismo (uma action por prefixo); (3) ranges privados de SSRF são tabela
fixa (RFC 1918 + metadata); (4) multipart+magic+compensação é o padrão 3b
provado. *Inverteria o desenho (aí sim vira spike):* se o path-scope `models/*`
fosse rejeitado pelo `-s3.config` legacy (não provável — F4.0 provou o mecanismo
com 2 prefixos) → fallback: `Read:heph-data/*` escopado no código pela 2ª
barreira `scoped_key` (piora o princípio do menor privilégio, mas mantém o
invariante no código); se o ultralytics 8.3.x recusasse caminho absoluto de
peso (não provável — é o uso documentado) → o trainer ajusta o argumento na
mesma linha `YOLO(...)`.

## Riscos e contingências

- **R1 — Dupla contagem de storage** (checkpoint está em `job_artifacts` E em
  `models`): mitigado por construção — `artifactsBytes` exclui `kind='model'`,
  `modelsBytes` soma a tabela (D8); teste db cobre o total exato.
- **R2 — Idempotência do hook de treino** (report `done` pode repetir):
  `UNIQUE(s3_key)` + `ON CONFLICT DO NOTHING`; teste de report 2× → 1 row.
- **R3 — SSRF / DNS rebinding no download por URL**: allow-list fail-closed via env `MODEL_DOWNLOAD_ALLOWED_HOSTS` + deny de ranges privados/metadata + re-validação de redirects a cada hop; risco residual: rebinding entre resolução e conexão — mitigado por re-validação de hop e deny de IP privado na resolução; feature nasce desligada (403 sem env); dívida: proxy dedicado com pin de IP resolvido se o produto sair da LAN.
- **R4 — Download de 2 GiB**: stream p/ tempdir + `put()` streama do disco —
  sem RAM; timeouts 30s/120s; 502 honesto em falha.
- **R5 — Checkpoint de treino morre se o job for deletado** (bytes em
  `artifacts/<job_id>/`, FK `ON DELETE SET NULL`): sem rota de DELETE de job em
  produto (R2 da ADR-0009); documentado; dívida de "promover artefato a modelo
  independente" (copy para `models/`) se DELETE de job surgir.
- **R6 — Mudança de semântica do StatCard** (dedupe morre; conta checkpoints):
  comportamento novo documentado na sync; a verificação F6.4.1 citava o dedupe —
  o teste de list_models do manager é atualizado (I.2a).
- **R7 — Contrato interno `DispatchRequest.weights_ref`** (manager→orquestrador):
  shape fixado NESTA ADR; serde ignora campo ausente (tolerância de
  forward-compat no sentido manager→orquestrador) — landing: manager antes do
  orquestrador no merge se houver intercalação; o smoke valida o par completo.
- **R8 — Job mock com `weights` baixa bytes reais** (desperdício local):
  aceito — é o cenário do E2E; documentado no sync.
- **R9 — test-db apaga `orchestrators`** (regra conhecida): restart do manager
  após qualquer test-db (re-adota no boot); lembrar no plano.
- **R10 — Bind-mount do `seaweedfs-s3.json` não detecta mudança** (lição F4.6
  bug #5): após I.1, `--force-recreate` do seaweedfs na verificação/smoke.

## O que fica falso nos docs (lista para o `@docs-sync`, commit I.10)

**Não aplicar agora** — docs descrevem o que existe, não o que foi aprovado.

- `backend.md` §9/:141-142 — `GET /api/models` "derivado" → **fonte trocada**
  (tabela `models`; derived morre — D2); `POST /api/models/upload` e
  `POST /api/models/download` → **IMPLEMENTADOS** (spec 0.11.0; upload multipart
  D3, download server-side no principal D4 — divergência consciente do §9/:104).
- `backend.md` §9/:104 — visão "manager roteia ao orquestrador, salva em
  `models/<engine>/` no volume, notifica via WS" → **substituída no v1**: o
  download é síncrono no principal, bytes no bucket (`models/`), WS permanece
  dívida; a visão do volume persiste só como cache de engine (embedder).
  `POST /api/models/download` real = síncrono no principal + allow-list fail-closed
  (`MODEL_DOWNLOAD_ALLOWED_HOSTS`), divergência consciente registrada.
- `backend.md` §9/:175 — `GET /api/storage/usage` → `StorageUsage` ganha
  `modelsBytes`; `artifactsBytes` exclui `kind='model'` (D8).
- `backend.md` §10/:298-302 — tabela `models` **MIGRADA** (0007) com colunas
  reais: `s3_key` (não `path`), `source ∈ train|upload|download` (não
  `hf|civitai|upload`), `hash` md5, `job_id`; `url` = fonte do download
  (transporte interno, não wire); nota "NÃO migrada" → implementada; a tabela é
  dona do **manager** (a nota antiga não dizia dono).
- `backend.md` §10/:314-319 — `job_artifacts` nota: `kind='model'` continua
  existindo (registro do job) mas o catálogo canônico de modelos agora é a
  tabela `models` (hook registra `best.pt` por job `done`).
- `frontend.md` §10/:249 — `ModelWeight` → `Model` (ganha `source`/`md5`/`url`;
  `model`/`jobId` nullable).
- `frontend.md` §10/:250 — `StorageUsage` ganha `modelsBytes`.
- `frontend.md` §10/:253 — upload/download **pendentes** → **implementados**;
  página `/models` habilitada (I.5); `/treino` ganha seletor de pesos (I.6).
- `frontend.md` §5.1 / dashboard — StatCard "Modelos & Pesos" conta
  **checkpoints** (não modelos distintos — D2/R6); subtexto do storage ganha
  "+ modelos".
- `frontend.md` Sidebar — módulo "Modelos & Pesos" habilitado (badge Roadmap
  sai; L246-253 do código).
- `docs/adr/0009-web-integracao-monitoramento.md` D2 — nota "a rota troca a
  fonte, o contrato permanece" → **CUMPRIDA** (fonte = tabela, contrato
  preservado com shape aditivo); R2 (CASCADE do job) → emenda: `models.job_id`
  é `SET NULL`, o binário de treino permanece em `artifacts/` (dívida R5).
- `dividas.md` — novas: gestão de modelos (DELETE/rotate/promote de artefato a
  modelo independente — R5); playground/inferência com modelos; WS de progresso
  de download; proxy de download dedicado com allow-list (SSRF fora da LAN —
  R3); sniff `.safetensors`/multi-engine (D0); `GET /api/models/:id/data` como
  fallback sem `S3_PUBLIC_ENDPOINT_URL` (D0). Reafirmadas: reconciliação
  bucket×banco, test-db isolamento.
- `coordenacao.md` — bloco da fatia I reescrito a cada commit.

## Plano de commits (I.0–I.10; branch `feat/models-real` de `main`)

Um dispatch = um commit; produção < 400 linhas por commit (contrato/testes fora
da conta — exceção da casa). **Fase 1: I.1 ∥ I.2a ∥ I.3** (ownership disjunto:
infra vs manager vs orchestrator); **Fase 2: I.2b** (depende da tabela de I.2a;
mesmo arquivo do manager); **Fase 3: I.4a → I.4b** (sequenciais no principal);
**Fase 4: rebuild+recreate (regra F4.7) → I.5 → I.6** (frontend via
/impeccable); **Fase 5: I.7/I.8 review → I.9 smoke E2E → I.10 docs-sync**.

| # | Dono | Conteúdo | Critério de pronto |
|---|---|---|---|
| **I.0** | @architect | Esta ADR (proposta; vira executável após aceite do usuário) | auditoria do coordenador; arquivo commitado em `main` |
| **I.1** | @infra-dev | `infra/seaweedfs-s3.json`: identidade `heph-orchestrator` ganha `Read:heph-data/models/*` + `List:heph-data/models/*` (sem Write — o orquestrador só lê pesos) | `docker compose -f infra/compose.yaml config -q`; ~3 linhas; nota no README: `--force-recreate` do seaweedfs na smoke (R10) |
| **I.2a** | @rust-dev (manager) | migration `0007_models.sql` (tabela + backfill) + hook no `report_job` (best.pt → INSERT ON CONFLICT) + `list_models` lendo a tabela (troca da derived SQL; `ModelItem` novo com s3_key/source/hash) + storage: `artifactsBytes` exclui `kind='model'`, soma `models.bytes`; testes db (backfill, hook idempotente, list, usage) | `cargo test -p manager -- --ignored` + `bash scripts/test-db.sh` verdes (**restart do manager depois** — R9); fmt limpo; ~300-380 linhas |
| **I.2b** | @rust-dev (manager) | `POST /internal/models` (create com id/s3_key do principal; 201/400/409) + `CreateJobRequest.weights_id` (resolve `models` no create_job: 404 inexistente, 400 engine≠yolo; grava `params.weights_ref`) + `dispatch_body.weights_ref`; testes | `cargo test -p manager -- --ignored` + test-db verdes; fmt; ~150-220 linhas |
| **I.3** | @rust-dev (orchestrator) | `S3Scope::Models` (`prefix() = "models/"`) + testes de recusa do `scoped_key` estendidos + `DispatchRequest.weights_ref: Option<WeightsRef{s3_key, md5}>` + `run_job_inner`: escopo por prefixo, `get_to_file` → staging `outputs/<job_id>/weights/<name>`, verificação md5, substituição de `{weights_path}` no config.yaml (só quando presente) | `cargo test -p orchestrator` verde (novos: scoped_key Models, staging, md5 mismatch, substituição condicional); fmt; ~200-280 linhas |
| **I.4a** | @rust-dev (principal) | `POST /api/models/upload` (multipart `file`+`engine`+`name?`, `DefaultBodyLimit` 2 GiB+8 MiB, magic `PK\x03\x04`, sanitização de nome) + `POST /api/models/download` (reqwest stream, deny-list SSRF, cap 2 GiB, timeouts, 502 `model_download_failed`) + `ManagerPort::create_model`/`HttpManager`/`MockManager` + `POST /internal/models` client + `PROTECTED_ROUTES` | `cargo test -p api-principal` verde (units de validação/compensação/502); fmt; ~300-380 linhas |
| **I.4b** | @rust-dev (principal) | `GET /api/models` re-map para a tabela (`Model` com source/md5/url/model nullable; `InternalModel` novo) + `StorageUsage.modelsBytes` + `YoloJobRequest.weights` (UUID, `weights_path` no config.yaml) + spec **0.11.0** (rotas/schemas/`model_download_failed`) + contract | `cargo test -p api-principal` verde + contract 0.11.0 ≡ router; fmt; ~250-350 linhas |
| **I.5** | @frontend-dev (via /impeccable) | Página `/models`: Sidebar habilitada (badge Roadmap sai), lista GlassCard (nome/engine/badge source/bytes/data/Baixar), modais Upload e Baixar por URL, empty state, `lib/models.ts` + `types/studio.ts` (Model), erros pt-BR | `npm run build --workspace=web` verde; review por página (1 página = 1 fatia = 1 review); DESIGN.md como contrato |
| **I.6** | @frontend-dev (via /impeccable) | `/treino` (`ForjaYoloSetup`): seletor "Pesos iniciais (fine-tune)" (listModels engine=yolo; envia `weights`) — TrainYoloModal INTOCADO | `npm run build --workspace=web` verde; review por página |
| **I.7** | @reviewer | review do diff I.1–I.4 vs esta ADR (2 partes: manager+orchestrator / principal+infra; pontos: hook idempotente, escopo `models/*` + scoped_key, compensação de upload, SSRF, weights_ref no dispatch, storage sem dupla contagem) | APROVA (com ou sem nits); fixes roteados como commits próprios |
| **I.8** | @reviewer/@ui-designer | review das páginas I.5/I.6 (pontos: badges de origem honestos, url null → Baixar desabilitado, empty state, dropdown de pesos em /treino) | APROVA; fixes roteados |
| **I.9** | @coordenador (smoke E2E, fora do CI) | stack mock: upload `.pt` → 201; `GET /api/models` ≥ 2 itens (backfill+upload, source/url corretos, presigned baixável); job yolo com `weights` → done (pipeline completa); storage com modelsBytes e total exato; download por URL (servidor http local) → 201; Chrome: `/models` (lista/modais/download) + `/treino` (dropdown) + console limpo. **Fine-tune real @gpu**: manual opcional (checklist README-gpu; critério: `YOLO(path)` carrega pesos reais) | critérios binários da tabela de Testes; teardown limpo (banco/objetos de smoke); `--force-recreate` do seaweedfs (R10) |
| **I.10** | @docs-sync | Aplica "O que fica falso nos docs" (backend.md §9/§10, frontend.md §10/§5.1/Sidebar, emenda ADR-0009 D2/R2, dividas.md, coordenacao.md) | diff só de docs; conferência doc↔código nos dois sentidos |

**Notas de processo:** mock NUNCA quebra (baterias existentes = critério: pytest
57 do trainer, contract 0.11.0); I.3 e I.2b tocam o mesmo contrato interno
(`DispatchRequest.weights_ref`) — shape fixado aqui, landing do manager antes
do orchestrator se intercalar (R7); `cargo fmt --all` antes de reportar;
implementador que achar problema FORA do escopo **para e reporta ao
coordenador** (norma das fatias 4/5); test-db.sh exige restart do manager (R9);
UI só depois do backend vivo e rebuildado (F4.7).
