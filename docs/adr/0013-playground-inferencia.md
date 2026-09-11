# ADR-0013 — Playground: inferência YOLO real com modelos (`predictions.json` + overlay) (Fatia J)

- **Status:** **PROPOSTA** (aguarda aceite do usuário). Nada implementado. Este
  documento é a especificação executável da fatia "PLAYGROUND — INFERÊNCIA YOLO
  REAL"; os deltas de contrato abaixo são aplicados **apenas nos commits da
  fatia** (openapi junto do código, docs de texto no `docs-sync` do fim), nunca
  antes.
- **Data:** 2026-09-10
- **Componentes:** `services/api-principal` (BFF: `POST /api/jobs/predict`,
  validação + `generate_predict_config_yaml`, reuso do `build_package` e do
  `weights_id`), `services/manager` (`mode` no `dispatch_body`, substituição do
  `jobs.model` pela variante do modelo no predict — resto reusado),
  `services/orchestrator` (`DispatchRequest.mode` + matriz de subcomando/
  artefatos por `(engine, mode)`), `engines/trainer-yolo` (subcomando `predict`
  mock determinístico + real `YOLO(weights).predict`), `apps/web` (módulo
  "Playground" habilitado → página `/playground` com overlay de boxes + download
  de `predictions.json` — via /impeccable, 1 página = 1 review),
  `packages/contracts` (spec 0.11.0 → **0.12.0**).
- **Fontes:** `IDEIA.md` (produto: ver detecções de um modelo treinado/baixado
  sobre imagens de um dataset); `docs/adr/0012-models-real.md` (formato desta
  ADR; infra de modelos: tabela `models`, staging de pesos no orquestrador D5,
  escopo `models/*`/`artifacts/*`, D0 "playground/inferência com modelos" fora
  daquela fatia = Roadmap → **esta fatia executa**); `docs/adr/0008-autotracker-
  v1.md` (D0 job na fila vs preview efêmero — mesmo desenho; D1 retorno de dados
  via artefato + apply explícito — o v1 desta fatia NÃO aplica, D4); `docs/
  backend.md` §4/:56 (tipos de job incluem `playground`), §5 (playground/runners
  preemptíveis — **não implementado**), §9/:145-146 (jobs implementados),
  §9/:161-163 (runners adiados); `docs/frontend.md` §10/:252/:340 (runners e
  playground = runner sob demanda — futuro), Sidebar `apps/web/components/studio/
  Sidebar.tsx` L238-245 (módulo "Playground" = badge Roadmap desabilitado
  honesto); `docs/dividas.md` :90 (dívida "playground/inferência com modelos").
  Código (verificado por graft/grep nesta data): `services/orchestrator/src/
  lib.rs` (`run_job_inner` L842-1214 — pipeline completo: package→md5→unzip→
  staging de pesos L927-973→config→docker run; ramificação de subcomando
  L1088-1105 e de artefatos L1134-1148 por `dispatch.engine`; `DispatchRequest`
  L29-41 **sem `mode`**; guarda anti-mock L879-886; `replace_config_placeholders`
  L319-332 com `weights_path`), `services/manager/src/lib.rs` (`dispatch_next`
  L1727-1855 — SELECT já traz `mode` mas o `dispatch_body` L1824-1833 **não o
  repassa**; `create_job` L344-417 — resolução de `weights_id` L361-384 →
  `params.weights_ref`, 404 inexistente, 400 engine≠yolo; `VramTable`
  L294-327 — entrada faltante ⇒ None permissivo), `packages/policies/
  vram-table.yaml` (só entradas `mode: train` de yolo/difusao/clip),
  `services/api-principal/src/jobs/handlers.rs` (`submit_yolo_job` L490-643 e
  `submit_autotracker_job` L653-800 — padrão: validação pura → dataset existe
  (404) → readiness (409 `dataset_not_ready`) → `build_package` → config_yaml →
  `create_job` com compensação do package em falha do manager),
  `services/api-principal/src/jobs/models.rs` (`validate_yolo_request`,
  `generate_config_yaml` L340-380; `validate_autotrack_request`,
  `generate_autotrack_config_yaml` L143-161; `parse_boxes_json` L215-251),
  `engines/trainer-yolo/src/trainer_yolo/autotrack.py` (`_read_dataset` L75-121,
  `_box_for_image` L128-169, `_generate_boxes_for_image` L172-202,
  `_mock_autotrack` L209-259 — boxes.json `{engine,model,seed,conf,images:
  [{filename,boxes:[{class,x,y,w,h,conf}]}]}` + metrics.jsonl 1-linha),
  `engines/trainer-yolo/src/trainer_yolo/train.py` (`_real_train` L286-342 —
  `YOLO(weights_path) if weights_path else YOLO(variante)`; `main` L383-403
  despacha `train|health|autotrack`), `apps/web/app/(studio)/jobs/page.tsx`
  (painel "Execuções" 546 linhas — lista+detalhe, `handleApplyBoxes` L251-286),
  `apps/web/components/studio/ConvergenceChart.tsx` L135-136 (`metrics` vazio →
  `null` — sem quebra), `apps/web/components/studio/Sidebar.tsx` L238-245,
  `packages/contracts/openapi.yaml` (version 0.11.0; enum `Error.code` L2031 com
  19 códigos; rotas `/api/jobs/*` L1363-1750).
- **Sequência:** Fatia I (mergeada) → **J (esta: playground/inferência)** →
  Fatia K (AutoTracker real — reusará o mecanismo de inferência desta fatia) →
  dívidas registradas.

## Contexto

O ciclo dados → treino → modelo fechou na Fatia I: tabela `models` canônica
(hook `best.pt` no report, upload, download por URL), fine-tune com
`weights?: uuid` (staging de pesos provado @gpu no TrueNAS: `YOLO(path)` real) e
página `/models`. O produto tem **modelos reais disponíveis** — mas não há
nenhuma forma de **usá-los para enxergar o que o modelo detecta**: o módulo
"Playground" da Sidebar é um badge "Roadmap" desabilitado honesto (L238-245), e
a dívida "playground/inferência com modelos" foi registrada no fecho da Fatia I
(dividas.md :90; ADR-0012 D0). A Fatia K (AutoTracker real) precisará de
inferência real de modelo — esta fatia é a peça que ela reaproveita.

O desenho é majoritariamente **reuso**: a infraestrutura que a inferência
precisa já existe e está provada. O orquestrador já baixa o package do dataset
(`packages/*`), já stagia pesos (`weights_ref` → `outputs/<job_id>/weights/`,
escopo `models/*`|`artifacts/*`, ADR-0012 D5), já roda o trainer com
`ENGINE_MOCK=0` em GPU e já sobe artefatos (`artifacts/*`). O engine já sabe
carregar peso real (`YOLO(weights_path)`). O que falta é: (a) uma **rota de
submit** de job de inferência, (b) um **subcomando `predict`** no trainer-yolo
(mock determinístico + real ultralytics), (c) a **ramificação** do orquestrador
por `mode` (hoje só por `engine` — predict usa `engine='yolo'`), (d) a
**página `/playground`** com overlay das detecções, e (e) a **verificação real
@GPU** com um `best.pt` treinado (critério binário: detecções reais ≠ mock).

