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
        Multipart, Path, Query, State,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use uuid::Uuid;

use super::models::{
    color_for, derive, derived_source, normalize_classes, parse_id, plan_classes, slugify,
    validate_batch_boxes_update, validate_boxes, validate_caption, BatchBoxesUpdateRequest,
    BatchBoxesUpdateResponse, BoxResponse, CaptionResponse, CreateDatasetRequest,
    DatasetClassResponse, DatasetResponse, DatasetRow, DatasetType, ImageDetailResponse, ImagePage,
    ImageResponse, ImageRow, PutBoxesRequest, PutBoxesResponse, PutCaptionRequest,
    PutClassesRequest, PutClassesResponse, UploadItem, UploadResult,
};
use crate::{
    error::{
        err, MSG_CLASSES_IN_USE, MSG_INVALID_REQUEST, MSG_NOT_FOUND, MSG_SLUG_CONFLICT,
        MSG_STORAGE_UNAVAILABLE,
    },
    state::AppState,
    storage::{keys, sniff::MediaType, StorageError},
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
        // Derivado, não coluna (dívida T7): true se alguma box com origin='autotracker' em imagem ATIVA do dataset (lixeira não conta, 3g.3).
        "SELECT {COLS}, EXISTS(SELECT 1 FROM images i JOIN boxes b ON b.image_id = i.id WHERE i.dataset_id = datasets.id AND i.deleted_at IS NULL AND b.origin = 'autotracker') AS auto_tracked, (SELECT count(*)::int FROM images i WHERE i.dataset_id = datasets.id AND i.deleted_at IS NOT NULL) AS trash_count FROM datasets ORDER BY updated_at DESC, id"
    ))
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    let class_rows: Vec<(Uuid, Uuid, String, i32, String)> = match sqlx::query_as(
        "SELECT dataset_id, id, name, idx, color FROM classes ORDER BY dataset_id, idx",
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    let mut by_dataset: HashMap<Uuid, Vec<DatasetClassResponse>> = HashMap::new();
    for (dataset_id, id, name, idx, color) in class_rows {
        by_dataset
            .entry(dataset_id)
            .or_default()
            .push(DatasetClassResponse::from((id, name, idx, color)));
    }
    let out: Vec<DatasetResponse> = rows
        .into_iter()
        .map(|row| {
            let id = row.id;
            let images_count = row.images_count;
            let mut resp = DatasetResponse::from(row);
            if let Some(classes) = by_dataset.remove(&id) {
                resp.classes = classes;
            }
            resp.source = derived_source(&state.storage_config.bucket, id, images_count);
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
        // Derivado, não coluna (dívida T7): true se alguma box com origin='autotracker' em imagem ATIVA do dataset (lixeira não conta, 3g.3).
        "INSERT INTO datasets (slug, title, category, type, task, format, status) \
         VALUES ($1,$2,$3,$4,$5,$6,'needs_labeling') \
         ON CONFLICT (slug) DO NOTHING RETURNING {COLS}, EXISTS(SELECT 1 FROM images i JOIN boxes b ON b.image_id = i.id WHERE i.dataset_id = datasets.id AND i.deleted_at IS NULL AND b.origin = 'autotracker') AS auto_tracked, (SELECT count(*)::int FROM images i WHERE i.dataset_id = datasets.id AND i.deleted_at IS NOT NULL) AS trash_count"
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
            return err(StatusCode::CONFLICT, "slug_conflict", MSG_SLUG_CONFLICT);
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
    let ds_id = row.id;
    let images_count = row.images_count;
    let mut resp = DatasetResponse::from(row);
    let created_classes: Vec<(Uuid, String, i32, String)> = match sqlx::query_as(
        "SELECT id, name, idx, color FROM classes WHERE dataset_id = $1 ORDER BY idx",
    )
    .bind(ds_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    resp.classes = created_classes
        .into_iter()
        .map(DatasetClassResponse::from)
        .collect();
    resp.source = derived_source(&state.storage_config.bucket, ds_id, images_count);
    (StatusCode::CREATED, Json(resp)).into_response()
}

/// GET /api/datasets/:id — detalhe (mesmo shape do item da lista).
pub async fn get_one(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let id = match parse_id(&id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    let row: Option<DatasetRow> = match sqlx::query_as::<_, DatasetRow>(&format!(
        // Derivado, não coluna (dívida T7): true se alguma box com origin='autotracker' em imagem ATIVA do dataset (lixeira não conta, 3g.3).
        "SELECT {COLS}, EXISTS(SELECT 1 FROM images i JOIN boxes b ON b.image_id = i.id WHERE i.dataset_id = datasets.id AND i.deleted_at IS NULL AND b.origin = 'autotracker') AS auto_tracked, (SELECT count(*)::int FROM images i WHERE i.dataset_id = datasets.id AND i.deleted_at IS NOT NULL) AS trash_count FROM datasets WHERE id = $1"
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
    let class_rows: Vec<(Uuid, String, i32, String)> = match sqlx::query_as(
        "SELECT id, name, idx, color FROM classes WHERE dataset_id = $1 ORDER BY idx",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    let images_count = row.images_count;
    let mut resp = DatasetResponse::from(row);
    resp.classes = class_rows
        .into_iter()
        .map(DatasetClassResponse::from)
        .collect();
    resp.source = derived_source(&state.storage_config.bucket, id, images_count);
    (StatusCode::OK, Json(resp)).into_response()
}

/// DELETE /api/datasets/:id — remove dataset (CASCADE leva classes/images/
/// boxes/captions/videos); 204 sem corpo.
///
/// Ordem commit→sweep (ADR-0003 D7): o banco commita primeiro; depois o handler
/// varre o prefixo `datasets/{id}/` no storage best-effort. Estado bom = nenhum
/// objeto sob o prefixo; órfão sob prefixo deletado é o aceitável (R5) — falha
/// da varredura NÃO transforma o 204 em erro.
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
    let prefix = format!("datasets/{id}/");
    if let Err(e) = state.storage.delete_prefix(&prefix).await {
        // D7: falha da varredura NÃO transforma o 204 em erro — o prefixo fica
        // reapável por script. Sem framework de log ainda (dívida nomeada em
        // docs/archive/coordenacao-historico-2026-09.md), o eprintln é o mínimo honesto. `e` é Display
        // ESTÁTICO (nunca endpoint/credencial).
        eprintln!(
            "aviso: sweep do prefixo {prefix} falhou ({e}) — dataset deletado, objetos reapáveis"
        );
    }
    StatusCode::NO_CONTENT.into_response()
}

/// DELETE /api/datasets/:id/images/:imageId — soft delete (3g.2, ADR-0005 D4).
///
/// 204 sem corpo, SEM sweep (objeto intocado — restaurável via lixeira).
/// Inexistente, já na lixeira, de outro dataset ou UUID inválido ⇒ 404 (D8).
/// O trigger `images_refresh_counters` (0005) recalcula os contadores.
pub async fn delete_image(
    State(state): State<AppState>,
    Path((id, image_id)): Path<(String, String)>,
) -> Response {
    let ds_id: Uuid = match parse_id(&id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    let img_id: Uuid = match parse_id(&image_id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    let gone: Option<Uuid> = match sqlx::query_scalar(
        "UPDATE images SET deleted_at = now() WHERE id = $1 AND dataset_id = $2 AND deleted_at IS NULL RETURNING id",
    )
    .bind(img_id)
    .bind(ds_id)
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

/// Resposta do restore COM rename: o filename novo (200, wire camelCase).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreImageResponse {
    pub filename: String,
}

/// Separa stem/ext na ÚLTIMA extensão (`foto.jpg` ⇒ (`foto`, `jpg`)).
fn split_stem_ext(filename: &str) -> (&str, &str) {
    match filename.rfind('.') {
        Some(i) if i > 0 => (&filename[..i], &filename[i + 1..]),
        _ => (filename, ""),
    }
}

/// Filename livre p/ restore com conflito (`{stem}_restaurado{ext}`,
/// desambiguando `_restaurado_2`, `_restaurado_3`… contra os nomes ATIVOS).
fn restore_candidate(filename: &str, active: &[String]) -> String {
    let (stem, ext) = split_stem_ext(filename);
    let dot_ext = if ext.is_empty() {
        String::new()
    } else {
        format!(".{ext}")
    };
    let mut candidate = format!("{stem}_restaurado{dot_ext}");
    let mut n = 2;
    while active.contains(&candidate) {
        candidate = format!("{stem}_restaurado_{n}{dot_ext}");
        n += 1;
    }
    candidate
}

/// POST /api/datasets/:id/images/:imageId/restore — tira da lixeira (3g.2).
///
/// (a) linha soft-deleted escopada ⇒ None ⇒ 404 (inclui imagem ATIVA);
/// (b) sem conflito de filename ativo ⇒ `deleted_at = NULL` ⇒ 204;
/// (c) com conflito ⇒ rename + `copy_object` p/ key nova (mesmo imageId) +
/// UPDATE ⇒ 200 `{filename}`; copy falha ⇒ 503 (nada escrito); UPDATE
/// pós-copy falha ⇒ `delete(key_nova)` best-effort + 500. Em sucesso COM
/// rename, `delete(key_antiga)` best-effort pós-commit (eprintln, nunca erro).
pub async fn restore_image(
    State(state): State<AppState>,
    Path((id, image_id)): Path<(String, String)>,
) -> Response {
    let ds_id: Uuid = match parse_id(&id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    let img_id: Uuid = match parse_id(&image_id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    // (a) só soft-deleted escopada ao dataset é restaurável.
    let row: Option<(String, String)> = match sqlx::query_as(
        "SELECT filename, object_key FROM images WHERE id = $1 AND dataset_id = $2 AND deleted_at IS NOT NULL",
    )
    .bind(img_id)
    .bind(ds_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    let (filename, object_key) = match row {
        Some(r) => r,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    // Nomes ativos do dataset (UNIQUE parcial só sobre ativas).
    let active: Vec<String> = match sqlx::query_scalar(
        "SELECT filename FROM images WHERE dataset_id = $1 AND deleted_at IS NULL",
    )
    .bind(ds_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    // (b) sem conflito: só tira da lixeira.
    if !active.contains(&filename) {
        let back: Option<Uuid> = match sqlx::query_scalar(
            "UPDATE images SET deleted_at = NULL WHERE id = $1 AND dataset_id = $2 AND deleted_at IS NOT NULL RETURNING id",
        )
        .bind(img_id)
        .bind(ds_id)
        .fetch_optional(&state.pool)
        .await
        {
            Ok(r) => r,
            Err(_) => return internal(),
        };
        if back.is_none() {
            return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND);
        }
        return StatusCode::NO_CONTENT.into_response();
    }
    // (c) com conflito: rename + cópia server-side + UPDATE.
    let new_filename = restore_candidate(&filename, &active);
    let new_key = format!("datasets/{ds_id}/images/{img_id}/{new_filename}");
    if state
        .storage
        .copy_object(&object_key, &new_key)
        .await
        .is_err()
    {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_unavailable",
            MSG_STORAGE_UNAVAILABLE,
        );
    }
    let ok: Result<_, _> = sqlx::query(
        "UPDATE images SET filename = $1, object_key = $2, deleted_at = NULL WHERE id = $3 AND dataset_id = $4",
    )
    .bind(&new_filename)
    .bind(&new_key)
    .bind(img_id)
    .bind(ds_id)
    .execute(&state.pool)
    .await;
    if ok.is_err() {
        // Compensação: a cópia ficou órfã — remove best-effort.
        let _ = state.storage.delete(&new_key).await;
        return internal();
    }
    // Pós-commit: a key antiga sai best-effort (D7 — falha loga, nunca erro).
    if let Err(e) = state.storage.delete(&object_key).await {
        eprintln!("aviso: delete da key antiga {object_key} falhou ({e}) — objeto reapável");
    }
    (
        StatusCode::OK,
        Json(RestoreImageResponse {
            filename: new_filename,
        }),
    )
        .into_response()
}

/// DELETE /api/datasets/:id/trash — purga permanente da lixeira (3g.2).
///
/// (a) dataset inválido/inexistente ⇒ 404 (D8); (b) DELETE das soft-deleted
/// com RETURNING (CASCADE em boxes/captions + trigger recalcula — statement
/// ÚNICO, atômico); (c+d) `delete_prefix` por imagem best-effort (falha loga,
/// nunca vira erro); (e) 204 sempre (lixeira vazia ⇒ idempotente).
pub async fn delete_trash(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let ds_id: Uuid = match parse_id(&id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    let exists: bool =
        match sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM datasets WHERE id = $1)")
            .bind(ds_id)
            .fetch_one(&state.pool)
            .await
        {
            Ok(v) => v,
            Err(_) => return internal(),
        };
    if !exists {
        return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND);
    }
    let purged: Vec<Uuid> = match sqlx::query_scalar(
        "DELETE FROM images WHERE dataset_id = $1 AND deleted_at IS NOT NULL RETURNING id",
    )
    .bind(ds_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    for img_id in &purged {
        let prefix = format!("datasets/{ds_id}/images/{img_id}/");
        if let Err(e) = state.storage.delete_prefix(&prefix).await {
            eprintln!("aviso: sweep da lixeira {prefix} falhou ({e}) — objetos reapáveis");
        }
    }
    StatusCode::NO_CONTENT.into_response()
}

/// As 9 colunas de `images` (sem md5/sha256: ficam no banco, fora do wire).
const ICOLS: &str = "id, filename, object_key, bytes, width, height, media_type, split, created_at";

/// Limite por-arquivo, D2/D10 reason too_large: ao exceder, o handler para
/// de escrever no disco mas drena a stream até EOF e marca rejected/too_large.
pub const MAX_FILE_BYTES: i64 = 200 * 1024 * 1024;

fn stored_item(
    image_id: Uuid,
    filename: String,
    bytes: i64,
    width: i32,
    height: i32,
) -> UploadItem {
    UploadItem {
        image_id: Some(image_id.to_string()),
        filename,
        status: "stored".to_string(),
        reason: None,
        bytes: Some(bytes),
        width: Some(width),
        height: Some(height),
    }
}

fn duplicate_item(image_id: Uuid, filename: String) -> UploadItem {
    UploadItem {
        image_id: Some(image_id.to_string()),
        filename,
        status: "duplicate".to_string(),
        reason: Some("duplicate_filename".to_string()),
        bytes: None,
        width: None,
        height: None,
    }
}

fn rejected_item(filename: String, reason: &'static str) -> UploadItem {
    UploadItem {
        image_id: None,
        filename,
        status: "rejected".to_string(),
        reason: Some(reason.to_string()),
        bytes: None,
        width: None,
        height: None,
    }
}

fn failed_item(filename: String) -> UploadItem {
    UploadItem {
        image_id: None,
        filename,
        status: "failed".to_string(),
        reason: Some("storage_error".to_string()),
        bytes: None,
        width: None,
        height: None,
    }
}

/// Erro opaco do axum: `LengthLimitError` no debug (estouro do
/// `DefaultBodyLimit` do CORPO TOTAL) ⇒ 413; o resto é stream morto.
fn is_too_large(err: &axum::extract::multipart::MultipartError) -> bool {
    format!("{err:?}").contains("LengthLimit")
}

/// POST /api/datasets/:id/upload — lote multipart `files` (spool+sniff+hash, D7).
///
/// Disco só efêmero (D1): cada arquivo faz spool num `tempfile` apagado no
/// drop. Ordem objeto→linha→compensação (D7): o PUT precede o INSERT; em
/// `duplicate` o objeto recém-enviado é deletado best-effort. PUT que falha
/// interrompe tudo com 503 `storage_unavailable` (D10): itens já committed
/// do lote permanecem, sem rollback de banco.
///
/// Erro de stream (`next_field`/`chunk`): no axum 0.7.9 o `DefaultBodyLimit`
/// embrulha o corpo inteiro e o multer nunca fuseja — após o primeiro `Err`,
/// `next_field()` retorna `Err` para sempre. Por isso nunca há `continue`
/// pós-`Err`: `LengthLimitError` ⇒ `return` 413 no envelope (itens já
/// committed do lote permanecem); outro erro ⇒ `break` (lote parcial
/// responde com o que existe; vazio ⇒ o 400 do passo 9).
/// Erro duro de banco no INSERT/SELECT ⇒ `return` 500 (lote abortado; itens
/// anteriores committed). `UploadItem.filename` = nome canônico server-side
/// (stem sanitizado + extensão do sniff), não o do form.
pub async fn upload(
    State(state): State<AppState>,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> Response {
    // 1. Dataset existe? (id não-UUID ⇒ mesmo 404, padrão 3a).
    let ds_id: Uuid = match parse_id(&id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    let exists: bool =
        match sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM datasets WHERE id = $1)")
            .bind(ds_id)
            .fetch_one(&state.pool)
            .await
        {
            Ok(v) => v,
            Err(_) => return internal(),
        };
    if !exists {
        return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND);
    }

    tracing::info!(%ds_id, "upload: iniciando processamento de lote multipart");

    let mut items: Vec<UploadItem> = Vec::new();
    // 2. Loop de fields.
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                if is_too_large(&e) {
                    tracing::warn!(%ds_id, "upload: corpo multipart excede o limite global (PAYLOAD_TOO_LARGE)");
                    return err(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "invalid_request",
                        MSG_INVALID_REQUEST,
                    );
                }
                tracing::warn!(%ds_id, error = ?e, "upload: erro ao ler proximo campo multipart");
                break;
            }
        };
        let raw_name = match field.file_name() {
            Some(n) => n.to_string(),
            None => continue,
        };
        if field.name() != Some("files") {
            continue;
        }
        let filename = keys::sanitize_filename(&raw_name);

        // 3. Spool em tempfile (stream chunk → write_all até EOF).
        let tmp = match tempfile::NamedTempFile::new() {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(%ds_id, file = %raw_name, error = ?e, "upload: falha ao criar tempfile");
                items.push(failed_item(filename));
                continue;
            }
        };
        let tmp_path = tmp.path().to_path_buf();
        let mut out = match tokio::fs::File::create(&tmp_path).await {
            Ok(f) => f,
            Err(e) => {
                tracing::error!(%ds_id, file = %raw_name, error = ?e, "upload: falha ao abrir tempfile para escrita");
                items.push(failed_item(filename));
                continue;
            }
        };
        // Spool com teto por-arquivo (MAX_FILE_BYTES): ao exceder, para de
        // escrever mas drena `field.chunk()` até EOF sem acumular em RAM
        // (só soma `n`) e finaliza rejected/too_large. Erro de chunk:
        // `LengthLimitError` ⇒ return 413 (backstop do lote); outro erro ⇒
        // failed + break (stream morto, não dá pra seguir o lote).
        let mut spool: Result<(), &'static str> = Ok(());
        {
            use tokio::io::AsyncWriteExt;
            let mut field = field;
            let mut total: i64 = 0;
            let mut over = false;
            loop {
                match field.chunk().await {
                    Ok(Some(bytes)) => {
                        total += bytes.len() as i64;
                        if total > MAX_FILE_BYTES {
                            over = true;
                            continue;
                        }
                        if out.write_all(&bytes).await.is_err() {
                            spool = Err("io");
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        if is_too_large(&e) {
                            tracing::warn!(%ds_id, file = %raw_name, "upload: limite global excedido durante chunk");
                            return err(
                                StatusCode::PAYLOAD_TOO_LARGE,
                                "invalid_request",
                                MSG_INVALID_REQUEST,
                            );
                        }
                        spool = Err("dead");
                        break;
                    }
                }
            }
            let _ = out.flush().await;
            if over {
                spool = Err("too_large");
            }
        }
        match spool {
            Ok(()) => {}
            Err("too_large") => {
                tracing::warn!(%ds_id, file = %raw_name, limit_bytes = MAX_FILE_BYTES, "upload: arquivo rejeitado (too_large)");
                items.push(rejected_item(filename, "too_large"));
                continue;
            }
            Err("dead") => {
                tracing::error!(%ds_id, file = %raw_name, "upload: stream de upload corrompida ou interrompida");
                items.push(failed_item(filename));
                break;
            }
            Err(_) => {
                tracing::error!(%ds_id, file = %raw_name, "upload: erro de I/O no spool temporario");
                items.push(failed_item(filename));
                continue;
            }
        }

        // 4. Normalização para WebP, higienização de metadados e hashing (ADR-0017).
        let raw_bytes = match tokio::fs::read(&tmp_path).await {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(%ds_id, file = %raw_name, error = ?e, "upload: falha ao ler bytes do tempfile");
                items.push(failed_item(filename));
                continue;
            }
        };

        let norm = match super::normalize::normalize_image(&raw_bytes) {
            Ok(n) => n,
            Err(
                e @ (super::normalize::NormalizeError::UnsupportedMedia
                | super::normalize::NormalizeError::DecodeFailed),
            ) => {
                tracing::warn!(%ds_id, file = %raw_name, error = %e, "upload: imagem rejeitada por formato invalido ou incompativel");
                items.push(rejected_item(filename, "unsupported_media"));
                continue;
            }
            Err(e) => {
                tracing::error!(%ds_id, file = %raw_name, error = %e, "upload: falha na normalizacao da imagem");
                items.push(failed_item(filename));
                continue;
            }
        };

        // Sobrescreve o tempfile com os bytes WebP normalizados.
        if let Err(e) = tokio::fs::write(&tmp_path, &norm.webp_bytes).await {
            tracing::error!(%ds_id, file = %raw_name, error = ?e, "upload: falha ao sobrescrever tempfile com webp normalizado");
            items.push(failed_item(filename));
            continue;
        }

        // 5. Chave e Nome Canônico por Hash MD5 ({md5}.webp).
        let canonical = norm.filename;
        let image_id = Uuid::new_v4();
        let key = keys::image_object_key(ds_id, image_id, &canonical);
        if let Err(e) = state.storage.put(&key, &tmp_path).await {
            tracing::error!(%ds_id, file = %raw_name, key = %key, error = %e, "upload: falha ao persistir no storage");
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "storage_unavailable",
                MSG_STORAGE_UNAVAILABLE,
            );
        }

        let bytes = norm.webp_bytes.len() as i64;
        let width = norm.width;
        let height = norm.height;
        let md5hex = norm.md5;
        let shahex = norm.sha256;

        // 6. INSERT com ON CONFLICT DO NOTHING (reenvio do mesmo hash ⇒ duplicate).
        let inserted: Option<Uuid> = match sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO images (id, dataset_id, filename, object_key, bytes, width, height, md5, sha256, media_type) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
             ON CONFLICT (dataset_id, filename) WHERE deleted_at IS NULL DO NOTHING RETURNING id",
        )
        .bind(image_id)
        .bind(ds_id)
        .bind(&canonical)
        .bind(&key)
        .bind(bytes)
        .bind(width)
        .bind(height)
        .bind(&md5hex)
        .bind(&shahex)
        .bind("webp")
        .fetch_optional(&state.pool)
        .await
        {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(%ds_id, file = %raw_name, canonical = %canonical, error = %e, "upload: erro no banco de dados ao registrar imagem");
                let _ = state.storage.delete(&key).await;
                return internal();
            }
        };
        match inserted {
            Some(id) => {
                tracing::info!(%ds_id, image_id = %id, file = %raw_name, canonical = %canonical, bytes, width, height, "upload: imagem normalizada e armazenada");
                items.push(stored_item(id, canonical, bytes, width, height));
            }
            None => {
                // Compensação D7 best-effort (falha do delete: ignora).
                let _ = state.storage.delete(&key).await;
                let existing: Result<Option<Uuid>, _> = sqlx::query_scalar(
                    "SELECT id FROM images WHERE dataset_id = $1 AND filename = $2 AND deleted_at IS NULL",
                )
                .bind(ds_id)
                .bind(&canonical)
                .fetch_optional(&state.pool)
                .await;
                match existing {
                    Ok(Some(id)) => {
                        tracing::info!(%ds_id, existing_id = %id, file = %raw_name, canonical = %canonical, "upload: imagem duplicada detectada (mesmo hash MD5)");
                        items.push(duplicate_item(id, canonical));
                    }
                    Ok(None) => {
                        tracing::warn!(%ds_id, file = %raw_name, canonical = %canonical, "upload: conflito de insercao mas registro existente nao foi localizado");
                        items.push(failed_item(canonical));
                    }
                    Err(e) => {
                        tracing::error!(%ds_id, canonical = %canonical, error = %e, "upload: erro ao consultar imagem duplicada");
                        return internal();
                    }
                }
            }
        }
        // `tmp` morre aqui: drop apaga o spool (D1, disco só efêmero).
    }

    let stored_count = items.iter().filter(|i| i.status == "stored").count();
    let dup_count = items.iter().filter(|i| i.status == "duplicate").count();
    let rej_count = items.iter().filter(|i| i.status == "rejected").count();
    let fail_count = items.iter().filter(|i| i.status == "failed").count();

    tracing::info!(
        %ds_id,
        total = items.len(),
        stored = stored_count,
        duplicates = dup_count,
        rejected = rej_count,
        failed = fail_count,
        "upload: lote multipart finalizado"
    );

    // 9. Códigos do lote: vazio ⇒ 400; todo-rejected/unsupported ⇒ 400;
    // senão 200 mesmo com `rejected`/`failed` individuais.
    if items.is_empty()
        || items
            .iter()
            .all(|i| i.status == "rejected" && i.reason.as_deref() == Some("unsupported_media"))
    {
        return err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            MSG_INVALID_REQUEST,
        );
    }
    // 10. Disparo da indexação (3f.4, ADR-0004 D4): fire-and-forget das
    // imagens `stored` do lote — a resposta não espera; erros são logados
    // dentro do indexer (best-effort R4).
    let stored_ids: Vec<Uuid> = items
        .iter()
        .filter(|i| i.status == "stored")
        .filter_map(|i| i.image_id.as_deref().and_then(|s| s.parse().ok()))
        .collect();
    if !stored_ids.is_empty() {
        let st = state.clone();
        tokio::spawn(async move {
            let wrote =
                crate::search::indexer::index_dataset_images(st, ds_id, Some(stored_ids)).await;
            tracing::info!(%ds_id, wrote, "upload: embeddings indexados");
        });
    }
    (StatusCode::OK, Json(UploadResult { items })).into_response()
}

#[derive(serde::Deserialize)]
pub struct ImageQuery {
    split: Option<String>,
    labeled: Option<String>,
    limit: Option<String>,
    offset: Option<String>,
    deleted: Option<String>,
    #[serde(alias = "classId")]
    class_id: Option<String>,
    tag: Option<String>,
}

/// URL híbrida D3 (3b.5): com `public_endpoint` configurado é presigned
/// (assinatura local, sem rede); sem ele, fallback incondicional para a
/// rota `/data`. `Err` = resposta 503 `storage_unavailable` já montada.
/// `pub(crate)` para reutilização pela busca (3f.5: mesmo wire `Image`).
pub(crate) async fn image_url(
    state: &AppState,
    object_key: &str,
    ds_id: Uuid,
    img_id: Uuid,
) -> Result<String, Response> {
    if state.storage_config.public_endpoint.is_some() {
        match state.storage.presign_get(object_key).await {
            Ok(u) => Ok(u),
            Err(_) => Err(err(
                StatusCode::SERVICE_UNAVAILABLE,
                "storage_unavailable",
                MSG_STORAGE_UNAVAILABLE,
            )),
        }
    } else {
        Ok(format!("/api/datasets/{ds_id}/images/{img_id}/data"))
    }
}

/// GET /api/datasets/:id/images — página de imagens com filtros.
///
/// `url` por imagem via `image_url` (D3): presigned com endpoint público
/// (falha ⇒ 503 no request inteiro); sem ele, fallback incondicional
/// `GET …/images/:imageId/data` (sempre disponível).
pub async fn list_images(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ImageQuery>,
) -> Response {
    let ds_id: Uuid = match parse_id(&id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    // Parse manual p/ 400 no envelope (nada tipado no extractor).
    let bad = || {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            MSG_INVALID_REQUEST,
        )
    };
    let split: Option<String> = match q.split {
        None => None,
        Some(s) if s == "train" || s == "val" => Some(s),
        Some(_) => return bad(),
    };
    let labeled: Option<bool> = match q.labeled {
        None => None,
        Some(s) if s.eq_ignore_ascii_case("true") => Some(true),
        Some(s) if s.eq_ignore_ascii_case("false") => Some(false),
        Some(_) => return bad(),
    };
    // Lixeira (3g.2): `deleted=true` lista só soft-deleted; default `false`
    // filtra `deleted_at IS NULL` (comportamento de sempre, agora explícito).
    // Parse igual ao `labeled` — inválido ⇒ 400 no envelope.
    let deleted: bool = match q.deleted {
        None => false,
        Some(s) if s.eq_ignore_ascii_case("true") => true,
        Some(s) if s.eq_ignore_ascii_case("false") => false,
        Some(_) => return bad(),
    };
    let limit: i64 = match q.limit.map(|s| s.parse::<i64>()) {
        None => 50,
        Some(Ok(v)) if (1..=200).contains(&v) => v,
        _ => return bad(),
    };
    let offset: i64 = match q.offset.map(|s| s.parse::<i64>()) {
        None => 0,
        Some(Ok(v)) if v >= 0 => v,
        _ => return bad(),
    };
    let class_id: Option<Uuid> = match q.class_id {
        None => None,
        Some(ref s) if s.trim().is_empty() => None,
        Some(ref s) => match s.parse::<Uuid>() {
            Ok(u) => Some(u),
            Err(_) => return bad(),
        },
    };
    let tag: Option<String> = match q.tag {
        None => None,
        Some(ref s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else if trimmed.chars().count() > 200 {
                return bad();
            } else {
                Some(trimmed.to_string())
            }
        }
    };

    // Formato do dataset (R9 como no trigger da 0003) + existência (404).
    let format: Option<String> =
        match sqlx::query_scalar("SELECT format FROM datasets WHERE id = $1")
            .bind(ds_id)
            .fetch_optional(&state.pool)
            .await
        {
            Ok(v) => v,
            Err(_) => return internal(),
        };
    let format = match format {
        Some(f) => f,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };

    let mut qb = sqlx::QueryBuilder::new(format!(
        "SELECT {ICOLS}, count(*) OVER() AS total__, \
         (SELECT count(*) FROM boxes b WHERE b.image_id = i.id) AS boxes_count__, \
         (SELECT c.text FROM captions c WHERE c.image_id = i.id) AS caption__ \
         FROM images i WHERE i.dataset_id = "
    ));
    qb.push_bind(ds_id);
    qb.push(if deleted {
        " AND i.deleted_at IS NOT NULL"
    } else {
        " AND i.deleted_at IS NULL"
    });
    if let Some(s) = split {
        qb.push(" AND i.split = ");
        qb.push_bind(s);
    }
    if let Some(cid) = class_id {
        qb.push(" AND EXISTS (SELECT 1 FROM boxes b WHERE b.image_id = i.id AND b.class_id = ");
        qb.push_bind(cid);
        qb.push(")");
    }
    if let Some(t) = tag {
        let pattern = format!("%{t}%");
        qb.push(" AND (EXISTS (SELECT 1 FROM boxes b JOIN classes c ON c.id = b.class_id WHERE b.image_id = i.id AND c.name ILIKE ");
        qb.push_bind(pattern.clone());
        qb.push(
            ") OR EXISTS (SELECT 1 FROM captions cap WHERE cap.image_id = i.id AND cap.text ILIKE ",
        );
        qb.push_bind(pattern.clone());
        qb.push(") OR i.filename ILIKE ");
        qb.push_bind(pattern);
        qb.push(")");
    }
    if let Some(lab) = labeled {
        // Taxonomia R9 do trigger da 0003: yolo_txt ⇒ boxes, demais ⇒ captions.
        let table = if format == "yolo_txt" {
            "boxes b"
        } else {
            "captions c"
        };
        let col = if format == "yolo_txt" {
            "b.image_id"
        } else {
            "c.image_id"
        };
        qb.push(if lab {
            " AND EXISTS (SELECT 1 FROM "
        } else {
            " AND NOT EXISTS (SELECT 1 FROM "
        });
        qb.push(table);
        qb.push(" WHERE ");
        qb.push(col);
        qb.push(" = i.id)");
    }
    qb.push(" ORDER BY i.created_at DESC, i.id DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    type ImgTuple = (
        Uuid,
        String,
        String,
        i64,
        i32,
        i32,
        String,
        String,
        chrono::DateTime<chrono::Utc>,
        i64,
        Option<i64>,
        Option<String>,
    );
    let rows: Vec<ImgTuple> = match qb.build_query_as().fetch_all(&state.pool).await {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    let total: i64 = rows.first().map(|r| r.9).unwrap_or(0);
    let mut out: Vec<ImageResponse> = Vec::with_capacity(rows.len());
    for (
        img_id,
        filename,
        object_key,
        bytes,
        width,
        height,
        media_type,
        split,
        created_at,
        _total,
        boxes_count,
        caption,
    ) in rows
    {
        let url = match image_url(&state, &object_key, ds_id, img_id).await {
            Ok(u) => u,
            Err(resp) => return resp,
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
        resp.boxes_count = boxes_count;
        resp.caption = caption;
        out.push(resp);
    }
    (
        StatusCode::OK,
        Json(ImagePage {
            items: out,
            total,
            limit,
            offset,
        }),
    )
        .into_response()
}

/// GET /api/datasets/:id/images/:imageId — detalhe (Image + boxes + caption).
///
/// A query da imagem já escopa por `dataset_id` (sem `SELECT EXISTS`
/// separado): linha ausente ⇒ 404. `presign_get` é assinatura local (sem
/// rede), por isso este handler não declara 503. Erro de banco em qualquer
/// step ⇒ `internal()`.
pub async fn get_image(
    State(state): State<AppState>,
    Path((id, image_id)): Path<(String, String)>,
) -> Response {
    let ds_id: Uuid = match parse_id(&id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    let img_id: Uuid = match parse_id(&image_id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    type ImgTuple = (
        Uuid,
        String,
        String,
        i64,
        i32,
        i32,
        String,
        String,
        chrono::DateTime<chrono::Utc>,
    );
    let row: Option<ImgTuple> = match sqlx::query_as(&format!(
        "SELECT {ICOLS} FROM images WHERE id = $1 AND dataset_id = $2 AND deleted_at IS NULL"
    ))
    .bind(img_id)
    .bind(ds_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    let (img_id, filename, object_key, bytes, width, height, media_type, split, created_at) =
        match row {
            Some(r) => r,
            None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
        };
    type BoxTuple = (
        Uuid,
        Uuid,
        f64,
        f64,
        f64,
        f64,
        Option<f64>,
        String,
        Option<i32>,
    );
    let box_rows: Vec<BoxTuple> = match sqlx::query_as(
        "SELECT id, class_id, x, y, w, h, conf, origin, track_id FROM boxes WHERE image_id = $1 ORDER BY id",
    )
    .bind(img_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    type CaptionTuple = (
        String,
        String,
        Option<String>,
        chrono::DateTime<chrono::Utc>,
    );
    let caption_row: Option<CaptionTuple> = match sqlx::query_as(
        "SELECT text, origin, model, updated_at FROM captions WHERE image_id = $1",
    )
    .bind(img_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    let url = match image_url(&state, &object_key, ds_id, img_id).await {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let detail = ImageDetailResponse::from((
        ImageRow {
            id: img_id,
            filename,
            object_key,
            bytes,
            width,
            height,
            media_type,
            split,
            created_at,
        },
        box_rows.into_iter().map(BoxResponse::from).collect(),
        caption_row.map(CaptionResponse::from),
        url,
    ));
    (StatusCode::OK, Json(detail)).into_response()
}

/// GET /api/datasets/:id/images/:imageId/data — proxy do objeto (fallback
/// incondicional da D3, funciona SEM flag pública).
///
/// Linha sem objeto no storage é estado proibido pela D7, mas a rota
/// responde honesto: `NotFound` ⇒ 404, `Unavailable` ⇒ 503.
pub async fn get_data(
    State(state): State<AppState>,
    Path((id, image_id)): Path<(String, String)>,
) -> Response {
    let ds_id: Uuid = match parse_id(&id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    let img_id: Uuid = match parse_id(&image_id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    let row: Option<(String, String)> = match sqlx::query_as(
        "SELECT object_key, media_type FROM images WHERE id = $1 AND dataset_id = $2 AND deleted_at IS NULL",
    )
    .bind(img_id)
    .bind(ds_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    let (object_key, media_type) = match row {
        Some(r) => r,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    let bytes = match state.storage.get(&object_key).await {
        Ok(b) => b,
        Err(StorageError::NotFound) => {
            return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND);
        }
        Err(StorageError::Unavailable(_)) => {
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "storage_unavailable",
                MSG_STORAGE_UNAVAILABLE,
            );
        }
    };
    // Colunas CHECK limitam a jpeg|png|webp; o fallback é defesa.
    let content_type: &str = match media_type.as_str() {
        "jpeg" => MediaType::Jpeg.content_type(),
        "png" => MediaType::Png.content_type(),
        "webp" => MediaType::WebP.content_type(),
        _ => "application/octet-stream",
    };
    (
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, content_type),
            (
                axum::http::header::CACHE_CONTROL,
                "private, max-age=31536000, immutable",
            ),
        ],
        bytes,
    )
        .into_response()
}

/// Body JSON no envelope (cópia do `create`): o rejection padrão do axum
/// seria 413/400 `text/plain` fora do envelope D6; aqui excesso de limite
/// vira 413 `invalid_request` e qualquer outro erro de buffer ou parse vira
/// 400 `invalid_request`.
fn parse_json_body<T>(body: Result<Bytes, BytesRejection>) -> Result<T, Response>
where
    T: serde::de::DeserializeOwned,
{
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
    match serde_json::from_slice(&body) {
        Ok(v) => Ok(v),
        Err(_) => Err(err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            MSG_INVALID_REQUEST,
        )),
    }
}

/// PUT /api/datasets/:id/images/:imageId/boxes — substituição total (3b.6).
///
/// Ordem LEI: (a) parse ds/image uuid ⇒ não ⇒ 404; (b) body no envelope ⇒
/// 413/400; (c) validação pura ⇒ 400; (d) imagem escopada ao dataset ⇒
/// 404; (e) classes do dataset (`count` sobre ids dedup; classe de outro
/// dataset NÃO vaza existência — 400 seco); (f) transação DELETE + INSERT
/// com RETURNING (erro ⇒ rollback + 500); (g) 200 canônico.
/// Os triggers de contagem disparam por linha — recálculo idempotente.
pub async fn put_boxes(
    State(state): State<AppState>,
    Path((id, image_id)): Path<(String, String)>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    // (a) uuid antes de tudo (nunca 400 — ADR-0002 D8).
    let ds_id: Uuid = match parse_id(&id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    let img_id: Uuid = match parse_id(&image_id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    // (b) body no envelope.
    let req: PutBoxesRequest = match parse_json_body(body) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    // (c) validação pura (sem DB).
    let valid = match validate_boxes(&req) {
        Ok(v) => v,
        Err(_) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                MSG_INVALID_REQUEST,
            );
        }
    };
    // (d) imagem existe ESCOPADA ao dataset.
    let exists: bool = match sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM images WHERE id = $1 AND dataset_id = $2 AND deleted_at IS NULL)",
    )
    .bind(img_id)
    .bind(ds_id)
    .fetch_one(&state.pool)
    .await
    {
        Ok(v) => v,
        Err(_) => return internal(),
    };
    if !exists {
        return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND);
    }
    // (e) todas as classes pertencem ao dataset (dedup antes do count).
    if !valid.is_empty() {
        let mut distinct: Vec<Uuid> = valid.iter().map(|b| b.0).collect();
        distinct.sort();
        distinct.dedup();
        let n: i64 = match sqlx::query_scalar(
            "SELECT count(*) FROM classes WHERE dataset_id = $1 AND id = ANY($2)",
        )
        .bind(ds_id)
        .bind(&distinct)
        .fetch_one(&state.pool)
        .await
        {
            Ok(v) => v,
            Err(_) => return internal(),
        };
        if n != distinct.len() as i64 {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                MSG_INVALID_REQUEST,
            );
        }
    }
    // (f) transação: DELETE total + INSERT em massa com RETURNING.
    let mut tx = match state.pool.begin().await {
        Ok(t) => t,
        Err(_) => return internal(),
    };
    if sqlx::query("DELETE FROM boxes WHERE image_id = $1")
        .bind(img_id)
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return internal();
    }
    type BoxTuple = (
        Uuid,
        Uuid,
        f64,
        f64,
        f64,
        f64,
        Option<f64>,
        String,
        Option<i32>,
    );
    let rows: Vec<BoxTuple> = if valid.is_empty() {
        Vec::new()
    } else {
        let class_ids: Vec<Uuid> = valid.iter().map(|b| b.0).collect();
        let xs: Vec<f64> = valid.iter().map(|b| b.1).collect();
        let ys: Vec<f64> = valid.iter().map(|b| b.2).collect();
        let ws: Vec<f64> = valid.iter().map(|b| b.3).collect();
        let hs: Vec<f64> = valid.iter().map(|b| b.4).collect();
        let confs: Vec<Option<f64>> = valid.iter().map(|b| b.5).collect();
        let origins: Vec<String> = valid.iter().map(|b| b.6.clone()).collect();
        let tracks: Vec<Option<i32>> = valid.iter().map(|b| b.7).collect();
        match sqlx::query_as(
            "INSERT INTO boxes (image_id, class_id, x, y, w, h, conf, origin, track_id) \
             SELECT $1, t.class_id, t.x, t.y, t.w, t.h, t.conf, t.origin, t.track_id \
             FROM unnest($2::uuid[], $3::float8[], $4::float8[], $5::float8[], $6::float8[], $7::float8[], $8::text[], $9::int[]) \
             AS t(class_id, x, y, w, h, conf, origin, track_id) \
             RETURNING id, class_id, x, y, w, h, conf, origin, track_id",
        )
        .bind(img_id)
        .bind(&class_ids)
        .bind(&xs)
        .bind(&ys)
        .bind(&ws)
        .bind(&hs)
        .bind(&confs)
        .bind(&origins)
        .bind(&tracks)
        .fetch_all(&mut *tx)
        .await
        {
            Ok(r) => r,
            Err(_) => return internal(),
        }
    };
    if tx.commit().await.is_err() {
        return internal();
    }
    // (g) 200 canônico (ordem de inserção; client ordena por id p/ estabilidade).
    let resp = PutBoxesResponse {
        boxes: rows.into_iter().map(BoxResponse::from).collect(),
    };
    (StatusCode::OK, Json(resp)).into_response()
}

/// PUT /api/datasets/:id/images/:imageId/caption — upsert (3b.6).
///
/// Mesma espinha do PUT boxes: (a) uuid ⇒ 404; (b) body no envelope ⇒
/// 413/400; (c) `text` 1..=8000 chars + `origin` no domínio ⇒ 400 ANTES do
/// banco (`''` nunca nasce como linha); (d) imagem escopada ⇒ 404;
/// (e) n/a (sem classes); (f) upsert + releitura; (g) 200.
pub async fn put_caption(
    State(state): State<AppState>,
    Path((id, image_id)): Path<(String, String)>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    // (a) uuid antes de tudo.
    let ds_id: Uuid = match parse_id(&id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    let img_id: Uuid = match parse_id(&image_id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    // (b) body no envelope.
    let req: PutCaptionRequest = match parse_json_body(body) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    // (c) validação pura (text vazio ⇒ 400, a linha não nasce).
    let (text, origin, model) =
        match validate_caption(&req.text, req.origin.as_deref(), req.model.as_deref()) {
            Ok(v) => v,
            Err(_) => {
                return err(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    MSG_INVALID_REQUEST,
                );
            }
        };
    // (d) imagem existe ESCOPADA ao dataset.
    let exists: bool = match sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM images WHERE id = $1 AND dataset_id = $2 AND deleted_at IS NULL)",
    )
    .bind(img_id)
    .bind(ds_id)
    .fetch_one(&state.pool)
    .await
    {
        Ok(v) => v,
        Err(_) => return internal(),
    };
    if !exists {
        return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND);
    }
    // (f) upsert de statement ÚNICO com RETURNING (revisão 3b.6): releitura
    // separada poderia ecoar linha de writer concorrente; o RETURNING já vem
    // com o `updated_at` do trigger `tg_set_updated_at` aplicado.
    type CaptionTuple = (
        String,
        String,
        Option<String>,
        chrono::DateTime<chrono::Utc>,
    );
    let row: Option<CaptionTuple> = match sqlx::query_as(
        "INSERT INTO captions (image_id, text, origin, model) VALUES ($1,$2,$3,$4) \
         ON CONFLICT (image_id) DO UPDATE SET text = EXCLUDED.text, origin = EXCLUDED.origin, model = EXCLUDED.model \
         RETURNING text, origin, model, updated_at",
    )
    .bind(img_id)
    .bind(&text)
    .bind(&origin)
    .bind(model)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    let row = match row {
        Some(r) => r,
        None => return internal(),
    };
    // (g) 200 canônico (struct da 3b.5, reaproveitada).
    (StatusCode::OK, Json(CaptionResponse::from(row))).into_response()
}

/// PUT /api/datasets/:id/classes — substituição total com reconciliação por
/// id (3g.1, ADR-0005 D2/D3).
///
/// Ordem LEI: (a) uuid do dataset ⇒ não ⇒ 404; (b) body no envelope ⇒
/// 413/400; (c) dataset existe ⇒ não ⇒ 404; (d) validação pura + plano
/// (`plan_classes` sobre as classes atuais: id de outro dataset ⇒ 400 seco);
/// (e+f) transação ÚNICA: guard de órfãos DENTRO dela (primeira instrução)
/// ⇒ 409 `classes_in_use` sem escrever nada; fase 1 (tmp `__tmp_<simple>`,
/// idx+1000000); DELETE das removidas (libera os slots de idx); fase 2
/// (name/idx/color finais); INSERT das novas;
/// (g) 200 canônico (releitura ordenada por idx).
pub async fn put_classes(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    // (a) uuid antes de tudo (nunca 400 — ADR-0002 D8).
    let ds_id: Uuid = match parse_id(&id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    // (b) body no envelope.
    let req: PutClassesRequest = match parse_json_body(body) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    // (c) dataset existe escopado ao id.
    let exists: bool =
        match sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM datasets WHERE id = $1)")
            .bind(ds_id)
            .fetch_one(&state.pool)
            .await
        {
            Ok(v) => v,
            Err(_) => return internal(),
        };
    if !exists {
        return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND);
    }
    // Classes atuais do dataset (ids para o plano).
    let existing: Vec<Uuid> =
        match sqlx::query_scalar("SELECT id FROM classes WHERE dataset_id = $1")
            .bind(ds_id)
            .fetch_all(&state.pool)
            .await
        {
            Ok(v) => v,
            Err(_) => return internal(),
        };
    // (d) validação pura + reconciliação (id estranho ⇒ 400 seco).
    let plan = match plan_classes(&existing, &req) {
        Ok(p) => p,
        Err(_) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                MSG_INVALID_REQUEST,
            );
        }
    };
    // (e+f) transação ÚNICA: o guard de órfãos roda DENTRO dela, como
    // primeira instrução — 409 aborta antes de qualquer write (TOCTOU
    // fechado; o CASCADE de `boxes.class_id` nunca apaga anotação).
    let mut tx = match state.pool.begin().await {
        Ok(t) => t,
        Err(_) => return internal(),
    };
    if !plan.remove.is_empty() {
        let hit: bool =
            match sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM boxes WHERE class_id = ANY($1))")
                .bind(&plan.remove)
                .fetch_one(&mut *tx)
                .await
            {
                Ok(v) => v,
                Err(_) => return internal(),
            };
        if hit {
            return err(StatusCode::CONFLICT, "classes_in_use", MSG_CLASSES_IN_USE);
        }
    }
    // Fase 1: mantidos para nome/idx temporários (fora de qualquer colisão).
    for (cid, _, final_idx) in &plan.keep {
        let tmp_name = format!("__tmp_{}", cid.simple());
        let tmp_idx = *final_idx as i32 + 1_000_000;
        if sqlx::query("UPDATE classes SET name = $1, idx = $2 WHERE id = $3 AND dataset_id = $4")
            .bind(&tmp_name)
            .bind(tmp_idx)
            .bind(cid)
            .bind(ds_id)
            .execute(&mut *tx)
            .await
            .is_err()
        {
            return internal();
        }
    }
    // DELETE das removidas ANTES da fase 2: com as removidas fora, os
    // slots de idx ficam livres e a renumeração 0..n-1 não colide com o
    // UNIQUE(dataset_id, idx) (remover idx menor + manter idx maior ⇒ 500).
    if !plan.remove.is_empty() {
        if sqlx::query("DELETE FROM classes WHERE dataset_id = $1 AND id = ANY($2)")
            .bind(ds_id)
            .bind(&plan.remove)
            .execute(&mut *tx)
            .await
            .is_err()
        {
            return internal();
        }
    }
    // Fase 2: mantidos para name/idx/color finais (ordem do array).
    for (cid, name, final_idx) in &plan.keep {
        let color = color_for(*final_idx);
        if sqlx::query(
            "UPDATE classes SET name = $1, idx = $2, color = $3 WHERE id = $4 AND dataset_id = $5",
        )
        .bind(name)
        .bind(*final_idx as i32)
        .bind(color)
        .bind(cid)
        .bind(ds_id)
        .execute(&mut *tx)
        .await
        .is_err()
        {
            return internal();
        }
    }
    // INSERT das novas (id pelo default do banco).
    for (name, final_idx) in &plan.create {
        let color = color_for(*final_idx);
        if sqlx::query("INSERT INTO classes (dataset_id, name, idx, color) VALUES ($1, $2, $3, $4)")
            .bind(ds_id)
            .bind(name)
            .bind(*final_idx as i32)
            .bind(color)
            .execute(&mut *tx)
            .await
            .is_err()
        {
            return internal();
        }
    }
    if tx.commit().await.is_err() {
        return internal();
    }
    // (g) 200 canônico (releitura ordenada por idx, mesmo shape de Dataset.classes).
    let rows: Vec<(Uuid, String, i32, String)> = match sqlx::query_as(
        "SELECT id, name, idx, color FROM classes WHERE dataset_id = $1 ORDER BY idx",
    )
    .bind(ds_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    let resp = PutClassesResponse {
        classes: rows.into_iter().map(DatasetClassResponse::from).collect(),
    };
    (StatusCode::OK, Json(resp)).into_response()
}

/// POST /api/datasets/:id/boxes/batch — atualização ou remoção em lote de bounding boxes.
pub async fn batch_update_boxes(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    // (a) uuid antes de tudo (404 not_found)
    let ds_id: Uuid = match parse_id(&id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    // (b) body no envelope
    let req: BatchBoxesUpdateRequest = match parse_json_body(body) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    // (c) validação pura
    if validate_batch_boxes_update(&req).is_err() {
        return err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            MSG_INVALID_REQUEST,
        );
    }
    // (d) dataset existe escopado ao id
    let exists: bool =
        match sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM datasets WHERE id = $1)")
            .bind(ds_id)
            .fetch_one(&state.pool)
            .await
        {
            Ok(v) => v,
            Err(_) => return internal(),
        };
    if !exists {
        return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND);
    }
    // (e) classes pertencem ao dataset
    let source_exists: bool = match sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM classes WHERE id = $1 AND dataset_id = $2)",
    )
    .bind(req.source_class_id)
    .bind(ds_id)
    .fetch_one(&state.pool)
    .await
    {
        Ok(v) => v,
        Err(_) => return internal(),
    };
    if !source_exists {
        return err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            MSG_INVALID_REQUEST,
        );
    }

    if let Some(target_id) = req.target_class_id {
        let target_exists: bool = match sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM classes WHERE id = $1 AND dataset_id = $2)",
        )
        .bind(target_id)
        .bind(ds_id)
        .fetch_one(&state.pool)
        .await
        {
            Ok(v) => v,
            Err(_) => return internal(),
        };
        if !target_exists {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                MSG_INVALID_REQUEST,
            );
        }
    }

    // (f) se image_ids informado, valida que todas pertencem ao dataset e estão ativas
    if let Some(ref img_ids) = req.image_ids {
        let mut distinct = img_ids.clone();
        distinct.sort();
        distinct.dedup();
        let count: i64 = match sqlx::query_scalar(
            "SELECT count(*) FROM images WHERE dataset_id = $1 AND deleted_at IS NULL AND id = ANY($2)",
        )
        .bind(ds_id)
        .bind(&distinct)
        .fetch_one(&state.pool)
        .await
        {
            Ok(v) => v,
            Err(_) => return internal(),
        };
        if count != distinct.len() as i64 {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                MSG_INVALID_REQUEST,
            );
        }
    }

    // (g) transação para a mutação
    let mut tx = match state.pool.begin().await {
        Ok(t) => t,
        Err(_) => return internal(),
    };

    let affected_image_ids: Vec<Uuid> = match (req.action.as_str(), &req.image_ids) {
        ("remap", Some(ids)) => {
            let target_id = req.target_class_id.unwrap();
            match sqlx::query_scalar::<_, Uuid>(
                "UPDATE boxes SET class_id = $1 WHERE class_id = $2 AND image_id = ANY($3) RETURNING image_id",
            )
            .bind(target_id)
            .bind(req.source_class_id)
            .bind(ids)
            .fetch_all(&mut *tx)
            .await
            {
                Ok(r) => r,
                Err(_) => return internal(),
            }
        }
        ("remap", None) => {
            let target_id = req.target_class_id.unwrap();
            match sqlx::query_scalar::<_, Uuid>(
                "UPDATE boxes SET class_id = $1 WHERE class_id = $2 AND image_id IN (SELECT id FROM images WHERE dataset_id = $3 AND deleted_at IS NULL) RETURNING image_id",
            )
            .bind(target_id)
            .bind(req.source_class_id)
            .bind(ds_id)
            .fetch_all(&mut *tx)
            .await
            {
                Ok(r) => r,
                Err(_) => return internal(),
            }
        }
        ("delete", Some(ids)) => {
            match sqlx::query_scalar::<_, Uuid>(
                "DELETE FROM boxes WHERE class_id = $1 AND image_id = ANY($2) RETURNING image_id",
            )
            .bind(req.source_class_id)
            .bind(ids)
            .fetch_all(&mut *tx)
            .await
            {
                Ok(r) => r,
                Err(_) => return internal(),
            }
        }
        ("delete", None) => {
            match sqlx::query_scalar::<_, Uuid>(
                "DELETE FROM boxes WHERE class_id = $1 AND image_id IN (SELECT id FROM images WHERE dataset_id = $2 AND deleted_at IS NULL) RETURNING image_id",
            )
            .bind(req.source_class_id)
            .bind(ds_id)
            .fetch_all(&mut *tx)
            .await
            {
                Ok(r) => r,
                Err(_) => return internal(),
            }
        }
        _ => return internal(),
    };

    if tx.commit().await.is_err() {
        return internal();
    }

    let affected_boxes = affected_image_ids.len() as i64;
    let mut unique_imgs = affected_image_ids;
    unique_imgs.sort();
    unique_imgs.dedup();
    let affected_images = unique_imgs.len() as i64;

    let resp = BatchBoxesUpdateResponse {
        affected_boxes,
        affected_images,
    };
    (StatusCode::OK, Json(resp)).into_response()
}
