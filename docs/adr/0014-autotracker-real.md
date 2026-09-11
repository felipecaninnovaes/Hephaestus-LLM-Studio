# ADR-0014 — AutoTracker real: yolov8x-world (open-set) na plumbagem das Fatias I/J (Fatia K)

- **Status:** **PROPOSTA** (aguarda aceite do usuário). Nada implementado. Este
  documento é a especificação executável da fatia "AUTOTRACKER REAL"; os deltas
  de contrato abaixo são aplicados **apenas nos commits da fatia** (openapi junto
  do código, docs de texto no `docs-sync` do fim), nunca antes.
- **Data:** 2026-09-11
- **Componentes:** `services/api-principal` (BFF: `modelId` no
  `POST /api/jobs/autotracker`, validação + config com `weights_path`, aceite de
  `engine='world'` no upload/download — apply INTOCADO), `services/manager`
  (resolução de `weights_id` aceita engine `world`; `jobs.model` variante/
  `"world"`), `services/orchestrator` (**zero delta de produção** — testes
  novos), `engines/trainer-yolo` (modo real do subcomando `autotrack`:
  `YOLO(weights_path)` + `set_classes(classes do dataset)` → `boxes.json` no
  mesmo shape do mock), `apps/web` (dropdown "Modelo" no `AutoTrackerModal` —
  via /impeccable, 1 modal = 1 review), `packages/contracts` (spec 0.12.0 →
  **0.13.0**), migration **0008** (CHECK `models.engine` ganha `'world'`).