As decisões centrais desta fatia: **transporte** (job assíncrono na fila
existente — D1), **entrada** (reuso do package — D2), **engine** (subcomando
`predict` reusando os geradores determinísticos do autotrack — D3), **destino**
(só-leitura: overlay + download; aplicar boxes ao dataset fica FORA — D4),
**deltas mínimos em manager/orquestrador** (D5/D6), **UI** (D7), **contrato**
(D8) e **verificação** (D9).

Regras da casa aplicadas: wire camelCase em `/api/*`, transporte interno
snake_case (ADR-0002 D1); 404 `not_found` para id não-UUID em path (ADR-0002
D8); erros `{code,message}` estáticos; PRs < ~400 linhas de produção; branch
`feat/playground-inferencia`; mock continua default (`ENGINE_MOCK=1`); GPU real
atrás de `ENGINE_MOCK=0` + guarda anti-mock existente (orquestrador com GPU
recusa imagem `:local` — L879-886); frontend SEMPRE via /impeccable com
`docs/DESIGN.md` como contrato, review por página.

## Decisões já travadas (base — citar, não redecidir)

- **O pipeline manager→orquestrador→artefatos é genérico e provado** (ADR-0007/
  0008): `create_job`/`dispatch_next` não ramificam por engine; o orquestrador é
  o único que hardcoda subcomando/artefatos (hoje por `dispatch.engine`, L1088-
  1105/L1134-1148); o principal é o único que valida por tipo.
- **Pesos: `weights_id` → `params.weights_ref` no manager; staging no
  orquestrador; placeholder `{weights_path}`** (ADR-0012 D5; `create_job`
  L361-384 — 404 inexistente, 400 engine≠yolo; `run_job_inner` L927-973;
  `replace_config_placeholders` L319-332). O predict **reusa tudo isso sem
  delta** (predict SEMPRE tem modelo — `weights_id` é obrigatório no submit).
- **Package = zip autossuficiente com imagens+labels+dataset.yaml; congelado em
  `dataset_versions`; build compartilhado `build_package()`** (F4.1/F4.2b;
  `submit_yolo_job` L566-570). Compensação: manager falhou → `delete_prefix` +
  DELETE row (padrão F4.2b).
- **`jobs.engine`/`mode`/`kind` são TEXT livres sem CHECK** (migration 0006:
  `engine TEXT NOT NULL, mode TEXT NOT NULL, kind TEXT NOT NULL`) — registrar
  `mode='predict'`, `kind='yolo_predict'` **sem migration**.
- **`vram_min_gb` gravado mas ignorado na decisão de fila** (ADR-0010 D9/
  ADR-0011 D3; `dispatch_next` L1756-1774 — `VramTable.resolve_required_gb`
  retorna `None` para entrada faltante ⇒ permissivo; `vram-table.yaml` só tem
  `mode: train`). Predict entra com `vram_min_gb: null` — **sem delta na
  vram-table**.
- **Guarda anti-mock**: orquestrador com GPU recusa imagem `:local` (L879-886);
  `ENGINE_MOCK=0` injetado no docker run quando GPU presente (L1033-1037).
  Predict real @gpu segue o MESMO padrão — zero delta.
- **Artefatos lidos pelo browser via `GET /api/jobs/:id/artifacts` + proxy
  `.../:artifactId/data`** (defesa de path + md5) — o overlay do playground lê
  `predictions.json` por esse caminho, **sem rota nova de leitura**.
- **ManagerError mapeado no principal**: `NotFound → 404`, `Unavailable → 503
  queue_unavailable` (padrão das rotas de leitura; o `submit_yolo_job` atual só
  distingue `Unavailable` do resto — ver D5/R6). O handler do abort (L825-831) é
  o padrão rico (NotFound/NotAbortable/Unavailable/InvalidRequest).
- **Spec OpenAPI atual: 0.11.0**; versão dos crates 0.1.0. Enum `Error.code`
  (L2031) tem 19 códigos — **nenhum código novo necessário nesta fatia**.
- **`ConvergenceChart` tolera `metrics` vazio** (L135-136 → `null`) — um job
  sem `metrics.jsonl` não quebra o painel de detalhe do `/jobs`.
- **UI**: módulo "Playground" da Sidebar = badge Roadmap (L238-245); `/jobs` =
  "Execuções" (lista + detalhe com métricas/artefatos/abort/apply de
  autotracker); página `/models` real (Fatia I); editor BBox da galeria renderiza
  boxes por classe (padrão de overlay reusado); toda UI via /impeccable com
  DESIGN.md (workspace 2 colunas, Monospace Truth, Anti-Scroll-Trap, One CTA).

## Decisões

### D0 — Escopo v1: 1 modelo `models` (engine yolo) × imagens ativas de 1 dataset × conf → `predictions.json` + overlay na UI; fora: apply, multi-engine, vídeo, streaming, Fatia K

**Decidido — entra na fatia:**
- **Inferência de 1 modelo** (row da tabela `models`, engine `yolo` — o mesmo
  catálogo da Fatia I, qualquer `source`: train/upload/download) **sobre as
  imagens ativas de 1 dataset** (o package congelado no submit), com **limiar de
  confiança único** (`conf`, slider na UI).
- Resultado = artefato **`predictions.json`** (`artifacts/<job_id>/`,
  `kind='predictions'`) + **overlay de boxes na página `/playground`** +
  download do artefato (proxy existente).
- O mecanismo de inferência (subcomando `predict` + staging de pesos + rota de
  submit) é a **peça que a Fatia K (AutoTracker real) reaproveitará** — o
  `predictions.json` tem o mesmo shape de boxes do `boxes.json` do autotracker
  (D3), de propósito.

**Decidido — fora (dívida registrada, NÃO implementado agora):**
- **Aplicar detecções ao dataset** (origin `playground` na tabela `boxes`) —
  D4; exigiria migration 0008 (`boxes.origin` CHECK é `manual|autotracker|
  import` desde a 0003) + rota de apply própria. Fica para fatia própria (o
  AutoTracker real/K trará o fluxo de ingest).
- **Multi-engine** (difusão/CLIP), **vídeo**, **threshold por imagem**,
  **streaming de detecções por imagem** (progresso incremental), **imgsz
  configurável** (fixo 640 no real), **re-treino a partir do playground**,
  **runner quente preemptível** (backend.md §5 — permanece dívida; v1 usa a
  fila, mesmo desenho do ADR-0008 D0).

**Descartado:** "mais uma rota de leitura de predictions" (o proxy de artefato
já cobre); "fazer overlay dentro de `/jobs`" (resultado é visualização de
detecções sobre imagens — página própria `/playground`, `/jobs` continua sendo o
painel de execução; link entre as duas).

