//! Handlers de visualização e persistência de anotações (AutoTracker e AutoLabel).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use uuid::Uuid;

use super::helpers::{
    dataset_not_ready, invalid_request, job_not_done, not_found, parse_uuid, queue_unavailable,
    storage_unavailable, validate_artifact_path,
};
use super::types::AutotrackerApplyResponse;
use crate::error::{err, MSG_STORAGE_UNAVAILABLE};
use crate::jobs::manager_client::ManagerError;
use crate::jobs::models;
use crate::state::AppState;
use crate::storage::StorageError;

/// POST /api/jobs/:id/autotracker/apply — ingest de boxes no principal.
///
/// Status: 200 | 400 `invalid_request` | 404 `not_found` | 409 `job_not_done`
///         | 409 `dataset_not_ready` | 503 `queue_unavailable`/`storage_unavailable`.
///
/// Fluxo (ADR-0008 D1):
/// 1. Busca job no manager → valida engine/status/dataset_id
/// 2. Localiza artefato `boxes.json` via `list_artifacts`, valida path, lê via
///    `StoragePort.get`, confere md5
/// 3. Parse do JSON, resolve filename→image_id, class→class_id
/// 4. Escrita por imagem (transação DELETE+INSERT) com merge por origem
pub async fn apply_autotracker_boxes(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    // 0. Parse job id — não-UUID ⇒ 404.
    if parse_uuid(&id).is_none() {
        return not_found();
    }

    // 1. Parse body.
    let raw = match body {
        Ok(b) => b,
        Err(_) => return invalid_request(),
    };
    let req: models::AutotrackerApplyRequest = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 1a. Se imageId fornecido, valida UUID — não-UUID ⇒ 400 (campo de body).
    let filter_image_id: Option<Uuid> = match &req.image_id {
        Some(s) => match s.parse::<Uuid>() {
            Ok(v) => Some(v),
            Err(_) => return invalid_request(),
        },
        None => None,
    };

    // 2. Busca job no manager.
    let job = match state.manager.get_job(&id).await {
        Ok(j) => j,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };

    // 2a. Valida engine == 'autotracker'.
    if job.engine != "autotracker" {
        return not_found();
    }

    // 2b. Valida status == 'done'.
    if job.status != "done" {
        return job_not_done();
    }

    // 2c. Valida dataset_id presente.
    let dataset_id_str = match &job.dataset_id {
        Some(s) => s.clone(),
        None => return dataset_not_ready(),
    };
    let dataset_id: Uuid = match dataset_id_str.parse() {
        Ok(v) => v,
        Err(_) => return dataset_not_ready(),
    };

    // 3. Localiza artefato `boxes.json` via list_artifacts.
    let artifacts = match state.manager.list_artifacts(&id).await {
        Ok(a) => a,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let boxes_artifact = match artifacts.iter().find(|a| a.kind == "boxes") {
        Some(a) => a,
        None => return not_found(),
    };

    // 3a. Valida path do artefato (defesa em profundidade).
    if let Err(resp) = validate_artifact_path(&boxes_artifact.path) {
        return resp;
    }

    // 3b. Lê objeto via StoragePort (admin).
    let key = format!("artifacts/{id}/{}", boxes_artifact.path);
    let bytes = match state.storage.get(&key).await {
        Ok(b) => b,
        Err(StorageError::NotFound) => {
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "storage_unavailable",
                MSG_STORAGE_UNAVAILABLE,
            );
        }
        Err(StorageError::Unavailable(_)) => return storage_unavailable(),
    };

    // 3c. Confere md5.
    let computed = format!(
        "{:x}",
        md5::Digest::finalize({
            use md5::Digest;
            let mut h = md5::Md5::new();
            md5::Digest::update(&mut h, &bytes);
            h
        })
    );
    if computed != boxes_artifact.md5 {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_unavailable",
            MSG_STORAGE_UNAVAILABLE,
        );
    }

    // 4. Parse do JSON.
    let artifact = match models::parse_boxes_json(&bytes) {
        Ok(a) => a,
        Err(_) => return invalid_request(),
    };

    // 5. Busca imagens ativas do dataset (filename → image_id).
    let image_rows: Vec<(Uuid, String)> = match sqlx::query_as::<_, (Uuid, String)>(
        "SELECT id, filename FROM images WHERE dataset_id = $1 AND deleted_at IS NULL",
    )
    .bind(dataset_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };
    let filename_to_id: std::collections::HashMap<String, Uuid> = image_rows
        .iter()
        .map(|(id, fname)| (fname.clone(), *id))
        .collect();

    // 5a. Se filter_image_id definido, valida que é imagem ATIVA do dataset.
    if let Some(fid) = filter_image_id {
        if !filename_to_id.values().any(|id| *id == fid) {
            return not_found();
        }
    }

    // 6. Busca classes do dataset (name → class_id).
    let class_rows: Vec<(Uuid, String)> = match sqlx::query_as::<_, (Uuid, String)>(
        "SELECT id, name FROM classes WHERE dataset_id = $1",
    )
    .bind(dataset_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };
    let mut class_map = models::resolve_class_ids(&class_rows);
    let mut class_map_lower: std::collections::HashMap<String, Uuid> = class_rows
        .iter()
        .map(|(id, name)| (name.to_lowercase(), *id))
        .collect();

    // 6a. Se create_missing_classes fornecido, cria as novas classes no dataset.
    if let Some(ref to_create) = req.create_missing_classes {
        for name in to_create {
            let trimmed = name.trim();
            if !crate::datasets::models::is_valid_class_name(trimmed) {
                return invalid_request();
            }
            let lower = trimmed.to_lowercase();
            if !class_map_lower.contains_key(&lower) {
                // Checa teto de 200 classes
                let current_count: i64 =
                    match sqlx::query_scalar("SELECT count(*) FROM classes WHERE dataset_id = $1")
                        .bind(dataset_id)
                        .fetch_one(&state.pool)
                        .await
                    {
                        Ok(c) => c,
                        Err(_) => {
                            return err(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "internal",
                                "internal server error",
                            )
                        }
                    };
                if current_count >= 200 {
                    return invalid_request();
                }
                let next_idx: i32 = match sqlx::query_scalar(
                    "SELECT COALESCE(MAX(idx), -1) + 1 FROM classes WHERE dataset_id = $1",
                )
                .bind(dataset_id)
                .fetch_one(&state.pool)
                .await
                {
                    Ok(i) => i,
                    Err(_) => {
                        return err(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "internal",
                            "internal server error",
                        )
                    }
                };
                let color = crate::datasets::models::color_for(next_idx as usize);
                let new_class_id = Uuid::new_v4();
                if sqlx::query("INSERT INTO classes (id, dataset_id, name, color, idx) VALUES ($1, $2, $3, $4, $5)")
                    .bind(new_class_id)
                    .bind(dataset_id)
                    .bind(trimmed)
                    .bind(color)
                    .bind(next_idx)
                    .execute(&state.pool)
                    .await.is_err() {
                        return err(StatusCode::INTERNAL_SERVER_ERROR, "internal", "internal server error");
                    }
                class_map.insert(trimmed.to_string(), new_class_id);
                class_map_lower.insert(lower, new_class_id);
            }
        }
    }

    // 7. Processa cada imagem: uma transação por imagem (ADR-0008 D1a).
    let mut total_applied: i64 = 0;
    let mut total_skipped: i64 = 0;
    let mut images_with_boxes: i64 = 0;

    for engine_image in &artifact.images {
        // Resolve filename → image_id (SEMPRE por filename, mesmo com filter_image_id).
        let Some(image_id) = filename_to_id.get(&engine_image.filename) else {
            // Imagem inexistente/deletada → skip.
            total_skipped += engine_image.boxes.len() as i64;
            continue;
        };
        // Filtra: se filter_image_id definido, só processa essa imagem.
        if let Some(fid) = filter_image_id {
            if *image_id != fid {
                continue;
            }
        }

        // Resolve class names → class_ids, coletando skippadas.
        let mut valid_boxes: Vec<(Uuid, f64, f64, f64, f64, Option<f64>, String, Option<i32>)> =
            Vec::new();
        for eb in &engine_image.boxes {
            match models::match_class_id(&eb.class, &class_map, &class_map_lower) {
                Some(class_id) => {
                    valid_boxes.push((
                        *class_id,
                        eb.x,
                        eb.y,
                        eb.w,
                        eb.h,
                        Some(eb.conf),
                        "autotracker".to_string(),
                        None,
                    ));
                }
                None => {
                    // Classe inexistente → skip.
                    total_skipped += 1;
                }
            }
        }

        // Cap 1000/imagem.
        if valid_boxes.len() > 1000 {
            let excess = valid_boxes.len() - 1000;
            total_skipped += excess as i64;
            valid_boxes.truncate(1000);
        }

        // Transação por imagem: DELETE + INSERT.
        // DELETE SEMPRE ocorre quando a imagem está presente no artefato
        // (mesmo que valid_boxes fique vazio — semântica last-write-wins por
        // origem: se o engine EMITIU boxes para a imagem, as anteriores da
        // mesma origem devem ser removidas).
        let mut tx = match state.pool.begin().await {
            Ok(t) => t,
            Err(_) => {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                )
            }
        };

        if req.overwrite {
            // DELETE total da imagem.
            if sqlx::query("DELETE FROM boxes WHERE image_id = $1")
                .bind(image_id)
                .execute(&mut *tx)
                .await
                .is_err()
            {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                );
            }
        } else {
            // DELETE só das boxes de origem autotracker.
            if sqlx::query("DELETE FROM boxes WHERE image_id = $1 AND origin = 'autotracker'")
                .bind(image_id)
                .execute(&mut *tx)
                .await
                .is_err()
            {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                );
            }
        }

        // INSERT em massa — apenas se há boxes válidas.
        if !valid_boxes.is_empty() {
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
            let inserted: Vec<BoxTuple> = {
                let class_ids: Vec<Uuid> = valid_boxes.iter().map(|b| b.0).collect();
                let xs: Vec<f64> = valid_boxes.iter().map(|b| b.1).collect();
                let ys: Vec<f64> = valid_boxes.iter().map(|b| b.2).collect();
                let ws: Vec<f64> = valid_boxes.iter().map(|b| b.3).collect();
                let hs: Vec<f64> = valid_boxes.iter().map(|b| b.4).collect();
                let confs: Vec<Option<f64>> = valid_boxes.iter().map(|b| b.5).collect();
                let origins: Vec<String> = valid_boxes.iter().map(|b| b.6.clone()).collect();
                let tracks: Vec<Option<i32>> = valid_boxes.iter().map(|b| b.7).collect();
                match sqlx::query_as::<_, BoxTuple>(
                    "INSERT INTO boxes (image_id, class_id, x, y, w, h, conf, origin, track_id) \
                     SELECT $1, t.class_id, t.x, t.y, t.w, t.h, t.conf, t.origin, t.track_id \
                     FROM unnest($2::uuid[], $3::float8[], $4::float8[], $5::float8[], $6::float8[], $7::float8[], $8::text[], $9::int[]) \
                     AS t(class_id, x, y, w, h, conf, origin, track_id) \
                     RETURNING id, class_id, x, y, w, h, conf, origin, track_id",
                )
                .bind(image_id)
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
                    Err(_) => {
                        return err(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "internal",
                            "internal server error",
                        )
                    }
                }
            };

            let count = inserted.len() as i64;
            total_applied += count;
            if count > 0 {
                images_with_boxes += 1;
            }
        }

        // Sempre commita — a fase DELETE ocorreu mesmo sem INSERT.
        if tx.commit().await.is_err() {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            );
        }
    }

    // 8. Resposta 200.
    (
        StatusCode::OK,
        Json(AutotrackerApplyResponse {
            applied: total_applied,
            skipped: total_skipped,
            images: images_with_boxes,
        }),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// GET /api/jobs/:id/autotracker/preview — prévia e análise de classes do autotracker
// ---------------------------------------------------------------------------

/// GET /api/jobs/:id/autotracker/preview — retorna resumo de detecções com análise de classes existentes e ausentes.
///
/// Status: 200 | 401 | 404 `not_found` | 409 `job_not_done` | 503 `queue_unavailable` | 503 `storage_unavailable`.
pub async fn preview_autotracker_boxes(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    // 0. Parse job id — não-UUID ⇒ 404.
    if parse_uuid(&id).is_none() {
        return not_found();
    }

    // 1. Busca job no manager.
    let job = match state.manager.get_job(&id).await {
        Ok(j) => j,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };

    // 1a. Valida engine == 'autotracker'.
    if job.engine != "autotracker" {
        return not_found();
    }

    // 1b. Valida status == 'done'.
    if job.status != "done" {
        return job_not_done();
    }

    // 1c. Valida dataset_id presente.
    let dataset_id_str = match &job.dataset_id {
        Some(s) => s.clone(),
        None => return dataset_not_ready(),
    };
    let dataset_id: Uuid = match dataset_id_str.parse() {
        Ok(v) => v,
        Err(_) => return dataset_not_ready(),
    };

    // 2. Localiza artefato `boxes.json` via list_artifacts.
    let artifacts = match state.manager.list_artifacts(&id).await {
        Ok(a) => a,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let boxes_artifact = match artifacts.iter().find(|a| a.kind == "boxes") {
        Some(a) => a,
        None => return not_found(),
    };

    if let Err(resp) = validate_artifact_path(&boxes_artifact.path) {
        return resp;
    }

    // 2b. Lê objeto via StoragePort.
    let key = format!("artifacts/{id}/{}", boxes_artifact.path);
    let bytes = match state.storage.get(&key).await {
        Ok(b) => b,
        Err(StorageError::NotFound) => {
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "storage_unavailable",
                MSG_STORAGE_UNAVAILABLE,
            );
        }
        Err(StorageError::Unavailable(_)) => return storage_unavailable(),
    };

    // 2c. Confere md5.
    let computed = format!(
        "{:x}",
        md5::Digest::finalize({
            use md5::Digest;
            let mut h = md5::Md5::new();
            md5::Digest::update(&mut h, &bytes);
            h
        })
    );
    if computed != boxes_artifact.md5 {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_unavailable",
            MSG_STORAGE_UNAVAILABLE,
        );
    }

    // 3. Parse do JSON.
    let artifact = match models::parse_boxes_json(&bytes) {
        Ok(a) => a,
        Err(_) => return invalid_request(),
    };

    // 4. Busca classes existentes do dataset.
    let class_rows: Vec<(Uuid, String)> = match sqlx::query_as::<_, (Uuid, String)>(
        "SELECT id, name FROM classes WHERE dataset_id = $1",
    )
    .bind(dataset_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };
    let class_map = models::resolve_class_ids(&class_rows);
    let class_map_lower: std::collections::HashMap<String, Uuid> = class_rows
        .iter()
        .map(|(id, name)| (name.to_lowercase(), *id))
        .collect();

    // 5. Agrega contagem de boxes por classe.
    let mut total_boxes: i64 = 0;
    let mut class_counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();

    for img in &artifact.images {
        for b in &img.boxes {
            total_boxes += 1;
            *class_counts.entry(b.class.clone()).or_insert(0) += 1;
        }
    }

    let mut existing_classes = Vec::new();
    let mut missing_classes = Vec::new();

    for (class_name, count) in class_counts {
        if models::match_class_id(&class_name, &class_map, &class_map_lower).is_some() {
            existing_classes.push(models::AutotrackerClassCount {
                name: class_name,
                boxes_count: count,
            });
        } else {
            missing_classes.push(models::AutotrackerClassCount {
                name: class_name,
                boxes_count: count,
            });
        }
    }

    existing_classes.sort_by(|a, b| {
        b.boxes_count
            .cmp(&a.boxes_count)
            .then_with(|| a.name.cmp(&b.name))
    });
    missing_classes.sort_by(|a, b| {
        b.boxes_count
            .cmp(&a.boxes_count)
            .then_with(|| a.name.cmp(&b.name))
    });

    let resp = models::AutotrackerPreviewResponse {
        total_images: artifact.images.len() as i64,
        total_boxes,
        existing_classes,
        missing_classes,
    };

    (StatusCode::OK, Json(resp)).into_response()
}

