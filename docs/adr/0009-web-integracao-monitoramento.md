# ADR-0009 — Integração Web/Banco: endpoints de monitoramento honestos para o /dashboard (F6.0/F6.1)

- **Status:** **PROPOSTA** (aguarda aceite do usuário). Nada implementado. Este
  documento é a especificação executável da parte **backend** da fase F6
  (integração web/backend); os deltas de contrato abaixo são aplicados **apenas
  nos commits da fase** (openapi junto do código, docs de texto no `docs-sync`
  do fim), nunca antes. Escopo da ADR: F6.0 (este desenho) + **F6.1** (backend).
  F6.2 (dashboard) e F6.3 (IA /treino) são fatias de frontend posteriores —
  referenciadas, não especificadas aqui.
- **Data:** 2026-09-09
- **Componentes:** `services/orchestrator` (heartbeat ganha `ram_total`),
  `services/manager` (cache de telemetria + **3 rotas internas novas de
  leitura**), `services/api-principal` (BFF: 3 rotas públicas + `ramTotal` no
  wire + `version` no `/health`), `packages/contracts` (spec 0.8.0 → **0.9.0**),
  `apps/web` (F6.2 — dashboard consome; só referência).
- **Fontes:** `IDEIA.md` §1/:15 (UI é o ponto único de comunicação) e :54
  (principal estrutura e repassa ao orquestrador — versões de software honestas
  vêm de quem roda, não de string fixa na UI); `docs/backend.md` §9/:137
  (`models` planejado: GET /api/models + upload/download), :158-161
  (orchestrators via manager + alias `/api/environments*`), :162 (telemetria
  wire sem `ramTotal`), :168 (`queue_unavailable` generalizado nas rotas que
  falam com o manager), :173 (nota `measured:false` = caminho morto — **ver
  R5**: o código diverge desta nota), §10/:277-283 (`orchestrators`/
  `models` no schema), :308 (contador `size_bytes` por trigger
  `heph_refresh_dataset_counters` — `SUM(datasets.size_bytes)` é grátis);
  `docs/frontend.md` §10/:224 (Ambientes — rota fantasma), :244 (Models —
  rota fantasma), :242 (telemetria real sem `ramTotal`); `docs/adr/0007-
  jobs-v1.md` D3 (auto-adoção `orchestrator-local`, Bearer `MANAGER_TOKEN`),
  D8 (`job_artifacts.kind='model'` p/ `best.pt`/`last.pt`), R6 (packages
  órfãos); `docs/adr/0008-autotracker-v1.md` D2 (yolo → artefatos `[best.pt
  model, last.pt model, metrics.jsonl metrics]`). Código (verificado por grep/
  graft nesta data): `services/manager/src/lib.rs` (`receive_heartbeat`
  L800-825 — cache **global** single-node; `get_telemetry` L828-863 —
  `measured:true` = heartbeat ≤ 10s; `adopt_orchestrator` L888-900 —
  INSERT `('orchestrator-local','http://orchestrator-local:8082','local',
  'online')`, nunca grava `gpus`/`vram_total_gb`), `services/manager/src/
  main.rs` (`build_router` L325-349 — 7 paths `/internal/*` (8 métodos; POST+GET em `/internal/jobs`), **sem** listagem de
  orquestradores; `heartbeat_handler` L293-313; `telemetry_handler` L316-319),
  `services/orchestrator/src/main.rs` L355-375 (heartbeat ~2s com `gpus:
  vec![]` fixo no mock), `services/orchestrator/src/lib.rs` (`HeartbeatBody`
  L58-65 — sem `ram_total`; `read_ram` L1011-1040 — lê `MemTotal` e
  `MemAvailable` mas só devolve usada), `services/api-principal/src/auth/
  routes.rs` (`PROTECTED_ROUTES` L28-113 / `PUBLIC_ROUTES` L116-122 / `health`
  L145-152 — devolve `{status,service,auth}`, **sem version**), `src/jobs/
  handlers.rs` (`get_telemetry` L458-474 — todo erro do manager vira 503
  `queue_unavailable`), `src/jobs/manager_client.rs` (`InternalTelemetry`
  L77-85, `ManagerPort` L103-129, `MockManager` L331+), `src/auth/handlers.rs`
  (`MeResponse` L78-99 — só `{userId, loggedAt}`), `packages/contracts/
  openapi.yaml` (version 0.8.0), dashboard `apps/web/app/(studio)/dashboard/
  page.tsx` L109-194/L266-277/L334-350/L582-588, Sidebar
  `apps/web/components/studio/Sidebar.tsx` L336/L386-388/L600-604/L643-683,
  `infra/compose.yaml` (volume `models` montado no orquestrador e no embedder).
- **Sequência:** 5 (mergeada) → **F6** (esta: F6.0 ADR → F6.1 backend →
  F6.2 dashboard → F6.3 IA → F6.4 docs-sync/review) → dívidas registradas.

## Contexto

