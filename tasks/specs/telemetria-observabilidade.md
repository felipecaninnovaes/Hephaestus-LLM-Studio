# Spec — Telemetria, Logs e Observabilidade

**Data da especificação:** 2026-10-01  
**Autor:** @orchestrator (plano base do @planner revisado com 13 correções; redação @docs)  
**Status:** Aprovada — escopo COMPLETO (ondas 0–5, 17 fatias)  
**Branches:** uma por fatia, a partir de `develop` (tabela §8)

Restrições: single-user/homelab; nó GPU 10.15.50.114 (RTX 3060 12GB + GTX 1660S 6GB, 158G disco); topologia fixa BFF (`api-principal`) → `manager` → `orchestrator` → engines. Wire público camelCase (`packages/contracts/openapi.yaml`); Rust/Postgres snake_case (`crates/heph-contracts`). Contract ≡ router: toda rota/campo novo entra no openapi na mesma fatia.

---

## 1. Diagnóstico (verificado)

| Problema | Evidência |
|---|---|
| Séries em JSONB, O(n²) por job | `services/manager/src/reporting/metrics.rs:33-76` — `upsert_metrics_conn` lê o array inteiro, faz merge por `epoch*1_000_000+step` e regrava tudo a cada report; sem `FOR UPDATE` (lost update em reports concorrentes); `metrics_key` (`:26-30`) devolve `None` sem `epoch` → ponto descartado em silêncio. |
| SSE por polling | `services/api-principal/src/jobs/handlers/stream.rs:81` — `sleep(300ms)` por cliente; cada tick chama `manager.get_job()` trazendo o JSONB inteiro. |
| Cadeia de 3 pollings + falha silenciosa | engine → `telemetry.jsonl` → tail do orchestrator (2s) → report ao manager → poll BFF 300ms → SSE. `services/orchestrator/src/app/stages/collector.rs:464,498`: `let _ = report_client.report(...)` descarta erro. |
| Contrato desatualizado | `crates/heph-contracts/src/telemetry.rs` só tem `vramUsedGb`; engine-kit já emite `vramReservedGb`, `stepTimeSeconds`, `etaSeconds`, `etaFormatted` (`engines/engine-kit/src/engine_kit/telemetry.py:130-143`). |
| stdout/stderr não persistido | `services/orchestrator/src/adapters/executor_docker.rs:94-97` captura; `services/orchestrator/src/app/mod.rs:1037-1066` descarta (só últimas 20 linhas no erro). Só `telemetry.jsonl` vai ao S3. |
| Sem downsampling server-side | Web recebe o array JSONB inteiro. |
| Zero observabilidade de plataforma | Sem OTel/Loki; `x-request-id` existe no middleware do BFF mas não é propagado; `/metrics` do manager manual. |

Lacunas funcionais: comparação de runs (overlay, hiperparâmetros, diff), métricas de sistema em série temporal, diagnóstico de treino (grad norm, LR, NaN/Inf, norma LoRA), alertas, galeria de samples por step, linhagem dataset→job→checkpoint→resume, captura de log bruto, export CSV/Parquet, retrocompat dos jobs com `jobs.metrics` legado.

---

## 2. Decisões

### 2.1. Decisões do usuário (2026-10-01) — substituem as recomendações do §2 do plano

- **Escopo:** COMPLETO, ondas 0–5 (todas as 17 fatias), na ordem de dependência do plano.
- **Observabilidade:** Loki + Tempo + Grafana (Grafana fora da porta 3000, que é do web; plano usa a porta 4000).
- **Alertas:** só UI (badge/lista no job; sem webhook — webhook registrado como item de backlog futuro, não nesta spec).
- **Retenção:** indefinida — `run.log` e `telemetry.jsonl` são artefatos do job no S3, apagados só junto com o job (sweep de `manager/src/jobs/delete.rs`); métricas em `job_metric_points` permanentes com `ON DELETE CASCADE`. Loki/Tempo são índice operacional: retenção configurável por env com default 30d.
  - *Interpretação do orchestrator, sujeita a ajuste do usuário:* o default de 30d para Loki/Tempo. Os dados canônicos (S3 + Postgres) não expiram.

