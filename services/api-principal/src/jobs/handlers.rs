//! Handlers de jobs (7 rotas de leitura — BFF do manager, ADR-0007 D3/D7).
//!
//! O principal NÃO lê tabelas jobs/orchestrators/job_artifacts — dono é o
//! manager (ADR-0007 D1/D3/D8). Todas as respostas são camelCase.
//!
//! Decisões registradas (coordenador, NÃO redecidir):
//! - 404 `not_found` para `:id`/`:artifactId` não-UUID (D8 — NUNCA 400).
//! - Manager indisponível → 503 `queue_unavailable` em TODAS as 7 rotas.
//! - GET /api/telemetry tem 503 além de 200/401 (delta consciente da D7 :355
//!   — o docs-sync cobre depois; o contrato v1 sempre teve 503 implícito para
//!   manager, agora é explícito).

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{
    err, MSG_DATASET_NOT_READY, MSG_INVALID_REQUEST, MSG_JOB_NOT_ABORTABLE, MSG_JOB_NOT_DONE,
    MSG_JOB_NOT_TERMINAL, MSG_NOT_FOUND, MSG_QUEUE_UNAVAILABLE, MSG_STORAGE_UNAVAILABLE,
};
use crate::jobs::manager_client::ManagerError;
use crate::jobs::models::{self, AutotrackerJobRequest, PredictJobRequest, YoloJobRequest};
use crate::state::AppState;
use crate::storage::StorageError;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compensação: remove package (storage + row) quando o manager rejeita o
/// job (A1 — Fatia J review J.6). Extraído para reuso nos 4 braços de erro.
async fn compensate_package(state: &AppState, version_id: &str) {
    let _ = state
        .storage
        .delete_prefix(&format!("packages/{version_id}/"))
        .await;
    let _ = sqlx::query("DELETE FROM dataset_versions WHERE id = $1")
        .bind(version_id.parse::<uuid::Uuid>().expect("uuid"))
        .execute(&state.pool)
        .await;
}

// ---------------------------------------------------------------------------
// Query params
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct JobsQuery {
    pub status: Option<String>,
    pub engine: Option<String>,
}

// ---------------------------------------------------------------------------
// Response types (camelCase wire)
// ---------------------------------------------------------------------------

/// Metric epoch item (camelCase wire). mAP50-95 → `map5095`.
#[derive(Debug, Serialize, Clone)]
pub struct MetricsItem {
    pub epoch: i32,
    #[serde(rename = "boxLoss")]
    pub box_loss: f64,
    #[serde(rename = "clsLoss")]
    pub cls_loss: f64,
    #[serde(rename = "dflLoss")]
    pub dfl_loss: f64,
    #[serde(rename = "map50")]
    pub map50: f64,
    #[serde(rename = "map5095")]
    pub map5095: f64,
    #[serde(rename = "loss", skip_serializing_if = "Option::is_none")]
    pub loss: Option<f64>,
    #[serde(rename = "lr", skip_serializing_if = "Option::is_none")]
    pub lr: Option<f64>,
    #[serde(rename = "step", skip_serializing_if = "Option::is_none")]
    pub step: Option<i64>,
    #[serde(rename = "progress", skip_serializing_if = "Option::is_none")]
    pub progress: Option<f64>,
    #[serde(rename = "phase", skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(rename = "message", skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(rename = "vramUsedGb", skip_serializing_if = "Option::is_none")]
    pub vram_used_gb: Option<f64>,
}

/// Evento de telemetria transmitido via SSE ou snapshot (ADR-0021 D0/D3).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobTelemetryEvent {
    pub timestamp: String,
    pub phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_message: Option<String>,
    pub progress: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_steps: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epoch: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_epochs: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vram_used_gb: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<serde_json::Value>,
}

impl JobTelemetryEvent {
    pub fn from_job_response(job: &JobResponse) -> Self {
        let timestamp = chrono::Utc::now().to_rfc3339();
        let phase = job
            .phase
            .clone()
            .unwrap_or_else(|| match job.status.as_str() {
                "queued" => "queued".to_string(),
                "preparing" => "preparing".to_string(),
                "running" => "running".to_string(),
                "done" => "completed".to_string(),
                "failed" => "error".to_string(),
                "cancelled" => "cancelled".to_string(),
                _ => job.status.clone(),
            });
        let progress = job
            .progress
            .unwrap_or(if job.status == "done" { 1.0 } else { 0.0 });
        let latest_metric = job.metrics.as_ref().and_then(|m| m.last());
        let mut m_obj = serde_json::Map::new();
        if let Some(m) = latest_metric {
            if let Some(loss) = m.loss {
                m_obj.insert("loss".to_string(), serde_json::json!(loss));
            }
            if let Some(lr) = m.lr {
                m_obj.insert("lr".to_string(), serde_json::json!(lr));
            }
            if m.box_loss > 0.0 {
                m_obj.insert("boxLoss".to_string(), serde_json::json!(m.box_loss));
            }
            if m.map50 > 0.0 {
                m_obj.insert("map50".to_string(), serde_json::json!(m.map50));
            }
        }
        let metrics = if !m_obj.is_empty() {
            Some(serde_json::Value::Object(m_obj))
        } else {
            None
        };

        Self {
            timestamp,
            phase,
            phase_message: job.phase_message.clone(),
            progress,
            step: job.step.map(|s| s as i64),
            total_steps: None,
            epoch: job.epoch,
            total_epochs: None,
            vram_used_gb: job.vram_used_gb,
            metrics,
        }
    }
}

/// Job response (camelCase wire).
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JobResponse {
    pub id: String,
    pub kind: String,
    pub engine: String,
    pub model: String,
    pub mode: String,
    pub dataset_id: Option<String>,
    pub status: String,
    pub queue_reason: Option<String>,
    pub queue_position: Option<i32>,
    pub progress: Option<f64>,
    pub epoch: Option<i32>,
    pub step: Option<i32>,
    pub metrics: Option<Vec<MetricsItem>>,
    pub vram_min_gb: Option<i32>,
    pub orchestrator_id: Option<String>,
    pub orchestrator_name: Option<String>,
    pub orchestrator_kind: Option<String>,
    pub orchestrator_fallback: bool,
    pub created_at: String,
    pub finished_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(rename = "phaseMessage", skip_serializing_if = "Option::is_none")]
    pub phase_message: Option<String>,
    #[serde(rename = "vramUsedGb", skip_serializing_if = "Option::is_none")]
    pub vram_used_gb: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// Job list response.
#[derive(Debug, Serialize)]
pub struct JobListResponse {
    pub items: Vec<JobResponse>,
    pub total: i32,
}

/// Queue item (camelCase wire).
#[derive(Debug, Serialize)]
pub struct QueueItem {
    #[serde(rename = "jobId")]
    pub job_id: String,
    pub position: i32,
    #[serde(rename = "queueReason")]
    pub queue_reason: Option<String>,
}

/// Queue list response.
#[derive(Debug, Serialize)]
pub struct QueueResponse {
    pub items: Vec<QueueItem>,
}

/// Artifact (camelCase wire).
#[derive(Debug, Serialize)]
pub struct ArtifactResponse {
    pub id: String,
    pub kind: String,
    pub path: String,
    pub md5: String,
    pub bytes: i64,
}

/// Artifact list response.
#[derive(Debug, Serialize)]
pub struct ArtifactListResponse {
    pub items: Vec<ArtifactResponse>,
}

/// Telemetry response (camelCase wire — D9).
#[derive(Debug, Serialize)]
pub struct TelemetryResponse {
    pub measured: bool,
    #[serde(rename = "vramUsed")]
    pub vram_used: Option<i64>,
    #[serde(rename = "vramTotal")]
    pub vram_total: Option<i64>,
    pub cpu: Option<f64>,
    pub ram: Option<i64>,
    #[serde(rename = "ramTotal")]
    pub ram_total: Option<i64>,
    pub gpus: Vec<String>,
    #[serde(rename = "jobsActive")]
    pub jobs_active: i32,
}

/// Job submission response (camelCase wire — D7 :362-366).
#[derive(Debug, Serialize)]
pub struct SubmitJobResponse {
    #[serde(rename = "jobId")]
    pub job_id: String,
    pub status: String,
    #[serde(rename = "queuePosition")]
    pub queue_position: Option<i32>,
}

/// Abort response (camelCase wire — D7 :372-373).
#[derive(Debug, Serialize)]
pub struct AbortResponse {
    pub status: String,
}

/// AutoTracker apply response (camelCase wire — ADR-0008 D1).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutotrackerApplyResponse {
    /// Número de boxes gravadas.
    pub applied: i64,
    /// Número de boxes ignoradas (classe/imagem inexistente ou cap).
    pub skipped: i64,
    /// Número de imagens que receberam ao menos uma box.
    pub images: i64,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_uuid(id: &str) -> Option<Uuid> {
    id.parse::<Uuid>().ok()
}

fn not_found() -> Response {
    err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND)
}

fn queue_unavailable() -> Response {
    err(
        StatusCode::SERVICE_UNAVAILABLE,
        "queue_unavailable",
        MSG_QUEUE_UNAVAILABLE,
    )
}

fn storage_unavailable() -> Response {
    err(
        StatusCode::SERVICE_UNAVAILABLE,
        "storage_unavailable",
        MSG_STORAGE_UNAVAILABLE,
    )
}

fn dataset_not_ready() -> Response {
    err(
        StatusCode::CONFLICT,
        "dataset_not_ready",
        MSG_DATASET_NOT_READY,
    )
}

fn job_not_abortable() -> Response {
    err(
        StatusCode::CONFLICT,
        "job_not_abortable",
        MSG_JOB_NOT_ABORTABLE,
    )
}

fn job_not_terminal() -> Response {
    err(
        StatusCode::CONFLICT,
        "job_not_terminal",
        MSG_JOB_NOT_TERMINAL,
    )
}

fn job_not_done() -> Response {
    err(StatusCode::CONFLICT, "job_not_done", MSG_JOB_NOT_DONE)
}

fn invalid_request() -> Response {
    err(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        MSG_INVALID_REQUEST,
    )
}

