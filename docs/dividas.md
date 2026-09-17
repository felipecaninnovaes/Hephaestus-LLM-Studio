# Dívidas técnicas e pendências — registro da casa

Registro **permanente** de dívidas técnicas e pendências que as próximas fatias
precisam honrar. Inspirado no `tech-debt-tracker.md` do artigo "Harness
Engineering" (OpenAI, 2026-02-11): dívida como registro de primeira classe do
repo, legível por qualquer agente sem contexto externo — paga continuamente em
pequenas parcelas, não em rajadas.

Separado de propósito: o estado de sessão/plano vive em `tasks/todo.md`
(volátil, reescrito por sessão); este arquivo é sistema de registro (permanente,
versionado). O `coordenacao.md` referencia este arquivo e não o duplica.

**Como atualizar**: ao REGISTRAR (revisor, spike, ADR, lição de ambiente), ao
MARCAR a fatia que honra, e ao QUITAR (mover para "Quitadas" com o commit que
fechou). Enquanto em aberto, uma dívida NÃO pode ser violada por uma fatia nova
— honrar no nascedouro.

## Em aberto

### Com fatia marcada

- **T4 — `jobs.dataset_id` + `dataset_versions` (fatia 4 — jobs/package/
  materialização)**: `jobs.dataset_id UUID NULL REFERENCES datasets(id) ON
  DELETE SET NULL` + snapshot `dataset_versions` (nunca `RESTRICT`) —
  ADR-0002 T4. Nessa mesma fatia o orquestrador ganha cliente S3 com
  credencial escopada por prefixo. (O `POST /:id/package` também é da
  fatia 4 — revisão D0 da ADR-0006 sobre o ADR-0003 D9.)
- **M2 — lacunas de teste do export/import (re-auditoria do reviewer da 3e,
  sem fatia marcada)**: (a) nenhum teste abre o zip gerado PELO HANDLER (o
  unit cobre `build_label_entries` puro; o loop real com `a.jpg`+`a.png` +
  zip final sem teste); (b) skip de órfão sem teste-db; (c) fixture de
  dedupe pós-sniff e teto de reader contado em zip real, sem fixture.
- **Borda composta em `label_arcname_for` (re-auditoria do reviewer da 3e,
  sem fatia marcada)** — `src/datasets/export.rs:292`: `used.insert` ignora
  o retorno — o caso de 3 vias (`a.jpg`+`a_png.jpg`+`a.png` na ordem criada)
  ainda produz arcname duplicado; correção = loop de sufixo + teste 3-vias.
  Raríssimo; roundtrip interno não afetado (import ignora labels).
- **SELECTs pós-commit do import sem cleanup (re-auditoria do reviewer da
  3e, sem fatia marcada)** — `src/datasets/import.rs:637-645` (mapa
  class_idx→id) e `:761-784` (SELECT final do dataset + classes): falha de
  leitura aí deixa o dataset novo commitado (vazio/completo) com 500;
  all-or-nothing estrito chamaria `cleanup_failed_import` nesses retornos
  também.
- **NIT a11y do ImportDatasetModal (re-auditoria do reviewer da 3e, sem
  fatia marcada)**: o foco não migra para o botão "Substituir" na fase
  `confirm` (padrão do ConfirmDialog — consistente, registrável).
- **NIT RAM do import (re-auditoria do reviewer da 3e, sem fatia
  marcada)**: `extract_images` lê a imagem inteira em memória para
  sniff/decode (teto 200 MiB/imagem limita; a 3b spoola sem acumular).
- **Revisão de caption no AutoLabel (fatia futura de curadoria/human-in-the-loop)**:
  Permitir inspeção, edição e aprovação individual ou em lote das legendas geradas
  pelos modelos VLM antes de aplicar no dataset. Hoje o apply (`POST /api/jobs/:id/autolabel/apply`)
  aplica 100% das legendas do `captions.jsonl` de uma vez só sem visualização prévia
  nem edição das legendas geradas.

### Sem fatia marcada

**Backend**

