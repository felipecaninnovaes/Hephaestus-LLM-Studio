# ADR-0008 — AutoTracker v1 (mock, local): execução em lote + retorno de boxes ao Postgres (Fatia 5)

- **Status:** **ACEITA** (2026-09-08 — usuário: "sim aceito", fecha P1–P4). Especificação executável da fatia 5.
  Este documento é a **especificação executável** da fatia 5; os deltas de
  contrato abaixo são aplicados **apenas nos commits da fatia** (openapi junto do
  código, docs de texto no `docs-sync` do fim), nunca antes.
- **Data:** 2026-09-08
- **Componentes:** `services/api-principal` (rota de job autotracker + ingest de
  boxes), `services/manager` (fila — **sem mudança**), `services/orchestrator`
  (seleção de subcomando/artefatos por engine), `engines/trainer-yolo`
  (subcomando `autotrack` mock), `apps/web` (AutoTracker na galeria + modal +
  "Aplicar boxes" em `/jobs`), `packages/contracts` (spec 0.7.0 → **0.8.0**).
- **Fontes:** `IDEIA.md` §3/:39-46 (AutoTracker "gera bounding boxes
  automaticamente, usado para o YOLO"; "Video e Imagem"; modelo local OU upload),
  §2/:34 (YOLO → preparo via AutoTracker); `docs/backend.md` §4/:56 (tipos de
  job incluem `autotracker`), §9/:137 (`POST /api/preview/{autolabel,
  autotracker,...}` job efêmero ou runner quente — **rota de preview vs fila**,
  ver D0), §9/:138-146 (jobs já implementados — reuso total), §10/:240-245
  (`boxes.origin CHECK manual|autotracker|import` — **origem já existe desde a
  0003**), §10/:270-280 (jobs — `engine`/`mode` TEXT livres, sem CHECK), §11/:307
  (config.yaml com placeholders); `docs/frontend.md` §4.3/:74 (context menu
  "Executar AutoLabel ou AutoTracker conforme categoria"; `sample_image`
  "Re-executar AutoTracker"), §5.1/:89 (tag `AutoTracker` no card),
  §7.1/:133-137 (AutoTracker workspace: model conf slider 0.3–0.95 default 0.65,
  check sobrescrever, dataset alvo YOLO), §10/:191-201 (jobs contratos reusados);
  `docs/dividas.md` "Ordem estável de boxes" (:77-83 — PUT boxes DELETE+INSERT
  sem ordem; o AutoTracker escreve em massa e deve decidir se honra/agrava);
  `docs/adr/0007-jobs-v1.md` (fonte do ciclo: fila manager, dispatch,
  artefatos, D8 retorno). Código: `services/api-principal/src/jobs/handlers.rs`
  (submit_yolo_job L471-619, get_artifact_data L372-436),
  `services/api-principal/src/jobs/models.rs` (validate_yolo_request,
  generate_config_yaml L154-188), `services/api-principal/src/datasets/
  handlers.rs` (put_boxes L1360-1495 — DELETE+INSERT transacional,
  unnest RETURNING), `services/api-principal/src/datasets/package.rs`
  (build_package L265-517; snapshot `SnapshotClass{index,name}` sem id),
  `services/manager/src/lib.rs` (create_job L254-302, dispatch_next L922-1016 —
  genérico, não depende de engine), `services/orchestrator/src/lib.rs`
  (run_job_inner L670-939 — subcomando `"train"` hardcoded L849-855, artefatos
  fixos L874-878; parse_metrics_line L189-203),
  `engines/trainer-yolo/src/trainer_yolo/train.py` (CLI train L240-260),
  `apps/web/app/(studio)/datasets/[id]/page.tsx` (AutoTracker disabled L736-744/
  821-829), `packages/contracts/openapi.yaml` (0.7.0).
- **Sequência:** 4 (mergeada) → **5 (esta)** → dívidas F4.8 (proposta B) → clip/
  difusão (proposta C) → backlog (D).

## Contexto

A fatia 4 entregou o ciclo vertical completo de **jobs**: `POST /api/jobs/yolo`
→ fila no manager → orquestrador-local (docker run do trainer-yolo) → artefatos
de volta via S3 escopado → report push → leitura BFF pelo principal → UI `/jobs`.
O AutoTracker é a **outra metade do loop YOLO**: a ferramenta de preparo de dados
que gera as boxes que alimentam o treino. O schema já a suporta desde a 0003
(`boxes.origin='autotracker'`, `conf`), a UI já tem os ganchos (botão AutoTracker
desabilitado na galeria, context menu "Executar AutoTracker"/"Re-executar",
badge `autoTracked` derivado por `EXISTS(boxes.origin='autotracker')`) e
`jobs.engine` é TEXT livre (sem CHECK) — **engine novo NÃO exige migration**.

O problema de desenho central desta fatia é o **retorno de dados**: as boxes são
geradas pelo engine num container **sem credencial no Postgres** (boundary rígido
da fatia 4 — orquestrador stateless, só S3 escopado a `packages/*`+`artifacts/*`),
e o dono de `boxes` é o **principal**. Como as boxes voltam? (D1). A isso se
somam: rota na fila vs preview efêmero (D0), engine mock (D2), params (D3), UI
(D4), e os recortes conscientes de vídeo e upload de modelo (D5/D6).

Regras da casa aplicadas: wire camelCase em `/api/*`; artefatos de transporte
(`boxes.json`, `config.yaml`) snake_case (ADR-0002 D1); erros `{code,message}`
com `message` estática; engine mock atrás de `ENGINE_MOCK=1` com default seguro;
GPU real só `@gpu` manual fora do compose; PRs < ~400 linhas de produção;
branch `feat/autotracker-v1`.

## Decisões já travadas (base — citar, não redecidir)

- **`boxes.origin` já aceita `'autotracker'`** (migration 0003, CHECK `manual|
  autotracker|import`) e a coluna `conf DOUBLE NULL` existe — **nenhuma migration
  de schema para esta fatia** (confirmado). `track_id` disponível para tracking
  futuro (frontend.md §7.1; API não impõe).
- **`jobs.engine`/`mode`/`kind` são TEXT livres** (0006, sem CHECK) — registrar
  `engine='autotracker'`, `kind='autotracker'`, `mode='autotrack'` sem migration.
- **`POST /api/jobs/yolo` e o pipeline manager→orquestrador→artefatos são
  genéricos**: `create_job`/`dispatch_next` do manager não ramificam por engine;
  o orquestrador é o único que hardcoda `train`/artefatos (D2). O principal é o
  único que valida por tipo.
- **`PUT boxes` é substituição total transacional** por imagem (DELETE+INSERT
  com `RETURNING` via unnest, cap 1000, domínio 0..1, `classId` pertence ao
  dataset, dono = principal). O ingest do AutoTracker reusa **a mesma transação
  e a mesma classe de INSERT** (D1).
- **Contadores/derivações automáticos**: trigger `heph_refresh_dataset_counters`
  recalcula `labeled_count`/`images_count`/`status` a partir de `boxes`/`captions`
  (0003); `autoTracked` deriva de `EXISTS(boxes.origin='autotracker')` (T7
  quitada). **Ingerir boxes com `origin='autotracker'` já atualiza o badge e o
  progresso sem código extra.**
- **Orquestrador não toca Postgres**; manager é dono de `jobs`/`job_artifacts`;
  principal é dono de `boxes`/`datasets`. O retorno de boxes **só pode ocorrer
  pelo principal**, lendo o artefato que o orquestrador subiu ao bucket (D1).
- **`job_artifacts`** grava `kind/path/md5/bytes` por job a partir do report
  `done` (ADR-0007 D8); `path` é relativo a `artifacts/<job_id>/`. O ingest
  localiza o artefato de boxes por essa tabela (via manager).

## Decisões

### D0 — Rota: job na fila existente (`POST /api/jobs/autotracker`), NÃO preview efêmero

**Decidido:** o AutoTracker v1 é um **job na fila existente** —
`POST /api/jobs/autotracker` → 202, fluindo pelo mesmo pipeline da fatia 4
(manager FIFO + orquestrador-local + artefatos + `/jobs`). NÃO usar
`POST /api/preview/autotracker` (backend.md §9/:137) nesta fatia.

*Por quê:* a fatia 4 provou o pipeline de ponta a ponta; reusá-lo para o
AutoTracker custa uma rota de submit + um subcomando no engine + o ingest —
quase nada de novo em manager/orquestrador. O preview efêmero (sandbox de
"Testar Inferência" em uma imagem, frontend.md §7.1) é uma UX **distinta** do
"Executar AutoTracker no Dataset" em lote (frontend.md §9 fluxo 3): o lote é
assíncrono, durável e audível — exatamente o que a fila faz. Fazer o preview na
v1 sem o runner quente (playground fora) seria uma rota nova sem consumidor de
tempo real e sem estado (lição P1 da ADR-0006). O sandbox de uma imagem fica
registrado como fatia futura junto com o runner. *Gotcha:* custo do job = package
build + container run (mais pesado que um preview); aceito local mock.

**Decidido — semântica "Re-executar AutoTracker":** o lote roda **sobre o
dataset inteiro** (package). "Re-executar" (frontend.md §4.3, `sample_image`) é a
mesma operação de aplicar resultados de um job novo, que **substitui apenas as
boxes de origem `autotracker`** das imagens afetadas (D1) — as manuais/import são
preservadas por padrão. A variante por-imagem ("re-executar nesta imagem") é um
recorte futuro: a rota de apply já aceita `imageId` opcional (D1), mas a UI v1
não a expõe.

**Descartado:** preview efêmero sem fila (rota nova sem estado/consumidor real);
re-executar por imagem na v1 (o job processa o package inteiro; filtrar por
imagem no apply é trivial depois — D1).

### D1 — D-return: ingest pós-done pelo principal, via artefato `boxes.json`, merge dirigido por origem

**Decidido — fluxo:** o engine grava um artefato **`boxes.json`** em
`artifacts/<job_id>/` (subido pelo orquestrador no report `done`, igual aos
demais artefatos da fatia 4). Quando o job está `done`, a UI chama uma rota nova
**`POST /api/jobs/:id/autotracker/apply`**; o principal:
1. busca o job no manager (confirma `status='done'`, `engine='autotracker'`,
   `dataset_id` presente — se `null` por dataset deletado → 409/404);
2. localiza o artefato `boxes.json` via `list_artifacts` do manager (metadata +
   **reuso da validação de path** `validate_artifact_path` — defesa em profundidade,
   como `get_artifact_data`), lê o objeto via `StoragePort.get` (admin) e confere
   o `md5` (barato, bytes já em RAM — espelho de `get_artifact_data`);
3. mapeia **filename → image_id** (imagens ativas do dataset) e **class name →
   class_id** (por `UNIQUE(dataset_id, name)`), e escreve as boxes com
   `origin='autotracker'`, `conf` do engine, **na mesma transação/INSERT do
   `PUT boxes`** (unnest + RETURNING).

*Por quê — artefato em vez de report push estendido:* o report push
orquestrador→manager (ADR-0007 D4) não carrega payload de boxes (é só
`{status,progress,metrics,artifacts}`) e o orquestrador não tem Postgres; o
artefato no bucket é o **único canal que já existe** para dados do engine voltarem
e sobrevive a restart (S3, não volume). *Por quê — apply explícito em vez de
auto-ingest no `done`:* a fila/manager não tem chamada de volta ao principal
(topologia é principal→manager→orquestrador; adicionar manager→principal seria
topologia nova) e a ingestão altera o dataset do usuário — o **consentimento
explícito** ("Aplicar boxes") é mais seguro e resiliente a refresh. Idempotente
por natureza (D1a).

**Decidido — formato do artefato `boxes.json`** (snake_case, transporte):
```json
{ "engine": "autotracker", "model": "mock", "seed": 42, "conf": 0.65,
  "images": [ { "filename": "img_0001.jpg",
                "boxes": [ { "class": "solda_fria", "x": 0.1, "y": 0.2,
                             "w": 0.3, "h": 0.4, "conf": 0.96 } ] } ] }
```
Chaveado por **filename** (o engine não conhece image UUID) e por **class name**
(do `dataset.yaml` do package; robusto a reordenação de classes). O principal
resolve filename→image_id e name→class_id no ingest.

**Decidido (D1a) — política de escrita por imagem (merge dirigido por origem):**
- `overwrite=false` (default): **DELETE só das boxes `origin='autotracker'`**
  daquela imagem, depois INSERT das boxes do engine — **preserva manual/import**.
  Re-aplicar o mesmo job = mesmo resultado (idempotente); aplicar outro job de
  autotracker = substitui o autotracker anterior (last-write-wins por origem).
- `overwrite=true` (check "sobrescrever", frontend.md §7.1): DELETE **total** da
  imagem + INSERT — mesmo comportamento do `PUT boxes` manual.
- Imagem sem boxes no resultado do engine: **intocada** (nada apaga).
- Cap 1000/imagem (mesma regra do PUT boxes): se o engine emitir >1000 numa
  imagem, **trunca em 1000** com contagem registrada no response (v1 mock não
  atinge; modelo real precisará de bounding).
- Classe do engine inexistente/renomeada no dataset atual → **skip da box** com
  contagem `skipped` (não falha o ingest; residual documentado).
- Imagem do artefato inexistente (deletada) → skip com contagem.

*Por quê — merge por origem e não substituição total no default:* o AutoTracker
prepara dados para revisão manual (frontend.md §9 fluxo 4); apagar manual no
default destruiria trabalho humano. O `overwrite=true` é a saída explícita.
*Gotcha:* re-aplicar sobrescreve **qualquer** ajuste manual feito sobre boxes de
origem `autotracker` (é a semântica de "re-executar"); documentar na UI.
*Gotcha — dívida "Ordem estável de boxes" (dividas.md :77-83):* o ingest agrava a
dívida (INSERT em massa sem ordem determinística). **Não a honra nesta fatia**
(efeitos cosméticos — a UI correlaciona por índice pós-save); registra-se que a
fatia que "conserta caixas existentes" (a própria dívida pede o autotracker como
gatilho) deve introduzir `ORDER BY`/coluna de ordenação — o apply não a piora
semanticamente porque o merge por origem é atômico por imagem.

**Decidido — donos/boundary:** o ingest é **100% do principal** (único dono de
`boxes`); o manager só fornece o estado do job e a metadata do artefato; o
orquestrador só sobe o `boxes.json` no bucket. Nenhuma credencial nova de S3 nem
de Postgres é criada.

**Descartado:** orquestrador escrevendo boxes (viola stateless + boundary);
auto-ingest no `done` via callback novo manager→principal (topologia nova sem
necessidade); reuso da rota de download `.../artifacts/:id/data` via HTTP interno
(rota é para o browser; o principal lê o bucket direto com a mesma defesa de
path/md5); engine emitindo por image UUID (não conhece ids).

### D2 — D-engine: subcomando `autotrack` no `trainer-yolo` (reuso da imagem), mock determinístico

**Decidido:** a v1 **reusa a imagem `hephaestus/trainer-yolo:local`** com um
**subcomando novo `autotrack`** (`python -m trainer_yolo autotrack --config
<config.yaml> --output <output_path>`), em vez de criar uma imagem
`runner-autotracker`. *Rationale:* o mock é stdlib puro (sem deps pesadas — o
mock de autotrack não precisa de modelo), reusar a imagem custa **zero infra
nova** e reaproveita o build do compose da fatia 4. Registra-se a divergência
consciente com backend.md §4/:60 ("uma imagem por engine"): a v1 é **mock**, e o
modelo real (florence-2/qwen-vl, frontend.md §7.1) é uma fatia futura que **terá
sua própria imagem/runner** — a D6 documenta esse caminho.

O `autotrack` mock (com `ENGINE_MOCK=1`, default):
- valida `config.yaml` com shape próprio (`autotrack:` em vez de `yolo:`) e
  valida `dataset_path` (contém imagens + `dataset.yaml`);
- lê classes e a lista de imagens do `dataset.yaml`, e para cada imagem gera
  **boxes determinísticas** (mesmo `(seed, filename, class)` → mesmas coordenadas,
  padrão `_seed_bytes` do train), com `conf` determinístico;
- grava **`boxes.json`** (formato D1) + **`metrics.jsonl` com 1 linha nas mesmas
  6 keys** (`epoch=1, box_loss, cls_loss, dfl_loss, mAP50, mAP50-95`) para **reusar
  o parser `parse_metrics_line` do orquestrador sem mudança** (L189-203).

**Decidido — mudança mínima no orquestrador (única no boundary manager↔
orquestrador):** em `run_job_inner` (L849-855, L874-878), o subcomando e a lista
de artefatos a coletar passam a depender de `dispatch.engine`:
- `engine=="yolo"` → subcomando `train`, artefatos `[best.pt model, last.pt
  model, metrics.jsonl metrics]` (inalterado);
- `engine=="autotracker"` → subcomando `autotrack`, artefatos `[boxes.json boxes,
  metrics.jsonl metrics]`.

O `dispatch` já carrega `engine` (o manager o repassa de `jobs.engine`), então o
orquestrador ramifica sem contrato novo. Testes unit de `run_job_inner` cobrem os
dois casos (args + artefatos).

**Descartado:** imagem nova na v1 (sem modelo real para justificar — custo de
infra e build sem retorno); mock de autotrack que dependa de modelo/peso (não há
upload de modelo na v1 — D6).

### D3 — Params v1: `{model, conf}` + `config.yaml` com seção `autotrack`

**Decidido:** `POST /api/jobs/autotracker` body (wire camelCase):
`{datasetId, model?, conf?}` →
- `model`: **`'mock'`** apenas na v1 (400 `invalid_request` nos demais; os ids
  reais de frontend.md §7.1 — `florence-2-large`, `yolov8x-world`, `qwen2-vl-7b` —
  ficam para a fatia real);
- `conf` (opcional, default `0.65`, domínio `0..1` — alinhado ao slider
  frontend.md §7.1 0.3–0.95): limiar de confiança passado ao engine e gravado em
  `boxes.conf`;
- `model` é opcional com default `mock` na v1 (aceita apenas `mock`; valor diverso
  → 400 `invalid_request`).

`jobs.params` JSONB: `{model, conf}` + `package_ref` (reuso do
builder). `jobs.config_yaml` (snake_case, placeholders):
```yaml
job_id: "{job_id}"
engine: "autotracker"
model: "mock"
mode: "autotrack"
dataset_path: "{dataset_path}"
output_path: "{output_path}"
seed: 42
autotrack:
  model: "mock"
  conf: 0.65
```
Validação pura em `jobs/models.rs` (`validate_autotrack_request`), espelho de
`validate_yolo_request`. `datasetId` não-UUID/inexistente → 404 (D8); dataset
não-pronto (category≠yolo, 0 classes, 0 imagens ativas) → 409 `dataset_not_ready`
(mesma regra do yolo). `mode='autotrack'` (jobs.mode TEXT livre; não colide com
`train`/`infer` da vram-table, que no mock é no-op).

**Descartado:** parâmetro de classes-alvo na v1 (o mock rotula todas as classes;
classes-alvo é da fatia do modelo real); upload de modelo na v1 (D6).

### D4 — UI v1: galeria → modal → `/jobs` → "Aplicar boxes" → editor (origin autotracker)

**Decidido (contratos §10 em alto nível; docs-sync detalha no fecho):**
- **Habilitar AutoTracker** na galeria (botão `AutoTracker` L736-744/821-829 e
  context menu "Executar AutoLabel ou AutoTracker conforme categoria" §4.3) **só
  para `category==='yolo'`** com `classes.length>0 && imagesCount>0`; senão
  disabled com `title` honesto. (AutoLabel continua desabilitado — fatia futura.)
- **Modal AutoTracker** (`AutoTrackerModal`): `model` (só "mock"), slider `conf`
  0.3–0.95 default 0.65, checkbox "sobrescrever anotações existentes"
  (`overwrite`) → CTA "Executar" → `POST /api/jobs/autotracker` → toast 202 →
  navega para `/jobs` (reuso total do painel de jobs da fatia 4).
- **Em `/jobs`**: job autotracker usa o mesmo `Job`/`/metrics`/`artifacts`
  existentes; quando `status==='done'` e `engine==='autotracker'`, a UI mostra
  botão **"Aplicar boxes ao dataset"** → `POST /api/jobs/:id/autotracker/apply`
  (`{overwrite}`) → toast com `{applied, skipped}` → link/refresh da galeria.
- **Galeria/editor**: após aplicar, as boxes aparecem no editor com origem
  `autotracker` (o `BoxResponse` já carrega `origin`/`conf` — indicador visual de
  origem e conf é dado derivado, cor consistente com o design-system v2; badge
  `autoTracked` no card da lista já deriva do banco — zero código extra).
- **"Re-executar AutoTracker"** por imagem (§4.3 `sample_image`): **fora da v1**
  (o job é de dataset; a rota de apply aceita `imageId` — D1 — mas a UI não
  expõe). Registrado como fatia futura.
- Estilo: design-system.md v2 (brand translúcido, One CTA, toasts por `code`).

**Descartado:** AutoLabel junto nesta fatia (é difusão/openclip, outro retorno —
`captions`); re-executar por imagem na UI v1; painel de sandbox/preview
(frontend.md §7.1 sandbox) — sem runner quente na v1.

### D5 — Vídeo: FORA da v1 (recorte consciente)

**Decidido:** a v1 é **imagem apenas**. `IDEIA.md` §3/:42 diz "Video e Imagem",
mas a tabela `videos` (0003) **não tem rota de escrita** e o tracking por frames
(exigiria extração de frames + associação de track) é uma fatia inteira. Registra
dívida em `docs/dividas.md`: "AutoTracker de vídeo — extração de frames +
tracking por `track_id` (`boxes.track_id` já existe) + rota de escrita de
`videos`". *Por quê:* honestidade de escopo — a v1 prova o retorno de boxes ao
Postgres, que é o mecanismo que o vídeo também usará.

### D6 — Upload de modelo: FORA da v1 (recorte consciente)

**Decidido:** o modelo é o mock determinístico embutido; `model='mock'` apenas.
`IDEIA.md` §3/:37/:45-46 permite modelo local OU por upload — mas o upload real
(rota `POST /api/models/upload`, backend.md §9/:136) e o modelo real
(florence-2/qwen-vl) exigem runner/imagem próprios e GPU. Registra dívida/fatia
futura: "AutoTracker real — modelo local (`florence-2-large`, `yolov8x-world`,
`qwen2-vl-7b` em frontend.md §7.1) + upload de modelo + imagem
`runner-autotracker` própria (honra backend.md §4 uma-imagem-por-engine) + classe
open-set mapeada para classes existentes/adicionadas". *Por quê:* dev CPU-only,
`ENGINE_MOCK=1` é o caminho verificável (regra da casa); modelo real é `@gpu`
manual e fatia própria.