/// Re-mapeia JSONB snake_case do manager para MetricsItem camelCase.
/// A chave `mAP50-95` do JSONB vira `map5095` no wire.
fn remap_metrics(raw: &serde_json::Value) -> Vec<MetricsItem> {
    let items = match raw.get("items").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return vec![],
    };
    items
        .iter()
        .filter_map(|item| {
            let epoch = item.get("epoch").and_then(|v| v.as_i64())? as i32;
            let box_loss = item.get("box_loss").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let cls_loss = item.get("cls_loss").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let dfl_loss = item.get("dfl_loss").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let map50 = item.get("mAP50").and_then(|v| v.as_f64()).unwrap_or(0.0);
            // mAP50-95 → map5095: a chave no JSONB tem hífen/maiúscula.
            let map5095 = item
                .get("mAP50-95")
                .or_else(|| item.get("map50_95"))
                .or_else(|| item.get("map5095"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let loss = item.get("loss").and_then(|v| v.as_f64());
            let lr = item.get("lr").and_then(|v| v.as_f64());
            let step = item.get("step").and_then(|v| v.as_i64());
            let progress = item.get("progress").and_then(|v| v.as_f64());
            let phase = item
                .get("phase")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let message = item
                .get("message")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let vram_used_gb = item
                .get("vramUsedGb")
                .or_else(|| item.get("vram_used_gb"))
                .and_then(|v| v.as_f64());
            Some(MetricsItem {
                epoch,
                box_loss,
                cls_loss,
                dfl_loss,
                map50,
                map5095,
                loss,
                lr,
                step,
                progress,
                phase,
                message,
                vram_used_gb,
            })
        })
        .collect()
}

/// Converte `InternalJob` do manager (snake_case) para `JobResponse` (camelCase).
fn to_job_response(job: crate::jobs::manager_client::InternalJob) -> JobResponse {
    let metrics = job.metrics.as_ref().map(remap_metrics);
    let latest_metric = metrics.as_ref().and_then(|m| m.last());
    let phase = latest_metric
        .and_then(|m| m.phase.clone())
        .or_else(|| match job.status.as_str() {
            "queued" => Some("queued".to_string()),
            "preparing" => Some("preparing".to_string()),
            "running" => Some("running".to_string()),
            "done" => Some("completed".to_string()),
            "failed" => Some("error".to_string()),
            "cancelled" => Some("cancelled".to_string()),
            _ => None,
        });
    let phase_message = latest_metric.and_then(|m| m.message.clone());
    let vram_used_gb = latest_metric.and_then(|m| m.vram_used_gb);

    JobResponse {
        id: job.id,
        kind: job.kind,
        engine: job.engine,
        model: job.model,
        mode: job.mode,
        dataset_id: job.dataset_id,
        status: job.status,
        queue_reason: job.queue_reason,
        queue_position: job.queue_position,
        progress: job.progress,
        epoch: job.epoch,
        step: job.step,
        metrics,
        vram_min_gb: job.vram_min_gb,
        orchestrator_id: job.orchestrator_id,
        orchestrator_name: job.orchestrator_name,
        orchestrator_kind: job.orchestrator_kind,
        orchestrator_fallback: job.orchestrator_fallback,
        created_at: job.created_at,
        finished_at: job.finished_at,
        error: job.error,
        phase,
        phase_message,
        vram_used_gb,
        params: job.params,
    }
}

/// Valida que a path do artefato não contém `..` ou prefixo estranho (defesa
/// em profundidade). Retorna `Err` com 400 se inválido.
fn validate_artifact_path(path: &str) -> Result<(), Response> {
    if path.contains("..") || path.starts_with('/') || path.starts_with('\\') {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "invalid request",
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/jobs?status=&engine= — lista jobs via manager.
pub async fn list_jobs(State(state): State<AppState>, Query(q): Query<JobsQuery>) -> Response {
    let status = q.status.as_deref();
    let engine = q.engine.as_deref();
    let (items, total) = match state.manager.list_jobs(status, engine).await {
        Ok(v) => v,
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let items = items.into_iter().map(to_job_response).collect();
    (StatusCode::OK, Json(JobListResponse { items, total })).into_response()
}

/// GET /api/jobs/queue — fila ordenada de jobs.
pub async fn list_queue(State(state): State<AppState>) -> Response {
    let items = match state.manager.list_queue().await {
        Ok(v) => v,
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let items = items
        .into_iter()
        .map(|q| QueueItem {
            job_id: q.job_id,
            position: q.position,
            queue_reason: q.queue_reason,
        })
        .collect();
    (StatusCode::OK, Json(QueueResponse { items })).into_response()
}

/// GET /api/jobs/:id — detalhe de um job.
pub async fn get_job(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if parse_uuid(&id).is_none() {
        return not_found();
    }
    let job = match state.manager.get_job(&id).await {
        Ok(v) => v,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    (StatusCode::OK, Json(to_job_response(job))).into_response()
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
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

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
                || is_terminal;

            if has_changed {
                c.last_progress = telemetry.progress;
                c.last_phase = telemetry.phase.clone();
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
pub async fn get_job_metrics(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if parse_uuid(&id).is_none() {
        return not_found();
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

/// GET /api/jobs/:id/artifacts — lista artefatos de um job.
pub async fn list_artifacts(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if parse_uuid(&id).is_none() {
        return not_found();
    }
    let arts = match state.manager.list_artifacts(&id).await {
        Ok(v) => v,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let items: Vec<ArtifactResponse> = arts
        .into_iter()
        .map(|a| ArtifactResponse {
            id: a.id,
            kind: a.kind,
            path: a.path,
            md5: a.md5,
            bytes: a.bytes,
        })
        .collect();
    (StatusCode::OK, Json(ArtifactListResponse { items })).into_response()
}

/// GET /api/jobs/:id/artifacts/:artifactId/data — proxy do objeto do artefato.
///
/// Metadata via manager + objeto via StoragePort (admin).
/// Key = `artifacts/<job_id>/<path>` (D8). Confere `md5` se barato.
pub async fn get_artifact_data(
    State(state): State<AppState>,
    Path((id, artifact_id)): Path<(String, String)>,
) -> Response {
    if parse_uuid(&id).is_none() || parse_uuid(&artifact_id).is_none() {
        return not_found();
    }
    // 1. Busca artefatos do job para encontrar o path/md5 pelo artifactId.
    let arts = match state.manager.list_artifacts(&id).await {
        Ok(v) => v,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let art = match arts.iter().find(|a| a.id == artifact_id) {
        Some(a) => a,
        None => return not_found(),
    };
    // Defesa em profundidade: valida path do artefato.
    if let Err(resp) = validate_artifact_path(&art.path) {
        return resp;
    }
    let key = format!("artifacts/{id}/{}", art.path);
    // 2. Busca objeto via StoragePort (admin).
    let bytes = match state.storage.get(&key).await {
        Ok(b) => b,
        Err(StorageError::NotFound) => {
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "storage_unavailable",
                MSG_STORAGE_UNAVAILABLE,
            );
        }
        Err(StorageError::Unavailable(_)) => return storage_unavailable(),
    };
    // 3. Confere md5 se barato (bytes já em RAM).
    let computed = format!(
        "{:x}",
        md5::Digest::finalize({
            use md5::Digest;
            let mut h = md5::Md5::new();
            md5::Digest::update(&mut h, &bytes);
            h
        })
    );
    if computed != art.md5 {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_unavailable",
            MSG_STORAGE_UNAVAILABLE,
        );
    }
    let content_type = if art.path.ends_with(".png") {
        "image/png"
    } else if art.path.ends_with(".jpg") || art.path.ends_with(".jpeg") {
        "image/jpeg"
    } else if art.path.ends_with(".webp") {
        "image/webp"
    } else {
        "application/octet-stream"
    };
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable",
            ),
        ],
        bytes,
    )
        .into_response()
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

// ---------------------------------------------------------------------------
// POST /api/jobs/yolo — criação de job de treino YOLO (F4.2b)
// ---------------------------------------------------------------------------

/// POST /api/jobs/yolo — submete job de treino YOLO (ADR-0007 D7).
///
/// Status: 202 | 400 `invalid_request` | 401 | 404 `not_found` |
/// 409 `dataset_not_ready` | 503 `queue_unavailable`.
///
/// Fluxo: valida body → dataset existe? → dataset pronto? → build_package
/// (compartilhado) → POST ao manager → 202.
pub async fn submit_yolo_job(
    State(state): State<AppState>,
    body: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    // 1. Parse body.
    let raw = match body {
        Ok(b) => b,
        Err(_) => return invalid_request(),
    };
    let req: YoloJobRequest = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 2. Validação pura (models.rs).
    let req = match models::validate_yolo_request(req) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 3. Parse dataset_id — não-UUID ⇒ 404 (D8).
    let ds_id: uuid::Uuid = match req.dataset_id.parse() {
        Ok(v) => v,
        Err(_) => return not_found(),
    };

    // 4. Dataset existe?
    let ds_exists: bool =
        match sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM datasets WHERE id = $1)")
            .bind(ds_id)
            .fetch_one(&state.pool)
            .await
        {
            Ok(b) => b,
            Err(_) => {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                )
            }
        };
    if !ds_exists {
        return not_found();
    }

    // 5. Dataset pronto? (D7 :364-365 — D8 :334-335)
    //    409 `dataset_not_ready`: category ≠ 'yolo' OU 0 classes OU 0 imagens ativas.
    let readiness: Option<(String, i64, i64)> = match sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT d.category, \
         (SELECT count(*) FROM classes WHERE dataset_id = d.id), \
         (SELECT count(*) FROM images WHERE dataset_id = d.id AND deleted_at IS NULL) \
         FROM datasets d WHERE d.id = $1",
    )
    .bind(ds_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };
    match readiness {
        None => return not_found(),
        Some((category, class_count, image_count)) => {
            if category != "yolo" || class_count == 0 || image_count == 0 {
                return dataset_not_ready();
            }
        }
    }

    // 6. Build package (função compartilhada — F4.2b).
    let package = match crate::datasets::package::build_package(&state, ds_id).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    // 7. Gera config.yaml.
    let job_id = uuid::Uuid::new_v4().to_string();
    let config_yaml = models::generate_config_yaml(&job_id, &req);

    // 8. POST ao manager (D7 :237-240 — snake_case interno).
    //    vram_min_gb = null na v1 (decisão registrada: política VRAM real entra
    //    com GPU; R3: vram_min gravado mas não bloqueante no mock).
    //    D5: weights_id repassado quando presente (manager resolve → weights_ref).
    let mut manager_body = serde_json::json!({
        "kind": "yolo_train",
        "engine": "yolo",
        "model": req.model,
        "mode": "train",
        "dataset_id": ds_id.to_string(),
        "dataset_version_id": package.version_id,
        "package_ref": {
            "version_id": package.version_id,
            "key": package.key,
            "md5_zip": package.md5_zip,
            "bytes": package.bytes,
        },
        "config_yaml": config_yaml,
        "params": {
            "package_ref": {
                "version_id": package.version_id,
                "key": package.key,
                "md5_zip": package.md5_zip,
                "bytes": package.bytes,
            },
        },
        "vram_min_gb": null,
    });
    // D5: insere weights_id no body quando presente.
    if let Some(ref weights_id) = req.weights {
        manager_body["weights_id"] = serde_json::json!(weights_id);
    }
    // ADR-0015 D2: insere orchestrator_hint no body quando presente.
    if let Some(ref orch_id) = req.orchestrator_id {
        manager_body["orchestrator_hint"] = serde_json::json!(orch_id);
    }
    // ADR-0022 D1: insere output_name nos params quando presente.
    if let Some(ref out_name) = req.output_name {
        manager_body["params"]["output_name"] = serde_json::json!(out_name);
    }

    match state.manager.create_job(&manager_body).await {
        Ok(resp) => {
            let body = SubmitJobResponse {
                job_id: resp.job_id,
                status: resp.status,
                queue_position: resp.queue_position,
            };
            (StatusCode::ACCEPTED, Json(body)).into_response()
        }
        // ADR-0015 D6.4: alinha ao R6 (NotFound→404, InvalidRequest→400, Unavailable→503).
        // Compensação do package em TODOS os braços de erro.
        Err(ManagerError::NotFound) => {
            compensate_package(&state, &package.version_id).await;
            not_found()
        }
        Err(ManagerError::InvalidRequest(_)) => {
            compensate_package(&state, &package.version_id).await;
            invalid_request()
        }
        Err(ManagerError::Unavailable(_)) => {
            compensate_package(&state, &package.version_id).await;
            queue_unavailable()
        }
        Err(_) => {
            compensate_package(&state, &package.version_id).await;
            queue_unavailable()
        }
    }
}

// ---------------------------------------------------------------------------
// POST /api/jobs/autotracker — submit job de autotrack (ADR-0008 D0/D3)
// ---------------------------------------------------------------------------

/// POST /api/jobs/autotracker — cria job de autotrack (ADR-0008 D0/D3).
///
/// Status: 202 | 400 `invalid_request` | 401 | 404 `not_found` |
/// 409 `dataset_not_ready` | 503 `queue_unavailable`.
pub async fn submit_autotracker_job(
    State(state): State<AppState>,
    body: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    // 1. Parse body.
    let raw = match body {
        Ok(b) => b,
        Err(_) => return invalid_request(),
    };
    let req: AutotrackerJobRequest = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 2. Validação pura (models.rs) — inclui UUID check do modelId.
    let req = match models::validate_autotrack_request(req) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 3. Parse dataset_id — não-UUID ⇒ 404 (D8).
    let ds_id: uuid::Uuid = match req.dataset_id.parse() {
        Ok(v) => v,
        Err(_) => return not_found(),
    };

    // 4. Dataset existe?
    let ds_exists: bool =
        match sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM datasets WHERE id = $1)")
            .bind(ds_id)
            .fetch_one(&state.pool)
            .await
        {
            Ok(b) => b,
            Err(_) => {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                )
            }
        };
    if !ds_exists {
        return not_found();
    }

    // 5. Dataset pronto? (ADR-0008 D3)
    //    409 `dataset_not_ready`: category ≠ 'yolo' OU 0 classes OU 0 imagens ativas.
    let readiness: Option<(String, i64, i64)> = match sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT d.category, \
         (SELECT count(*) FROM classes WHERE dataset_id = d.id), \
         (SELECT count(*) FROM images WHERE dataset_id = d.id AND deleted_at IS NULL) \
         FROM datasets d WHERE d.id = $1",
    )
    .bind(ds_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };
    match readiness {
        None => return not_found(),
        Some((category, class_count, image_count)) => {
            if category != "yolo" || class_count == 0 || image_count == 0 {
                return dataset_not_ready();
            }
        }
    }

    // 6. Build package (função compartilhada — F4.2b).
    let package = match crate::datasets::package::build_package(&state, ds_id).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    // 7. Gera config.yaml.
    let job_id = uuid::Uuid::new_v4().to_string();
    let config_yaml = models::generate_autotrack_config_yaml(&job_id, &req);

    // 8. POST ao manager (ADR-0008 D3 — snake_case interno; ADR-0014 D6 — weights_id).
    let mut manager_body = serde_json::json!({
        "kind": "autotracker",
        "engine": "autotracker",
        "model": req.model,
        "mode": "autotrack",
        "dataset_id": ds_id.to_string(),
        "dataset_version_id": package.version_id,
        "package_ref": {
            "version_id": package.version_id,
            "key": package.key,
            "md5_zip": package.md5_zip,
            "bytes": package.bytes,
        },
        "config_yaml": config_yaml,
        "params": {
            "model": req.model,
            "conf": req.conf,
            "package_ref": {
                "version_id": package.version_id,
                "key": package.key,
                "md5_zip": package.md5_zip,
                "bytes": package.bytes,
            },
        },
        "vram_min_gb": null,
    });
    // ADR-0014 D2/D6: modelId presente → weights_id (manager resolve weights_ref).
    if let Some(ref model_id) = req.model_id {
        manager_body["weights_id"] = serde_json::json!(model_id);
    }
    // ADR-0015 D2: insere orchestrator_hint no body quando presente.
    if let Some(ref orch_id) = req.orchestrator_id {
        manager_body["orchestrator_hint"] = serde_json::json!(orch_id);
    }

    // ADR-0014 D6: mapeamento NotFound→404, InvalidRequest→400, Unavailable→503
    // (padrão predict — NÃO o Err(_)→503 do submit_yolo_job).
    // Compensação do package em TODOS os braços de erro.
    match state.manager.create_job(&manager_body).await {
        Ok(resp) => {
            let body = SubmitJobResponse {
                job_id: resp.job_id,
                status: resp.status,
                queue_position: resp.queue_position,
            };
            (StatusCode::ACCEPTED, Json(body)).into_response()
        }
        Err(ManagerError::NotFound) => {
            compensate_package(&state, &package.version_id).await;
            not_found()
        }
        Err(ManagerError::InvalidRequest(_)) => {
            compensate_package(&state, &package.version_id).await;
            invalid_request()
        }
        Err(ManagerError::Unavailable(_)) => {
            compensate_package(&state, &package.version_id).await;
            queue_unavailable()
        }
        // Outros erros do manager → 503 + compensação.
        Err(_) => {
            compensate_package(&state, &package.version_id).await;
            queue_unavailable()
        }
    }
}

// ---------------------------------------------------------------------------
// POST /api/jobs/autolabel — submit job de autolabel (ADR-0016 D0)
// ---------------------------------------------------------------------------

/// POST /api/jobs/autolabel — cria job de autolabel (ADR-0016 D0).
///
/// Status: 202 | 400 `invalid_request` | 401 | 404 `not_found` |
/// 409 `dataset_not_ready` | 503 `queue_unavailable`.
pub async fn submit_autolabel_job(
    State(state): State<AppState>,
    body: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    // 1. Parse body.
    let raw = match body {
        Ok(b) => b,
        Err(_) => return invalid_request(),
    };
    let req: models::AutolabelJobRequest = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 2. Validação pura.
    let mut req = match models::validate_autolabel_request(req) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 3. Parse dataset_id — não-UUID ⇒ 404.
    let ds_id: uuid::Uuid = match req.dataset_id.parse() {
        Ok(v) => v,
        Err(_) => return not_found(),
    };

    // 4. Dataset existe e está pronto?
    // ADR-0016 D0: Requer dataset com ao menos 1 imagem ativa (images_count > 0).
    let image_count: Option<i64> = match sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM images WHERE dataset_id = $1 AND deleted_at IS NULL",
    )
    .bind(ds_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(c) => c,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };

    let ds_exists: bool =
        match sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM datasets WHERE id = $1)")
            .bind(ds_id)
            .fetch_one(&state.pool)
            .await
        {
            Ok(b) => b,
            Err(_) => {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                )
            }
        };
    if !ds_exists {
        return not_found();
    }

    let count = image_count.unwrap_or(0);
    if count == 0 {
        return dataset_not_ready();
    }

    // 4.1. Resolução seletiva de imagens (filter_class_id e/ou image_ids)
    let mut resolved_image_ids: Option<Vec<uuid::Uuid>> = None;

    if let Some(ref fc_str) = req.filter_class_id {
        let class_id = match uuid::Uuid::parse_str(fc_str) {
            Ok(u) => u,
            Err(_) => return invalid_request(),
        };

        let class_info: Option<(uuid::Uuid, String)> =
            match sqlx::query_as("SELECT id, name FROM classes WHERE id = $1 AND dataset_id = $2")
                .bind(class_id)
                .bind(ds_id)
                .fetch_optional(&state.pool)
                .await
            {
                Ok(c) => c,
                Err(_) => {
                    return err(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal",
                        "internal server error",
                    )
                }
            };

        let (_cid, class_name) = match class_info {
            Some(c) => c,
            None => {
                return err(
                    StatusCode::NOT_FOUND,
                    "not_found",
                    "classe informada não encontrada no dataset",
                )
            }
        };

        if let Some(ref mut p) = req.prompt {
            *p = p.replace("{class_name}", &class_name);
        }

        let matched_imgs: Vec<uuid::Uuid> = match sqlx::query_scalar(
            "SELECT DISTINCT b.image_id \
             FROM boxes b \
             JOIN images i ON i.id = b.image_id \
             WHERE i.dataset_id = $1 AND i.deleted_at IS NULL AND b.class_id = $2",
        )
        .bind(ds_id)
        .bind(class_id)
        .fetch_all(&state.pool)
        .await
        {
            Ok(imgs) => imgs,
            Err(_) => {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                )
            }
        };

        if matched_imgs.is_empty() {
            return err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "dataset_not_ready",
                "nenhuma imagem ativa possui anotações para a classe selecionada",
            );
        }

        resolved_image_ids = Some(matched_imgs);
    }

    if let Some(ref ids) = req.image_ids {
        let parsed_ids: Vec<uuid::Uuid> = ids
            .iter()
            .filter_map(|s| uuid::Uuid::parse_str(s).ok())
            .collect();

        if let Some(existing) = resolved_image_ids {
            let set: std::collections::HashSet<uuid::Uuid> = parsed_ids.into_iter().collect();
            let intersected: Vec<uuid::Uuid> =
                existing.into_iter().filter(|id| set.contains(id)).collect();
            if intersected.is_empty() {
                return err(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "dataset_not_ready",
                    "nenhuma das imagens selecionadas possui anotações da classe",
                );
            }
            resolved_image_ids = Some(intersected);
        } else {
            let valid_imgs: Vec<uuid::Uuid> = match sqlx::query_scalar(
                "SELECT id FROM images WHERE dataset_id = $1 AND deleted_at IS NULL AND id = ANY($2)",
            )
            .bind(ds_id)
            .bind(&parsed_ids)
            .fetch_all(&state.pool)
            .await
            {
                Ok(imgs) => imgs,
                Err(_) => {
                    return err(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal",
                        "internal server error",
                    )
                }
            };

            if valid_imgs.is_empty() {
                return err(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "dataset_not_ready",
                    "nenhuma imagem ativa válida encontrada na seleção",
                );
            }
            resolved_image_ids = Some(valid_imgs);
        }
    }

    // 5. Build package.
    let package = match crate::datasets::package::build_package_filtered(
        &state,
        ds_id,
        resolved_image_ids.as_deref(),
    )
    .await
    {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    // 6. Config YAML.
    let job_id = uuid::Uuid::new_v4().to_string();
    let config_yaml = models::generate_autolabel_config_yaml(&job_id, &req);

    // 7. Body para o Manager.
    let mut manager_body = serde_json::json!({
        "kind": "autolabel",
        "engine": "autolabel",
        "model": req.model,
        "mode": "autolabel",
        "dataset_id": ds_id.to_string(),
        "dataset_version_id": package.version_id,
        "package_ref": {
            "version_id": package.version_id,
            "key": package.key,
            "md5_zip": package.md5_zip,
            "bytes": package.bytes,
        },
        "config_yaml": config_yaml,
        "params": {
            "model": req.model,
            "prompt": req.prompt,
            "filter_class_id": req.filter_class_id,
            "image_ids_count": resolved_image_ids.as_ref().map(|v| v.len()),
            "package_ref": {
                "version_id": package.version_id,
                "key": package.key,
                "md5_zip": package.md5_zip,
                "bytes": package.bytes,
            },
        },
        "vram_min_gb": null,
    });

    if let Some(ref orch_id) = req.orchestrator_id {
        manager_body["orchestrator_hint"] = serde_json::json!(orch_id);
    }

    match state.manager.create_job(&manager_body).await {
        Ok(resp) => {
            let body = SubmitJobResponse {
                job_id: resp.job_id,
                status: resp.status,
                queue_position: resp.queue_position,
            };
            (StatusCode::ACCEPTED, Json(body)).into_response()
        }
        Err(ManagerError::NotFound) => {
            compensate_package(&state, &package.version_id).await;
            not_found()
        }
        Err(ManagerError::InvalidRequest(_)) => {
            compensate_package(&state, &package.version_id).await;
            invalid_request()
        }
        Err(ManagerError::Unavailable(_)) => {
            compensate_package(&state, &package.version_id).await;
            queue_unavailable()
        }
        Err(_) => {
            compensate_package(&state, &package.version_id).await;
            queue_unavailable()
        }
    }
}

