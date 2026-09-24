//! Handlers HTTP para gerações (MM-06).

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use crate::error::*;
use crate::http::extract::{AppJson, GenerationId};
use crate::http::state::AppState;
use crate::DeleteGenerationsRequest;

#[derive(Deserialize, Default)]
pub struct ListGenerationsQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub deleted: bool,
    pub base_model: Option<String>,
}

fn default_limit() -> i64 {
    50
}

/// GET /internal/generations — lista generations com paginação e filtros.
pub async fn list_generations_handler(
    State(state): State<AppState>,
    Query(params): Query<ListGenerationsQuery>,
) -> Response {
    let limit = params.limit.clamp(1, 200);
    let offset = params.offset.max(0);
    match crate::list_generations(
        &state.pool,
        limit,
        offset,
        params.deleted,
        params.base_model.as_deref(),
    )
    .await
    {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(ManagerError::InvalidRequest(msg)) => bad_request(&msg),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// POST /internal/generations/delete — soft delete de generations.
pub async fn delete_generations_handler(
    State(state): State<AppState>,
    AppJson(req): AppJson<DeleteGenerationsRequest>,
) -> Response {
    match crate::soft_delete_generations(&state.pool, &req.ids).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(ManagerError::InvalidRequest(msg)) => bad_request(&msg),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// GET /internal/generations/:id — retorna uma generation por ID (proxy de imagem da api-principal).
/// Soft-deletadas retornam 404.
pub async fn get_generation_handler(
    State(state): State<AppState>,
    GenerationId(uid): GenerationId,
) -> Response {
    match crate::get_generation(&state.pool, uid).await {
        Ok(Some(row)) => (StatusCode::OK, Json(row)).into_response(),
        Ok(None) => not_found(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}