- **Fontes:** `IDEIA.md` §3/:37/:45-46 (modelo local OU upload; "gera bounding
  boxes automaticamente, usado para o YOLO"); `docs/adr/0008-autotracker-v1.md`
  (D1 formato do `boxes.json` + D1a merge por origem do apply; D2 subcomando
  `autotrack`; D3 params `{datasetId, model?, conf?}`; D6 dívida "AutoTracker
  real"); `docs/adr/0012-models-real.md` (D1 dono/escopo `models/*`; D3 upload
  `.pt`+magic PK+2 GiB; D4 download por URL allow-list fail-closed; D5
  `weights_id`→`weights_ref`, staging `outputs/<job_id>/weights/<name>`,
  placeholder `{weights_path}`); `docs/adr/0013-playground-inferencia.md` (D3
  padrão `_real_predict`/`_xywhn_to_topleft_clamped`; D5 `mode` no dispatch +
  `jobs.model` variante; D6 matriz `(engine,mode)` — linha `("autotracker",
  "autotrack")` **inalterada**; D8 mapeamento `NotFound→404`/`InvalidRequest→400`;
  R4 mock honesto com weights). Código (verificado por graft nesta data):
  `services/api-principal/src/jobs/handlers.rs` L670-818
  (`submit_autotracker_job` — manager body sem `weights_id`), L1025-1360
  (`apply_autotracker_boxes` — INTOCADO), `services/api-principal/src/jobs/
  models.rs` L96 (`ALLOWED_AUTOTRACK_MODELS=["mock"]`), L104-110
  (`AutotrackerJobRequest{datasetId,model?,conf?}`), L143-161
  (`generate_autotrack_config_yaml` sem `weights_path`), L199-207
  (`BoxesArtifact.seed: u64` **obrigatório** no parse), L215-251
  (`parse_boxes_json` — não valida `seed`), `services/manager/src/lib.rs`
  L54-67 (`CreateJobRequest.weights_id`), resolução de `weights_id` (ADR-0012
  D5: `engine != 'yolo'` → 400), `services/orchestrator/src/lib.rs` L1096-1123
  (subcomando por `(engine,mode)` — `("autotracker", _)` existe), L1152-1163
  (artefatos `[boxes.json boxes, metrics.jsonl metrics]`), **L1167-1190
  (`if file_path.exists()` — artefato ausente é SKIP, não erro)**,
  `engines/trainer-yolo/src/trainer_yolo/autotrack.py` (L40-68 validação;
  L75-121 `_read_dataset`; L209-259 `_mock_autotrack` — shape `{engine,model,
  seed,conf,images}`; **L266-282 `cmd_autotrack` chama `_mock_autotrack`
  incondicionalmente, sem check `ENGINE_MOCK`**), `predict.py` L115-127
  (`_xywhn_to_topleft_clamped`), L130-192 (`_real_predict` — `YOLO(path)`,
  `predict(source, conf, imgsz=640, device=0)`, `model.names`), L199-219
  (`cmd_predict` com `ENGINE_MOCK` check — padrão a espelhar), `services/api-
  principal/migrations/0007_models.sql` L7 (`CHECK (engine IN ('yolo'))`),
  `packages/contracts/openapi.yaml` (version 0.12.0; rotas autotracker L1492/
  L1538), `apps/web/components/studio/AutoTrackerModal.tsx` L123-131 (select
  `model` fixo `"mock"` disabled), `docs/dividas.md` :88 (dívida "AutoTracker
  real" com emenda da Fatia J), `packages/policies/vram-table.yaml` (só
  entradas `(yolo,*,train)` — `resolve_required_gb` → None ⇒ permissivo).
- **Sequência:** Fatia R1 (fechada) / Fatia J (merge pendente do usuário) →
  **K (esta: AutoTracker real)** → L (difusão treino real) → M (playground
  multi-engine). K é o **segundo consumidor** da plumbagem de inferência
  provada na J (matriz `(engine,mode)`, staging de pesos, tabela `models`,
  upload/download com allow-list).

## Contexto

O AutoTracker v1 (ADR-0008) entrega o fluxo completo **mock**: submit → fila →
orquestrador (`autotrack`) → `boxes.json` determinístico → apply explícito com
merge por origem (`origin='autotracker'` preserva manual/import). O que falta
para o produto fechar a promessa de `IDEIA.md` ("modelo local OU upload") é o
**modelo real**: detectar objetos de verdade nas imagens com classe definida
por texto (open-set), produzindo `boxes.json` no MESMO shape do mock para o
apply existente funcionar **sem mudança**.

A plumbagem que o real precisa já existe e está provada @gpu nas Fatias I/J:
tabela `models` (dono: manager), upload/download com allow-list (teto 2 GiB —
cabe o `yolov8x-worldv2.pt` de ~1.3 GB), `weights_id` → `weights_ref` →
staging `outputs/<job_id>/weights/` + substituição de `{weights_path}`
(orquestrador, genérico — ADR-0012 I.3), matriz `(engine,mode)` do orquestrador
(ADR-0013 D6 — a linha `("autotracker", "autotrack")` existe e **não muda**), e
o padrão `_real_predict` do engine (`YOLO(path).predict` + conversão
`xywhn`→top-left — ADR-0013 D3). **Zero delta esperado em orquestrador**:
evidência no código — staging de pesos é anterior ao `docker run` e não
ramifica por engine; a coleção de artefatos **já tolera arquivo ausente**
(`file_path.exists()` L1169), então o real pode não emitir `metrics.jsonl`
(progresso binário, como o predict) sem falhar o job.

O ponto novo de design desta fatia: **como o modelo world entra no produto**
(D1 — engine `'world'` na tabela), **como o job real é disparado** (D2 —
`modelId` opcional no body existente), **como o engine real gera boxes** (D3 —
`set_classes(classes do dataset)`, mapeamento 1:1 sem heurística), e as
decisões honestas de metadados (D5 — `jobs.model`/VRAM) e de escopo (D0).

Regras da casa aplicadas: wire camelCase em `/api/*`, transporte interno
snake_case (ADR-0002 D1); 404 `not_found` p/ id não-UUID em path (ADR-0002 D8);
erros `{code,message}` estáticos; PRs < ~400 linhas de produção; branch
`feat/autotracker-real`; mock continua default (`ENGINE_MOCK=1`) e **nunca
quebra** (baterias existentes = critério); GPU real atrás de `ENGINE_MOCK=0` +
guarda anti-mock (imagem `:local` recusada com GPU — ADR-0011); frontend SEMPRE
via /impeccable com `docs/DESIGN.md` como contrato, review por página.

## Decisões já travadas (base — citar, não redecidir)

- **`POST /api/jobs/autotracker` existe** (202, fila) com body
  `{datasetId, model?='mock', conf?=0.65}` e `model ∈ {mock}` apenas; manager
  body `{kind:'autotracker', engine:'autotracker', mode:'autotrack',
  model, params{model,conf,package_ref}, vram_min_gb:null}` (ADR-0008 D3;
  `handlers.rs` L756-782). **`generate_autotrack_config_yaml` não emite
  `weights_path`** (models.rs L143-161).
- **`POST /api/jobs/:id/autotracker/apply` existe e é o dono do ingest**:
  merge por origem (`overwrite=false` substitui SÓ `origin='autotracker'`,
  preserva manual/import; `overwrite=true` total), cap 1000, skip honesto de
  classe/imagem inexistente (drift), idempotente, `imageId` opcional (ADR-0008
  D1/D1a; `handlers.rs` L1025-1360). **O apply resolve class por NOME no
  dataset ATUAL** — o engine pode emitir exatamente os nomes do package; renomear
  classe pós-job continua = skip honesto (R1 da ADR-0008).
- **`boxes.json`**: `{engine:"autotracker", model, seed, conf, images:
  [{filename, boxes:[{class,x,y,w,h,conf}]}]}` (ADR-0008 D1). **`seed` é
  `u64` obrigatório no parse** (`BoxesArtifact` L199-207) mas **não é
  validado** por `parse_boxes_json` (L215-251) nem consumido pelo apply — o real
  pode gravar um sentinela (D3).
- **Plumbagem de pesos provada**: `weights_id` → `params.weights_ref{s3_key,
  md5}` no manager (resolução no submit: inexistente → 404, `engine != 'yolo'`
  → 400 — ADR-0012 D5) → staging no orquestrador `outputs/<job_id>/weights/
  <name>` (escopo inferido **por prefixo**: `models/*` → `S3Scope::Models`,
  `artifacts/*` → `Artifacts`) + substituição de `{weights_path}` no config.yaml
  (ADR-0012 I.3; `run_job_inner` L927-973). **O predict (J) reusou isso com
  zero delta** (ADR-0013 D5).
- **Matriz `(engine,mode)` do orquestrador** (ADR-0013 D6): `("autotracker", _)
  → subcomando autotrack + artefatos [boxes.json boxes, metrics.jsonl metrics]`
  (`lib.rs` L1111-1117/L1160). **A coleção de artefatos tolera arquivo ausente**
  (`if file_path.exists()` L1169 — skip; `read_final_metrics` → `None` ⇒ report
  done com `metrics: None`, padrão do predict).
- **`jobs.engine`/`mode`/`kind` são TEXT livres** (0006); `jobs.params` JSONB.
  **`models.engine` tem CHECK `('yolo')`** (0007 L7) — 'world' exige migration.
- **`ENGINE_MOCK=1` default**: `cmd_predict` checa a env (predict.py L215-219);
  **`cmd_autotrack` NÃO checa** (chama `_mock_autotrack` incondicional —
  autotrack.py L266-282) — a K alinha o padrão.
- **`parse_boxes_json` é o contrato do artefato no principal** — o real deve
  produzir um JSON que ele aceite (seed u64, coords/conf 0..1, engine não-vazio,
  filenames únicos).
- **`vram_min_gb: null` + vram-table permissiva** (ADR-0010 D9/ADR-0011 D3):
  entrada faltante ⇒ `resolve_required_gb` = None ⇒ roteamento permissivo; a
  guarda anti-mock (GPU exige imagem ≠ `:local`) é a proteção real de GPU.
- **Spec OpenAPI atual: 0.12.0** (openapi.yaml L4); enum `Error.code` com 19
  códigos — **nenhum código novo nesta fatia**. Versão dos crates 0.1.0.
- **UI**: `AutoTrackerModal` com select `model` fixo `"mock"` disabled
  (L123-131); `lib/autotracker.ts` (`startAutotrackerJob`, `applyAutotracker-
  Boxes`); página `/models` com `listModels()` (Fatia I); `/jobs` com botão
  "Aplicar boxes" quando `done && engine==='autotracker'` (Fatia 5).
- **Sessão GPU é manual e fora do CI** (padrão ADR-0010 G.6 / ADR-0012 I.9 /
  ADR-0013 J.8); TrueNAS com imagem `hephaestus/trainer-yolo:gpu` construída
  (ultralytics 8.3.x — provado no treino real da fatia G).

## Decisões

### D0 — Escopo v1: autotrack REAL com `yolov8x-worldv2.pt` (prompts = classes do dataset); FORA: florence-2/qwen, vídeo, track_id, UI de revisão

**Decidido — entra na fatia:**
- **Modo real do subcomando `autotrack`** com o modelo open-set
  **`yolov8x-worldv2.pt`** do ultralytics (~1.3 GB): as **classes do dataset
  viram os prompts de texto** (`model.set_classes([...])`) — mapeamento 1:1 sem
  heurística (D3).
- **`modelId?: uuid`** no `POST /api/jobs/autotracker` (presente → real;
  ausente → mock — comportamento atual intocado, D2).
- **`engine='world'`** no catálogo `models` (migration 0008; upload/download
  reusados — D1).
- Apply **INTOCADO** (shape do `boxes.json` preservado = prova do contrato).
- Spec **0.13.0**; migration **0008**.

**Decidido — fora (dívida/emenda registrada, NÃO implementado agora):**
- **Florence-2 / Qwen2-VL** (alternativas de modelo open-set do ADR-0008 D6) —
  o v1 usa o world do ultralytics porque roda **na mesma imagem trainer-yolo**
  (regra uma-imagem-por-engine preservada sem runner novo); florence/qwen
  exigiriam imagem própria — dívida.
- **Vídeo** (D5 da ADR-0008) e **track_id** (`boxes.track_id` existe, não é
  preenchido — mesma convenção do mock).
- **Re-executar por imagem** na UI (apply aceita `imageId` desde a 0008; UI
  não expõe — registrado na própria 0008).
- **Classes-alvo configuráveis** (subconjunto das classes do dataset como
  prompts) — o v1 passa TODAS as classes; refino é fatia futura (o mock também
  rotula todas — ADR-0008 D3).

**Descartado:** trocar o engine do job para `'world'`/`'yolo_predict'` (o
orquestrador roteia por `engine='autotracker'` para a imagem trainer-yolo; o
modelo é um peso, não um engine — D2); fazer o apply entender um `origin`
novo (`'playground'` — dívida separada da ADR-0013 D4, o K reusa o ingest
`'autotracker'` existente).

### D1 — O modelo world entra como `engine='world'` na tabela `models` (migration 0008 estende o CHECK); upload/download reusados com validação idêntica (`.pt` + magic PK)

**Decidido (opção a):** a row do yolov8x-world no catálogo nasce com
**`models.engine = 'world'`**:
- **Migration 0008** estende o CHECK: `CHECK (engine IN ('yolo','world'))`
  (recriação da constraint com o mesmo nome; `models_engine_idx` já cobre o
  filtro por engine — sem índice novo).
- **`POST /api/models/upload`** aceita `engine='world'` com **as mesmas regras**
  do yolo: extensão `.pt` + magic `PK\x03\x04` (torch.save zip — o
  `yolov8x-worldv2.pt` é zip, como todo `.pt` moderno), nome sanitizado ≤255,
  teto 2 GiB (folga sobre os ~1.3 GB reais). **A validação por engine não muda
  de mecanismo — só o enum de engines aceitos ganha 'world'** (400
  `invalid_request` nos demais).
- **`POST /api/models/download`** cobre o peso oficial via allow-list
  `MODEL_DOWNLOAD_ALLOWED_HOSTS` (env do usuário; a fatia documenta os hosts
  necessários — ver D8); 1.3 GB < 2 GiB de cap; SSRF fail-closed intocado.

*Por quê (a) e não (b) `engine='yolo'` com nome indicando world:* o engine da
row é a **identidade do mecanismo de inferência** — o yolo-train resolve
`weights_id` com `engine='yolo'` (fine-tune) e o autotracker real vai resolver
com `engine='world'`; misturar os dois (b) obrigaria o fine-tune a aceitar pesos
world (arquitetura world ≠ variante yolo — falha honesta tardia no `YOLO(path)`)
ou o K a distinguir por nome (heurística frágil). `engine='world'` mantém a
regra da ADR-0012 D5 ("engine ≠ esperado → 400") como defesa ativa. O
orquestrador **não precisa conhecer 'world'**: o `scoped_key` infere o escopo
**por prefixo do s3_key** (`models/world/<id>/<name>` → `S3Scope::Models` — já
genérico desde a I.3). *Gotcha:* o dispatch do job real segue `engine=
'autotracker'` (imagem trainer-yolo) — o `'world'` vive **só na tabela models**,
nunca em `jobs.engine`. *Descartado:* (b) `engine='yolo'` poluído (acima);
tabela nova `world_models` (catálogo único é o contrato da I; row de engine
diferente é o mecanismo previsto na própria ADR-0012 D0 "multi-engine além de
yolo entram com migration própria ampliando o CHECK").

### D2 — Transporte: `modelId?: uuid` no body existente; kind/engine/mode CONSTANTES; o engine decide por `weights_path` no config (padrão train/predict)

**Decidido:** o body de `POST /api/jobs/autotracker` ganha **`modelId?: string
(UUID)`** — presente → job REAL; ausente → **mock, comportamento atual
intocado** (o smoke de regressão prova o caminho antigo). O `model` do body
permanece `'mock'` default e validado ∈ {mock} (o real é selecionado por
`modelId`, não por string de modelo — mesmo desenho do predict da ADR-0013 D8).
```
kind   = 'autotracker'   (constante — inalterado)
engine = 'autotracker'   (constante — inalterado; roteia para a imagem trainer-yolo)
mode   = 'autotrack'     (constante — inalterado)
```
**O engine (container) decide mock vs real pelo `weights_path` presente no
config.yaml** — sem `mode` novo, sem campo novo no dispatch: o principal emite
`weights_path: "{weights_path}"` **apenas quando `modelId` presente** (padrão do
fine-tune, ADR-0012 D5) e o `cmd_autotrack` espelha o `cmd_predict`
(`ENGINE_MOCK=1` → mock ignora pesos; `ENGINE_MOCK=0` → real, que exige o
caminho — D3).

*Por quê — coerência com a D6 da ADR-0013:* a matriz do orquestrador tem a linha
`("autotracker", _)` desde a 0008 (subcomando + artefatos) e o staging de pesos
é **anterior e independente** do subcomando (`run_job_inner` L927-973 — qualquer
job com `weights_ref` stagia e substitui `{weights_path}`). Um `mode` novo
(`'autotrack_real'`?) adicionaria linha na matriz e ramificação de artefatos
para ganhar... nada: o engine já sabe o que fazer pelo config. **Zero delta de
contrato interno manager→orquestrador** (o `weights_ref`/`DispatchRequest.mode`
já existem). *Gotcha — mock com modelId:* o orquestrador baixa o peso do bucket
e stagia mesmo em job mock (desperdício local aceito — R8 da ADR-0012; é o
cenário do smoke E2E). *Descartado:* `kind`/`engine`/`mode` novos (o engine é o
MESMO — trainer-yolo rodando autotrack; `jobs.engine='world'` mentiria sobre o
motor e quebraria a matriz); rota nova `POST /api/jobs/autotracker-real`
(duplica contrato sem necessidade).

### D3 — Engine real: `cmd_autotrack` com check `ENGINE_MOCK`; `_real_autotrack` = `YOLO(path)` + `set_classes(classes do dataset)` + `predict` → `boxes.json` NO MESMO SHAPE (seed: 0 sentinela, sem metrics.jsonl)

**Decidido (`autotrack.py`, espelho do padrão predict da ADR-0013 D3):**
1. **`cmd_autotrack`** ganha o check `ENGINE_MOCK` (hoje chama `_mock_autotrack`
   incondicionalmente — L266-282): `mock = os.environ.get("ENGINE_MOCK","1")=="1"`;
   `mock` → `_mock_autotrack` (inalterado); senão → `_real_autotrack`.
2. **Config**: `weights_path` **opcional** na validação (o mock não o tem — ao
   contrário do predict onde é obrigatório); o real faz `_die` honesto se
   ausente (`cfg.get("weights_path")`).
3. **`_real_autotrack(cfg, output)`**:
   - `from ultralytics import YOLO` (lazy, padrão `_real_predict`);
   - `class_names, _ = _read_dataset(dataset_path)` (**as classes do
     `dataset.yaml` do package congelado** — mesmas que o mock usa);
   - `model = YOLO(weights_path)`; **`model.set_classes(class_names)`** — os
     prompts de texto SÃO os nomes das classes do dataset; a saída fica restrita
     a elas (`model.names` = prompts após a chamada — **mapeamento 1:1 sem
     heurística**: qualquer box emitida tem `class` ∈ classes do dataset);
   - `results = model.predict(source=<images_dir|dataset_path>, conf=conf,
     imgsz=640, device=0)` — **mesmo padrão do `_real_predict`** (prefere
     `<dataset_path>/images`); para cada `result`: `fname = Path(r.path).name`,
     `xywhn → _xywhn_to_topleft_clamped` (reuso da função do predict.py,
     import), `class = model.names[int(box.cls[0])]`, `conf = float(box.conf[0])`;
     imagem sem detecção → `boxes: []` (honestidade);
   - **`boxes.json` NO MESMO SHAPE do mock**:
     `{"engine":"autotracker", "model": cfg["model"], "seed": 0, "conf": conf,
     "images": [...]}` — `model` = `cfg["model"]` (literal `"world"` — D6);
     **`seed: 0`** é a sentinela "não-determinístico" (o parse exige `u64`
     presente — L199-207 — mas não valida valor nem o apply consome; **zero
     delta no principal**);
   - **SEM `metrics.jsonl`** no real (não há epochs/loss — progresso binário
     honesto, padrão do predict ADR-0013 D3/D6; o orquestrador **já skipa**
     artefato ausente — L1169).
4. **Mock intocado**: sem `weights_path` (ou com, sob `ENGINE_MOCK=1`) →
   `_mock_autotrack` atual, `metrics.jsonl` 1-linha mantido (baterias existentes
   = critério).

*Por quê — `set_classes` e não pós-filtro por nome:* o world é **open-vocabulary**
(o CLIP está embutido — `cached_clip_model`); `set_classes` é a API canônica do
ultralytics para restringir a saída a conceitos por texto. Pós-filtrar boxes de
classes COCO (o default) por nome seria heurística frágil (só pegaria as ~80
classes conhecidas e exigiria mapeamento nome↔COCO — exatamente o que a tarefa
pede para evitar). *Gotcha:* (a) `set_classes` limita a saída AOS prompts — uma
classe do dataset não presente nas imagens simplesmente não gera box (não é
erro); (b) o peso world é baixado na primeira carga (cache CLIP) — latência de
job maior que o mock, aceita; (c) seed `0` no artefato real documentado no
`docs-sync` (o apply não distingue mock de real pelo JSON — só pelo `model` e
pela ausência de metrics). *Descartado:* real escrever `metrics.jsonl` sintético
(curva fake — rejeitado por honestidade, ADR-0013 D6); `seed: null`/omitido
(quebraria `BoxesArtifact.seed: u64` — delta desnecessário no principal).

### D4 — Orchestrator: ZERO delta de produção (evidência no código); testes novos do caso (autotracker, autotrack) com `weights_ref`

**Decidido:** **nenhuma linha de produção muda no orquestrador.** Evidências no
código atual: (a) subcomando `("autotracker", _) → autotrack` existe
(L1111-1117); (b) artefatos `("autotracker", _) → [boxes.json boxes,
metrics.jsonl metrics]` existem (L1160) e a coleção **skipa arquivo ausente**
(`file_path.exists()` L1169) — o real sem metrics.jsonl sobe só `boxes.json`;
(c) o report done lê `read_final_metrics` → `None` → `metrics: None, epoch:
None` (padrão do predict — progresso binário 0→100 na UI); (d) o staging de
pesos é genérico (L927-973; escopo por prefixo `models/*` → `S3Scope::Models`
— a row world cai nele sem conhecer `'world'`); (e) `replace_config_placeholders`
substitui `{weights_path}` se presente (ADR-0012 I.3). A K **adiciona apenas
testes unit** (K.4): dispatch `(autotracker, autotrack)` com `weights_ref` →
staging + `{weights_path}` substituído no config + `boxes.json` subido + sem
`metrics.jsonl` → done com `metrics: None` (regressão do caso sem weights
inalterada).

*Por quê — zero delta e não "artefatos condicionais":* o skip já existe por
desenho (o mock de train também pode não produzir `best.pt` — `_copy_flat_weights`
logar warning); condicionar a lista de artefatos ao `weights_ref` adicionaria
estado ao orquestrador para resolver um caso que o código já trata. *Gotcha:*
o `container_name = trainer-<engine>-<job_id>` deriva de `engine='autotracker'`
— inalterado; a imagem é a mesma (`:local` mock / `:gpu` real — D8).

### D5 — Manager: resolução de `weights_id` aceita `engine IN ('yolo','world')`; `jobs.model` = variante | "world" | "mock"; `vram_min_gb: null` + guarda anti-mock (padrão predict)

**Decidido — 2 mudanças pontuais no manager:**
1. **Resolução de `weights_id` no `create_job`** (ADR-0012 D5): o check
   `engine != 'yolo'` → 400 passa a aceitar **`engine IN ('yolo','world')`**
   (a row `world` é o peso legítimo do autotracker; `yolo` continua o único do
   fine-tune). Fora disso → 400 `invalid_request` (defesa mantida);
   inexistente → 404 `not_found` (inalterado). `params.weights_ref{s3_key,md5}`
   gravado (inalterado).
2. **`jobs.model`** (display honesto no `/jobs` — extensão da regra do predict,
   ADR-0013 J.2): quando o job é autotracker **com `weights_id` presente** e a
   row tem `model` (variante) NOT NULL → `jobs.model = variante`; sem variante
   (upload/download de world → `model` NULL) → **`jobs.model = "world"`**.
   Sem `weights_id` → `"mock"` (inalterado). O SELECT de resolução já traz a
   variante (J.2).

**Decidido — VRAM: `vram_min_gb: null` no real, mesma política do predict.**
O submit real manda `vram_min_gb: null` (inalterado no body do manager); a
vram-table não ganha entrada `(autotracker, world, autotrack)` →
`resolve_required_gb` = None ⇒ roteamento **permissivo**. A garantia de GPU não
vem da policy (que é dívida — ADR-0011 D3) e sim da **guarda anti-mock** (nó
com GPU recusa imagem `:local` — L879-886) + sessão GPU com imagem `:gpu` +
`ENGINE_MOCK=0` (D8). *Por quê:* o world-x inferência usa ~2-4 GB (cabe na 3060
12 GB); fixar um valor arbitrário na vram-table criaria expectativa de policy
que não existe (o roteamento é estático/permissivo hoje) — mesmo desenho do
predict (ADR-0013 D5). *Gotcha:* um job real roteado para nó mock roda mock
(`ENGINE_MOCK=1`) **mesmo com modelId** — comportamento documentado (R4 da
ADR-0013); o critério binário da sessão GPU é o que prova o real. *Descartado:*
valor fixo `vram_min_gb: 8` (policy não aplicada — valor morto que mentiria);
parametrizar no body (superfície de contrato sem uso).

### D6 — Principal: `modelId` no request + config com `weights_path` + mapeamento `NotFound→404`/`InvalidRequest→400`; apply INTOCADO; spec 0.13.0

**Decidido — delta no `submit_autotracker_job` (espelho do `submit_predict_job`):**
1. **`AutotrackerJobRequest`** ganha `model_id: Option<String>` (wire
   `modelId`, `deny_unknown_fields` preservado). **Validação**
   (`validate_autotrack_request`): `modelId` presente e não-UUID → 400
   `invalid_request`; `model` ∈ {mock} e `conf` 0..1 (inalterados).
2. **Dataset elegibilidade INALTERADA**: `category='yolo'` + **classes ≥ 1** +
   imagens ≥ 1 — o real **requer** classes ≥ 1 (são os prompts do
   `set_classes`), então a condição do modal atual é exatamente a do real
   (divergência consciente do predict, que aceita 0 classes — ADR-0013 D8).
3. **`generate_autotrack_config_yaml`**: com `modelId` → adiciona
   `weights_path: "{weights_path}"` (placeholder literal) e emite
   `autotrack.model: "world"` (o `model` do body permanece `"mock"` como motor —
   a derivação é interna, documentada); sem `modelId` → config atual byte a
   byte (regressão).
4. **Manager body**: `weights_id = modelId` quando presente (+ `params`
   ganham `weights_ref` por resolução do manager — inalterado); `vram_min_gb:
   null`.
5. **Mapeamento do manager — padrão predict (R6 da ADR-0013), NÃO o
   `Err(_)→503` do submit_yolo_job**: `NotFound → 404` (modelId inexistente),
   `InvalidRequest → 400` (row com engine≠world — defesa da D5), `Unavailable →
   503`; **compensação do package em TODOS os erros** (padrão atual já faz —
   L793-816).
6. **Apply INTOCADO**: `apply_autotracker_boxes` (L1025-1360) não muda nenhuma
   linha — o `boxes.json` real tem o mesmo shape (D3) e o merge por origem já
   resolve por nome. A prova do contrato é o próprio smoke (D8).
7. **Upload/download**: aceitam `engine='world'` (validação idêntica — D1); o
   `Model` do wire não muda (engine é campo livre).

*Por quê — mapeamento rico:* o `submit_autotracker_job` atual mapeia
`Err(_)→503` (só distingue Unavailable) — com `modelId`, um modelo inexistente
viraria 503 em vez de 404; o padrão do abort/predict (L825-831) já resolve isso
e é o comportamento correto para referência a catálogo. *Gotcha:* o
`submit_yolo_job` existente NÃO é corrigido nesta fatia (R6 da ADR-0013 —
mudança de comportamento de rota existente = escopo separado, dívida).

### D7 — UI: `AutoTrackerModal` ganha dropdown "Modelo" (Mock + pesos `engine='world'`); sem outra mudança — via /impeccable, 1 modal = 1 review

**Decidido (fatia K.5; 1 modal = 1 fatia = 1 review, /impeccable com
`docs/DESIGN.md`):**
- **`AutoTrackerModal`** (L123-131): o select `model` fixo `"mock"` disabled
  vira um dropdown habilitado "Modelo":
  - opção **"Mock (determinístico)"** (value vazio — comportamento atual: sem
    `modelId`);
  - opções **"<name> · <origem>"** por modelo `engine==='world'` (via
    `listModels()` filtrado client-side — mesmo padrão do dropdown de pesos do
    `/treino`, ADR-0012 D7); selecionado → envia `modelId`;
  - empty state honesto se não houver modelo world: só a opção Mock + nota
    "Importe pesos world em Modelos & Pesos" (link opcional — polish, decisão
    do /impeccable).
- **`lib/autotracker.ts`**: `startAutotrackerJob` ganha `modelId?: string`
  (omitido → mock); erros pt-BR por `code` — 404 (modelId inexistente) e 400
  (engine≠world) entram no `autotrackerErrorMessage` (`types/studio.ts`).
- **Conf/overwrite/CTA intocados** (o overwrite continua decisão única no card
  de apply de `/jobs` — fecho da ADR-0008). O fluxo pós-submit (202 → `/jobs` →
  done → "Aplicar boxes" → editor com origin `autotracker`) é o MESMO.

*Por quê — dropdown no modal em vez de página nova:* o AutoTracker já tem
home e fluxo (modal na galeria → /jobs → apply — ADR-0008 D4); o real é a
mesma jornada com uma escolha a mais. O workspace AutoTracker do frontend.md
§7.1 permanece fatia futura. *Gotcha:* o badge do job no `/jobs` mostra
`model` (`mock`/variante/`world`) — sem badge especial "real" (o `source` do
modelo já conta a história; polish fica para o /impeccable se julgar
necessário).

### D8 — Verificação: baterias por commit + smoke mock E2E (regressão + pipeline com modelId) + sessão GPU com detecções reais (critério binário)

**Decidido — padrão da casa (unit + contract + test-db + smoke E2E + GPU
manual):**
- **unit**: engine (mock inalterado; real monkeypatchado — `YOLO(path)`
  chamado, **`set_classes` chamado com as classes do dataset**, `predict`
  chamado com `conf/imgsz=640/device=0`, `xywhn`→top-left, `class` de
  `model.names`, shape do `boxes.json` com `seed: 0` e SEM metrics.jsonl);
  manager (resolução aceita 'world', `jobs.model` variante/"world"/"mock");
  orquestrador (K.4 — ver D4); principal (modelId UUID, config com
  `weights_path`, mapeamento 404/400/503 + compensação, upload engine='world',
  contract 0.13.0 ≡ router).
- **test-db**: submit com modelId → dispatch com `weights_ref` → done;
  modelId inexistente → 404; **apply idêntico (regressão do merge por origem)**.
- **smoke mock E2E** (stack `ENGINE_MOCK=1`, `EXEC_MODE=docker`, Chrome):
  **(1) regressão**: submit SEM modelId → done → apply → boxes mock
  `origin='autotracker'` (fluxo da 0008 intacto); **(2) pipeline real em mock**:
  upload de `.pt` pequeno com `engine='world'` → submit COM modelId → done →
  `boxes.json` no bucket (mock ignora pesos — R4 ADR-0013) → apply → badge
  `autoTracked`. Critério: `weights_ref` → staging → `{weights_path}` →
  `boxes.json` → apply, sem regressão do fluxo antigo.
- **Sessão GPU (manual, fora do CI — padrão ADR-0010 G.6/ADR-0013 J.8)**:
  no TrueNAS com a imagem `:gpu` — obter o `yolov8x-worldv2.pt` (download por
  URL com a allow-list configurada para os hosts do release do ultralytics
  [github.com + objects.githubusercontent.com — redirects re-validados a cada
  hop, ADR-0012 D4] OU upload via `/models`) → `ENGINE_MOCK=0` → autotrack
  real com classes reais do dataset → **`boxes.json` REAL** → apply → **boxes
  `origin='autotracker'` reais no editor**. **Critério binário: detecções
  reais ≠ mock determinístico** (coordenadas/classes distintas das geradas por
  seed; idealmente detecção coerente — ex.: classe detectada na imagem onde a
  box real foi anotada; usar imagens plausíveis — lição J.8: modelo real pode
  retornar 0 detecções em dados sintéticos, o que é resultado honesto, não
  bug). Guarda anti-mock: imagem `:local` recusada com GPU. Teardown completo
  (repo na main, volumes gpu_* removidos, banco limpo, manager re-adota local).

## Migration — `0008_world_models.sql` (dono: manager; arquivo em `services/api-principal/migrations/`)

```sql
-- 0008_world_models.sql — AutoTracker real (ADR-0014 D1): engine 'world' no catálogo.
-- Dono: manager. O mundo world (yolov8x-worldv2.pt) é um peso do ecossistema
-- ultralytics; a row nasce com engine='world' para o fine-tune yolo continuar
-- recusando-o (400 na resolução de weights_id — ADR-0012 D5) e o autotracker
-- real aceitá-lo (ADR-0014 D5). Sem trigger e sem índice novo: o filtro por
-- engine usa `models_engine_idx` existente (0007).

ALTER TABLE models DROP CONSTRAINT models_engine_check;
ALTER TABLE models ADD CONSTRAINT models_engine_check
    CHECK (engine IN ('yolo', 'world'));
```

Invariantes: a constraint é recriada com o **mesmo nome**
(`models_engine_check` — Postgres gera `<tabela>_<coluna>_check` para CHECK
inline), então nenhum código referencia o nome antigo; CHECK simples
não-deferrável (sem invariante multi-tabela); `models_engine_idx` (0007) já
cobre `WHERE engine='world'` para a lista da UI. **Nenhuma coluna nova, nenhum
backfill** (não há rows world pré-existentes). O resto do schema (jobs TEXT
livres, `boxes.origin` inalterado — o `'playground'` NÃO nasce, dívida ADR-0013
D4) permanece intocado.

## Delta OpenAPI (0.13.0) — descrição na ADR

Rota alterada (entra em `PROTECTED_ROUTES` com status exatos — **nenhuma rota
nova**):
```
POST /api/jobs/autotracker   202 400 401 404 409 503   (+ modelId?: uuid)
```
- Body `AutotrackerJobRequest` (wire camelCase, `deny_unknown_fields`):
  `{datasetId, model?='mock', conf?=0.65, modelId?: string|null}` → 202
  `SubmitJobResponse`. Erros: 400 `invalid_request` (model∉{mock}, conf fora
  de 0..1, **modelId não-UUID**, **row de models com engine≠world — via
  manager**); 404 `not_found` (datasetId não-UUID/inexistente, **modelId
  inexistente — via manager**); 409 `dataset_not_ready` (category≠yolo, 0
  classes, 0 imagens — **inalterada**); 503 `queue_unavailable` (manager fora —
  compensação do package).
- **Erros novos: NENHUM.** Reuso: `invalid_request`, `not_found`,
  `dataset_not_ready`, `queue_unavailable` (enum de 19 códigos intocada).
- **`POST /api/models/upload`** e **`POST /api/models/download`**: `engine`
  aceita `'world'` (validação idêntica — `.pt` + magic `PK\x03\x04`; 400 nos
  demais). Nenhuma mudança de schema.
- Job real: `kind='autotracker'`, `engine='autotracker'`, `mode='autotrack'`,
  `model=<variante|"world">`, `params{model:"world", conf, package_ref,
  weights_ref}`, `vram_min_gb: null`. Artefatos: `boxes.json` (`kind='boxes'`,
  shape idêntico; `seed: 0` no real) — SEM `metrics.jsonl` no real.
- Rotas internas (resolução aceitando 'world', `jobs.model` variante/"world")
  **não** entram na OpenAPI (padrão dos paths internos).

## Testes

| Camada | Infra | Prova |
|---|---|---|
| `pytest engines/trainer-yolo` | unit (ultralytics monkeypatchado) | `cmd_autotrack` com `ENGINE_MOCK=1` → mock (regressão, sem ler peso); com `ENGINE_MOCK=0` → `_real_autotrack`: `YOLO(weights_path)` chamado com o caminho, **`set_classes(class_names)` chamado com as classes do `_read_dataset`**, `predict(source, conf, imgsz=640, device=0)`, `xywhn`→top-left clamp, `class` de `model.names` (após set_classes), shape `boxes.json` (`seed: 0`, sem `metrics.jsonl`), imagem sem detecção → `boxes: []`, `weights_path` ausente no real → die honesto; **37+ testes existentes verdes (mock train/autotrack intocados)** |
| `cargo test -p manager` (+ test-db) | Postgres do compose | resolução de `weights_id` aceita row `engine='world'` (202/ok) e `'yolo'` (inalterado); engine≠ → 400; inexistente → 404; `jobs.model`: variante NOT NULL → variante, world sem variante → `"world"`, sem weights_id → `"mock"`; fine-tune yolo com row world → 400 (defesa D5/D1); dispatch_body com `weights_ref` para autotracker |
| `cargo test -p orchestrator` | unit (sem S3) | **novos**: dispatch `(autotracker, autotrack)` com `weights_ref` → staging em `outputs/<job_id>/weights/<name>`, `{weights_path}` substituído no config, `boxes.json` subido, **sem `metrics.jsonl` → skip (L1169) + done com `metrics: None`**; regressão: sem weights_ref (comportamento atual) inalterado; 75+ existentes verdes |
| `cargo test -p api-principal` | MockStorage + pool lazy | `validate_autotrack_request` com `modelId` (não-UUID → 400, deny_unknown); `generate_autotrack_config_yaml` com `weights_path`+`autotrack.model:"world"` (sem modelId → byte a byte); manager body com `weights_id`; **mapeamento `NotFound→404`/`InvalidRequest→400`/`Unavailable→503` + compensação do package**; upload com `engine='world'` (mesma validação .pt/magic); **contract: spec 0.13.0 ≡ router** |
| Smoke E2E (stack `ENGINE_MOCK=1`, Chrome) | compose completo | **(1) regressão 0008**: submit sem modelId → done → apply → boxes mock origin autotracker → badge; **(2) pipeline real em mock**: upload `.pt` pequeno `engine='world'` → submit com modelId → done → `boxes.json` (mock) → apply → badge `autoTracked`; console limpo. **Critério: `weights_ref`→staging→`{weights_path}`→`boxes.json`→apply sem regressão do fluxo antigo** |
| Sessão GPU (manual, checklist README-gpu) | TrueNAS + RTX 3060, imagem `:gpu` | `yolov8x-worldv2.pt` (download por URL com allow-list OU upload) → `ENGINE_MOCK=0` → autotrack real com classes reais → `boxes.json` REAL → apply → **boxes `origin='autotracker'` reais no editor**; **critério binário: detecções ≠ mock determinístico**; guarda anti-mock (`:local` recusado); teardown completo |

O CI cobre as baterias locais (pytest + cargo dos 3 + test-db + contract); o
smoke @gpu continua manual (padrão ADR-0010/0012/0013).

## Spike obrigatório? — **NÃO** (com nota sobre `set_classes`)

Premissas externas são comportamento padrão ou já provado: (1) `YOLO(caminho)` +
`predict(source, conf, imgsz, device)` + `model.names` são a API canônica do
ultralytics — a MESMA lib/versão (8.3.x) já roda na imagem `:gpu` (treino real
da fatia G, inferência real da fatia J); (2) `set_classes(lista)` é API
documentada do ultralytics para pesos world (presente desde 8.1.0, restringe a
saída aos prompts dados e reescreve `model.names`) — **é a única chamada nunca
exercitada neste repo**, mas é a mesma biblioteca do ponto (1) e o teste é a
sessão GPU (K.8, critério binário); (3) staging de pesos + `{weights_path}`
provados @gpu (ADR-0012 I.9); (4) allow-list + redirects re-validados provados
(ADR-0012 I.4a); (5) guarda anti-mock provada (ADR-0011 fase 2). *Inverteria o
desenho (aí sim vira fix, não redesenho):* se `set_classes` não existir na
versão instalada da imagem `:gpu` (não provável — 8.3.x ≫ 8.1.0) → upgrade do
ultralytics no `Dockerfile.gpu` (fix de ambiente, mesmo escopo do commit
`9c8d41d` da fatia G) e re-teste na sessão GPU; se o peso world recusasse
carregar via `YOLO(path)` (não provável — é torch.save zip, mesmo magic do
upload) → o job falha honesto e o smoke GPU registra o erro exato.

## Riscos e contingências

- **R1 — `set_classes` × versão do ultralytics na imagem `:gpu`**: ver "Spike"
  — contingência: upgrade no Dockerfile.gpu (fix de ambiente).
- **R2 — Drift de classes package→apply** (reafirmação R1 da ADR-0008): o
  `set_classes` congela os prompts do package; o apply resolve por nome no
  dataset ATUAL; classe renomeada/deletada pós-job → box skippada (contagem
  honesta). Sem mudança — o real não piora nem melhora o residual.
- **R3 — Peso world ~1.3 GB**: (a) download por URL exige `MODEL_DOWNLOAD_
  ALLOWED_HOSTS` com `github.com` + `objects.githubusercontent.com` (redirect do
  release — revalidado a cada hop, ADR-0012 D4) — config de ambiente do
  usuário documentada no README-gpu; (b) upload 2 GiB cobre; (c) **job mock com
  modelId baixa bytes reais** do bucket (staging) — desperdício local aceito
  (R8 da ADR-0012; cenário do smoke).
- **R4 — VRAM do world-x (~2-4 GB) na 3060 12 GB**: cabe; roteamento
  permissivo (`vram_min_gb: null` — D5); OOM real → job `failed` honesto
  (padrão da falha honesta provada na fatia G). A 3060 é intermitente
  (llama.cpp/graft deep do usuário) — pre-flight de VRAM na sessão GPU (emenda
  E1 da ADR-0010).
- **R5 — Zero detecções em dataset sintético** (lição J.8): o modelo real pode
  retornar 0 boxes em imagens sintéticas — resultado honesto, não bug; o smoke
  GPU usa imagens plausíveis para o teste e o critério binário é "≠ mock", não
  "≥1 detecção".
- **R6 — Progresso binário no real** (sem `metrics.jsonl`): barra 0→100 no
  done; `ConvergenceChart` tolera ausência (base travada) — honesto, padrão
  predict (ADR-0013 D6).
- **R7 — Mapeamento do manager no principal**: `NotFound→404`/`InvalidRequest→
  400` (padrão predict); o `submit_autotracker_job` atual mapeia `Err(_)→503` —
  o handler do K **não repete** esse padrão (diverge conscientemente; o
  `submit_yolo_job` fica como está — dívida R6 da ADR-0013).
- **R8 — Intercalação manager↔orquestrador**: a K não muda contrato interno
  (`weights_ref`/`mode` existem desde I/J); landing sequenciado K.2 → K.3 no
  smoke; `#[serde(default)]` já protege.
- **R9 — test-db apaga `orchestrators`** (regra conhecida): restart do manager
  após qualquer test-db (re-adota no boot); lembrar no plano.
- **R10 — Mock com modelId roda mock mesmo com pesos reais** (R4 ADR-0013):
  documentado na sync e na UI via badge do job (`model`/estado); a distinção
  real só existe com `ENGINE_MOCK=0` @gpu — coerente com todo o produto.

## O que fica falso nos docs (lista para o `@docs-sync`, commit K.9)

**Não aplicar agora** — docs descrevem o que existe, não o que foi aprovado.

- `backend.md` §9/:198 — `POST /api/jobs/autotracker`: body ganha
  `modelId?: uuid`; validação (modelId não-UUID 400; modelId inexistente 404;
  row engine≠world 400 — mapeamento rico NotFound/InvalidRequest); config real
  com `weights_path`; `jobs.model` variante/"world"; `vram_min_gb: null` mesmo
  no real. A linha "model ∈ {mock} apenas" permanece (o real é por modelId).
- `backend.md` §9/:199 — `POST /api/jobs/:id/autotracker/apply`: **sem
  mudança** (reafirmar: shape preservado, merge por origem idêntico).
- `backend.md` §4/:63 — emenda ADR-0008 D2/D6: o autotracker real NÃO ganhou
  runner/imagem próprios (florence-2/qwen-vl); usa a MESMA imagem trainer-yolo
  com o modelo open-set **yolov8x-world do ultralytics** (`set_classes`);
  florence-2/qwen continuam alternativas futuras.
- `backend.md` §9/:141-142 e §10/:298-302 — tabela `models`: CHECK
  `engine IN ('yolo','world')` (migration 0008); upload/download aceitam
  `engine='world'` (validação `.pt`+magic PK idêntica).
- `backend.md` §10 — tabela `boxes`: `origin` continua `manual|autotracker|
  import` (o `'playground'` NÃO entra — dívida ADR-0013 D4); `track_id` não é
  preenchido pelo real.
- `frontend.md` §10/:242 — `AutoTrackerModal`: modelo deixou de ser fixo
  `mock` — dropdown "Modelo" (Mock determinístico + pesos `engine='world'` via
  `listModels()`); `lib/autotracker.ts:startAutotrackerJob` ganha `modelId?`;
  erros 404/400 no `autotrackerErrorMessage`.
- `frontend.md` §7.1/:169 — workspace AutoTracker: continua fatia futura; o
  real é a mesma UX do v1 (modal → /jobs → apply), só com a escolha de modelo.
- `docs/dividas.md` :88 — dívida "AutoTracker real" → **QUITADA** (esta
  fatia); emenda: implementado com yolov8x-world na imagem trainer-yolo;
  florence-2/qwen-vl permanecem registrados como alternativas (imagem própria).
- `docs/dividas.md` :89 — "Apply de boxes do playground (`origin='playground'`)"
  → **NÃO quitada** (o K reusa o ingest `'autotracker'` existente; o origin
  playground continua dívida).
- `docs/adr/0008-autotracker-v1.md` D6 — nota "AutoTracker real ... modelo
  local (florence-2/qwen-vl) + upload de modelo + imagem runner-autotracker
  própria" → **emenda**: upload de modelo veio na Fatia I, a plumbagem na J, o
  real na K com yolov8x-world (set_classes, mesma imagem); florence-2/qwen e
  runner próprio continuam futuros.
- `docs/adr/0013-playground-inferencia.md` D4 — "A Fatia K (AutoTracker real)
  trará o fluxo de ingest com a semântica certa (merge por origem +
  consentimento explícito)" → **CUMPRIDA** (o ingest já existia da 0008 e foi
  reusado sem mudança — o K provou o contrato, não o criou).
- `coordenacao.md` — bloco da fatia K reescrito a cada commit.

## Plano de commits (K.0–K.9; branch `feat/autotracker-real` de `main`)

Um dispatch = um commit; produção < 400 linhas por commit (contrato/testes fora
da conta — exceção da casa). **Fase 1: K.1 ∥ K.2 ∥ K.3 ∥ K.4** (ownership
disjunto: engine vs manager vs principal+contracts vs orchestrator-testes — o
K.4 não depende de produção nova, monta o dispatch manualmente); **Fase 2:
rebuild+recreate (regra F4.7) → K.5** (frontend via /impeccable); **Fase 3:
K.6 review → K.7 smoke E2E → K.8 sessão GPU → K.9 docs-sync**. Migration 0008
entra no K.2 (dono manager, mesmo arquivo de migrations do repo).

| # | Dono | Conteúdo | Critério de pronto |
|---|---|---|---|
| **K.0** | @architect | Esta ADR (proposta; vira executável após aceite do usuário) | auditoria do coordenador; arquivo commitado em `main` |
| **K.1** | @python-engines | `autotrack.py`: `cmd_autotrack` com check `ENGINE_MOCK` (padrão predict) + `_real_autotrack` (lazy `YOLO(weights_path)` + `model.set_classes(class_names)` + `predict(source, conf, imgsz=640, device=0)` + reuso de `_xywhn_to_topleft_clamped`/`_read_dataset` + `boxes.json` com `seed: 0` e SEM `metrics.jsonl`) + testes (real monkeypatchado: set_classes chamado, shape, seed 0, sem metrics, die sem weights_path no real; mock intocado) | `pytest` verde (novos + 37+ existentes); `python -m trainer_yolo autotrack --config … --output …` com `ENGINE_MOCK=1` produz EXATAMENTE o boxes.json atual; fmt; ~200-300 linhas |
| **K.2** | @rust-dev (manager) | migration `0008_world_models.sql` (CHECK `('yolo','world')`) + `create_job`: resolução de `weights_id` aceita `engine IN ('yolo','world')`; `jobs.model` = variante (NOT NULL) | `"world"` (sem variante) quando autotracker com weights_id; `"mock"` inalterado sem weights_id; testes (world ok, yolo inalterado, engine≠ 400, jobs.model 3 casos, fine-tune com row world → 400) | `cargo test -p manager -- --ignored` + test-db verdes (**restart do manager depois** — R9); fmt; ~80-140 linhas |
| **K.3** | @rust-dev (principal) | `AutotrackerJobRequest.modelId` + `validate_autotrack_request` (UUID) + `generate_autotrack_config_yaml` (com modelId: `weights_path` + `autotrack.model:"world"`; sem: byte a byte) + submit com `weights_id` + **mapeamento `NotFound→404`/`InvalidRequest→400`/`Unavailable→503` + compensação** + upload aceita `engine='world'` + spec **0.13.0** + contract + `PROTECTED_ROUTES` | `cargo test -p api-principal` verde + contract 0.13.0 ≡ router; fmt; ~200-300 linhas |
| **K.4** | @rust-dev (orchestrator) | testes unit novos: `(autotracker, autotrack)` com `weights_ref` → staging + `{weights_path}` substituído + `boxes.json` subido + **sem `metrics.jsonl` → skip (L1169) + done `metrics: None`**; regressão sem weights_ref inalterada. **Zero produção** | `cargo test -p orchestrator` verde; fmt; ~60-100 linhas (só teste) |
| **K.5** | @frontend-dev (via /impeccable) | `AutoTrackerModal`: dropdown "Modelo" (Mock determinístico + `listModels()` filtrado `engine==='world'` rótulo `name · origem`; empty state honesto) + `lib/autotracker.ts` (`modelId?`) + `types/studio.ts` (erros 404/400 pt-BR) | `npm run build --workspace=web` verde; review por página (1 modal = 1 fatia = 1 review); DESIGN.md como contrato |
| **K.6** | @reviewer | review do diff K.1–K.5 vs esta ADR (2 partes: rust principal+manager+orchestrator / engine+web; pontos: set_classes 1:1, seed 0 sentinela, mapeamento 404/400, CHECK 'world' + defesa do fine-tune, mock intocado, dropdown honesto) | APROVA (com ou sem nits); fixes roteados como commits próprios |
| **K.7** | @coordenador (smoke E2E, fora do CI) | stack mock: **(1) regressão 0008** (sem modelId → done → apply → badge); **(2) pipeline com modelId** (upload `.pt` pequeno engine='world' → submit com modelId → done → `boxes.json` → apply → badge) | critérios binários da tabela de Testes; teardown limpo; restart do manager pós test-db (R9) |
| **K.8** | @coordenador (sessão GPU, manual) | TrueNAS imagem `:gpu`: world (download allow-list OU upload) → `ENGINE_MOCK=0` → autotrack real com classes reais → `boxes.json` REAL → apply → **boxes reais no editor**; **critério binário: detecções ≠ mock**; guarda anti-mock (`:local` recusado); teardown completo (repo na main, volumes gpu_* removidos, banco limpo, manager re-adota local) | checklist README-gpu; pre-flight VRAM 3060 (E1 ADR-0010) |
| **K.9** | @docs-sync | Aplica "O que fica falso nos docs" (backend.md §4/§9/§10, frontend.md §7.1/§10, emendas ADR-0008 D6 e ADR-0013 D4, dividas.md :88/:89, coordenacao.md) | diff só de docs; conferência doc↔código nos dois sentidos |

**Notas de processo:** mock NUNCA quebra (baterias existentes = critério:
pytest e cargo verdes a cada commit); `cargo fmt --all` antes de reportar;
contract test exige spec ≡ router a cada commit (delta OpenAPI incremental —
K.3 declara o modelId e o schema no mesmo commit); despachos de fix nunca
editam fora do escopo (reportam ao coordenador); a sessão GPU usa o checklist
`infra/README-gpu.md` (imagem `:gpu` JÁ construída — não rebuildar sem
necessidade; se o `set_classes` falhar por versão, upgrade do ultralytics no
Dockerfile.gpu é fix de ambiente roteado ao @infra-dev, R1).

## Perguntas ao usuário — ABERTAS (aguardam aceite; a ADR só vira executável após resposta)

- **P1 — Modelo (D1):** o world entra como **`engine='world'`** na tabela
  `models` (migration 0008 estende o CHECK; upload/download reusados com
  validação `.pt`+magic PK idêntica; fine-tune yolo continua recusando row
  world — defesa mantida). **Recomendação:** sim. Alternativa: `engine='yolo'`
  poluído (rejeitada — identidade do mecanismo misturada, fine-tune aceitaria
  peso world).
- **P2 — Transporte (D2):** `modelId?: uuid` opcional no body existente;
  **kind/engine/mode constantes** (`autotracker`/`autotracker`/`autotrack`); o
  engine decide por `weights_path` no config. **Recomendação:** sim — zero
  delta de contrato interno; matriz da D6 (ADR-0013) intocada. Alternativa:
  mode/kind novos (rejeitada — linha nova na matriz sem ganho).
- **P3 — Engine (D3):** real = `YOLO(path)` + **`set_classes(classes do
  dataset)`** (prompts = nomes das classes, mapeamento 1:1) + `predict` →
  `boxes.json` no MESMO shape (seed `0` sentinela) e **sem `metrics.jsonl`**
  (progresso binário; orquestrador já skipa ausente — L1169). **Recomendação:**
  sim. Alternativa: pós-filtro por nome sobre classes COCO (rejeitada —
  heurística frágil).
- **P4 — VRAM (D5):** `vram_min_gb: null` no real + **guarda anti-mock**
  garante GPU (padrão predict); entrada na vram-table fica dívida com a policy
  VRAM real. **Recomendação:** sim. Alternativa: valor fixo 8 GB (rejeitada —
  policy não aplicada hoje, valor morto).
- **P5 — Escopo (D0):** florence-2/qwen-vl ficam FORA (alternativas futuras,
  imagem própria); mock intocado (critério de não-quebra); apply INTOCADO
  (shape preservado = prova do contrato). **Recomendação:** sim.

**Regra de aceitação:** a ADR-0014 só vira plano executável após o aceite
explícito do usuário (P1–P5); até lá tudo é proposta.
