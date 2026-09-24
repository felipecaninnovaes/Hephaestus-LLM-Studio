//! Serviço e repositório de gerações de imagem (ADR-0023).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::error::ManagerError;

#[derive(Debug, Clone, Serialize)]
pub struct GenerationRow {
    pub id: String,
    pub job_id: Option<String>,
    pub s3_key: String,
    pub thumb_s3_key: Option<String>,
    pub filename: String,
    pub seed: i64,
    pub prompt: String,
    pub negative_prompt: Option<String>,
    pub width: i32,
    pub height: i32,
    pub params: serde_json::Value,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListGenerationsResponse {
    pub items: Vec<GenerationRow>,
    pub total: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeleteGenerationsRequest {
    pub ids: Vec<Uuid>,
}

/// Lista generations com paginação e filtros (GET /internal/generations).
pub async fn list_generations(
    pool: &PgPool,
    limit: i64,
    offset: i64,
    deleted: bool,
    base_model: Option<&str>,
) -> Result<ListGenerationsResponse, ManagerError> {
    let mut where_clauses = Vec::new();
    let mut bind_idx: u32 = 1;

    if deleted {
        where_clauses.push("g.deleted_at IS NOT NULL".to_string());
    } else {
        where_clauses.push("g.deleted_at IS NULL".to_string());
    }

    if base_model.is_some() {
        where_clauses.push(format!("g.params->>'base_model' = ${bind_idx}"));
        bind_idx += 1;
    }

    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };

    // Count query.
    let count_sql = format!("SELECT COUNT(*) FROM generations g {where_sql}");
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    if let Some(bm) = base_model {
        count_q = count_q.bind(bm);
    }
    let total: i64 = count_q
        .fetch_one(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("count generations: {e}")))?;

    // Main query.
    let limit_idx = bind_idx;
    let offset_idx = bind_idx + 1;
    let main_sql = format!(
        "SELECT g.id, g.job_id, g.s3_key, g.thumb_s3_key, g.filename, g.seed, \
         g.prompt, g.negative_prompt, g.width, g.height, g.params, g.created_at, g.deleted_at \
         FROM generations g {where_sql} \
         ORDER BY g.created_at DESC LIMIT ${limit_idx} OFFSET ${offset_idx}"
    );
    let mut main_q = sqlx::query(&main_sql);
    if let Some(bm) = base_model {
        main_q = main_q.bind(bm);
    }
    main_q = main_q.bind(limit).bind(offset);

    let rows = main_q
        .fetch_all(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("list generations: {e}")))?;

    let items = rows
        .into_iter()
        .map(|r| {
            let id: Uuid = r.get("id");
            let job_id: Option<Uuid> = r.get("job_id");
            let created_at: DateTime<Utc> = r.get("created_at");
            let deleted_at: Option<DateTime<Utc>> = r.get("deleted_at");
            GenerationRow {
                id: id.to_string(),
                job_id: job_id.map(|u| u.to_string()),
                s3_key: r.get("s3_key"),
                thumb_s3_key: r.get("thumb_s3_key"),
                filename: r.get("filename"),
                seed: r.get("seed"),
                prompt: r.get("prompt"),
                negative_prompt: r.get("negative_prompt"),
                width: r.get("width"),
                height: r.get("height"),
                params: r.get("params"),
                created_at: created_at.to_rfc3339(),
                deleted_at: deleted_at.map(|t| t.to_rfc3339()),
            }
        })
        .collect();

    Ok(ListGenerationsResponse { items, total })
}

/// Busca uma generation por ID — exclui soft-deletadas (deleted_at IS NULL).
pub async fn get_generation(
    pool: &PgPool,
    id: Uuid,
) -> Result<Option<GenerationRow>, ManagerError> {
    let row = sqlx::query(
        "SELECT id, job_id, s3_key, thumb_s3_key, filename, seed, prompt, negative_prompt, \
         width, height, params, created_at, deleted_at \
         FROM generations WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("get generation: {e}")))?;

    Ok(row.map(|r| {
        let id: Uuid = r.get("id");
        let job_id: Option<Uuid> = r.get("job_id");
        let created_at: DateTime<Utc> = r.get("created_at");
        let deleted_at: Option<DateTime<Utc>> = r.get("deleted_at");
        GenerationRow {
            id: id.to_string(),
            job_id: job_id.map(|u| u.to_string()),
            s3_key: r.get("s3_key"),
            thumb_s3_key: r.get("thumb_s3_key"),
            filename: r.get("filename"),
            seed: r.get("seed"),
            prompt: r.get("prompt"),
            negative_prompt: r.get("negative_prompt"),
            width: r.get("width"),
            height: r.get("height"),
            params: r.get("params"),
            created_at: created_at.to_rfc3339(),
            deleted_at: deleted_at.map(|t| t.to_rfc3339()),
        }
    }))
}

/// Soft delete de generations por IDs (POST /internal/generations/delete).
pub async fn soft_delete_generations(pool: &PgPool, ids: &[Uuid]) -> Result<(), ManagerError> {
    if ids.is_empty() || ids.len() > 100 {
        return Err(ManagerError::InvalidRequest(
            "ids must have 1..100 items".into(),
        ));
    }
    sqlx::query(
        "UPDATE generations SET deleted_at = now() WHERE id = ANY($1) AND deleted_at IS NULL",
    )
    .bind(ids)
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("soft delete generations: {e}")))?;
    Ok(())
}