- **Abort races (review F4.8):** abort durante `preparing` pode ser engolido (estado substituído pelo progress report do orquestrador; `is_cancelled` não é consultado no pipeline — dead code); abort em `dispatched` não notifica o orquestrador (o dispatch já foi feito mas o orquestrador pode estar starting); abort em voo termina `failed` (nunca `cancelled` — o orquestrador reporta `failed` com erro "job not found or already finished" quando o container é stopado). Janela de segundos em mock local; conserto exige testes de abort-em-voo. (relacionado ADR-0024/cc17527: reports terminais done/failed já carregam e persistem phase/message via COALESCE; o ramo `cancelled` de report continua inexistente — ao consertar as races, adicionar arm cancelled em `report_job` + emissão no orquestrador).
- **Diffusion sem fingerprint de reuso (ADR-0025 P4a):** `build_package_diffusion`
  (`services/api-principal/src/datasets/package.rs:583`) não recebe nem grava
  `fingerprint` no manifest — `try_reuse_package` só acerta p/ `build_package_filtered`;
  job `diffusion_train` SEMPRE reconstrói o pacote (só o dedupe local de submits
  simultâneos via `job_prepares` o protege). Fechar = assinar `build_package_diffusion`
  com `fingerprint: Option<&str>` + gravar no manifest (fusão P4a+P4b).
- **Watchdog `preparing` ancorado em `created_at` (ADR-0025 D3):** o timeout de 60min
  (`watchdog_prepare_timeout`, `services/manager/src/lib.rs:1261`) conta desde a
  criação — reports de progresso (`prepare-complete` renova, mas reports
  intermediários de `packaging_*` NÃO) não estendem o prazo. Build legítimo >60min
  morre `prepare_timeout`. Fechar = ancorar em `updated_at` (heartbeat do worker)
  ou renovar `created_at` a cada report de progresso.
- **Pânico pós-upload deixa versão órfã reusável (ADR-0025 P4a):** se o worker
  panica DEPOIS do PUT do zip mas ANTES do `prepare_complete`, a `dataset_version`
  fica sem referência e `try_reuse_package` a reutiliza em submits futuros (bytes
  íntegros — só falta o vínculo com o job original). Defesa atual: GC de
  `dataset_versions` pula datasets com job não-terminal (`params.prepare.datasetId`).
  Fechar = marcar versão como `complete` no `prepare_complete` e só reusar marcadas.
- **Progresso do build por marcos, sem % real de download (ADR-0025 P4b):**
  `run_prepare` reporta 0.02/0.1/0.6(reuso)/0.9 (`services/api-principal/src/jobs/prepare.rs`);
  dentro de `materialize_images_from_storage` há `i/N` aproximado só na mensagem de
  erro — a barra salta por marcos, não acompanha bytes/imagens reais. Fechar = reportar
  progresso fracionário a partir do stream `buffer_unordered` (p/ ex. a cada N arquivos).