### D7 — Testes

**Decidido — padrão da casa (unit + contract + test-db + smoke E2E), spec
0.7.0 → 0.8.0.** Casos-chave:
- **unit principal** (`jobs/models.rs`): validação `validate_autotrack_request`
  (model∈{mock}, conf domínio, datasetId UUID); `generate_autotrack_config_yaml`
  (shape `autotrack:` + placeholders); parser do `boxes.json` (D1) e resolução
  filename→image_id / class name→class_id.
- **unit orquestrador**: ramificação `engine` → subcomando `train` vs `autotrack`
  e artefatos `[boxes.json boxes, metrics.jsonl metrics]`; `boxes.json` coletado e
  subido; metrics 1-linha parseada por `parse_metrics_line` inalterado.
- **unit engine**: mock `autotrack` determinístico (mesmo seed → mesmas boxes);
  `boxes.json` com shape D1; 1 linha de metrics nas 6 keys.
- **contract**: spec 0.8.0 ≡ router (2 rotas novas + erro novo).
- **test-db** (extensão): ingest idempotente (aplicar 2× o mesmo job → mesmas
  boxes, sem duplicação); **preservação de manual** (`overwrite=false`: box manual
  permanece; box autotracker substituída); `overwrite=true` (substituição total);
  re-executar (job novo substitui autotracker anterior, manual preservada);
  `job_not_done` (apply antes de done); `dataset_not_ready` (0 classes/imagens);
  abort em autotrack (ciclo via manager, igual ao yolo); cap 1000 truncado; classe
  inexistente → skip; imagem deletada → skip; `dataset_id` null (dataset deletado)
  → 404/409.
