//! Handlers stub da galeria de gerações (G.1 — ADR-0023 D5).
//!
//! Todos retornam 503 `queue_unavailable` até a implementação real (G.6).
//! O envelope de erro segue o padrão do projeto (`err()` de `crate::error`).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Response,
};

use crate::error::{err, MSG_QUEUE_UNAVAILABLE};
use crate::state::AppState;

fn queue_unavailable() -> Response {
    err(
        StatusCode::SERVICE_UNAVAILABLE,
        "queue_unavailable",
        MSG_QUEUE_UNAVAILABLE,
    )
}

/// GET /api/generations — lista gerações da galeria.
pub async fn list_generations(_state: State<AppState>) -> Response {
    queue_unavailable()
}

/// GET /api/generations/:id/data — proxy binário da imagem gerada.
pub async fn get_generation_data(_state: State<AppState>, _id: Path<uuid::Uuid>) -> Response {
    queue_unavailable()
}

/// POST /api/generations/delete — soft-delete em lote.
pub async fn delete_generations(_state: State<AppState>) -> Response {
    queue_unavailable()
}

/// POST /api/generations/export — exporta gerações como ZIP.
pub async fn export_generations(_state: State<AppState>) -> Response {
    queue_unavailable()
}