- ~~**Watchdog de orquestrador (review F4.8)**~~ **QUITADA 2026-09-10** (Fatia H, ADR-0011 D4: watchdog no worker loop existente, 15s → `degraded`, 60s → `offline` + re-queue dos jobs do nó morto via CTE).
- **CI: pytest do trainer-yolo (review F4.8):** contrato das 6 keys do `metrics.jsonl` (`epoch, box_loss, cls_loss, dfl_loss, mAP50, mAP50-95`) com o parser do orquestrador (`parse_metrics_line`) não roda em CI — job Python ausente em `.gitea/workflows/ci.yml`. Validado manualmente no E2E.
- **Mock: best.pt e last.pt idênticos (review F4.8):** artefatos deterministas (bytes de cabeçalho, mesmos 32 bytes) — diferenciar por mAP50 se a UI consumir histograma ou comparação entre best/last.
- **Reports de métrica best-effort (review F4.8):** `let _ =` sem retry no orquestrador (`lib.rs` — report de progresso/métricas); terminal de erro agora tem log (`tracing::warn`), mas falha do manager no momento da falha prende o job até recovery no boot.
- **Defaults de token inconsistentes (review F4.8):** manager aceita `manager-dev-token` como default (`MANAGER_TOKEN` env); orquestrador bypassa auth sem `MANAGER_TOKEN` (Bearer vazio aceito na v1 local); principal fail-fast sem `MANAGER_TOKEN`. Sem fail-fast uniforme entre os 3.
- **`unzip_safe` não rejeita separador `\` (review F4.8):** divergência com `validate_artifact_path` do principal (que aceita `\`); inócuo em Linux mas viola defesa em profundidade.
- **NITs frontend (review F4.8):**
  - ~~Highlight de módulo ativo da Sidebar hardcoded em "Dados & Anotação" (sem `usePathname` — o módulo ativo NÃO muda quando o usuário navega para `/jobs`).~~ **QUITADA 2026-09-08** (Fatia 5, commit `61492ea`: `usePathname` destaca módulo ativo via `pathname` no `Sidebar.tsx`).
  - Duplicação tripla de `canTrain`/`trainDisabledReason` (lógica de condição de treino repetida no botão da galeria, no `DatasetMenu` e na galeria de datasets).
  - Toast "Job cancelado." no 200 quando o estado real é `cancelling` (o handler retorna `{"status":"cancelling"}` mas a UI pode exibir mensagem genérica).
  - Telemetria pollada a cada 3s mesmo com drawer da Sidebar fechado (consumo de requests desnecessário quando o card não está visível).
- **SubprocessExecutor = stub honesto (review F4.8):** `EXEC_MODE=subprocess` implementado mas é stub (só `subprocess.run`, sem progress streaming). RunPod = fatia futura. Se já não estiver em dividas.md, registrado aqui.
- **GC automático da lixeira (TTL) — ABERTA 2026-09-06** (fatia futura, fora da
  3g por D9 da ADR-0005): hoje só purge manual via UI (`DELETE /:id/trash`);
  `images.deleted_at` já dá o dado (idade do soft delete); falta TTL +
  purge agendado (job/cron que DELETE físico + sweep das vencidas).
- **Logging server-side (Fatia 4 parcial — review F4.8):** `tracing` + `tracing-subscriber` com formatter JSON + `x-request-id` + `/ready` entraram nos 3 serviços Rust (D11 ADR-0007); **pendente:** buffer de logs 1000/2000 linhas, `?since_seq`, `WS` de logs (fatia de logs/WS).
- **CLI `studio reset-password`** (ADR-0001 T4).
- **Gate aceita `sub` órfão** (ADR-0002 T8): cookie assinado com segredo
  antigo sobrevive a reset de `users` e passa a ler/deletar datasets;
  mitigação = `SELECT EXISTS` no gate ou rotacionar segredo no reset.
- **Ordem estável de `boxes` entre saves (nota menor da 3d)** — o PUT boxes é
  `DELETE`+`INSERT` com `RETURNING`: os ids nascem novos a cada save e
  `GET detail` não garante ordem. A UI correlaciona seleção por índice de
  payload pós-save (correto), mas os chips `#N` no canvas podem reordenar
  entre loads. Correção real = coluna de ordenação/`ORDER BY` determinístico
  no backend — só valerá a pena quando o autotracker consertar caixas
  existentes, hoje os efeitos são cosméticos. **Reafirmação (Fatia 5):** o apply do autotracker não piora a ordem semanticamente porque o merge por origem é atômico por imagem — mas a dívida permanece aberta para quando o autotracker consertar caixas existentes.