- **smoke E2E** (extensão do smoke_f4): criar dataset yolo + classe + imagem →
  **AutoTracker** → modal (conf/overwrite) → 202 → `/jobs` → done → **Aplicar
  boxes** → galeria com boxes (origin autotracker) → badge `autoTracked` →
  editor mostra as boxes → re-executar substitui → abort num segundo job →
  console limpo.

### D8 — Versionamento OpenAPI e plano de commits (A.1–A.8)

**Decidido:** spec **0.7.0 → 0.8.0** (regra "versão = ordem de landing",
ADR-0005 D1), branch **`feat/autotracker-v1`**. Contract ≡ router a cada commit.
**A.1 é a base do contrato do artefato** (engine produz `boxes.json`); A.2
(principal, job submit) e A.3 (orquestrador) paralelizáveis (ownership disjunto);
A.4 (principal, apply) depois de A.3 (depende do contrato do artefato + ramificação
do orquestrador); A.6 (frontend) depois de A.2/A.4; A.7 reviewer; A.8 docs-sync.
Sem migration de banco → `test-db.sh` não muda de schema, mas ganha os casos de
ingest. Ver tabela do §"Plano de commits".

## Migration

**Nenhuma.** `boxes.origin='autotracker'`/`conf` existem desde a 0003;
`jobs.engine/mode/kind` são TEXT livres desde a 0006. (Confirmação pedida na
coordenacao.md — verificado: migration 0006 cria `engine TEXT NOT NULL, mode TEXT
NOT NULL, kind TEXT NOT NULL` sem CHECK.) Nada de `ALTER` nesta fatia.