O `/dashboard` é o epicentro dos mockups (~25 valores fixos, inventariados em
`docs/coordenacao.md`): `nodes[]` hardcoded com um nó RunPod inexistente e uma
"RTX 4090" com pct 82.4/8.5 fake, StatCards "Modelos & Pesos = 14" e
"Storage = 34.8 GB" sem chamada nenhuma, fallbacks de telemetria (CPU 5.4, RAM
13.2/62.8 GB, VRAM 4.2/24.0), strings de identidade/versão fixas na Sidebar
("Hephaestus Admin"/"admin@localhost"/"v1.3.0"/"CUDA 12.4") e 2 botões "⋯"
decorativos. O backend tem `GET /api/telemetry` real (proxy do cache do
manager) e a tabela `orchestrators` populada por auto-adoção, mas **não** tem
rota de listagem de orquestradores, nem `GET /api/models`, nem
`GET /api/storage/usage` — todas só planejadas no backend.md §9. O usuário
fechou (não reabrir): **fazer no backend tudo que o front precisa**; só o nó
RunPod some; disposição do dashboard mantida; 8 módulos Roadmap da Sidebar
continuam fora.

Regras da casa aplicadas: wire camelCase em `/api/*`, transporte interno
snake_case (ADR-0002 D1); 404 `not_found` p/ id não-UUID (ADR-0002 D8);
erros `{code,message}` com `message` estática; dev CPU-only (`ENGINE_MOCK=1`)
— **nenhum número inventado** quando não há medição (`measured:false`,
`vramUsed/Tot:null`, `gpus:[]` são a verdade); PRs < ~400 linhas de produção;
branch `feat/integracao-web`.

## Decisões já travadas (base — citar, não redecidir)

- **Q1/Q2/Q3/Q4 (2026-09-09):** `/jobs` vira "Execuções" + rota `/treino`
  (IA, sem backend — F6.3); backend faz tudo que o front precisa para o
  dashboard honesto; **RunPod some**; disposição mantida; 8 módulos Roadmap
  desabilitados honestos (sem backend neles); órfãos de UI = ADOTAR (F6.2).
- **`GET /api/telemetry` e o cache do manager:** o manager cacheia telemetria
  do heartbeat do orquestrador **globalmente** (não por orquestrador) —
  `services/manager/src/lib.rs:800-825` grava `state.{measured,vram_used,
  vram_total,cpu,ram,gpus,jobs_active,last_heartbeat}`. `measured:true` =
  heartbeat recebido nos últimos 10s (`get_telemetry` L832-843); com GPU
  ausente o mock envia `gpus:[]` e `vram_*:null` **com `measured:true`**
  (CPU/RAM reais do `/proc` do orquestrador — docs/backend.md :173 descreve o
  contrário; corrigir na sync — R5).
- **`orchestrators` (0006) é tabela do manager; o orquestrador nunca toca
  Postgres; principal só fala com manager via Bearer `MANAGER_TOKEN`; dono de
  `job_artifacts` é o manager; dono de `datasets`/`images`/`videos` é o
  principal (backend.md §1/:32; ADR-0007 D3/D8).
- **`queue_unavailable` (503) é o mapeamento padrão** das rotas de leitura que
  falam com o manager quando ele está inalcançável (backend.md :168 — Nota
  Fatia 4; `get_telemetry` mapeia **qualquer** erro do manager para 503).
- **`job_artifacts.kind='model'`** identifica `best.pt`/`last.pt` dos jobs
  yolo `done` (ADR-0007 D8; ADR-0008 D2) — **é o único "peso" que o sistema
  produz hoje** (mock de 110 bytes "HEPHMOCK"; nenhum peso real existe em
  volume `models/`, que está vazio).
- **`datasets.size_bytes` é mantido por trigger** (`heph_refresh_dataset_
  counters`, migration 0003/0005): soma `images` ativas (`deleted_at IS
  NULL`) + `videos` por dataset. `SUM(datasets.size_bytes)` = soma canônica
  de mídia ativa sem SQL novo sobre `images`.
- **`GET /api/auth/me`** (público, isento do gate) devolve só
  `{userId, loggedAt}` — não há nome/email/role no sistema (tabela `users` =
  `id` + `password_hash`); **"admin@localhost"/"Hephaestus Admin"/"Root" não
  têm fonte e não terão**.
- **Espec OpenAPI atual: 0.8.0** (`packages/contracts/openapi.yaml:4`);
  versão dos crates dos 3 serviços Rust: 0.1.0. Não existe versão "v1.3.0"
  em lugar nenhum do código — é invenção da UI.

## Decisões

### D0 — Escopo v1 (F6.1): só o que o /dashboard precisa; o resto registrado como dívida

**Decidido — entra no F6.1 (backend):**
- **`GET /api/orchestrators`** — lista real (1 nó: `orchestrator-local`).
- **`GET /api/models`** — pesos derivados de `job_artifacts.kind='model'`.
- **`GET /api/storage/usage`** — bytes reais por soma SQL dos contadores.
- **Telemetria estendida:** `ramTotal` no wire (orquestrador heartbeat →
  manager cache → `GET /api/telemetry`). Sem `gpuName` novo — o front usa
  `gpus[]` direto (preferência D4).
