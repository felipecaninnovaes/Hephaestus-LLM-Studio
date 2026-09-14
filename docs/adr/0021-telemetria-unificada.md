# ADR-0021 — Telemetria Unificada: eventos em tempo real, fases padronizadas e streaming SSE

- **Status:** ACEITA
- **Data:** 2026-09-13
- **Componentes:** `packages/contracts` (OpenAPI 0.20.0 → 0.21.0), `engines/trainer-difusao`, `engines/trainer-yolo`, `services/orchestrator`, `services/manager`, `services/api-principal`, `apps/web`.

## Contexto

À medida que o Hephaestus Studio incorporou novas capacidades (treinamento YOLO, autotracker, prévia/aplicação de autolabel, treinamento LoRA de difusão quantizado e playground Text-to-Image), cada motor implementou a emissão de métricas e status de forma ad-hoc:
1. `trainer-yolo` gerava saída textual Ultralytics parsing-dependent.
2. `trainer-difusao` escrevia métricas com campos arbitrários em `metrics.jsonl`.
3. `autotracker` media progresso pelo número de imagens processadas.
4. Jobs interativos rápidos (como geração no Playground) ou fases de preparação de hardware (download de pesos e quantização) ficavam invisíveis para a interface web, pois o frontend dependia de polling a cada 2–3 segundos e de uma tabela rígida de métricas indexada por `epoch`.

O estúdio requer um padrão universal, extensível e leve de telemetria de execução que atenda tanto a jobs rápidos de segundos quanto a treinamentos longos de horas.

## Decisões

### D0 — Contrato Canônico de Eventos de Telemetria (`JobTelemetryEvent`)
- Definido em `packages/contracts/openapi.yaml` sob o schema `JobTelemetryEvent`:
  - `timestamp`: RFC 3339 UTC.
  - `phase`: String identificando a fase atual (ex.: `init`, `downloading`, `quantizing`, `training`, `generating`, `saving`, `completed`, `error`).
  - `phaseMessage`: Mensagem amigável legível para o usuário final.
  - `progress`: Número decimal entre 0.0 e 1.0 (contínuo).
  - `step`: Passo atual de execução (inteiro opcional).
  - `totalSteps`: Total estimado de passos (inteiro opcional).
  - `epoch`: Época atual (inteiro opcional).
  - `totalEpochs`: Total de épocas (inteiro opcional).
  - `vramUsedGb`: Uso medido de VRAM em gigabytes (float opcional).
  - `metrics`: Dicionário chave-valor com métricas numéricas instantâneas (ex.: `loss`, `lr`, `mAP50`, `it_s`, `eta_seconds`).
- No modelo de job (`JobResponse`), adicionados os campos de snapshot do último evento: `phase`, `phaseMessage`, `vramUsedGb`.

### D1 — Emissor Unificado em Python (`engines/common` ou módulo canônico de telemetria)
- Arquivo padronizado de saída gerado pelo container: `outputs/telemetry.jsonl` (e retrocompatibilidade com `metrics.jsonl`).
- Helper reutilizável `TelemetryEmitter`:
  - Gravação de linhas JSON com `flush=True` atômico.
  - Métodos ergonômicos: `emit(phase, message, progress, ...)`, `set_phase()`, `log_metric()`.
  - Tratamento preventivo de erros com captura de exceções não tratadas e emissão de evento de fase `error`.

### D2 — Ingestão e Enriquecimento no Orquestrador (Rust)
- O orquestrador monitora o arquivo de telemetria gerado no diretório do job montado (`telemetry.jsonl` / `metrics.jsonl`).
- Quando disponível telemetria de hardware (ex.: via `nvidia-smi` / NVML), o orquestrador injeta `vram_used_gb` real no payload do evento se o motor não tiver informado.
- Os eventos são despachados de forma contínua para o `manager` via `POST /internal/jobs/{id}/telemetry` (com fallback para `POST /internal/jobs/{id}/metrics`).

### D3 — Manager e API Principal: Streaming via Server-Sent Events (SSE)
- O Manager mantém em memória o último snapshot de telemetria por job ativo e distribui eventos via canal assíncrono Tokio broadcast (`tokio::sync::broadcast`).
- A API Principal disponibiliza o endpoint HTTP streaming:
  `GET /api/jobs/{id}/events` (Content-Type: `text/event-stream`, Server-Sent Events).
- Se o cliente conectar enquanto o job já estiver em andamento, o endpoint envia imediatamente o snapshot atual como primeiro evento (`event: snapshot`) e em seguida encaminha os eventos ao vivo (`event: telemetry`).
- Quando o job encerra (`done` ou `failed`), um evento final (`event: finished`) é emitido e o canal é fechado graciosamente.

### D4 — Frontend Web: Hook Reutilizável `useJobTelemetry` e Componente `JobProgressLive`
- Hook `useJobTelemetry(jobId)`:
  - Estabelece conexão `EventSource` com `/api/jobs/${jobId}/events`.
  - Possui fallback transparente para polling em `GET /api/jobs/${jobId}` caso o navegador ou proxy interrompa a conexão SSE.
  - Devolve: `{ phase, phaseMessage, progress, vramUsedGb, metrics, isLive, error }`.
- Componente `JobProgressLive`:
  - Pílula de fase com indicador visual pulsante em Vidro Óptico.
  - Barra de progresso contínua e suave (0–100%).
  - Medição de VRAM e ETA formatados em `font-mono`.
  - Utilizado de forma unificada no Playground, na página de detalhes de jobs (`/jobs/[id]`) e nos cards da Central de Ações.