## Delta OpenAPI (0.8.0) — descrição na ADR

Rotas novas (entram em `PROTECTED_ROUTES` com status exatos):
```
POST /api/jobs/autotracker                        202 400 401 404 409 503
POST /api/jobs/:id/autotracker/apply              200 400 401 404 409 503
```
- `POST /api/jobs/autotracker` — body `{datasetId, model?, conf?}` →
  202 `SubmitJobResponse{jobId,status:"queued",queuePosition?}`; 400
  `invalid_request` (model∉{mock}, conf fora de 0..1) | 404 dataset | 409
  `dataset_not_ready` (category≠yolo, 0 classes, 0 imagens) | 503
  `queue_unavailable` (manager).
- `POST /api/jobs/:id/autotracker/apply` — body `{overwrite?: bool, imageId?:
  string}` → 200 `{applied, skipped, images}` onde `applied` = boxes gravadas,
  `skipped` = boxes ignoradas (classe/imagem inexistente ou cap), `images` = nº de
  imagens que receberam ao menos uma box | 400 `invalid_request` | 404 (job
  não autotracker / id não-UUID) | **409 `job_not_done`** (job não está `done`;
  erro novo) | 409 `dataset_not_ready` (dataset_id null/deletado) | 503
  `queue_unavailable`/`storage_unavailable` (manager/storage fora).

