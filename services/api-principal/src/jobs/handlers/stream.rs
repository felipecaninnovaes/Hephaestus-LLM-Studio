//! Handlers de streaming e telemetria (SSE, métricas e telemetria global).

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use super::helpers::{not_found, parse_uuid, queue_unavailable, remap_metrics, to_job_response};
use super::types::{JobTelemetryEvent, JobTelemetryEventExt, TelemetryResponse};
use crate::error::err;
use crate::jobs::manager_client::ManagerError;
use crate::state::AppState;

/// Faixa válida de `maxPoints` (fatia 1a §3.4) — fora disso ⇒ 400
/// `invalid_max_points`. 10000 é um teto honesto contra downsampling
/// absurdo (sem benefício real acima disso para um gráfico).
const MAX_POINTS_RANGE: std::ops::RangeInclusive<i64> = 1..=10_000;

/// Query de `GET /api/jobs/:id/metrics` (fatia 1a §3.4). Sem NENHUM destes
/// parâmetros, a rota preserva o shape legado (`{items: MetricsItem[]}`,
/// consumido por `apps/web/lib/jobs.ts#getJobMetrics` e o gráfico de
/// convergência) — a reconstrução pivotada é feita pelo manager. Com
/// qualquer parâmetro presente, retorna o shape de pontos brutos
/// `{items: MetricPointWithKey[], maxSeq, downsampled}`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobMetricsQuery {
    pub after_seq: Option<i64>,
    pub max_points: Option<i64>,
    pub keys: Option<String>,
}

/// Dessassina do hub de fan-out quando o stream termina (fim normal,
/// desconexão do cliente, ou drop por qualquer motivo) — único jeito
/// confiável de saber "0 assinantes" já que `broadcast::Sender` nunca
/// fecha sozinho. `Drop` não pode ser `async`: dispara um spawn
/// fire-and-forget, best-effort (perder essa limpeza só deixa uma entrada
/// vazia no mapa até o próximo notice do job, sem vazamento de memória
/// sem limite: o run_fallback_poller ignora jobs sem assinante).
struct UnsubscribeGuard {
    hub: std::sync::Arc<crate::jobs::events_hub::JobEventsHub>,
    job_id: String,
}

impl Drop for UnsubscribeGuard {
    fn drop(&mut self) {
        let hub = std::sync::Arc::clone(&self.hub);
        let job_id = self.job_id.clone();
        tokio::spawn(async move {
            hub.unsubscribe_if_empty(&job_id).await;
        });
    }
}

