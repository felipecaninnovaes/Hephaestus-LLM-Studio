//! Handler de linhagem do job (fatia 5b): proxy camelCase do grafo calculado pelo manager.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

use super::helpers::{not_found, parse_uuid, queue_unavailable};
use super::types::{LineageEdgeResponse, LineageGraphResponse, LineageNodeResponse};
use crate::jobs::manager_client::ManagerError;
use crate::state::AppState;

/// GET /api/jobs/:id/lineage — grafo dataset→job→checkpoint→resume→geração.
pub async fn get_job_lineage(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if parse_uuid(&id).is_none() {
        return not_found();
    }
    let graph = match state.manager.get_lineage(&id).await {
        Ok(g) => g,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let nodes = graph
        .nodes
        .into_iter()
        .map(|n| LineageNodeResponse {
            id: n.id,
            kind: n.kind,
            label: n.label,
            status: n.status,
            created_at: n.created_at,
            epoch: n.epoch,
        })
        .collect();
    let edges = graph
        .edges
        .into_iter()
        .map(|e| LineageEdgeResponse {
            from: e.from,
            to: e.to,
            kind: e.kind,
        })
        .collect();
    (StatusCode::OK, Json(LineageGraphResponse { nodes, edges })).into_response()
}
