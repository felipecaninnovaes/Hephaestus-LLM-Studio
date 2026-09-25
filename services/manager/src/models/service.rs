//! Serviço e repositório do catálogo de modelos (ADR-0012, ADR-0022).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::ManagerError;
use crate::jobs::lifecycle::is_valid_md5;

#[derive(Debug, Clone, Serialize)]
pub struct ModelItem {
    pub id: String,
    pub name: String,
    pub engine: String,
    pub model: Option<String>,
    pub source: String,
    pub hash: String,
    pub bytes: i64,
    pub path: String,
    pub job_id: Option<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelsResponse {
    pub items: Vec<ModelItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateModelRequest {
    pub id: Uuid,
    pub engine: String,
    pub name: String,
    pub model: Option<String>,
    pub s3_key: String,
    pub source: String,
    pub url: Option<String>,
    pub hash: String,
    pub bytes: i64,
    pub job_id: Option<Uuid>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateModelRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StorageUsageResponse {
    pub artifacts_bytes: i64,
    pub models_bytes: i64,
}

type ModelDbRow = (
    Uuid,
    String,
    String,
    Option<String>,
    String,
    String,
    i64,
    String,
    Option<Uuid>,
    DateTime<Utc>,
    Option<String>,
    Option<String>,
);

fn db_row_to_model_item(r: ModelDbRow) -> ModelItem {
    ModelItem {
        id: r.0.to_string(),
        name: r.1,
        engine: r.2,
        model: r.3,
        source: r.4,
        hash: r.5,
        bytes: r.6,
        path: r.7,
        job_id: r.8.map(|u| u.to_string()),
        created_at: r.9.to_rfc3339(),
        kind: r.10,
        arch: r.11,
    }
}
pub fn validate_create_model(req: &CreateModelRequest) -> Result<(), ManagerError> {
    if req.engine != "yolo"
        && req.engine != "world"
        && req.engine != "diffusion"
        && req.engine != "clip"
    {
        return Err(ManagerError::InvalidRequest(format!(
            "engine must be 'yolo', 'world', 'diffusion', or 'clip', got '{}'",
            req.engine
        )));
    }
    if !matches!(req.source.as_str(), "train" | "upload" | "download") {
        return Err(ManagerError::InvalidRequest(format!(
            "source must be 'train', 'upload', or 'download', got '{}'",
            req.source
        )));
    }
    if !is_valid_md5(&req.hash) {
        return Err(ManagerError::InvalidRequest(format!(
            "hash must be a 32-char lowercase hex md5, got '{}'",
            req.hash
        )));
    }
    if req.bytes < 0 {
        return Err(ManagerError::InvalidRequest(format!(
            "bytes must be >= 0, got {}",
            req.bytes
        )));
    }
    if req.name.is_empty() || req.name.len() > 255 {
        return Err(ManagerError::InvalidRequest(format!(
            "name must be between 1 and 255 chars, got {}",
            req.name.len()
        )));
    }
    if (req.kind.is_some() || req.arch.is_some()) && req.engine != "diffusion" {
        return Err(ManagerError::InvalidRequest(format!(
            "kind/arch are only allowed for engine='diffusion', got engine='{}'",
            req.engine
        )));
    }
    if let Some(ref kind) = req.kind {
        if kind != "lora" && kind != "checkpoint" && kind != "text_encoder" {
            return Err(ManagerError::InvalidRequest(format!(
                "kind must be 'lora', 'checkpoint' or 'text_encoder', got '{}'",
                kind
            )));
        }
    }
    if let Some(ref arch) = req.arch {
        if arch != "flux-2-klein-4b" && arch != "sdxl" && arch != "sd15" && arch != "qwen-image-2.1"
        {
            return Err(ManagerError::InvalidRequest(format!(
                "arch must be 'flux-2-klein-4b', 'sdxl', 'sd15', or 'qwen-image-2.1', got '{}'",
                arch
            )));
        }
    }
    if req.kind.as_deref() == Some("checkpoint") && req.arch.is_none() {
        return Err(ManagerError::InvalidRequest(
            "checkpoint requires arch ('flux-2-klein-4b', 'sdxl', 'sd15', or 'qwen-image-2.1')"
                .into(),
        ));
    }
    if req.kind.as_deref() == Some("text_encoder") && req.arch.as_deref() != Some("flux-2-klein-4b")
    {
        return Err(ManagerError::InvalidRequest(
            "text_encoder requires arch 'flux-2-klein-4b'".into(),
        ));
    }
    Ok(())
}

pub fn validate_update_model(req: &UpdateModelRequest) -> Result<(), ManagerError> {
    let clean = req.name.trim();
    if clean.is_empty() || clean.len() > 255 {
        return Err(ManagerError::InvalidRequest(format!(
            "name must be between 1 and 255 chars, got {}",
            clean.len()
        )));
    }
    Ok(())
}

pub async fn list_models(pool: &PgPool) -> Result<ModelsResponse, ManagerError> {
    let rows: Vec<ModelDbRow> = sqlx::query_as(
        "SELECT id, name, engine, model, source, hash, bytes, s3_key, job_id, created_at, kind, arch \
         FROM models ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("list models: {e}")))?;

    let items = rows.into_iter().map(db_row_to_model_item).collect();

    Ok(ModelsResponse { items })
}

pub async fn create_model(
    pool: &PgPool,
    req: CreateModelRequest,
) -> Result<ModelItem, ManagerError> {
    validate_create_model(&req)?;

    let row: Result<Option<ModelDbRow>, sqlx::Error> = sqlx::query_as(
        "INSERT INTO models (id, engine, name, model, s3_key, source, url, hash, bytes, job_id, kind, arch) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) \
         RETURNING id, name, engine, model, source, hash, bytes, s3_key, job_id, created_at, kind, arch",
    )
    .bind(req.id)
    .bind(&req.engine)
    .bind(&req.name)
    .bind(&req.model)
    .bind(&req.s3_key)
    .bind(&req.source)
    .bind(&req.url)
    .bind(&req.hash)
    .bind(req.bytes)
    .bind(req.job_id)
    .bind(&req.kind)
    .bind(&req.arch)
    .fetch_optional(pool)
    .await;

    match row {
        Ok(Some(r)) => Ok(db_row_to_model_item(r)),
        Ok(None) => Err(ManagerError::Internal(
            "insert model: no row returned".into(),
        )),
        Err(e) => {
            if e.as_database_error()
                .map(|db| db.is_unique_violation())
                .unwrap_or(false)
            {
                Err(ManagerError::Internal("model_exists".to_string()))
            } else {
                Err(ManagerError::Internal(format!("insert model: {e}")))
            }
        }
    }
}

pub async fn delete_model(pool: &PgPool, id: Uuid) -> Result<ModelItem, ManagerError> {
    let row: Option<ModelDbRow> = sqlx::query_as(
        "DELETE FROM models WHERE id = $1 \
         RETURNING id, name, engine, model, source, hash, bytes, s3_key, job_id, created_at, kind, arch",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("delete model: {e}")))?;

    match row {
        Some(r) => Ok(db_row_to_model_item(r)),
        None => Err(ManagerError::NotFound),
    }
}

pub async fn update_model(
    pool: &PgPool,
    id: Uuid,
    req: UpdateModelRequest,
) -> Result<ModelItem, ManagerError> {
    validate_update_model(&req)?;
    let clean_name = req.name.trim();

    let row: Option<ModelDbRow> = sqlx::query_as(
        "UPDATE models SET name = $1 WHERE id = $2 \
         RETURNING id, name, engine, model, source, hash, bytes, s3_key, job_id, created_at, kind, arch",
    )
    .bind(clean_name)
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("update model: {e}")))?;
    match row {
        Some(r) => Ok(db_row_to_model_item(r)),
        None => Err(ManagerError::NotFound),
    }
}

pub async fn get_storage_usage(pool: &PgPool) -> Result<StorageUsageResponse, ManagerError> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(bytes), 0)::bigint AS artifacts_bytes FROM job_artifacts WHERE kind <> 'model'",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("storage usage artifacts: {e}")))?;

    let models_row: (i64,) =
        sqlx::query_as("SELECT COALESCE(SUM(bytes), 0)::bigint AS models_bytes FROM models")
            .fetch_one(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("storage usage models: {e}")))?;

    Ok(StorageUsageResponse {
        artifacts_bytes: row.0,
        models_bytes: models_row.0,
    })
}
