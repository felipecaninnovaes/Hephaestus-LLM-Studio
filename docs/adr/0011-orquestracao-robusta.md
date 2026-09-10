# ADR-0011 — Orquestração robusta: heartbeat com identidade, roteamento por capacidade, watchdog offline e adopt por token (Fatia H)

- **Status:** **ACEITA** (usuário, 2026-09-09 — "Aprovado"; auditoria do coordenador: APROVA COM 1 NOTA não-bloqueante — `cpu/ram:null` no agregado com >1 nó é comportamento aditivo documentado). Nada implementado. Este
  documento é a especificação executável da fatia H ("Orquestração robusta"); os
  deltas de contrato abaixo são aplicados **apenas nos commits da fatia** (openapi
  junto do código no H.4, docs de texto no `docs-sync` do fim, H.7), nunca antes.
- **Data:** 2026-09-09
- **Componentes:** `services/orchestrator` (identidade no heartbeat + endpoint de
  pareamento), `services/manager` (cache de telemetria **por nó**, roteamento por
  capacidade, watchdog no worker loop, adopt/revoke internos), `services/api-principal`
  (BFF: `POST adopt`/`POST revoke`, lista enriquecida, alias `/api/environments*`,
  spec **0.10.0**), `apps/web` (H.5 — módulo "Orquestradores" habilitado + dashboard
  por nó), `infra/README-gpu.md` (sessão GPU sem psql), `packages/policies/vram-table.yaml`
  (passa a ser consumida pelo manager).