- **R1 drift de classes snapshot→apply (curta, Fatia 5):** o snapshot (`SnapshotClass{index,name}`, sem id) congela classes do pacote; no apply, o principal resolve por `name` no dataset atual. Se uma classe foi renomeada/deletada entre job e apply → box skippada (contagem honesta). Aceito na v1 mock; alternativa futura: snapshot com id de classe + decisão de re-mapear.
- **AutoTracker de vídeo — ABERTA 2026-09-08 (ADR-0008 D5):** extração de frames + tracking por `track_id` (`boxes.track_id` já existe desde 0003) + rota de escrita de `videos` (tabela existe sem rota de escrita desde 3b). IDEIA.md §3/:42 pede "Video e Imagem"; a v1 é imagem apenas.
- **AutoTracker real — QUITADA 2026-09-11** (Fatia K, ADR-0014: yolov8x-worldv2.pt via `set_classes(classes do dataset)`, engine `world` na tabela models, migration 0008, `modelId?` no body de submit, mapeamento rico NotFound→404/InvalidRequest→400, boxes.json com seed:0 sentinela sem metrics.jsonl, AutoTrackerModal com dropdown "Modelo", spec 0.13.0; sessão GPU provada com detecções reais + fix openai-clip no Dockerfile.gpu). **Emenda:** implementado com yolov8x-world na mesma imagem trainer-yolo (regra uma-imagem-por-engine preservada); florence-2/qwen-vl permanecem registrados como alternativas futuras (imagem própria — não quitada).
- **Apply de boxes do playground (Fatia K — ADR-0013 D4):** o v1 do playground é read-only (overlay + download); aplicar detecções como anotações (`origin='playground'`) exigiria migration nova (CHECK de `boxes.origin` é `manual|autotracker|import` desde 0003), rota de apply nova, e decisão de merge. **NÃO quitada pela Fatia K** (o K reusa o ingest `'autotracker'` existente; o origin playground continua dívida).
- ~~**Telemetria por orquestrador (ADR-0009 R1)**~~ **QUITADA 2026-09-10** (Fatia H, ADR-0011 D1/D2/D4: heartbeat identificado com `endpoint`, cache por nó `HashMap<Uuid, TelemetryState>`, watchdog `degraded`/`offline` com re-queue, `GET /api/orchestrators` enriquecido com telemetria por nó).
- **Models real (ADR-0009 R2/D2) — QUITADA 2026-09-10** (Fatia I, ADR-0012). **Gestão de modelos & suporte a `.safetensors`/multi-engine QUITADA 2026-09-12** (Fatia Gestão de Modelos & Infra: `DELETE /api/models/:id` e `/internal/models/:id`, remoção física do S3 para upload/download, migration 0010 com engines `diffusion` e `clip`, validação de cabeçalho `.safetensors`, UI de exclusão com `ConfirmDialog`, spec OpenAPI 0.16.0). **Pendente remanescente:** WS de progresso de download; proxy `GET /api/models/:id/data` como fallback sem `S3_PUBLIC_ENDPOINT_URL`.
- **Reconciliação storage (ADR-0009 R3) — ABERTA 2026-09-09:** reconciliação bucket×banco / storage real via `ListObjectsV2`. Hoje: soma SQL (`datasets.size_bytes` + `SUM(job_artifacts.bytes)`), NÃO ListObjects (StoragePort não tem método de listagem; rejeitado em D3).
- ~~**POST /api/orchestrators/{adopt,rotate,revoke,enable,disable,remove} + GET /:id/health (ADR-0009 D0)**~~ **PARCIALMENTE QUITADA 2026-09-10** (Fatia H, ADR-0011 D5: adopt/revoke implementados + alias `/api/environments*` implementado). **Pendente:** rotate, enable/disable, GET /:id/health.
- ~~**Roteamento por capacidade no manager (GPU/VRAM/kind)**~~ **PARCIALMENTE QUITADA 2026-09-10** (Fatia H, ADR-0011 D3: roteamento estático via SQL determinístico com vram-table no manager, `waiting_vram` quando sem capacidade, ORDER BY determinístico). **Pendente:** policy VRAM completa (fila por VRAM livre dinâmica, paralelismo 2+ jobs por nó, preempção de runner, `max_parallel_trainers`).
- **Rotacionar credenciais S3 / exigir TLS fora da LAN (ADR-0010 D3/D10) — ABERTA 2026-09-09:** credenciais `heph-admin`/`heph-orchestrator` são LOCAL-DEV e agora alcançáveis da LAN; bind em IP específico + identidade escopada reduzem superfície (não pioramos — 8080/8081/8082 já são 0.0.0.0); rotacionar e exigir TLS se o ambiente sair da LAN caseira.
- ~~**`orchestrators.gpus`/`vram_total_gb` preenchidos por INSERT manual (ADR-0010 D1)**~~ **QUITADA 2026-09-10** (Fatia H, ADR-0011 D1: heartbeat identificado grava `gpus`/`vram_total_gb` dinamicamente — round(MiB/1024), dado real do host, não INSERT manual).
- **VRAM transportada em MiB (não MB) no wire — NOTA 2026-09-09 (ADR-0010):** `nvidia-smi` retorna MiB puros; a spec documentava MB; review G.5 decidiu transportar MiB puro (erro <5% irrelevante para gauge); dashboard divide por 1024 (fix web `c71bb0d`). Registro aqui para que a fatia de roteamento por capacidade não assuma MB.
- **Policy VRAM aplicada completa (ADR-0011 D3, dívida) — ABERTA 2026-09-10:** fila por VRAM livre dinâmica (paralelismo 2+ jobs por nó, preempção de runner, `max_parallel_trainers`). Roteamento estático (capacidade declarada vs requisito) já implementado (Fatia H); o "se livre >= min → sobe em paralelo" do §6/:91 continua dívida.
- **Credenciais por nó `heph_o_*` + rotação + TLS com pin (ADR-0011 D5, dívida) — ABERTA 2026-09-10:** colunas `token_hash`/`fingerprint` existem e ficam NULL; transporte continua com `MANAGER_TOKEN` compartilhado. Rate-limit de 5 tentativas (§8) também pendente.
- **Detecção de dupla execução / reconciliação de report pós-re-queue (ADR-0011 R3) — ABERTA 2026-09-10:** re-queue do watchdog assume nó morto; sem detecção de "ainda vivo mas isolado" (partição de rede). Artefatos last-write-wins por `job_id` (benigno na prática).
- **Rate-limit / TTL do pairing code (ADR-0011 R2) — ABERTA 2026-09-10:** single-use em memória, mas código via env não tem TTL e o flag reseta no restart do orquestrador. Aceito para LAN caseira; dívida para uso externo.
- **D1 — daemon difusão: integração @gpu real (ADR-0023 D1) — ABERTA 2026-09-15:** wire validado em mock (daemon HTTP sobe, responde health/generate/shutdown, sentinela de cancel funciona); sessão GPU pendente para validar: `from_single_file` quantizado (SDXL 4bit/8bit), multi-LoRA peft no Flux2Klein (`pipe.transformer.set_adapters`), e daemon quente real (pipeline carregado, 2 gerações sequenciais sem reload).
- **Prova runtime de checkpoint custom (ADR-0023 D4) — ABERTA 2026-09-15:** `from_single_file` + quantização para SDXL/SD15 requer safetensors real; spike S1 provou API suportada no diffusers 0.40.0 mas não rodou @gpu com pesos reais.
- **Filtro quantization da galeria quebra total (ADR-0023 D5) — ABERTA 2026-09-15:** `GET /api/generations` filtra quantization em memória no BFF (push-down no manager pendente); com muitas gerações, total retornado é honesto mas performance degrada.
- **Restore/lixeira de gerações + sweep S3 de soft-deletados (ADR-0023 D5) — ABERTA 2026-09-15:** `generations.deleted_at` dá o soft-delete mas não há restore nem sweep de objetos S3 (segue o padrão da lixeira de imagens — dívida pré-existente generalizada).
- **NIT: nome custom_model_path vs custom_model_id no meta do engine (ADR-0023 D4) — ABERTA 2026-09-15:** engine python usa `custom_checkpoint_path` no config.yaml mas o meta reportado usa `custom_model_id`; alinhar na próxima iteração.
- **Yaml legado inclui `batch_size: 1` (ADR-0023 D2) — ABERTA 2026-09-15:** quando modo legado (sem loras, sem custom), o config.yaml emite `batch_size: 1` mesmo para batch único — funcionalmente inócuo mas polui o yaml; limpar na iteração seguinte.
- **GC de `generation_inputs` inexistente (feat/img2img) — ABERTA 2026-09-17:** objetos S3 sob
  `generation_inputs/` e linhas com `used_at` preenchido não são varridos — mesmo padrão de
  artifacts sem sweep (linhas consumidas permanecem p/ auditoria por decisão consciente da
  migration 0017). Fechar = TTL + cron de purge (DELETE físico + sweep das vencidas), na mesma
  fatia que generalizar o sweep de artifacts/gerações soft-deletadas.
