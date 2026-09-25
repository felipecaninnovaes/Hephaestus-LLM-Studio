//! Ciclo de vida e consultas de jobs (MM-13).

use sqlx::PgPool;
use uuid::Uuid;

use super::repo::{fetch_queue_positions, row_to_job_row, SELECT_JOB_FIELDS};
use super::types::{
    ArtifactRow, JobRow, ListJobsResponse, PrepareCompleteRequest, PrepareFailRequest,
};
use crate::error::ManagerError;

/// Valida md5: hex lowercase de 32 chars.
pub fn is_valid_md5(s: &str) -> bool {
    s.len() == 32
        && s.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// Lista jobs com filtros opcionais.
pub async fn list_jobs(
    pool: &PgPool,
    status: Option<&str>,
    engine: Option<&str>,
) -> Result<ListJobsResponse, ManagerError> {
    let pos_map = fetch_queue_positions(pool).await?;

    let mut query = format!("{SELECT_JOB_FIELDS} WHERE 1=1");
    let mut count_query = String::from("SELECT COUNT(*) FROM jobs j WHERE 1=1");
    let mut param_idx = 1;

    if status.is_some() {
        query.push_str(&format!(" AND j.status = ${param_idx}"));
        count_query.push_str(&format!(" AND j.status = ${param_idx}"));
        param_idx += 1;
    }
    if engine.is_some() {
        query.push_str(&format!(" AND j.engine = ${param_idx}"));
        count_query.push_str(&format!(" AND j.engine = ${param_idx}"));
    }
    query.push_str(" ORDER BY j.created_at DESC");

    let mut count_q = sqlx::query_as::<_, (i64,)>(&count_query);
    if let Some(s) = status {
        count_q = count_q.bind(s);
    }
    if let Some(e) = engine {
        count_q = count_q.bind(e);
    }
    let total: (i64,) = count_q
        .fetch_one(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("count jobs: {e}")))?;

    let mut main_q = sqlx::query(&query);
    if let Some(s) = status {
        main_q = main_q.bind(s);
    }
    if let Some(e) = engine {
        main_q = main_q.bind(e);
    }
    let rows = main_q
        .fetch_all(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("list jobs: {e}")))?;

    let items = rows.iter().map(|r| row_to_job_row(r, &pos_map)).collect();

    Ok(ListJobsResponse {
        items,
        total: total.0 as i32,
    })
}

/// Retorna um job por ID.
pub async fn get_job(pool: &PgPool, id: Uuid) -> Result<JobRow, ManagerError> {
    let pos_map = fetch_queue_positions(pool).await?;

    let query = format!("{SELECT_JOB_FIELDS} WHERE j.id = $1");
    let row = sqlx::query(&query)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("get job: {e}")))?;

    let r = row.ok_or(ManagerError::NotFound)?;
    Ok(row_to_job_row(&r, &pos_map))
}

/// Lista artefatos de um job.
pub async fn get_job_artifacts(
    pool: &PgPool,
    job_id: Uuid,
) -> Result<Vec<ArtifactRow>, ManagerError> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM jobs WHERE id = $1)")
        .bind(job_id)
        .fetch_one(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("check job: {e}")))?;
    if !exists {
        return Err(ManagerError::NotFound);
    }

    let rows: Vec<(Uuid, String, String, String, i64)> = sqlx::query_as(
        "SELECT id, kind, path, md5, bytes FROM job_artifacts WHERE job_id = $1 \
         ORDER BY path, id",
    )
    .bind(job_id)
    .fetch_all(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("list artifacts: {e}")))?;

    Ok(rows
        .into_iter()
        .map(|r| ArtifactRow {
            id: r.0.to_string(),
            kind: r.1,
            path: r.2,
            md5: r.3,
            bytes: r.4,
        })
        .collect())
}