- **`version` no `/health` público** — fonte única da versão de produto.

**Decidido — fora (registrado como dívida, NÃO implementado agora):**
- **POST adopt/rotate/revoke/enable/disable/remove** e
  `GET /api/orchestrators/:id/health` — o nó remoto (RunPod) morreu; não há
  ação de gestão de orquestrador com consumidor no v1 (lição P1 da ADR-0006:
  código sem consumidor não se escreve).
- **Alias `/api/environments*`** — é o alias do módulo Roadmap "Ambientes"
  (desabilitado honesto); vira rota junto com a fatia do módulo.
- **`POST /api/models/upload` e `POST /api/models/download`** — upload real
  (tabela `models` §10, volume `models/`, download HF/Civitai) é a fatia do
  módulo Roadmap "Modelos & Pesos" + treino com peso real (ADR-0008 D6).
- **ListObjects no bucket** para medir storage — rejeitado em D3.
- **Nova rota `/api/version`** — `/health` já existe e é pública (D5).
- **Backend novo para identidade** — `/api/auth/me` basta (D6).
- **WebSocket, módulos Roadmap (difusao/openclip/playground/models-page/
  environments-page/storage-page/events/settings), samples** — intactos.

**Descartado:** entregar "mais uma rota fake" (o problema é o mockup); fazer
F6.2 junto com F6.1 (1 fatia = 1 boundary; backend primeiro — ordem do
usuário).

### D1 — `GET /api/orchestrators`: lista real da tabela do manager, com telemetria por nó (Fatia H)

**Decidido (implementado na Fatia H, ADR-0011 D2):** rota interna **`GET /internal/orchestrators`** no manager
(leitura da tabela `orchestrators`, dono = manager) + BFF
**`GET /api/orchestrators`** no principal (padrão `ManagerPort`/`HttpManager`/
`MockManager`, Bearer `MANAGER_TOKEN`). Item (camelCase no `/api/*`) agora
**enriquecido com telemetria por nó** (ADR-0011 D2):

```json
{ "id": "uuid", "name": "orchestrator-local", "kind": "local",
  "endpoint": "http://orchestrator-local:8082",
  "status": "online", "lastHeartbeat": "2026-09-09T12:00:00Z",
  "measured": true, "cpu": 0.5, "ram": 1234567, "ramTotal": 33563316224,
  "vramUsed": 0, "vramTotal": 0, "vramTotalGb": null,
  "gpus": [], "jobsActive": 0 }
```

*Por quê — telemetria por nó:* a Fatia H (ADR-0011 D1/D2) implementou heartbeat
com identidade (`endpoint` no `HeartbeatBody`) + cache por nó (`HashMap<Uuid,
TelemetryState>`). Cada item da lista agora carrega a telemetria do próprio nó.
`measured` por nó = heartbeat ≤ 10s; sem heartbeat → `measured:false` + campos
`null` (nunca número inventado — R5). *Gotcha:* o `status` agora sai de
`online` (watchdog 15s/60s — ADR-0011 D4); o card UI mostra status + "visto há
Xs". *Descartado:* manter lista sem métricas + regra `items.length===1` (morreu
no H.5 — ADR-0011 D7).

### D2 — `GET /api/models`: pesos derivados de `job_artifacts.kind='model'` (zero infra nova)

**Decidido (auditando a sugestão c do usuário — adotada):** o "Modelos &
Pesos" do v1 é a lista dos **checkpoints produzidos por treinos `done`**:
último artefato `kind='model'` por par `(job.engine, job.model)`, via rota
interna nova **`GET /internal/models`** no manager (dono de `job_artifacts`/
`jobs`) + BFF **`GET /api/models`** no principal. SQL (manager):

```sql
SELECT DISTINCT ON (j.engine, j.model)
       ja.id, ja.job_id, ja.path, ja.bytes, j.engine, j.model, j.created_at
FROM job_artifacts ja JOIN jobs j ON j.id = ja.job_id
WHERE ja.kind = 'model' AND j.status = 'done'
ORDER BY j.engine, j.model,
         (ja.path LIKE '%best%') DESC,   -- prefere best.pt a last.pt
         j.created_at DESC               -- mais recente por modelo treinado
```

Item (wire camelCase; `name` = basename do `path`; `jobId` carrega o download
futuro via rota de artefato existente):

```json
{ "id": "uuid-do-artefato", "name": "best.pt", "engine": "yolo",
  "model": "yolo11m", "jobId": "uuid", "bytes": 110, "createdAt": "…Z" }
```