### 2.2. Decisões técnicas fixadas pelo orchestrator

1. **Séries:** tabela Postgres append-only `job_metric_points` (sem Timescale/ClickHouse), chave idempotente + `seq` monotônica por job, `epoch` nullable, `ON CONFLICT DO UPDATE` (outbox at-least-once), backfill único do `jobs.metrics` JSONB via `jsonb_each`, **sem data de corte e sem leitura combinada legado+tabela**; coluna `jobs.metrics` removida em migration posterior após verificação de contagem.
2. **Pub-sub:** manager faz `pg_notify` na mesma transação do insert (payload só ponteiro `jobId`/`seq`); BFF com **um** `PgListener` e fan-out in-process por job; SSE com `Last-Event-ID`/delta por `seq`; poll lento só se o listener cair.
3. **Log bruto:** orchestrator captura stdout/stderr do container em `outputs/<job>/logs/run.log` e faz upload growth-gated; dono **@backend**.
4. **Downsampling server-side** em `GET /api/jobs/:id/metrics` (`?maxPoints=`, `?afterSeq=`), rota existente evoluída; contract ≡ router.
5. **Fix do descarte silencioso** `collector.rs:464,498` dentro da fatia 1a.
6. **Ordem onda 2:** propagação `x-request-id`/`traceparent` + `job_id` em spans ANTES do OTel; depois collector; depois Loki/Tempo/Grafana.
7. **Diagnóstico de treino só difusão** (`models/loop.py` + `models/qwen_image.py`); sensores de sistema reusam `tasks/specs/multi-gpu-sensores-selecao.md` (uma fonte só).
8. **Dependência de deploy:** fatias que tocam `services/orchestrator`/engines só sobem no nó GPU após o cutover bloqueado de `feat/no-gpu-reuso-dataset-embeds` (`tasks/active.md:15`); implementação/merge em `develop` liberados.

---

## 3. Contratos

### 3.1. Schema SQL (Postgres 16 — `infra/compose.yaml:9` usa `pgvector/pgvector:pg16`)

Migrations em `services/api-principal/migrations/` seguindo a numeração `NNNN_nome.sql` (última existente: `0019_models_arch_qwen.sql`).

```sql
-- 0020_job_metric_points.sql
ALTER TABLE jobs ADD COLUMN metric_seq BIGINT NOT NULL DEFAULT 0;

CREATE TABLE job_metric_points (
    job_id  UUID             NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    seq     BIGINT           NOT NULL,
    epoch   INTEGER,                       -- nullable: ponto sem epoch não é mais descartado
    step    BIGINT           NOT NULL,
    key     TEXT             NOT NULL,
    value   DOUBLE PRECISION NOT NULL,
    ts      TIMESTAMPTZ      NOT NULL DEFAULT now(),
    PRIMARY KEY (job_id, seq),
    CONSTRAINT job_metric_points_natural_key
        UNIQUE NULLS NOT DISTINCT (job_id, key, epoch, step)
);

CREATE INDEX job_metric_points_job_key_step_idx
    ON job_metric_points (job_id, key, step);
```

- **`seq` monotônica por job:** alocada na transação do report com `UPDATE jobs SET metric_seq = metric_seq + $n WHERE id = $1 RETURNING metric_seq` (o lock de linha serializa reports concorrentes do mesmo job — elimina o lost update atual). A PK `(job_id, seq)` serve de índice para delta por `seq`.
- **Idempotência (outbox at-least-once):** chave natural `(job_id, key, epoch, step)` com `NULLS NOT DISTINCT` (epoch nulo conta como valor). Insert:
  ```sql
  INSERT INTO job_metric_points (job_id, seq, epoch, step, key, value, ts)
  SELECT ... 
  ON CONFLICT ON CONSTRAINT job_metric_points_natural_key
  DO UPDATE SET value = EXCLUDED.value, ts = EXCLUDED.ts, seq = EXCLUDED.seq;
  ```
  Reenvio regrava valor e ganha `seq` nova (o cliente SSE recebe a correção como delta).