**Erros novos:** `job_not_done` (409). Reuso: `invalid_request` (400),
`not_found` (404), `dataset_not_ready` (409), `queue_unavailable` (503),
`storage_unavailable` (503). Schema novo: `AutotrackerJobRequest`,
`AutotrackerApplyResponse`. Wire camelCase global; `boxes.json`/`config.yaml`
snake_case (transporte).

## Testes

| Camada | Infra | Prova |
|---|---|---|
| `cargo test -p api-principal` | MockStorage + pool lazy | validação autotrack (model/conf/UUID); config yaml autotrack; parser `boxes.json` + resolução filename/class; inventário de rotas ≡ spec 0.8.0; 401; `dataset_not_ready`/`job_not_done`/`invalid_request`; manager mockado (202/503) |
| `cargo test -p orchestrator` | mocks | ramificação engine→subcomando/artefatos (yolo vs autotracker); `boxes.json` coletado/subido; metrics 1-linha parseada |
| `cargo test -p api-principal -- --ignored` + `scripts/test-db.sh` estendido | Postgres do compose | ingest idempotente; preservação de manual; `overwrite` total; re-executar; `job_not_done`; `dataset_not_ready`; cap 1000; skip de classe/imagem inexistente; dataset_id null |
| `python -m pytest` (trainer-yolo) | local | mock autotrack determinístico; shape `boxes.json`; metrics 1-linha |
| E2E smoke (stack `ENGINE_MOCK=1`, `EXEC_MODE=docker`, Chrome) | compose completo | criar yolo + classe + imagem → AutoTracker → modal → 202 → `/jobs` → done → Aplicar boxes → galeria/editor com boxes autotracker → badge → re-executar → abort → console limpo. Critério: **boxes do engine chegam ao Postgres via principal e aparecem na UI** |