*Por quê:* é o **único peso que o sistema produz hoje** (mock `best.pt`/
`last.pt` de jobs yolo; o volume `models/` do orquestrador está vazio e nada
o escreve). Tabela `models` (opção a) seria schema sem escrita — a doutrina
da ADR-0007 D1 ("tabelas mortas não migram") e zero infra nova (opção c).
Endpoint no orquestrador listando o volume (opção b) devolveria sempre `[]` e
criaria superfície de leitura num serviço stateless sem ganho. O shape
`{name, engine, model, bytes, createdAt}` já serve o dropdown de treino
futuro (lista de checkpoints por engine) — quando a fatia real de modelos
chegar (upload/volume/tabela), **a rota troca a fonte, o contrato permanece**.
*Gotcha:* são pesos **mock** (110 bytes "HEPHMOCK") — o card não pode rotular
"YOLOv11/Flux.1/SDXL"; a contagem é por modelo treinado distinto
(DISTINCT ON), não por artefato (2 treinos do mesmo yolo11m = **1** linha, o
checkpoint mais novo). *Dívida registrada:* models real (tabela §10 + upload/
download + volume) = fatia do módulo Roadmap "Modelos & Pesos".

### D3 — `GET /api/storage/usage`: soma SQL por dono (verdade relacional), NÃO ListObjects

**Decidido (opção a, refinada pelo boundary):** o BFF calcula por dono —
`datasetsBytes` no **principal** (`SELECT COALESCE(SUM(size_bytes),0) FROM
datasets` — contador mantido por trigger, custo zero) e `artifactsBytes` no
**manager** via rota interna nova **`GET /internal/storage/usage`**
(`SELECT COALESCE(SUM(bytes),0) FROM job_artifacts`). Resposta do BFF
(camelCase):

```json
{ "datasetsBytes": 0, "artifactsBytes": 0, "totalBytes": 0, "measured": true }
```

*Por quê — SQL em vez de ListObjectsV2:* (1) o `StoragePort` **não tem** método
de listagem (port.rs: 7 métodos — put/get/get_to_file/presign_get/delete/
copy_object/delete_prefix) — ListObjects exigiria método novo + implementação
S3 paginada + `MockStorage` novo, para o dashboard **polling 3s** varrer o
bucket inteiro a cada tick; (2) a doutrina §11 é "Postgres é a verdade
relacional, bucket é a verdade binária" — o SQL diz o que o **app** rastreia;
o List diria o que sobrou no bucket, incluindo `packages/*` órfãos de jobs
(ADR-0007 R6) e mídia da lixeira ainda não purgada — número maior e
**enganoso** para "uso do app"; (3) sem credencial nova (o método `delete_prefix`
do principal já lista o bucket no sweep — capacidade existe, só não é o dado
certo). *Gotcha honesto:* `size_bytes` exclui imagens soft-deletadas (lixeira)
até o purge; o card não cobre `packages/*` nem objetos órfãos — o subtexto
deve dizer o que mede ("imagens+vídeos ativos + artefatos de jobs"). Onde
está a coluna de tamanho: `images.bytes` (0003) e `videos.bytes` (0003)
alimentam `datasets.size_bytes` via trigger; `job_artifacts.bytes` (0006)
existe e é o wire `{...bytes}` já conhecido. *Descartado:* híbrido SQL+List
(v1 sem consumidor para a precisão do bucket; reconciliação bucket×banco é
dívida futura), contar só datasets sem artefatos (o card perderia os pesos
gerados por treinos, que D2 lista).

### D4 — Telemetria: `ramTotal` no wire; GPU sem mudança de contrato (`gpus[]` + gauges derivados)

**Decidido:** adicionar **`ramTotal`** (aditivo, `Option<i64>` = bytes) nas 5
camadas: (1) orquestrador — `read_ram_total()` (parse de `MemTotal`, espelho
do `read_ram` L1011-1040, que **já lê MemTotal mas não devolve**) +
`HeartbeatBody.ram_total` + preenchimento no loop do heartbeat; (2) manager —
cache `state.ram_total` + `TelemetryResponse` do `/internal/telemetry`;
(3) principal — `InternalTelemetry`/`TelemetryResponse` + handler
`get_telemetry`; (4) spec 0.9.0; (5) front `Telemetry.ramTotal: number | null`
(F6.2 mata o `realRamTotalGb = "62.8"` fixo do dashboard L116).

**Decidido — NÃO criar `gpuName`/contrato novo para GPU:** o front usa
`gpus[]` direto (`gpus[0]` quando houver) e o gauge de VRAM deriva de
`vramUsed/vramTotal` — ambos **já existem no wire**. `measured:false` sem
GPU significa: heartbeat ausente → CPU/RAM `null`; heartbeat com `gpus:[]` →
`measured:true` com CPU/RAM reais e VRAM `null` (semântica real do código —
R5). Regra de UI (F6.2): `vramTotal == null || gpus.length === 0` ⇒ "sem GPU
(mock)", nunca número. *Gotcha de compat:* campo **aditivo e opcional** — front
antigo ignora `ramTotal`; front novo contra backend antigo recebe `undefined`
⇒ UI mostra "—" (nenhum fallback inventado). Unidades: bytes (mesma do `ram`).

