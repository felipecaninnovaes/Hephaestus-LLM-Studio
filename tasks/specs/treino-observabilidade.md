# Spec em análise — Telemetria, Logs, Repetir Treino, ETA e ZIP de Artefatos

Origem: sugestões do usuário (2026-09-20). **Status: IMPLEMENTADO — fatia
`fix/treino-observabilidade` (2026-09-20/21); deploy de orquestrador/BFF/engines
pendente de janela segura (treino em andamento impediu restart dos serviços).**
Toda causa raiz abaixo foi verificada no código (linhas exatas). Ordem de execução
sugerida ao final.

---

## C1. "Repetir Treino" / import de config (defeito — 3 falhas empilhadas)

### Falha A — chaves sessionStorage órfãs (fatal, no ActionCenter)
- `apps/web/components/studio/action-center/ActionCenter.tsx:215,254` escreve
  `heph_resume_job`; `:280` escreve `heph_rerun_yolo`. **Zero consumidores** no
  codebase. O roteador `/difusao` lê apenas `hephaestus_diffusion_resume`
  (`apps/web/app/(studio)/difusao/page.tsx:29`). O `?rerun=1`/`?resume=1` da URL
  não é parseado por ninguém.
- Consequência: "Repetir Treino" e "Retomar" do drawer abrem a Forja com defaults
  (não importam nada).
- A variante de `apps/web/app/(studio)/jobs/page.tsx:426,538` usa a chave correta e
  funciona → **dois mappers duplicados e divergentes da mesma lógica**.

### Falha B — training_config.json ≠ formato de preset
- `training_config.json` é o YAML da engine convertido (`serde_yaml→serde_json`)
  em `services/orchestrator/src/app/mod.rs:592-599`: snake_case, com
  `rank`/`alpha`/`learning_rate` **aninhados sob `lora:`** e `train_batch_size`.
- O importador da Forja (`apps/web/components/studio/diffusion/ForjaDifusaoSetup.tsx:253+`)
  espera camelCase (`baseModel`, `batchSize`, …); fallbacks snake são parciais e só
  de topo (`base_model`, `epochs`, `rank`). Export de preset e config de treino são
  dois contratos sem mapper entre si → "o arquivo de config não importa direito".

### Falha C — download fabrica config quando o artefato não existe
- `handleDownloadJobConfig` (`apps/web/app/(studio)/jobs/page.tsx:551+`), na ausência
  do artefato `kind:"config"`, gera um JSON de `job.params` e o entrega como se
  fosse a config real da engine (enganoso; job pode ter falhado antes do upload).

### Nota de dados
- `job.params` persistido pelo BFF (`services/api-principal/src/jobs/handlers.rs:1757-1785`)
  contém **todos** os hyperparams reais em camelCase. O rerun do ActionCenter ainda
  dropa `lrScheduler`, `lrWarmupSteps`, `enableBucket`, `sample*`, `weights`,
  `outputName` que a versão de `/jobs` mapeia.

### Direção de fix (sem contrato novo)
1. Extrair `paramsToPreset(job)` única em `apps/web/lib/`; ActionCenter passa a
   consumir `hephaestus_diffusion_resume` (e chave análoga p/ YOLO consumida por
   `/treino`), eliminando o mapper duplicado.
2. Mapper snake→preset no `handleImportPreset` aceitando o shape real do
   `training_config.json` (percorrer `lora:`/`data:`), **ou** orquestrador passar a
   gravar também o preset canônico como artefato adicional.
3. No fallback do download, rotular o JSON gerado (ex.: campo `_source: "job.params"`)
  — nunca entregar como config real.
- Esforço: pequeno (~100 LOC), só `apps/web` (+ opcional orquestrador).

### Nota de não-escopo (2026-09-20 — decisão do coordenador)
- "Retomar" neste spec = **continuação por pesos LoRA + `epoch_offset`**,
  mecanismo que já existe na engine (`weights_path`/`epoch_offset`; coberto por
  `engines/trainer-difusao/tests/test_train.py:416` `test_train_mock_resume_with_offset_and_weights`)
  e já funciona via `/jobs` (`handleResumeFromCheckpoint`). O que a C1 conserta é
  só o roteamento UI (chave órfã do ActionCenter) + mapper de preset.
