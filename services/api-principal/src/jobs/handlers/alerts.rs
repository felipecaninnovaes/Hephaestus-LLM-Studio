//! Handler GET /api/jobs/:id/alerts (fatia 3c).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use uuid::Uuid;

use super::helpers::{not_found, queue_unavailable};
use crate::jobs::manager_client::ManagerError;
use crate::state::AppState;

/// GET /api/jobs/:id/alerts — lista alertas de um job (fatia 3c).
/// Retorna `JobAlertsResponse = { "items": [JobAlert] }` camelCase.
/// 404 se o job não existir ou UUID for inválido.
pub async fn get_job_alerts(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if Uuid::parse_str(&id).is_err() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "code": "not_found",
                "message": format!("Job não encontrado: {id}")
            })),
        )
            .into_response();
    }

    match state.manager.get_job_alerts(&id).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(ManagerError::NotFound) => not_found(),
        Err(_) => queue_unavailable(),
    }
}