### D5 — Versão de produto: campo `version` no `/health` público (sem endpoint novo)

**Decidido:** `GET /health` (público, isento do gate) passa a devolver
`{"status":"ok","service":"api-principal","auth":…,"version":"0.1.0"}`, onde
`version = env!("CARGO_PKG_VERSION")` do crate do principal — a única versão
de software **real** do sistema (os 3 crates são 0.1.0; web é 0.1.0; não
existe "v1.3.0" em lugar nenhum). O dashboard e a Sidebar (F6.2) consomem
`/health` (rota pública; dá para chamar de qualquer página autenticada).
*Por quê:* evitar endpoint novo (`/api/version`) para um dado que muda por
release, não por sessão; `/health` já está no contrato público e na lista
`PUBLIC_ROUTES`; as strings "Rust Core + PyTorch 2.6 CUDA 12.4"/"CUDA 12.4"/
"PyTorch CUDA" da UI **não têm fonte e morrem** (F6.2 as substitui por dados
reais de telemetria — `gpus[]`/`ramTotal` — ou some). *Gotcha:* versão de
produto ≠ versão de contrato (0.9.0) — são escalas diferentes; documentar na
sync. *Descartado:* const mágica "v1.3.0" versionada no front; versão no
orquestrador via heartbeat (o chip de nó mostra a versão de produto do v1;
versão **por serviço/nó** é dívida da telemetria por orquestrador — R1).

### D6 — Identidade do operador: nenhum backend novo; rótulo honesto "Operador local"

**Decidido:** não há backend novo. `/api/auth/me` (público) devolve
`{userId, loggedAt}` — não existe nome/email/role (tabela `users` = id +
password_hash; single-user por decisão da ADR-0001). F6.2 substitui
"Hephaestus Admin"/"admin@localhost"/"Root" (Sidebar L643-650) e o
"<Saudação>, Hephaestus Admin" do dashboard (L205) por **"Operador local"**
derivado da presença do cookie válido (200 de `/me` — o mesmo gate que o
`proxy.ts` já usa), mantendo o menu de logout. *Gotcha:* o `userId` (UUID) não
é nome legível — não exibir; `loggedAt` pode alimentar "sessão desde HH:MM"
sem backend novo.

### D7 — Erros e casing: zero erro novo; `queue_unavailable` reusado nas 3 rotas

**Decidido:** as 3 rotas públicas novas seguem o mapeamento padrão da casa
(Nota Fatia 4, backend.md :168): manager inalcançável ou erro do manager →
**503 `queue_unavailable`** (espelho do `get_telemetry` L458-474 — qualquer
erro do client vira 503, com log). 401 via gate; `not_found` não se aplica
(leitura de coleção; sem `:id` nas rotas novas). **Nenhum `Error.code` novo**
no spec 0.9.0 — a enum não cresce. Wire camelCase global (ADR-0002 D1):
`datasetsBytes/artifactsBytes/totalBytes/measured/lastHeartbeat/ramTotal`;
rotas internas do manager **snake_case** (transporte — `last_heartbeat`,
`artifacts_bytes`), re-mapeadas pelo BFF. Status exatos nas listas:
`GET /api/orchestrators|/api/models|/api/storage/usage` → `[200, 401, 503]`.

### D8 — Versionamento OpenAPI e ordem de commits (F6.0–F6.1b)

**Decidido:** spec **0.8.0 → 0.9.0** (regra "versão = ordem de landing",
ADR-0005 D1), branch **`feat/integracao-web`**. Contract ≡ router a cada
commit (delta OpenAPI incremental). Ordem **SEQUENCIAL**: F6.1a.0
(orquestrador: `ram_total` no heartbeat — base do wire) → F6.1a (manager:
cache + 3 rotas internas) → F6.1b (principal: BFF + `/health` version + spec
0.9.0). F6.1a.0/F6.1a tocam serviços diferentes — se o F6.1a passar de ~400
linhas de produção, dividir (ver notas do plano). F6.2/F6.3 (frontend) só
depois do backend vivo (regra F4.7: rebuild + recreate antes de despachar
UI). `cargo fmt --all` antes de reportar. Sem migration e sem spike (ver
seções próprias).

## Migration

**Nenhuma.** `orchestrators`/`job_artifacts`/`jobs`/`datasets` já existem com
as colunas necessárias; `ram_total` é transporte (não coluna nova — o cache
do manager é em memória); a lista de orquestradores lê colunas existentes;
a contagem de storage usa o contador por trigger existente. `test-db.sh` não
muda de schema — ganha casos (ver Testes).

## Delta OpenAPI (0.9.0) — descrição na ADR

Rotas públicas novas (entram em `PROTECTED_ROUTES` com status exatos):
```
GET /api/orchestrators     200 401 503     # lista real (tabela do manager)
GET /api/models            200 401 503     # pesos derivados (artifacts kind=model)
GET /api/storage/usage     200 401 503     # {datasetsBytes,artifactsBytes,totalBytes,measured}
```