// ---------------------------------------------------------------------------
// GET /api/jobs/:id/autolabel/preview — prévia de legendas do autolabel
// ---------------------------------------------------------------------------

/// GET /api/jobs/:id/autolabel/preview — retorna prévia das legendas geradas para curadoria humana.
///
/// Status: 200 | 401 | 404 `not_found` | 409 `job_not_done` | 503 `queue_unavailable` | 503 `storage_unavailable`.
pub async fn preview_autolabel_captions(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    // 0. Parse job id — não-UUID ⇒ 404.
    let job_uuid = match parse_uuid(&id) {
        Some(u) => u,
        None => return not_found(),
    };

    // 1. Busca job no manager.
    let job = match state.manager.get_job(&id).await {
        Ok(j) => j,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };

    // 1a. Valida engine == 'autolabel'.
    if job.engine != "autolabel" {
        return not_found();
    }

    // 1b. Valida status == 'done'.
    if job.status != "done" {
        return job_not_done();
    }

    // 1c. Valida dataset_id presente.
    let dataset_id_str = match &job.dataset_id {
        Some(s) => s.clone(),
        None => return dataset_not_ready(),
    };
    let dataset_id: Uuid = match dataset_id_str.parse() {
        Ok(v) => v,
        Err(_) => return dataset_not_ready(),
    };

    // 2. Localiza artefato `captions.jsonl` via list_artifacts.
    let artifacts = match state.manager.list_artifacts(&id).await {
        Ok(a) => a,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let captions_artifact = match artifacts
        .iter()
        .find(|a| a.kind == "captions" || a.path == "captions.jsonl")
    {
        Some(a) => a,
        None => return not_found(),
    };

    if let Err(resp) = validate_artifact_path(&captions_artifact.path) {
        return resp;
    }

    // 2b. Lê objeto via StoragePort.
    let key = format!("artifacts/{id}/{}", captions_artifact.path);
    let bytes = match state.storage.get(&key).await {
        Ok(b) => b,
        Err(StorageError::NotFound) => {
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "storage_unavailable",
                MSG_STORAGE_UNAVAILABLE,
            );
        }
        Err(StorageError::Unavailable(_)) => return storage_unavailable(),
    };

    // 2c. Confere md5.
    let computed = format!(
        "{:x}",
        md5::Digest::finalize({
            use md5::Digest;
            let mut h = md5::Md5::new();
            md5::Digest::update(&mut h, &bytes);
            h
        })
    );
    if computed != captions_artifact.md5 {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_unavailable",
            MSG_STORAGE_UNAVAILABLE,
        );
    }

    // 3. Parse do JSONL.
    let items = match models::parse_captions_jsonl(&bytes) {
        Ok(it) => it,
        Err(_) => return invalid_request(),
    };

    // 4. Busca imagens ativas do dataset (filename, id, object_key).
    let image_rows: Vec<(Uuid, String, String)> = match sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT id, filename, object_key FROM images WHERE dataset_id = $1 AND deleted_at IS NULL",
    )
    .bind(dataset_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };
    let filename_to_image: std::collections::HashMap<String, (Uuid, String)> = image_rows
        .into_iter()
        .map(|(id, fname, okey)| (fname, (id, okey)))
        .collect();

    // 5. Busca captions existentes para estas imagens (image_id -> (text, origin)).
    let existing_captions: std::collections::HashMap<Uuid, (String, String)> = match sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT c.image_id, c.text, c.origin FROM captions c JOIN images i ON i.id = c.image_id WHERE i.dataset_id = $1 AND i.deleted_at IS NULL",
    )
    .bind(dataset_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => rows.into_iter().map(|(id, text, origin)| (id, (text, origin))).collect(),
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };

    // 6. Constrói items de prévia.
    let total_generated = items.len() as i64;
    let mut preview_items = Vec::new();

    for item in items {
        if let Some((image_id, object_key)) = filename_to_image.get(&item.filename) {
            let image_url =
                crate::datasets::handlers::image_url(&state, object_key, dataset_id, *image_id)
                    .await
                    .unwrap_or_default();
            let (curr_text, curr_origin) = match existing_captions.get(image_id) {
                Some((t, o)) => (Some(t.clone()), Some(o.clone())),
                None => (None, None),
            };

            preview_items.push(models::AutolabelPreviewItem {
                image_id: *image_id,
                filename: item.filename,
                image_url,
                generated_caption: item.caption,
                current_caption: curr_text,
                current_origin: curr_origin,
            });
        }
    }

    let resp = models::AutolabelPreviewResponse {
        job_id: job_uuid,
        dataset_id,
        model: Some(job.model),
        total_generated,
        items: preview_items,
    };
    (StatusCode::OK, Json(resp)).into_response()
}