## Spike obrigatório? — **NÃO**

Não há premissa externa não verificada: S3 por prefixo, trixie/aws-lc e docker-run
do trainer já foram provados no spike F4.0 da fatia 4; o orquestrador só ramifica
por `engine` (código nosso, testável por unit); o parser de metrics é reusado sem
mudança. O `boxes.json` é um contrato interno entre engine (A.1) e orquestrador/
principal (A.3/A.4), fechado na própria ADR (D1) — verificado por testes, não por
spike. *Inverteria:* se `parse_metrics_line` não aceitasse o metrics 1-linha de
autotrack sem mudança → A.1 adiciona um parser tolerante (fácil, sem spike).

## Riscos

- **R1 — Drift de classes entre package e apply:** o snapshot
  (`SnapshotClass{index,name}`, sem id) congela classes do pacote; no apply, o
  principal resolve por `name` no dataset atual. Se uma classe foi renomeada/
  deletada pós-job → box skippada (contagem honesta). Aceito na v1 mock;
  documentar. Alternativa futura: snapshot com id de classe + decisão de
  re-mapear.
- **R2 — Ingest parcial:** aplicar numa imagem com box manual `overwrite=true`
  destrói a manual (explícito, consentido); `overwrite=false` preserva. A
  transação é por **imagem** (uma imagem = uma transação DELETE+INSERT), não por
  job inteiro — uma imagem falha não trava as outras; contagem `skipped`.
