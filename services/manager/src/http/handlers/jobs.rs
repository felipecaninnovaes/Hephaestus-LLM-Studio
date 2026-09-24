//! Handlers HTTP para ciclo de vida e fila de jobs (MM-06).

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use crate::error::*;
use crate::http::extract::{AppJson, JobId, OptionalAppJson};
use crate::http::state::AppState;
use crate::{
    AbortResponse, ArtifactsListResponse, CreateJobRequest, PrepareCompleteRequest,
    PrepareFailRequest, ReportRequest,
};

#[derive(Deserialize, Default)]
pub struct ListJobsQuery {
    pub status: Option<String>,
    pub engine: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CleanupJobsRequest {
    #[serde(default)]
    pub older_than_days: Option<i64>,
    #[serde(default)]
    pub statuses: Option<Vec<String>>,
}

/// POST /internal/jobs — cria job.
pub async fn create_job_handler(
    State(state): State<AppState>,
    AppJson(req): AppJson<CreateJobRequest>,
) -> Response {
    match crate::create_job(&state.pool, req).await {
        Ok(resp) => (StatusCode::ACCEPTED, Json(resp)).into_response(),
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::InvalidRequest(ref msg)) => bad_request(msg),
        Err(ManagerError::Conflict(ref code)) => conflict(code, "conflict"),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// GET /internal/jobs — lista jobs.
pub async fn list_jobs_handler(
    State(state): State<AppState>,
    Query(params): Query<ListJobsQuery>,
) -> Response {
    match crate::list_jobs(
        &state.pool,
        params.status.as_deref(),
        params.engine.as_deref(),
    )
    .await
    {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// GET /internal/jobs/:id — detalhe de um job.
pub async fn get_job_handler(
    State(state): State<AppState>,
    JobId(uuid): JobId,
) -> Response {
    match crate::get_job(&state.pool, uuid).await {
        Ok(job) => (StatusCode::OK, Json(job)).into_response(),
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// GET /internal/jobs/:id/artifacts — lista artefatos de um job.
pub async fn list_artifacts_handler(
    State(state): State<AppState>,
    JobId(uuid): JobId,
) -> Response {
    match crate::get_job_artifacts(&state.pool, uuid).await {
        Ok(arts) => (StatusCode::OK, Json(ArtifactsListResponse { items: arts })).into_response(),
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// POST /internal/jobs/:id/abort — aborta um job.
pub async fn abort_job_handler(
    State(state): State<AppState>,
    JobId(uuid): JobId,
) -> Response {
    match crate::abort_job(&state.pool, uuid, state.orch_client.as_ref()).await {
        Ok(status) => (StatusCode::OK, Json(AbortResponse { status })).into_response(),
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::NotAbortable) => not_abortable(),
        Err(ManagerError::NotDeletable) => internal_error("unexpected job_not_terminal"),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(ManagerError::InvalidRequest(msg)) => bad_request(&msg),
        Err(ManagerError::PairingInvalid) => internal_error("unexpected pairing_invalid"),
        Err(ManagerError::Conflict(code)) => {
            internal_error(&format!("unexpected conflict: {code}"))
        }
    }
}

/// DELETE /internal/jobs/:id — apaga um job terminal (AC-003).
/// Guarda: não-terminal → 409 job_not_terminal; inexistente → 404.
pub async fn delete_job_handler(
    State(state): State<AppState>,
    JobId(uuid): JobId,
) -> Response {
    match crate::delete_job(&state.pool, uuid).await {
        Ok(deleted) => (StatusCode::OK, Json(deleted)).into_response(),
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::NotDeletable) => not_deletable(),
        Err(ManagerError::InvalidRequest(msg)) => bad_request(&msg),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// POST /internal/jobs/cleanup — limpeza em lote de jobs terminais (AC-003).
pub async fn cleanup_jobs_handler(
    State(state): State<AppState>,
    OptionalAppJson(req): OptionalAppJson<CleanupJobsRequest>,
) -> Response {
    let req = req.unwrap_or_default();
    match crate::cleanup_jobs(&state.pool, req.older_than_days, req.statuses).await {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(ManagerError::InvalidRequest(msg)) => bad_request(&msg),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// POST /internal/jobs/:id/report — report do orquestrador.
pub async fn report_job_handler(
    State(state): State<AppState>,
    JobId(uuid): JobId,
    AppJson(req): AppJson<ReportRequest>,
) -> Response {
    match crate::report_job(&state.pool, uuid, req).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// POST /internal/jobs/:id/prepare-complete — worker do BFF concluiu o empacotamento.
pub async fn prepare_complete_handler(
    State(state): State<AppState>,
    JobId(uuid): JobId,
    AppJson(req): AppJson<PrepareCompleteRequest>,
) -> Response {
    match crate::prepare_complete(&state.pool, uuid, req).await {
        Ok(()) => {
            // Disparo normal de dispatch (best-effort: falha não desfaz o complete).
            match crate::dispatch_next(
                &state.pool,
                state.orch_client.as_ref(),
                &state.exec_mode,
                &state.orch_workdir,
                &state.trainer_image,
                &state.vram_table,
            )
            .await
            {
                Ok(true) => tracing::info!("dispatch após prepare-complete: job despachado"),
                Ok(false) => {}
                Err(e) => tracing::warn!("dispatch após prepare-complete falhou: {e}"),
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({"ok": true, "status": "queued"})),
            )
                .into_response()
        }
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::Conflict(code)) => conflict(&code, "job is not in preparing state"),
        Err(ManagerError::InvalidRequest(msg)) => bad_request(&msg),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// POST /internal/jobs/:id/prepare-fail — worker do BFF falhou o empacotamento.
pub async fn prepare_fail_handler(
    State(state): State<AppState>,
    JobId(uuid): JobId,
    AppJson(req): AppJson<PrepareFailRequest>,
) -> Response {
    match crate::prepare_fail(&state.pool, uuid, req).await {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"ok": true, "status": "failed"})),
        )
            .into_response(),
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::Conflict(code)) => conflict(&code, "job is not in preparing state"),
        Err(ManagerError::InvalidRequest(msg)) => bad_request(&msg),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// POST /internal/jobs/:id/prepare-cancel — cancelamento de preparação pelo BFF.
pub async fn prepare_cancel_handler(
    State(state): State<AppState>,
    JobId(uuid): JobId,
) -> Response {
    match crate::prepare_cancel(&state.pool, uuid).await {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"ok": true, "status": "cancelled"})),
        )
            .into_response(),
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::Conflict(code)) => {
            conflict(&code, "job is not in preparing or cancelling state")
        }
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}