- **Validação @gpu manual do img2img real (feat/img2img) — ABERTA 2026-09-17 (follow-up,
  não bloqueia docs):** wire validado em mock (upload, XOR, placeholder, staging, variante por
  componentes, meta); sessão GPU pendente com pesos reais (sdxl + flux-2-klein, `ENGINE_MOCK=0`)
  para provar `StableDiffusionXLImg2ImgPipeline`/`StableDiffusionImg2ImgPipeline` via
  `**pipe.components` e `image=` nativo do `Flux2KleinPipeline` (diffusers 0.40.0) antes de
  anunciar pronto.
- **Cache de quantização de treino usa path per-job no slug (feat/pesos-custom-flux2, reviewer N1) — ABERTA 2026-09-17:** `models/flux.py` isola o cache por checkpoint+encoder mas o slug carrega path per-job — dois treinos do MESMO checkpoint requantizam; dedup por md5-only é decisão de custo pendente.
- **Validação @gpu manual de checkpoint/encoder custom flux-2 (feat/pesos-custom-flux2) — ABERTA 2026-09-17:** load real pendente (`Flux2Transformer2DModel.from_single_file` + encoder override Qwen3, `ENGINE_MOCK=0`); CPU-only cobre só validação/meta/cache.
- **Nomes internos `custom_checkpoint` vs `text_encoder_ref` no manager (feat/pesos-custom-flux2, reviewer N5, inócuo) — ABERTA 2026-09-17:** padronizar na próxima passada (ex. ambos `*_ref` ou ambos sem sufixo).

