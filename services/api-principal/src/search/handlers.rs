//! Handlers de busca semântica (ADR-0004 D4/D5, fatia 3f.4).
//!
//! - `POST /api/datasets/:id/search/index`: rebuild fire-and-forget (202 sempre).
//! - `GET /api/datasets/:id/search/status`: estado derivado images ×
//!   image_embeddings (200). `stale` nunca é emitido na v1 (reservado — D5).

use axum::{
    body::Bytes,
    extract::{
        rejection::{BytesRejection, FailedToBufferBody},
        Path, Query, State,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use std::collections::{HashMap, HashSet};

use super::embed::{EmbeddingError, DIM};
use crate::{
    datasets::{
        handlers::image_url,
        models::{parse_id, ImageResponse, ImageRow},
    },
    error::{
        err, MSG_EMBEDDING_UNAVAILABLE, MSG_INDEX_NOT_READY, MSG_INVALID_REQUEST, MSG_NOT_FOUND,
    },
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

/// Mapeamento puro `EmbeddingError` → 503 `embedding_unavailable` (espelha
/// `storage_unavailable` da 3b). `InvalidResponse` cai na mesma família
/// (o envelope `{code, message}` não tem campo de detalhe — ambas as
/// variantes produzem o mesmo corpo).
pub fn embedding_error_response(e: &EmbeddingError) -> Response {
    let _ = e;
    err(
        StatusCode::SERVICE_UNAVAILABLE,
        "embedding_unavailable",
        MSG_EMBEDDING_UNAVAILABLE,
    )
}

fn bad() -> Response {
    err(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        MSG_INVALID_REQUEST,
    )
}

fn index_not_ready() -> Response {
    err(StatusCode::CONFLICT, "index_not_ready", MSG_INDEX_NOT_READY)
}

/// Item de busca: o MESMO wire `Image` do `GET …/images` + `score` =
/// cosseno RAW (`1.0 - dist`, faixa -1..1 — CLIP normaliza L2 no embedder).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchItem {
    image: ImageResponse,
    score: f64,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchResponse {
    items: Vec<SearchItem>,
}

/// Candidato do SQL: id + distância coseno + split (o split entra no SELECT
/// para o pós-filtro em Rust — HNSW global não filtra, D5/R2).
struct Candidate {
    id: uuid::Uuid,
    dist: f64,
    split: String,
}

/// Busca vetorial com o vetor de query já resolvido + pós-filtros em Rust.
/// `class_filter`: `Some` ⇒ só imagens com ≥1 box da classe passam.
/// `split_filter`: `Some` ⇒ só imagens do split passam.
/// `threshold`: `Some` ⇒ só `score >= threshold` passa (by-image).
/// Ordem de score desc é preservada (o SQL já ordena por dist asc).
async fn run_search(
    state: &AppState,
    ds_id: uuid::Uuid,
    query: pgvector::Vector,
    limit: i64,
    class_filter: Option<uuid::Uuid>,
    split_filter: Option<&str>,
    threshold: Option<f64>,
) -> Result<Vec<SearchItem>, Response> {
    type CandRow = (uuid::Uuid, f64, String);
    let rows: Vec<CandRow> = match sqlx::query_as(
        "SELECT i.id, (e.embedding <=> $3)::float8 AS dist, i.split \
         FROM image_embeddings e \
         JOIN images i ON i.id = e.image_id \
         WHERE e.dataset_id = $1 AND e.model = $2 AND i.deleted_at IS NULL \
         ORDER BY e.embedding <=> $3 \
         LIMIT $4",
    )
    .bind(ds_id)
    .bind(&state.embedding_model)
    .bind(query)
    .bind(limit)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return Err(internal()),
    };
    let mut cands: Vec<Candidate> = rows
        .into_iter()
        .map(|(id, dist, split)| Candidate { id, dist, split })
        .collect();
    // Pós-filtro split (Rust — D5/R2).
    if let Some(s) = split_filter {
        cands.retain(|c| c.split == s);
    }
    // Pós-filtro classId (Rust — D5/R2): imagens com ao menos 1 box da classe.
    if let Some(class_id) = class_filter {
        let ids: Vec<uuid::Uuid> = cands.iter().map(|c| c.id).collect();
        let hits: Vec<(uuid::Uuid, uuid::Uuid)> =
            match sqlx::query_as("SELECT image_id, class_id FROM boxes WHERE image_id = ANY($1)")
                .bind(&ids)
                .fetch_all(&state.pool)
                .await
            {
                Ok(r) => r,
                Err(_) => return Err(internal()),
            };
        let keep: HashSet<uuid::Uuid> = hits
            .into_iter()
            .filter(|(_, c)| *c == class_id)
            .map(|(img, _)| img)
            .collect();
        cands.retain(|c| keep.contains(&c.id));
    }
    // Pós-filtro threshold (by-image): score = 1.0 - dist.
    if let Some(t) = threshold {
        cands.retain(|c| 1.0 - c.dist >= t);
    }
    if cands.is_empty() {
        return Ok(Vec::new());
    }
    // Linhas completas no wire do list (mesma construção — `url` incluída).
    let ids: Vec<uuid::Uuid> = cands.iter().map(|c| c.id).collect();
    type ImgTuple = (
        uuid::Uuid,
        String,
        String,
        i64,
        i32,
        i32,
        String,
        String,
        chrono::DateTime<chrono::Utc>,
    );
    let img_rows: Vec<ImgTuple> = match sqlx::query_as(
        "SELECT id, filename, object_key, bytes, width, height, media_type, split, created_at \
         FROM images WHERE id = ANY($1) AND deleted_at IS NULL",
    )
    .bind(&ids)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return Err(internal()),
    };
    let mut by_id: HashMap<uuid::Uuid, ImgTuple> = HashMap::with_capacity(img_rows.len());
    for r in img_rows {
        by_id.insert(r.0, r);
    }
    let mut out = Vec::with_capacity(cands.len());
    for c in cands {
        let Some((
            img_id,
            filename,
            object_key,
            bytes,
            width,
            height,
            media_type,
            split,
            created_at,
        )) = by_id.remove(&c.id)
        else {
            continue;
        };
        let url = match image_url(state, &object_key, ds_id, img_id).await {
            Ok(u) => u,
            Err(resp) => return Err(resp),
        };
        let mut resp = ImageResponse::from(ImageRow {
            id: img_id,
            filename,
            object_key,
            bytes,
            width,
            height,
            media_type,
            split,
            created_at,
        });
        resp.url = url;
        out.push(SearchItem {
            image: resp,
            score: 1.0 - c.dist,
        });
    }
    Ok(out)
}

/// GET /api/datasets/:id/search?q&k&classId&split — 200.
///
/// Ordem de validação (todas ANTES de tocar o embedder): `parse_id` → 404;
/// dataset inexistente → 404; validação pura → 400 (`q` 1..=500 chars,
/// `k` 1..=100 default 20, `classId` UUID, `split` train|val — o CHECK da
/// migration 0003 só admite esses dois); `indexedCount == 0` → 409
/// `index_not_ready`; embedder → 503 `embedding_unavailable`.
pub async fn get_search(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let ds_id = match parse_id(&id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    // Validação pura (400) — ANTES de qualquer query (o pool lazy dos
    // testes de contrato nunca é tocado; divergência da ordem numerada da
    // spec 3f.5, que lista o 404 do dataset antes — aqui o 400 precede).
    let q = match params.get("q") {
        Some(s) if (1..=500).contains(&s.chars().count()) => s.clone(),
        _ => return bad(),
    };
    let k: i64 = match params.get("k") {
        None => 20,
        Some(s) => match s.parse::<i64>() {
            Ok(v) if (1..=100).contains(&v) => v,
            _ => return bad(),
        },
    };
    let class_filter: Option<uuid::Uuid> = match params.get("classId") {
        None => None,
        Some(s) => match s.parse() {
            Ok(v) => Some(v),
            // Filtro opcional, não id de recurso: não-UUID ⇒ 400
            // (divergência CONSCIENTE do D8, documentada na ADR-0004).
            Err(_) => return bad(),
        },
    };
    let split_filter: Option<String> = match params.get("split") {
        None => None,
        Some(s) if s == "train" || s == "val" => Some(s.clone()),
        Some(_) => return bad(),
    };
    match dataset_exists(&state, ds_id).await {
        Ok(true) => {}
        Ok(false) => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
        Err(r) => return r,
    }
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
    if indexed == 0 {
        return index_not_ready();
    }
    let query_vec = match state.embedder.embed_texts(&[q]).await {
        Ok(mut v) => v.pop().expect("embed_texts ecoa o input"),
        Err(e) => return embedding_error_response(&e),
    };
    let items = match run_search(
        &state,
        ds_id,
        pgvector::Vector::from(query_vec),
        k.saturating_mul(4).min(400),
        class_filter,
        split_filter.as_deref(),
        None,
    )
    .await
    {
        Ok(items) => items,
        Err(r) => return r,
    };
    (StatusCode::OK, Json(SearchResponse { items })).into_response()
}

/// Corpo do `POST …/search/by-image` (`deny_unknown_fields` como a casa).
/// `imageId` é `String` de propósito: não-UUID ⇒ 404 `not_found` (D8
/// replicado) — com `Uuid` tipado o extractor devolveria 400/422.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ByImageRequest {
    image_id: String,
    k: Option<i64>,
    threshold: Option<f64>,
}

/// Body JSON no envelope (padrão `parse_json_body` da 3b): excesso de limite
/// ⇒ 413 `invalid_request`, qualquer outro erro de buffer ou parse ⇒ 400.
fn parse_json_body<T: serde::de::DeserializeOwned>(
    body: Result<Bytes, BytesRejection>,
) -> Result<T, Response> {
    let body = match body {
        Ok(b) => b,
        Err(BytesRejection::FailedToBufferBody(FailedToBufferBody::LengthLimitError(_))) => {
            return Err(err(
                StatusCode::PAYLOAD_TOO_LARGE,
                "invalid_request",
                MSG_INVALID_REQUEST,
            ));
        }
        Err(_) => {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                MSG_INVALID_REQUEST,
            ));
        }
    };
    match serde_json::from_slice::<T>(&body) {
        Ok(v) => Ok(v),
        Err(_) => Err(bad()),
    }
}