### D1 — Transporte: job assíncrono na fila existente (`POST /api/jobs/predict` → 202), NÃO rota síncrona

**Decidido:** o predict é um **job na fila existente** —
`POST /api/jobs/predict` → 202 `SubmitJobResponse{jobId,status:"queued",
queuePosition?}`, fluindo pelo mesmo pipeline (manager FIFO + dispatch por VRAM
permissiva + orquestrador + artefatos + `/jobs`). **Não** criar rota síncrona
(`POST /api/playground/predict`) nem runner quente.

*Por quê:* (1) **N imagens × tempo de inferência em GPU** é assíncrono natural —
um predict de um dataset com centenas de imagens pode levar minutos; rota
síncrona exigiria timeout e não sobreviveria a restart; (2) **precedentes**: o
autotracker (ADR-0008 D0) já decidiu exatamente isso — "o lote é assíncrono,
durável e audível — exatamente o que a fila faz"; (3) **reuso total**: manager
(FIFO, recovery, watchdog, abort), orquestrador (pipeline inteiro), UI (`/jobs`
já renderiza qualquer job com engine/model/mode); (4) **cancelamento** e
**progresso** de graça; (5) o roteamento por VRAM da fila é permissivo no v1
(mesma policy do train — D5). *Gotcha:* um predict pode ficar atrás de um treino
longo na FIFO única (R3); aceito no v1 (policy de prioridade é dívida). O runner
quente (backend.md §5, frontend.md §10/:340) é UX distinta (latência
interativa) e fica registrado como dívida — v1 prova o mecanismo de inferência,
que é o que a Fatia K precisa.

**Descartado:** rota síncrona (timeout/restart/UX — sem runner quente não há
consumidor de tempo real; lição P1 ADR-0006: código sem consumidor não se
escreve); runner sob demanda (backend.md §5 pressupõe preempção por VRAM + WS —
arquitetura de futuro, dívida existente).

### D2 — Entrada das imagens: reuso do `build_package` (zip com imagens+labels; labels ignoradas no predict), NÃO export "só imagens" novo

**Decidido:** o submit de predict chama o **mesmo `build_package()`** já usado
por yolo/autotracker (`submit_yolo_job` L566-570) — o package congelado em
`dataset_versions` carrega imagens + labels + `dataset.yaml`, o orquestrador já
o baixa/descompacta, e o predict **ignora as labels** (o engine lê só as imagens
e, no mock, o `dataset.yaml` para classes). Nenhuma rota nova de export.