- A chave inclui `epoch` porque o legado deduplica por `(epoch, step)` (`metrics.rs:26-30`); `step` pode reiniciar por época.

```sql
-- 0021_backfill_job_metric_points.sql (backfill único, sem data de corte)
WITH exploded AS (
    SELECT j.id AS job_id,
           NULLIF(item->>'epoch', '')::INTEGER AS epoch,
           COALESCE((item->>'step')::BIGINT, 0) AS step,
           kv.key, (kv.value #>> '{}')::DOUBLE PRECISION AS value
    FROM jobs j
    CROSS JOIN LATERAL jsonb_array_elements(
        CASE jsonb_typeof(j.metrics)
            WHEN 'array'  THEN j.metrics
            WHEN 'object' THEN COALESCE(j.metrics->'items', jsonb_build_array(j.metrics))
            ELSE '[]'::jsonb END) AS item
    CROSS JOIN LATERAL jsonb_each(item) AS kv
    WHERE kv.key NOT IN ('epoch', 'step')
      AND jsonb_typeof(kv.value) = 'number'
), numbered AS (
    SELECT e.*, ROW_NUMBER() OVER (PARTITION BY job_id ORDER BY epoch NULLS FIRST, step, key) AS seq
    FROM exploded e
)
INSERT INTO job_metric_points (job_id, seq, epoch, step, key, value)
SELECT job_id, seq, epoch, step, key, value FROM numbered
ON CONFLICT ON CONSTRAINT job_metric_points_natural_key DO NOTHING;

UPDATE jobs j SET metric_seq = s.max_seq
FROM (SELECT job_id, MAX(seq) AS max_seq FROM job_metric_points GROUP BY job_id) s
WHERE j.id = s.job_id;
```
O formato aceito espelha `normalize_metrics_to_array` (`metrics.rs:11-22`: array, `{"items":[...]}` ou objeto único).

```sql
-- migration posterior (gate manual após contagem legado == tabela)
ALTER TABLE jobs DROP COLUMN metrics;
```

```sql
-- 0022_job_alerts.sql (fatia 3c)
CREATE TABLE job_alerts (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    job_id      UUID        NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    rule_id     TEXT        NOT NULL CHECK (rule_id IN ('nan_detected','vram_high','disk_high','telemetry_stale')),
    severity    TEXT        NOT NULL CHECK (severity IN ('warning','critical')),
    message     TEXT        NOT NULL,
    fired_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at TIMESTAMPTZ
);

-- dedupe: no máximo um alerta ativo por (job, regra)
CREATE UNIQUE INDEX job_alerts_active_uniq
    ON job_alerts (job_id, rule_id) WHERE resolved_at IS NULL;

CREATE INDEX job_alerts_job_fired_idx ON job_alerts (job_id, fired_at DESC);
```

### 3.2. `pg_notify`

- Canal único `job_metrics` (não um canal por job — permite um `PgListener` só no BFF).
- Emitido pelo manager **na mesma transação** do insert (entregue no commit).
- Payload (ponteiro, nunca valores): `{"jobId":"<uuid>","seq":<maxSeq do batch>}`.
- Alertas (3c) usam o canal `job_alerts` com payload `{"jobId":"<uuid>","alertId":"<uuid>"}`.

### 3.3. Eventos SSE (`GET /api/jobs/:id/stream`)

| `event:` | `id:` | `data:` |
|---|---|---|
| `metrics` | `<seq>` (último do lote) | `{"items":[{"seq","epoch","step","key","value","ts"}],"maxSeq":N}` — delta `seq > lastSeq` |
| `status` | — | snapshot de status do job (comportamento atual) |
| `alert` | — | objeto `JobAlert` (3c) |