**Verificação / toolchain**

- ~~**test-db.sh apaga estado de produto quando a stack está de pé — ABERTA 2026-09-08**~~ **QUITADA 2026-09-12** (Fatia Gestão de Modelos & Infra: `scripts/test-db.sh` agora cria o banco efêmero isolado `studio_test`, roda todas as migrations, executa a suíte de testes de integração e descarta o banco no final via trap EXIT, sem nunca tocar no banco de produto `studio` ou invalidar a stack local).
- **Teste `@gpu` manual do CLIP real (fatia futura, sem número) — ABERTA 2026-09-06**
  (spike/ADR-0004): o modo real do embedder (`ENGINE_MOCK` desligado,
  `open_clip_torch` ViT-B-32, peso ~600 MB fora do compose) nunca rodou em GPU;
  manual, não bloqueia fatias.
- **Planner do pgvector prefere seq scan com ~10k linhas (sem fatia) — ABERTA
  2026-09-06** (spike): custo do LIMIT pequeno faz o planner ignorar o HNSW;
  NÃO forçar `enable_seqscan=off`; re-avaliar com datasets reais grandes.

- **`cargo fmt -p api-principal`** viola o padrão (hunks pré-existentes da
  Fatia 2 + acréscimo da 3b); nenhum CI de fmt — decisão de quando formatar é
  do usuário.
- **`e2e-smoke.sh`**: confirmar cobertura do bloco `--datasets`.

**Infra / ambiente**

- ~~**Fixar digests de imagem no compose/Dockerfiles**~~ **QUITADA 2026-09-05**
  (decisão do usuário; commit `4b8c7e4` em `chore/pin-digests` — aguardando merge;
  despacho `@rust-dev` com digests resolvidos no registry pelo coordenador; 11/11
  referências pinadas, `compose config -q` verde nos dois arquivos). Padrão
  `tag@sha256:` mantém a tag legível e o digest como trava.
  - **Parte `db` re-honrada na 3f (2026-09-06, `feat/semantic-search`):** a imagem
    do `db` trocou `postgres:16` → `pgvector/pgvector:pg16-trixie@sha256:c8483555…`
    (spike provou upgrade sem dump/restore; `pg16` sem sufixo é bookworm →
    collation mismatch). Só a parte postgres/db está paga aqui; o item como um
    todo segue quitado acima.
- **Nota (atualizada Fatia 4):** o **orquestrador** migrou para `trixie-slim`
  (`rust:1.97.1-slim` builder, `debian:trixie-slim` runtime — GLIBC 2.41 para
  aws-lc-sys); o **manager** mantém `bookworm-slim` (sem S3 na v1). Os 3
  Dockerfiles Rust seguem compartilhando o digest do builder.

**Frontend**

- **Paleta de classes no backend desalinhada do v2 — ABERTA 2026-09-07**
  (origem: fatia redesign UI v2): cores de classes nascem no Postgres com a
  paleta da v1 (`services/api-principal/src/datasets/models.rs:11-18`
  `CLASS_PALETTE`, cabeça `#10b981` emerald + `#f59e0b` amber) e o editor
  renderiza `cls.color` como veio do banco. É dado, não estilo — realinhar a
  rotação de cores no backend numa fatia futura (decidir: migration dos
  valores existentes + enum novo).
