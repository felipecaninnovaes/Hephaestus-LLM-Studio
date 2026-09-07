//! Handlers de busca semântica (ADR-0004 D4/D5, fatia 3f.4).
//!
//! - `POST /api/datasets/:id/search/index`: rebuild fire-and-forget (202 sempre).
//! - `GET /api/datasets/:id/search/status`: estado derivado images ×
//!   image_embeddings (200). `stale` nunca é emitido na v1 (reservado — D5).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

use super::embed::DIM;
use crate::{
    datasets::models::parse_id,
    error::{err, MSG_NOT_FOUND},
    state::AppState,
};

const MSG_INTERNAL: &str = "internal server error";

fn internal() -> Response {
    err(StatusCode::INTERNAL_SERVER_ERROR, "internal", MSG_INTERNAL)
}

async fn dataset_exists(state: &AppState, ds_id: uuid::Uuid) -> Result<bool, Response> {
    match sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM datasets WHERE id = $1)")
        .bind(ds_id)
        .fetch_one(&state.pool)
        .await
    {
        Ok(v) => Ok(v),
        Err(_) => Err(internal()),
    }
}

/// Resposta 202 do rebuild: `indexing` (spawn disparado) ou `not_indexed`
/// (dataset sem imagens — não trabalha).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct IndexResponse {
    status: &'static str,
}

/// POST /api/datasets/:id/search/index — 202 sempre (padrão D8: id
/// não-UUID ou dataset inexistente ⇒ 404 `not_found`).
pub async fn post_index(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let ds_id = match parse_id(&id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    match dataset_exists(&state, ds_id).await {
        Ok(true) => {}
        Ok(false) => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
        Err(r) => return r,
    }
    let images: i64 = match sqlx::query_scalar(
        "SELECT count(*) FROM images WHERE dataset_id = $1 AND deleted_at IS NULL",
    )
    .bind(ds_id)
    .fetch_one(&state.pool)
    .await
    {
        Ok(n) => n,
        Err(_) => return internal(),
    };
    if images == 0 {
        return (
            StatusCode::ACCEPTED,
            Json(IndexResponse {
                status: "not_indexed",
            }),
        )
            .into_response();
    }
    let st = state.clone();
    tokio::spawn(async move {
        let wrote = super::indexer::index_dataset_images(st, ds_id, None).await;
        eprintln!("[indexer] dataset {ds_id} rebuild: {wrote} embeddings escritos");
    });
    (
        StatusCode::ACCEPTED,
        Json(IndexResponse { status: "indexing" }),
    )
        .into_response()
}

/// Resposta 200 do status derivado (camelCase no wire — D1).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusResponse {
    status: &'static str,
    images_count: i64,
    indexed_count: i64,
    model: String,
    dim: usize,
}

/// GET /api/datasets/:id/search/status — 200 com contadores derivados.
pub async fn get_status(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let ds_id = match parse_id(&id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    match dataset_exists(&state, ds_id).await {
        Ok(true) => {}
        Ok(false) => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
        Err(r) => return r,
    }
    let images: i64 = match sqlx::query_scalar(
        "SELECT count(*) FROM images WHERE dataset_id = $1 AND deleted_at IS NULL",
    )
    .bind(ds_id)
    .fetch_one(&state.pool)
    .await
    {
        Ok(n) => n,
        Err(_) => return internal(),
    };
    let indexed: i64 = match sqlx::query_scalar(
        "SELECT count(*) FROM image_embeddings WHERE dataset_id = $1 AND model = $2",
    )
    .bind(ds_id)
    .bind(&state.embedding_model)
    .fetch_one(&state.pool)
    .await
    {
        Ok(n) => n,
        Err(_) => return internal(),
    };
    // D5 literal: 0 embeddings do modelo ativo -> not_indexed (o front mostra
    // "Indexar agora" — que também é o reparo manual para crash puro); com o
    // spawn morto o estado não mente como "indexing". 0 < idx < img -> indexing.
    let status = if indexed == 0 {
        "not_indexed"
    } else if indexed < images {
        "indexing"
    } else {
        "ready"
    };
    (
        StatusCode::OK,
        Json(StatusResponse {
            status,
            images_count: images,
            indexed_count: indexed,
            model: state.embedding_model.clone(),
            dim: DIM,
        }),
    )
        .into_response()
}