- **R3 — Cap 1000 truncado silenciosamente:** o mock não atinge; modelo real
  precisará de bounding por imagem + aviso. Registrado.
- **R4 — Job autotracker sem ingest (usuário esquece de aplicar):** as boxes
  ficam só no bucket até o usuário aplicar (apply explícito, D1). Risco de
  "job done mas nada visível" → a UI mostra o botão "Aplicar boxes" prominentemente
  no `/jobs` quando done; sem auto-aplicar.
- **R5 — `boxes.json` grande (muitas imagens):** o artefato é lido inteiro em RAM
  pelo principal (como `get_artifact_data`). Dataset grande → payload grande;
  aceito local mock; otimização (stream/limite) futura se necessário.
- **R6 — Reuso da imagem trainer-yolo para autotrack:** divergência com "uma
  imagem por engine" (D2) — registrada; o modelo real terá imagem própria (D6).
- **R7 — `captions.origin` sem `autolabel`:** inconsistência de nomenclatura de
  produto (backend §10/:244 `CHECK manual|autotracker|import` não tem `autolabel`)
  — **fora do escopo desta fatia** (captions são AutoLabel, não AutoTracker);
  registrada para a fatia de AutoLabel.

## O que fica falso nos docs (lista para o `@docs-sync`, commit A.8)

**Não aplicar agora** — docs descrevem o que existe, não o que foi aprovado.

- `backend.md` §9/:137 — `preview: POST /api/preview/{autolabel,autotracker,...}`
  → permanece pendente (v1 usa a fila, D0); bloco jobs (:138-146) ganha
  `POST /api/jobs/autotracker` + `POST /:id/autotracker/apply` implementados;
  `autotracker` sai de "Adiados" para "implementado (imagem; vídeo pendente)".
- `backend.md` §9/:146 — "Adiados: pause/resume, samples, WS, runners, **outros
  engines**" → `autotracker` passa a implementado (engine), resta "outros engines"
  (difusao/clip/autolabel).
- `backend.md` §4/:60 — "uma imagem por engine" → nota de emenda: autotracker v1
  reusa `trainer-yolo:local` (mock); runner-autotracker real é fatia futura (D2/D6).
- `backend.md` §10/:244 — `captions.origin` sem `autolabel`: permanece (R7).
- `frontend.md` §4.3/:74 — context menu AutoTracker habilitado (yolo); "Re-executar
  AutoTracker" por imagem permanece desabilitado (futuro).
- `frontend.md` §5.1/:89 / §7.1/:133-137 — badge AutoTracker já deriva; workspace
  AutoTracker real continua fatia futura (v1 = modal na galeria, não workspace).
- `frontend.md` §6.1 — "preparo via AutoTracker": o fluxo v1 (galeria→modal→/jobs→
  apply) implementado; sandbox/teste de inferência (§7.1) permanece futuro.
- `dividas.md` — nova dívida "AutoTracker de vídeo" (D5) e "AutoTracker real com
  upload de modelo" (D6); "Ordem estável de boxes" (:77-83) **reafirmada** (não
  honrada; o apply não a piora semanticamente — D1).
- `coordenacao.md` — bloco da fatia 5 reescrito a cada commit; `plano` novo.

## Plano de commits (A.1–A.8; branch `feat/autotracker-v1` de `main`)

