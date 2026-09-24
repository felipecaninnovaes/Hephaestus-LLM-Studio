//! Handlers HTTP para catálogo de modelos (MM-06).

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

use crate::error::*;
use crate::http::extract::{AppJson, ModelId};
use crate::http::state::AppState;
use crate::{CreateModelRequest, UpdateModelRequest};

/// GET /internal/models — modelos derivados de job_artifacts.kind='model'.
pub async fn list_models_handler(State(state): State<AppState>) -> Response {
    match crate::list_models(&state.pool).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// POST /internal/models — cria row na tabela models (ADR-0012 D1/I.2b).
pub async fn create_model_handler(
    State(state): State<AppState>,
    AppJson(req): AppJson<CreateModelRequest>,
) -> Response {
    match crate::create_model(&state.pool, req).await {
        Ok(item) => (StatusCode::CREATED, Json(item)).into_response(),
        Err(ManagerError::InvalidRequest(msg)) => bad_request(&msg),
        Err(ManagerError::Internal(ref msg)) if msg == "model_exists" => error_response(
            StatusCode::CONFLICT,
            "model_exists",
            "model with this s3_key already exists",
        ),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// GET /internal/storage/usage — soma de bytes de job_artifacts.
pub async fn get_storage_usage_handler(State(state): State<AppState>) -> Response {
    match crate::get_storage_usage(&state.pool).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// DELETE /internal/models/:id — remove modelo da tabela models.
pub async fn delete_model_handler(
    State(state): State<AppState>,
    ModelId(uid): ModelId,
) -> Response {
    match crate::delete_model(&state.pool, uid).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// PATCH /internal/models/:id — atualiza o nome de um modelo na tabela models (ADR-0022 D2).
pub async fn update_model_handler(
    State(state): State<AppState>,
    ModelId(uid): ModelId,
    AppJson(req): AppJson<UpdateModelRequest>,
) -> Response {
    match crate::update_model(&state.pool, uid, req).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::InvalidRequest(msg)) => bad_request(&msg),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}
