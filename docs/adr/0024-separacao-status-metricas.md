# ADR-0024 — Separação eventos de status × métricas de treino (AC-006-A)

Data: 2026-09-16 · Status: ACEITA (coordenador; plano `docs/plano-action-center.md` AC-006,
decisão nº4 do usuário: "B agora + A depois") · Fatia: `feat/jobs-status-metrics-split`

## Contexto

Os engines escrevem **mensagens de status** (boot, carregamento de modelo/dataset,
`training_started`, progresso por imagem do AutoLabel) no MESMO `metrics.jsonl` das
métricas de treino (`engines/trainer-difusao/.../common.py:68-98` + callsites em
`flux/sd15/sdxl/mock`; `autolabel.py:443-452`). O orquestrador reporta cada linha como
métrica (`services/orchestrator/src/lib.rs:1677-1724`) e o manager persiste tudo no JSONB
`jobs.metrics` (`services/manager/src/lib.rs:983-1024`). Consequências provadas:

1. O gráfico de convergência nasce com ~10 "steps" falsos e "zera" quando o loop real
   reinicia a numeração; a contagem de checkpoints mente.
2. Colisão silenciosa `(epoch, step)` no upsert: a linha `training_started` usa
   `(epoch_offset, 10)`; se o loop global passar por `(0, 10)`, sobrescreve.
3. O chip "Step" do Action Center lê `latestMetric.step` — valor de status.

O stopgap AC-006-B (filtro no selector compartilhado `apps/web/lib/jobMetrics.ts`) já
remove a sintoma na UI, mas o banco continua poluído e a colisão (2) persiste.

## Decisões

- **D0 — Contrato do engine NÃO muda.** `metrics.jsonl` continua transporte misto;
  quem separa é a borda. Engines existentes (todos os formatos hoje aceitos por
  `parse_metrics_line`) continuam válidos sem release de imagem.
- **D1 — Classificação no orquestrador.** Linha é **evento de status** ⇔ não carrega
  nenhum valor de treino: `loss == null && lr == null && box_loss == 0 && cls_loss == 0
  && dfl_loss == 0 && mAP50 == 0 && mAP50-95 == 0`. Caso contrário é **métrica**.
  (É a mesma regra do filtro B do front, espelhada na borda — duple sourcing aceito
  pois B é defensivo.) Linhas com fase E valores (ex.: métrica com `phase` de época)
  são métricas e vão integralmente.
- **D2 — Report ganha canal de status no topo.** `ReportBody`/`ReportRequest` passam a
  aceitar campos opcionais `phase: Option<String>` e `message: Option<String>` (serde
  default = retrocompatível com orquestradores antigos). Evento de status → report com
  `metrics: None` + `progress`/`epoch`/`step` + `phase`/`message`; métrica → como hoje
  (`metrics: Some(...)`). `vramUsedGb` continua derivado das linhas de métrica (os
  emissores de telemetria já o espelham nelas; eventos de status não carregam VRAM).
- **D3 — Manager persiste o ÚLTIMO status no job.** Migration `0013_jobs_status.sql`:
  colunas `jobs.phase TEXT` e `jobs.message TEXT` (snapshot, não histórico — eventos
  históricos ficam fora de escopo v1). `report_job` (ramo `preparing|running`) grava
  `COALESCE($n, phase)`; `JobRow` expõe `phase`/`message`. O array `jobs.metrics` passa
  a conter **apenas pontos de dados** ⇒ colisão D0.2 eliminada. Reports terminais
  carregam phase/message e são persistidos via COALESCE (job done mostra `completed`,
  failed mostra `error` + mensagem).
- **D4 — Principal lê o status das colunas.** `to_job_response` deriva
  `phase`/`phaseMessage` dos campos topo do `InternalJob` (colunas D3),
  com o fallback de status-para-fase atual; a derivação a partir do último item do
  array metrics é REMOVIDA. `vramUsedGb` permanece derivado da última métrica. O SSE
  `JobTelemetryEvent` segue inalterado (lê do `JobResponse`). `GET /api/jobs/:id/metrics`
  passa a devolver só pontos reais — mudança de comportamento de endpoint especificado
  ⇒ **openapi minor + nota em `docs/backend.md` §9** (versão: a fatia abre 0.28.0 se AC-003 já subiu 0.27.0 na main).
- **D5 — AutoLabel**: linhas por-imagem (zeros + `progress`) viram eventos de status ⇒
  `jobs.metrics` fica vazio para autolabel; a contagem de imagens vem de
  `progress`/`step` do job (UI já feita na AC-002: chip "Imagens processadas").
- **D6 — Front**: o filtro B (`lib/jobMetrics.ts`) permanece como defesa em profundidade;
  nenhum novo trabalho de UI nesta fatia.

## Consequências

- Jobs antigos (pré-migration) continuam com linhas de status dentro de
  `jobs.metrics` — o filtro B cobre a leitura; nenhuma migração de backfill.
- `phase`/`message` são last-write-wins por design (o que o SSE já mostra).
- O report de progresso do staging (AC-007) reusa exatamente o canal criado aqui
  (`phase = staging_*`), consumidor nº 2.
