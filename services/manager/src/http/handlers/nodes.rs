//! Handlers HTTP para nós e telemetria (MM-06).

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

use crate::error::*;
use crate::http::extract::{AppJson, OrchestratorId};
use crate::http::state::AppState;
use crate::{AdoptRequest, HeartbeatRequest};

/// POST /internal/heartbeat — heartbeat do orquestrador.
pub async fn heartbeat_handler(
    State(state): State<AppState>,
    AppJson(req): AppJson<HeartbeatRequest>,
) -> Response {
    match crate::receive_heartbeat(&state.pool, &state.telemetry_cache, req).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// GET /internal/telemetry — telemetria do cache de heartbeat.
pub async fn telemetry_handler(State(state): State<AppState>) -> Response {
    let resp = crate::get_telemetry(&state.pool, &state.telemetry_cache).await;
    (StatusCode::OK, Json(resp)).into_response()
}

/// GET /internal/orchestrators — lista orquestradores com telemetria por nó.
pub async fn list_orchestrators_handler(State(state): State<AppState>) -> Response {
    match crate::list_orchestrators(&state.pool, &state.telemetry_cache).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// POST /internal/adopt — adopt de orquestrador remoto/local via pairing code.
pub async fn adopt_handler(
    State(state): State<AppState>,
    AppJson(req): AppJson<AdoptRequest>,
) -> Response {
    match crate::adopt_internal(&state.pool, state.orch_client.as_ref(), &req).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(ManagerError::InvalidRequest(msg)) => bad_request(&msg),
        Err(ManagerError::PairingInvalid) => pairing_invalid(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// POST /internal/orchestrators/:id/revoke — revoke orquestrador.
pub async fn revoke_handler(
    State(state): State<AppState>,
    OrchestratorId(uuid): OrchestratorId,
) -> Response {
    match crate::revoke_orchestrator(&state.pool, uuid).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}