Reconexão: cliente envia `Last-Event-ID: <seq>`; BFF responde com delta `afterSeq=<seq>` antes de voltar ao fan-out. Listener caído → poll lento de 5s por job (só delta), retorna ao listener quando reconectar.

### 3.4. Rotas (openapi, camelCase)

| Rota | Fatia | Contrato |
|---|---|---|
| `GET /api/jobs/:id/metrics?afterSeq=<int>&maxPoints=<int>&keys=<csv>` | 1a | `{"items":[{"seq","epoch","step","key","value","ts"}],"maxSeq":N,"downsampled":bool}`; com `maxPoints`, min/max por bucket de `step` por `key` |
| `GET /api/jobs/:id/logs` | 1c | rota existente (`services/api-principal/src/jobs/handlers/artifacts.rs:186-231`), passa a ler `logs/run.log` |
| `GET /api/jobs/:id/alerts` | 3c | `{"items":[JobAlert]}` ativos + resolvidos recentes; `JobAlert = {id, jobId, ruleId, severity, message, firedAt, resolvedAt?}` |
| `GET /api/jobs/:id/lineage` | 5b | `{"nodes":[{id, kind: dataset|job|checkpoint|generation, label}],"edges":[{from,to,kind}]}` |
| `GET /api/jobs/:id/export?format=csv|parquet` | 5c | stream `text/csv` ou `application/vnd.apache.parquet` (colunas `seq,epoch,step,key,value,ts`) |

Header: `x-request-id` (UUID) e `traceparent` (W3C) em toda resposta e chamada interna (2b).

### 3.5. `heph-contracts` (`crates/heph-contracts/src/telemetry.rs`, snake_case + `#[serde(rename_all = "camelCase")]`)

`JobTelemetryEvent` ganha (todos `Option`, `skip_serializing_if`):
- 0a: `vram_reserved_gb: f64`, `step_time_seconds: f64`, `eta_seconds: i64`, `eta_formatted: String`.
- 3a: `system_metrics: SystemMetrics { cpu_pct, ram_used_gb, disk_read_mb_s, disk_write_mb_s }` (GPU vem do `GpuDeviceTelemetry` da spec multi-GPU, não daqui).
- 3b: `diagnostics: TrainingDiagnostics { grad_norm_l2: f64, nan_count: u32, inf_count: u32, lr_per_group: Vec<f64>, lora_norms: BTreeMap<String, f64> }`.

### 3.6. `TelemetryEmitter` (`engines/engine-kit/src/engine_kit/telemetry.py`)

Campos de wire já emitidos: `timestamp`, `phase`, `phaseMessage`, `message`, `progress`, `step`, `totalSteps`, `epoch`, `totalEpochs`, `vramUsedGb`, `vramReservedGb`, `stepTimeSeconds`, `speed`, `etaSeconds`, `etaFormatted`, `metrics`. Novos kwargs de `emit()`:
- 3a: `system_metrics: dict | None` → `systemMetrics {cpuPct, ramUsedGb, diskReadMbS, diskWriteMbS}`.
- 3b: `diagnostics: dict | None` → `diagnostics {gradNormL2, nanCount, infCount, lrPerGroup, loraNorms}`.

---

## 4. Ondas e fatias

Formato: dono · branch · arquivos · depende de · aceite (binário; smoke real).

### Onda 0 — Contrato + Migração (0a ∥ 0b)

**0a — Contrato `JobTelemetryEvent` v2**  
@backend · `feat/telemetry-contract-v2` · `crates/heph-contracts/src/telemetry.rs`, `packages/contracts/openapi.yaml` · depende: —  
- [ ] Os 4 campos de §3.5/0a existem no crate e no openapi (camelCase).
- [ ] Teste de round-trip desserializa uma linha real de `telemetry.jsonl` emitida pelo engine-kit (mock) com os 4 campos preenchidos.

