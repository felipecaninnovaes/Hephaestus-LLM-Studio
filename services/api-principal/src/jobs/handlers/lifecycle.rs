//! Handlers de controle de execução e ciclo de vida de jobs (abort, delete, cleanup).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

use super::helpers::{
    cleanup_result_to_wire, extract_object_keys, invalid_request, job_deleted_to_wire,
    job_not_abortable, job_not_terminal, not_found, parse_uuid, queue_unavailable,
    sweep_object_keys,
};
use super::types::AbortResponse;
use crate::jobs::manager_client::ManagerError;
use crate::state::AppState;

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