- **Canvas do editor com navy v1 hardcoded — ABERTA 2026-09-07**
  (origem: fatia redesign UI v2): `apps/web/app/(studio)/datasets/[id]/
  annotate/[imageId]/page.tsx:670` usa `bg-[#0b0f17]` pré-existente (fora do
  token v2 `zinc-950`/`--bg`). Limpeza trivial em fatia de manutenção.
- ~~**Telemetria real da sidebar — ABERTA 2026-09-07~~ **QUITADA 2026-09-08**
  (Fatia 4, `feat/jobs-v1`): card TELEMETRIA DO NÓ agora faz polling
  `getTelemetry()` a cada 3s (`Sidebar.tsx:30-48`), preenche VRAM/CPU/RAM
  (barras `brand`); VRAM `"sem GPU (mock)"` quando `!measured`; remove
  `title="Telemetria chega na fatia 4"`.
- **Contraste do CTA documentado (NÃO é dívida — decisão da fatia redesign
  UI v2)**: CTA em `brand-500` (`#8350f2`) + `text-white` ≈ 4.78:1 — passa
  WCAG AA (texto normal, 4.5:1); piso mínimo, ver nota em
  `docs/design-system.md` (Components → Buttons). Registrado aqui só como
  referência contra re-litígios de contraste.
- **Testes de UI** (backlog §12 do `frontend.md`) — cobrir
  criar→listar→excluir quando o e2e for ampliado.
- **SegmentedControl `h-7` < hit-area mínima de 28px — ABERTA 2026-09-09**
  (origem: auditoria visual F6.2, `apps/web/components/ui/SegmentedControl.tsx`
  ~L47; `h-7` com root 14px = 24.5px). Contradição interna do `docs/DESIGN.md`
  (linha ~302 manda `h-7` por fidelidade Arcane v2.1; linha ~263 exige ≥28px —
  WCAG 2.5.8). Decisão do coordenador: aceito por ora (fidelidade Arcane),
  componente global usado em várias páginas — resolver na fatia de refinamento
  de componentes com medição.
- ~~**`emerald-400` de marca em `CreateDatasetModal.tsx` — ABERTA 2026-09-09**
  (origem: achado do F6.2 fora do escopo, `apps/web/components/studio/
  CreateDatasetModal.tsx` ~L565). Viola a regra Brand-Only do `docs/DESIGN.md`
  (emerald só como semântica literal documentada; classes em emerald em marca
  são proibidas). Fix trivial em fatia de limpeza web (F6.4+).~~ **QUITADA 2026-09-11** (Fatia R1, commit `f1892f1` — trocado por `text-[#34d399]`, token semântico sancionado do DESIGN.md).
- **"V1.3" hardcoded na página de login — ABERTA 2026-09-09**
  (origem: smoke F6.2, `apps/web/app/login/page.tsx` ~L151). A versão de
  produto real vem de `GET /health` (`version:"0.1.0"`, ADR-0009 D5); o login
  mostra "V1.3" inventado. Alinhar na fatia de limpeza web (mesma da 2).
- **AutoLabel v2 (modelos reais/VLM e runners GPU) — ABERTA 2026-09-11**
  (ADR-0016 D5): AutoLabel v1 foi entregue com modelo mock determinístico local
  reutilizando o subcomando `autolabel` em `trainer-yolo`. A v2 prevê a integração
  de modelos reais (Florence-2, Qwen-VL, BLIP-2 ou endpoint de API multimodal) e
  runner dedicado com aceleração GPU.

- **Action Center (review 2026-09-16) — ABERTA 2026-09-16:**
  NIT-1 — sem testes unit de jobCapabilities/jobMetrics/galeria/sync-URL;
  NIT-3 — gates `kind===`/`engine===` redundantes ao registry (switch ainda
  espalhado); NIT-6 — `selectJob`/`setFocus` leem `window.location.search` em
  vez de `searchParams` (risco de dessync + flash no deep-link `&focus=1`);
  NIT-9 — `untaggedSamples` furam o colapso da galeria; teste de paridade
  front `isTrainingMetric` × orquestrador `is_training_metric` (pós-merge
  das 3 branches).