/// POST /api/datasets/:id/search/by-image — 200.
///
/// `imageId` não-UUID ou imagem fora do dataset ⇒ 404; sem embedding da
/// imagem no modelo ativo ⇒ 409 `index_not_ready`. O vetor de query é o
/// embedding da própria imagem (NUNCA chama `state.embedder` aqui).
/// A própria imagem aparece com score ~1.0 (não excluída — a UI destaca).
pub async fn post_search_by_image(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let ds_id = match parse_id(&id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    let req: ByImageRequest = match parse_json_body(body) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let img_id = match parse_id(&req.image_id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    let k = match req.k {
        None => 20,
        Some(v) if (1..=100).contains(&v) => v,
        Some(_) => return bad(),
    };
    let threshold: Option<f64> = match req.threshold {
        None => None,
        Some(t) if (-1.0..=1.0).contains(&t) => Some(t),
        Some(_) => return bad(),
    };
    match dataset_exists(&state, ds_id).await {
        Ok(true) => {}
        Ok(false) => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
        Err(r) => return r,
    }
    let belongs: bool = match sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM images WHERE id = $1 AND dataset_id = $2 AND deleted_at IS NULL)",
    )
    .bind(img_id)
    .bind(ds_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(v) => v.unwrap_or(false),
        Err(_) => return internal(),
    };
    if !belongs {
        return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND);
    }
    let row_opt = match sqlx::query(
        "SELECT embedding FROM image_embeddings WHERE image_id = $1 AND model = $2",
    )
    .bind(img_id)
    .bind(&state.embedding_model)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    let row = match row_opt {
        Some(r) => r,
        None => return index_not_ready(),
    };
    use sqlx::Row as _;
    let query: pgvector::Vector = match row.try_get("embedding") {
        Ok(v) => v,
        Err(_) => return internal(),
    };
    let items = match run_search(
        &state,
        ds_id,
        query,
        k.saturating_mul(4).min(400),
        None,
        None,
        threshold,
    )
    .await
    {
        Ok(items) => items,
        Err(r) => return r,
    };
    (StatusCode::OK, Json(SearchResponse { items })).into_response()
}
