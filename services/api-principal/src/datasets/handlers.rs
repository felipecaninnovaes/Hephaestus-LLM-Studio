//! Handlers datasets (contrato `packages/contracts/openapi.yaml`, Fatia 3a).
//!
//! - `list`: 200 array cru (vazio ⇒ `[]`, nunca 404).
//! - `create`: 201 | 400 `invalid_request` | 409 `slug_conflict` | 413
//!   `invalid_request` (body acima do limite, sempre no envelope).
//! - `get_one`: 200 | 404 `not_found` (id não-UUID também é 404, nunca 400).
//! - `delete`: 204 sem corpo | 404 `not_found`.
//!
//! 401 `unauthorized` em todas vem do gate (`auth::gate::require_auth`),
//! não daqui. 500 `internal` em qualquer falha de sqlx (sem detalhe do
//! driver, nada logado). Nada de filesystem na 3a (upload é a 3b).

use std::collections::HashMap;

use axum::{
    body::Bytes,
    extract::{
        rejection::{BytesRejection, FailedToBufferBody},
        Path, State,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use uuid::Uuid;

use super::models::{
    color_for, derive, normalize_classes, parse_id, slugify, CreateDatasetRequest,
    DatasetResponse, DatasetRow, DatasetType,
};
use crate::{
    error::{err, MSG_INVALID_REQUEST, MSG_NOT_FOUND, MSG_SLUG_CONFLICT},
    state::AppState,
};

const MSG_INTERNAL: &str = "internal server error";

/// As 13 colunas de `datasets` (ordem do `DatasetRow`).
const COLS: &str = "id, slug, title, category, type, task, format, status, size_bytes, images_count, labeled_count, created_at, updated_at";

fn internal() -> Response {
    err(StatusCode::INTERNAL_SERVER_ERROR, "internal", MSG_INTERNAL)
}

/// GET /api/datasets — coleção inteira por `updated_at` DESC (sem paginação na 3a).
pub async fn list(State(state): State<AppState>) -> Response {
    let rows: Vec<DatasetRow> = match sqlx::query_as::<_, DatasetRow>(&format!(
        "SELECT {COLS} FROM datasets ORDER BY updated_at DESC, id"
    ))
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    let class_rows: Vec<(Uuid, String)> =
        match sqlx::query_as("SELECT dataset_id, name FROM classes ORDER BY dataset_id, idx")
            .fetch_all(&state.pool)
            .await
        {
            Ok(r) => r,
            Err(_) => return internal(),
        };
    let mut by_dataset: HashMap<Uuid, Vec<String>> = HashMap::new();
    for (dataset_id, name) in class_rows {
        by_dataset.entry(dataset_id).or_default().push(name);
    }
    let out: Vec<DatasetResponse> = rows
        .into_iter()
        .map(|row| {
            let id = row.id;
            let mut resp = DatasetResponse::from(row);
            if let Some(names) = by_dataset.remove(&id) {
                resp.classes = names;
            }
            resp
        })
        .collect();
    (StatusCode::OK, Json(out)).into_response()
}

/// POST /api/datasets — cria metadados + classes (status `needs_labeling`).
///
/// O body chega como `Result<Bytes, BytesRejection>` porque o rejection
/// padrão do axum seria 413 `text/plain` fora do envelope D6; aqui todo
/// excesso de limite vira 413 `invalid_request` no envelope. O `POST /:id/upload`
/// da 3b repete o padrão com `DefaultBodyLimit` dedicado de 200 MB.
pub async fn create(
    State(state): State<AppState>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let body = match body {
        Ok(b) => b,
        Err(BytesRejection::FailedToBufferBody(FailedToBufferBody::LengthLimitError(_))) => {
            return err(
                StatusCode::PAYLOAD_TOO_LARGE,
                "invalid_request",
                MSG_INVALID_REQUEST,
            );
        }
        Err(_) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                MSG_INVALID_REQUEST,
            );
        }
    };
    let req: CreateDatasetRequest = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                MSG_INVALID_REQUEST,
            );
        }
    };
    if !(1..=96).contains(&req.title.chars().count()) {
        return err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            MSG_INVALID_REQUEST,
        );
    }
    let slug = slugify(&req.title);
    if slug.is_empty() {
        return err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            MSG_INVALID_REQUEST,
        );
    }
    let classes = match normalize_classes(&req.classes) {
        Ok(v) => v,
        Err(_) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                MSG_INVALID_REQUEST,
            );
        }
    };
    let (category, task, format) = derive(req.r#type);
    let type_wire: &str = match req.r#type {
        DatasetType::YoloBbox => "yolo_bbox",
        DatasetType::YoloSeg => "yolo_seg",
        DatasetType::DifusaoLora => "difusao_lora",
        DatasetType::ClipImageText => "clip_image_text",
    };

    let mut tx = match state.pool.begin().await {
        Ok(t) => t,
        Err(_) => return internal(),
    };
    let row: Option<DatasetRow> = match sqlx::query_as::<_, DatasetRow>(&format!(
        "INSERT INTO datasets (slug, title, category, type, task, format, status) \
         VALUES ($1,$2,$3,$4,$5,$6,'needs_labeling') \
         ON CONFLICT (slug) DO NOTHING RETURNING {COLS}"
    ))
    .bind(&slug)
    .bind(&req.title)
    .bind(category)
    .bind(type_wire)
    .bind(task)
    .bind(format)
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    let row = match row {
        Some(r) => r,
        None => {
            let _ = tx.rollback().await;
            return err(
                StatusCode::CONFLICT,
                "slug_conflict",
                MSG_SLUG_CONFLICT,
            );
        }
    };
    let idxs: Vec<i32> = (0..classes.len()).map(|i| i as i32).collect();
    let colors: Vec<String> = (0..classes.len())
        .map(|i| color_for(i).to_string())
        .collect();
    if sqlx::query(
        "INSERT INTO classes (dataset_id, name, idx, color) \
         SELECT $1, t.name, t.idx, t.color \
         FROM unnest($2::text[], $3::int[], $4::text[]) AS t(name, idx, color)",
    )
    .bind(row.id)
    .bind(&classes)
    .bind(&idxs)
    .bind(&colors)
    .execute(&mut *tx)
    .await
    .is_err()
    {
        let _ = tx.rollback().await;
        return internal();
    }
    if tx.commit().await.is_err() {
        return internal();
    }
    let mut resp = DatasetResponse::from(row);
    resp.classes = classes;
    (StatusCode::CREATED, Json(resp)).into_response()
}

/// GET /api/datasets/:id — detalhe (mesmo shape do item da lista).
pub async fn get_one(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let id = match parse_id(&id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    let row: Option<DatasetRow> = match sqlx::query_as::<_, DatasetRow>(&format!(
        "SELECT {COLS} FROM datasets WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    let row = match row {
        Some(r) => r,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    let classes: Vec<String> = match sqlx::query_scalar::<_, String>(
        "SELECT name FROM classes WHERE dataset_id = $1 ORDER BY idx",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    let mut resp = DatasetResponse::from(row);
    resp.classes = classes;
    (StatusCode::OK, Json(resp)).into_response()
}

/// DELETE /api/datasets/:id — remove dataset (CASCADE leva as classes); 204 sem corpo.
pub async fn delete(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let id = match parse_id(&id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    let gone: Option<Uuid> =
        match sqlx::query_scalar::<_, Uuid>("DELETE FROM datasets WHERE id = $1 RETURNING id")
            .bind(id)
            .fetch_optional(&state.pool)
            .await
        {
            Ok(r) => r,
            Err(_) => return internal(),
        };
    if gone.is_none() {
        return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND);
    }
    StatusCode::NO_CONTENT.into_response()
}