**0b — `job_metric_points` + backfill único**  
@backend · `feat/job-metrics-migration-sql` · `services/api-principal/migrations/0020_job_metric_points.sql`, `0021_backfill_job_metric_points.sql` · depende: —  
- [ ] Migrations aplicam numa cópia do banco de dev sem erro.
- [ ] Para cada job, `COUNT(*)` em `job_metric_points` == nº de pares numéricos (fora `epoch`/`step`) no JSONB legado (query de verificação anexada ao PR).
- [ ] Rodar o backfill duas vezes não altera a contagem.
- [ ] `DELETE FROM jobs WHERE id=<job teste>` remove seus pontos (CASCADE).

### Onda 1 — Camada de dados, pub-sub, captura de log

**1a — Insert append-only + downsampling + fix do descarte silencioso**  
@backend · `feat/metrics-append-only` · `services/manager/src/reporting/metrics.rs` (troca `upsert_metrics_conn` por insert em lote com alocação de `seq`), handler de métricas em `services/api-principal/src/jobs/handlers/stream.rs` (`get_job_metrics`), `services/orchestrator/src/app/stages/collector.rs:464,498`, `packages/contracts/openapi.yaml` · depende: 0a, 0b  
- [ ] Dois reports concorrentes (5 pontos cada) no mesmo job → 10 linhas, `seq` 1..10 sem buraco/duplicata.
- [ ] Ponto sem `epoch` é persistido (`epoch IS NULL`).
- [ ] `GET /api/jobs/:id/metrics?maxPoints=100` num job com ≥10k pontos devolve ≤100 buckets por key, `downsampled:true`.
- [ ] `?afterSeq=N` devolve só `seq > N`.
- [ ] Manager fora do ar durante um report → orchestrator loga WARN com `job_id` e reenvia (nenhum `let _ =` restante em `collector.rs` para `report`).
- [ ] Nenhum leitor de `jobs.metrics` restante no manager/BFF (grep).
- [ ] `cargo test -p manager -p api-principal -p orchestrator` verde.

**1b — Pub-sub SSE via `pg_notify` + `PgListener`**  
@backend · `feat/sse-pubsub-pgnotify` · `services/manager/src/reporting/metrics.rs`, `services/api-principal/src/state.rs`, `services/api-principal/src/jobs/handlers/stream.rs` · depende: 1a  
- [ ] Loop `sleep(300ms)` (`stream.rs:81`) removido.
- [ ] 3 clientes SSE (`curl -N`) no mesmo job recebem o mesmo evento `metrics` após um report real; BFF mantém 1 conexão `LISTEN` (verificado em `pg_stat_activity`).
- [ ] Reconexão com `Last-Event-ID: <seq>` → recebe só `seq` maiores, sem duplicata.
- [ ] Derrubar a conexão do listener (`pg_terminate_backend`) → clientes continuam recebendo via poll de 5s; listener volta sozinho.
- [ ] `cargo test -p api-principal` verde.

**1c — Captura stdout/stderr → `run.log`**  
@backend · `feat/capture-container-logs` · `services/orchestrator/src/adapters/executor_docker.rs`, `services/orchestrator/src/app/stages/collector.rs`, `services/orchestrator/src/app/mod.rs:1037-1066`, `services/api-principal/src/jobs/handlers/artifacts.rs` · depende: — (deploy: §5)  
- [ ] Job real no nó GPU termina → `outputs/<job>/logs/run.log` existe no S3 com stdout e stderr intercalados.
- [ ] Durante execução, o arquivo no S3 cresce (upload growth-gated, conferido em 2 leituras).
- [ ] `GET /api/jobs/:id/logs` pagina linhas do `run.log`.
- [ ] Apagar o job remove `run.log` (sweep de `services/manager/src/jobs/delete.rs`).