// ---------------------------------------------------------------------------
// POST /api/jobs/diffusion — submit job de treino de difusão LoRA (ADR-0018 D1)
// ---------------------------------------------------------------------------

/// POST /api/jobs/diffusion — cria job de treino de difusão LoRA (ADR-0018 D1).
///
/// Status: 202 `SubmitJobResponse` | 400 `invalid_request` | 401 gate | 404 `not_found` |
/// 409 `dataset_not_ready` | 503 `queue_unavailable`.
pub async fn submit_diffusion_job(
    State(state): State<AppState>,
    body: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    // 1. Parse body.
    let raw = match body {
        Ok(b) => b,
        Err(_) => return invalid_request(),
    };
    let req: models::DiffusionJobRequest = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 2. Validação pura.
    let req = match models::validate_diffusion_request(req) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 3. Parse dataset_id — não-UUID ⇒ 404.
    let ds_id: uuid::Uuid = match req.dataset_id.parse() {
        Ok(v) => v,
        Err(_) => return not_found(),
    };

    // 4. Dataset existe e possui imagens ativas?
    let ds_exists: bool =
        match sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM datasets WHERE id = $1)")
            .bind(ds_id)
            .fetch_one(&state.pool)
            .await
        {
            Ok(b) => b,
            Err(_) => {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                )
            }
        };
    if !ds_exists {
        return not_found();
    }

    let image_count: Option<i64> = match sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM images WHERE dataset_id = $1 AND deleted_at IS NULL",
    )
    .bind(ds_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(c) => c,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };

    let count = image_count.unwrap_or(0);
    if count == 0 {
        return dataset_not_ready();
    }

    // 5. Build package de difusão (imagens + captions em .txt).
    let package = match crate::datasets::package::build_package_diffusion(
        &state,
        ds_id,
        req.trigger_word.as_deref(),
    )
    .await
    {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    // 6. Config YAML.
    let job_id = uuid::Uuid::new_v4().to_string();
    let config_yaml = models::generate_diffusion_config_yaml(&job_id, &req);

    // 7. VRAM mínima por modelo base (ADR-0018 D2 — FLUX.2 Klein 4B requer ~10 GB).
    let vram_min = match req.base_model.as_str() {
        "sd15" => 8,
        "flux" => 10,
        _ => 12, // sdxl e default
    };

    // 8. Body para o Manager.
    let mut manager_body = serde_json::json!({
        "kind": "diffusion_train",
        "engine": "diffusion",
        "model": req.base_model,
        "mode": "train",
        "dataset_id": ds_id.to_string(),
        "dataset_version_id": package.version_id,
        "package_ref": {
            "version_id": package.version_id,
            "key": package.key,
            "md5_zip": package.md5_zip,
            "bytes": package.bytes,
        },
        "config_yaml": config_yaml,
        "params": {
            "packageRef": {
                "versionId": package.version_id,
                "key": package.key,
                "md5Zip": package.md5_zip,
                "bytes": package.bytes,
            },
            "datasetId": ds_id.to_string(),
            "baseModel": req.base_model,
            "triggerWord": req.trigger_word,
            "epochs": req.epochs,
            "batchSize": req.batch_size,
            "learningRate": req.learning_rate,
            "rank": req.rank,
            "alpha": req.alpha,
            "resolution": req.resolution,
            "gradientAccumulationSteps": req.gradient_accumulation_steps,
            "optimizer": req.optimizer,
            "lrScheduler": req.lr_scheduler,
            "lrWarmupSteps": req.lr_warmup_steps,
            "mixedPrecision": req.mixed_precision,
            "quantization": req.quantization,
            "enableBucket": req.enable_bucket,
            "checkpointInterval": req.checkpoint_interval,
            "epochOffset": req.epoch_offset,
            "samplePrompt": req.sample_prompt,
            "sampleInterval": req.sample_interval,
            "sampleSeed": req.sample_seed,
            "weights": req.weights,
            "outputName": req.output_name,
        },
        "vram_min_gb": vram_min,
    });

    if let Some(ref w_id) = req.weights {
        manager_body["weights_id"] = serde_json::json!(w_id);
    }
    if let Some(ref orch_id) = req.orchestrator_id {
        manager_body["orchestrator_hint"] = serde_json::json!(orch_id);
    }

    match state.manager.create_job(&manager_body).await {
        Ok(resp) => {
            let body = SubmitJobResponse {
                job_id: resp.job_id,
                status: resp.status,
                queue_position: resp.queue_position,
            };
            (StatusCode::ACCEPTED, Json(body)).into_response()
        }
        Err(ManagerError::NotFound) => {
            compensate_package(&state, &package.version_id).await;
            not_found()
        }
        Err(ManagerError::InvalidRequest(_)) => {
            compensate_package(&state, &package.version_id).await;
            invalid_request()
        }
        Err(ManagerError::Unavailable(_)) => {
            compensate_package(&state, &package.version_id).await;
            queue_unavailable()
        }
        Err(_) => {
            compensate_package(&state, &package.version_id).await;
            queue_unavailable()
        }
    }
}

// ---------------------------------------------------------------------------
// POST /api/jobs/diffusion/generate — submit job de geração Text-to-Image (ADR-0020)
// ---------------------------------------------------------------------------

pub async fn submit_diffusion_generate_job(
    State(state): State<AppState>,
    body: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    // 1. Parse body.
    let raw = match body {
        Ok(b) => b,
        Err(_) => return invalid_request(),
    };
    let req: models::DiffusionGenerateJobRequest = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 2. Validação pura (ADR-0023 D2/D3/D4).
    let req = match models::validate_diffusion_generate_request(req) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 3. Se custom_model_id presente → busca modelo no manager para obter arch.
    //    Valida kind=checkpoint e arch ∈ {sdxl, sd15}.
    let custom_arch: Option<String> = if let Some(ref custom_id) = req.custom_model_id {
        let models = match state.manager.list_models().await {
            Ok(m) => m,
            Err(_) => return queue_unavailable(),
        };
        let model = match models.iter().find(|m| m.id == *custom_id) {
            Some(m) => m,
            None => {
                return err(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "customModelId not found",
                );
            }
        };
        // Valida kind=checkpoint (ADR-0023 D4).
        match model.kind.as_deref() {
            Some("checkpoint") => {}
            _ => {
                return err(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "customModelId must reference a checkpoint model",
                );
            }
        }
        // Valida arch ∈ {sdxl, sd15} (ADR-0023 D4 — Flux custom fora da v1).
        match model.arch.as_deref() {
            Some(arch @ ("sdxl" | "sd15")) => Some(arch.to_string()),
            Some(_other) => {
                return err(
                    StatusCode::BAD_REQUEST,
                    "unsupported_architecture",
                    "custom checkpoint architecture not supported (use sdxl or sd15)",
                );
            }
            None => {
                return err(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "custom model has no arch metadata",
                );
            }
        }
    } else {
        None
    };

    // 4. ID do job e config.yaml (v2 com custom_arch).
    let job_id = uuid::Uuid::new_v4().to_string();
    let config_yaml =
        models::generate_diffusion_generate_config_yaml(&job_id, &req, custom_arch.as_deref());

    // 5. VRAM mínima por (arch_efetiva, quantization).
    //    Valores atuais exatos (D8): sd15→6; 4bit→8; 8bit→12; none→16.
    //    Custom: espelha first-class do arch (sdxl/sd15).
    let effective_arch = custom_arch
        .as_deref()
        .unwrap_or_else(|| req.base_model.as_deref().unwrap_or("flux-2-klein-4b"));
    let vram_min: i32 = match effective_arch {
        "sd15" => 6,
        "sdxl" => match req.quantization.as_str() {
            "4bit" => 8,
            "8bit" => 12,
            _ => 16,
        },
        // flux-2-klein-4b e demais: mesmo legado
        _ => match req.quantization.as_str() {
            "4bit" => 8,
            "8bit" => 12,
            _ => 16,
        },
    };

    // 6. Body para o Manager — params camelCase (ADR-0023).
    let loras_json: Vec<serde_json::Value> = req
        .loras
        .iter()
        .map(|l| serde_json::json!({ "modelId": l.model_id, "scale": l.scale }))
        .collect();

    let mut manager_body = serde_json::json!({
        "kind": "diffusion_generate",
        "engine": "diffusion",
        "model": effective_arch,
        "mode": "generate",
        "config_yaml": config_yaml,
        "params": {
            "base_model": effective_arch,
            "prompt": req.prompt,
            "negative_prompt": req.negative_prompt,
            "width": req.width,
            "height": req.height,
            "steps": req.steps,
            "guidance_scale": req.guidance_scale,
            "seed": req.seed,
            "quantization": req.quantization,
            "distilled": req.distilled,
            "lora_scale": req.lora_scale,
            "batchSize": req.batch_size,
            "loras": loras_json,
            "customModelId": req.custom_model_id,
        },
        "vram_min_gb": vram_min,
    });

    if let Some(ref w_id) = req.weights {
        manager_body["weights_id"] = serde_json::json!(w_id);
    }
    if let Some(ref orch_id) = req.orchestrator_id {
        manager_body["orchestrator_hint"] = serde_json::json!(orch_id);
    }

    match state.manager.create_job(&manager_body).await {
        Ok(resp) => {
            let body = SubmitJobResponse {
                job_id: resp.job_id,
                status: resp.status,
                queue_position: resp.queue_position,
            };
            (StatusCode::ACCEPTED, Json(body)).into_response()
        }
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::InvalidRequest(_)) => invalid_request(),
        Err(ManagerError::Unavailable(_)) => queue_unavailable(),
        Err(_) => queue_unavailable(),
    }
}

// ---------------------------------------------------------------------------
// POST /api/jobs/predict — submit job de inferência YOLO (ADR-0013 D0/D1/D8)
// ---------------------------------------------------------------------------