- `GET /api/orchestrators` — 200 `{items:[Orchestrator]}` onde
  `Orchestrator = {id, name, kind: "local"|"remoto", endpoint, status,
  lastHeartbeat: string|null}`; 503 `queue_unavailable` (manager fora).
- `GET /api/models` — 200 `{items:[ModelWeight]}` onde `ModelWeight = {id,
  name, engine, model, jobId, bytes, createdAt}` (último checkpoint
  `kind='model'` por `(engine, model)` de jobs `done`); `name` = basename do
  path (`best.pt`/`last.pt`); 503 `queue_unavailable`.
- `GET /api/storage/usage` — 200 `StorageUsage = {datasetsBytes, artifacts
  Bytes, totalBytes, measured}` (sempre `measured:true` quando 200 — bytes
  reais dos contadores; o 200 é atômico: qualquer fonte fora ⇒ 503, sem
  resposta parcial que o front precisaria interpretar); 503
  `queue_unavailable` (manager fora — artefatos indisponíveis).

Schemas alterados:
- `Telemetry` ganha `ramTotal: integer|null` (bytes; aditivo, opcional).
- `GET /health` (200) ganha `version: string` no body (não é rota nova).

**Erros novos: nenhum** — enum `Error.code` inalterada (0.9.0 reusa
`invalid_request/not_found/unauthorized/queue_unavailable`). Rotas internas
do manager (`GET /internal/orchestrators`, `GET /internal/models`,
`GET /internal/storage/usage`, heartbeat com `ram_total`) **não** entram na
OpenAPI (fora do contrato público, padrão dos paths internos existentes).

## Testes

| Camada | Infra | Prova |
|---|---|---|
| `cargo test -p orchestrator` | unit (sem S3) | `read_ram_total()` (parse de `MemTotal` com fixture de conteúdo — espelho do teste do `read_ram`); `HeartbeatBody` serializa `ram_total` |
| `cargo test -p manager -- --ignored` + `scripts/test-db.sh` | Postgres do compose | **orchestrators**: auto-adoção → 1 row `online`; `GET /internal/orchestrators` devolve id/name/endpoint/kind/status/last_heartbeat; **models**: job yolo `done` com artefatos `[best.pt model, last.pt model, metrics.jsonl metrics]` → 1 item (`best.pt`); 2º treino do mesmo `(engine,model)` → segue 1 item, o mais novo (`created_at` DESC + preferência `best`); artefato `boxes` (autotracker) excluído; sem jobs `done` → `{items:[]}`; **usage**: `SUM(job_artifacts.bytes)` = soma esperada; **telemetria**: heartbeat com `ram_total` → cache/response com `ram_total` Some; sem heartbeat recente → `ram_total` None |
| `cargo test -p api-principal` | MockStorage + pool lazy | mapeamento 503 `queue_unavailable` nas 3 rotas (MockManager fail); 401 nas 3; shape camelCase dos itens (mapeamento `last_heartbeat`→`lastHeartbeat`, `artifacts_bytes`→`artifactsBytes`, `name` = basename); `get_telemetry` repassa `ramTotal`; `/health` body contém `version`; inventário de rotas ≡ spec 0.9.0 (contract, 3 rotas novas) |
| `scripts/test-db.sh` (extensão) | Postgres do compose | principal: `GET /api/storage/usage` soma `datasets.size_bytes` de datasets com imagens/vídeos ativos + artefatos do manager (valores reais, `measured:true`); modelos dedupe ponta-a-ponta via manager |
| E2E smoke (stack `ENGINE_MOCK=1`, `EXEC_MODE=docker`, Chrome) | compose completo | `GET /api/orchestrators` 200 com 1 item `orchestrator-local online`; rodar 1 treino yolo `done` → `GET /api/models` 1 item `best.pt`; `GET /api/storage/usage` com total > 0; `GET /api/telemetry` com `ramTotal` > 0 e `ramTotal` == MemTotal do host aproximado; `/health` com `version`; dashboard sem valores fixos (F6.2); console limpo |

O CI cobre: `cargo test` dos 3 serviços + `scripts/test-db.sh` + contract
(spec ≡ router) — as baterias desta ADR entram nas suítes existentes, sem
job novo de CI (sem migration/ci.yml).

## Spike obrigatório? — **NÃO**

Não há premissa externa não verificada: `ramTotal` vem do mesmo `/proc/meminfo`
que o `read_ram` **já parseia** (L1011-1040) — código nosso, padrão testado;
as listas vêm de SQL sobre tabelas existentes; o acesso do principal ao
manager é o padrão `ManagerPort` provado nas fatias 4/5; nenhuma credencial
S3 nova (D3 rejeitou ListObjects). *Inverteria (sem spike, só ajuste):* se o
manager algum dia tiver >1 orquestrador e a UI precisar de gauges por nó →
fatia própria de "telemetria por orquestrador" (heartbeat com identidade +
cache por nó + watchdog) — registrado como dívida (R1), não como spike.

## Riscos

