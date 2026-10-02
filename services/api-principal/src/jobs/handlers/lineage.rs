//! Handler de linhagem do job (fatia 5b): repassa o grafo calculado pelo
//! manager. Sem DTO espelho — `heph_contracts::LineageResponse` já é
//! camelCase (wire público = wire interno, como `MetricPointsResponse`).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

use super::helpers::{not_found, parse_uuid, queue_unavailable};
use crate::jobs::manager_client::ManagerError;
use crate::state::AppState;

/// GET /api/jobs/:id/lineage — grafo dataset→job→checkpoint→resume→geração.
pub async fn get_job_lineage(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if parse_uuid(&id).is_none() {
        return not_found();
    }
    match state.manager.get_lineage(&id).await {
        Ok(graph) => (StatusCode::OK, Json(graph)).into_response(),
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::Unavailable(_)) => queue_unavailable(),
        Err(_) => queue_unavailable(),
    }
}
