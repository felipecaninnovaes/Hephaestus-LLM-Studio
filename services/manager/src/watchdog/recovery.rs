//! Recuperação de jobs no boot do manager (ADR-0007, MM-09).

use sqlx::PgPool;
use crate::error::ManagerError;

/// Recupera jobs órfãos no boot do manager (F4.3):
/// - `cancelling` → `cancelled` (com finished_at=now(), queue_reason='recovered_cancel')
/// - `dispatched` | `running` → `queued` (com orchestrator_id=NULL, queue_reason='recovered')
///
/// `preparing` é EXCLUÍDO de propósito: recuperação de prepares pertence ao
/// principal (`job_prepares`/`recover_stale_prepares`, ADR-0025 D3) — um
/// preparing órfão nunca vira `queued` sem pacote; morre pelo watchdog de
/// 60min (`watchdog_prepare_timeout`) se o worker não voltar.
pub async fn recover_jobs(pool: &PgPool) -> Result<u64, ManagerError> {
    // 1. Jobs que estavam em 'cancelling' no boot passam para 'cancelled':
    sqlx::query(
        "UPDATE jobs SET status = 'cancelled', finished_at = now(), queue_reason = 'recovered_cancel' \
         WHERE status = 'cancelling'",
    )
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("recover cancelling jobs: {e}")))?;

    // 2. Jobs em voo (dispatched ou running) no boot passam para queued (preparing segue intocado):
    let result = sqlx::query(
        "UPDATE jobs SET status = 'queued', queue_reason = 'recovered', orchestrator_id = NULL \
         WHERE status IN ('dispatched', 'running')",
    )
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("recover jobs: {e}")))?;

    Ok(result.rows_affected())
}