- **Fontes:** `IDEIA.md` §1/:18-21 (orquestrador "decide/gerencia onde o treino vai
  rodar (local ou remoto)"); `docs/backend.md` §1/:28-33 (manager dono da fila/
  política VRAM; orquestrador stateless; Postgres dividido), §6/:74-92 (vram-table,
  headroom, `default_train_gb`, "se livre >= min → sobe em paralelo"), §8/:104-113
  (adoção por token `heph_p_*`/`heph_o_*`, health-check 15s→degraded/5→offline —
  o fluxo futuro que esta fatia implementa, **com divergências conscientes em D5**),
  §9/:160-165 (adopt/revoke pendentes; alias `/api/environments*`), §10/:230-232
  (schema `orchestrators` com `fingerprint`/`token_hash`/`gpus`/`vram_total_gb`);
  `docs/adr/0007-jobs-v1.md` D3 (auto-adoção local dedupe endpoint, Bearer
  `MANAGER_TOKEN`, recovery no boot) e D9 (policy VRAM no-op na v1);
  `docs/adr/0009-web-integracao-monitoramento.md` D1 (`GET /api/orchestrators` sem
  métricas por nó; UI compõe gauges só com `items.length===1`; "coluna vazia ≠
  dado") e R1 (heartbeat sem identidade + cache global = dívida);
  `docs/adr/0010-treino-real-gpu.md` D1 (INSERT manual preenche
  `gpus`/`vram_total_gb`), **D2 (contrato operacional manual da sessão GPU — o que
  esta fatia mata)**, D8 (oscilação de telemetria entre 2 nós) e D9 (envelope
  seguro; entrada `yolo11n=6` não cabe na 1660S de 6GB); `docs/dividas.md`
  (telemetria por orquestrador; watchdog F4.8; adopt/rotate/revoke + alias;
  roteamento por capacidade; nota MiB); `packages/policies/vram-table.yaml`
  (yolo11n=6, yolo11m=10, headroom 2, default_train_gb 16). Código (verificado por
  graft nesta data): `services/manager/src/lib.rs` — `receive_heartbeat` L804-830
  (UPDATE `last_heartbeat` de **TODOS** os `online/degraded` + cache global
  sobrescrito), `get_telemetry` L833-870 (global), `TelemetryCache` L237-241
  (`Arc<RwLock<TelemetryState>>` single), `dispatch_next` L1044-1138 (**`SELECT id,
  endpoint FROM orchestrators WHERE status='online' LIMIT 1`** — arbitrário),
  `adopt_orchestrator` L901-913 (só local), `list_orchestrators` L935-956,
  `recover_jobs` L1025-1035 (re-queue `dispatched/preparing/running/cancelling` →
  `queued recovered`), `OrchestratorClient::post` L164-202 (sem retorno de body —
  o adopt precisa verificar o pairing code); `services/manager/src/main.rs` L457-475
  (worker loop 2s), L422-431 (`AUTO_ADOPT_LOCAL`); `services/orchestrator/src/lib.rs`
  `HeartbeatBody` L58-66 (**sem identidade**), `HttpHeartbeatClient` L470-508,
  `services/orchestrator/src/main.rs` L395-424 (loop ~2s), rotas do orquestrador
  (só `/internal/dispatch`, `/internal/abort`, `/health`, `/ready`);
  `services/api-principal/src/monitoring.rs` (`get_orchestrators` L99-117,
  `OrchestratorResponse` L29-37), `src/auth/routes.rs` L113/L303 (só `GET`);
  `services/api-principal/migrations/0006_jobs.sql` L4-16 (`orchestrators`:
  `fingerprint TEXT`, `token_hash TEXT`, `gpus JSONB`, `vram_total_gb INT`, `status
  TEXT NOT NULL DEFAULT 'unknown'` **sem CHECK**); `apps/web/components/studio/
  Sidebar.tsx` L258-266 (módulo "Orquestradores" id `environments`, badge Roadmap,
  `isAvailable:false`), L316-322 (`orchStatus` com `length===1`).
- **Sequência:** …→ G (mergeada) → **H (esta: orquestração robusta)** → dívidas
  registradas (H.7).

## Contexto

A fatia G (ADR-0010) provou o treino real @gpu no TrueNAS usando um **contrato
operacional manual** (D2): parar o orquestrador local, `DELETE`/`INSERT` na tabela
`orchestrators` via psql, `AUTO_ADOPT_LOCAL=0` + recreate do manager. Esse
contrato existe porque o manager v1 tem 4 lacunas estruturais (todas dívidas
registradas): (1) o heartbeat não identifica o orquestrador — `receive_heartbeat`
atualiza `last_heartbeat` de TODOS os `online/degraded` e sobrescreve um cache
**global** (ADR-0009 R1; 2 nós simultâneos = telemetria oscilando — ADR-0010 D8);
(2) `dispatch_next` escolhe orquestrador com `LIMIT 1` sem `ORDER BY` (arbitrário
entre 2+ online — ADR-0010 D2); (3) não há watchdog — `status` nunca sai de
`online` (dívida F4.8); (4) a adoção de um orquestrador remoto só existe via
INSERT manual (backend.md §8 descreve o fluxo por token `heph_p_*`/`heph_o_*` como
futuro). O usuário fechou a fatia H com 4 itens: heartbeat com identidade + cache
por nó; roteamento por capacidade (matar o `LIMIT 1`, conectar `vram-table.yaml` e
`jobs.vram_min_gb`); watchdog offline; adopt por token (matar o INSERT manual).
O objetivo declarado é tornar a orquestração multi-nó robusta e **matar o contrato
operacional manual da sessão GPU**.

Regras da casa aplicadas: mock nunca quebra (baterias existentes intocadas =
critério); wire camelCase em `/api/*`, transporte interno snake_case (ADR-0002
D1); erros `{code,message}` com `message` estática; PRs < ~400 linhas de produção;
spec junto do código no commit; branch `feat/orquestracao-robusta`; smoke manual
@multi-nó fora do CI; teste @gpu continua manual (postura ADR-0010).

## Decisões já travadas (base — citar, não redecidir)

- **`orchestrators` (0006) é tabela do manager; orquestrador nunca toca Postgres;
  principal só fala com o manager via Bearer `MANAGER_TOKEN`** (ADR-0007 D3/D8).
- **Dispatch/report/heartbeat = HTTP; auto-adoção do `orchestrator-local` no boot
  por dedupe de endpoint UNIQUE; recovery no boot re-queueia `dispatched/
  preparing/running/cancelling` → `queued recovered`** (ADR-0007 D3; `recover_jobs`
  L1025-1035). O re-queue do watchdog (D4) **espelha** essa função — mesma
  semântica, escopada ao nó morto.
- **Guarda anti-mock do orquestrador remoto PERMANECE intocada** (ADR-0010 D2:
  `ORCH_GPU_DEVICES` setado + imagem `:local` ⇒ recusa com erro visível; escapada
  `ORCH_GPU_ALLOW_MOCK=1`). Esta fatia não toca em nada do caminho GPU do
  orquestrador.
- **Schema já tem tudo que a fatia precisa — SEM migration** (confirmado na
  `0006_jobs.sql`): `orchestrators.fingerprint TEXT` e `token_hash TEXT` existem
  (L9-10) e ficam **NULL na v1** (D5); `status TEXT` é livre (sem CHECK — os
  estados `revoked`/`disabled` cabem sem DDL); `queue_reason TEXT` já aceita
  `waiting_vram`; `jobs.vram_min_gb INT` existe (hoje sempre NULL no submit —
  `handlers.rs:601/759`); `gpus JSONB`/`vram_total_gb INT` existem.
- **`vram-table.yaml`**: `yolo11n=6`, `yolo11m=10`, `headroom_gb=2`,
  `default_train_gb=16`; emenda G.7: `yolo11n=6+2=8` **não cabe** na 1660S (6GB
  físicos) — revisão de entradas fica para a fatia de policy (D3 não reescreve a
  tabela). `jobs_active` do heartbeat é do orquestrador que executa (ADR-0010 D8).
- **`GET /api/telemetry` = proxy do cache do manager; `measured:true` = heartbeat
  ≤ 10s; GPU ausente = `gpus:[]`/`vram_*:null` com CPU/RAM reais** (semântica real
  do código — R5 ADR-0009, já corrigida na sync).
- **Espec OpenAPI atual: 0.9.0; regra "versão = ordem de landing"** (ADR-0005 D1)
  → **0.10.0**. Versão dos crates dos 3 serviços: 0.1.0 (intocada).
- **Unidades: VRAM viaja em MiB no wire do heartbeat** (nota dividas.md; ADR-0010
  fix `c71bb0d`); a coluna `vram_total_gb` é INT em **GB**. D2 faz a conversão
  `round(MiB/1024)` — nunca assumir MB.

## Decisões

### D0 — Escopo da fatia H (entra/sai)

**Decidido — entra:**
- **Heartbeat com identidade** (D1): campo `endpoint` no `HeartbeatBody` interno +
  env `ORCH_ADVERTISE_URL`; o manager casa por `endpoint UNIQUE` e atualiza **só**
  a linha certa (mata o UPDATE global).
- **Cache de telemetria por nó** (D2): `HashMap<Uuid, TelemetryState>` no manager;
  `GET /internal/orchestrators` (e BFF `/api/orchestrators`) enriquecido com a
  telemetria de cada nó; `GET /internal/telemetry` (global) vira **agregação**
  definida; o heartbeat identificado passa a **gravar `gpus`/`vram_total_gb`** na
  linha (mata a nota "nunca escritos pelo manager" e o dado estático do INSERT
  manual).
  **Emenda E2 (produto, 2026-09-10):** `vram_total_gb` = **maior GPU individual**
  (`round(max_gpu_mib/1024)`) — 1 job = 1 GPU (§6), a soma das GPUs instaladas
  (18GB no TrueNAS) NÃO é capacidade de treino; telemetria `vramTotal` continua a
  soma (VRAM instalada). Fallback: heartbeat sem `max_gpu_mib` (legado) usa a soma.
- **Roteamento por capacidade** (D3): substituir o `LIMIT 1` por SQL com filtro de
  nó ocupado + capacidade estática (`vram_min` da vram-table + headroom vs
  `vram_total_gb`) + `ORDER BY` determinístico; `queue_reason='waiting_vram'`
  quando há nó online mas nenhum com capacidade.
- **Watchdog offline** (D4): no worker loop existente (~2s); `online → degraded`
  (15s sem heartbeat) → `offline` (60s); na transição para `offline`, re-queue dos
  jobs do nó (espelho de `recover_jobs`); heartbeat revive `online` (mas **não**
  `revoked`).
- **Adopt por token** (D5): `POST /api/orchestrators/adopt {name, endpoint, kind,
  pairingCode}` (BFF → manager `POST /internal/adopt` → verificação no orquestrador
  `POST {endpoint}/internal/pairing/verify`); `POST /api/orchestrators/:id/revoke`
  (status `revoked`, não DELETE); alias `/api/environments*`; erro novo
  `pairing_invalid` (409); spec 0.10.0.
- **UI** (D7): módulo "Orquestradores" da Sidebar habilitado (página `/environments`
  — lista + adopt + revoke); dashboard com gauges **por nó** (a regra
  `items.length===1` da ADR-0009 D1 **morre**).
- **Compat** (D8): o contrato manual da ADR-0010 D2 morre (revoke+adopt via API);
  `AUTO_ADOPT_LOCAL` vira legacy; README-gpu atualizado na sync.

**Decidido — fora (dívida/futuro, NÃO agora):**
- **Policy VRAM aplicada completa** (fila `waiting_vram` por **VRAM livre
  dinâmica**, paralelismo 2+ jobs por nó, preempção de runner, `max_parallel_
  trainers`): D3 faz só o **roteamento estático** (capacidade declarada vs
  requisito) — o "se livre >= min → sobe em paralelo" do §6/:91 continua dívida.
- **Credenciais por nó (`heph_o_*`) + rotação (`rotate`) + TLS com pin de
  fingerprint**: `token_hash`/`fingerprint` ficam NULL; transporte continua com
  `MANAGER_TOKEN` compartilhado (divergência consciente — D5).
- **`enable`/`disable`** (flip manual de status sem consumidor) e
  **`GET /api/orchestrators/:id/health`** (o watchdog dá o status; health-check
  ativo por nó é redundante na v1).
- **Reconciliação de jobs órfãos com dupla execução** (partição de rede): o
  re-queue do D4 assume nó morto; sem detecção de "ainda vivo mas isolado"
  (documentado em R3).
- **Transporte chunked, WS, pause/resume, samples** — intactos (fora).

**Descartado:** "continuar com o contrato manual + só documentar" (o usuário pediu
matar o manual); "identificar o heartbeat por token por nó já nesta fatia" (mexe no
transporte dos 3 serviços sem necessidade — o adopt resolve a porta de entrada,
D5); "watchdog derrubando jobs com `failed`" (nó morto ≠ job falho; re-queue é a
semântica do recovery).

### D1 — Identidade do heartbeat: `endpoint` no body, anunciado por `ORCH_ADVERTISE_URL`

**FATO (código):** `HeartbeatBody` (lib.rs L58-66) não carrega identidade; o
manager faz `UPDATE orchestrators SET last_heartbeat=now() WHERE status IN
('online','degraded')` — **todos** os nós — e sobrescreve o cache global
(`receive_heartbeat` L804-830). Com 2 nós heartbeating, todo nó "vê" o heartbeat
dos outros e o cache oscila (ADR-0010 D8).

**Decidido:** o `HeartbeatBody` ganha **`endpoint: String`** (delta de contrato
**interno** manager↔orquestrador — permitido; rotas internas não entram na
OpenAPI), preenchido pelo orquestrador a partir do env **`ORCH_ADVERTISE_URL`**
(default **`http://orchestrator-local:8082`** — exatamente o endpoint da
auto-adoção local, para o mock local nunca quebrar sem config). O manager, no
`receive_heartbeat`:
1. resolve `SELECT id FROM orchestrators WHERE endpoint = $1` (UNIQUE);
2. sem match → `warn!` (orquestrador não adotado ou `ORCH_ADVERTISE_URL` errado) e
   retorna — **sem auto-adopt por heartbeat** (a porta de adoção é o D5, não um
   heartbeat anônimo);
3. com match → `UPDATE ... SET last_heartbeat=now(), status='online' WHERE id=$1
   AND status <> 'revoked'` (revive `degraded/offline/unknown`; **não** revive
   `revoked` — D5) **e** grava `gpus = $gpus`/`vram_total_gb = round($vram_total_
   MiB / 1024)` quando o heartbeat carrega `Some(vram_total)` e `gpus` não-vazio
   (kills a nota "nunca escritos pelo manager" — dado passa a ser **dinâmico**, não
   estático do INSERT);
4. cache por nó (D2).

*Por quê — endpoint, não `name`:* `endpoint` é a chave natural **UNIQUE** da tabela
e a mesma usada pela auto-adoção (dedupe) e pelo adopt (D5) — uma única identidade
em todo o ciclo de vida; `name` não é UNIQUE (colisão de nomes = atualização na
linha errada). *Por quê — env, não autodetecção:* o orquestrador atrás de docker
network não conhece seu endpoint público; `ORCH_ADVERTISE_URL` é config explícita
(compose local: `http://orchestrator-local:8082`; TrueNAS: `http://10.15.1.2:8082`
no env.gpu). *Gotcha:* `ORCH_ADVERTISE_URL` errado = heartbeat órfão (warn no log
do manager, nó sem `last_heartbeat` → watchdog derruba para `degraded/offline` —
falha **visível**, não silenciosa). *Descartado:* header (`X-Orch-Id`) — mesmo dado
no body, body é o padrão do transporte interno (snake_case JSON); `orchestrator_id`
UUID no heartbeat — o orquestrador **não tem** o id da linha (gerado pelo manager
no adopt/auto-adopt); token por nó — escopo do D5 (fora da v1).

### D2 — Cache por nó + telemetria agregada + `GET /api/orchestrators` enriquecido

**FATO (código):** `TelemetryCache = Arc<RwLock<TelemetryState>>` single (L237-241);
`GET /internal/orchestrators` devolve a tabela **sem métricas** (`list_orchestrators`
L935-956); a regra de UI "gauges só quando `items.length===1`" (ADR-0009 D1) existe
porque o cache global não pode ser atribuído a nó nenhum.

**Decidido (opção b):**
1. **Cache por nó**: `TelemetryCache = Arc<RwLock<HashMap<Uuid, TelemetryState>>>`
   (chave = id do orquestrador resolvido no D1). O `receive_heartbeat` grava na
   entrada do nó; `TelemetryState` ganha `endpoint` (para log) — resto do shape
   igual (campos já existentes).
2. **`GET /internal/orchestrators` (e BFF `/api/orchestrators`) enriquecido**: cada
   item ganha `{measured, cpu, ram, ram_total, vram_used, vram_total, gpus,
   jobs_active}` (snake_case interno; camelCase no BFF: `ramTotal`, `vramTotal`,
   `jobsActive`) + **`vram_total_gb`** (coluna, GB — visibilidade do dado de
   roteamento). Nó sem heartbeat → telemetria `measured:false` + campos `null`
   (nunca número inventado — R5 ADR-0009). `measured` por nó = heartbeat ≤ 10s
   (mesma regra do global hoje).
3. **`GET /internal/telemetry` (global, contrato público `GET /api/telemetry`
   inalterado na rota) vira agregação com semântica definida**:
   - **0 nós no cache** → comportamento atual (`measured:false` + contagem da
     fila como `jobsActive` — fallback de hoje, L852-870);
   - **1 nó** → **idêntico ao de hoje** (o dado do único nó; compat total com o
     front atual e com a regra ADR-0009 D1 que morre no H.5);
   - **>1 nó** → `vram_used`/`vram_total` = **soma** dos `Some` (se todos `None` →
     `null`), `gpus` = **união** (ordem por nó, estável), `jobs_active` = **soma**,
     `cpu`/`ram`/`ram_total` = **`null`** (não agregáveis de forma honesta — CPU é
     razão por nó; RAM é total por nó; somar seria mentir), `measured:true` se ≥1
     nó fresco.
4. **UI (H.5)**: o dashboard passa a compor gauges **por nó** a partir do
   `/api/orchestrators` enriquecido — a regra `items.length===1` **morre**
   (substituída por "cada card de nó mostra a telemetria do próprio nó"). A
   telemetria global continua servindo a Sidebar/consumidores existentes com a
   semântica de agregação acima.

*Por quê — (b) e não (a) "agregar no global e manter lista sem métricas":* a lista
enriquecida é o único shape que permite ao dashboard ser honesto com 2+ nós (o
objetivo da fatia é multi-nó); manter só agregação global esconderia qual nó está
com GPU ocupada. *Por quê — cpu/ram null no agregado:* "nenhum número inventado"
(R5 ADR-0009) — média de CPU entre nós com cargas diferentes é enganosa; o dado
real fica no card por nó. *Gotcha:* `GET /api/telemetry` muda de semântica com >1
nó (antes: "o nó", agora: "a frota") — documentado no delta e na sync; front
antigo que só consome o global vê `cpu:null` e "—" (comportamento aditivo
definido em R7). *Descartado:* (c) "UI passa a consumir só por nó e o global vira
deprecated" (quebraria consumidores existentes sem ganho — ActionCenter e Sidebar
usam o global; agregação definida é compat); manter o cache global e apenas
"desagregar" por heurística (mentira por nó — ADR-0009 D1 já rejeitou).

