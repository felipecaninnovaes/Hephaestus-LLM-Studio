//! Handlers da galeria de gerações (G.6b — ADR-0023 D5).
//!
//! BFF do manager: lista/delete/export com presigned URLs condicionais
//! e proxy de imagem via StoragePort.

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::error::{err, MSG_INVALID_REQUEST, MSG_QUEUE_UNAVAILABLE, MSG_STORAGE_UNAVAILABLE};
use crate::jobs::manager_client::{InternalGeneration, ManagerError};
use crate::state::AppState;
use crate::storage::StorageError;

// ---------------------------------------------------------------------------
// Wire types (camelCase — ADR-0002 D1)
// ---------------------------------------------------------------------------

/// Generation pública (camelCase wire — ADR-0023 D6).
#[derive(Debug, serde::Serialize)]
pub struct Generation {
    pub id: String,
    #[serde(rename = "jobId")]
    pub job_id: String,
    pub filename: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(rename = "thumbUrl", skip_serializing_if = "Option::is_none")]
    pub thumb_url: Option<String>,
    pub width: i32,
    pub height: i32,
    pub seed: i64,
    pub prompt: String,
    #[serde(rename = "negativePrompt", skip_serializing_if = "Option::is_none")]
    pub negative_prompt: Option<String>,
    pub params: serde_json::Value,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

/// Query params de GET /api/generations.
#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub deleted: bool,
    #[serde(rename = "baseModel")]
    pub base_model: Option<String>,
    #[serde(default)]
    pub quantization: Option<String>,
}

fn default_limit() -> i64 {
    50
}

/// Body de POST /api/generations/delete e /api/generations/export.
#[derive(Debug, Deserialize)]
pub struct GenerationIdsRequest {
    pub ids: Vec<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn queue_unavailable() -> Response {
    err(
        StatusCode::SERVICE_UNAVAILABLE,
        "queue_unavailable",
        MSG_QUEUE_UNAVAILABLE,
    )
}

fn storage_unavailable() -> Response {
    err(
        StatusCode::SERVICE_UNAVAILABLE,
        "storage_unavailable",
        MSG_STORAGE_UNAVAILABLE,
    )
}

fn invalid_request() -> Response {
    err(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        MSG_INVALID_REQUEST,
    )
}

fn not_found() -> Response {
    err(StatusCode::NOT_FOUND, "not_found", "generation not found")
}

/// Converte InternalGeneration → Generation com presigned URLs condicionais.
///
/// `S3_PUBLIC_ENDPOINT_URL` setado ⇒ presigned URLs; senão NULL.
async fn to_public(gen: &InternalGeneration, state: &AppState) -> Generation {
    let presign = state.storage_config.public_endpoint.is_some();
    let url = if presign {
        state.storage.presign_get(&gen.s3_key).await.ok()
    } else {
        None
    };
    let thumb_url: Option<String> = if presign {
        if let Some(ref key) = gen.thumb_s3_key {
            state.storage.presign_get(key).await.ok()
        } else {
            None
        }
    } else {
        None
    };
    Generation {
        id: gen.id.clone(),
        job_id: gen.job_id.clone(),
        filename: gen.filename.clone(),
        url,
        thumb_url,
        width: gen.width,
        height: gen.height,
        seed: gen.seed,
        prompt: gen.prompt.clone(),
        negative_prompt: gen.negative_prompt.clone(),
        params: gen.params.clone(),
        created_at: gen.created_at.clone(),
    }
}

/// Content-Type pela extensão do arquivo.
fn content_type_for(filename: &str) -> &'static str {
    let lower = filename.to_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

/// Valida UUID.
fn parse_uuid(s: &str) -> Option<Uuid> {
    Uuid::parse_str(s).ok()
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/generations — lista gerações da galeria.
///
/// Query params camelCase: limit (1..200, default 50), offset (≥0),
/// baseModel (string, opcional), deleted (bool, default false),
/// quantization (opcional — filtrado em memória sobre os items retornados).
pub async fn list_generations(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Response {
    let limit = query.limit.clamp(1, 200);
    let offset = query.offset.max(0);

    let (items, total) = match state
        .manager
        .list_generations(limit, offset, query.deleted, query.base_model.as_deref())
        .await
    {
        Ok(r) => r,
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };

    // Filtragem de quantization em memória (manager não suporta este filtro).
    let filtered: Vec<&InternalGeneration> = if let Some(ref quant) = query.quantization {
        items
            .iter()
            .filter(|g| {
                g.params
                    .get("quantization")
                    .and_then(|v| v.as_str())
                    .map(|q| q == quant.as_str())
                    .unwrap_or(false)
            })
            .collect()
    } else {
        items.iter().collect()
    };

    // Ajuste honesto de total quando quantization filtra.
    let effective_total = if query.quantization.is_some() {
        filtered.len() as i64
    } else {
        total
    };

    // Mapeia para Generation (presigned URLs condicionais).
    // NOTA: to_public é async (presign_get), então precisamos de um loop async.
    let mut generations = Vec::with_capacity(filtered.len());
    for gen in filtered {
        generations.push(to_public(gen, &state).await);
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "items": generations,
            "total": effective_total,
        })),
    )
        .into_response()
}