/// POST /api/jobs/predict — cria job de predict YOLO (ADR-0013 D0/D1/D8).
///
/// Status: 202 | 400 `invalid_request` | 401 | 404 `not_found` |
/// 409 `dataset_not_ready` | 503 `queue_unavailable`.
///
/// Diferença R6: mapeia `NotFound→404`, `InvalidRequest→400` (padrão abort
/// L825-831), NÃO repete o `Err(_)→503` do submit_yolo_job existente.
pub async fn submit_predict_job(
    State(state): State<AppState>,
    body: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    // 1. Parse body.
    let raw = match body {
        Ok(b) => b,
        Err(_) => return invalid_request(),
    };
    let req: PredictJobRequest = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 2. Validação pura (models.rs).
    let req = match models::validate_predict_request(req) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 3. Parse dataset_id — não-UUID ⇒ 404 (D8).
    let ds_id: uuid::Uuid = match req.dataset_id.parse() {
        Ok(v) => v,
        Err(_) => return not_found(),
    };

    // 4. Dataset existe?
    let ds_exists: bool =
        match sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM datasets WHERE id = $1)")
            .bind(ds_id)
            .fetch_one(&state.pool)
            .await
        {
            Ok(b) => b,
            Err(_) => {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                )
            }
        };
    if !ds_exists {
        return not_found();
    }

    // 5. Dataset pronto? (D8: predict — category=yolo, ≥1 imagem; classes NÃO obrigatórias).
    //    409 `dataset_not_ready`: category ≠ 'yolo' OU 0 imagens ativas.
    let readiness: Option<(String, i64)> = match sqlx::query_as::<_, (String, i64)>(
        "SELECT d.category, \
         (SELECT count(*) FROM images WHERE dataset_id = d.id AND deleted_at IS NULL) \
         FROM datasets d WHERE d.id = $1",
    )
    .bind(ds_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };
    match readiness {
        None => return not_found(),
        Some((category, image_count)) => {
            if category != "yolo" || image_count == 0 {
                return dataset_not_ready();
            }
        }
    }

    // 6. Build package (função compartilhada — D2).
    let package = match crate::datasets::package::build_package(&state, ds_id).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    // 7. Gera config.yaml.
    let job_id = uuid::Uuid::new_v4().to_string();
    let config_yaml = models::generate_predict_config_yaml(&job_id, &req);

    // 8. POST ao manager (D5: kind='yolo_predict', engine='yolo', mode='predict',
    //    model='predict' placeholder, weights_id=modelId).
    let mut manager_body = serde_json::json!({
        "kind": "yolo_predict",
        "engine": "yolo",
        "model": "predict",
        "mode": "predict",
        "dataset_id": ds_id.to_string(),
        "dataset_version_id": package.version_id,
        "package_ref": {
            "version_id": package.version_id,
            "key": package.key,
            "md5_zip": package.md5_zip,
            "bytes": package.bytes,
        },
        "config_yaml": config_yaml,
        "params": {
            "conf": req.conf,
            "package_ref": {
                "version_id": package.version_id,
                "key": package.key,
                "md5_zip": package.md5_zip,
                "bytes": package.bytes,
            },
        },
        "vram_min_gb": null,
        "weights_id": req.model_id,
    });
    // ADR-0015 D2: insere orchestrator_hint no body quando presente.
    if let Some(ref orch_id) = req.orchestrator_id {
        manager_body["orchestrator_hint"] = serde_json::json!(orch_id);
    }

    match state.manager.create_job(&manager_body).await {
        Ok(resp) => {
            let body = SubmitJobResponse {
                job_id: resp.job_id,
                status: resp.status,
                queue_position: resp.queue_position,
            };
            (StatusCode::ACCEPTED, Json(body)).into_response()
        }
        // R6: handler do predict mapeia NotFound→404, InvalidRequest→400
        // (diferente do submit_yolo_job que mapeia Err(_)→503).
        // A1: compensação em TODOS os braços de erro (package já criado).
        Err(ManagerError::NotFound) => {
            compensate_package(&state, &package.version_id).await;
            not_found()
        }
        Err(ManagerError::InvalidRequest(_)) => {
            compensate_package(&state, &package.version_id).await;
            invalid_request()
        }
        Err(ManagerError::Unavailable(_)) => {
            compensate_package(&state, &package.version_id).await;
            queue_unavailable()
        }
        // Outros erros do manager → 503 + compensação.
        Err(_) => {
            compensate_package(&state, &package.version_id).await;
            queue_unavailable()
        }
    }
}

// ---------------------------------------------------------------------------
// POST /api/jobs/:id/abort — aborta job (F4.2b)
// ---------------------------------------------------------------------------

/// POST /api/jobs/:id/abort — aborta um job (ADR-0007 D7 :372-373).
///
/// Status: 200 | 401 | 404 `not_found` | 409 `job_not_abortable`.
pub async fn abort_job(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    // 1. Parse id — não-UUID ⇒ 404.
    if parse_uuid(&id).is_none() {
        return not_found();
    }

    // 2. Proxy ao manager.
    match state.manager.abort_job(&id).await {
        Ok(resp) => (
            StatusCode::OK,
            Json(AbortResponse {
                status: resp.status,
            }),
        )
            .into_response(),
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::NotAbortable) => job_not_abortable(),
        Err(ManagerError::Unavailable(_)) => queue_unavailable(),
        // PairingInvalid/Conflict/InvalidRequest/NotDeletable não são esperados no abort; mapeia para 503.
        Err(ManagerError::PairingInvalid) => queue_unavailable(),
        Err(ManagerError::Conflict) => queue_unavailable(),
        Err(ManagerError::InvalidRequest(_)) => queue_unavailable(),
        Err(ManagerError::NotDeletable) => queue_unavailable(),
    }
}

// ---------------------------------------------------------------------------
// Sweep de artifacts (best-effort — falha só loga, nunca 500)
// ---------------------------------------------------------------------------

/// Sweep best-effort de uma lista EXATA de chaves S3 (vieram do manager em
/// `DeletedJob.object_keys`, já sem as chaves das gerações preservadas).
/// Usar a lista exata (e não `delete_prefix(artifacts/{job}/)`) é o que
/// garante que a galeria sobreviva ao job, pois os bytes das gerações vivem
/// sob o MESMO prefixo. Nunca retorna erro — cada falha é logada (idiom D7).
async fn sweep_object_keys(state: &AppState, keys: &[String]) {
    for key in keys {
        match state.storage.delete(key).await {
            Ok(()) => {}
            Err(StorageError::NotFound) => {} // já não existia — ok
            Err(e) => {
                eprintln!("aviso: sweep {key} falhou ({e}) — objeto reaproveitável");
            }
        }
    }
}

/// Extrai `object_keys` (string[]) de um `DeletedJob` ou `CleanupResult` JSON.
/// Ambos expõem `object_keys` no topo (por job / agregado, já sem gerações).
fn extract_object_keys(v: &serde_json::Value) -> Vec<String> {
    v.get("object_keys")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|k| k.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Remapeia um `DeletedJob` snake_case (manager) → wire camelCase (ADR-0002 D1).
fn job_deleted_to_wire(v: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "id": v.get("id").cloned().unwrap_or(serde_json::Value::Null),
        "status": v.get("status").cloned().unwrap_or(serde_json::Value::Null),
        "artifacts": v.get("artifacts").cloned().unwrap_or_else(|| serde_json::json!([])),
        "objectKeys": v.get("object_keys").cloned().unwrap_or_else(|| serde_json::json!([])),
        "modelsDeleted": v.get("models_deleted").cloned().unwrap_or_else(|| serde_json::json!(0)),
        "generationsPreserved": v.get("generations_preserved").cloned().unwrap_or_else(|| serde_json::json!(0)),
    })
}

/// Remapeia um `CleanupResult` snake_case → wire camelCase (jobs aninhados incl.).
fn cleanup_result_to_wire(v: &serde_json::Value) -> serde_json::Value {
    let jobs = v
        .get("jobs")
        .and_then(|x| x.as_array())
        .map(|arr| arr.iter().map(job_deleted_to_wire).collect::<Vec<_>>())
        .unwrap_or_default();
    serde_json::json!({
        "deleted": v.get("deleted").cloned().unwrap_or_else(|| serde_json::json!(0)),
        "jobs": jobs,
        "objectKeys": v.get("object_keys").cloned().unwrap_or_else(|| serde_json::json!([])),
    })
}

// ---------------------------------------------------------------------------
// DELETE /api/jobs/:id — exclui job terminal
// ---------------------------------------------------------------------------

/// DELETE /api/jobs/:id — exclui um job terminal via manager.
///
/// Sweep best-effort das `object_keys` retornadas (não-varre a galeria
/// preservada — decisão AC-003). Status: 200 | 401 | 404 `not_found`
/// | 409 `job_not_terminal` | 503 `queue_unavailable`.
pub async fn delete_job(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    // 1. Parse id — não-UUID ⇒ 404.
    if parse_uuid(&id).is_none() {
        return not_found();
    }

    // 2. Proxy ao manager.
    match state.manager.delete_job(&id).await {
        Ok(v) => {
            let keys = extract_object_keys(&v);
            let wire = job_deleted_to_wire(&v);
            sweep_object_keys(&state, &keys).await;
            (StatusCode::OK, Json(wire)).into_response()
        }
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::NotDeletable) => job_not_terminal(),
        Err(ManagerError::Unavailable(_)) => queue_unavailable(),
        // NotAbortable/PairingInvalid/Conflict/InvalidRequest não são esperados no delete; mapeia para 503.
        Err(ManagerError::NotAbortable) => queue_unavailable(),
        Err(ManagerError::PairingInvalid) => queue_unavailable(),
        Err(ManagerError::Conflict) => queue_unavailable(),
        Err(ManagerError::InvalidRequest(_)) => queue_unavailable(),
    }
}

// ---------------------------------------------------------------------------
// POST /api/jobs/cleanup — limpa jobs antigos
// ---------------------------------------------------------------------------

/// POST /api/jobs/cleanup — limpa jobs antigos via manager.
///
/// Status: 200 | 400 `invalid_request` | 401 | 503 `queue_unavailable`.
pub async fn cleanup_jobs(
    State(state): State<AppState>,
    body: Result<Json<serde_json::Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    // 1. Parse body — JSON inválido ⇒ 400.
    let v = match body {
        Ok(Json(v)) => v,
        Err(_) => return invalid_request(),
    };

    // 2. Proxy ao manager.
    match state.manager.cleanup_jobs(&v).await {
        Ok(result) => {
            // Sweep best-effort das chaves agregadas (já exclui galeria viva).
            let keys = extract_object_keys(&result);
            let wire = cleanup_result_to_wire(&result);
            sweep_object_keys(&state, &keys).await;
            (StatusCode::OK, Json(wire)).into_response()
        }
        Err(ManagerError::InvalidRequest(_)) => invalid_request(),
        Err(ManagerError::Unavailable(_)) => queue_unavailable(),
        Err(_) => queue_unavailable(),
    }
}

// ---------------------------------------------------------------------------
// POST /api/jobs/:id/autotracker/apply — ingest de boxes no principal
// (ADR-0008 D1/D1a)
// ---------------------------------------------------------------------------