### D3 — Roteamento por capacidade: SQL determinístico, vram-table no manager, nó ocupado fora

**FATO (código):** `dispatch_next` faz `SELECT id, endpoint FROM orchestrators
WHERE status='online' LIMIT 1` (L1067-1071) — arbitrário entre 2+ online; a
`vram-table.yaml` é lida por ninguém (no-op desde ADR-0007 D9); `jobs.vram_min_gb`
nasce NULL no submit (`handlers.rs:601/759`); o worker despacha 1 job a cada 2s e
**pode** re-despachar para o mesmo nó ocupado (sem filtro de jobs ativos).

**Decidido:**
1. **Resolução do requisito no MANAGER, em tempo de dispatch** (não no submit do
   principal — zero delta público no `POST /api/jobs/yolo`, e mudança de policy não
   exige re-submeter jobs): o manager carrega `vram-table.yaml` no boot (dep
   `serde_yaml` nova; env **`VRAM_TABLE_PATH`** com default compilado via
   `include_str!("../../../packages/policies/vram-table.yaml")` — arquivo versionado
   no mesmo repo; fail-fast no boot se o parse falhar). Para cada job `queued`:
   `required_gb = entries[engine][model][mode].vram_min_gb + defaults.headroom_gb`;
   **entrada faltante ⇒ requisito `NULL` (permissivo — ver 3)** — o
   `default_train_gb=16` **não** é aplicado como requisito estático nesta fatia
   (bloquearia engines sem medição, ex. autotracker, em nós GPU de 12GB; a nota
   `unmeasured` do §6/:89 permanece dívida da policy).
2. **SQL do `dispatch_next`** (substitui o LIMIT 1):
   ```sql
   SELECT o.id, o.endpoint
   FROM orchestrators o
   WHERE o.status = 'online'
     AND NOT EXISTS (SELECT 1 FROM jobs j
                     WHERE j.orchestrator_id = o.id
                       AND j.status IN ('dispatched','preparing','running','cancelling'))
     AND ($1::int IS NULL OR o.vram_total_gb IS NULL OR o.vram_total_gb >= $1)
   ORDER BY (o.vram_total_gb IS NULL) ASC,   -- nós com capacidade declarada primeiro
            o.vram_total_gb DESC NULLS LAST, -- maior GPU primeiro
            o.name ASC                       -- tie-break determinístico
   LIMIT 1
   ```
   com `$1 = required_gb` (ou NULL). Semânticas deliberadas:
   - **Nó ocupado excluído** (NOT EXISTS em jobs não-terminais do nó): 1
     orquestrador = 1 job ativo na v1 da fatia (o paralelismo por VRAM — 2+ jobs
     por nó — é a policy futura, §6/:91); sem isso, com 2 nós o ORDER BY esmagaria
     tudo no nó maior.
   - **`vram_total_gb IS NULL` = permissivo** (cabe qualquer job): o mock local
     auto-adotado não declara capacidade e **mock nunca quebra** (job yolo com
     requisito 8GB continua rodando no local quando é o único nó). O permissivo é
     **último** no ORDER BY: com remoto declarado (18GB) + local NULL, o job GPU
     vai para o remoto; o trap residual (remoto offline → job cai no mock) é
     mitigado pela sessão (D8: revoke do local) e documentado em R1.
   - **`vram_total_gb IS NOT NULL AND < required` ⇒ excluído**: a 1660S (6GB) não
     recebe `yolo11n` (6+2=8) — honesto, espelha a emenda G.7 da vram-table.