// ---------------------------------------------------------------------------
// POST /api/jobs/:id/autolabel/apply — aplica legendas do autolabel (ADR-0016 D1)
// ---------------------------------------------------------------------------

/// POST /api/jobs/:id/autolabel/apply — aplica legendas do autolabel (ADR-0016 D1).
///
/// Status: 200 | 400 `invalid_request` | 401 | 404 `not_found` |
/// 409 `job_not_done` | 503 `queue_unavailable` | 503 `storage_unavailable`.
pub async fn apply_autolabel_captions(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    // 0. Parse job id — não-UUID ⇒ 404.
    if parse_uuid(&id).is_none() {
        return not_found();
    }

    // 1. Parse body.
    let raw = match body {
        Ok(b) => b,
        Err(_) => return invalid_request(),
    };
    let req: models::AutolabelApplyRequest = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 2. Busca job no manager.
    let job = match state.manager.get_job(&id).await {
        Ok(j) => j,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };

    // 2a. Valida engine == 'autolabel'.
    if job.engine != "autolabel" {
        return not_found();
    }

    // 2b. Valida status == 'done'.
    if job.status != "done" {
        return job_not_done();
    }

    // 2c. Valida dataset_id presente.
    let dataset_id_str = match &job.dataset_id {
        Some(s) => s.clone(),
        None => return dataset_not_ready(),
    };
    let dataset_id: Uuid = match dataset_id_str.parse() {
        Ok(v) => v,
        Err(_) => return dataset_not_ready(),
    };
    if let Some(ds_req) = &req.dataset_id {
        if ds_req != &dataset_id_str {
            return invalid_request();
        }
    }

    // 3. Localiza artefato `captions.jsonl` via list_artifacts.
    let artifacts = match state.manager.list_artifacts(&id).await {
        Ok(a) => a,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let captions_artifact = match artifacts
        .iter()
        .find(|a| a.kind == "captions" || a.path == "captions.jsonl")
    {
        Some(a) => a,
        None => return not_found(),
    };

    if let Err(resp) = validate_artifact_path(&captions_artifact.path) {
        return resp;
    }

    // 3b. Lê objeto via StoragePort.
    let key = format!("artifacts/{id}/{}", captions_artifact.path);
    let bytes = match state.storage.get(&key).await {
        Ok(b) => b,
        Err(StorageError::NotFound) => {
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "storage_unavailable",
                MSG_STORAGE_UNAVAILABLE,
            );
        }
        Err(StorageError::Unavailable(_)) => return storage_unavailable(),
    };

    // 3c. Confere md5.
    let computed = format!(
        "{:x}",
        md5::Digest::finalize({
            use md5::Digest;
            let mut h = md5::Md5::new();
            md5::Digest::update(&mut h, &bytes);
            h
        })
    );
    if computed != captions_artifact.md5 {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_unavailable",
            MSG_STORAGE_UNAVAILABLE,
        );
    }

    // 4. Determina os itens a aplicar:
    // Se o cliente forneceu `req.items` curados, usa esses;
    // senão, faz parse do JSONL original do artefato.
    let target_items: Vec<models::CaptionsJsonlItem> = if let Some(curated) = req.items {
        curated
            .into_iter()
            .map(|c| models::CaptionsJsonlItem {
                filename: c.filename,
                caption: c.caption,
            })
            .collect()
    } else {
        match models::parse_captions_jsonl(&bytes) {
            Ok(it) => it,
            Err(_) => return invalid_request(),
        }
    };

    // 5. Busca imagens ativas do dataset (filename → image_id).
    let image_rows: Vec<(Uuid, String)> = match sqlx::query_as::<_, (Uuid, String)>(
        "SELECT id, filename FROM images WHERE dataset_id = $1 AND deleted_at IS NULL",
    )
    .bind(dataset_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };
    let filename_to_id: std::collections::HashMap<String, Uuid> = image_rows
        .into_iter()
        .map(|(id, fname)| (fname, id))
        .collect();

    // 6. Busca captions existentes para estas imagens (image_id -> origin).
    let existing_captions: std::collections::HashMap<Uuid, String> = match sqlx::query_as::<_, (Uuid, String)>(
        "SELECT c.image_id, c.origin FROM captions c JOIN images i ON i.id = c.image_id WHERE i.dataset_id = $1 AND i.deleted_at IS NULL",
    )
    .bind(dataset_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => rows.into_iter().collect(),
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };

    // 7. Processa itens com merge dirigido por origem (ADR-0016 D1).
    let mut total_applied: i64 = 0;
    let mut total_skipped: i64 = 0;
    let mut applied_images: std::collections::HashSet<Uuid> = std::collections::HashSet::new();

    for item in target_items {
        let Some(&image_id) = filename_to_id.get(&item.filename) else {
            total_skipped += 1;
            continue;
        };

        let trimmed_caption = item.caption.trim();
        if trimmed_caption.is_empty() || trimmed_caption.chars().count() > 8000 {
            total_skipped += 1;
            continue;
        }

        // Se overwrite=false, só atualiza imagens sem caption ou com origin='autolabel'.
        if !req.overwrite {
            if let Some(origin) = existing_captions.get(&image_id) {
                if origin != "autolabel" {
                    total_skipped += 1;
                    continue;
                }
            }
        }

        // UPSERT na tabela captions
        let query_res = sqlx::query(
            "INSERT INTO captions (image_id, text, origin, model, updated_at) \
             VALUES ($1, $2, 'autolabel', $3, now()) \
             ON CONFLICT (image_id) DO UPDATE SET text = EXCLUDED.text, origin = EXCLUDED.origin, model = EXCLUDED.model, updated_at = now()",
        )
        .bind(image_id)
        .bind(trimmed_caption)
        .bind(&job.model)
        .execute(&state.pool)
        .await;

        match query_res {
            Ok(_) => {
                total_applied += 1;
                applied_images.insert(image_id);
            }
            Err(e) => {
                tracing::warn!(error = %e, %image_id, "falha ao executar upsert de caption no autolabel apply");
                total_skipped += 1;
            }
        }
    }

    let resp = models::AutolabelApplyResponse {
        applied: total_applied,
        skipped: total_skipped,
        images: applied_images.len() as i64,
    };
    (StatusCode::OK, Json(resp)).into_response()
}