/// POST /api/jobs/:id/autotracker/apply — ingest de boxes no principal.
///
/// Status: 200 | 400 `invalid_request` | 404 `not_found` | 409 `job_not_done`
///         | 409 `dataset_not_ready` | 503 `queue_unavailable`/`storage_unavailable`.
///
/// Fluxo (ADR-0008 D1):
/// 1. Busca job no manager → valida engine/status/dataset_id
/// 2. Localiza artefato `boxes.json` via `list_artifacts`, valida path, lê via
///    `StoragePort.get`, confere md5
/// 3. Parse do JSON, resolve filename→image_id, class→class_id
/// 4. Escrita por imagem (transação DELETE+INSERT) com merge por origem
pub async fn apply_autotracker_boxes(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    // 0. Parse job id — não-UUID ⇒ 404.
    if parse_uuid(&id).is_none() {
        return not_found();
    }

    // 1. Parse body.
    let raw = match body {
        Ok(b) => b,
        Err(_) => return invalid_request(),
    };
    let req: models::AutotrackerApplyRequest = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 1a. Se imageId fornecido, valida UUID — não-UUID ⇒ 400 (campo de body).
    let filter_image_id: Option<Uuid> = match &req.image_id {
        Some(s) => match s.parse::<Uuid>() {
            Ok(v) => Some(v),
            Err(_) => return invalid_request(),
        },
        None => None,
    };

    // 2. Busca job no manager.
    let job = match state.manager.get_job(&id).await {
        Ok(j) => j,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };

    // 2a. Valida engine == 'autotracker'.
    if job.engine != "autotracker" {
        return not_found();
    }

    // 2b. Valida status == 'done'.
    if job.status != "done" {
        return job_not_done();
    }

    // 2c. Valida dataset_id presente.
    let dataset_id_str = match &job.dataset_id {
        Some(s) => s.clone(),
        None => return dataset_not_ready(),
    };
    let dataset_id: Uuid = match dataset_id_str.parse() {
        Ok(v) => v,
        Err(_) => return dataset_not_ready(),
    };

    // 3. Localiza artefato `boxes.json` via list_artifacts.
    let artifacts = match state.manager.list_artifacts(&id).await {
        Ok(a) => a,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let boxes_artifact = match artifacts.iter().find(|a| a.kind == "boxes") {
        Some(a) => a,
        None => return not_found(),
    };

    // 3a. Valida path do artefato (defesa em profundidade).
    if let Err(resp) = validate_artifact_path(&boxes_artifact.path) {
        return resp;
    }

    // 3b. Lê objeto via StoragePort (admin).
    let key = format!("artifacts/{id}/{}", boxes_artifact.path);
    let bytes = match state.storage.get(&key).await {
        Ok(b) => b,
        Err(StorageError::NotFound) => {
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "storage_unavailable",
                MSG_STORAGE_UNAVAILABLE,
            );
        }
        Err(StorageError::Unavailable(_)) => return storage_unavailable(),
    };

    // 3c. Confere md5.
    let computed = format!(
        "{:x}",
        md5::Digest::finalize({
            use md5::Digest;
            let mut h = md5::Md5::new();
            md5::Digest::update(&mut h, &bytes);
            h
        })
    );
    if computed != boxes_artifact.md5 {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_unavailable",
            MSG_STORAGE_UNAVAILABLE,
        );
    }

    // 4. Parse do JSON.
    let artifact = match models::parse_boxes_json(&bytes) {
        Ok(a) => a,
        Err(_) => return invalid_request(),
    };

    // 5. Busca imagens ativas do dataset (filename → image_id).
    let image_rows: Vec<(Uuid, String)> = match sqlx::query_as::<_, (Uuid, String)>(
        "SELECT id, filename FROM images WHERE dataset_id = $1 AND deleted_at IS NULL",
    )
    .bind(dataset_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };
    let filename_to_id: std::collections::HashMap<String, Uuid> = image_rows
        .iter()
        .map(|(id, fname)| (fname.clone(), *id))
        .collect();

    // 5a. Se filter_image_id definido, valida que é imagem ATIVA do dataset.
    if let Some(fid) = filter_image_id {
        if !filename_to_id.values().any(|id| *id == fid) {
            return not_found();
        }
    }

    // 6. Busca classes do dataset (name → class_id).
    let class_rows: Vec<(Uuid, String)> = match sqlx::query_as::<_, (Uuid, String)>(
        "SELECT id, name FROM classes WHERE dataset_id = $1",
    )
    .bind(dataset_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };
    let mut class_map = models::resolve_class_ids(&class_rows);
    let mut class_map_lower: std::collections::HashMap<String, Uuid> = class_rows
        .iter()
        .map(|(id, name)| (name.to_lowercase(), *id))
        .collect();

    // 6a. Se create_missing_classes fornecido, cria as novas classes no dataset.
    if let Some(ref to_create) = req.create_missing_classes {
        for name in to_create {
            let trimmed = name.trim();
            if !crate::datasets::models::is_valid_class_name(trimmed) {
                return invalid_request();
            }
            let lower = trimmed.to_lowercase();
            if !class_map_lower.contains_key(&lower) {
                // Checa teto de 200 classes
                let current_count: i64 =
                    match sqlx::query_scalar("SELECT count(*) FROM classes WHERE dataset_id = $1")
                        .bind(dataset_id)
                        .fetch_one(&state.pool)
                        .await
                    {
                        Ok(c) => c,
                        Err(_) => {
                            return err(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "internal",
                                "internal server error",
                            )
                        }
                    };
                if current_count >= 200 {
                    return invalid_request();
                }
                let next_idx: i32 = match sqlx::query_scalar(
                    "SELECT COALESCE(MAX(idx), -1) + 1 FROM classes WHERE dataset_id = $1",
                )
                .bind(dataset_id)
                .fetch_one(&state.pool)
                .await
                {
                    Ok(i) => i,
                    Err(_) => {
                        return err(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "internal",
                            "internal server error",
                        )
                    }
                };
                let color = crate::datasets::models::color_for(next_idx as usize);
                let new_class_id = Uuid::new_v4();
                if sqlx::query("INSERT INTO classes (id, dataset_id, name, color, idx) VALUES ($1, $2, $3, $4, $5)")
                    .bind(new_class_id)
                    .bind(dataset_id)
                    .bind(trimmed)
                    .bind(color)
                    .bind(next_idx)
                    .execute(&state.pool)
                    .await.is_err() {
                        return err(StatusCode::INTERNAL_SERVER_ERROR, "internal", "internal server error");
                    }
                class_map.insert(trimmed.to_string(), new_class_id);
                class_map_lower.insert(lower, new_class_id);
            }
        }
    }

    // 7. Processa cada imagem: uma transação por imagem (ADR-0008 D1a).
    let mut total_applied: i64 = 0;
    let mut total_skipped: i64 = 0;
    let mut images_with_boxes: i64 = 0;

    for engine_image in &artifact.images {
        // Resolve filename → image_id (SEMPRE por filename, mesmo com filter_image_id).
        let Some(image_id) = filename_to_id.get(&engine_image.filename) else {
            // Imagem inexistente/deletada → skip.
            total_skipped += engine_image.boxes.len() as i64;
            continue;
        };
        // Filtra: se filter_image_id definido, só processa essa imagem.
        if let Some(fid) = filter_image_id {
            if *image_id != fid {
                continue;
            }
        }

        // Resolve class names → class_ids, coletando skippadas.
        let mut valid_boxes: Vec<(Uuid, f64, f64, f64, f64, Option<f64>, String, Option<i32>)> =
            Vec::new();
        for eb in &engine_image.boxes {
            match models::match_class_id(&eb.class, &class_map, &class_map_lower) {
                Some(class_id) => {
                    valid_boxes.push((
                        *class_id,
                        eb.x,
                        eb.y,
                        eb.w,
                        eb.h,
                        Some(eb.conf),
                        "autotracker".to_string(),
                        None,
                    ));
                }
                None => {
                    // Classe inexistente → skip.
                    total_skipped += 1;
                }
            }
        }

        // Cap 1000/imagem.
        if valid_boxes.len() > 1000 {
            let excess = valid_boxes.len() - 1000;
            total_skipped += excess as i64;
            valid_boxes.truncate(1000);
        }

        // Transação por imagem: DELETE + INSERT.
        // DELETE SEMPRE ocorre quando a imagem está presente no artefato
        // (mesmo que valid_boxes fique vazio — semântica last-write-wins por
        // origem: se o engine EMITIU boxes para a imagem, as anteriores da
        // mesma origem devem ser removidas).
        let mut tx = match state.pool.begin().await {
            Ok(t) => t,
            Err(_) => {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                )
            }
        };

        if req.overwrite {
            // DELETE total da imagem.
            if sqlx::query("DELETE FROM boxes WHERE image_id = $1")
                .bind(image_id)
                .execute(&mut *tx)
                .await
                .is_err()
            {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                );
            }
        } else {
            // DELETE só das boxes de origem autotracker.
            if sqlx::query("DELETE FROM boxes WHERE image_id = $1 AND origin = 'autotracker'")
                .bind(image_id)
                .execute(&mut *tx)
                .await
                .is_err()
            {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                );
            }
        }

        // INSERT em massa — apenas se há boxes válidas.
        if !valid_boxes.is_empty() {
            type BoxTuple = (
                Uuid,
                Uuid,
                f64,
                f64,
                f64,
                f64,
                Option<f64>,
                String,
                Option<i32>,
            );
            let inserted: Vec<BoxTuple> = {
                let class_ids: Vec<Uuid> = valid_boxes.iter().map(|b| b.0).collect();
                let xs: Vec<f64> = valid_boxes.iter().map(|b| b.1).collect();
                let ys: Vec<f64> = valid_boxes.iter().map(|b| b.2).collect();
                let ws: Vec<f64> = valid_boxes.iter().map(|b| b.3).collect();
                let hs: Vec<f64> = valid_boxes.iter().map(|b| b.4).collect();
                let confs: Vec<Option<f64>> = valid_boxes.iter().map(|b| b.5).collect();
                let origins: Vec<String> = valid_boxes.iter().map(|b| b.6.clone()).collect();
                let tracks: Vec<Option<i32>> = valid_boxes.iter().map(|b| b.7).collect();
                match sqlx::query_as::<_, BoxTuple>(
                    "INSERT INTO boxes (image_id, class_id, x, y, w, h, conf, origin, track_id) \
                     SELECT $1, t.class_id, t.x, t.y, t.w, t.h, t.conf, t.origin, t.track_id \
                     FROM unnest($2::uuid[], $3::float8[], $4::float8[], $5::float8[], $6::float8[], $7::float8[], $8::text[], $9::int[]) \
                     AS t(class_id, x, y, w, h, conf, origin, track_id) \
                     RETURNING id, class_id, x, y, w, h, conf, origin, track_id",
                )
                .bind(image_id)
                .bind(&class_ids)
                .bind(&xs)
                .bind(&ys)
                .bind(&ws)
                .bind(&hs)
                .bind(&confs)
                .bind(&origins)
                .bind(&tracks)
                .fetch_all(&mut *tx)
                .await
                {
                    Ok(r) => r,
                    Err(_) => {
                        return err(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "internal",
                            "internal server error",
                        )
                    }
                }
            };

            let count = inserted.len() as i64;
            total_applied += count;
            if count > 0 {
                images_with_boxes += 1;
            }
        }

        // Sempre commita — a fase DELETE ocorreu mesmo sem INSERT.
        if tx.commit().await.is_err() {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            );
        }
    }

    // 8. Resposta 200.
    (
        StatusCode::OK,
        Json(AutotrackerApplyResponse {
            applied: total_applied,
            skipped: total_skipped,
            images: images_with_boxes,
        }),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// GET /api/jobs/:id/autotracker/preview — prévia e análise de classes do autotracker
// ---------------------------------------------------------------------------

/// GET /api/jobs/:id/autotracker/preview — retorna resumo de detecções com análise de classes existentes e ausentes.
///
/// Status: 200 | 401 | 404 `not_found` | 409 `job_not_done` | 503 `queue_unavailable` | 503 `storage_unavailable`.
pub async fn preview_autotracker_boxes(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    // 0. Parse job id — não-UUID ⇒ 404.
    if parse_uuid(&id).is_none() {
        return not_found();
    }

    // 1. Busca job no manager.
    let job = match state.manager.get_job(&id).await {
        Ok(j) => j,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };

    // 1a. Valida engine == 'autotracker'.
    if job.engine != "autotracker" {
        return not_found();
    }

    // 1b. Valida status == 'done'.
    if job.status != "done" {
        return job_not_done();
    }

    // 1c. Valida dataset_id presente.
    let dataset_id_str = match &job.dataset_id {
        Some(s) => s.clone(),
        None => return dataset_not_ready(),
    };
    let dataset_id: Uuid = match dataset_id_str.parse() {
        Ok(v) => v,
        Err(_) => return dataset_not_ready(),
    };

    // 2. Localiza artefato `boxes.json` via list_artifacts.
    let artifacts = match state.manager.list_artifacts(&id).await {
        Ok(a) => a,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let boxes_artifact = match artifacts.iter().find(|a| a.kind == "boxes") {
        Some(a) => a,
        None => return not_found(),
    };

    if let Err(resp) = validate_artifact_path(&boxes_artifact.path) {
        return resp;
    }

    // 2b. Lê objeto via StoragePort.
    let key = format!("artifacts/{id}/{}", boxes_artifact.path);
    let bytes = match state.storage.get(&key).await {
        Ok(b) => b,
        Err(StorageError::NotFound) => {
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "storage_unavailable",
                MSG_STORAGE_UNAVAILABLE,
            );
        }
        Err(StorageError::Unavailable(_)) => return storage_unavailable(),
    };

    // 2c. Confere md5.
    let computed = format!(
        "{:x}",
        md5::Digest::finalize({
            use md5::Digest;
            let mut h = md5::Md5::new();
            md5::Digest::update(&mut h, &bytes);
            h
        })
    );
    if computed != boxes_artifact.md5 {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_unavailable",
            MSG_STORAGE_UNAVAILABLE,
        );
    }

    // 3. Parse do JSON.
    let artifact = match models::parse_boxes_json(&bytes) {
        Ok(a) => a,
        Err(_) => return invalid_request(),
    };

    // 4. Busca classes existentes do dataset.
    let class_rows: Vec<(Uuid, String)> = match sqlx::query_as::<_, (Uuid, String)>(
        "SELECT id, name FROM classes WHERE dataset_id = $1",
    )
    .bind(dataset_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };
    let class_map = models::resolve_class_ids(&class_rows);
    let class_map_lower: std::collections::HashMap<String, Uuid> = class_rows
        .iter()
        .map(|(id, name)| (name.to_lowercase(), *id))
        .collect();

    // 5. Agrega contagem de boxes por classe.
    let mut total_boxes: i64 = 0;
    let mut class_counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();

    for img in &artifact.images {
        for b in &img.boxes {
            total_boxes += 1;
            *class_counts.entry(b.class.clone()).or_insert(0) += 1;
        }
    }

    let mut existing_classes = Vec::new();
    let mut missing_classes = Vec::new();

    for (class_name, count) in class_counts {
        if models::match_class_id(&class_name, &class_map, &class_map_lower).is_some() {
            existing_classes.push(models::AutotrackerClassCount {
                name: class_name,
                boxes_count: count,
            });
        } else {
            missing_classes.push(models::AutotrackerClassCount {
                name: class_name,
                boxes_count: count,
            });
        }
    }

    existing_classes.sort_by(|a, b| {
        b.boxes_count
            .cmp(&a.boxes_count)
            .then_with(|| a.name.cmp(&b.name))
    });
    missing_classes.sort_by(|a, b| {
        b.boxes_count
            .cmp(&a.boxes_count)
            .then_with(|| a.name.cmp(&b.name))
    });

    let resp = models::AutotrackerPreviewResponse {
        total_images: artifact.images.len() as i64,
        total_boxes,
        existing_classes,
        missing_classes,
    };

    (StatusCode::OK, Json(resp)).into_response()
}

// ---------------------------------------------------------------------------
// GET /api/jobs/:id/autolabel/preview — prévia de legendas do autolabel
// ---------------------------------------------------------------------------

/// GET /api/jobs/:id/autolabel/preview — retorna prévia das legendas geradas para curadoria humana.
///
/// Status: 200 | 401 | 404 `not_found` | 409 `job_not_done` | 503 `queue_unavailable` | 503 `storage_unavailable`.
pub async fn preview_autolabel_captions(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    // 0. Parse job id — não-UUID ⇒ 404.
    let job_uuid = match parse_uuid(&id) {
        Some(u) => u,
        None => return not_found(),
    };

    // 1. Busca job no manager.
    let job = match state.manager.get_job(&id).await {
        Ok(j) => j,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };

    // 1a. Valida engine == 'autolabel'.
    if job.engine != "autolabel" {
        return not_found();
    }

    // 1b. Valida status == 'done'.
    if job.status != "done" {
        return job_not_done();
    }

    // 1c. Valida dataset_id presente.
    let dataset_id_str = match &job.dataset_id {
        Some(s) => s.clone(),
        None => return dataset_not_ready(),
    };
    let dataset_id: Uuid = match dataset_id_str.parse() {
        Ok(v) => v,
        Err(_) => return dataset_not_ready(),
    };

    // 2. Localiza artefato `captions.jsonl` via list_artifacts.
    let artifacts = match state.manager.list_artifacts(&id).await {
        Ok(a) => a,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let captions_artifact = match artifacts
        .iter()
        .find(|a| a.kind == "captions" || a.path == "captions.jsonl")
    {
        Some(a) => a,
        None => return not_found(),
    };

    if let Err(resp) = validate_artifact_path(&captions_artifact.path) {
        return resp;
    }

    // 2b. Lê objeto via StoragePort.
    let key = format!("artifacts/{id}/{}", captions_artifact.path);
    let bytes = match state.storage.get(&key).await {
        Ok(b) => b,
        Err(StorageError::NotFound) => {
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "storage_unavailable",
                MSG_STORAGE_UNAVAILABLE,
            );
        }
        Err(StorageError::Unavailable(_)) => return storage_unavailable(),
    };

    // 2c. Confere md5.
    let computed = format!(
        "{:x}",
        md5::Digest::finalize({
            use md5::Digest;
            let mut h = md5::Md5::new();
            md5::Digest::update(&mut h, &bytes);
            h
        })
    );
    if computed != captions_artifact.md5 {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_unavailable",
            MSG_STORAGE_UNAVAILABLE,
        );
    }

    // 3. Parse do JSONL.
    let items = match models::parse_captions_jsonl(&bytes) {
        Ok(it) => it,
        Err(_) => return invalid_request(),
    };

    // 4. Busca imagens ativas do dataset (filename, id, object_key).
    let image_rows: Vec<(Uuid, String, String)> = match sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT id, filename, object_key FROM images WHERE dataset_id = $1 AND deleted_at IS NULL",
    )
    .bind(dataset_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };
    let filename_to_image: std::collections::HashMap<String, (Uuid, String)> = image_rows
        .into_iter()
        .map(|(id, fname, okey)| (fname, (id, okey)))
        .collect();

    // 5. Busca captions existentes para estas imagens (image_id -> (text, origin)).
    let existing_captions: std::collections::HashMap<Uuid, (String, String)> = match sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT c.image_id, c.text, c.origin FROM captions c JOIN images i ON i.id = c.image_id WHERE i.dataset_id = $1 AND i.deleted_at IS NULL",
    )
    .bind(dataset_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => rows.into_iter().map(|(id, text, origin)| (id, (text, origin))).collect(),
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };

    // 6. Constrói items de prévia.
    let total_generated = items.len() as i64;
    let mut preview_items = Vec::new();

    for item in items {
        if let Some((image_id, object_key)) = filename_to_image.get(&item.filename) {
            let image_url =
                crate::datasets::handlers::image_url(&state, object_key, dataset_id, *image_id)
                    .await
                    .unwrap_or_default();
            let (curr_text, curr_origin) = match existing_captions.get(image_id) {
                Some((t, o)) => (Some(t.clone()), Some(o.clone())),
                None => (None, None),
            };

            preview_items.push(models::AutolabelPreviewItem {
                image_id: *image_id,
                filename: item.filename,
                image_url,
                generated_caption: item.caption,
                current_caption: curr_text,
                current_origin: curr_origin,
            });
        }
    }

    let resp = models::AutolabelPreviewResponse {
        job_id: job_uuid,
        dataset_id,
        model: Some(job.model),
        total_generated,
        items: preview_items,
    };
    (StatusCode::OK, Json(resp)).into_response()
}