3. **Nenhum nó elegível com requisito presente** → `queue_reason='waiting_vram'`
   (valor já suportado pela coluna e já renderizado pelo front como `queueReason`);
   sem requisito e sem nó online → `waiting_slot` (comportamento atual).
4. **`kind` NÃO entra no filtro** na v1 (nenhum consumidor sinaliza "quero remoto";
   a decisão local-vs-remoto da IDEIA §1 é realizada **por capacidade**: o remoto
   declara, o local mock não — GPU jobs fluem para o declarado). Filtro por kind
   fica como extensão trivial futura (1 linha de WHERE) se a UI pedir.
5. **Política VRAM aplicada (fila por VRAM livre, paralelismo, preempção) —
   dívida, não esta fatia** (D0). O que entra é o **roteamento estático** acima
   (~40 linhas SQL + parse do yaml), não o escalonador dinâmico.

*Por quê — requisito no manager:* o manager é o dono da política VRAM (backend.md
§1/:28) e da tabela; o `jobs.vram_min_gb` da coluna fica como snapshot/transporte
(hoje NULL; o manager poderia gravá-lo no dispatch — **não grava** nesta fatia
para não duplicar fonte de verdade: o requisito é derivado do yaml a cada dispatch,
a coluna fica para quando o submit declarar requisito explícito). *Gotcha:*
`required = min + headroom` significa `yolo11m` (10+2=12) só cabe na 3060 (12GB,
no limite — ADR-0010 D9) e nunca na 1660S; coerente com o envelope já validado.
*Descartado:* `ORDER BY (kind='remoto') DESC` (ADR-0010 D2 já rejeitou — rotearia
tudo para o remoto sempre); resolver `vram_min_gb` no principal no submit (delta
público + policy congelada no job); aplicar `default_train_gb` incondicional
(quebraria o mock — requisito 16 > nenhuma capacidade declarada e o NULL
permissivo deixaria tudo no local de qualquer forma, escondendo o erro).

### D4 — Watchdog offline no worker loop; re-queue dos jobs do nó morto

**FATO (código):** o worker do manager já ticka a cada 2s (`main.rs` L457-475);
`status` nunca sai de `online` (dívida F4.8); `recover_jobs` (L1025-1035) re-queueia
`dispatched/preparing/running/cancelling` → `queued recovered` no boot.

**Decidido:**
1. **Quem executa**: o MESMO worker loop (uma função `watchdog_tick(pool)` chamada
   por tick, ~30 linhas) — sem task nova (o tick de 2s já existe; UPDATEs são
   baratos).
2. **Janelas** (envs com default): `last_heartbeat` mais velho que
   **`ORCH_WATCHDOG_DEGRADED_S`=15s** ⇒ `online → degraded` (~5-7 heartbeats
   perdidos); mais velho que **`ORCH_WATCHDOG_OFFLINE_S`=60s** ⇒ `degraded →
   offline`. Transições só para `online`/`degraded` (nunca toca `revoked`/`unknown`
   — `unknown` é pré-adopt, sem heartbeat esperado).
3. **Efeito em jobs**: na transição `degraded → offline`, um CTE re-queueia os
   jobs do nó (espelho EXATO de `recover_jobs`, escopado por `orchestrator_id`):
   ```sql
   WITH morto AS (
     UPDATE orchestrators SET status = 'offline'
     WHERE status = 'degraded' AND last_heartbeat < now() - $offline::interval
     RETURNING id
   )
   UPDATE jobs SET status = 'queued', queue_reason = 'recovered', orchestrator_id = NULL
   WHERE orchestrator_id IN (SELECT id FROM morto)
     AND status IN ('dispatched','preparing','running','cancelling')
   ```
   O job volta para a fila e o D3 o roteia para outro nó (ou fica `waiting_slot`
   honesto se não houver). *Por quê — re-queue entra (não é dívida):* sem ele, um
   nó morto mid-run deixa o job `running` para sempre (fantasma invisível — o
   watchdog sozinho só flipe o status); com ele, o comportamento é o MESMO do
   recovery de boot (semântica já aceita), e o smoke "matar um nó" tem desfecho
   verificável (job re-queued → roda no outro nó). *Gotcha:* partição de rede (nó
   vivo mas isolado) → dupla execução possível — risco aceito, igual ao do
   recovery de boot, documentado em R3; artefatos são last-write-wins por `job_id`
   no S3 (mesmo md5 para mesmos params/seed → benigno na prática).
4. **Revive**: o heartbeat (D1) volta `offline/degraded/unknown → online` na hora
   (o nó se reapresenta) — mas **nunca** `revoked` (D5). Sem janela de graça
   adicional: o próprio heartbeat é a prova de vida.
5. **Health-check ativo por nó (15s/2 falhas do §8) NÃO entra**: o heartbeat ~2s
   já é o sinal de vida; o watchdog sobre ele é mais rápido e mais simples que um
   health-check HTTP extra (e não depende de rota nova no orquestrador).

*Por quê — 15s/60s:* heartbeat de 2s + margem ~7× para `degraded` (não derrubar
em GC/slowdown momentâneo) e ~30× para `offline` (job mid-run não é re-queueado
por um soluço); os números do §8 (15s/5 falhas ≈ 75s) são de health-CHECK, aqui a
fonte é o heartbeat. *Descartado:* watchdog em task separada com tick próprio
(complexidade sem ganho — o worker já ticka 2s); transição direta
`online → offline` sem passar por `degraded` (a UI perde o aviso intermediário).

### D5 — Adopt por token: pairing code no orquestrador, adoção upsert, revoke = status

**FATO (código):** a adoção remota hoje só existe por INSERT manual na tabela
(ADR-0010 D1/D2); `orchestrators.fingerprint`/`token_hash` existem e são NULL; o
transporte manager↔orquestrador usa `MANAGER_TOKEN` compartilhado nos dois
sentidos; `OrchestratorClient::post` (manager L164-202) não devolve body (o adopt
precisa ler `{valid}` do orquestrador); backend.md §8/:104-110 descreve o fluxo
futuro (pairing `heph_p_*` de uso único exibido no log, credencial `heph_o_*`
guardada como hash, TLS com pin de fingerprint).

**Decidido — fluxo v1 (divergência consciente do §8, documentada):**
1. **Pairing code no orquestrador**: env **`ORCH_PAIRING_CODE`** (operador define —
   determinístico e scriptável na sessão GPU); ausente → o orquestrador **gera** um
   aleatório no boot e **loga uma vez** (espelho do §8 "exibido uma vez no log").
   Formato `heph_p_*` encorajado mas **não enforceado** (v1 aceita string 1..128 —
   divergência da proposta §8/:107).
2. **Verificação no orquestrador**: rota nova **`POST /internal/pairing/verify`**
   body `{code}` → 200 `{valid:true}` **e consome** (single-use, flag em memória);
   código errado ou já usado → 200 `{valid:false}`. Sem TTL no v1 (o §8 propõe 15
   min; código via env é controlado pelo operador; o flag single-use reseta no
   restart do orquestrador — aceito, documentado em R2).
3. **Adoção no manager**: rota interna **`POST /internal/adopt`** body
   `{name, endpoint, kind, pairing_code}` (Bearer `MANAGER_TOKEN`): valida domínio
   (400); chama `POST {endpoint}/internal/pairing/verify` (timeout 10s — padrão da
   casa); `valid:false` ou orquestrador inalcançável ⇒ **409 `pairing_invalid`**
   (message estática: "código de pareamento inválido ou orquestrador inalcançável");
   `valid:true` ⇒ **upsert** na tabela: `INSERT ... ON CONFLICT (endpoint) DO
   UPDATE SET name=EXCLUDED.name, kind=EXCLUDED.kind, status='online'` — cria com
   `status='online'` e **revive** uma linha `revoked` (adotar de novo é intenção
   explícita do operador). `token_hash`/`fingerprint` ficam **NULL** (v1: sem
   credencial por nó, sem TLS — ver 6). Capacidade (`gpus`/`vram_total_gb`) **não**
   vai no body: vem do heartbeat (D1), que preenche dinamicamente após o primeiro
   tick.
