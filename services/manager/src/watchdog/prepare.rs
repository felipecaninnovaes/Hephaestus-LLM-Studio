//! Watchdog de expiração de preparação e GC de dataset versions (ADR-0025, MM-09).

use sqlx::PgPool;
use crate::error::ManagerError;

/// Watchdog de preparação (ADR-0025 D3): `preparing` com created_at > N minutos
/// → `failed` (`params.error = 'prepare_timeout'`).
pub async fn watchdog_prepare_timeout_with_minutes(
    pool: &PgPool,
    timeout_mins: i64,
) -> Result<u64, ManagerError> {
    let result = sqlx::query(
        "UPDATE jobs SET status = 'failed', finished_at = now(), \
          params = params || jsonb_build_object('error', 'prepare_timeout') \
         WHERE status = 'preparing' AND created_at < now() - make_interval(mins => $1::int)",
    )
    .bind(timeout_mins as i32)
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("watchdog prepare timeout: {e}")))?;

    Ok(result.rows_affected())
}

/// Watchdog de preparação com default de 60 minutos (ADR-0025 D3).
pub async fn watchdog_prepare_timeout(pool: &PgPool) -> Result<u64, ManagerError> {
    watchdog_prepare_timeout_with_minutes(pool, 60).await
}

/// GC de dataset_versions (ADR-0025 D4): apaga versões com >7 dias NUNCA
/// referenciadas por job aceito (referência = params.package_ref.version_id;
/// NUNCA apaga pacote referenciado). Query defensiva: IS NOT NULL no
/// version_id para NULL não virar match.
pub async fn gc_dataset_versions(pool: &PgPool) -> Result<u64, ManagerError> {
    let result = sqlx::query(
        "DELETE FROM dataset_versions dv \
         WHERE dv.created_at < now() - interval '7 days' \
           AND NOT EXISTS (SELECT 1 FROM jobs j \
             WHERE (j.params->'package_ref'->>'version_id') IS NOT NULL \
               AND j.params->'package_ref'->>'version_id' = dv.id::text) \
           AND NOT EXISTS (SELECT 1 FROM jobs j \
             WHERE j.status NOT IN ('done','failed','cancelled') \
               AND j.params->'prepare'->>'datasetId' = dv.dataset_id::text)",
    )
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("gc dataset versions: {e}")))?;

    Ok(result.rows_affected())
}