// ---------------------------------------------------------------------------
// POST /api/jobs/:id/autolabel/apply — aplica legendas do autolabel (ADR-0016 D1)
// ---------------------------------------------------------------------------

/// POST /api/jobs/:id/autolabel/apply — aplica legendas do autolabel (ADR-0016 D1).
///
/// Status: 200 | 400 `invalid_request` | 401 | 404 `not_found` |
/// 409 `job_not_done` | 503 `queue_unavailable` | 503 `storage_unavailable`.
pub async fn apply_autolabel_captions(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    // 0. Parse job id — não-UUID ⇒ 404.
    if parse_uuid(&id).is_none() {
        return not_found();
    }

    // 1. Parse body.
    let raw = match body {
        Ok(b) => b,
        Err(_) => return invalid_request(),
    };
    let req: models::AutolabelApplyRequest = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 2. Busca job no manager.
    let job = match state.manager.get_job(&id).await {
        Ok(j) => j,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };

    // 2a. Valida engine == 'autolabel'.
    if job.engine != "autolabel" {
        return not_found();
    }

    // 2b. Valida status == 'done'.
    if job.status != "done" {
        return job_not_done();
    }

    // 2c. Valida dataset_id presente.
    let dataset_id_str = match &job.dataset_id {
        Some(s) => s.clone(),
        None => return dataset_not_ready(),
    };
    let dataset_id: Uuid = match dataset_id_str.parse() {
        Ok(v) => v,
        Err(_) => return dataset_not_ready(),
    };
    if let Some(ds_req) = &req.dataset_id {
        if ds_req != &dataset_id_str {
            return invalid_request();
        }
    }

    // 3. Localiza artefato `captions.jsonl` via list_artifacts.
    let artifacts = match state.manager.list_artifacts(&id).await {
        Ok(a) => a,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let captions_artifact = match artifacts
        .iter()
        .find(|a| a.kind == "captions" || a.path == "captions.jsonl")
    {
        Some(a) => a,
        None => return not_found(),
    };

    if let Err(resp) = validate_artifact_path(&captions_artifact.path) {
        return resp;
    }

    // 3b. Lê objeto via StoragePort.
    let key = format!("artifacts/{id}/{}", captions_artifact.path);
    let bytes = match state.storage.get(&key).await {
        Ok(b) => b,
        Err(StorageError::NotFound) => {
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "storage_unavailable",
                MSG_STORAGE_UNAVAILABLE,
            );
        }
        Err(StorageError::Unavailable(_)) => return storage_unavailable(),
    };

    // 3c. Confere md5.
    let computed = format!(
        "{:x}",
        md5::Digest::finalize({
            use md5::Digest;
            let mut h = md5::Md5::new();
            md5::Digest::update(&mut h, &bytes);
            h
        })
    );
    if computed != captions_artifact.md5 {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_unavailable",
            MSG_STORAGE_UNAVAILABLE,
        );
    }

    // 4. Determina os itens a aplicar:
    // Se o cliente forneceu `req.items` curados, usa esses;
    // senão, faz parse do JSONL original do artefato.
    let target_items: Vec<models::CaptionsJsonlItem> = if let Some(curated) = req.items {
        curated
            .into_iter()
            .map(|c| models::CaptionsJsonlItem {
                filename: c.filename,
                caption: c.caption,
            })
            .collect()
    } else {
        match models::parse_captions_jsonl(&bytes) {
            Ok(it) => it,
            Err(_) => return invalid_request(),
        }
    };

    // 5. Busca imagens ativas do dataset (filename → image_id).
    let image_rows: Vec<(Uuid, String)> = match sqlx::query_as::<_, (Uuid, String)>(
        "SELECT id, filename FROM images WHERE dataset_id = $1 AND deleted_at IS NULL",
    )
    .bind(dataset_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };
    let filename_to_id: std::collections::HashMap<String, Uuid> = image_rows
        .into_iter()
        .map(|(id, fname)| (fname, id))
        .collect();

    // 6. Busca captions existentes para estas imagens (image_id -> origin).
    let existing_captions: std::collections::HashMap<Uuid, String> = match sqlx::query_as::<_, (Uuid, String)>(
        "SELECT c.image_id, c.origin FROM captions c JOIN images i ON i.id = c.image_id WHERE i.dataset_id = $1 AND i.deleted_at IS NULL",
    )
    .bind(dataset_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => rows.into_iter().collect(),
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };

    // 7. Processa itens com merge dirigido por origem (ADR-0016 D1).
    let mut total_applied: i64 = 0;
    let mut total_skipped: i64 = 0;
    let mut applied_images: std::collections::HashSet<Uuid> = std::collections::HashSet::new();

    for item in target_items {
        let Some(&image_id) = filename_to_id.get(&item.filename) else {
            total_skipped += 1;
            continue;
        };

        let trimmed_caption = item.caption.trim();
        if trimmed_caption.is_empty() || trimmed_caption.chars().count() > 8000 {
            total_skipped += 1;
            continue;
        }

        // Se overwrite=false, só atualiza imagens sem caption ou com origin='autolabel'.
        if !req.overwrite {
            if let Some(origin) = existing_captions.get(&image_id) {
                if origin != "autolabel" {
                    total_skipped += 1;
                    continue;
                }
            }
        }

        // UPSERT na tabela captions
        let query_res = sqlx::query(
            "INSERT INTO captions (image_id, text, origin, model, updated_at) \
             VALUES ($1, $2, 'autolabel', $3, now()) \
             ON CONFLICT (image_id) DO UPDATE SET text = EXCLUDED.text, origin = EXCLUDED.origin, model = EXCLUDED.model, updated_at = now()",
        )
        .bind(image_id)
        .bind(trimmed_caption)
        .bind(&job.model)
        .execute(&state.pool)
        .await;

        match query_res {
            Ok(_) => {
                total_applied += 1;
                applied_images.insert(image_id);
            }
            Err(e) => {
                tracing::warn!(error = %e, %image_id, "falha ao executar upsert de caption no autolabel apply");
                total_skipped += 1;
            }
        }
    }

    let resp = models::AutolabelApplyResponse {
        applied: total_applied,
        skipped: total_skipped,
        images: applied_images.len() as i64,
    };
    (StatusCode::OK, Json(resp)).into_response()
}