- **R1 — Cache de telemetria global e `last_heartbeat` sem identidade:** ~~dívida~~ **QUITADA (Fatia H, ADR-0011 D1/D2/D4):** heartbeat identificado (`endpoint` no body + `ORCH_ADVERTISE_URL`), cache por nó (`HashMap<Uuid, TelemetryState>`), watchdog `degraded`/`offline` com re-queue. Multi-nó remoto funciona.
- **R2 — `GET /api/models` é derivado de `job_artifacts`:** (a) os pesos são
  mock de 110 bytes — o card deve rotular "pesos de treinos (mock)", nunca
  "YOLOv11/Flux.1/SDXL"; (b) `job_artifacts` tem FK `ON DELETE CASCADE` do
  job — não há rota de DELETE de job em produto, mas se surgir, pesos somem
  com o job (comportamento documentado); (c) dedupe por `(engine, model)` —
  "latest best.pt por modelo treinado", sem paginação (conjunto pequeno e
  limitado pelos modelos válidos de treino). Dívida: models real (tabela +
  upload/download + volume `models/`) = fatia Roadmap.
- **R3 — Storage medido ≠ bucket real:** a soma SQL não cobre `packages/*`
  órfãos (ADR-0007 R6), mídia da lixeira não purgada e objetos órfãos de
  restauração — drift possível entre o número do card e o disco do SeaweedFS.
  Mitigação: subtexto honesto do card ("rastreados pelo banco") + dívida de
  reconciliação bucket×banco quando houver gestão de storage real.
- **R4 — Compat de shape no rollout do `ramTotal`:** campo aditivo opcional
  nas 5 camadas — ordem de deploy não quebra nada (front antigo ignora; front
  novo com backend antigo vê `null`/`undefined` → "—"). Risco residual: o
  front (F6.2) deve tratar `null` e `undefined` igual — nenhum fallback
  numérico (regra "nenhum número inventado").
- **R5 — Semântica de `measured` divergente entre doc e código:** ~~docs/backend
  .md :173 afirma "sem GPU → `measured:false`"~~ **CORRIGIDO na sync H.7**: a
  semântica real (código) é: `measured:true` = heartbeat ≤ 10s; GPU ausente =
  `gpus:[]`/`vram_*:null` com CPU/RAM reais. A UI deriva "sem GPU" de
  `vramTotal==null || gpus.length===0`.
- **R6 — Versão via `CARGO_PKG_VERSION`:** expõe 0.1.0 dos crates como versão
  de produto. Se o produto evoluir com versionamento próprio (1.x), troca-se
  a fonte no mesmo lugar — o contrato (`/health.version`) não muda.
- **R7 — Lista de orquestradores expõe endpoint interno docker**
  (`http://orchestrator-local:8082`): inalcançável do browser do host. A API
  devolve o dado real da tabela; a UI (F6.2) decide o rótulo amigável
  ("Nó local") sem mentir sobre o endpoint (mostrar em `title`/mono onde
  couber).
- **R8 — Polling do dashboard × 3 rotas novas:** o tick atual já faz 3 chamadas
  (telemetry + datasets + jobs); as 3 rotas novas podem somar ao mesmo tick ou
  ficar em frequência própria por card (decisão do F6.2). Custo trivial
  single-user; as rotas novas são leituras SQL baratas (contador por trigger,
  `DISTINCT ON` sobre conjunto pequeno). Sem cache adicional no v1.

## O que fica falso nos docs (lista para o `@docs-sync`, commit F6.4)

**Não aplicar agora** — docs descrevem o que existe, não o que foi aprovado.

- `backend.md` §9/:137 — `models: GET /api/models … POST upload/download` →
  `GET /api/models` **implementado** (v1 derivado de `job_artifacts
  kind='model'`, fonte trocável quando a tabela `models` migrar); upload/
  download **permanecem pendentes** (Roadmap).
- `backend.md` §9/:158-161 — bloco orchestrators → `GET /api/orchestrators`
  implementado (leitura via manager); `POST /api/orchestrators/adopt`,
  `POST /:id/{enable,disable,remove}`, `GET /:id/health` e o alias
  `/api/environments*` **permanecem pendentes** (adote remoto = RunPod, fora);
  nota da auto-adoção local (:161) confirmada.
- `backend.md` §9/:162 — telemetria wire ganha `ramTotal`; nota da leitura
  `/proc` (CPU/RAM do container ≈ host) permanece.
- `backend.md` §"Nota Fatia 4" :173 — `measured:false` = caminho morto →
  **corrigir para a semântica real** (R5): `measured:true` = heartbeat ≤ 10s;
  GPU ausente = `gpus:[]`/`vram_*:null` com CPU/RAM reais.
- `backend.md` §10/:282-283 — tabela `models` **não migrada** nesta fase (a
  lista v1 deriva de `job_artifacts`; a tabela nasce com a fatia Roadmap de
  modelos); §10/:277-281 `orchestrators` — nota: `gpus`/`vram_total_gb` nunca
  escritos no v1 (telemetria é cache global; watchdog/telemetria por nó =
  dívida).
