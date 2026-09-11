# ADR-0015 — Visibilidade e seleção de nó: nome legível no job + seletor opcional com fallback honesto (Fatia N)

- **Status:** **PROPOSTA** (aguarda aceite do usuário). Nada implementado. Este
  documento é a especificação executável da fatia N ("Visibilidade e seleção de
  nó"); os deltas de contrato abaixo são aplicados **apenas nos commits da
  fatia** (openapi junto do código, docs de texto no `docs-sync` do fim),
  nunca antes.
- **Data:** 2026-09-11
- **Componentes:** `services/manager` (JOIN `orchestrators` em `list_jobs`/
  `get_job` para `name`/`kind`; `create_job` aceita `orchestrator_hint` e grava
  em `params` JSONB; `dispatch_next` com preferência de nó + fallback
  automático com flag), `services/api-principal` (BFF: 3 requests ganham
  `orchestratorId?`, JobResponse ganha `orchestratorName`/`orchestratorKind`/
  `orchestratorFallback`, alinhamento do mapeamento de erro dos 3 submits —
  spec **0.12.0 → 0.13.0**), `apps/web` (N.4–N.7: componente `NodeSelect` +
  "Executado em" no detalhe do `/jobs` + seletor em `/playground`, `/treino` e
  `AutoTrackerModal` — via /impeccable, 1 página = 1 review).
- **Fontes:** `IDEIA.md` §1/:18-21 (orquestrador "decide/gerencia onde o treino
  vai rodar (local ou remoto)") e §1/:52-54 (local = GPU do usuário; remoto =
  VPS/RunPod com GPUs mais potentes — a decisão local-vs-remoto é hoje **por
  capacidade**, ADR-0011 D3.4); `docs/dividas.md` :55 (**dívida "Seleção
  manual de nó/GPU na UI — ABERTA 2026-09-10"**: "campo `orchestratorId?` no
  create_job + seletor em /treino, com validação de capacidade e fallback
  honesto quando o nó escolhido não tem slot" — **o que esta fatia executa**);
  `docs/adr/0011-orquestracao-robusta.md` D3 (roteamento estático por
  capacidade — SQL determinístico, requisito resolvido no manager,
  `waiting_vram`), D4 (watchdog + re-queue por CTE), D1/D2 (`/api/orchestrators`
  enriquecido com telemetria por nó — `name`/`kind`/`vramTotalGb` no wire);
  `docs/adr/0009-web-integracao-monitoramento.md` D1 (lista de orquestradores
  real); `docs/adr/0012-models-real.md` D5 (padrão `weights_id` → resolução
  fail-fast no `create_job` do manager: inexistente → 404, engine errado →
  400); `docs/adr/0013-playground-inferencia.md` D8 (spec 0.11.0 → 0.12.0) e
  **R6** (mapeamento `NotFound→404`/`InvalidRequest→400`/`Unavailable→503` do
  `submit_predict_job` — os submits yolo/autotracker ainda mapeiam `Err(_)→503`);
  `docs/backend.md` §9/:146-151 (submits implementados), §9/:161-167 (leitura),
  §6/:78-92 (policy VRAM), §10 (schema `jobs`); `docs/frontend.md` §10/:235-238
  (`startYoloJob`), :242 (autotracker), :262-263 (predict/playground).
  Código (verificado por graft/grep nesta data): `services/manager/src/lib.rs`
  — `list_jobs` L433-563 (SELECT L450 **sem JOIN** com `orchestrators`),
  `get_job` L566-619 (SELECT L584 idem), `JobRow` L85-103 (só
  `orchestrator_id`), `create_job` L344-426 (merge de `package_ref` L352-359 e
  `weights_id→weights_ref` L361-393 → **404 inexistente / 400 engine≠yolo**,
  INSERT em `jobs.params` JSONB L395-410), `dispatch_next` L1736-1865 (FIFO
  L1752-1758, requisito da vram-table L1766, SQL elegível ADR-0011 D3.2
  L1769-1784, sem elegível → `waiting_vram`/`waiting_slot` L1786-1803, UPDATE
  dispatched zera `queue_reason` L1806-1814, payload do dispatch **não inclui
  `params`** L1833-1848), `recover_jobs` L1538-1548 e watchdog CTE L1578-1592
  (re-queue **não toca `params`**); `services/manager/src/main.rs`
  `create_job_handler` L184-206 (mapeia `NotFound→404`, `InvalidRequest→400`);
  `services/api-principal/src/jobs/manager_client.rs` (`InternalJob` L46-64,
  `create_job` via body JSON L384-416, `list_orchestrators` L444-451),
  `src/jobs/handlers.rs` (`JobResponse` L81-99, `to_job_response` L276-297,
  `submit_yolo_job` L490-660 — **`Err(_)→503` L648**, `submit_autotracker_job`
  L670-818 — **`Err(_)→503` L806**, `submit_predict_job` L831-975 — **R6:
  `NotFound→404` L957, `InvalidRequest→400` L961**), `src/jobs/models.rs`
  (`YoloJobRequest` L39-59, `AutotrackerJobRequest` L104-110,
  `PredictJobRequest` L277-284 — todos `deny_unknown_fields`);
  `packages/contracts/openapi.yaml` (**version 0.12.0** L4; `Job` L2801-2865 —
  `orchestratorId` L2857); migrations existentes: `services/api-principal/
  migrations/` **0001..0007** (0008 **não existe** em lugar nenhum do repo);
  `apps/web/types/studio.ts` (`Job` L206-224 — `orchestratorId` L221),
  `apps/web/lib/jobs.ts` (`startYoloJob` L13-29), `lib/playground.ts`
  (`startPredictJob` L11), `lib/autotracker.ts` (`startAutotrackerJob` L5),
  `lib/monitoring.ts` (`listOrchestrators` L120 → `GET /api/environments`),
  `apps/web/app/(studio)/jobs/page.tsx` (hero do detalhe L469-532 — meta
  `kind · engine` L477), `components/studio/ForjaYoloSetup.tsx`
  (`handleSubmit` L276-304), `components/studio/AutoTrackerModal.tsx`
  (`handleSubmit` L52-67, aberto da galeria `datasets/[id]/page.tsx`),
  `components/studio/TrainYoloModal.tsx` (galeria — **intocado**, Q1 F6.3).
- **Sequência:** Fatia J (mergeada) → R1 (mergeada) → **N (esta)** → Fatia K
  (AutoTracker real) → dívidas registradas.

## Contexto

O roteamento por capacidade da Fatia H (ADR-0011 D3) resolveu *para onde* o
manager despacha, mas deixou dois buracos de produto que o usuário fechou na
abertura da fatia I (dividas.md :55) e reafirma agora:

1. **(a) Visibilidade:** o wire do job carrega só `orchestratorId` (UUID —
   `Job.orchestratorId`, openapi L2857) — sem nome legível, e a UI do `/jobs`
   não renderiza o campo. O usuário não consegue ver **em qual nó** um job
   rodou. O JOIN é trivial: a tabela `orchestrators` (dona = manager) tem
   `name`/`kind` e o manager é quem monta o SELECT — falta `LEFT JOIN` +
   2 colunas.
2. **(b) Seleção:** não existe "escolher orquestrador/GPU" no POST de job nem
   na UI. A dívida registrada pede exatamente: "campo `orchestratorId?` no
   create_job + seletor em /treino, com validação de capacidade e fallback
   honesto quando o nó escolhido não tem slot".

A fatia entrega **visibilidade sempre-on** (independe de seleção) + **seletor
opcional** (default "Automático" — comportamento atual intacto) com fallback
honesto. Não mexe em nada do caminho GPU: a guarda anti-mock (ADR-0010 D2), a
vram-table e `TRAINER_IMAGE` seguem intocados (D3).

Regras da casa aplicadas: wire camelCase em `/api/*`, transporte interno
snake_case (ADR-0002 D1); erros `{code,message}` — **nenhum `Error.code`
novo** (padrão das fatias I/J/K); 404 `not_found` para id de recurso inexistente
(ADR-0002 D8); dev CPU-only (`ENGINE_MOCK=1`); PRs < ~400 linhas de produção;
branch `feat/visibilidade-no`; frontend sempre via /impeccable, 1 página = 1
review; "nenhum número inventado" (R5 ADR-0009).

## Decisões já travadas (base — citar, não redecidir)

- **`orchestrators` é tabela do manager; o principal é BFF puro e nunca lê as
  tabelas de jobs/orchestrators** (ADR-0007 D3/D8; backend.md §1/:31-32). A
  validação do hint no submit segue o padrão `weights_id` (ADR-0012 D5): o
  principal repassa, o **manager resolve** (`create_job` → 404/400).
- **Roteamento = SQL determinístico por capacidade no `dispatch_next`**
  (ADR-0011 D3.2): nó `online` + sem job não-terminal + `vram_total_gb >=
  required` (ou NULL permissivo), `ORDER BY` capacidade DESC + nome; sem nó
  elegível → `queue_reason = waiting_vram|waiting_slot`. **O requisito VRAM é
  resolvido no MANAGER em tempo de dispatch** (vram-table — ADR-0011 D3.1) —
  o principal não conhece a policy e **não** a duplicará nesta fatia (D2/D3).
- **`queue_reason` cobre só o estado `queued`** — é zerado no UPDATE de
  dispatch (`lib.rs:1807`); **não serve** para carregar semântica
  "escolhi X mas rodou Y" pós-dispatch (confirmado — D3 usa `params`).
- **Watchdog/recovery re-queueia sem tocar `params`** (CTE `lib.rs:1578-1592`
  e `recover_jobs` L1538-1548 só UPDATE `status/queue_reason/orchestrator_id`)
  — base do D4 (hint sobrevive ao re-queue).
- **`GET /api/orchestrators` (alias `/api/environments`) já entrega
  `name`/`kind`/`status`/`vramTotalGb`** por nó (ADR-0011 D2/D7) — o seletor
  da UI consome o endpoint **existente**, sem rota nova.
- **Espec OpenAPI atual: 0.12.0** (`openapi.yaml:4`; Fatia J landou 0.12.0 —
  ADR-0013 D8). **Correção de fato do enunciado:** o disco está em 0.12.0, não
  0.13.0; migrations existentes: 0001–0007 (0008 não existe — a menção em
  dividas.md :89 é a hipótese da Fatia K para `boxes.origin`). D6 recalibra.
- **TrainYoloModal da galeria INTOCADO** (decisão Q1/F6.3) — envia default
  Automático; só as 3 superfícies do D5 ganham seletor.
- **Guarda anti-mock + `TRAINER_IMAGE` vigem** (ADR-0010 D2) — o hint não
  abre caminho para rodar mock no nó GPU: as condições do SQL de dispatch são
  as mesmas; a recusa do orquestrador remoto (`:local` × `ORCH_GPU_DEVICES`)
  continua valendo.

## Decisões

### D0 — Escopo da fatia N (entra/sai)

**Decidido — entra:**
- **N.1 — Visibilidade sempre-on:** `orchestratorName` + `orchestratorKind`
  no wire do job (JOIN no manager; aditivo camelCase; `null` quando o job não
  foi despachado ou o nó foi removido — `orchestrator_id` já é `ON DELETE SET
  NULL`) + renderização nos 3 lugares: detalhe do `/jobs` ("Executado em"),
  histórico do `/playground` e lista/detalhe do `/treino` (o detalhe é o
  mesmo componente do `/jobs` — a lista é tile compacta).
- **N.2 — Seletor opcional de nó:** campo `orchestratorId?` (uuid) nos 3
  POSTs públicos (`/api/jobs/yolo|predict|autotracker`) + dropdown "Nó"
  (default **"Automático (recomendado)"**) nas 3 superfícies de submit
  (`/treino` ForjaYoloSetup, `/playground`, `AutoTrackerModal` — da galeria).
  **Fallback automático honesto** quando o nó escolhido não puder ser usado
  no momento do dispatch.

**Decidido — fora (dívida/futuro, NÃO agora):**
- **Cache de modelos por nó** (inventário models×nodes — a pergunta "onde o
  peso X está baixado" continua dívida; o staging de pesos do orquestrador
  baixa sob demanda — ADR-0012 D5 — e o hint não muda isso).
- **Múltipla seleção / afinidade / grupo de nós** (ex.: "qualquer remoto") e
  **agendamento por janela** (rodar só em horário X).
- **Filtro por `kind` no roteamento** (ADR-0011 D3.4 deixou como extensão
  trivial — continua fora; `kind` entra no wire e no rótulo da UI **só como
  legibilidade**).
- **Policy VRAM completa** (paralelismo por VRAM livre, preempção,
  `max_parallel_trainers`) — dívida da ADR-0011 D3, intocada.
- **`TrainYoloModal` da galeria**: sem seletor, default Automático (Q1/F6.3 —
  "INTOCADO").

**Descartado:** validar capacidade do nó escolhido no submit do principal
(duplicaria a policy VRAM fora do dono — ADR-0011 D3.1; o requisito é
derivado da vram-table no manager; ver D2); coluna nova
`jobs.orchestrator_hint` com FK (hint em `params` JSONB é reversível, zero
DDL e o nó pode morrer/revogar sem amarrar o job — D2); "sem fallback, erro
se o nó escolhido não servir no dispatch" (hostil: nó pisca `degraded`/
`offline` pelo watchdog 15s/60s e a fila é FIFO — o job ficaria preso à toa;
o fallback com flag é o comportamento honesto).

### D1 — Wire do job: `orchestratorName` + `orchestratorKind` (+ `orchestratorFallback`)

**FATO (código):** `list_jobs` SELECT L450 e `get_job` SELECT L584 não fazem
JOIN com `orchestrators`; `JobRow` L85-103 expõe só `orchestrator_id`;
`InternalJob` L46-64 → `to_job_response` L276-297 → `JobResponse` L81-99 →
spec `Job` L2801-2865 → `types/studio.ts` `Job` L206-224. Cadeia de 6
camadas, todas aditivas.

**Decidido — caminho confirmado (D1):**
1. **Manager:** `list_jobs`/`get_job` ganham
   `LEFT JOIN orchestrators o ON o.id = j.orchestrator_id` e selecionam
   `o.name AS orchestrator_name, o.kind AS orchestrator_kind` + a expressão
   `(j.params->>'orchestrator_fallback') = 'true' AS orchestrator_fallback`
   (bool — D3). `JobRow` ganha `orchestrator_name: Option<String>`,
   `orchestrator_kind: Option<String>`, `orchestrator_fallback: bool`
   (`#[serde(default)]` para compat com consumidores antigos do
   `/internal/jobs`). Nó removido (`ON DELETE SET NULL`) ou job não
   despachado → `null`/`false` (nenhum número/string inventado).
2. **Principal:** `InternalJob` ganha os 3 campos (`#[serde(default)]` no
   fallback); `JobResponse` ganha `orchestratorName`/`orchestratorKind`
   (`Option<String>`) e `orchestratorFallback: bool` (camelCase via
   `rename_all`); `to_job_response` repassa.
3. **Spec 0.13.0** e **`types/studio.ts`**: `Job` ganha `orchestratorName:
   string|null`, `orchestratorKind: string|null`, `orchestratorFallback:
   boolean`.

*Por quê — `kind` no wire:* o rótulo "Executado em" sem o tipo seria
ambíguo entre local (mock) e remoto (GPU real) — o usuário precisa saber se o
job rodou no nó com GPU; `kind` já existe na tabela e custa 1 coluna no JOIN.
*Por quê — fallback bool no wire (e não só no params):* `params` **não** é
exposto no wire do job (InternalJob não o carrega); sem o bool a UI não teria
como distinguir "pedi X, rodou em X" de "pedi X, caiu em Y" — a semântica do
D3 ficaria invisível, exatamente o que a fatia quer matar. *Gotcha:* o nome
do nó **real** vem do JOIN (`orchestrators` é dona do manager); o nó
**solicitado** fica só em `params` (sem FK) — ver D2/D3.

### D2 — Transporte da escolha: `orchestratorId?` nos 3 POSTs; hint em `params` (SEM migration)

**Decidido — opção `params` (a proposta do enunciado; reversível):**
1. **Público:** os 3 requests (`YoloJobRequest`, `AutotrackerJobRequest`,
   `PredictJobRequest` — todos `deny_unknown_fields`) ganham
   `orchestrator_id: Option<String>` (`#[serde(rename_all = "camelCase")]` →
   `orchestratorId?` no wire; `deny_unknown_fields` **exige** adicionar o
   campo aos 3 structs — sem isso, o campo novo seria 400). Ausente/`null` =
   Automático.
2. **Validação de forma (principal, `models.rs`):** presente e não-UUID →
   400 `invalid_request` (precedente `weights` — ADR-0012 D5: referência
   opcional de recurso no body ⇒ 400; divergência consciente do "id não-UUID
   → 404" do D8 ADR-0002, que vale para ids de **recurso da rota**, não campo
   opcional de body).
3. **Transporte interno:** os 3 `manager_body` ganham
   `"orchestrator_hint": <uuid>` (snake_case, top-level no body do
   `/internal/jobs`). `CreateJobRequest` ganha
   `orchestrator_hint: Option<String>` (`#[serde(default)]` — cliente antigo
   segue válido).
4. **Resolução fail-fast NO SUBMIT, no MANAGER** (padrão `weights_id`,
   ADR-0012 D5 — o dono de `orchestrators` resolve, o BFF repassa):
   no `create_job`, quando `orchestrator_hint` presente:
   - não-UUID → `Err(ManagerError::InvalidRequest(...))` → **400**
     `invalid_request` (dupla defesa; o principal já validou);
   - UUID sem linha em `orchestrators` → `Err(ManagerError::NotFound)` →
     **404 `not_found`** (nó inexistente — id válido mas recurso sumido;
     precedente weights inexistente → 404);
   - linha com `status <> 'online'` (`revoked`/`offline`/`degraded`/`unknown`)
     → `Err(ManagerError::InvalidRequest("orchestrator not available"))` →
     **400 `invalid_request`** com **mensagem estática honesta**
     ("nó de execução indisponível (offline ou revogado) — escolha outro ou
     Automático"). **Não uso 409**: no padrão da casa 409 é conflito de
     estado de recurso **em mutação** (`dataset_not_ready`, `job_not_done`,
     `job_not_abortable`, `pairing_invalid`); escolher um nó indisponível é
     erro de domínio do request — casa com `invalid_request` (precedentes:
     model∉{mock}, conf fora de domínio, weights não-UUID). **Sem erro novo.**
   - ok → `params["orchestrator_hint"] = json!(hint)` (mesmo merge do
     `weights_ref` L380-383) e INSERT normal.
5. **Capacidade NÃO é validada no submit** (ver D3): o requisito VRAM é
   derivado da vram-table no manager em tempo de dispatch (ADR-0011 D3.1);
   duplicá-lo no principal violaria o boundary e congelaria a policy no
   cliente. A UI mostra `vramTotalGb` por nó como **informação** (D5) e o
   dispatch decide.

*Por quê — validação no manager e não no principal via `list_orchestrators`:*
fonte única de verdade (o manager é dono da tabela); sem round-trip extra no
submit do BFF; reusa o mapeamento `NotFound→404`/`InvalidRequest→400` que o
`create_job_handler` **já tem** (main.rs L184-206) e que o `submit_predict_job`
já aplica (R6 ADR-0013). *Por quê — params e não coluna:* zero DDL, reversível
com 1 commit; o hint é metadado de orquestração (nunca vira condição SQL de
filtro/índice na v1 — sem consumidor para isso); `jobs.params` já é o lugar
dos metadados de dispatch (`package_ref`, `weights_ref`); **o payload do
dispatch não inclui `params`** (L1833-1848) — o hint **não vaza** para o
orquestrador. *Gotcha:* sem FK, um hint órfão (nó DELETE — não existe path de
DELETE em produto; revoke é tombstone) só é pego no **dispatch** (D3) e no
**submit** (404); o 404 do submit é a defesa do caso comum.

### D3 — Dispatch com hint: preferência no 1º nível, fallback automático honesto

**FATO (código):** `dispatch_next` L1736-1865 — FIFO, requisito da vram-table,
SQL elegível (ADR-0011 D3.2), `waiting_vram|waiting_slot`, UPDATE dispatched
**zera `queue_reason`** (L1807). A semântica "escolhi X mas foi Y" **não
cabe** em `queue_reason` (confirmado: ela morre no dispatch) — o D1 já levou
a flag para o wire.

**Decidido (o SQL do D3 ADR-0011 vira o *fallback*; o hint é preferência de
1º nível, sem mudar a policy):**
1. No `dispatch_next`, após resolver o requisito, extrai
   `params["orchestrator_hint"]` (string uuid) do job selecionado. Sem hint →
   comportamento atual **byte a byte** (regressão Automático = bateria).
2. Com hint → lookup do nó pedido com as MESMAS condições do roteamento
   (nada de janela de exceção):
   ```sql
   SELECT o.id, o.endpoint FROM orchestrators o
   WHERE o.id = $1 AND o.status = 'online'
     AND NOT EXISTS (SELECT 1 FROM jobs j
                     WHERE j.orchestrator_id = o.id
                       AND j.status IN ('dispatched','preparing','running','cancelling'))
     AND ($2::int IS NULL OR o.vram_total_gb IS NULL OR o.vram_total_gb >= $2)
   ```
   com `$1 = hint`, `$2 = required_gb`. **Encontrou** → dispatch para ele
   (UPDATE dispatched com `orchestrator_id = hint` + **remove**
   `params.orchestrator_fallback` → `params - 'orchestrator_fallback'`).
3. **Não encontrou** (offline/revogado/ocupado/sem capacidade) → **fallback
   automático**: o SQL elegível existente (D3.2 ADR-0011) roda normal, com a
   flag honesta: o UPDATE dispatched ganha
   `params = jsonb_set(params, '{orchestrator_fallback}', 'true')`. Se nem o
   fallback achar nó → `waiting_vram|waiting_slot` como hoje (o hint **não**
   muda o motivo: com requisito presente e nenhum nó com capacidade →
   `waiting_vram`; sem requisito e nenhum online → `waiting_slot`). Nesse
   caso a flag **não** é gravada (o job nem despachou; o `queue_reason` já é
   o sinal honesto na UI).
4. **Guarda anti-mock, vram-table e `TRAINER_IMAGE` intocados**: o hint só
   restringe *qual* nó do conjunto elegível; as condições de capacidade e o
   caminho GPU do orquestrador são os mesmos. Um hint apontando para o nó
   remoto com a guarda ativa se comporta igual ao roteamento automático (o
   orquestrador recusa — `failed` honesto).
5. **Flag re-escrita a cada dispatch**: remove quando honrado, grava `true`
   quando caiu no fallback — o wire `orchestratorFallback` reflete o ÚLTIMO
   dispatch (D4).

*Por quê — fallback e não erro no dispatch:* o nó escolhido pode piscar
(`degraded` 15s/`offline` 60s — watchdog) ou ficar ocupado entre o submit e o
despacho (fila FIFO); falhar o job seria hostil e o "slot" do nó escolhido é
exatamente o caso que a dívida pede para resolver com fallback honesto. O
badge na UI (D5) torna a troca visível — ninguém é enganado. *Por quê —
capacidade só no dispatch:* ADR-0011 D3.1 (requisito derivado da vram-table
no manager); o submit não sabe o `required_gb` do job (e não deve — policy é
do manager). *Descartado:* fila dedicada por nó escolhido (o fallback cobre o
mesmo caso com menos estado); "hint como filtro duro" (sem fallback = jobs
presos quando o nó pisca).

### D4 — Recovery/watchdog: o hint SOBREVIVE ao re-queue

**FATO (código):** `recover_jobs` L1538-1548 e o CTE do watchdog L1578-1592
re-queueiam `dispatched/preparing/running/cancelling` → `queued recovered`
**sem tocar `params`** — o `orchestrator_hint` permanece no JSONB.

**Decidido:** **não** zerar o hint no re-queue. Comportamento: o job volta à
fila, o `dispatch_next` re-tenta o hint (D3) — se o nó voltou a `online`
(heartbeat revive — ADR-0011 D4.4), o job volta para ele (intenção do usuário
preservada); se não, fallback automático com a flag re-escrita (D3.5) — o
detalhe do job mostra "Executado em: Y" + badge. *Por quê — sobrevive:* o
re-queue acontece porque o nó **morreu**; apagar o hint depois disso seria
punir o usuário duas vezes e perder a intenção original (o nó pode ter sido
só um soluço); o mecanismo de fallback (D3) já torna seguro re-tentar. O caso
"nó morto permanentemente" cai no fallback a cada tentativa **com badge
visível** — comportamento simples e honesto, sem estado novo. *Gotcha:*
nenhuma mudança nos 2 SQLs de re-queue (0 linhas tocadas); a flag é
re-escrita pelo próximo dispatch.

### D5 — UI: `NodeSelect` reutilizável + "Executado em" no detalhe (via /impeccable)

**Decidido:**
1. **Componente novo `NodeSelect`** (`apps/web/components/studio/`):
   dropdown de nó com as opções:
   - `""` → **"Automático (recomendado)"** (default — comportamento atual);
   - cada orquestrador com `status === "online"` de `listOrchestrators()`
     (`GET /api/environments` — endpoint **existente**, ADR-0011 D2),
     rotulado `name · kind · VRAM <vramTotalGb> GB` (VRAM só quando não-null;
     `vramTotalGb: null` → "—" — **nenhum número inventado**).
   - `listOrchestrators()` falhar (503/401) ou devolver 0 online → só
     "Automático" + nota discreta "Seletor indisponível — roteamento
     automático" (nunca bloqueia o submit).
   - Estados fora de `online` (degraded/offline/revoked) **não** são opções
     (o submit as recusaria com 400/404 de qualquer forma — D2).
2. **Superfícies de submit (N.6/N.7):** `/playground` (coluna de controle,
   abaixo do slider conf — ADR-0013 D7), `/treino` (ForjaYoloSetup — junto
   do seletor de pesos), `AutoTrackerModal` (galeria — junto do slider conf).
   As 3 libs (`startYoloJob`/`startPredictJob`/`startAutotrackerJob`) ganham
   `orchestratorId?: string | null` (omissão = Automático). Erros honestos do
   submit: 404 → "Nó não encontrado (pode ter sido removido)"; 400
   `invalid_request` → a mensagem do wire quando for do nó (o front mapeia
   por código, padrão `orchestratorErrorMessage` — monitoring.ts L145-156).
3. **Visibilidade (N.5 — independe do seletor):** no hero do detalhe do
   `/jobs` (L469-532), a linha de meta ganha **"Executado em: `name`
   (`kind`)"** quando `orchestratorName` presente (job despachado);
   `orchestratorFallback === true` → badge/subtexto discreto **"nó solicitado
   indisponível no momento — roteado automaticamente"** (texto genérico cobre
   offline e capacidade — D3); job `queued` sem nó → nada (honesto). O
   histórico do `/playground` e a lista do `/treino` ganham o nome em
   subtexto do card/tile (mesma fonte).
4. **Reviews:** 1 página = 1 review (regra da casa) → proponho **3 reviews**:
   `/jobs` (N.5), `/playground` (N.6), `/treino` + `AutoTrackerModal` (N.7 —
   agrupados: mesma integração de form, componente já revisado; se o usuário
   preferir 4, N.7 divide em N.7a/N.7b). `TrainYoloModal` **intocado** (D0).

*Por quê — componente reutilizável:* 3 superfícies + 1 detalhe, mesma
semântica (lista online + Automático + falha honesta) — um componente, uma
revisão de base, e o `NodeSelect` vira o ponto único quando a galeria
(`TrainYoloModal`) ganhar seletor no futuro. *Por quê — só online no
dropdown:* o submit recusa não-online (D2) — oferecer degradado/offline seria
rota de erro garantida. *Gotcha:* a lista do seletor é um snapshot — o nó
pode cair entre o load e o submit (race); o 400 honesto do D2 + o fallback do
D3 cobrem (duas camadas, nenhuma silenciosa).

### D6 — Contrato: spec 0.12.0 → 0.13.0; **sem migration**; sem erro novo

**Decidido:**
1. **Spec 0.12.0 → 0.13.0** (regra "versão = ordem de landing", ADR-0005 D1).
   **Correção de fato:** o enunciado afirmava "0.13.0 vigente" — o
   `openapi.yaml:4` está em **0.12.0** (Fatia J landou 0.12.0, ADR-0013 D8;
   Fatia K ainda não mergeou). Se a Fatia K landar **antes** desta, ela toma
   0.13.0 e esta recalibra para **0.14.0** (ponto de decisão §final).
2. **Delta público (aditivo):** os 3 schemas de request ganham
   `orchestratorId: string (uuid, format)` **opcional** (ausente = Automático;
   `additionalProperties: false` mantido); `Job` ganha
   `orchestratorName: string|null`, `orchestratorKind: string|null`,
   `orchestratorFallback: boolean` (default false). **Nenhum `Error.code`
   novo** (enum inalterada — reusa `invalid_request`/`not_found`/
   `queue_unavailable`); **nenhum status novo** nos submits (202/400/401/404/
   409/503 já existem — o 404/400 do hint reusa os status declarados).
3. **Migration: NENHUMA** (confirmado): hint e fallback vivem em
   `jobs.params` (JSONB, migration 0006); o JOIN usa colunas existentes de
   `orchestrators`; migrations atuais 0001–0007 (0008 não existe — não é
   necessária aqui). `test-db.sh` não muda de schema — ganha casos.
4. **Alinhamento de mapeamento (necessário para o 404/400 do hint
   atravessar os 3 submits):** `submit_yolo_job` (L648) e
   `submit_autotracker_job` (L806) mapeiam `Err(_)→503` — engolem o
   `NotFound`/`InvalidRequest` do manager (gap pré-existente vs ADR-0012
   :373, que já documenta 404/400 para weights; o `submit_predict_job` já
   faz o mapeamento R6). N.3 alinha os 2 submits ao padrão R6
   (`NotFound→404`, `InvalidRequest→400`, `Unavailable→503` — com a
   compensação do package em todos os caminhos de erro, como já é feito).
5. **Delta interno (fora da OpenAPI — transporte snake_case):**
   `CreateJobRequest.orchestrator_hint: Option<String>` (`#[serde(default)]`);
   `JobRow`/resposta interna ganham `orchestrator_name`/`orchestrator_kind`/
   `orchestrator_fallback`; `params.orchestrator_hint` e
   `params.orchestrator_fallback` (JSONB, semântica definida em D2/D3/D4).

### D7 — Verificação

| Camada | Infra | Prova |
|---|---|---|
| `cargo test -p manager -- --ignored` + `scripts/test-db.sh` | Postgres do compose | **wire**: job despachado → `list_jobs`/`get_job` com `orchestrator_name`/`orchestrator_kind` do JOIN; job `queued` ou nó deletado → `null`; `orchestrator_fallback` default `false`. **hint no submit**: `orchestrator_hint` não-UUID → 400; UUID inexistente → 404; `revoked`/`offline` → 400 (create_job); hint ok → `params.orchestrator_hint` gravado no INSERT. **dispatch**: hint online+livre+capacidade → despacha para ele, `params` sem fallback; hint offline/revogado → fallback automático (outro nó ou `waiting_vram`/`waiting_slot`) + `params.orchestrator_fallback=true`; hint ocupado (job não-terminal no nó) → fallback; hint sem requisito (`required_gb` NULL) → honrado (permissivo); sem hint → **comportamento atual byte a byte** (regressão). **re-queue**: watchdog/recovery com hint → `params` preservados, re-dispatch re-tenta o hint; flag re-escrita (honrado → removida) |
| `cargo test -p api-principal` | MockStorage + pool lazy | 3 requests aceitam `orchestratorId` (ausente default); não-UUID → 400; `manager_body` carrega `orchestrator_hint`; mapeamento: manager `NotFound→404`, `InvalidRequest→400`, `Unavailable→503` nos **3** submits (yolo/autotracker alinhados ao R6) com compensação do package em todos os erros; `to_job_response` mapeia `orchestratorName/Kind/Fallback`; MockManager com hint: submit feliz → `CreateJobRequest` recebe o hint; **contract: spec 0.13.0 ≡ router** |
| `scripts/test-db.sh` (extensão) | Postgres do compose | ciclo ponta-a-ponta com 2 nós (ambos `online`): hint → nó A; hint morto (watchdog) → fallback para B com flag; re-queue preserva hint; `GET /api/jobs/:id` expõe os 3 campos novos |
| E2E smoke manual (**fora do CI**) | compose `ENGINE_MOCK=1` + Chrome | submit com hint válido → job roda no nó (detail mostra "Executado em"); hint inexistente → 404 honesto; nó revogado → 400 honesto; nó offline no dispatch → fallback com badge; regressão Automático (submit sem campo → roteamento por capacidade); seletor com 1 nó e com 0 nós online; console limpo. Sessão GPU **opcional** (roteamento já provado — ADR-0011 H.7; hint com 2 nós é local) |

O CI cobre as baterias locais (pytest + cargo dos 3 + test-db + contract
0.13.0); o smoke é manual (postura das fatias anteriores).

## Migration

**Nenhuma** (D6). Confirmação: `jobs.params` é JSONB (migration 0006) e o
`create_job` já o popula (`package_ref`/`weights_ref`); `orchestrators.name`/
`kind`/`status` existem desde a 0006; migrations atuais 0001–0007.
`test-db.sh` não muda de schema — ganha casos (Testes).

## Delta de contrato (OpenAPI 0.13.0) — descrição na ADR

| Schema | Campo | Tipo | Semântica |
|---|---|---|---|
| `YoloJobRequest` | `orchestratorId?` | string (uuid) | opcional; ausente = Automático; não-UUID → 400 |
| `AutotrackerJobRequest` | `orchestratorId?` | string (uuid) | idem |
| `PredictJobRequest` | `orchestratorId?` | string (uuid) | idem |
| `Job` | `orchestratorName` | string\|null | nome do nó real (JOIN; null = não despachado/nó removido) |
| `Job` | `orchestratorKind` | string\|null | `local`\|`remoto` do nó real |
| `Job` | `orchestratorFallback` | boolean (default false) | true = hint presente no submit mas não honrado no dispatch |

**Erros novos: nenhum.** Status dos submits inalterados (202/400/401/404/409/
503 — o hint reusa `invalid_request` (400) e `not_found` (404) com mensagens
estáticas honestas). **Sem migration.** **Spec 0.12.0 → 0.13.0** (ou 0.14.0
se a Fatia K landar antes — regra ADR-0005 D1).

**Delta interno (fora da OpenAPI — transporte snake_case):**
| Rota/body | Serviço | Delta |
|---|---|---|
| `CreateJobRequest.orchestrator_hint: Option<String>` (`#[serde(default)]`) | principal → manager | fail-fast no submit (D2.4): não-UUID→400, inexistente→404, não-online→400; grava `params.orchestrator_hint` |
| `JobRow` + `/internal/jobs` (list/get) | manager | `orchestrator_name`/`orchestrator_kind` (LEFT JOIN) + `orchestrator_fallback` (bool, de `params->>'orchestrator_fallback'`) |
| `params.orchestrator_fallback` | manager | JSONB, `true` apenas quando o hint não foi honrado; removida quando honrado (D3.5) |
| `dispatch_next` | manager | lookup do hint (D3.2) + fallback (D3.3); sem hint → SQL atual |

## Spike obrigatório? — **NÃO**

Nenhuma premissa externa não verificada: JOIN em colunas existentes (SQL
nosso), JSONB `params` já usado pelo `create_job`, flag em `params` (mesmo
mecanismo do `weights_ref`), seletor consumindo `GET /api/environments`
**existente** (ADR-0011 D2 — telemetria por nó provada no H.7). *Inverteria
(sem spike, só ajuste):* se o usuário exigir fail-fast de **capacidade** no
submit, o principal precisaria da vram-table (viola ADR-0011 D3.1 — decidir
conscientemente, §final); se o `NodeSelect` precisar de cache de modelos por
nó, é a dívida de inventário models×nodes (fora do D0).

## Riscos e contingências

- **R1 — Fallback do hint cai no mock local (trap do NULL permissivo):** o
  usuário escolhe o remoto, o remoto morre, o job cai no `orchestrator-local`
  (permissivo-último — ADR-0011 D3.2). Mitigação em camadas: submit recusa
  nó não-online (D2 — o caso "já morto" nem chega ao dispatch); badge
  `orchestratorFallback` no detalhe (D5 — troca **visível**); watchdog mostra
  o remoto `offline` honesto. Residual = mesmo trap documentado no R1 da
  ADR-0011 (sessão GPU revoga o local); agora **com** evidência no job.
- **R2 — Corrida submit↔dispatch:** o nó pisca entre a validação (D2) e o
  despacho (fila FIFO) — coberto pelo fallback (D3) + badge; o job nunca fica
  preso. Teste: test-db com nó `degraded` entre submit e dispatch.
- **R3 — Hint órfão:** nó sem linha (sem path de DELETE em produto; revoke é
  tombstone `revoked`) — 404 no submit, fallback no dispatch; sem FK, o
  `params` não amarra. Aceito: o hint é metadado, não integridade.
- **R4 — Mapeamento de erro dos submits yolo/autotracker muda de 503 para
  400/404:** o alinhamento ao R6 (D6.4) **é** o comportamento documentado nas
  ADRs 0008/0012 (400/404 já declarados); clientes que dependiam do 503 para
  `NotFound` do weights ganham o 404 correto. Sem risco de compat (aditivo no
  sentido de "mais preciso"); coberto por contract + units.
- **R5 — `deny_unknown_fields` + spec `additionalProperties: false`:** a
  adição do campo aos 3 structs e aos 3 schemas é **obrigatória no mesmo
  commit** (campo novo sem struct → 400 em runtime; spec sem campo → contract
  vermelho). Ordem do N.3 cobre.
- **R6 — `params` JSONB cresce:** hint + flag somam ~60 bytes por job;
  `params` não vai ao dispatch (L1833-1848) nem ao wire — sem impacto em
  engine/UI além dos campos novos explícitos.
- **R7 — Flag de fallback reflete só o último dispatch:** job re-queueado
  (watchdog) e re-despachado honrando o hint → flag removida (D3.5) — o
  histórico de "caiu uma vez" não fica no wire. Aceito (o wire descreve o
  estado, não o histórico; o log do manager tem a transição).
- **R8 — `orchestratorFallback` extração JSONB no SELECT:** custo trivial
  (string compare por linha, conjunto pequeno single-user); sem índice novo.

## O que fica falso nos docs (lista para o `@docs-sync`, commit N.9)

**Não aplicar agora** — docs descrevem o que existe, não o que foi aprovado.

- `backend.md` §9/:146-151 — submits: emenda — os 3 POSTs ganham
  `orchestratorId?` (opcional, uuid; ausente = Automático) com 404 `not_found`
  (nó inexistente) / 400 `invalid_request` (nó indisponível ou não-UUID);
  §9/:161-167 — `GET /api/jobs` e `/:id`: `Job` ganha `orchestratorName`/
  `orchestratorKind`/`orchestratorFallback` (JOIN no manager; fallback = hint
  não honrado). Nota: mapeamento de erro dos 3 submits alinhado ao padrão R6
  (ADR-0013) — o `Err(_)→503` do yolo/autotracker morre.
- `backend.md` §6/:78-92 — policy VRAM: emenda — o roteamento por capacidade
  (ADR-0011 D3) ganha **preferência de 1º nível** (`orchestrator_hint` em
  `params`) com fallback automático e flag; o requisito continua resolvido no
  manager (nada muda na vram-table/guarda anti-mock).
- `backend.md` §10 — schema `jobs`: nota — `params` JSONB passa a aceitar
  `orchestrator_hint` (uuid, do submit) e `orchestrator_fallback` (bool, do
  dispatch); **sem coluna nova**.
- `docs/adr/0011-orquestracao-robusta.md` D3 — emenda: o SQL D3.2 vira o
  **fallback**; o hint é preferência do usuário sobre o mesmo conjunto
  elegível (mesmas condições); `queue_reason` segue só para `queued`.
- `docs/adr/0012-models-real.md` :373 e `docs/adr/0008-autotracker-v1.md`
  D3 — o mapeamento 404/400 documentado passa a valer de fato nos 3 submits
  (o `Err(_)→503` do yolo/autotracker era gap).
- `docs/adr/0013-playground-inferencia.md` D8 — spec recalibrada (esta fatia
  toma 0.13.0; se a K landar antes, 0.14.0).
- `frontend.md` §10/:235-238 (`startYoloJob`), :242 (`startAutotrackerJob`),
  :262 (`startPredictJob`) — bodies ganham `orchestratorId?`; §10/:263
  (playground) e /treino — seletor "Nó" (Automático default + online com
  name·kind·VRAM); `/jobs` detalhe — "Executado em: name (kind)" + badge de
  fallback; `Job` (types) — 3 campos novos.
- `docs/dividas.md` :55 — "Seleção manual de nó/GPU na UI — ABERTA
  2026-09-10" → **QUITADA** (Fatia N: `orchestratorId?` no create_job +
  seletor com fallback honesto + visibilidade do nó no job). Reafirmada: a
  policy VRAM completa (paralelismo/preempção) e o inventário models×nodes
  continuam dívidas abertas.
