//! SQL de `comfy_targets`/`comfy_exports` e leitura de `models` (único ponto
//! com SQL do módulo). Todo `$N` tem `.bind()`.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow)]
pub struct TargetRow {
    pub id: Uuid,
    pub name: String,
    pub base_url: String,
    pub token_ciphertext: Vec<u8>,
    pub token_nonce: Vec<u8>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_check_at: Option<DateTime<Utc>>,
    pub last_check_ok: Option<bool>,
    pub last_check_error: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct ExportRow {
    pub id: Uuid,
    pub target_id: Uuid,
    pub target_name: String,
    pub filename: String,
    pub status: String,
    pub bytes_total: i64,
    pub bytes_sent: i64,
    pub error: Option<String>,
    pub error_code: Option<String>,
    pub remote_path: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct ModelRow {
    pub name: String,
    pub s3_key: String,
    pub kind: Option<String>,
    pub arch: Option<String>,
    pub bytes: i64,
    pub job_id: Option<Uuid>,
}

pub struct NewExport<'a> {
    pub id: Uuid,
    pub target_id: Uuid,
    pub model_id: Option<Uuid>,
    pub job_id: Option<Uuid>,
    pub artifact_id: Option<Uuid>,
    pub filename: &'a str,
    pub bytes_total: i64,
}

const TARGET_COLS: &str = "id, name, base_url, token_ciphertext, token_nonce, created_at, \
     updated_at, last_check_at, last_check_ok, last_check_error";
const EXPORT_SELECT: &str = "SELECT e.id, e.target_id, t.name AS target_name, e.filename, \
     e.status, e.bytes_total, e.bytes_sent, e.error, e.error_code, e.remote_path, e.created_at, e.updated_at \
     FROM comfy_exports e JOIN comfy_targets t ON t.id = e.target_id";

pub fn is_unique_violation(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .and_then(|d| d.code())
        .is_some_and(|c| c == "23505")
}

pub async fn list_targets(pool: &PgPool) -> Result<Vec<TargetRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {TARGET_COLS} FROM comfy_targets ORDER BY created_at, id"
    ))
    .fetch_all(pool)
    .await
}

pub async fn get_target(pool: &PgPool, id: Uuid) -> Result<Option<TargetRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {TARGET_COLS} FROM comfy_targets WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn insert_target(
    pool: &PgPool,
    id: Uuid,
    name: &str,
    base_url: &str,
    ciphertext: &[u8],
    nonce: &[u8],
) -> Result<TargetRow, sqlx::Error> {
    sqlx::query_as(&format!(
        "INSERT INTO comfy_targets (id, name, base_url, token_ciphertext, token_nonce) \
         VALUES ($1, $2, $3, $4, $5) RETURNING {TARGET_COLS}"
    ))
    .bind(id)
    .bind(name)
    .bind(base_url)
    .bind(ciphertext)
    .bind(nonce)
    .fetch_one(pool)
    .await
}

/// `token` = `(ciphertext, nonce)` novo; `None` mantém o atual. Trocar a URL
/// zera o resultado do último teste (ele valia para o endereço antigo).
pub async fn update_target(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    base_url: Option<&str>,
    token: Option<(&[u8], &[u8])>,
) -> Result<Option<TargetRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "UPDATE comfy_targets SET \
           name = COALESCE($2, name), \
           base_url = COALESCE($3, base_url), \
           token_ciphertext = COALESCE($4, token_ciphertext), \
           token_nonce = COALESCE($5, token_nonce), \
           last_check_at = CASE WHEN $3 IS NULL THEN last_check_at END, \
           last_check_ok = CASE WHEN $3 IS NULL THEN last_check_ok END, \
           last_check_error = CASE WHEN $3 IS NULL THEN last_check_error END, \
           updated_at = now() \
         WHERE id = $1 RETURNING {TARGET_COLS}"
    ))
    .bind(id)
    .bind(name)
    .bind(base_url)
    .bind(token.map(|t| t.0))
    .bind(token.map(|t| t.1))
    .fetch_optional(pool)
    .await
}