4. **Revoke**: rota interna **`POST /internal/orchestrators/:id/revoke`** ⇒
   `UPDATE ... SET status='revoked'` (**status, não DELETE** — mantém o histórico e
   dá semântica de tombstone). Efeitos: dispatch só escolhe `online` (revoked fora);
   watchdog não toca `revoked`; heartbeat não revive `revoked`; **auto-adoção não
   ressuscita `revoked`** (D8 — aí morre o `AUTO_ADOPT_LOCAL=0` como necessidade).
5. **BFF público**: **`POST /api/orchestrators/adopt`** body camelCase
   `{name, endpoint, kind, pairingCode}` → 200 (upsert create-or-update, devolve o
   item enriquecido) | 400 `invalid_request` | 409 `pairing_invalid` | 503
   `queue_unavailable`; **`POST /api/orchestrators/:id/revoke`** → 204 | 404
   `not_found` (id não-UUID ou inexistente — ADR-0002 D8) | 503. Alias
   **`/api/environments*`** (backend.md §9/:164): `GET /api/environments`,
   `POST /api/environments/adopt`, `POST /api/environments/:id/revoke` — mesmos
   handlers (nasce com o módulo da UI, D7 — ADR-0009 D0 quita o alias).
6. **Transporte segue com `MANAGER_TOKEN` compartilhado** (sem `heph_o_*` por nó,
   sem TLS com pin — **divergência consciente do §8/:108-109**): o pairing code
   autentica **quem entra na tabela** (a porta), não o tráfego; a postura de
   segurança continua a da ADR-0010 D10 (LAN caseira, bind específico, secrets
   escopados). `token_hash`/`fingerprint` ficam prontos para a fatia de
   credenciais por nó (rotate/TLS). Rate-limit de 5 tentativas (§8) fica de fora
   (single-user LAN; documentado).
7. **`AUTO_ADOPT_LOCAL`**: **permanece, default `1`, fail-open** (criação do
   `orchestrator-local` em ambiente fresco continua automática) — MAS a linha
   `revoked` nunca é ressuscitada por ele: o `adopt_orchestrator` ganha
   `ON CONFLICT (endpoint) DO UPDATE SET status='online' WHERE orchestrators.status
   <> 'revoked'`. O valor `0` continua funcionando (legacy). A sessão GPU passa a
   usar revoke+adopt via API (D8) — o `0` deixa de ser necessário.

*Por quê — revoke=status e não DELETE:* DELETE reabriria o buraco da auto-adoção
(próximo boot ressuscita o local — o trap que `AUTO_ADOPT_LOCAL=0` tapava);
tombstone `revoked` + guarda na auto-adoção fecha o ciclo sem env novo. *Por quê —
verificação no orquestrador e não hash no manager:* o código de pareamento é
segredo do orquestrador (o operador o lê de lá e o cola no manager); o manager não
guarda o código — só pergunta. *Gotcha:* verificar com sucesso e o INSERT falhar
depois queima o código (single-use) — o operador regenera (env) ou espera novo
boot; raro e visível. *Descartado:* adopt sem verificação (qualquer host com o
`MANAGER_TOKEN` se auto-adotaria — pior que o INSERT manual atual); token `heph_o_*`
por nó nesta fatia (mexe no transporte dos 3 serviços e no hash — fatia própria);
`revoke` = DELETE (abre o buraco da auto-adoção, acima).

### D6 — Contrato, schema e versionamento: delta público esperado, spec 0.10.0, SEM migration

**Decidido:** **migration: NENHUMA** (confirmado na 0006: `fingerprint`/`token_hash`/
`gpus`/`vram_total_gb` existem; `status` e `queue_reason` são TEXT livres; nada de
coluna nova — o cache por nó é memória, o vram-table é arquivo). **Spec 0.9.0 →
0.10.0** (ordem de landing — ADR-0005 D1), delta no H.4 (openapi junto do código).
Erro novo: **`pairing_invalid` (409)** — único `Error.code` novo. Rotas internas
novas (heartbeat com `endpoint`, `POST /internal/adopt`, `POST /internal/
orchestrators/:id/revoke`, `POST /internal/pairing/verify`, lista enriquecida) não
entram na OpenAPI (padrão dos paths internos). Envs novos — delta de contrato
interno (tabela no final).

**Casos de borda do contrato público:** adopt com `kind` fora de `local|remoto` →
400 (CHECK da coluna); `endpoint` sem esquema http(s) → 400; `name` vazio/`>128` →
400; `pairingCode` vazio/`>128` → 400; revoke de id não-UUID → 404 (D8 ADR-0002);
adopt quando o manager está fora → 503 `queue_unavailable` (mapeamento padrão);
orquestrador inalcançável no verify → 409 `pairing_invalid` (message cobre).

### D7 — UI: módulo "Orquestradores" habilitado + dashboard por nó (H.5, via /impeccable)

**Decidido — entra na fatia** (1 página = 1 review — regra da casa):
- **Sidebar**: módulo "Orquestradores" (id `environments`, href `/environments`)
  habilitado — badge "Roadmap" removido, `isAvailable:true`; "Storage S3" continua
  Roadmap. O chip de status do nó na Sidebar passa a usar a lista enriquecida
  (multi-nó: "2 orquestradores" em vez de `length===1` só).
- **Página `/environments`**: lista de orquestradores (cards glass — padrão do
  dashboard), status chip (`online/degraded/offline/revoked/unknown`), "visto há
  Xs" (derivado de `lastHeartbeat` — nunca "agora" fixo), **gauges por nó**
  (CPU/RAM/VRAM + nomes de GPU de `gpus[]`), ações: **Adotar** (modal com
  `name/endpoint/kind/pairingCode` → `adoptOrchestrator`) e **Revogar**
  (`ConfirmDialog` → `revokeOrchestrator`; revogado mostra "Revogado" + re-adotar).
  Empty state honesto ("Nenhum orquestrador adotado") com o modal de adopt à mão.
- **Dashboard**: a regra "gauges só quando `items.length===1`" (ADR-0009 D1)
  **morre** — cada card de nó compõe os gauges da telemetria **do próprio nó**
  (dados do `/api/orchestrators` enriquecido); o agregado `/api/telemetry` segue
  para a Sidebar/frota. Com 1 nó o dashboard fica visualmente igual ao de hoje
  (compat).
- **Contrato mínimo que o front exige** (lib/monitoring.ts): `Orchestrator` ganha
  `{measured, cpu, ram, ramTotal, vramUsed, vramTotal, vramTotalGb, gpus,
  jobsActive}`; `adoptOrchestrator(body)` → `POST /api/environments/adopt` (erros:
  `pairing_invalid` → "Código de pareamento inválido ou orquestrador inalcançável";
  503 → "Indisponível (manager fora)"); `revokeOrchestrator(id)` → `POST
  /api/environments/:id/revoke`.

*Por quê — a UI entra na fatia:* o adopt por token sem superfície de UI troca o
psql por curl (não mata o "manual"); a página dá ao operador o fluxo completo da
sessão GPU na interface — e o alias `/api/environments*` só tem consumidor com o
módulo habilitado (ADR-0009 D0 quita o alias junto). *Descartado:* módulo
desabilitado com API pronta (código sem consumidor — lição P1 da ADR-0006); página
só de leitura sem adopt/revoke (o fluxo manual sobreviveria na UI).

### D8 — Compat: o contrato manual da ADR-0010 morre; guarda anti-mock intocada

**Decidido:**
1. **Sessão GPU nova (README-gpu na sync H.7)**: substitui os 4 passos do
   ADR-0010 D2 por: (a) `docker compose stop orchestrator-local`; (b)
   `POST /api/orchestrators/:id/revoke` do local (uma chamada — ou UI); (c)
   `POST /api/orchestrators/adopt` do remoto com o `ORCH_PAIRING_CODE` do env.gpu
   (uma chamada — ou UI). **MORREM**: `DELETE`/`INSERT` via psql, o
   `AUTO_ADOPT_LOCAL=0` como necessidade, o recreate do manager por causa de env.
   Teardown: revoke do remoto + `docker compose start orchestrator-local` + adopt
   do local (revive a linha revoked). Nenhum comando psql.