- Resume de **estado de otimizador/scheduler/global_step** NÃO existe no repo e
  NÃO é prometido por esta spec. Implementadores da C1 são proibidos de escalar
  o fix para dentro da engine ou do contrato de dispatch.
- Remover junto os query params mortos `?resume=1&checkpointId=...&epochOffset=...`
  do `router.push` do ActionCenter (`ActionCenter.tsx:222`) — ninguém os parseia.

---

## C2. Logs perdem no refresh; gráfico de loss sumiu; cache latent sem log

### C2a. Logs não são persistidos em lugar nenhum (arquitetural)
- `apps/web/components/studio/JobLogViewer.tsx:61` (`liveLogEntries`) é estado React
  puro alimentado por SSE transitório; as demais linhas são **sintetizadas** de
  `job.status/phase/metrics/artifacts` (:106-267). Manager não tem coluna/tabela de
  logs; orquestrador nunca reporta stdout da engine.
- Refresh → estado zero. Fix exige decisão de design:
  - (a) **Recomendado:** upload incremental de `telemetry.jsonl` como artefato
    (o tail já existe — `services/orchestrator/src/app/stages/collector.rs`) +
    `GET /api/jobs/:id/logs?offset=` no BFF lendo o objeto via StoragePort.
  - (b) Tabela `job_log_lines` no manager alimentada por `report_job`.
- (a) evita schema novo e sobrevive à modularização do orquestrador.
- **Quitado 2026-09-21 (opção (a)):** orquestrador faz upload incremental de
  `telemetry.jsonl` para `artifacts/<job>/logs/telemetry.jsonl` (commit 7c89958
  one-shot+daemon com growth-gate e anúncio único; dedupe por path no manager);
  BFF `GET /api/jobs/{id}/logs?offset=&limit=` pagina o objeto via StoragePort
  (7366c9b); `JobLogViewer` consome com dedupe por texto contra SSE/sintetizado
  e degrada honestamente enquanto o endpoint não estiver deployado (9863021).

### C2b. Gráfico de loss vazio — REGRESSÃO concreta (provável RD-022/ADR-0023)
Cadeia verificada:
1. Collector prioriza `telemetry.jsonl` sobre `metrics.jsonl`
   (`services/orchestrator/src/app/stages/collector.rs:354-358`;
   `services/orchestrator/src/app/mod.rs:946-951`; `read_final_metrics` :136-140).
2. Em `telemetry.jsonl`, `loss`/`lr` vão **aninhados sob `"metrics"`**
   (`engines/engine-kit/src/engine_kit/telemetry.py:78-80`); só o espelho legado
   achata (`legacy_payload.update(metrics)`, :96).
3. `parse_metrics_line` lê **somente topo**
   (`services/orchestrator/src/telemetry/metrics.rs:131-132`).
4. → `is_training_metric()==false` → `telemetry_report_for_line` reporta
   `metrics: None` (:201-205) → `upsert_metrics` (manager) nunca recebe linha →
   `jobs.metrics` vazio → gráfico/chips vazios em runtime **e pós-refresh**, até em
   job concluído.
- **Fix:** `parse_metrics_line` achatar `v["metrics"]` como fonte fallback.
  1 função no orquestrador + teste unitário. Menor diff, maior impacto — destrava
  gráfico, chips e a F1 (ETA).

### C2c. Fases de pré-processamento (split/cache latent) sem telemetria
- O pré-compute emite só `print("[INFO] ...")`
  (`engines/trainer-difusao/src/trainer_difusao/common_pkg/text_embeds.py:88-108`);
  não há `emitter.emit(phase=...)` para preparação de dataset nem cache de
  latents/text embeddings. No modo daemon (hot path ADR-0023) o stdout não é
  roteado por job → invisível no log do job.
- **Fix:** emitir fases estruturadas (`preparing_dataset`, `preparing_cache` com
  progresso i/N) junto de C2a.