### Onda 2 — Plataforma (2b → 2a → 2c)

**2b — Propagação `x-request-id`/`traceparent` + `job_id` em spans**  
@backend · `feat/request-id-propagation` · `services/api-principal/src/auth/routes.rs:309` (`request_id_middleware`), middlewares HTTP do manager e do orchestrator, `services/orchestrator/src/adapters/report_http.rs` · depende: —  
- [ ] `curl -i` em `/api/jobs` devolve `x-request-id`.
- [ ] Uma requisição de submit gera logs no BFF, manager e orchestrator com o mesmo `request_id`.
- [ ] Report orchestrator→manager carrega `traceparent` e todos os spans do job têm `job_id`.

**2a — OTel (serviços → OTLP collector)**  
@infra + @backend · `feat/otel-instrumentation` · `infra/compose.yaml` (`otel-collector`, 4317/4318), `main.rs` de manager/api-principal/orchestrator, `Cargo.toml` (`opentelemetry`, `opentelemetry-otlp`, `tracing-opentelemetry`) · depende: 2b  
- [ ] `otel-collector` sobe e fica healthy.
- [ ] Com `OTEL_EXPORTER_OTLP_ENDPOINT` definido, um submit gera spans recebidos pelo collector (debug exporter mostra `service.name` dos 3 serviços).
- [ ] Sem a env ou com o collector parado, os serviços sobem e atendem normalmente.

**2c — Loki + Tempo + Grafana**  
@infra · `feat/loki-tempo-grafana` · `infra/compose.yaml` (`loki` 3100, `tempo`, `grafana` na porta 4000), `infra/loki-config.yaml`, `infra/tempo-config.yaml`, provisionamento de datasources do Grafana · depende: 2a  
- [ ] `docker compose -f infra/compose.yaml up -d loki tempo grafana` sobe os 3.
- [ ] Grafana em `http://<host>:4000` consulta logs do manager no Loki e abre o trace de um submit no Tempo pelo `trace_id`.
- [ ] Retenção de Loki e Tempo lida de env, default 30d (verificado na config renderizada).
- Nó GPU sem rota ao collector: export em arquivo local (fallback do 2a) — ver §6.

### Onda 3 — Sensores, diagnóstico, alertas (3a → 3b → 3c)

**3a — Métricas de sistema em série temporal**  
@engines (CPU/RAM/disco no engine-kit) + @backend (persistência dos sensores de GPU) · `feat/system-metrics-sensors` · `engines/engine-kit/src/engine_kit/sensors.py`, `engines/engine-kit/src/engine_kit/telemetry.py`, `crates/heph-contracts/src/telemetry.rs`, manager (grava sensores de GPU do job como pontos `sys.gpu.*`) · depende: 0a, 1a, e a coleta por GPU de `tasks/specs/multi-gpu-sensores-selecao.md` §3.1 (`GpuDeviceTelemetry`)  
- GPU util/power/temp/VRAM vêm **só** do coletor da spec multi-GPU (uma fonte só); engine-kit não abre `pynvml`/`nvidia-smi`.
- [ ] Job real no nó GPU: `telemetry.jsonl` tem `systemMetrics` (cpuPct, ramUsedGb, diskReadMbS, diskWriteMbS) em ≥70% das linhas de step.
- [ ] `GET /api/jobs/:id/metrics?keys=sys.gpu.util_pct,sys.gpu.temp_c` devolve série não vazia para o job, da GPU em que ele rodou.

**3b — Diagnóstico de treino (só difusão)**  
@engines · `feat/training-diagnostics` · `engines/trainer-difusao/src/trainer_difusao/models/loop.py`, `engines/trainer-difusao/src/trainer_difusao/models/qwen_image.py`, `engines/engine-kit/src/engine_kit/telemetry.py`, `crates/heph-contracts/src/telemetry.rs` · depende: 0a, 3a (deploy: §5)  
- [ ] Treino de difusão real: todo step emitido tem `diagnostics.gradNormL2` e `lrPerGroup`.
- [ ] Teste com loss forçado a NaN → `diagnostics.nanCount > 0` no `telemetry.jsonl`.
- [ ] `loraNorms` presente quando há adaptador LoRA.