2. **`AUTO_ADOPT_LOCAL` permanece** (default `1`) — só deixa de ser peça da sessão
   GPU; a guarda contra ressuscitar o local é o tombstone `revoked` (D5.7).
3. **Guarda anti-mock do ADR-0010 D2 (imagem `:local` × `ORCH_GPU_DEVICES`)**
   **PERMANECE intocada** — nenhum byte do caminho GPU do orquestrador muda nesta
   fatia (H.1 só toca heartbeat e pairing).
4. **ADR-0010 D1 (INSERT manual preenche `gpus`/`vram_total_gb`)**: o dado
   estático morre — o heartbeat identificado (D1) passa a preencher as colunas
   dinamicamente (dado real do host, não digitação manual).
5. **`measured` e a regra de gauges**: R5 ADR-0009 permanece (semântica real
   inalterada); o que muda é a regra de UI (D7).
6. **Rollback**: spec aditiva (R7); o manager H.2/H.3 é uma unidade — rollback =
   reverter o commit (o banco não muda, sem migration para desfazer).

## Migration

**Nenhuma** (D6). Confirmação na `0006_jobs.sql`: `orchestrators.fingerprint TEXT`
(L9), `token_hash TEXT` (L10), `gpus JSONB` (L11), `vram_total_gb INT` (L12) já
existem; `status` (L13) e `jobs.queue_reason` (L40) são TEXT livres (aceitam
`revoked`/`waiting_vram` sem CHECK novo); `jobs.vram_min_gb` (L42) existe. O
`test-db.sh` não muda de schema — ganha casos (Testes).

## Delta de contrato (OpenAPI 0.10.0) — descrição na ADR

Rotas públicas (entram em `PROTECTED_ROUTES`; status exatos):
```
GET  /api/orchestrators           200 401 503    # enriquecido com telemetria por nó
POST /api/orchestrators/adopt     200 400 401 409 503
POST /api/orchestrators/:id/revoke 204 401 404 503
GET  /api/environments            200 401 503    # alias do GET acima
POST /api/environments/adopt      200 400 401 409 503
POST /api/environments/:id/revoke 204 401 404 503
```

- `GET /api/orchestrators` (e alias `/api/environments`) — 200 `{items:[Orchestrator]}`;
  `Orchestrator` **ganha** `{measured: bool, cpu: number|null, ram: i64|null,
  ramTotal: i64|null, vramUsed: i64|null, vramTotal: i64|null, vramTotalGb: int|null,
  gpus: string[], jobsActive: int}` (telemetria do nó; `vramUsed/vramTotal` em MiB —
  mesma unidade do `/api/telemetry`; `vramTotalGb` = coluna, GB — dado de
  roteamento). Campo aditivo: front antigo ignora. 503 `queue_unavailable`.
- `POST /api/orchestrators/adopt` — body `{name: string (1..128), endpoint: string
  (http(s) URL), kind: "local"|"remoto", pairingCode: string (1..128)}` → **200**
  (upsert create-or-update — re-adopt revive `revoked`; idempotente) com o item
  `Orchestrator` enriquecido | 400 `invalid_request` (domínio) | 409
  `pairing_invalid` (código inválido/usado/orquestrador inalcançável) | 503
  `queue_unavailable` (manager fora).
- `POST /api/orchestrators/:id/revoke` — 204 (status → `revoked`) | 404 `not_found` (id não-UUID ou inexistente — D8 ADR-0002) | 503. Alias `/api/environments/:id/revoke` idêntico.

**Erros novos: `pairing_invalid` (409)** — único; enum `Error.code` cresce em 1.
**Sem migration** (D6). **Spec 0.9.0 → 0.10.0.**

**Delta interno (fora da OpenAPI — transporte snake_case):**
| Rota/body | Serviço | Delta |
|---|---|---|
| `HeartbeatBody.endpoint: String` + `.max_gpu_mib: Option<i64>` | orchestrator → manager | identidade (D1); `ORCH_ADVERTISE_URL` (default `http://orchestrator-local:8082`); `max_gpu_mib` = maior VRAM individual entre GPUs (capacidade de 1 job — §6) |
| `POST /internal/pairing/verify {code}` → `{valid:bool}` | orchestrator | single-use em memória; `ORCH_PAIRING_CODE` (ausente → gera no boot + loga) |
| `POST /internal/adopt {name, endpoint, kind, pairing_code}` → item completo | manager | upsert + verify no orquestrador (timeout 10s) |
| `POST /internal/orchestrators/:id/revoke` → 204 | manager | status `revoked` |
| `GET /internal/orchestrators` | manager | item ganha `{measured, cpu, ram, ram_total, vram_used, vram_total, vram_total_gb, gpus, jobs_active}` |
| `GET /internal/telemetry` | manager | agregação (0/1/>1 nós — D2.3) |
| `OrchestratorClient.post_json` (retorno de body) | manager | método novo p/ o verify do adopt (~15 linhas) |
| env `VRAM_TABLE_PATH` | manager | vram-table no boot (default compilado) |
| env `ORCH_WATCHDOG_DEGRADED_S`/`ORCH_WATCHDOG_OFFLINE_S` | manager | 15/60 |
| `POST /api/jobs/yolo` | principal | **zero delta** (requisito resolvido no manager — D3) |

## Testes

| Camada | Infra | Prova |
|---|---|---|
| `cargo test -p orchestrator` | unit (sem S3) | `HeartbeatBody` serializa `endpoint`; default de `ORCH_ADVERTISE_URL` = `http://orchestrator-local:8082`; `ORCH_PAIRING_CODE` ausente → gera no boot (env limpo) e presente → usa o do env; `/internal/pairing/verify`: código correto → `{valid:true}` e 2ª chamada → `{valid:false}` (single-use); código errado → `{valid:false}`; body inválido → 400 |
| `cargo test -p manager -- --ignored` + `scripts/test-db.sh` | Postgres do compose | **identidade**: 2 nós (local + remoto) heartbeating → `last_heartbeat` atualizado **só** na linha do endpoint do heartbeat; heartbeat de endpoint inexistente → warn + nada gravado; heartbeat revive `offline→online` e **não** revive `revoked`. **cache por nó**: 2 nós → `GET /internal/telemetry` = soma vram/jobsActive + união gpus + `cpu/ram/ram_total:null`; 1 nó → idêntico ao de hoje; 0 → fallback atual. **colunas**: heartbeat com `vram_total` (MiB) e `gpus` não-vazio → `vram_total_gb` = round(MiB/1024) e `gpus` gravados na linha. **roteamento**: 2 online (18GB e NULL) + job com requisito 8 → vai para o de 18; job sem requisito → NULL também elegível, `ORDER BY name` determinístico; nó com job não-terminal excluído (NOT EXISTS); requisito 12 + só 1660S (6) → `waiting_vram`; sem requisito + nenhum online → `waiting_slot`. **watchdog**: nó sem heartbeat 15s → `degraded`; 60s → `offline` + jobs `dispatched/preparing/running/cancelling` do nó → `queued recovered` com `orchestrator_id NULL`; heartbeat revive. **adopt/revoke**: verify válido → upsert `online` (2ª adoção idempotente, revive `revoked`); verify inválido → 409; revoke → `revoked`; dispatch não escolhe `revoked`; watchdog não toca `revoked`; auto-adoção não ressuscita `revoked` (boot com linha revoked + `AUTO_ADOPT_LOCAL=1` → linha segue revoked) |
| `cargo test -p api-principal` | MockStorage + pool lazy | `GET /api/orchestrators` mapeia os campos novos camelCase (incl. `vramTotalGb`, `ramTotal`); adopt 200 (upsert) / 400 (domínio) / 409 `pairing_invalid` (MockManager falha o verify) / 503 (manager fora); revoke 204 / 404 (id não-UUID) / 503; alias `/api/environments` ≡ handlers de orchestrators (mesmas respostas); inventário de rotas ≡ spec 0.10.0 (contract, 6 rotas) |
| `scripts/test-db.sh` (extensão) | Postgres do compose | ciclo ponta-a-ponta: adopt remoto (verify mockado no manager? **não** — o test-db testa o manager direto com verify stub) → dispatch → watchdog derruba → job re-queued → dispatch para o outro nó |
| E2E smoke manual (**fora do CI**) | compose + 2 orquestradores | critérios em "Riscos/verificação" — 2 nós online simultâneos, roteamento, telemetria estável, watchdog matando um nó, adopt/revoke via UI |