Um dispatch = um commit; produção < 400 linhas por commit (contrato/testes fora da
conta — exceção da casa). **A.1 define o contrato do artefato**; **A.2 e A.3
paralelizáveis** (ownership disjunto: `services/api-principal`+`contracts` vs
`services/orchestrator`); **A.4 depois de A.3** (depende do contrato do artefato +
ramificação do orquestrador); A.6 depois de A.2/A.4; A.7/A.8 ao fim. **Sem
migration e sem mudança de compose/CI** (imagem reusada) — A.5 infra **não é
necessário**; se o build da imagem já existente acusar algo, vira um commit de
fix marcado (fora do esperado).

| # | Dono | Conteúdo | Critério de pronto |
|---|---|---|---|
| **A.1** | @python-engines | `trainer_yolo/autotrack.py` (mock determinístico: lê dataset.yaml classes+imagens, gera `boxes.json` shape D1 + `metrics.jsonl` 1-linha 6 keys) + subcomando `autotrack` no `main`/CLI + testes (determinismo, shape) | `pytest` verde; `python -m trainer_yolo autotrack --config … --output …` com mock produz `boxes.json` + `metrics.jsonl`; fmt ok |
| **A.2** | @rust-dev (principal) | `POST /api/jobs/autotracker` (validação `models.rs` + `generate_autotrack_config_yaml` + manager call) + spec 0.8.0 declarando SÓ a rota de submit + erro reusado | `cargo test -p api-principal` verde; contract≡router; fmt limpo |
| **A.3** | @rust-dev (orchestrator) | ramificação `engine` em `run_job_inner` (subcomando `autotrack` + artefatos `[boxes.json boxes, metrics.jsonl metrics]`) + testes unit dos 2 casos | `cargo test -p orchestrator` verde (args + artefatos por engine); fmt limpo |
| **A.4** | @rust-dev (principal) | `POST /api/jobs/:id/autotracker/apply` (job via manager → status done → artefato `boxes.json` via list_artifacts + path/md5 → resolve filename/class → transação merge por origem/overwrite, cap 1000, contagens) + erro `job_not_done` + spec 0.8.0 completa | `cargo test -p api-principal` verde; test-db com casos de ingest; contract≡router; fmt limpo |
| **A.5** | — (não necessário) | sem mudança de compose/CI/infra (imagem reusada) | — |
| **A.6** | @frontend-dev | habilitar AutoTracker (galeria+context menu, yolo com classes+imagens) + `AutoTrackerModal` (model mock, conf slider, overwrite) + `lib/autotracker.ts` (`startAutotrackerJob`, `applyAutotracker`) + botão "Aplicar boxes" no `/jobs` + refresh galeria/editor com origin autotracker | `npm run build --workspace=web` verde; smoke Chrome (AutoTracker→202→/jobs→done→Aplicar→galeria/editor com boxes→badge→re-executar→abort; console limpo) |
| **A.7** | @reviewer | Revisão do diff completo vs esta ADR | invariantes da casa: camelCase, contract≡router a cada commit, boundary (ingest só no principal; orquestrador sem Postgres), merge por origem, `<400`/commit, sem migration |
| **A.8** | @docs-sync | Lista "O que fica falso nos docs" acima + dividas (D5/D6 novas, ordem de boxes reafirmada) | docs descrevem o que existe |

**Notas de processo (lições da fatia 4):** `cargo fmt --all` antes de reportar;
rotas novas com status exatos em `PROTECTED_ROUTES`; contract test exige spec ≡
router a cada commit (delta OpenAPI incremental); A.4 pode dividir em A.4a
(leitura/parse do `boxes.json` + resolução) e A.4b (transação de escrita + erros)
se estourar ~400 linhas de produção — specs incrementais, contract≡router a cada
commit. Despachos de fix nunca editam fora do escopo (reportam ao coordenador).

## Perguntas ao usuário — FECHADAS (2026-09-08: aceite P1–P4)

- **P1 — Retorno de boxes (D1):** ingest pós-done pelo principal via artefato
  `boxes.json`, com apply **explícito** (`POST /:id/autotracker/apply`) e merge
  dirigido por origem (`overwrite=false` preserva manual; `overwrite=true`
  substituição total). **Recomendação:** sim. Alternativa: auto-ingest no `done`
  (rejeitada — topologia nova manager→principal, sem consentimento).
- **P2 — Rota (D0):** job na fila existente (`POST /api/jobs/autotracker`), NÃO
  preview efêmero. **Recomendação:** sim (reuso máximo; preview é sandbox futuro).
- **P3 — Engine (D2):** subcomando `autotrack` no `trainer-yolo` (reuso da
  imagem), divergindo conscientemente de "uma imagem por engine" na v1 mock.
  **Recomendação:** sim (modelo real terá runner próprio — D6).
- **P4 — Escopo (D5/D6):** vídeo e upload de modelo ficam FORA da v1 (dívidas
  registradas). **Recomendação:** sim (sem rota de escrita de `videos`; mock).

**Aceitas (2026-09-08)**: P1–P4 conforme recomendado. Branch: `feat/autotracker-v1`.
