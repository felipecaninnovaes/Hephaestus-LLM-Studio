use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

pub mod rules;
pub use rules::*;

use crate::error::ManagerError;
use heph_contracts::alerts::{JobAlert, JobAlertsResponse};

/// Busca todos os alertas de um job ordenados por fired_at DESC.
pub async fn get_job_alerts(
    pool: &PgPool,
    job_id: Uuid,
) -> Result<JobAlertsResponse, ManagerError> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM jobs WHERE id = $1)")
        .bind(job_id)
        .fetch_one(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("check job exists: {e}")))?;

    if !exists {
        return Err(ManagerError::NotFound);
    }

    let rows = sqlx::query_as::<
        _,
        (
            Uuid,
            Uuid,
            String,
            String,
            String,
            DateTime<Utc>,
            Option<DateTime<Utc>>,
        ),
    >(
        "SELECT id, job_id, rule_id, severity, message, fired_at, resolved_at \
         FROM job_alerts \
         WHERE job_id = $1 \
         ORDER BY fired_at DESC",
    )
    .bind(job_id)
    .fetch_all(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("fetch job alerts: {e}")))?;

    let items = rows
        .into_iter()
        .map(
            |(id, jid, rule_id, severity, message, fired_at, resolved_at)| JobAlert {
                id: id.to_string(),
                job_id: jid.to_string(),
                rule_id,
                severity,
                message,
                fired_at: fired_at.to_rfc3339(),
                resolved_at: resolved_at.map(|dt| dt.to_rfc3339()),
            },
        )
        .collect();

    Ok(JobAlertsResponse { items })
}

/// Loop/tick de avaliação periódica de alertas (ex.: `telemetry_stale`).
/// Executado pelo watchdog (~2s).
pub async fn evaluate_periodic_alerts(pool: &PgPool) -> Result<(), ManagerError> {
    let stale_secs = get_alert_stale_secs();

    // 1. Encontra jobs 'running' cuja última telemetria (MAX(ts) em job_metric_points, ou created_at de jobs)
    //    tem idade >= stale_secs.
    let stale_jobs = sqlx::query_as::<_, (Uuid,)>(
        "SELECT j.id FROM jobs j \
         LEFT JOIN LATERAL ( \
             SELECT MAX(ts) AS last_ts FROM job_metric_points WHERE job_id = j.id \
         ) m ON true \
         WHERE j.status = 'running' \
           AND COALESCE(m.last_ts, j.created_at) < now() - make_interval(secs => $1::float)",
    )
    .bind(stale_secs as f64)
    .fetch_all(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("find stale jobs: {e}")))?;

    for (job_id,) in stale_jobs {
        let mut tx = pool
            .begin()
            .await
            .map_err(|e| ManagerError::Internal(format!("begin stale alert tx: {e}")))?;

        let fired = fire_alert(
            &mut tx,
            job_id,
            RULE_TELEMETRY_STALE,
            SEVERITY_WARNING,
            &format!("Job em execução sem novos pontos de telemetria há mais de {stale_secs}s"),
        )
        .await?;

        if fired {
            crate::notify::notify_alert(&mut *tx, job_id).await?;
        }

        tx.commit()
            .await
            .map_err(|e| ManagerError::Internal(format!("commit stale alert tx: {e}")))?;
    }

    Ok(())
}