O CI cobre as baterias locais (pytest + cargo dos 3 + test-db + contract 0.10.0);
o smoke multi-nó é manual (mesma postura do @gpu da ADR-0010).

## Spike obrigatório? — **NÃO**

Nenhuma premissa externa não verificada: identidade (campo em body JSON nosso),
cache por nó (Rust puro), roteamento (SQL sobre colunas existentes), watchdog
(SQL + worker existente), adopt (protocolo nosso entre 2 serviços nossos). A única
adição de dependência é `serde_yaml` no manager (crate puro e estável, parse de
arquivo versionado no repo — não é premissa externa de comportamento). *Inverteria
(sem spike, só ajuste):* se `ORCH_ADVERTISE_URL` mal configurado for comum na
prática → log de warn vira log de erro com hint (R4); se o verify no orquestrador
for lento na LAN → timeout do adopt sobe de 10s (parâmetro).

## Riscos e contingências

- **R1 — Trap do NULL permissivo no roteamento:** com remoto offline e local mock
  online, um job GPU cai no mock (permissivo-último) — o trap do ADR-0010 R1
  ressurge. Mitigação em camadas: sessão GPU revoga o local (D8) + watchdog mostra
  o remoto `offline` honesto + ORDER BY prefere declarados. Residual: operador que
  não revoga o local na sessão → comportamento documentado (não silencioso: o job
  roda no mock e o artefato tem 110 bytes — o smoke confere tamanho, critério do
  ADR-0010 G.6 mantido).
- **R2 — Pairing code na LAN:** single-use em memória, mas código via env não tem
  TTL e o flag reseta no restart do orquestrador (reuso pós-restart). Aceito:
  LAN caseira + `MANAGER_TOKEN` compartilhado já é a postura atual (ADR-0010 D10);
  o código autentica a porta de entrada, não o tráfego. Dívida: `heph_o_*` por nó
  + TLS com pin (colunas prontas).
- **R3 — Re-queue em partição de rede = dupla execução:** nó vivo mas isolado →
  watchdog marca `offline` e re-queueia; o nó original pode terminar o job e o
  re-despacho rodar de novo. Mesmo risco do recovery de boot (semântica já aceita);
  artefatos last-write-wins por `job_id` (mesmos params/seed ⇒ mesmo md5 ⇒ benigno);
  dívida: detecção de "ainda vivo" (reconciliar report pós-re-queue).
- **R4 — `ORCH_ADVERTISE_URL` errado:** heartbeat órfão (warn no manager), nó sem
  `last_heartbeat` → `degraded/offline`; falha visível (UI mostra offline), operador
  corrige o env. Teste: smoke confere `GET /api/orchestrators` com
  `lastHeartbeat` fresco nos 2 nós.
- **R5 — Enriquecimento expõe telemetria por nó:** mesma superfície do
  `/api/telemetry` (dado que o manager já tinha); nenhum dado novo além do
  `vramTotalGb` (coluna já listada? **não** — `vram_total_gb` não era exposto;
  expor é delta documentado no D6). Sem risco de segurança adicional na LAN.
- **R6 — Unidades MiB vs GB:** conversão `round(MiB/1024)` no D1. O MiB do
  nvidia-smi é binário (1 MiB = 1024² B); 3060+1660S = 12288+6144 = **18432 MiB →
  round(18432/1024) = 18** — **confere** com o INSERT manual de 18GB da ADR-0010
  (que, sem dizer, usava GiB). Coluna `vram_total_gb` = GiB arredondado; nunca
  assumir MB decimais (nota MiB da dividas.md reafirmada).
- **R7 — Rollback e compat:** spec aditiva (campos novos ignorados por front
  antigo; rotas novas não chamadas por front antigo); `/api/telemetry` muda de
  semântica com >1 nó (front antigo vê `cpu:null` → "—", aditivo definido); sem
  migration, rollback = reverter commit.
- **R8 — test-db.sh varre `orchestrators`** (dívida conhecida): a varredura apaga
  linhas adotadas — mas agora o re-adopt é via API (revoke+adopt) em vez de INSERT
  manual; a regra mental "restart manager após test-db" permanece (manager re-adota
  o local no boot; o remoto precisa de adopt novo).
- **R9 — Watchdog derrubando nó lento:** 15s/60s com heartbeat de 2s deixa ~7
  heartbeats de folga; nó com GC/IO lento pode piscar `degraded` sem re-queue
  (só `offline` re-queueia) — seguro por desenho.

## O que fica falso nos docs (lista para o `@docs-sync`, commit H.7)

**Não aplicar agora** — docs descrevem o que existe, não o que foi aprovado.

- `backend.md` §8/:104-110 — "Adoção (decisão: token colado)…": **implementado em
  v1 com divergências conscientes**: pairing code `heph_p_*` (formato encorajado,
  não enforceado) verificado no orquestrador (single-use), upsert no manager;
  `heph_o_*`/TLS-pin/rotação **continuam pendentes** (colunas `token_hash`/
  `fingerprint` existem e ficam NULL); sem rate-limit; `revoke` = status
  `revoked` (tombstone) e não corte de credencial. Health-check 15s/2-5 falhas do
  §8/:111 → substituído pelo watchdog sobre heartbeat (15s/60s).
- `backend.md` §9/:160-165 — bloco orchestrators: `POST /adopt` e `POST /:id/revoke`
  **implementados** (spec 0.10.0); `enable/disable` e `GET /:id/health` permanecem
  pendentes; alias `/api/environments*` **implementado** (módulo da UI habilitado);
  a auto-adoção local (:166) ganha a guarda "não ressuscita `revoked`".
- `backend.md` §9/:167-169 — telemetria: nota da **agregação** (0/1/>1 nós — D2.3);
  `gpus[]`/`vram*` reais continuam (emenda G.7), agora **por nó** via lista
  enriquecida.
- `backend.md` §6/:89-93 — policy VRAM: emenda — **roteamento estático entra**
  (vram-table no manager, `waiting_vram` quando nenhum nó tem capacidade);
  paralelismo por VRAM livre/preempção/`max_parallel_trainers` permanecem dívida;
  nota `unmeasured`/`default_train_gb` não aplicado como requisito.
- `backend.md` §10/:282-283 — "`gpus`/`vram_total_gb` NUNCA são escritos no v1":
  **morre** — o heartbeat identificado os escreve (dinâmicos, GiB/1024
  arredondado); emenda da nota.
- `docs/adr/0009-web-integracao-monitoramento.md` D1 — "sem métricas por nó" +
  "coluna vazia ≠ dado" + "UI compõe gauges só com `items.length===1`": **as três
  morrem** (lista enriquecida; colunas preenchidas por heartbeat; gauges por nó no
  H.5). R1 (telemetria por orquestrador) → **quitada** (implementada).
- `docs/adr/0010-treino-real-gpu.md` D1 (INSERT manual preenche
  `gpus`/`vram_total_gb`) e D2 (contrato operacional: stop + DELETE/INSERT +
  `AUTO_ADOPT_LOCAL=0` + recreate): **emenda — contrato morto**; sessão GPU = stop
  local + revoke local + adopt remoto via API (D8). Guarda anti-mock (D2) e
  envelope (D9) permanecem. D9 "policy no-op" → emenda parcial (D3).
- `docs/adr/0007-jobs-v1.md` D9 ("policy no-op / sem `waiting_vram`") → emenda
  parcial (roteamento estático + `waiting_vram` quando sem capacidade).
