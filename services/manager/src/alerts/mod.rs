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

    // 1. Encontra jobs 'running' cuja última telemetria (MAX(ts) em job_metric_points,
    //    senão started_at, senão created_at) tem idade >= stale_secs.
    let stale_jobs = sqlx::query_as::<_, (Uuid,)>(
        "SELECT j.id FROM jobs j \
         LEFT JOIN LATERAL ( \
             SELECT MAX(ts) AS last_ts FROM job_metric_points WHERE job_id = j.id \
         ) m ON true \
         WHERE j.status = 'running' \
           AND COALESCE(m.last_ts, j.started_at, j.created_at) < now() - make_interval(secs => $1::float)",
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

/// Avalia alertas de disco (`disk_high`) para jobs em status `running`.
///
/// Dispara em `>= ALERT_DISK_RATIO` (default 0.85) com warning.
/// Sobe para critical em `>= 0.95`.
/// Com histerese (ALERT_DISK_HYSTERESIS, default 0.05):
/// * Só volta de critical para warning abaixo de `0.95 - hysteresis` (0.90).
/// * Só resolve abaixo de `ALERT_DISK_RATIO - hysteresis` (0.80).
///
/// Quando `ratio` é None (nó sem heartbeat recente, cache vazio após restart, nó antigo):
/// no-op, sem disparar nem resolver. Só resolve com medição válida abaixo do limiar de resolução,
/// ou no término do job.
pub async fn evaluate_disk_alerts(
    pool: &PgPool,
    cache: &crate::nodes::TelemetryCache,
) -> Result<(), ManagerError> {
    let threshold_ratio = get_alert_disk_ratio();
    let hysteresis = get_alert_disk_hysteresis();
    let warning_resolve_ratio = threshold_ratio - hysteresis;
    let critical_downgrade_ratio = CRITICAL_ALERT_DISK_RATIO - hysteresis;

    // Busca todos os jobs running e seus nós
    let running_jobs = sqlx::query_as::<_, (Uuid, Option<Uuid>)>(
        "SELECT id, orchestrator_id FROM jobs WHERE status = 'running'",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("find running jobs for disk alert: {e}")))?;

    let cache_guard = cache.read().await;

    for (job_id, orch_id_opt) in running_jobs {
        let orch_telemetry = orch_id_opt.and_then(|id| cache_guard.get(&id));
        let (disk_total, disk_used, node_endpoint) = match orch_telemetry {
            Some(t) => (t.disk_total_gb, t.disk_used_gb, t.endpoint.clone()),
            None => (None, None, String::new()),
        };

        let ratio = match (disk_total, disk_used) {
            (Some(tot), Some(used)) if tot > 0.0 => Some(used / tot),
            _ => None,
        };

        // Quando ratio é None: no-op absoluto (não dispara nem resolve)
        let Some(r) = ratio else {
            continue;
        };

        let active_alert = sqlx::query_as::<_, (Uuid, String)>(
            "SELECT id, severity FROM job_alerts WHERE job_id = $1 AND rule_id = $2 AND resolved_at IS NULL",
        )
        .bind(job_id)
        .bind(RULE_DISK_HIGH)
        .fetch_optional(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("fetch active disk alert: {e}")))?;

        match active_alert {
            None => {
                // Sem alerta ativo: dispara se r >= threshold_ratio
                if r >= threshold_ratio {
                    let severity = if r >= CRITICAL_ALERT_DISK_RATIO {
                        SEVERITY_CRITICAL
                    } else {
                        SEVERITY_WARNING
                    };
                    let pct = (r * 100.0).round() as i64;
                    let message = format!(
                        "Uso de disco no nó ({node_endpoint}) em {pct}% (>= {}%)",
                        (threshold_ratio * 100.0).round() as i64
                    );

                    let mut tx = pool
                        .begin()
                        .await
                        .map_err(|e| ManagerError::Internal(format!("tx begin: {e}")))?;
                    let fired =
                        fire_alert(&mut tx, job_id, RULE_DISK_HIGH, severity, &message).await?;
                    if fired {
                        crate::notify::notify_alert(&mut *tx, job_id).await?;
                    }
                    tx.commit()
                        .await
                        .map_err(|e| ManagerError::Internal(format!("tx commit: {e}")))?;
                }
            }
            Some((_id, current_sev)) if current_sev == SEVERITY_WARNING => {
                if r >= CRITICAL_ALERT_DISK_RATIO {
                    // Sobe para critical
                    let pct = (r * 100.0).round() as i64;
                    let message = format!(
                        "Uso de disco no nó ({node_endpoint}) em {pct}% (>= {}%)",
                        (CRITICAL_ALERT_DISK_RATIO * 100.0).round() as i64
                    );
                    let mut tx = pool
                        .begin()
                        .await
                        .map_err(|e| ManagerError::Internal(format!("tx begin: {e}")))?;
                    resolve_alerts(&mut tx, job_id, Some(RULE_DISK_HIGH)).await?;
                    fire_alert(&mut tx, job_id, RULE_DISK_HIGH, SEVERITY_CRITICAL, &message)
                        .await?;
                    crate::notify::notify_alert(&mut *tx, job_id).await?;
                    tx.commit()
                        .await
                        .map_err(|e| ManagerError::Internal(format!("tx commit: {e}")))?;
                } else if r < warning_resolve_ratio {
                    // Resolve com histerese (< threshold_ratio - hysteresis)
                    let mut tx = pool
                        .begin()
                        .await
                        .map_err(|e| ManagerError::Internal(format!("tx begin: {e}")))?;
                    let resolved = resolve_alerts(&mut tx, job_id, Some(RULE_DISK_HIGH)).await?;
                    if resolved > 0 {
                        crate::notify::notify_alert(&mut *tx, job_id).await?;
                    }
                    tx.commit()
                        .await
                        .map_err(|e| ManagerError::Internal(format!("tx commit: {e}")))?;
                }
                // Se warning_resolve_ratio <= r < CRITICAL_ALERT_DISK_RATIO: mantém warning sem mexer
            }
            Some((_id, _current_sev)) => {
                // current_sev == SEVERITY_CRITICAL
                if r < warning_resolve_ratio {
                    // Resolve completamente se caiu abaixo do limiar de resolução geral
                    let mut tx = pool
                        .begin()
                        .await
                        .map_err(|e| ManagerError::Internal(format!("tx begin: {e}")))?;
                    let resolved = resolve_alerts(&mut tx, job_id, Some(RULE_DISK_HIGH)).await?;
                    if resolved > 0 {
                        crate::notify::notify_alert(&mut *tx, job_id).await?;
                    }
                    tx.commit()
                        .await
                        .map_err(|e| ManagerError::Internal(format!("tx commit: {e}")))?;
                } else if r < critical_downgrade_ratio {
                    // Volta para warning só abaixo de 0.95 - hysteresis (0.90)
                    let pct = (r * 100.0).round() as i64;
                    let message = format!(
                        "Uso de disco no nó ({node_endpoint}) em {pct}% (>= {}%)",
                        (threshold_ratio * 100.0).round() as i64
                    );
                    let mut tx = pool
                        .begin()
                        .await
                        .map_err(|e| ManagerError::Internal(format!("tx begin: {e}")))?;
                    resolve_alerts(&mut tx, job_id, Some(RULE_DISK_HIGH)).await?;
                    fire_alert(&mut tx, job_id, RULE_DISK_HIGH, SEVERITY_WARNING, &message).await?;
                    crate::notify::notify_alert(&mut *tx, job_id).await?;
                    tx.commit()
                        .await
                        .map_err(|e| ManagerError::Internal(format!("tx commit: {e}")))?;
                }
                // Se r >= critical_downgrade_ratio: mantém critical
            }
        }
    }

    Ok(())
}