/// POST /internal/jobs/:id/prepare-complete (ADR-0025 D1).
pub async fn prepare_complete(
    pool: &PgPool,
    id: Uuid,
    req: PrepareCompleteRequest,
) -> Result<(), ManagerError> {
    let dv_id = Uuid::parse_str(&req.dataset_version_id).map_err(|_| {
        ManagerError::InvalidRequest("dataset_version_id must be a valid UUID".into())
    })?;
    if req.package_ref.key.is_empty() {
        return Err(ManagerError::InvalidRequest(
            "package_ref.key must not be empty".into(),
        ));
    }
    if !is_valid_md5(&req.package_ref.md5_zip) {
        return Err(ManagerError::InvalidRequest(format!(
            "invalid md5_zip: {}",
            req.package_ref.md5_zip
        )));
    }
    if req.package_ref.bytes < 0 {
        return Err(ManagerError::InvalidRequest(format!(
            "negative bytes: {}",
            req.package_ref.bytes
        )));
    }

    let version_exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM dataset_versions WHERE id = $1)")
            .bind(dv_id)
            .fetch_one(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("check dataset version: {e}")))?;
    if !version_exists {
        return Err(ManagerError::NotFound);
    }

    let package_json = serde_json::json!({
        "version_id": dv_id.to_string(),
        "key": req.package_ref.key,
        "md5_zip": req.package_ref.md5_zip,
        "bytes": req.package_ref.bytes,
    });
    let result = sqlx::query(
        "UPDATE jobs SET status = 'queued', queue_reason = NULL, \
          params = params || jsonb_build_object('package_ref', $2::jsonb, 'dataset_version_id', $3) \
          WHERE id = $1 AND status = 'preparing'",
    )
    .bind(id)
    .bind(&package_json)
    .bind(dv_id.to_string())
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("prepare complete: {e}")))?;

    if result.rows_affected() == 0 {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM jobs WHERE id = $1)")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("check job: {e}")))?;
        if !exists {
            return Err(ManagerError::NotFound);
        }
        return Err(ManagerError::Conflict("job_not_preparing".into()));
    }

    sqlx::query("UPDATE dataset_versions SET created_at = now() WHERE id = $1")
        .bind(dv_id)
        .execute(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("touch dataset version: {e}")))?;
    Ok(())
}

/// POST /internal/jobs/:id/prepare-fail (ADR-0025 D1).
pub async fn prepare_fail(
    pool: &PgPool,
    id: Uuid,
    req: PrepareFailRequest,
) -> Result<(), ManagerError> {
    if req.code.is_empty() || req.code.len() > 128 {
        return Err(ManagerError::InvalidRequest(
            "code must be 1-128 characters".into(),
        ));
    }
    if req.message.is_empty() {
        return Err(ManagerError::InvalidRequest(
            "message must not be empty".into(),
        ));
    }
    let error = format!("prepare_failed:{}:{}", req.code, req.message);
    let result = sqlx::query(
        "UPDATE jobs SET status = 'failed', finished_at = now(), \
          params = params || jsonb_build_object('error', $2) \
         WHERE id = $1 AND status = 'preparing'",
    )
    .bind(id)
    .bind(&error)
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("prepare fail: {e}")))?;

    if result.rows_affected() == 0 {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM jobs WHERE id = $1)")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("check job: {e}")))?;
        if !exists {
            return Err(ManagerError::NotFound);
        }
        return Err(ManagerError::Conflict("job_not_preparing".into()));
    }
    Ok(())
}

/// POST /internal/jobs/:id/prepare-cancel (ADR-0025 D1).
pub async fn prepare_cancel(pool: &PgPool, id: Uuid) -> Result<(), ManagerError> {
    let result = sqlx::query(
        "UPDATE jobs SET status = 'cancelled', finished_at = now() \
         WHERE id = $1 AND status IN ('preparing', 'cancelling')",
    )
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("prepare cancel: {e}")))?;

    if result.rows_affected() == 0 {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM jobs WHERE id = $1)")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("check job: {e}")))?;
        if !exists {
            return Err(ManagerError::NotFound);
        }
        let is_cancelled: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM jobs WHERE id = $1 AND status = 'cancelled')",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("check job cancelled: {e}")))?;
        if is_cancelled {
            return Ok(());
        }
        return Err(ManagerError::Conflict("job_not_cancelling".into()));
    }
    Ok(())
}