- `frontend.md` §10/:225 (Ambientes pendente) → **implementado** (alias +
  módulo habilitado); §10/:64 (sidebar "Orquestradores, badge Roadmap,
  desabilitado") → habilitado; §10/:244 (telemetria) → nota da agregação + por nó;
  §10/:246 (`Orchestrator`) → campos novos.
- `dividas.md` — **QUITADAS**: "Telemetria por orquestrador" (ADR-0009 R1),
  "Watchdog de orquestrador" (F4.8), "POST /api/orchestrators/{adopt,rotate,revoke,
  …} + alias" (parcial: adopt/revoke + alias; rotate fica), "Roteamento por
  capacidade no manager" (parcial: estático; policy dinâmica fica). **Novas**:
  policy VRAM aplicada completa (paralelismo por nó, preempção, `max_parallel_
  trainers`); credenciais por nó `heph_o_*` + rotação + TLS com pin (colunas
  prontas); detecção de dupla execução (reconciliar report pós-re-queue); TTL/
  rate-limit do pairing code; nota "`vram_total_gb` agora é escrito pelo manager
  via heartbeat (GiB/1024)". Reafirmadas: test-db isolamento; reconciliar bucket×
  banco; MiB.
- `infra/README-gpu.md` — sessão GPU reescrita (sem psql; revoke+adopt via API/UI;
  `ORCH_ADVERTISE_URL` e `ORCH_PAIRING_CODE` no env.gpu).
- `coordenacao.md` — bloco da fatia H reescrito a cada commit.

## Plano de commits (H.0–H.7; branch `feat/orquestracao-robusta` de `main`)

Um dispatch = um commit; produção < 400 linhas por commit (contrato/testes fora da
conta — exceção da casa). **H.1 → H.2 → H.3 → H.4 SEQUENCIAIS** (o endpoint do
heartbeat é base do cache por nó, que é base do roteamento/watchdog, que é base do
BFF; todos no mesmo boundary Rust — ownership pode alternar entre
`@rust-dev`/`@fixer` mas a ordem é fixa). **H.5** (frontend, via /impeccable) só
depois do backend vivo (regra F4.7: rebuild + recreate antes de despachar UI).
**H.6** review; **H.7** docs-sync. Smoke multi-nó manual do coordenador após H.5
(NÃO é commit; fixes viram commits próprios).

| # | Dono | Conteúdo | Critério de pronto |
|---|---|---|---|
| **H.0** | @architect | Esta ADR (proposta; vira executável após aceite do usuário) | auditoria do coordenador; arquivo commitado em `main` |
| **H.1** | @rust-dev (orchestrator) | `HeartbeatBody.endpoint: String` + env `ORCH_ADVERTISE_URL` (default `http://orchestrator-local:8082`) + preenchimento no loop (~2s); pairing: env `ORCH_PAIRING_CODE` (ausente → gera no boot + `tracing::info` uma vez) + rota `POST /internal/pairing/verify` (single-use em memória, `{valid:bool}`; body inválido → 400) + tests (serialização do endpoint, default do env, generate-no-boot, verify single-use/errado/inválido) | `cargo test -p orchestrator` verde (42 existentes + novos); fmt limpo; ~200-260 linhas; mock intocado (tests antigos passam) |
| **H.2** | @rust-dev (manager) | `TelemetryCache` → `HashMap<Uuid, TelemetryState>`; `receive_heartbeat` resolve endpoint→id (warn se órfão), atualiza **só** a linha (status `online` exceto `revoked`), grava `gpus`/`vram_total_gb=round(MiB/1024)` quando o heartbeat carrega, cache por nó; `GET /internal/orchestrators` enriquecido (telemetria por nó + `vram_total_gb`); `GET /internal/telemetry` agregado (0/1/>1 — D2.3) + tests unit/db (2 nós, agregação, 1 nó = hoje, colunas gravadas) | `cargo test -p manager -- --ignored` + `bash scripts/test-db.sh` verdes (21 existentes + novos); fmt; ~300-380 linhas (**split H.2a cache+identidade / H.2b enrich+agregação se estourar — sem spec pública no meio**) |
| **H.3** | @rust-dev (manager) | Roteamento: dep `serde_yaml` + parse da vram-table no boot (`VRAM_TABLE_PATH`, default compilado; fail-fast) + SQL do D3.2 (busy-node NOT EXISTS + capacidade + ORDER BY determinístico) + `waiting_vram`; watchdog no worker loop (15s/60s, envs) + re-queue CTE (espelho `recover_jobs` escopado) + revive não-revoked; `POST /internal/adopt` (verify no orquestrador via `OrchestratorClient::post_json` novo) + `POST /internal/orchestrators/:id/revoke` + guarda na auto-adoção (`WHERE status <> 'revoked'`) + tests db (roteamento 4 casos, watchdog 3 casos, adopt/revoke, auto-adopt não ressuscita) | `cargo test -p manager -- --ignored` + test-db verdes; fmt; ~350-420 linhas (**split H.3a roteamento / H.3b watchdog+adopt se estourar**) |
| **H.4** | @rust-dev (principal) | BFF: `OrchestratorResponse` enriquecido (camelCase incl. `vramTotalGb`); `POST /api/orchestrators/adopt` (200 upsert/400/409 `pairing_invalid`/503) + `POST /:id/revoke` (204/404/503); alias `/api/environments` + `/api/environments/adopt` + `/api/environments/:id/revoke` (mesmos handlers); `ManagerPort`/`HttpManager`/`MockManager`: `adopt_orchestrator`/`revoke_orchestrator`; `PROTECTED_ROUTES` (6 entradas); spec **0.10.0** (rotas + `Orchestrator` enriquecido + `AdoptRequest` + `pairing_invalid` no enum) + units + contract | `cargo test -p api-principal` verde (188+14 existentes + novos); contract ≡ router 0.10.0; test-db verde; fmt; ~350-400 linhas (**split H.4a BFF+rotas / H.4b spec completa+testes se estourar**) |
| **H.5** | @frontend-dev (via /impeccable) | Página `/environments` (lista enriquecida, status chips, "visto há Xs", gauges por nó, modal Adotar com `pairingCode`, Revogar com ConfirmDialog, empty state com adopt); dashboard: gauges por nó (regra `items.length===1` morre; 1 nó = visual atual); `lib/monitoring.ts`: `Orchestrator` enriquecido + `adoptOrchestrator` + `revokeOrchestrator` (erros `pairing_invalid`/503 mapeados); Sidebar: módulo "Orquestradores" habilitado (badge Roadmap sai) + chip multi-nó | `npm run build --workspace=web` verde; 1 página = 1 review (DESIGN.md como contrato); console limpo no smoke Chrome; ~350-450 linhas (split por página se necessário) |
| **H.6** | @reviewer | review do diff H.1–H.5 vs esta ADR (pontos de atenção: mock intocado em H.1; semântica de agregação em H.2; NULL permissivo + unidades GiB/MiB em H.2/H.3; re-queue em partição de rede; revoke=tombstone vs DELETE; `pairing_invalid` única exceção do D7 ADR-0009; spec ≡ router) | APROVA (com ou sem nits); fixes roteados como commits próprios |
| **H.7** | @docs-sync | Aplica "O que fica falso nos docs" (backend.md §8/§9/§6/§10, emendas ADR-0007 D9/ADR-0009 D1/ADR-0010 D1/D2/D9, frontend.md §10, dividas.md — quita 4 + novas, README-gpu — sessão sem psql, coordenacao.md) | diff só de docs; conferência doc↔código nos dois sentidos (lição sessão 16) |

**Notas de processo:** nenhum passo pode quebrar o caminho mock — critério
"mock intocado" testado pelas baterias existentes (orchestrator 42, manager db
21+, contract 0.9.0→0.10.0). O smoke multi-nó é manual (coordenador, fora do CI):
**2 orquestradores online SIMULTÂNEOS** (local mock + remoto com capacidade
declarada via adopt), roteamento provado (`job.orchestrator_id` = uuid do remoto
para job GPU), telemetria **estável** (cards por nó sem oscilação; agregado com
`cpu:null` quando 2 nós), watchdog provado matando um nó (status `offline` + job
re-queued `recovered` + job roda no outro nó), adopt/revoke via UI, teardown
(revoke remoto + start local + adopt local). `cargo fmt --all` antes de reportar;
implementador que achar problema FORA do escopo **para e reporta** (norma das
fatias 4/5). Atenção ao R6 (unidades) e ao R1 (não "consertar" o NULL permissivo
fora da ADR).