/// GET /api/generations/:id/data — proxy binário da imagem gerada.
///
/// Stream do objeto via StoragePort com Content-Type pela extensão.
pub async fn get_generation_data(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let _uid = match parse_uuid(&id) {
        Some(u) => u,
        None => return not_found(),
    };

    // Busca generation via manager.
    // Pendência: GET /internal/generations/:id não existe no manager ainda.
    // Se o manager retornar 404 (rota ausente ou generation inexistente),
    // tratamos como not_found — honesto e temporário.
    let gen = match state.manager.get_generation(&id).await {
        Ok(g) => g,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };

    // Stream do objeto via StoragePort (padrão proxy de imagens — ADR-0003 D3).
    let bytes = match state.storage.get(&gen.s3_key).await {
        Ok(b) => b,
        Err(StorageError::NotFound) => return storage_unavailable(),
        Err(StorageError::Unavailable(_)) => return storage_unavailable(),
    };

    let ct = content_type_for(&gen.filename);
    (
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, ct.to_string()),
            (
                axum::http::header::CACHE_CONTROL,
                "private, max-age=31536000, immutable".to_string(),
            ),
        ],
        bytes,
    )
        .into_response()
}

/// POST /api/generations/delete — soft-delete em lote (≤100 IDs, idempotente).
pub async fn delete_generations(
    State(state): State<AppState>,
    Json(body): Json<GenerationIdsRequest>,
) -> Response {
    // Validação: 1..100 IDs, todos UUIDs.
    if body.ids.is_empty() || body.ids.len() > 100 {
        return invalid_request();
    }
    for id in &body.ids {
        if parse_uuid(id).is_none() {
            return invalid_request();
        }
    }

    match state.manager.delete_generations(&body.ids).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(ManagerError::Unavailable(_)) => queue_unavailable(),
        Err(ManagerError::InvalidRequest(_)) => invalid_request(),
        Err(_) => queue_unavailable(),
    }
}