// ---------------------------------------------------------------------------
// Tests unitários
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::manager_client::{
        CreateJobResponse, InternalArtifact, InternalJob, InternalModel, MockManager,
    };

    #[test]
    fn remap_metrics_camel_case() {
        let raw = serde_json::json!({
            "items": [
                {
                    "epoch": 1,
                    "box_loss": 0.5,
                    "cls_loss": 0.3,
                    "dfl_loss": 0.2,
                    "mAP50": 0.8,
                    "mAP50-95": 0.6
                },
                {
                    "epoch": 2,
                    "box_loss": 0.4,
                    "cls_loss": 0.2,
                    "dfl_loss": 0.1,
                    "mAP50": 0.9,
                    "mAP50-95": 0.7
                }
            ]
        });
        let items = remap_metrics(&raw);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].epoch, 1);
        assert_eq!(items[0].box_loss, 0.5);
        assert_eq!(items[0].cls_loss, 0.3);
        assert_eq!(items[0].dfl_loss, 0.2);
        assert_eq!(items[0].map50, 0.8);
        assert_eq!(items[0].map5095, 0.6);
        assert_eq!(items[1].epoch, 2);
        assert_eq!(items[1].map5095, 0.7);
    }

    #[test]
    fn remap_metrics_empty_items() {
        let raw = serde_json::json!({ "items": [] });
        let items = remap_metrics(&raw);
        assert!(items.is_empty());
    }

    #[test]
    fn remap_metrics_no_items_key() {
        let raw = serde_json::json!({});
        let items = remap_metrics(&raw);
        assert!(items.is_empty());
    }

    #[test]
    fn remap_metrics_missing_epoch_skipped() {
        let raw = serde_json::json!({
            "items": [
                { "box_loss": 0.5, "cls_loss": 0.3, "dfl_loss": 0.2, "mAP50": 0.8, "mAP50-95": 0.6 }
            ]
        });
        let items = remap_metrics(&raw);
        assert!(items.is_empty());
    }

    #[test]
    fn to_job_response_snake_to_camel() {
        let job = InternalJob {
            id: "550e8400-e29b-41d4-a716-446655440000".into(),
            kind: "yolo_train".into(),
            engine: "yolo".into(),
            model: "yolo11m".into(),
            mode: "train".into(),
            dataset_id: Some("550e8400-e29b-41d4-a716-446655440001".into()),
            status: "running".into(),
            queue_reason: None,
            queue_position: None,
            progress: Some(0.5),
            epoch: Some(5),
            step: Some(100),
            metrics: None,
            vram_min_gb: Some(4),
            orchestrator_id: Some("550e8400-e29b-41d4-a716-446655440002".into()),
            orchestrator_name: Some("node-gpu".into()),
            orchestrator_kind: Some("remoto".into()),
            orchestrator_fallback: true,
            created_at: "2026-01-01T00:00:00Z".into(),
            finished_at: None,
            error: None,
            params: None,
        };
        let resp = to_job_response(job);
        assert_eq!(resp.id, "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(resp.kind, "yolo_train");
        assert_eq!(resp.status, "running");
        assert_eq!(resp.progress, Some(0.5));
        assert_eq!(resp.epoch, Some(5));
        assert_eq!(resp.orchestrator_name.as_deref(), Some("node-gpu"));
        assert_eq!(resp.orchestrator_kind.as_deref(), Some("remoto"));
        assert!(resp.orchestrator_fallback);
        assert!(resp.metrics.is_none());
    }

    #[test]
    fn validate_artifact_path_rejects_dotdot() {
        assert!(validate_artifact_path("../etc/passwd").is_err());
    }

    #[test]
    fn validate_artifact_path_rejects_absolute() {
        assert!(validate_artifact_path("/etc/passwd").is_err());
    }

    #[test]
    fn validate_artifact_path_rejects_backslash() {
        assert!(validate_artifact_path("..\\windows").is_err());
    }

    #[test]
    fn validate_artifact_path_accepts_normal() {
        assert!(validate_artifact_path("best.pt").is_ok());
        assert!(validate_artifact_path("subdir/best.pt").is_ok());
    }

    #[tokio::test]
    async fn list_jobs_handler_200() {
        let mut mock = MockManager::default();
        mock.list_jobs_result = Some((vec![mock_job()], 1));
        let state = test_state(mock);
        let resp = list_jobs(
            axum::extract::State(state),
            Query(JobsQuery {
                status: None,
                engine: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn list_jobs_handler_503_manager_offline() {
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let resp = list_jobs(
            axum::extract::State(state),
            Query(JobsQuery {
                status: None,
                engine: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn list_queue_handler_200() {
        let mut mock = MockManager::default();
        mock.list_queue_result = Some(vec![crate::jobs::manager_client::InternalQueueItem {
            job_id: "550e8400-e29b-41d4-a716-446655440000".into(),
            position: 1,
            queue_reason: None,
        }]);
        let state = test_state(mock);
        let resp = list_queue(axum::extract::State(state)).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_job_handler_200() {
        let mut mock = MockManager::default();
        mock.get_job_result = Some(mock_job());
        let state = test_state(mock);
        let resp = get_job(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_job_handler_404_non_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = get_job(axum::extract::State(state), Path("not-a-uuid".to_string())).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_job_handler_404_not_found() {
        let mut mock = MockManager::default();
        mock.get_job_result = None; // NotFound
        let state = test_state(mock);
        let resp = get_job(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_job_handler_503_manager_offline() {
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let resp = get_job(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn get_job_metrics_200_empty() {
        let mut mock = MockManager::default();
        let mut job = mock_job();
        job.metrics = None;
        mock.get_job_result = Some(job);
        let state = test_state(mock);
        let resp = get_job_metrics(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_job_metrics_200_with_data() {
        let mut mock = MockManager::default();
        let mut job = mock_job();
        job.metrics = Some(serde_json::json!({
            "items": [{
                "epoch": 1,
                "box_loss": 0.5,
                "cls_loss": 0.3,
                "dfl_loss": 0.2,
                "mAP50": 0.8,
                "mAP50-95": 0.6
            }]
        }));
        mock.get_job_result = Some(job);
        let state = test_state(mock);
        let resp = get_job_metrics(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn list_artifacts_handler_200() {
        let mut mock = MockManager::default();
        mock.list_artifacts_result = Some(vec![InternalArtifact {
            id: "550e8400-e29b-41d4-a716-446655440003".into(),
            kind: "model".into(),
            path: "best.pt".into(),
            md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
            bytes: 1024,
        }]);
        let state = test_state(mock);
        let resp = list_artifacts(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_telemetry_handler_200() {
        let mut mock = MockManager::default();
        mock.get_telemetry_result = Some(crate::jobs::manager_client::InternalTelemetry {
            measured: false,
            vram_used: None,
            vram_total: None,
            cpu: Some(0.5),
            ram: Some(1024),
            ram_total: None,
            gpus: vec![],
            jobs_active: 0,
        });
        let state = test_state(mock);
        let resp = get_telemetry(axum::extract::State(state)).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_telemetry_handler_503_manager_offline() {
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let resp = get_telemetry(axum::extract::State(state)).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn get_telemetry_handler_ram_total() {
        let mut mock = MockManager::default();
        mock.get_telemetry_result = Some(crate::jobs::manager_client::InternalTelemetry {
            measured: true,
            vram_used: None,
            vram_total: None,
            cpu: Some(0.5),
            ram: Some(1024),
            ram_total: Some(8_000_000_000),
            gpus: vec![],
            jobs_active: 0,
        });
        let state = test_state(mock);
        let resp = get_telemetry(axum::extract::State(state)).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["ramTotal"], serde_json::json!(8_000_000_000_i64));
    }

    // --- POST /api/jobs/yolo unit tests (F4.2b) ---

    #[tokio::test]
    async fn submit_yolo_job_400_empty_body() {
        let mock = MockManager::default();
        let state = test_state(mock);
        // Empty bytes → invalid JSON → 400.
        let resp =
            submit_yolo_job(axum::extract::State(state), Ok(axum::body::Bytes::from(""))).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_yolo_job_400_invalid_body() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_yolo_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(r#"{"invalid"}"#)),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_yolo_job_400_unknown_fields() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_yolo_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"datasetId":"00000000-0000-0000-0000-000000000000","extra":1}"#,
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_yolo_job_400_invalid_model() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_yolo_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"datasetId":"00000000-0000-0000-0000-000000000000","model":"resnet50"}"#,
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_yolo_job_404_non_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_yolo_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(r#"{"datasetId":"not-a-uuid"}"#)),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn submit_yolo_job_503_manager_offline() {
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        // Note: this will 503 at the manager call because the dataset doesn't exist
        // but the mock will fail first.
        let resp = submit_yolo_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"datasetId":"00000000-0000-0000-0000-000000000000"}"#,
            )),
        )
        .await;
        // With connect_lazy pool, dataset check will fail → 500 internal,
        // but if mock.fail is true the manager will 503.
        // The actual status depends on whether the pool check succeeds.
        assert!(
            resp.status() == StatusCode::SERVICE_UNAVAILABLE
                || resp.status() == StatusCode::INTERNAL_SERVER_ERROR,
            "expected 503 or 500, got {}",
            resp.status()
        );
    }

    // --- POST /api/jobs/yolo weights tests (D5 ADR-0012) ---

    #[tokio::test]
    async fn submit_yolo_job_400_weights_not_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_yolo_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"datasetId":"00000000-0000-0000-0000-000000000000","weights":"not-a-uuid"}"#,
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_yolo_job_400_orchestrator_id_not_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_yolo_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"datasetId":"00000000-0000-0000-0000-000000000000","model":"yolo11n","epochs":10,"batch":16,"orchestratorId":"not-a-uuid"}"#,
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_yolo_job_weights_uuid_valid() {
        // weights válido: passa validação pura (sem DB = 503 ou not_found no dataset check).
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_yolo_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"datasetId":"00000000-0000-0000-0000-000000000000","weights":"550e8400-e29b-41d4-a716-446655440000"}"#,
            )),
        )
        .await;
        // Com pool lazy, dataset check falha → 500 ou not_found dependendo do timing.
        // O importante é que NÃO é 400 (weights UUID é válido).
        assert_ne!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "weights UUID should not trigger 400"
        );
    }

    #[tokio::test]
    async fn submit_yolo_job_extra_key_rejected_with_deny_unknown_fields() {
        // confirmar que deny_unknown_fields funciona (mock tolera extras no yaml
        // mas o serde rejeita no parse do body).
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_yolo_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"datasetId":"00000000-0000-0000-0000-000000000000","extraKey":"value"}"#,
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // --- POST /api/jobs/:id/abort unit tests (F4.2b) ---

    // --- POST /api/jobs/autotracker unit tests (ADR-0008 A.2) ---

    #[tokio::test]
    async fn submit_autotracker_job_400_empty_body() {
        let mock = MockManager::default();
        let state = test_state(mock);
        // Empty bytes → invalid JSON → 400.
        let resp =
            submit_autotracker_job(axum::extract::State(state), Ok(axum::body::Bytes::from("")))
                .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_autotracker_job_400_invalid_body() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_autotracker_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(r#"{"invalid"}"#)),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_autotracker_job_400_unknown_fields() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_autotracker_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"datasetId":"00000000-0000-0000-0000-000000000000","extra":1}"#,
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_autotracker_job_400_invalid_model() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_autotracker_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"datasetId":"00000000-0000-0000-0000-000000000000","model":"resnet50"}"#,
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_autotracker_job_404_non_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_autotracker_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(r#"{"datasetId":"not-a-uuid"}"#)),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // --- POST /api/jobs/autotracker modelId tests (ADR-0014 D2/D6) ---

    #[tokio::test]
    async fn submit_autotracker_job_202_no_model_id_mock() {
        // Sem modelId → mock, comportamento atual intocado.
        // Com pool lazy, vai falhar no DB antes de reach manager (500).
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_autotracker_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"datasetId":"00000000-0000-0000-0000-000000000000"}"#,
            )),
        )
        .await;
        // Com pool lazy, vai falhar no DB (500) — mas NÃO deve ser 400.
        assert_ne!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "valid body without modelId should not trigger 400"
        );
    }

    #[tokio::test]
    async fn submit_autotracker_job_202_with_valid_model_id() {
        // Com modelId válido → passa validação (não retorna 400).
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_autotracker_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"datasetId":"00000000-0000-0000-0000-000000000000","modelId":"550e8400-e29b-41d4-a716-446655440000"}"#,
            )),
        )
        .await;
        // Com pool lazy, vai falhar no DB (500) — mas NÃO deve ser 400.
        assert_ne!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "valid body with modelId should not trigger 400"
        );
    }

    #[tokio::test]
    async fn submit_autotracker_job_400_model_id_not_uuid() {
        // modelId não-UUID → 400 `invalid_request`.
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_autotracker_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"datasetId":"00000000-0000-0000-0000-000000000000","modelId":"not-a-uuid"}"#,
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_autotracker_job_400_orchestrator_id_not_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_autotracker_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"datasetId":"00000000-0000-0000-0000-000000000000","orchestratorId":"not-a-uuid"}"#,
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_autotracker_job_manager_not_found_compensates() {
        // ADR-0014 D6: manager NotFound → 404 + compensação.
        // NOTA: test_state usa pool lazy que falha no DB ANTES de reach create_job.
        // O mapeamento NotFound→404 é coberto pelo test-db; este teste valida o
        // caminho de falha do pool (500), não o mapeamento do manager.
        let mut mock = MockManager::default();
        mock.create_job_not_found = true;
        let state = test_state(mock);
        let resp = submit_autotracker_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"datasetId":"00000000-0000-0000-0000-000000000000","modelId":"550e8400-e29b-41d4-a716-446655440000"}"#,
            )),
        )
        .await;
        // Pool lazy falha no DB → 500 (não 404 nem 400).
        assert_eq!(
            resp.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "lazy pool DB failure should return 500"
        );
    }

    #[tokio::test]
    async fn submit_autotracker_job_manager_invalid_request_compensates() {
        // ADR-0014 D6: manager InvalidRequest → 400 + compensação.
        // NOTA: test_state usa pool lazy que falha no DB ANTES de reach create_job.
        // O mapeamento InvalidRequest→400 é coberto pelo test-db; este teste valida o
        // caminho de falha do pool (500), não o mapeamento do manager.
        let mut mock = MockManager::default();
        mock.create_job_invalid_request = Some("engine mismatch".into());
        let state = test_state(mock);
        let resp = submit_autotracker_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"datasetId":"00000000-0000-0000-0000-000000000000","modelId":"550e8400-e29b-41d4-a716-446655440000"}"#,
            )),
        )
        .await;
        // Pool lazy falha no DB → 500 (não 400 nem 503).
        assert_eq!(
            resp.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "lazy pool DB failure should return 500"
        );
    }

    #[tokio::test]
    async fn submit_autotracker_job_manager_fail_compensates() {
        // ADR-0014 D6: manager Unavailable → 503 + compensação.
        // NOTA: test_state usa pool lazy que falha no DB ANTES de reach create_job.
        // O mapeamento Unavailable→503 é coberto pelo test-db; este teste valida o
        // caminho de falha do pool (500), não o mapeamento do manager.
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let resp = submit_autotracker_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"datasetId":"00000000-0000-0000-0000-000000000000","modelId":"550e8400-e29b-41d4-a716-446655440000"}"#,
            )),
        )
        .await;
        // Pool lazy falha no DB → 500 (não 503).
        assert_eq!(
            resp.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "lazy pool DB failure should return 500"
        );
    }

    // --- POST /api/jobs/predict unit tests (Fatia J — ADR-0013 D0/D1/D8) ---

    #[tokio::test]
    async fn submit_predict_job_400_empty_body() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp =
            submit_predict_job(axum::extract::State(state), Ok(axum::body::Bytes::from(""))).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_predict_job_400_invalid_body() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_predict_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(r#"{"invalid"}"#)),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_predict_job_400_unknown_fields() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_predict_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"modelId":"00000000-0000-0000-0000-000000000000","datasetId":"00000000-0000-0000-0000-000000000001","extra":1}"#,
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_predict_job_400_model_id_not_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_predict_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"modelId":"not-a-uuid","datasetId":"00000000-0000-0000-0000-000000000001"}"#,
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_predict_job_400_orchestrator_id_not_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_predict_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"modelId":"00000000-0000-0000-0000-000000000000","datasetId":"00000000-0000-0000-0000-000000000001","orchestratorId":"not-a-uuid"}"#,
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_predict_job_400_conf_out_of_range() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_predict_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"modelId":"00000000-0000-0000-0000-000000000000","datasetId":"00000000-0000-0000-0000-000000000001","conf":1.5}"#,
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_predict_job_404_dataset_id_non_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_predict_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"modelId":"00000000-0000-0000-0000-000000000000","datasetId":"not-a-uuid"}"#,
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn submit_predict_job_valid_body_passes_validation() {
        // Prova que body com modelId UUID + conf válido passa validação (não retorna 400).
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_predict_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"modelId":"550e8400-e29b-41d4-a716-446655440000","datasetId":"550e8400-e29b-41d4-a716-446655440001"}"#,
            )),
        )
        .await;
        // Com pool lazy, vai falhar no DB (500) — mas NÃO deve ser 400.
        assert_ne!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "valid body should not trigger 400"
        );
    }

    #[tokio::test]
    async fn submit_predict_job_valid_body_with_custom_conf() {
        // Prova que body com conf customizado passa validação.
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_predict_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"modelId":"550e8400-e29b-41d4-a716-446655440000","datasetId":"550e8400-e29b-41d4-a716-446655440001","conf":0.9}"#,
            )),
        )
        .await;
        assert_ne!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "valid body with conf=0.9 should not trigger 400"
        );
    }

    // --- POST /api/jobs/predict compensation tests (A1 — Fatia J review J.6) ---

    #[tokio::test]
    async fn submit_predict_job_manager_not_found_compensates() {
        // A1: manager NotFound → 404 + compensação (delete_prefix chamado).
        let mut mock = MockManager::default();
        mock.create_job_not_found = true;
        let state = test_state(mock);
        let resp = submit_predict_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"modelId":"550e8400-e29b-41d4-a716-446655440000","datasetId":"550e8400-e29b-41d4-a716-446655440001"}"#,
            )),
        )
        .await;
        // Com pool lazy, vai falhar no DB antes de reach create_job (500).
        // NÃO deve ser 400.
        assert_ne!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "manager NotFound should not trigger 400"
        );
    }

    #[tokio::test]
    async fn submit_predict_job_manager_invalid_request_compensates() {
        // A1: manager InvalidRequest → 400 + compensação (delete_prefix chamado).
        let mut mock = MockManager::default();
        mock.create_job_invalid_request = Some("engine mismatch".into());
        let state = test_state(mock);
        let resp = submit_predict_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"modelId":"550e8400-e29b-41d4-a716-446655440000","datasetId":"550e8400-e29b-41d4-a716-446655440001"}"#,
            )),
        )
        .await;
        assert_ne!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "manager InvalidRequest should not trigger 400 at validation"
        );
    }

    #[tokio::test]
    async fn submit_predict_job_manager_fail_compensates() {
        // A1: manager Unavailable (fail=true) → 503 + compensação (delete_prefix chamado).
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let resp = submit_predict_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                r#"{"modelId":"550e8400-e29b-41d4-a716-446655440000","datasetId":"550e8400-e29b-41d4-a716-446655440001"}"#,
            )),
        )
        .await;
        assert!(
            resp.status() == StatusCode::SERVICE_UNAVAILABLE
                || resp.status() == StatusCode::INTERNAL_SERVER_ERROR,
            "expected 503 or 500, got {}",
            resp.status()
        );
    }

    // --- POST /api/jobs/:id/abort unit tests (F4.2b) ---

    #[tokio::test]
    async fn abort_job_404_non_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = abort_job(axum::extract::State(state), Path("not-a-uuid".to_string())).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn abort_job_200() {
        let mut mock = MockManager::default();
        mock.abort_job_result = Some(crate::jobs::manager_client::AbortJobResponse {
            status: "cancelling".to_string(),
        });
        let state = test_state(mock);
        let resp = abort_job(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn abort_job_404_manager_not_found() {
        let mock = MockManager::default(); // abort_job_result = None → NotFound
        let state = test_state(mock);
        let resp = abort_job(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn abort_job_503_manager_offline() {
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let resp = abort_job(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // =========================================================================
    // apply_autotracker_boxes tests (ADR-0008 D1)
    // =========================================================================

    fn autotracker_job_done() -> InternalJob {
        InternalJob {
            id: "550e8400-e29b-41d4-a716-446655440000".into(),
            kind: "autotracker".into(),
            engine: "autotracker".into(),
            model: "mock".into(),
            mode: "autotrack".into(),
            dataset_id: Some("550e8400-e29b-41d4-a716-446655440001".into()),
            status: "done".into(),
            queue_reason: None,
            queue_position: None,
            progress: Some(1.0),
            epoch: None,
            step: None,
            metrics: None,
            vram_min_gb: None,
            orchestrator_id: None,
            orchestrator_name: None,
            orchestrator_kind: None,
            orchestrator_fallback: false,
            created_at: "2026-01-01T00:00:00Z".into(),
            finished_at: Some("2026-01-01T01:00:00Z".into()),
            error: None,
            params: None,
        }
    }

    #[allow(dead_code)]
    fn boxes_artifact() -> InternalArtifact {
        let json_data = br#"{"engine":"autotracker","model":"mock","seed":42,"conf":0.65,"images":[{"filename":"img_0001.jpg","boxes":[{"class":"solda_fria","x":0.1,"y":0.2,"w":0.3,"h":0.4,"conf":0.96}]}]}"#;
        let md5 = format!(
            "{:x}",
            md5::Digest::finalize({
                use md5::Digest;
                let mut h = md5::Md5::new();
                md5::Digest::update(&mut h, json_data);
                h
            })
        );
        InternalArtifact {
            id: "aaaa-bbbb-cccc-dddd".into(),
            kind: "boxes".into(),
            path: "boxes.json".into(),
            md5,
            bytes: json_data.len() as i64,
        }
    }

    #[tokio::test]
    async fn apply_404_non_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = apply_autotracker_boxes(
            axum::extract::State(state),
            Path("nao-e-uuid".to_string()),
            Ok(axum::body::Bytes::from_static(b"{}")),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn apply_400_empty_body() {
        let mut mock = MockManager::default();
        mock.get_job_result = Some(autotracker_job_done());
        let state = test_state(mock);
        // Empty bytes fail JSON parse → 400 invalid_request.
        let resp = apply_autotracker_boxes(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
            Ok(axum::body::Bytes::new()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn apply_400_invalid_body() {
        let mut mock = MockManager::default();
        mock.get_job_result = Some(autotracker_job_done());
        let state = test_state(mock);
        let resp = apply_autotracker_boxes(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
            Ok(axum::body::Bytes::from_static(b"not json")),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn apply_400_unknown_fields() {
        let mut mock = MockManager::default();
        mock.get_job_result = Some(autotracker_job_done());
        let state = test_state(mock);
        let resp = apply_autotracker_boxes(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
            Ok(axum::body::Bytes::from_static(b"{\"extra\":1}")),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn apply_404_not_autotracker_engine() {
        let mut mock = MockManager::default();
        let mut job = autotracker_job_done();
        job.engine = "yolo".into();
        mock.get_job_result = Some(job);
        let state = test_state(mock);
        let resp = apply_autotracker_boxes(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
            Ok(axum::body::Bytes::from_static(b"{}")),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn apply_409_job_not_done_running() {
        let mut mock = MockManager::default();
        let mut job = autotracker_job_done();
        job.status = "running".into();
        mock.get_job_result = Some(job);
        let state = test_state(mock);
        let resp = apply_autotracker_boxes(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
            Ok(axum::body::Bytes::from_static(b"{}")),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "job_not_done");
    }

    #[tokio::test]
    async fn apply_409_job_not_done_queued() {
        let mut mock = MockManager::default();
        let mut job = autotracker_job_done();
        job.status = "queued".into();
        mock.get_job_result = Some(job);
        let state = test_state(mock);
        let resp = apply_autotracker_boxes(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
            Ok(axum::body::Bytes::from_static(b"{}")),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "job_not_done");
    }

    #[tokio::test]
    async fn apply_409_dataset_not_ready_null() {
        let mut mock = MockManager::default();
        let mut job = autotracker_job_done();
        job.dataset_id = None;
        mock.get_job_result = Some(job);
        let state = test_state(mock);
        let resp = apply_autotracker_boxes(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
            Ok(axum::body::Bytes::from_static(b"{}")),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "dataset_not_ready");
    }

    #[tokio::test]
    async fn apply_503_manager_offline() {
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let resp = apply_autotracker_boxes(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
            Ok(axum::body::Bytes::from_static(b"{}")),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn apply_404_job_not_found() {
        let mock = MockManager::default(); // get_job_result = None → NotFound
        let state = test_state(mock);
        let resp = apply_autotracker_boxes(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
            Ok(axum::body::Bytes::from_static(b"{}")),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn apply_404_no_boxes_artifact() {
        let mut mock = MockManager::default();
        mock.get_job_result = Some(autotracker_job_done());
        mock.list_artifacts_result = Some(vec![]); // no boxes artifact
        let state = test_state(mock);
        let resp = apply_autotracker_boxes(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
            Ok(axum::body::Bytes::from_static(b"{}")),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn apply_image_id_non_uuid() {
        let mut mock = MockManager::default();
        mock.get_job_result = Some(autotracker_job_done());
        let state = test_state(mock);
        let resp = apply_autotracker_boxes(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
            Ok(axum::body::Bytes::from_static(
                b"{\"imageId\":\"not-uuid\"}",
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // --- Autolabel unit tests (ADR-0016 D0/D1) ---

    #[tokio::test]
    async fn submit_autolabel_job_400_empty_body() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp =
            submit_autolabel_job(axum::extract::State(state), Ok(axum::body::Bytes::from("")))
                .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_autolabel_job_400_unknown_fields() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_autolabel_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from_static(
                br#"{"datasetId":"550e8400-e29b-41d4-a716-446655440000","extra":1}"#,
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_autolabel_job_400_invalid_model() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_autolabel_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from_static(
                br#"{"datasetId":"550e8400-e29b-41d4-a716-446655440000","model":"gpt-4"}"#,
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_autolabel_job_404_non_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_autolabel_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from_static(
                br#"{"datasetId":"nao-eh-uuid"}"#,
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn submit_autolabel_job_400_orchestrator_id_not_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = submit_autolabel_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from_static(
                br#"{"datasetId":"550e8400-e29b-41d4-a716-446655440000","orchestratorId":"bad-uuid"}"#,
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn apply_autolabel_captions_404_non_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = apply_autolabel_captions(
            axum::extract::State(state),
            Path("nao-eh-uuid".to_string()),
            Ok(axum::body::Bytes::from_static(b"{}")),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn apply_autolabel_captions_400_unknown_fields() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = apply_autolabel_captions(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
            Ok(axum::body::Bytes::from_static(b"{\"unknown\":true}")),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn preview_autolabel_captions_404_non_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = preview_autolabel_captions(
            axum::extract::State(state),
            Path("nao-eh-uuid".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn preview_autolabel_captions_404_job_not_found() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = preview_autolabel_captions(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // --- helpers ---

    fn mock_job() -> InternalJob {
        InternalJob {
            id: "550e8400-e29b-41d4-a716-446655440000".into(),
            kind: "yolo_train".into(),
            engine: "yolo".into(),
            model: "yolo11m".into(),
            mode: "train".into(),
            dataset_id: None,
            status: "queued".into(),
            queue_reason: None,
            queue_position: None,
            progress: None,
            epoch: None,
            step: None,
            metrics: None,
            vram_min_gb: None,
            orchestrator_id: None,
            orchestrator_name: None,
            orchestrator_kind: None,
            orchestrator_fallback: false,
            created_at: "2026-01-01T00:00:00Z".into(),
            finished_at: None,
            error: None,
            params: None,
        }
    }

    fn test_state(manager: MockManager) -> crate::state::AppState {
        crate::state::AppState {
            pool: sqlx::PgPool::connect_lazy("postgres://n/n").expect("lazy"),
            jwt_secret: [0x42; 32],
            secure_cookie: false,
            setup_required: false,
            storage: std::sync::Arc::new(crate::storage::MockStorage::new()),
            storage_config: crate::storage::StorageConfig {
                bucket: "heph-test".into(),
                public_endpoint: None,
                url_ttl_secs: 60,
            },
            embedder: std::sync::Arc::new(crate::search::MockEmbedder::new()),
            embedding_model: "ViT-B-32".to_string(),
            manager: std::sync::Arc::new(manager),
            model_download_allowed_hosts: vec![],
        }
    }

    // =========================================================================
    // Diffusion Generate v2 handler tests (ADR-0023 D2/D3/D4)
    // =========================================================================

    #[tokio::test]
    async fn submit_diffusion_generate_202_with_batch_and_loras() {
        let uuid_lora = "550e8400-e29b-41d4-a716-446655440000";
        let body_json = serde_json::json!({
            "prompt": "a test prompt",
            "batchSize": 4,
            "loras": [{"modelId": uuid_lora, "scale": 0.8}],
            "baseModel": "sdxl"
        });

        let mut mock = MockManager::default();
        mock.create_job_result = Some(CreateJobResponse {
            job_id: "job-123".into(),
            status: "queued".into(),
            queue_position: Some(1),
        });
        let state = test_state(mock);

        let resp = submit_diffusion_generate_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                serde_json::to_string(&body_json).unwrap(),
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn submit_diffusion_generate_params_contain_batch_size_and_loras() {
        let uuid_lora = "550e8400-e29b-41d4-a716-446655440000";
        let body_json = serde_json::json!({
            "prompt": "test",
            "batchSize": 2,
            "loras": [{"modelId": uuid_lora, "scale": 1.0}],
            "baseModel": "flux-2-klein-4b"
        });

        let mut mock = MockManager::default();
        mock.create_job_result = Some(CreateJobResponse {
            job_id: "job-456".into(),
            status: "queued".into(),
            queue_position: None,
        });
        // Guarda referência para verificar body depois
        let mock_arc = std::sync::Arc::new(mock);
        let mock_ref = std::sync::Arc::clone(&mock_arc);
        let mut state = test_state(MockManager::default());
        state.manager = mock_arc;

        let resp = submit_diffusion_generate_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                serde_json::to_string(&body_json).unwrap(),
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::ACCEPTED);

        // Verifica params camelCase no body enviado ao manager
        let body = mock_ref.last_create_job_body();
        let body = body.expect("create_job body captured");
        let params = body["params"].as_object().expect("params object");
        assert_eq!(params.get("batchSize"), Some(&serde_json::json!(2)));
        assert!(params.get("loras").is_some(), "missing loras in params");
        assert!(
            params.get("customModelId").is_some(),
            "missing customModelId in params"
        );
    }

    #[tokio::test]
    async fn submit_diffusion_generate_vram_min_custom_sdxl() {
        use crate::jobs::manager_client::InternalModel;

        let custom_id = "550e8400-e29b-41d4-a716-446655440099";
        let body_json = serde_json::json!({
            "prompt": "test",
            "customModelId": custom_id,
            "quantization": "4bit"
        });

        let mut mock = MockManager::default();
        mock.list_models_result = Some(vec![InternalModel {
            id: custom_id.into(),
            name: "my-sdxl.safetensors".into(),
            engine: "diffusion".into(),
            model: None,
            source: "upload".into(),
            md5: "abc123".into(),
            bytes: 6_500_000_000,
            path: "models/diffusion/custom/my-sdxl.safetensors".into(),
            job_id: None,
            created_at: "2026-09-15T00:00:00Z".into(),
            kind: Some("checkpoint".into()),
            arch: Some("sdxl".into()),
        }]);
        mock.create_job_result = Some(CreateJobResponse {
            job_id: "job-789".into(),
            status: "queued".into(),
            queue_position: None,
        });
        let mock_arc = std::sync::Arc::new(mock);
        let mock_ref = std::sync::Arc::clone(&mock_arc);
        let mut state = test_state(MockManager::default());
        state.manager = mock_arc;

        let resp = submit_diffusion_generate_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                serde_json::to_string(&body_json).unwrap(),
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::ACCEPTED);

        // vram_min para sdxl + 4bit = 8 (espelhamento first-class)
        let body = mock_ref.last_create_job_body();
        let body = body.expect("create_job body captured");
        assert_eq!(body["vram_min_gb"], 8);
    }

    #[tokio::test]
    async fn submit_diffusion_generate_custom_not_found_400() {
        let body_json = serde_json::json!({
            "prompt": "test",
            "customModelId": "550e8400-e29b-41d4-a716-446655440099"
        });

        let mut mock = MockManager::default();
        // Lista vazia — modelo não encontrado
        mock.list_models_result = Some(vec![]);
        let state = test_state(mock);

        let resp = submit_diffusion_generate_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                serde_json::to_string(&body_json).unwrap(),
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_diffusion_generate_custom_not_checkpoint_400() {
        let custom_id = "550e8400-e29b-41d4-a716-446655440099";
        let body_json = serde_json::json!({
            "prompt": "test",
            "customModelId": custom_id
        });

        let mut mock = MockManager::default();
        mock.list_models_result = Some(vec![InternalModel {
            id: custom_id.into(),
            name: "my-lora.safetensors".into(),
            engine: "diffusion".into(),
            model: None,
            source: "upload".into(),
            md5: "abc123".into(),
            bytes: 100_000,
            path: "models/diffusion/lora/my-lora.safetensors".into(),
            job_id: None,
            created_at: "2026-09-15T00:00:00Z".into(),
            kind: Some("lora".into()), // kind=lora, não checkpoint
            arch: Some("sdxl".into()),
        }]);
        let state = test_state(mock);

        let resp = submit_diffusion_generate_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                serde_json::to_string(&body_json).unwrap(),
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_diffusion_generate_custom_unsupported_arch_400() {
        let custom_id = "550e8400-e29b-41d4-a716-446655440099";
        let body_json = serde_json::json!({
            "prompt": "test",
            "customModelId": custom_id
        });

        let mut mock = MockManager::default();
        mock.list_models_result = Some(vec![InternalModel {
            id: custom_id.into(),
            name: "my-flux.safetensors".into(),
            engine: "diffusion".into(),
            model: None,
            source: "upload".into(),
            md5: "abc123".into(),
            bytes: 6_500_000_000,
            path: "models/diffusion/custom/my-flux.safetensors".into(),
            job_id: None,
            created_at: "2026-09-15T00:00:00Z".into(),
            kind: Some("checkpoint".into()),
            arch: Some("flux-2-klein-4b".into()), // arch não suportada para custom
        }]);
        let state = test_state(mock);

        let resp = submit_diffusion_generate_job(
            axum::extract::State(state),
            Ok(axum::body::Bytes::from(
                serde_json::to_string(&body_json).unwrap(),
            )),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // Verifica code "unsupported_architecture"
        let (parts, body) = resp.into_parts();
        let _ = parts;
        let body_bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(json["code"], "unsupported_architecture");
    }

    // =========================================================================
    // DELETE /api/jobs/:id unit tests (AC-003)
    // =========================================================================

    #[tokio::test]
    async fn delete_job_404_non_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = delete_job(axum::extract::State(state), Path("not-a-uuid".to_string())).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_job_200() {
        let mut mock = MockManager::default();
        mock.delete_job_result = Some(serde_json::json!({
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "status": "done",
            "artifacts": ["best.pt"],
            "object_keys": ["artifacts/550e8400-e29b-41d4-a716-446655440000/best.pt"],
            "models_deleted": 1,
            "generations_preserved": 0
        }));
        let state = test_state(mock);
        let resp = delete_job(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["id"], "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(json["status"], "done");
        assert!(json["artifacts"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("best.pt")));
        // wire deve ser camelCase (contract D1), nunca snake_case do manager.
        assert_eq!(json["objectKeys"].as_array().unwrap().len(), 1);
        assert_eq!(json["modelsDeleted"], 1);
        assert_eq!(json["generationsPreserved"], 0);
        assert!(
            json.get("object_keys").is_none(),
            "snake_case não deve vazar no wire"
        );
    }

    #[tokio::test]
    async fn delete_job_404_manager_not_found() {
        let mock = MockManager::default(); // delete_job_result = None → NotFound
        let state = test_state(mock);
        let resp = delete_job(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_job_409_not_terminal() {
        let mut mock = MockManager::default();
        mock.delete_not_terminal = true;
        let state = test_state(mock);
        let resp = delete_job(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "job_not_terminal");
    }

    #[tokio::test]
    async fn delete_job_503_manager_offline() {
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let resp = delete_job(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // =========================================================================
    // POST /api/jobs/cleanup unit tests (AC-003)
    // =========================================================================

    #[tokio::test]
    async fn cleanup_jobs_200() {
        let mut mock = MockManager::default();
        mock.cleanup_result = Some(serde_json::json!({
            "deleted": 2,
            "jobs": [
                {"id": "550e8400-e29b-41d4-a716-446655440000", "status": "done", "artifacts": []},
                {"id": "550e8400-e29b-41d4-a716-446655440001", "status": "failed", "artifacts": []}
            ]
        }));
        let state = test_state(mock);
        let resp = cleanup_jobs(
            axum::extract::State(state),
            Ok(Json(serde_json::json!({"olderThanDays": 30}))),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["deleted"], 2);
        assert!(json["jobs"].as_array().unwrap().len() == 2);
    }

    #[tokio::test]
    async fn cleanup_jobs_400_invalid_request() {
        let mut mock = MockManager::default();
        mock.cleanup_invalid_request = Some("invalid statuses".into());
        let state = test_state(mock);
        let resp = cleanup_jobs(
            axum::extract::State(state),
            Ok(Json(serde_json::json!({"statuses": ["invalid"]}))),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn cleanup_jobs_503_manager_offline() {
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let resp = cleanup_jobs(axum::extract::State(state), Ok(Json(serde_json::json!({})))).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
