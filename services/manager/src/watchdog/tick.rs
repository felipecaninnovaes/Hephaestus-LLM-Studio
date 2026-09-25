//! Tick periódico do watchdog para nós degradados/offline e expirações (MM-09).

use super::prepare::{gc_dataset_versions, watchdog_prepare_timeout};
use crate::error::ManagerError;
use sqlx::PgPool;

/// Tick do watchdog: transições online→degraded→offline com re-queue dos jobs.
/// Chamada pelo worker loop (~2s).
pub async fn watchdog_tick(pool: &PgPool) -> Result<(), ManagerError> {
    let degraded_s: i64 = std::env::var("ORCH_WATCHDOG_DEGRADED_S")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15);
    let offline_s: i64 = std::env::var("ORCH_WATCHDOG_OFFLINE_S")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);

    // online → degraded (last_heartbeat mais velho que degraded_s OU NULL).
    let _ = sqlx::query(
        "UPDATE orchestrators SET status = 'degraded' \
         WHERE status = 'online' \
           AND (last_heartbeat IS NULL OR last_heartbeat < now() - make_interval(secs => $1::float))",
    )
    .bind(degraded_s as f64)
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("watchdog degraded: {e}")))?;

    // degraded → offline + re-queue dos jobs do nó morto (CTE espelho de recover_jobs).
    // `preparing` excluído como no recover: jobs em preparação caem no caminho
    // prepare-timeout/fail, nunca viram queued sem pacote.
    // 1. Jobs do nó morto em 'cancelling' passam para 'cancelled':
    let _ = sqlx::query(
        "WITH morto AS ( \
             SELECT id FROM orchestrators \
             WHERE status = 'degraded' \
               AND (last_heartbeat IS NULL OR last_heartbeat < now() - make_interval(secs => $1::float)) \
         ) \
         UPDATE jobs SET status = 'cancelled', finished_at = now(), orchestrator_id = NULL \
         WHERE orchestrator_id IN (SELECT id FROM morto) \
           AND status = 'cancelling'",
    )
    .bind(offline_s as f64)
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("watchdog cancel cancelling jobs: {e}")))?;

    // 2. degraded → offline + re-queue dos jobs em voo do nó morto (preparing segue intocado):
    let result = sqlx::query(
        "WITH morto AS ( \
             UPDATE orchestrators SET status = 'offline' \
             WHERE status = 'degraded' \
               AND (last_heartbeat IS NULL OR last_heartbeat < now() - make_interval(secs => $1::float)) \
             RETURNING id \
         ) \
         UPDATE jobs SET status = 'queued', queue_reason = 'recovered', orchestrator_id = NULL \
         WHERE orchestrator_id IN (SELECT id FROM morto) \
           AND status IN ('dispatched','running')",
    )
    .bind(offline_s as f64)
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("watchdog offline: {e}")))?;
    if result.rows_affected() > 0 {
        tracing::info!(
            "watchdog: {} jobs re-queued de nós offline",
            result.rows_affected()
        );
    }

    // Preparação travada (ADR-0025 D3): `preparing` > 60min → failed.
    match watchdog_prepare_timeout(pool).await {
        Ok(n) if n > 0 => tracing::info!("watchdog: {n} jobs preparing expirados → failed"),
        Ok(_) => {}
        Err(e) => tracing::warn!("watchdog prepare-timeout error: {e}"),
    }

    // GC de dataset_versions órfãs >7 dias (ADR-0025 D4) — mesmo loop.
    match gc_dataset_versions(pool).await {
        Ok(n) if n > 0 => tracing::info!("gc: {n} dataset_versions órfãs removidas"),
        Ok(_) => {}
        Err(e) => tracing::warn!("gc dataset_versions error: {e}"),
    }

    Ok(())
}