- `frontend.md` §10/:224 — "Ambientes: GET /api/environments (= GET
  /api/orchestrators)…" → **rota fantasma corrigida**: o alias permanece
  pendente (módulo Roadmap); o que existe é `GET /api/orchestrators`,
  consumido pelo `/dashboard` (F6.2).
- `frontend.md` §10/:244 — "Models: GET /api/models (dropdowns) + POST
  /models/{upload,download}" → `GET /api/models` implementado (derivado);
  POSTs permanecem pendentes.
- `frontend.md` §10/:242 — `Telemetry` ganha `ramTotal`; §5.1 (dashboard) e §4
  (Sidebar): as descrições de nós/telemetria/identidade passam a descrever o
  estado integrado (após F6.2) — hoje descrevem parte do mock (chips fixos).
- `frontend.md` §10/:219 — `/health` body ganha `version` (nota no contrato
  de auth/health).
- `dividas.md` — novas: "telemetria por orquestrador (heartbeat com id +
  cache por nó + watchdog `degraded/offline`)" (R1); "models real — tabela +
  upload/download + volume `models/` + dropdowns de treino" (R2/D2);
  "reconciliação bucket×banco / storage real via List" (R3).
- `coordenacao.md` — bloco F6 reescrito a cada commit; se necessário, `plano`.

## Plano de commits (F6.0–F6.1b; branch `feat/integracao-web` de `main`)

Um dispatch = um commit; produção < 400 linhas por commit (contrato/testes
fora da conta — exceção da casa). **F6.1a.0 → F6.1a → F6.1b SEQUENCIAIS**
(o `ram_total` do orquestrador é a base do cache do manager; o BFF do
principal consome as rotas internas do manager). F6.2 (dashboard) e F6.3 (IA)
só depois do backend vivo.

| # | Dono | Conteúdo | Critério de pronto |
|---|---|---|---|
| **F6.0** | @architect | Esta ADR (proposta; vira executável após aceite do usuário) | auditoria do coordenador; arquivo commitado em `main` |
| **F6.1a.0** | @rust-dev (orchestrator) | `read_ram_total()` (parse `MemTotal`, com teste de fixture de conteúdo) + campo `ram_total: Option<i64>` no `HeartbeatBody` + preenchimento no loop do heartbeat (~2s) | `cargo test -p orchestrator` verde; `cargo fmt --all`; ~70 linhas |
| **F6.1a** | @rust-dev (manager) | cache `state.ram_total` (+ `TelemetryResponse.ram_total`) no `receive_heartbeat`/`get_telemetry`; rotas internas `GET /internal/orchestrators` (SELECT da tabela), `GET /internal/models` (DISTINCT ON kind='model' + JOIN jobs), `GET /internal/storage/usage` (`SUM(job_artifacts.bytes)`) + handlers em `main.rs` + extensão dos testes de manager/test-db (orchestrators/models/usage/ram_total — tabela de Testes) | `cargo test -p manager -- --ignored` + `bash scripts/test-db.sh` verdes; fmt limpo; ~330 linhas (se estourar: dividir em F6.1a-1 cache+telemetria e F6.1a-2 rotas internas, contract interno implícito — sem spec pública no meio) |
| **F6.1b** | @rust-dev (principal) | `ManagerPort` + `HttpManager` + `MockManager`: `list_orchestrators`/`list_models`/`get_storage_usage`; handlers `get_orchestrators`/`get_models`/`get_storage_usage` em `jobs/` (ou módulo novo `monitoring.rs`); `TelemetryResponse.ram_total`; `version` no `/health` (`env!("CARGO_PKG_VERSION")`); 3 entradas em `PROTECTED_ROUTES`; spec 0.9.0 (rotas + schemas `Orchestrator`/`ModelWeight`/`StorageUsage` + `Telemetry.ramTotal` + `/health.version`); units + test-db principal (usage soma datasets) + contract | `cargo test -p api-principal` verde; contract ≡ router 0.9.0; `bash scripts/test-db.sh` verde; fmt limpo; ~380-420 linhas → **dividir F6.1b em b1 (BFF+rotas+spec parcial) e b2 (spec completa + testes)** se estourar |

**Notas de processo (lições das fatias 4/5):** rotas novas com status exatos
em `PROTECTED_ROUTES`; contract test exige spec ≡ router a cada commit (delta
OpenAPI incremental); erros mapeados 1:1 para 503 `queue_unavailable`
(espelho do `get_telemetry` — não inventar erro novo); `cargo fmt --all`
antes de reportar; implementador que achar problema FORA do escopo **para e
reporta ao coordenador** (nunca edita fora, nem workaround). Atenção ao R5:
não "corrigir" o manager para casar com a nota antiga do backend.md. F6.2 e
F6.3 são despachos próprios de frontend (via /impeccable), com review por
página, **depois** do F6.1 verificado no produto (rebuild + recreate dos
containers antes de despachar UI — regra F4.7).