pub async fn delete_target(pool: &PgPool, id: Uuid) -> Result<bool, sqlx::Error> {
    let r = sqlx::query("DELETE FROM comfy_targets WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn set_check(
    pool: &PgPool,
    id: Uuid,
    ok: bool,
    error: Option<&str>,
) -> Result<Option<TargetRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "UPDATE comfy_targets SET last_check_at = now(), last_check_ok = $2, \
         last_check_error = $3 WHERE id = $1 RETURNING {TARGET_COLS}"
    ))
    .bind(id)
    .bind(ok)
    .bind(error)
    .fetch_optional(pool)
    .await
}

pub async fn insert_export(pool: &PgPool, e: &NewExport<'_>) -> Result<(), sqlx::Error> {
    let kind = if e.model_id.is_some() {
        "model"
    } else {
        "artifact"
    };
    sqlx::query(
        "INSERT INTO comfy_exports \
         (id, target_id, source_kind, model_id, job_id, artifact_id, filename, status, bytes_total) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, 'queued', $8)",
    )
    .bind(e.id)
    .bind(e.target_id)
    .bind(kind)
    .bind(e.model_id)
    .bind(e.job_id)
    .bind(e.artifact_id)
    .bind(e.filename)
    .bind(e.bytes_total)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_export(pool: &PgPool, id: Uuid) -> Result<Option<ExportRow>, sqlx::Error> {
    sqlx::query_as(&format!("{EXPORT_SELECT} WHERE e.id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn list_exports(pool: &PgPool, limit: i64) -> Result<Vec<ExportRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "{EXPORT_SELECT} ORDER BY e.created_at DESC, e.id LIMIT $1"
    ))
    .bind(limit)
    .fetch_all(pool)
    .await
}

pub async fn mark_uploading(pool: &PgPool, id: Uuid, bytes_total: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE comfy_exports SET status = 'uploading', bytes_total = $2, bytes_sent = 0, \
         updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(bytes_total)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_progress(pool: &PgPool, id: Uuid, bytes_sent: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE comfy_exports SET bytes_sent = $2, updated_at = now() \
         WHERE id = $1 AND status = 'uploading'",
    )
    .bind(id)
    .bind(bytes_sent)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn finish_done(
    pool: &PgPool,
    id: Uuid,
    bytes_total: i64,
    remote_path: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE comfy_exports SET status = 'done', bytes_sent = $2, remote_path = $3, \
         error = NULL, error_code = NULL, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(bytes_total)
    .bind(remote_path)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn finish_failed(
    pool: &PgPool,
    id: Uuid,
    code: &str,
    error: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE comfy_exports SET status = 'failed', error_code = $2, error = $3, \
         updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(code)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

/// Boot: a task de envio morreu com o processo anterior.
pub async fn fail_pending(pool: &PgPool, code: &str, error: &str) -> Result<u64, sqlx::Error> {
    let r = sqlx::query(
        "UPDATE comfy_exports SET status = 'failed', error_code = $1, error = $2, \
         updated_at = now() WHERE status IN ('queued', 'uploading')",
    )
    .bind(code)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(r.rows_affected())
}

pub async fn get_model(pool: &PgPool, id: Uuid) -> Result<Option<ModelRow>, sqlx::Error> {
    sqlx::query_as("SELECT name, s3_key, kind, arch, bytes, job_id FROM models WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn get_model_by_key(
    pool: &PgPool,
    s3_key: &str,
) -> Result<Option<ModelRow>, sqlx::Error> {
    sqlx::query_as("SELECT name, s3_key, kind, arch, bytes, job_id FROM models WHERE s3_key = $1")
        .bind(s3_key)
        .fetch_optional(pool)
        .await
}