- **Modal base sem focus-trap/foco inicial — ABERTA 2026-09-10 (review I.8, componente pré-existente):** `apps/web/components/ui/Modal.tsx:50-57` não captura Tab dentro do modal nem move o foco inicial para o primeiro elemento focável; Esc e click-outside funcionam. Modais novos da fatia I herdam. Correção global no componente base numa fatia de refinamento (afeta todos os modais da casa).

## Quitadas

- **AutoLabel v1 (mock, local) — QUITADA 2026-09-11** (Fatia AutoLabel v1, ADR-0016:
  migration 0009 expandindo `captions.origin` com `'autolabel'`, subcomando
  `autolabel` determinístico em `trainer-yolo`, matriz de despacho e coleta de
  `captions.jsonl` no orquestrador/manager, rotas `POST /api/jobs/autolabel` e
  `POST /api/jobs/:id/autolabel/apply`, `AutoLabelModal` com `NodeSelect` na
  galeria web, ação de aplicar legendas em `/jobs` e ActionCenter; spec OpenAPI
  0.15.0).

- **Seleção manual de nó/GPU na UI — QUITADA 2026-09-11** (Fatia N, ADR-0015:
  wire com `orchestratorName`, `orchestratorKind` e `orchestratorFallback` no
  manager e `api-principal`; `orchestratorId` opcional nos 3 submits públicos
  `/yolo`, `/autotracker`, `/predict`; validação fail-fast 400 não-UUID ou
  indisponível / 404 inexistente; roteamento de 1º nível com fallback automático
  após timeout de 120s gravando `orchestrator_fallback = true`; componente
  `NodeSelect` reutilizável, exibição de nó/fallback no `/jobs` e seleção em
  `/playground`, `/treino` e `AutoTrackerModal`; spec OpenAPI bumped para 0.14.0).

- **T7 — `autoTracked` derivado — QUITADA 2026-09-06** (`61dc75b` em
  `feat/datasets-gallery`, 3d.1): campo do wire `Dataset` agora deriva de
  `EXISTS(boxes.origin='autotracker')` nas 3 queries de `handlers.rs`
  (`DatasetRow.auto_tracked` + teste de integração com 3 casos: autotracker→
  true, manual→false, vazio→false). Sem mudança de contrato (campo já existia; 
  descrição do openapi atualizada).

- **3e (export/import de dataset) — QUITADA 2026-09-07**
  (`feat/datasets-export-import`: export `21b95b9`, import `f37d76f`, UI
  `44cf475`, fixes `71b5587`/`23e0f3e`/`b07c215`/`096eb95`; review final
  APROVA COM NITS). Entregue: `POST /:id/export` (200 401 404 503, zip em
  stream do banco em tempdir) + `POST /datasets/import` (201 400 401 409
  503, multipart `file`+`title`≤96+`replace`, substituição consentida
  409→`replace=true`, erro novo `import_invalid`, limite 200 MiB + 8 MiB
  envelope); UI na galeria (`lib/backup.ts`, `ImportDatasetModal`,
  Importar removido do header). `POST /:id/package` + `dataset_versions`
  (T4) ficam na fatia 4. Sem migration (schema já tinha `origin` com
  `import` desde a 0003 — §10 intocado).
  - Nota: a ordem de boxes no wire segue não-determinística (`ORDER BY id`
    UUID — dívida "Ordem estável de `boxes`" acima, pré-existente e
    reafirmada, não duplicada; o teste da 3e compara por identidade).

- **3b (upload/imagens) — QUITADA 2026-09-05** (`feat/datasets-storage`,
  `f6c6ff5`..`393163c`; docs no 3b.8). Entregue: bucket S3/SeaweedFS como blob
  canônico (volume `datasets`/`DATASETS_DIR` do principal mortos; sweep de
  prefixo `datasets/<id>/` pós-commit best-effort), `DefaultBodyLimit`
  dedicado (200 MiB total + 8 MiB + teto por arquivo, envelope — ADR-0002
  T10), `images.object_key` + `sha256`/`media_type`, `videos` sem rota de
  escrita, `heph_refresh_dataset_counters` (nunca `+=`, status derivado),
  `source` derivado `s3://{bucket}/datasets/{id}/`, `classes` como
  `{id,name,idx,color}`. Sobrou como PLANO (não dívida): export/import/package
  → 3e (backup interim = console + `mc mirror`); UI `/datasets` (3c, feita);
  galeria/anotação (3d, próxima).