**3c — Alertas (regras no manager + persistência + rota + badge)**  
@backend + @frontend · `feat/alerts-and-thresholds` · `services/manager/src/alerts/{mod.rs,rules.rs}`, migration `0022_job_alerts.sql`, `services/api-principal/src/jobs/handlers/alerts.rs`, openapi, badge/lista em `apps/web/` (card e detalhe do job) · depende: 1b, 3a, 3b  
- Regras: `nan_detected` (`diagnostics.nanCount>0` ou loss não finito), `vram_high` (uso ≥ `ALERT_VRAM_RATIO`, default 0.95 da VRAM da GPU), `disk_high` (disco do nó ≥ `ALERT_DISK_RATIO`, default 0.85), `telemetry_stale` (job `running` sem ponto novo há ≥ `ALERT_STALE_SECS`, default 300). `telemetry_stale` absorve o item 5.3 de `backend-autonomia`.
- [ ] Loss NaN injetado → `nan_detected` aparece em `GET /api/jobs/:id/alerts` e chega como evento SSE `alert`.
- [ ] Job `running` sem telemetria por mais que o limiar → `telemetry_stale`; telemetria volta → `resolvedAt` preenchido.
- [ ] Reavaliar a mesma regra não cria segundo alerta ativo (índice parcial).
- [ ] Web mostra badge com contagem no card do job; clique abre a lista (prova visual).

### Onda 4 — Gráficos e comparação (4a → 4b → 4c)

**4a — uPlot no lugar do `ConvergenceChart`**  
@frontend · `feat/charts-uplift` · `apps/web/components/studio/charts/` (componente uPlot), hook de métricas consumindo `GET /api/jobs/:id/metrics?maxPoints=` + delta SSE; `ConvergenceChartCanvas.tsx` removido · depende: 1a, 1b  
- [ ] Job em execução: gráfico atualiza via SSE sem refetch completo.
- [ ] Zoom (arrastar) e pan funcionam; legenda liga/desliga série.
- [ ] Job com 10k pontos renderiza sem travar a aba (prova visual + profile).
- [ ] Nenhum import restante de `ConvergenceChartCanvas`.

**4b — Busca de métricas multi-run**  
@frontend · `feat/metrics-multi-run-fetch` · `apps/web/lib/jobMetrics.ts`, hook multi-job · depende: 4a  
- [ ] Hook busca 3 jobs em paralelo (3 requests concorrentes no DevTools) e devolve séries separadas por `jobId`.

**4c — Comparação de runs**  
@frontend · `feat/run-comparison` · `apps/web/app/(studio)/jobs/compare/page.tsx`, `ComparisonChart.tsx`, `ComparisonTable.tsx` · depende: 4b  
- [ ] Selecionar 2–3 jobs → overlay colorido por run.
- [ ] Tabela de hiperparâmetros lado a lado destaca diferenças.

### Onda 5 — Galeria, linhagem, export

**5a — Galeria de samples por step**  
@frontend · `feat/gallery-by-step` · `apps/web/components/studio/` (galeria por step) · depende: —  
- [ ] Slider troca samples entre steps/épocas de um job real (prova visual).

**5b — Linhagem**  
@backend + @frontend · `feat/job-lineage` · manager (cálculo da cadeia), handler `GET /api/jobs/:id/lineage` no BFF, openapi, grafo em `apps/web/components/studio/` · depende: —  
- [ ] Job retomado mostra o pai, o dataset de origem e os checkpoints (rota + prova visual).