- `docs/coordenacao.md` — bloco da fatia N reescrito a cada commit.

## Plano de commits (N.0–N.9; branch `feat/visibilidade-no` de `main`)

Um dispatch = um commit; produção < 400 linhas por commit (contrato/testes
fora da conta — exceção da casa). **N.1 → N.2 → N.3 SEQUENCIAIS** (o wire do
JOIN é base do hint no create_job, que é base do BFF; mesmo boundary Rust —
ownership pode alternar entre `@rust-dev`/`@fixer`, ordem fixa). **N.4 → N.7
(frontend, via /impeccable) só depois do N.3 vivo** (regra F4.7: rebuild +
recreate dos containers antes de despachar UI); páginas sequenciais na mesma
branch, 1 página = 1 review. N.8 (reviewer) e N.9 (docs-sync) no fim.

| # | Dono | Conteúdo | Critério de pronto |
|---|---|---|---|
| **N.0** | @architect | Esta ADR (proposta; vira executável após aceite do usuário) | auditoria do coordenador; arquivo commitado em `main` |
| **N.1** | @rust-dev (manager) | `JobRow` + `orchestrator_name`/`orchestrator_kind`/`orchestrator_fallback`; `list_jobs` (L450) e `get_job` (L584) com `LEFT JOIN orchestrators` + extração `params->>'orchestrator_fallback'`; testes manager (wire: despachado/não-despachado/nó deletado; fallback default false) | `cargo test -p manager -- --ignored` + `bash scripts/test-db.sh` verdes; fmt; ~90-110 linhas |
| **N.2** | @rust-dev (manager) | `CreateJobRequest.orchestrator_hint` (`#[serde(default)]`) + validação no `create_job` (não-UUID→400, inexistente→404, não-online→400, ok→`params["orchestrator_hint"]`) + `dispatch_next`: lookup do hint (D3.2), fallback (D3.3), flag re-escrita (D3.5); testes (submit 400/404, dispatch honrado/fallback/ocupado/re-queue preserva hint/regressão sem hint) | `cargo test -p manager -- --ignored` + test-db verdes (2 nós); fmt; ~160-200 linhas (se estourar: dividir N.2a create_job / N.2b dispatch) |
| **N.3** | @rust-dev (principal) | `InternalJob`/`JobResponse`/`to_job_response` (3 campos novos); 3 requests + validação `orchestratorId` (não-UUID→400); 3 `manager_body` com `orchestrator_hint`; **alinhamento do mapeamento** dos submits yolo/autotracker ao R6 (NotFound→404, InvalidRequest→400, Unavailable→503, compensação mantida); spec **0.13.0** (3 requests + `Job` + delta interno) + `PROTECTED_ROUTES` intocadas + contract | `cargo test -p api-principal` verde + contract 0.13.0 ≡ router; test-db verde; fmt; ~280-340 linhas → **dividir N.3a (wire+requests+mapeamento) e N.3b (spec+contract)** se estourar |
| **N.4** | @frontend-dev (via /impeccable) | `types/studio.ts` (`Job` +3 campos); `lib/jobs.ts`/`playground.ts`/`autotracker.ts` com `orchestratorId?`; componente `NodeSelect` (Automático + online de `listOrchestrators` + estado de falha honesto) | `npm run build --workspace=web` verde; componente revisado no contexto da 1ª página (N.5); ~130-160 linhas |
| **N.5** | @frontend-dev | `/jobs` detalhe: "Executado em: name (kind)" + badge `orchestratorFallback`; subtexto nas tiles do histórico | smoke Chrome (detalhe com e sem nó; badge de fallback via estado de dev); **review 1**; ~60-90 linhas |
| **N.6** | @frontend-dev | `/playground`: `NodeSelect` na coluna de controle + `orchestratorId` no submit; toast de erro honesto (404/400) | smoke Chrome (submit com hint → job correto no detalhe; hint inexistente → 404 honesto); **review 2**; ~80-110 linhas |
| **N.7** | @frontend-dev | `/treino` (ForjaYoloSetup) + `AutoTrackerModal`: `NodeSelect` + `orchestratorId` no submit | smoke Chrome (2 superfícies); **review 3** (ou N.7a/N.7b = 2 reviews — decisão do usuário); ~110-150 linhas |
| **N.8** | @reviewer | review do diff N.1–N.7 vs esta ADR (pontos: JOIN/aditividade do wire, validação no manager, fallback+flag, mapeamento R6, seletor honesto) | APROVA (com ou sem nits); fixes roteados como commits próprios |
| **N.9** | @docs-sync | "O que fica falso nos docs" aplicado (backend/frontend/dividas/coordenacao; ADR-0011/0012/0013 emendadas) | doc↔código verificado nos dois sentidos; dívida :55 quitada |