*Por quê:* (1) **custo zero no pipeline** — package, S3 `packages/*`, md5,
unzip e `dataset_versions` já existem e são provados; um export "só imagens"
exigiria variante de package + validação de manifest + caminho novo no
orquestrador para ganhar... nada no v1; (2) o **peso extra das labels** no zip é
aceito (mesmo trade do autotracker — ADR-0008 D0 "custo do job = package build +
container run"); (3) o package é **autossuficiente e congelado** — o predict
roda sobre o snapshot, não sobre o dataset mutável (mesma garantia do treino);
(4) o `mode` do job (D6) já distingue predict de train sem marcação no package.
*Gotcha:* dataset com muitas imagens → package pesado baixado pelo orquestrador
só para inferir; dívida registrada ("package só-imagens para inferência" se
datasets grandes virarem problema — R2). *Descartado:* export "só imagens"
novo (rota + variante de package + delta no orquestrador — custo sem retorno no
v1); "apontar predict para as imagens do bucket direto" (o orquestrador só tem
escopo `packages/*`+`artifacts/*`+`models/*` — `datasets/*` fora da credencial;
e perderia o snapshot congelado).

### D3 — Engine: subcomando `predict` no `trainer-yolo` (reuso da imagem); mock determinístico reusando geradores do autotrack; real `YOLO(weights).predict` → `predictions.json`

**Decidido:** novo subcomando **`predict`** na imagem existente
`hephaestus/trainer-yolo:local` (`python -m trainer_yolo predict --config
<config.yaml> --output <output_path>`), registrado no `main()` (L383-403),
espelhando a estrutura do `autotrack.py` (novo módulo `predict.py`):
- **Config** (`load_and_validate_predict_config`, espelho de
  `load_and_validate_autotrack_config` L40-68): required keys `job_id, engine,
  model, mode, dataset_path, output_path, seed, weights_path` + seção
  `predict: {conf}` com `conf` em `0..=1` (400/exit 1 honesto fora do domínio).
  `weights_path` é **obrigatório** (predict sem modelo não existe — diferente do
  train onde é opcional).
- **Mock** (`_mock_predict`, com `ENGINE_MOCK=1` default): **reusa os
  geradores determinísticos do autotrack** — `_read_dataset` (classes +
  filenames do `dataset.yaml`, ordenação determinística), `_box_for_image`,
  `_generate_boxes_for_image` (mesmo `(seed, filename, classe)` → mesmas
  coordenadas) — mas escreve **artefato próprio `predictions.json`** com
  `engine: "yolo"` e ignora `weights_path` (o mock não lê o peso; mesmo padrão
  do mock train com fine-tune — ADR-0012 D5 gotcha b).
- **Real** (`_real_predict`, com `ENGINE_MOCK=0`, @gpu): `YOLO(cfg[
  "weights_path"])` (a MESMA chamada já usada no fine-tune — ADR-0012 D5/I.6a) →
  `model.predict(source=str(dataset_path), conf=conf, imgsz=640, device=0)` →
  para cada `result`: `fname = Path(r.path).name`; `box.xywhn[0]` (centro
  normalizado) → `x = cx - w/2`, `y = cy - h/2` (top-left, **clamp 0..1**);
  `class = model.names[int(box.cls[0])]`; `conf = float(box.conf[0])`. Toda
  imagem do dataset entra no `predictions.json` (boxes vazios quando sem
  detecção — honestidade do overlay).
- **`predictions.json`** (transporte snake_case, domínio 0..1 — mesmo domínio do
  `PUT boxes` e do `boxes.json` do autotracker):
  ```json
  { "engine": "yolo", "model": "predict", "conf": 0.65,
    "images": [ { "filename": "img_0001.jpg",
                  "boxes": [ { "class": "solda_fria", "x": 0.1, "y": 0.2,
                               "w": 0.3, "h": 0.4, "conf": 0.96 } ] } ] }
  ```
  `class` = **nome** (string; de `model.names` no real, do `dataset.yaml` no
  mock) — mesmo contrato do autotracker (ADR-0008 D1), o que habilita a Fatia K
  e um futuro apply sem re-mapear shape. `model` = `cfg["model"]` (literal
  `"predict"` — a variante real vive em `jobs.model` via D5).
- **Sem `metrics.jsonl`**: predict não tem epochs nem loss — **progresso
  binário e honesto** (D6). O `ConvergenceChart` tolera ausência (base já
  travada).

*Por quê — reusar geradores com artefato próprio (e não reusar `boxes.json`
literal):* o shape é quase idêntico, mas o contrato é distinto — `engine: yolo`
(não `autotracker`), seção de config `predict:` (não `autotrack:`), e o
orquestrador precisa coletar `predictions.json` com `kind='predictions'` (a
Fatia K e a UI filtram por ele). Reusar os **geradores** (funções puras,
engine-agnósticas) dá o determinismo de graça sem emaranhar os dois contratos.
*Gotcha:* (a) no mock, as classes vêm do dataset (não do modelo) — honesto como
mock, documentado na UI via badge do job; (b) `xywhn` é centro — a conversão
para top-left + clamp é a única lógica geométrica nova; (c) ultralytics
`predict(source=dir)` varre o diretório recursivamente — o `dataset_path`
descompactado tem `images/` + labels + `dataset.yaml`; seguro (o engine só lê o
que é imagem). *Descartado:* reusar `boxes.json` literal com flags
(contrato confuso para a Fatia K e para a UI — `kind` errado no orquestrador);
mock novo do zero (os geradores determinísticos já existem e são testados — 37
testes do autotrack).

### D4 — Destino dos resultados: SÓ-LEITURA no v1 (overlay + download de `predictions.json`); "aplicar boxes ao dataset" FORA (dívida)

**Decidido:** o v1 **não escreve no dataset**. O resultado do predict é (a)
**overlay das detecções** na página `/playground` (boxes por classe sobre as
imagens do dataset) e (b) **download** de `predictions.json` (via proxy de
artefato existente). Nada grava em `boxes` — sem `origin` novo, sem migration,
sem rota de apply.

*Por quê — pesei o risco vs o valor:* aplicar detecções como anotações
(`origin='playground'`) exigiria: migration 0008 (o CHECK de `boxes.origin` é
`manual|autotracker|import` desde a 0003), rota de apply nova, e uma **decisão
de merge** (o que fazer com boxes manuais/autotracker existentes? o autotracker
já tem merge por origem — ADR-0008 D1a — mas a semântica de "predições de um
modelo treinado" sobre o MESMO dataset re-escreveria labels humanas). O valor do
produto no v1 é **ver as detecções** — validar o modelo, inspecionar falsos
positivos/negativos; isso é coberto por overlay + download. Poluir o dataset com
predições de modelo sem um fluxo de revisão explícito (como o "Aplicar boxes" do
autotracker) é risco de corrupção de dados de anotação. A Fatia K (AutoTracker
real) trará o fluxo de ingest com a semântica certa (merge por origem +
consentimento explícito, padrão ADR-0008 D1). *Gotcha:* overlay client-side faz
match por **filename** (predictions.json ↔ lista de imagens do dataset) — imagem
deletada pós-job → skip silencioso com contagem honesta na UI. *Descartado:*
apply com `origin='playground'` no v1 (migration + rota + semântica de merge
indecidida — custo alto, valor baixo sem revisão); auto-ingest no `done`
(topologia nova manager→principal + corrompe dataset sem consentimento —
mesmo argumento do ADR-0008 D1).

### D5 — Manager: `mode` no `dispatch_body` + `jobs.model` = variante do modelo (predict); reuso integral de `weights_id`/VRAM permissiva

**Decidido — delta mínimo no manager, 2 mudanças pontuais:**
1. **`dispatch_body` ganha `"mode"`** (`dispatch_next` L1824-1833): o SELECT já
   traz `mode` (L1736-1749) mas o body não o repassa — 1 linha + serialização.
   Contrato interno snake_case; o orquestrador passa a ramificar por
   `(engine, mode)` (D6). **Sem isso o predict não pode usar `engine='yolo'`**
   (o dispatch não distinguiria `train` de `predict`).
2. **`create_job`: quando `req.model == "predict"`** (placeholder literal do
   submit) **e a row de `models` resolvida por `weights_id` tiver `model`
   (variante) NOT NULL → `jobs.model` = variante** (ex.: `yolo11m`) — display
   honesto no `/jobs` (o painel mostra engine+model+mode). Extensão do SELECT de
   resolução (L361-384) para `SELECT s3_key, hash, engine, model FROM models
   WHERE id = $1`. **Não afeta fine-tune** (lá `req.model` é variante escolhida
   pelo usuário, nunca `"predict"`). Sem variante (upload/download) →
   `jobs.model = "predict"` literal.

**Reuso integral (zero delta):** `weights_id` obrigatório → resolução 404
inexistente / 400 engine≠yolo → `params.weights_ref` (L361-384); fila FIFO +
recovery + watchdog; `vram_min_gb: null` + vram-table sem entrada
`(yolo, *, predict)` → `resolve_required_gb` = `None` → permissivo (mesma
política do train hoje; entradas de predict na vram-table = dívida quando a
policy VRAM real entrar — ADR-0011 D3).

*Por quê:* o `mode` no dispatch é a **única informação nova** que o orquestrador
precisa (o pipeline inteiro já é genérico); a substituição da variante evita
`model="predict"` redundante na UI sem violar o boundary (o principal não lê a
tabela `models` — a resolução continua no dono, o manager). *Gotcha — contrato
interno `DispatchRequest.mode`:* shape fixado NESTA ADR com `#[serde(default)]`
(tolerância forward-compat no sentido manager→orquestrador, padrão R7 da
ADR-0012); landing: manager antes do orquestrador no merge se houver
intercalação; smoke valida o par. *Descartado:* `mode` sniffado do
`config_yaml` no orquestrador (parse de YAML para decisão de roteamento — frágil
e esconde o contrato); ramificar por `engine='predict'` novo (a tabela `models`
tem CHECK `engine IN ('yolo')`; `jobs.engine='predict'` mentiria sobre o engine
e quebraria o reuso do `weights_ref` que exige `engine='yolo'` da row).

### D6 — Orchestrator: `DispatchRequest.mode` + matriz `(engine, mode)` de subcomando/artefatos; progresso binário honesto (sem metrics.jsonl)

**Decidido:** `DispatchRequest` ganha **`mode: String`** (`#[serde(default)]` —
ausente ⇒ `""`, tolerância de intercalação). Em `run_job_inner`, as duas
ramificações (subcomando L1088-1105, artefatos L1134-1148) passam a ser por
**matriz `(engine, mode)`**:

| `engine` | `mode` | subcomando | artefatos |
|---|---|---|---|
| `yolo` | `train` | `train` | `[best.pt model, last.pt model, metrics.jsonl metrics]` (inalterado) |
| `yolo` | `predict` | `predict` | `[predictions.json predictions]` |
| `autotracker` | `autotrack` | `autotrack` | `[boxes.json boxes, metrics.jsonl metrics]` (inalterado) |
| outro | — | — | `PipelineError::Other("unsupported engine/mode")` (limpo, sem panic) |

**Progresso do predict — binário e honesto:** o orquestrador reporta
`preparing → running (progress 0.0) → done (progress 1.0)`; o predict **não
escreve `metrics.jsonl`** (não há epochs/loss para medir) e o report `done` sai
com `metrics: None`, `epoch: None`. O metrics collector existente (L1039-1086)
apenas não encontra arquivo e idles — **zero mudança no pipeline de polling**. A
UI: barra de progresso salta 0→100 no done; `ConvergenceChart` não renderiza
(tolerância já travada). *Por quê:* fabricar `metrics.jsonl` sintético com loss
0.0 seria mentira visual (curva plana no gráfico de convergência); o progresso
por imagem (streaming) é dívida (D0). **Zero delta no resto**: package, md5,
unzip, staging de pesos, docker run (`container_name = trainer-<engine>-<job_id>`
já deriva de `engine='yolo'` — ok), upload de artefatos, anti-mock guard.

*Por quê — `mode` e não outro campo:* `mode` já existe no banco e no SELECT do
dispatch; é a semântica natural (o mesmo `engine='yolo'` roda train e predict).
*Gotcha:* `mode: ""` (default serde) com `engine='yolo'` cairia em
unsupported — proteção de intercalação é só para o manager ANTIGO com
orquestrador NOVO; no par commitado junto nunca ocorre. *Descartado:* predict
com `metrics.jsonl` 1-linha (curva fake — rejeitado por honestidade); campo
novo `kind` no dispatch (redundante com `mode`).

### D7 — UI: módulo "Playground" habilitado → página `/playground` (submit + acompanhar em `/jobs` + overlay + download); via /impeccable, 1 página = 1 review

**Decidido (fatia J.5; 1 página = 1 fatia = 1 review, /impeccable com
`docs/DESIGN.md`):**
- **Sidebar** (`Sidebar.tsx` L238-245): módulo "Playground" sai do Roadmap —
  badge removido, `isAvailable: true` (padrão da Fatia I no módulo Models).
- **Página `/playground`** (workspace 2 colunas — DESIGN.md):
  - *Coluna de controle (320–384px):* seletor **Modelo** (`listModels()`
    filtrado por `engine==='yolo'`, rótulo `name · source · variante`), seletor
    **Dataset** (elegível: `category==='yolo' && imagesCount>0` — classes NÃO
    obrigatórias, predict não usa labels; mesma lista do `ForjaYoloSetup`
    filtrada), slider **conf** 0.3–0.95 default 0.65 (consistente com o
    AutoTracker), CTA único "Executar Inferência" → `POST /api/jobs/predict` →
    toast 202 + link "Acompanhar em Execuções" (`/jobs?job=<id>` — auto-seleção
    existente).
  - *Coluna de resultado:* lista dos **jobs predict** (filtro client-side por
    `mode==='predict'` no `GET /api/jobs`); job `done` → **overlay de boxes**
    sobre as imagens do dataset (match por filename entre `predictions.json`
    baixado via proxy de artefato e a lista de imagens — `GET
    /api/datasets/:id/images`; renderização reusa o padrão de boxes por classe
    do editor da galeria) + contagem honesta (imagens com/sem detecção, skips)
    + botão **"Baixar predictions.json"** (proxy `.../artifacts/:id/data`).
  - Empty states honestos ("Nenhum modelo YOLO ainda — treine ou importe pesos
    em Modelos & Pesos" / "Nenhum dataset YOLO com imagens" / "Nenhuma execução
    de playground ainda").
- **`lib/playground.ts`** novo (`startPredictJob`, `getPredictions`) +
  `types/studio.ts` (aditivo); erros mapeados em pt-BR por `code` (padrão da
  casa).
- **`/jobs` (Execuções)**: predict aparece automaticamente (é um job —
  `engine='yolo'`, `mode='predict'`, `model=<variante>`); badge opcional
  "predict" no card (polish, não requisito — o wire `Job` já carrega
  `kind/model/mode`).

*Por quê:* o módulo Roadmap "Playground" é a promessa de UI que esta fatia
cumpre; resultado **na mesma página** (não só em `/jobs`) porque o valor é a
visualização das detecções — e o `/jobs` continua sendo o painel de execução
(link entre os dois, padrão do `/treino` → `/jobs?job=`). *Gotcha:* overlay
client-side sem servidor de imagem dedicado — URLs presigned do dataset; sem
`S3_PUBLIC_ENDPOINT_URL` → `url: null` → a UI desabilita o overlay com tooltip
honesto (mesmo padrão do botão Baixar da página `/models`). *Descartado:*
resultado dentro de `/jobs` (página de execução poluída com visualização de
imagens); modal de resultado (workspace 2 colunas é o contrato DESIGN.md).

### D8 — Contrato: `POST /api/jobs/predict`; spec 0.11.0 → 0.12.0; **sem erro novo**; **sem migration** (`mode` nasce em `jobs.mode` TEXT existente)

**Decidido:**
- **Rota nova única** (entra em `PROTECTED_ROUTES`):
  ```
  POST /api/jobs/predict   202 400 401 404 409 503
  ```
  Body (wire camelCase, `deny_unknown_fields`): `{modelId: string(uuid),
  datasetId: string(uuid), conf?: number (0..=1, default 0.65)}` → 202
  `SubmitJobResponse`. Erros: 400 `invalid_request` (modelId não-UUID, conf fora
  de 0..1, body malformado); 404 `not_found` (datasetId não-UUID/inexistente,
  **modelId inexistente** — vindo do manager); 400 `invalid_request` (row de
  `models` com `engine≠yolo` — vindo do manager); 409 `dataset_not_ready`
  (category≠yolo, 0 imagens ativas); 503 `queue_unavailable` (manager fora,
  com compensação do package).
- **`PredictJobRequest`** schema novo; **nenhum código de erro novo** (reuso de
  `invalid_request`/`not_found`/`dataset_not_ready`/`queue_unavailable` — a
  enum de 19 códigos já cobre). Spec **0.11.0 → 0.12.0** (regra "versão = ordem
  de landing", ADR-0005 D1).
- **Sem migration**: `jobs.mode='predict'` nasce na coluna TEXT existente
  (0006); `jobs.params` JSONB carrega `package_ref` + `weights_ref` (padrão);
  nada de `ALTER`. `kind='yolo_predict'`, `engine='yolo'`, `mode='predict'`.
- Rotas internas (`mode` no dispatch, `POST /internal/*` inalterado) **não**
  entram na OpenAPI (padrão dos paths internos).

*Por quê:* o contrato espelha exatamente o `POST /api/jobs/autotracker`
(ADR-0008 D3) — body análogo, erros análogos, 202 análogo — com `modelId` no
lugar de `model` (predict referencia o catálogo `models`, não uma variante);
reuso de códigos evita inflar a enum sem necessidade. *Gotcha — mapeamento do
manager no principal:* o `submit_yolo_job` atual mapeia `Err(_)` do manager
(NotFound/InvalidRequest) como 503 — **o handler do predict NÃO repete isso**:
mapeia `NotFound → 404`, `InvalidRequest → 400` (padrão do handler de abort,
L825-831). O mapping do `/api/jobs/yolo` fica como está (comportamento
existente; divergência de ADR-0012 D6 documentada na sync — R6). *Descartado:*
erro novo `model_not_found` (404 `not_found` já é a semântica do weights
inexistente — ADR-0012 D5); `POST /api/jobs/predict` com `model` (string de
variante) em vez de `modelId` (predict SEMPRE usa um peso real do catálogo —
variante sozinha baixaria peso pretrained sem ser "o modelo do usuário").

### D9 — Verificação: baterias por commit + smoke mock E2E + sessão GPU com `best.pt` real (critério binário: detecções reais ≠ mock)

**Decidido — padrão da casa (unit + contract + test-db + smoke E2E + GPU
manual):**
- **unit**: engine (mock determinístico, shape `predictions.json`, validação de
  config com conf domínio, real com YOLO monkeypatchado); manager (mode no
  dispatch, substituição da variante, 404/400 na resolução); orquestrador
  (matriz (engine,mode) → subcomando/artefatos, `DispatchRequest.mode` serde,
  `predictions.json` coletado/subido); principal (validação predict, config
  yaml, mapeamento 404/400/409/503, compensação, contract 0.12.0 ≡ router).
- **test-db** (extensão): predict → dispatch com `mode` → resolução de
  `weights_id` → `params.weights_ref`; 404 modelId inexistente; variante
  substituída em `jobs.model`.
- **smoke E2E mock** (stack `ENGINE_MOCK=1`, `EXEC_MODE=docker`, Chrome):
  dataset yolo + classe + imagem → modelo (backfill de treino OU upload `.pt`)
  → `/playground` → selecionar modelo+dataset+conf → submit → `/jobs` done →
  `predictions.json` com boxes → overlay na UI → download → abort num segundo
  job → console limpo. **Critério: predictions do engine chegam ao bucket e
  aparecem na UI.**
- **Sessão GPU (manual, fora do CI — padrão ADR-0010 G.6 / ADR-0012 I.9):
  inferência real com `best.pt` treinado na 3060** — critério binário:
  **detecções reais ≠ mock** (coordenadas/classes distintas das determinísticas;
  idealmente detecção coerente com o conteúdo — ex.: "solda_fria" detectado na
  imagem onde a box real foi anotada). Guarda anti-mock provada: imagem `:local`
  recusada com GPU. Teardown completo (TrueNAS na main, volumes gpu_* removidos,
  banco limpo, manager re-adota local).

## Migration

**Nenhuma.** `jobs.engine`/`mode`/`kind` são TEXT livres desde a 0006 (sem
CHECK); `jobs.params` JSONB carrega `package_ref`+`weights_ref`; nada de `ALTER`
nesta fatia. O `origin='playground'` na tabela `boxes` **não nasce** (D4 —
dívida junto com a Fatia K).

## Delta OpenAPI (0.12.0) — descrição na ADR

Rota nova (entra em `PROTECTED_ROUTES` com status exatos):
```
POST /api/jobs/predict   202 400 401 404 409 503
```
- Body `PredictJobRequest` (wire camelCase): `{modelId: string(uuid),
  datasetId: string(uuid), conf?: number (0..=1, default 0.65)}` →
  202 `SubmitJobResponse{jobId,status:"queued",queuePosition?}` | 400
  `invalid_request` (modelId não-UUID, conf fora de 0..1, body malformado,
  engine≠yolo da row de `models` — via manager) | 404 `not_found` (datasetId
  não-UUID/inexistente, modelId inexistente — via manager) | 409
  `dataset_not_ready` (category≠yolo, 0 imagens ativas) | 503
  `queue_unavailable` (manager fora — compensação do package).

**Erros novos: NENHUM.** Reuso: `invalid_request` (400), `not_found` (404),
`dataset_not_ready` (409), `queue_unavailable` (503). Schema novo:
`PredictJobRequest`. Job: `kind='yolo_predict'`, `engine='yolo'`,
`mode='predict'`, `model=<variante|"predict">`, `params.weights_ref` +
`package_ref` (JSONB), `vram_min_gb: null`. Artefato novo:
`predictions.json` (`kind='predictions'`, `artifacts/<job_id>/` — transporte
snake_case, fora da OpenAPI, lido via proxy existente).

## Testes

| Camada | Infra | Prova |
|---|---|---|
| `pytest engines/trainer-yolo` | unit (ultralytics monkeypatchado) | mock `predict` determinístico (mesmo seed → mesmas boxes); shape `predictions.json` (engine yolo, imagens com boxes vazios permitidos); `load_and_validate_predict_config` (weights_path obrigatório, conf fora de 0..1 → die); real: `YOLO(weights_path)` chamado com o caminho, `predict(source=dir, conf, imgsz=640)`, `xywhn` → `x,y` top-left com clamp, `class` de `model.names`, imagem sem detecção → boxes `[]`; **57+ testes existentes verdes (mock train/autotrack intocados)** |
| `cargo test -p manager` (+ test-db) | Postgres do compose | `dispatch_body` com `mode` (serde + teste de unidade do `dispatch_next`); `create_job` com `model=="predict"` e weights com variante → `jobs.model=variante`; sem variante → `"predict"`; `weights_id` inexistente → 404; engine≠yolo → 400; `params.weights_ref` gravado; fine-tune (model≠predict) **inalterado** |
| `cargo test -p orchestrator` | unit (sem S3) | `DispatchRequest.mode` (com valor, ausente → `""` via serde default); matriz (engine,mode) → subcomando/artefatos: (yolo,train), (yolo,predict), (autotracker,autotrack), (yolo,"") e (engine desconhecido) → PipelineError limpo; `predictions.json` coletado e subido (`kind='predictions'`); pipeline sem metrics.jsonl → report done com `metrics: None` (sem pânico no collector) |
| `cargo test -p api-principal` | MockStorage + pool lazy | validação `validate_predict_request` (modelId UUID, conf 0..1, deny_unknown); `generate_predict_config_yaml` (placeholders + `weights_path` + seção `predict:`); eligibility (category yolo + imagens>0; **0 classes aceito** — divergência do autotracker); manager body (`kind='yolo_predict'`, `model='predict'`, `mode='predict'`, `weights_id`); mapeamento `NotFound→404`, `InvalidRequest→400`, `Unavailable→503` + compensação do package; **contract: spec 0.12.0 ≡ router** |
| Smoke E2E (stack `ENGINE_MOCK=1`, Chrome) | compose completo | dataset yolo+classe+imagem → modelo (treino backfill ou upload) → `/playground` → submit → `/jobs` done → `GET /api/jobs/:id/artifacts` com `predictions.json` → proxy `data` baixável → overlay na UI com boxes → download → abort de job predict → console limpo. **Critério: predictions do engine chegam ao bucket e aparecem na UI** |
| Sessão GPU (manual, checklist README-gpu) | TrueNAS + RTX 3060 | `best.pt` treinado (ou fine-tune da Fatia I) → predict real (`ENGINE_MOCK=0`) sobre imagens → **detecções reais ≠ mock** (critério binário); guarda anti-mock (`:local` recusado); teardown completo |

O CI cobre as baterias locais (pytest + cargo dos 3 + test-db + contract); o
smoke @gpu continua manual (padrão ADR-0010/0012).

## Spike obrigatório? — **NÃO**

Premissas externas são comportamento padrão ou já provado: (1) `YOLO(caminho)` +
`model.predict(source=dir, conf, imgsz)` é a API canônica do ultralytics — a
MESMA biblioteca/versão já usada no `_real_train` (L286-342) e no fine-tune real
da sessão GPU da ADR-0012; (2) `result.boxes.xywhn` normalizado e
`model.names` são o formato padrão de saída de `predict`; (3) staging de pesos
(`weights_ref` → `outputs/<job_id>/weights/`) provado @gpu (ADR-0012 I.9); (4)
package + fila + artefatos provados (F4/F5); (5) guarda anti-mock provada
(ADR-0011 fase 2). *Inverteria o desenho (aí sim vira spike):* se o
`result.boxes.xywhn`/`model.names` não existisse na versão instalada do
ultralytics (não provável — API estável) → fallback: derivar boxes de `xyxy` +
shape da imagem no `_real_predict` (mesma função, sem spike).

## Riscos e contingências

- **R1 — Predict atrás de treino na FIFO única** (predict pode esperar um treino
  longo): aceito no v1 (mesma fila, roteamento por VRAM permissivo); a UI mostra
  posição de fila (202 já traz `queuePosition`); policy de prioridade = dívida.
- **R2 — Package pesado só para inferir** (labels baixadas sem uso): aceito
  (trade do autotracker, ADR-0008 D0); dívida "package só-imagens" se datasets
  grandes virarem problema.
- **R3 — `predictions.json` grande** (muitas imagens): o browser baixa o artefato
  inteiro via proxy (como `get_artifact_data`); aceito local; streaming por
  imagem = dívida (D0).
- **R4 — Mock gera boxes de classes do DATASET, não do modelo**: honesto como
  mock (o mock não lê pesos — mesmo padrão do mock train com fine-tune); a UI
  mostra badge de job/estado mock; o critério da sessão GPU é justamente provar
  a diferença (D9).
- **R5 — Match client-side por filename (overlay)**: imagem deletada pós-job →
  skip com contagem honesta; sem risco de escrita (D4 read-only).
- **R6 — Mapeamento do manager no principal**: o handler de predict mapeia
  `NotFound→404`/`InvalidRequest→400` (padrão abort, L825-831); o
  `submit_yolo_job` existente mapeia `Err(_)→503` — divergência de ADR-0012 D6
  ("404 weights inexistente") que o smoke da Fatia I não exercitou; **documentar
  na sync e NÃO corrigir nesta fatia** (mudança de comportamento de rota
  existente = escopo separado; registrar dívida).
- **R7 — Contrato interno `DispatchRequest.mode`**: `#[serde(default)]` protege
  intercalação manager-antigo→orquestrador-novo; landing sequenciado
  (J.2 → J.3); smoke valida o par.
- **R8 — Classes do modelo ≠ classes do dataset** (predict em dataset
  diferente do treino): permitido (predict é genérico); o overlay colore por
  classe do PRÓPRIO predictions (model.names), sem conflito (nada é escrito).
- **R9 — test-db apaga `orchestrators`** (regra conhecida): restart do manager
  após qualquer test-db (re-adota no boot); lembrar no plano.

## O que fica falso nos docs (lista para o `@docs-sync`, commit J.9)

**Não aplicar agora** — docs descrevem o que existe, não o que foi aprovado.

- `frontend.md` §3/:63 e Sidebar — módulo "Playground" **habilitado** (badge
  Roadmap sai; `Sidebar.tsx` L238-245).
- `backend.md` §9/:145-146 — bloco jobs ganha `POST /api/jobs/predict`
  implementado (Fatia J; spec 0.12.0); §9/:161-163 — "Adiados: ... runners,
  difusao/clip/autolabel" permanece (predict NÃO é runner).
- `backend.md` §4/:56 — tipos de job: `playground` na lista; emenda: a fatia J
  implementa inferência como **job na fila** (`kind='yolo_predict'`), não como o
  runner do §5; `playground`/runners quentes continuam dívida.
- `backend.md` §5 (playground/runners preemptíveis) e `frontend.md` §10/:252/
  :340 (runner sob demanda) — **permanecem pendentes**; o v1 = job assíncrono na
  fila + página `/playground` com overlay; divergência consciente registrada
  (mesmo desenho do ADR-0008 D0 com o preview efêmero).
- `frontend.md` §10 — novas: `POST /api/jobs/predict` (`lib/playground.ts:
  startPredictJob`, body `{modelId,datasetId,conf?}`), página `/playground`
  (overlay + download de `predictions.json` via proxy).
- `docs/adr/0012-models-real.md` D0 — "Playground/inferência com modelos —
  módulo Playground é Roadmap" → **CUMPRIDA** (esta fatia); `docs/dividas.md`
  :90 — dívida "playground/inferência com modelos" → **QUITADA** (inferência
  YOLO real; runners quentes permanecem).
- `docs/adr/0008-autotracker-v1.md` D6 — nota "AutoTracker real ... modelo local
  (florence-2/qwen-vl)": emenda — o **mecanismo de inferência real** (subcomando
  `predict` + staging de pesos + fila) nasce na Fatia J e é o que a Fatia K
  reaproveita; o modelo open-set/florence continua fatia futura.
- `docs/adr/0012-models-real.md` D5/R7 — `DispatchRequest` ganha `mode`
  (aditivo, `#[serde(default)]`); nada de `weights_ref` muda.
- `backend.md` §10 — tabela `boxes`: `origin` continua `manual|autotracker|
  import` (o `playground` NÃO entra — D4); `jobs` sem migration.
- `dividas.md` — novas: apply de predictions ao dataset (`origin='playground'` +
  rota de apply, com a Fatia K); policy VRAM de predict (entrada na vram-table);
  streaming de detecções por imagem; package "só imagens" para inferência (R2);
  mapping `NotFound/InvalidRequest` do `/api/jobs/yolo` (R6). Reafirmadas:
  runners quentes, reconciliação bucket×banco, test-db isolamento.
- `coordenacao.md` — bloco da fatia J reescrito a cada commit.

## Plano de commits (J.0–J.9; branch `feat/playground-inferencia` de `main`)

Um dispatch = um commit; produção < 400 linhas por commit (contrato/testes fora
da conta — exceção da casa). **Fase 1: J.1 ∥ J.2 ∥ J.4** (ownership disjunto:
engine vs manager vs principal+contracts — J.4 não depende do orquestrador nem
do engine: o principal não lê `predictions.json` no v1); **Fase 2: J.3** (após
J.2 — depende do `mode` no dispatch_body; mesmo contrato interno); **Fase 3:
rebuild+recreate (regra F4.7) → J.5** (frontend via /impeccable); **Fase 4:
J.6 review → J.7 smoke E2E → J.8 sessão GPU → J.9 docs-sync**.

| # | Dono | Conteúdo | Critério de pronto |
|---|---|---|---|
| **J.0** | @architect | Esta ADR (proposta; vira executável após aceite do usuário) | auditoria do coordenador; arquivo commitado em `main` |
| **J.1** | @python-engines | `engines/trainer-yolo`: `predict.py` (`load_and_validate_predict_config` — weights_path obrigatório + `predict.conf` 0..1; `_mock_predict` reusando `_read_dataset`/`_box_for_image`/`_generate_boxes_for_image` → `predictions.json` engine=yolo; `_real_predict` com `YOLO(weights_path).predict(source, conf, imgsz=640, device=0)`, `xywhn`→top-left com clamp, `model.names`) + subcomando `predict` no `main()` + testes (mock determinismo, shape, validação, real monkeypatchado) | `pytest` verde (novos + 57+ existentes — mock intocado); `python -m trainer_yolo predict --config … --output …` mock produz `predictions.json`; fmt ok; ~250-350 linhas |
| **J.2** | @rust-dev (manager) | `dispatch_body` ganha `"mode"` (SELECT já traz; L1824-1833) + `create_job`: SELECT de resolução ganha `model` (variante) e, quando `req.model=="predict"` com variante NOT NULL → `jobs.model=variante`; testes (mode no body, substituição, fine-tune inalterado, 404/400) | `cargo test -p manager -- --ignored` + test-db verdes (**restart do manager depois** — R9); fmt; ~60-120 linhas |
| **J.3** | @rust-dev (orchestrator) | `DispatchRequest.mode: String` (`#[serde(default)]`) + matriz (engine,mode) → subcomando/artefatos (L1088-1105/L1134-1148): (yolo,train)/(yolo,predict)/(autotracker,autotrack)/unsupported limpo; predict NÃO coleta metrics; testes (serde default, matriz, predictions coletado/subido, report done sem metrics) | `cargo test -p orchestrator` verde; fmt; ~100-160 linhas |
| **J.4** | @rust-dev (principal) | `POST /api/jobs/predict` (`validate_predict_request` + `generate_predict_config_yaml` com placeholders + `weights_path` + seção `predict:`; eligibility yolo+imagens>0; manager body `kind='yolo_predict'`/`engine='yolo'`/`mode='predict'`/`model='predict'`/`weights_id`/`vram_min_gb:null`; mapeamento `NotFound→404`, `InvalidRequest→400`, `Unavailable→503` + compensação) + spec **0.12.0** + contract + `PROTECTED_ROUTES` | `cargo test -p api-principal` verde + contract 0.12.0 ≡ router; fmt; ~200-300 linhas |
| **J.5** | @frontend-dev (via /impeccable) | Sidebar "Playground" habilitada + página `/playground` (modelo/dataset/conf → submit → link `/jobs?job=`; lista de jobs predict; overlay de boxes por classe sobre imagens via predictions.json + proxy; download; empty states honestos) + `lib/playground.ts` + tipos | `npm run build --workspace=web` verde; review por página (1 página = 1 fatia = 1 review); DESIGN.md como contrato |
| **J.6** | @reviewer | review do diff J.1–J.5 vs esta ADR (2 partes: rust principal+manager+orchestrator / engine+web; pontos: matriz (engine,mode), `serde(default)`, mapeamento 404/400, mock determinístico, overlay por filename, read-only garantido) | APROVA (com ou sem nits); fixes roteados como commits próprios |
| **J.7** | @coordenador (smoke E2E, fora do CI) | stack mock: dataset yolo+classe+imagem → modelo → `/playground` → submit → `/jobs` done → `predictions.json` com boxes → overlay → download → abort → console limpo | critérios binários da tabela de Testes; teardown limpo; restart do manager pós test-db (R9) |
| **J.8** | @coordenador (sessão GPU, manual) | TrueNAS: predict real com `best.pt` treinado (reuso do da Fatia I) sobre imagens → **detecções reais ≠ mock**; guarda anti-mock (`:local` recusado); teardown completo (repo na main, volumes gpu_* removidos, banco limpo, manager re-adota local) | critério binário D9; checklist README-gpu |
| **J.9** | @docs-sync | Aplica "O que fica falso nos docs" (backend.md §4/§5/§9, frontend.md §3/§10/Sidebar, emendas ADR-0012 D0/D5/R7 e ADR-0008 D6, dividas.md, coordenacao.md) | diff só de docs; conferência doc↔código nos dois sentidos |

**Notas de processo:** mock NUNCA quebra (baterias existentes = critério: pytest
e cargo verdes a cada commit); `cargo fmt --all` antes de reportar; rotas novas
com status exatos em `PROTECTED_ROUTES`; contract test exige spec ≡ router a
cada commit (delta OpenAPI incremental — J.4 declara a rota e o schema no mesmo
commit); despachos de fix nunca editam fora do escopo (reportam ao coordenador).

## Perguntas ao usuário — ABERTAS (aguardam aceite; a ADR só vira executável após resposta)

- **P1 — Transporte (D1):** job assíncrono na fila existente (`POST
  /api/jobs/predict` → 202, reuso total do pipeline). **Recomendação:** sim —
  N imagens × GPU é assíncrono natural; precedente do autotracker (ADR-0008 D0).
  Alternativa: rota síncrona (rejeitada — timeout/restart/UX).
- **P2 — Entrada (D2):** reuso do `build_package` (zip com imagens+labels;
  labels ignoradas). **Recomendação:** sim — custo zero no pipeline; o peso
  extra é aceito. Alternativa: export "só imagens" novo (rejeitada — rota +
  variante de package sem retorno no v1).
- **P3 — Destino (D4):** **só-leitura** no v1 (overlay + download de
  `predictions.json`); "aplicar boxes ao dataset" (`origin='playground'`) fica
  para fatia própria (exigiria migration 0008 + rota de apply + semântica de
  merge). **Recomendação:** sim — ver detecções é o valor; poluir o dataset sem
  fluxo de revisão é risco (lição do apply do autotracker, ADR-0008 D1).
  Alternativa: aplicar no v1 (rejeitada — custo alto, valor baixo).
- **P4 — Engine (D3):** subcomando `predict` reusando os **geradores
  determinísticos** do autotrack com artefato próprio `predictions.json`
  (`engine: "yolo"`). **Recomendação:** sim. Alternativa: reusar `boxes.json`
  literal (rejeitada — `kind` errado, contrato confuso para a Fatia K).
- **P5 — Display (D5):** `jobs.model` do predict = **variante do modelo** (ex.
  `yolo11m`) via substituição no manager (row `models`); sem variante →
  literal `"predict"`. **Recomendação:** sim (display honesto no `/jobs`).
  Alternativa: `"predict"` literal sempre (mais simples, display pior).

**Regra de aceitação:** a ADR-0013 só vira plano executável após o aceite
explícito do usuário (P1–P5); até lá tudo é proposta.