/// POST /api/generations/export — exporta gerações como ZIP (≤100 IDs).
///
/// Para cada ID: get_generation → get_to_file → zip com entradas
/// `{job8}_{filename}` (padrão ADR-0006: get_to_file → zip em
/// spawn_blocking → ReaderStream).
pub async fn export_generations(
    State(state): State<AppState>,
    Json(body): Json<GenerationIdsRequest>,
) -> Response {
    // Validação: 1..100 IDs, todos UUIDs.
    if body.ids.is_empty() || body.ids.len() > 100 {
        return invalid_request();
    }
    for id in &body.ids {
        if parse_uuid(id).is_none() {
            return invalid_request();
        }
    }

    // 1. Busca todas as generations via manager.
    let mut generations = Vec::with_capacity(body.ids.len());
    for id in &body.ids {
        match state.manager.get_generation(id).await {
            Ok(g) => generations.push(g),
            Err(ManagerError::NotFound) => {
                // ID inexistente: skip honesto (padrão idempotente).
                continue;
            }
            Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
            Err(_) => return queue_unavailable(),
        }
    }

    if generations.is_empty() {
        return invalid_request();
    }

    // 2. Download para tempdir via StoragePort::get_to_file.
    let tmp = match tempfile::TempDir::new() {
        Ok(d) => d,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            );
        }
    };

    let mut zip_entries: Vec<(String, std::path::PathBuf)> = Vec::new();

    for gen in &generations {
        let dest = tmp.path().join(&gen.filename);
        match state.storage.get_to_file(&gen.s3_key, &dest).await {
            Ok(()) => {
                // Arcname: {job_id primeiros 8 chars}_{filename}
                let job_short = if gen.job_id.len() >= 8 {
                    &gen.job_id[..8]
                } else {
                    &gen.job_id
                };
                let arcname = format!("{job_short}_{}", gen.filename);
                zip_entries.push((arcname, dest));
            }
            Err(StorageError::NotFound) => {
                // Objeto ausente: skip + eprintln (padrão ADR-0006).
                eprintln!(
                    "aviso: export pulou geração {} ({}) — objeto ausente",
                    gen.id, gen.filename
                );
            }
            Err(StorageError::Unavailable(_)) => return storage_unavailable(),
        }
    }

    if zip_entries.is_empty() {
        return err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "no generations could be exported",
        );
    }

    // 3. Zip em spawn_blocking (padrão ADR-0006).
    let zip_path = tmp.path().join("generations.zip");
    let zip_entries_blocking = zip_entries.clone();
    let zip_path_blocking = zip_path.clone();
    let blocking: Result<Result<(), std::io::Error>, tokio::task::JoinError> =
        tokio::task::spawn_blocking(move || {
            let file = std::fs::File::create(&zip_path_blocking)?;
            let mut zip = zip::ZipWriter::new(file);
            for (arcname, fs_path) in &zip_entries_blocking {
                let options = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored);
                zip.start_file(arcname, options)?;
                let mut src = std::fs::File::open(fs_path)?;
                std::io::copy(&mut src, &mut zip)?;
            }
            zip.finish()?;
            Ok(())
        })
        .await;

    match blocking {
        Ok(Ok(())) => {}
        _ => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            );
        }
    }

    // 4. Stream do zip via ReaderStream.
    let len = match tokio::fs::metadata(&zip_path).await {
        Ok(m) => m.len(),
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            );
        }
    };
    let file = match tokio::fs::File::open(&zip_path).await {
        Ok(f) => f,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            );
        }
    };
    let stream = tokio_util::io::ReaderStream::new(file);
    let body = Body::from_stream(stream);
    (
        StatusCode::OK,
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/zip".to_string(),
            ),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"generations.zip\"".to_string(),
            ),
            (axum::http::header::CONTENT_LENGTH, len.to_string()),
        ],
        body,
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::manager_client::{InternalGeneration, MockManager};
    use crate::state::AppState;
    use crate::storage::MockStorage;

    fn test_state(manager: MockManager) -> AppState {
        AppState {
            pool: sqlx::PgPool::connect_lazy("postgres://n/n").expect("lazy"),
            jwt_secret: [0x42; 32],
            secure_cookie: false,
            setup_required: false,
            storage: std::sync::Arc::new(MockStorage::new()),
            storage_config: crate::storage::StorageConfig {
                bucket: "heph-test".into(),
                public_endpoint: None,
                url_ttl_secs: 60,
            },
            embedder: std::sync::Arc::new(crate::search::MockEmbedder::new()),
            embedding_model: "ViT-B-32".to_string(),
            manager: std::sync::Arc::new(manager),
            model_download_allowed_hosts: vec![],
        }
    }

    fn make_generation(id: &str, job_id: &str) -> InternalGeneration {
        InternalGeneration {
            id: id.to_string(),
            job_id: job_id.to_string(),
            s3_key: format!("artifacts/{job_id}/generated.png"),
            thumb_s3_key: Some(format!("artifacts/{job_id}/thumb.jpg")),
            filename: "generated.png".to_string(),
            seed: 42,
            prompt: "a test".to_string(),
            negative_prompt: None,
            width: 512,
            height: 512,
            params: serde_json::json!({"base_model": "flux-2-klein-4b"}),
            created_at: "2026-09-15T00:00:00Z".to_string(),
            deleted_at: None,
        }
    }

    // --- list_generations ---

    #[tokio::test]
    async fn list_generations_200_empty() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = list_generations(
            axum::extract::State(state),
            Query(ListQuery {
                limit: 50,
                offset: 0,
                deleted: false,
                base_model: None,
                quantization: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn list_generations_200_with_items() {
        let mock = MockManager::default();
        mock.generations_by_id.write().unwrap().insert(
            "11111111-1111-1111-1111-111111111111".to_string(),
            make_generation(
                "11111111-1111-1111-1111-111111111111",
                "22222222-2222-2222-2222-222222222222",
            ),
        );
        let state = test_state(mock);
        let resp = list_generations(
            axum::extract::State(state),
            Query(ListQuery {
                limit: 50,
                offset: 0,
                deleted: false,
                base_model: None,
                quantization: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["items"].as_array().unwrap().len(), 1);
        assert_eq!(json["total"], 1);
    }

    #[tokio::test]
    async fn list_generations_503_manager_offline() {
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let resp = list_generations(
            axum::extract::State(state),
            Query(ListQuery {
                limit: 50,
                offset: 0,
                deleted: false,
                base_model: None,
                quantization: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn list_generations_quantization_filter() {
        let mock = MockManager::default();
        let mut g1 = make_generation(
            "11111111-1111-1111-1111-111111111111",
            "22222222-2222-2222-2222-222222222222",
        );
        g1.params = serde_json::json!({"base_model": "flux-2-klein-4b", "quantization": "4bit"});
        let mut g2 = make_generation(
            "33333333-3333-3333-3333-333333333333",
            "44444444-4444-4444-4444-444444444444",
        );
        g2.params = serde_json::json!({"base_model": "flux-2-klein-4b", "quantization": "8bit"});
        {
            let mut gens = mock.generations_by_id.write().unwrap();
            gens.insert("11111111-1111-1111-1111-111111111111".to_string(), g1);
            gens.insert("33333333-3333-3333-3333-333333333333".to_string(), g2);
        }
        let state = test_state(mock);
        let resp = list_generations(
            axum::extract::State(state),
            Query(ListQuery {
                limit: 50,
                offset: 0,
                deleted: false,
                base_model: None,
                quantization: Some("4bit".to_string()),
            }),
        )
        .await;
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["items"].as_array().unwrap().len(), 1);
        assert_eq!(json["total"], 1);
    }

    // --- delete_generations ---

    #[tokio::test]
    async fn delete_generations_204_ok() {
        let mock = MockManager::default();
        mock.generations_by_id.write().unwrap().insert(
            "11111111-1111-1111-1111-111111111111".to_string(),
            make_generation(
                "11111111-1111-1111-1111-111111111111",
                "22222222-2222-2222-2222-222222222222",
            ),
        );
        let state = test_state(mock);
        let body = GenerationIdsRequest {
            ids: vec!["11111111-1111-1111-1111-111111111111".to_string()],
        };
        let resp = delete_generations(axum::extract::State(state), Json(body)).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn delete_generations_400_empty_ids() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let body = GenerationIdsRequest { ids: vec![] };
        let resp = delete_generations(axum::extract::State(state), Json(body)).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn delete_generations_400_too_many() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let ids: Vec<String> = (0..101).map(|_| uuid::Uuid::new_v4().to_string()).collect();
        let body = GenerationIdsRequest { ids };
        let resp = delete_generations(axum::extract::State(state), Json(body)).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn delete_generations_400_invalid_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let body = GenerationIdsRequest {
            ids: vec!["not-a-uuid".to_string()],
        };
        let resp = delete_generations(axum::extract::State(state), Json(body)).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn delete_generations_503_manager_offline() {
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let body = GenerationIdsRequest {
            ids: vec![uuid::Uuid::new_v4().to_string()],
        };
        let resp = delete_generations(axum::extract::State(state), Json(body)).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // --- get_generation_data ---

    #[tokio::test]
    async fn get_generation_data_404_invalid_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp =
            get_generation_data(axum::extract::State(state), Path("not-a-uuid".to_string())).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_generation_data_404_not_found() {
        let mut mock = MockManager::default();
        mock.get_generation_not_found = true;
        let state = test_state(mock);
        let resp = get_generation_data(
            axum::extract::State(state),
            Path(uuid::Uuid::new_v4().to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_generation_data_503_manager_offline() {
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let resp = get_generation_data(
            axum::extract::State(state),
            Path(uuid::Uuid::new_v4().to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // --- content_type_for ---

    #[test]
    fn content_type_png() {
        assert_eq!(content_type_for("img.png"), "image/png");
    }

    #[test]
    fn content_type_jpeg() {
        assert_eq!(content_type_for("img.jpg"), "image/jpeg");
        assert_eq!(content_type_for("img.jpeg"), "image/jpeg");
    }

    #[test]
    fn content_type_webp() {
        assert_eq!(content_type_for("img.webp"), "image/webp");
    }

    #[test]
    fn content_type_unknown() {
        assert_eq!(content_type_for("model.bin"), "application/octet-stream");
    }
}