- **Quitado 2026-09-20:** implementado via `_emit_metric(..., telemetry_only=True)`
  (não contamina `metrics.jsonl`); progresso 0.07 dataset → 0.07–0.08 cache em
  chunks `i/N` → 0.08 `dataset_ready`; flux/sd15/sdxl/mock repassam
  `metrics_path`. **Achado:** não existe split train/val no trainer-difusao —
  `preparing_dataset` cobre scan + bucketing + dataset de controle. Pendência
  levada à Wave 2: labels `preparing_dataset`/`preparing_cache` em
  `PHASE_LABELS` (`JobProgressLive.tsx:30`).

---

## F1. ETA de treino (feature — custo baixo)
- Dados já existem: `JobTelemetryEvent` carrega `timestamp` ISO + `step`/`totalSteps`
  + `epoch`/`totalEpochs` (`crates/heph-contracts/src/telemetry.rs:6-24`) via SSE
  `/api/jobs/:id/events`; a UI já renderiza progresso (`JobProgressLive`,
  `formatDuration`).
- Algoritmo: média móvel dos deltas step/lastEventTimestamp × steps restantes.
- Riscos a tratar: epoch 1 inclui load de modelo + pré-compute de cache
  (superestima) → computar só com eventos da fase `training`; epochs com
  sample/checkpoint inflam a média → janela rolante (P50 ou descartando outliers).
- Escopo: só `apps/web` (`lib/jobMetrics.ts` + `JobProgressLive`), ~50–80 LOC.
- **Depende de C2b** para funcionar com dados persistidos.
- **Quitado 2026-09-20 (commit b8b3844):** mediana P50 de ms/step, janela 30
  deltas, só `phase === "training"`, stalls >120s (sample/checkpoint) e step
  retrocedido descartados; `estimateTrainingEtaMs` em `lib/jobMetrics.ts` +
  badge no `JobProgressLive` (compact/full).

## F2. ZIP de artefatos pós-treino (feature — decisão de arquitetura)
- Inventário pronto: `GET /api/jobs/:id/artifacts` + proxy por objeto
  `/artifacts/:id/data` (`services/api-principal/src/jobs/handlers.rs:682-783`);
  kinds `model`/`config`/`metrics`/samples já reportados.
- Opções:
  - **Streaming no BFF (recomendado):** zip `stored` (sem compressão — safetensors
    já comprimem), entradas config/logs/metrics/samples, resposta chunked.
    Custo: conexão longa no `api-principal` (alinhar timeout do Caddy/proxy).
  - Orquestrador gera zip no fim (`app/mod.rs:1181+` já faz upload final): trivial
    de integrar, mas dobra storage e só existe pós-conclusão.
- Precedente de contrato ZIP no repo: backup de dataset
  (`importDataset`/`runBackupImport` em `apps/web/components/studio/CreateDatasetModal.tsx`).
- Exige: rota nova + bump de `packages/contracts/openapi.yaml` (edição sequencial
  por regra de ownership). Esforço: médio.
- **Quitado 2026-09-21 (streaming no BFF):** `GET /api/jobs/{id}/artifacts/zip`
  stored, padrão ADR-0006 (spool por objeto via `get_to_file`, `ZipWriter` em
  `spawn_blocking`, `ReaderStream` no body — nunca o job inteiro em RAM),
  filename `outputName` sanitizado; bump único do openapi com as duas rotas +
  `JobLogLine`/`JobLogPage` (7366c9b); botões em `/jobs` e ActionCenter (9863021).
  ⚠ Alinhar timeout do Caddy/proxy antes de expor para jobs de vários GB.

---

## Sequência sugerida

| # | Item | Por quê |
|:--|:-----|:--------|
| 1 | C2b — parser `metrics.rs` com fallback `v["metrics"]` | Destrava gráfico, chips e F1; menor diff |
| 2 | C1 — chaves sessionStorage + mapper config | 100% web, sem contrato novo |
| 3 | C2a + C2c — persistência de logs + fases de cache | Design leve via (a) artefato + endpoint |
| 4 | F1 — ETA | Precisa da telemetria saudável do passo 1 |
| 5 | F2 — ZIP | Bump de openapi; sequencial por ownership de contratos |