**Notas de processo (lições das fatias anteriores):** contract test exige
spec ≡ router a cada commit (delta OpenAPI incremental); `cargo fmt --all`
antes de reportar; implementador que achar problema FORA do escopo **para e
reporta ao coordenador**; o mapeamento R6 do N.3 é **parte** do escopo (sem
ele o 404/400 do hint não atravessa o yolo/autotracker) — não tratar como
desvio; `deny_unknown_fields`/`additionalProperties:false` exigem struct +
spec no MESMO commit (R5).

## Pontos para decisão do usuário

1. **Versão da spec:** o repo está em **0.12.0** (não 0.13.0 como no
   enunciado) — esta fatia propõe **0.12.0 → 0.13.0**. Se a Fatia K
   (AutoTracker real) landar antes, K toma 0.13.0 e esta recalibra para
   0.14.0 (regra ADR-0005 D1).
2. **Código do "nó indisponível" no submit:** proponho **400
   `invalid_request`** (mensagem estática honesta) para nó offline/revogado e
   **404 `not_found`** para nó inexistente — sem erro novo. Alternativa
   considerada e rejeitada: 409 (na casa, 409 = conflito de recurso em
   mutação).
3. **Capacidade:** proponho validar **só no dispatch** (fallback + badge),
   mantendo ADR-0011 D3.1 (requisito resolvido no manager). Se o usuário
   quiser fail-fast de capacidade no submit, o principal precisaria da
   vram-table (quebra o boundary) — não recomendo; a UI mostra a VRAM por nó
   como informação.
4. **Hint no re-queue:** proponho que **sobreviva** (D4) — re-tenta o nó se
   voltou, fallback com badge se não.
5. **Reviews de UI:** proponho 3 (`/jobs`, `/playground`, `/treino` +
   `AutoTrackerModal` agrupados) — alternativa: 4 separados.
6. **`TrainYoloModal` da galeria:** proponho **sem seletor** (default
   Automático, intocado — Q1/F6.3). Se o usuário quiser o seletor lá, ele
   entra como 4ª superfície (mesmo `NodeSelect`, +1 review).