**5c — Export CSV + Parquet**  
@backend · `feat/export-metrics-formats` · `services/api-principal/src/jobs/handlers/` (export), openapi · depende: 1a  
- [ ] CSV com header `seq,epoch,step,key,value,ts` e uma linha por ponto.
- [ ] Parquet abre com `pandas.read_parquet` e tem a mesma contagem de linhas do CSV.

---

## 5. Bloqueios e sequenciamento

- **Cutover bloqueado** de `feat/no-gpu-reuso-dataset-embeds` (`tasks/active.md:15`): fatias que tocam `services/orchestrator` ou engines — **1a** (`collector.rs`), **1c**, **2a/2b** (parte orchestrator), **3a**, **3b** — podem ser implementadas e mergeadas em `develop`, mas o deploy no nó GPU espera o cutover.
- Deploy de manager/BFF: imediato após cada fatia aprovada pelo @reviewer.
- Caminho crítico: 0a ∥ 0b → 1a → 1b → (4a); 2b → 2a → 2c em paralelo com a onda 1; 3a → 3b → 3c; 4a → 4b → 4c; onda 5 após 1a.
- `DROP COLUMN jobs.metrics`: só depois de 1a em produção e verificação de contagem (gate manual do orchestrator).

## 6. Riscos

| Risco | Mitigação |
|---|---|
| Backfill inconsistente | Verificação de contagem por job (aceite 0b); coluna legada só cai após gate. |
| Perda de notificação (`pg_notify` é best-effort) | Delta por `seq` na reconexão + poll lento quando o listener cai. |
| Volume de `run.log` no S3 | Retenção acompanha o job; truncagem com marcador acima de 1 GiB por job. |
| Latência de export OTel | Exportador assíncrono; falha descarta span, nunca derruba o serviço. |
| Disco de Loki/Tempo | Retenção por env (default 30d). |
| Nó GPU sem rota ao collector | Export OTLP em arquivo local no nó; envio manual pelo operador. |
| Cutover atrasa deploy de orchestrator/engines | Merge em `develop` liberado; deploy sequenciado (§5). |

## 7. Fora de escopo

- Alertas via webhook (ntfy/Discord) — item futuro no backlog.
- MLflow/W&B.
- LTTB server-side (o servidor faz min/max por bucket; LTTB só no cliente se necessário).
- Diagnóstico de treino em YOLO e CLIP.
- Multi-tenancy/RBAC de observabilidade.
- Archive de Loki/Tempo para S3.

## 8. Resumo

| Onda | Fatia | Dono | Branch | Bloqueio de deploy |
|---|---|---|---|---|
| 0 | 0a | @backend | `feat/telemetry-contract-v2` | — |
| 0 | 0b | @backend | `feat/job-metrics-migration-sql` | — |
| 1 | 1a | @backend | `feat/metrics-append-only` | parte orchestrator: cutover |
| 1 | 1b | @backend | `feat/sse-pubsub-pgnotify` | — |
| 1 | 1c | @backend | `feat/capture-container-logs` | cutover |
| 2 | 2b | @backend | `feat/request-id-propagation` | parte orchestrator: cutover |
| 2 | 2a | @infra + @backend | `feat/otel-instrumentation` | parte orchestrator: cutover |
| 2 | 2c | @infra | `feat/loki-tempo-grafana` | — |
| 3 | 3a | @engines + @backend | `feat/system-metrics-sensors` | cutover |
| 3 | 3b | @engines | `feat/training-diagnostics` | cutover |
| 3 | 3c | @backend + @frontend | `feat/alerts-and-thresholds` | — |
| 4 | 4a | @frontend | `feat/charts-uplift` | — |
| 4 | 4b | @frontend | `feat/metrics-multi-run-fetch` | — |
| 4 | 4c | @frontend | `feat/run-comparison` | — |
| 5 | 5a | @frontend | `feat/gallery-by-step` | — |
| 5 | 5b | @backend + @frontend | `feat/job-lineage` | — |
| 5 | 5c | @backend | `feat/export-metrics-formats` | — |