/// GET /api/jobs/:id/events — stream SSE de telemetria em tempo real
/// (ADR-0021 D3). Fatia 1b: pub-sub via `pg_notify('job_events', …)` + hub
/// de fan-out in-process (`AppState.job_events`) — NÃO mais polling de
/// 300 ms por cliente. Reconexão com `Last-Event-ID` (seq do último
/// `metrics` recebido) busca o delta ANTES de entrar no fan-out.
pub async fn stream_job_events(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if parse_uuid(&id).is_none() {
        return not_found();
    }
    let initial_job = match state.manager.get_job(&id).await {
        Ok(v) => to_job_response(v),
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };

    let initial_telemetry = JobTelemetryEvent::from_job_response(&initial_job);
    let initial_terminal = matches!(initial_job.status.as_str(), "done" | "failed" | "cancelled");

    let first_type = if initial_terminal {
        "finished"
    } else {
        "snapshot"
    };
    let first_data = serde_json::to_string(&initial_telemetry).unwrap_or_default();
    let mut initial_events = vec![axum::response::sse::Event::default()
        .event(first_type)
        .data(first_data)];

    // Reconexão: `Last-Event-ID` é o seq do último ponto de métrica
    // recebido pelo cliente — busca só o delta antes de assinar o hub.
    let last_event_id: Option<i64> = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<i64>().ok());

    if !initial_terminal {
        if let Some(after) = last_event_id {
            if let Ok(resp) = state
                .manager
                .get_job_metric_points(&id, Some(after), None, None)
                .await
            {
                if !resp.items.is_empty() {
                    state.job_events.set_known_seq(&id, resp.max_seq).await;
                    let data = serde_json::json!({
                        "items": resp.items,
                        "maxSeq": resp.max_seq,
                    })
                    .to_string();
                    initial_events.push(
                        axum::response::sse::Event::default()
                            .event("metrics")
                            .id(resp.max_seq.to_string())
                            .data(data),
                    );
                }
            }
        }
    }

    let initial_stream = futures_util::stream::iter(
        initial_events
            .into_iter()
            .map(Ok::<_, std::convert::Infallible>),
    );

    if initial_terminal {
        return axum::response::sse::Sse::new(initial_stream)
            .keep_alive(axum::response::sse::KeepAlive::default())
            .into_response();
    }

    let rx = state.job_events.subscribe(&id).await;
    let guard = UnsubscribeGuard {
        hub: std::sync::Arc::clone(&state.job_events),
        job_id: id.clone(),
    };

    let fanout_stream = futures_util::stream::unfold(
        (rx, guard, false),
        move |(mut rx, guard, done)| async move {
            if done {
                return None;
            }
            loop {
                match rx.recv().await {
                    Ok(ev) => {
                        let mut sse = axum::response::sse::Event::default()
                            .event(ev.event)
                            .data(ev.data);
                        if let Some(sid) = ev.id {
                            sse = sse.id(sid);
                        }
                        let finished = ev.event == "finished";
                        return Some((
                            Ok::<_, std::convert::Infallible>(sse),
                            (rx, guard, finished),
                        ));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                }
            }
        },
    );

    let combined = futures_util::StreamExt::chain(initial_stream, fanout_stream);

    axum::response::sse::Sse::new(combined)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

/// GET /api/jobs/:id/metrics — métricas de um job (re-mapeadas camelCase).
///
/// Sem query params: shape legado pivotado por epoch/step (compat, removido
/// na fatia 4a). Com `afterSeq`/`maxPoints`/`keys`: proxy puro dos pontos
/// brutos do manager (`GET /internal/jobs/:id/metrics`), já camelCase.
pub async fn get_job_metrics(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<JobMetricsQuery>,
) -> Response {
    if parse_uuid(&id).is_none() {
        return not_found();
    }
    if let Some(mp) = q.max_points {
        if !MAX_POINTS_RANGE.contains(&mp) {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_max_points",
                "maxPoints deve estar entre 1 e 10000",
            );
        }
    }
    let has_points_params = q.after_seq.is_some() || q.max_points.is_some() || q.keys.is_some();
    if has_points_params {
        return match state
            .manager
            .get_job_metric_points(&id, q.after_seq, q.max_points, q.keys.as_deref())
            .await
        {
            Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
            Err(ManagerError::NotFound) => not_found(),
            Err(ManagerError::Unavailable(_)) => queue_unavailable(),
            Err(_) => queue_unavailable(),
        };
    }
    let job = match state.manager.get_job(&id).await {
        Ok(v) => v,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let items = match &job.metrics {
        Some(m) => remap_metrics(m),
        None => vec![],
    };
    (StatusCode::OK, Json(serde_json::json!({ "items": items }))).into_response()
}

/// GET /api/telemetry — telemetria do manager (proxy puro do cache de heartbeat).
///
/// Delta consciente: 503 além de 200/401 — a D7 original (:355) não listava
/// explicitamente o 503 para telemetry; o docs-sync cobre depois.
pub async fn get_telemetry(State(state): State<AppState>) -> Response {
    let t = match state.manager.get_telemetry().await {
        Ok(v) => v,
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let resp = TelemetryResponse {
        measured: t.measured,
        vram_used: t.vram_used,
        vram_total: t.vram_total,
        cpu: t.cpu,
        ram: t.ram,
        ram_total: t.ram_total,
        gpus: t.gpus,
        jobs_active: t.jobs_active,
    };
    (StatusCode::OK, Json(resp)).into_response()
}
