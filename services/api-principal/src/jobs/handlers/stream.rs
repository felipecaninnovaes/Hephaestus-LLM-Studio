//! Handlers de streaming e telemetria (SSE, métricas e telemetria global).

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
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

/// GET /api/jobs/:id/events — stream SSE de telemetria em tempo real (ADR-0021 D3).
pub async fn stream_job_events(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if parse_uuid(&id).is_none() {
        return not_found();
    }
    let initial_job = match state.manager.get_job(&id).await {
        Ok(v) => to_job_response(v),
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };

    struct StreamContext {
        id: String,
        manager: std::sync::Arc<dyn crate::jobs::manager_client::ManagerPort>,
        first_event_sent: bool,
        initial_event: JobTelemetryEvent,
        terminal_sent: bool,
        last_progress: f64,
        last_phase: String,
        last_message: Option<String>,
        last_step: Option<i64>,
        last_epoch: Option<i32>,
        last_vram: Option<f64>,
    }

    let initial_telemetry = JobTelemetryEvent::from_job_response(&initial_job);
    let initial_terminal = matches!(initial_job.status.as_str(), "done" | "failed" | "cancelled");

    let ctx = StreamContext {
        id: id.clone(),
        manager: std::sync::Arc::clone(&state.manager),
        first_event_sent: false,
        initial_event: initial_telemetry.clone(),
        terminal_sent: false,
        last_progress: initial_telemetry.progress,
        last_phase: initial_telemetry.phase.clone(),
        last_message: initial_telemetry.phase_message.clone(),
        last_step: initial_telemetry.step,
        last_epoch: initial_telemetry.epoch,
        last_vram: initial_telemetry.vram_used_gb,
    };

    let sse_stream = futures_util::stream::unfold(ctx, move |mut c| async move {
        if c.terminal_sent {
            return None;
        }

        if !c.first_event_sent {
            c.first_event_sent = true;
            let event_type = if initial_terminal {
                "finished"
            } else {
                "snapshot"
            };
            if initial_terminal {
                c.terminal_sent = true;
            }
            let data = serde_json::to_string(&c.initial_event).unwrap_or_default();
            let event = axum::response::sse::Event::default()
                .event(event_type)
                .data(data);
            return Some((Ok::<_, std::convert::Infallible>(event), c));
        }

        loop {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;

            let current_job = match c.manager.get_job(&c.id).await {
                Ok(v) => to_job_response(v),
                Err(_) => {
                    continue;
                }
            };

            let telemetry = JobTelemetryEvent::from_job_response(&current_job);
            let is_terminal =
                matches!(current_job.status.as_str(), "done" | "failed" | "cancelled");

            let has_changed = (telemetry.progress - c.last_progress).abs() > 0.0001
                || telemetry.phase != c.last_phase
                || telemetry.phase_message != c.last_message
                || telemetry.step != c.last_step
                || telemetry.epoch != c.last_epoch
                || (telemetry.vram_used_gb.unwrap_or(0.0) - c.last_vram.unwrap_or(0.0)).abs()
                    > 0.05
                || is_terminal;

            if has_changed {
                c.last_progress = telemetry.progress;
                c.last_phase = telemetry.phase.clone();
                c.last_message = telemetry.phase_message.clone();
                c.last_step = telemetry.step;
                c.last_epoch = telemetry.epoch;
                c.last_vram = telemetry.vram_used_gb;
                let event_type = if is_terminal { "finished" } else { "telemetry" };
                if is_terminal {
                    c.terminal_sent = true;
                }
                let data = serde_json::to_string(&telemetry).unwrap_or_default();
                let event = axum::response::sse::Event::default()
                    .event(event_type)
                    .data(data);
                return Some((Ok(event), c));
            }
        }
    });

    axum::response::sse::Sse::new(sse_stream)
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
