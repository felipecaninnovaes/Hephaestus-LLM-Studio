//! Aborto de jobs com cancelamento local e notificação ao orquestrador (MM-13).

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::ManagerError;
use crate::nodes::get_orchestrator;
use crate::orchestrator::OrchestratorClient;

/// Aborta um job. Retorna o status resultante ("cancelled" ou "cancelling").
pub async fn abort_job(
    pool: &PgPool,
    id: Uuid,
    orch_client: &dyn OrchestratorClient,
) -> Result<String, ManagerError> {
    let row: Option<(String, Option<Uuid>)> =
        sqlx::query_as("SELECT status, orchestrator_id FROM jobs WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("get job for abort: {e}")))?;

    let (status, orchestrator_id) = row.ok_or(ManagerError::NotFound)?;

    match status.as_str() {
        "done" | "failed" | "cancelled" => Err(ManagerError::NotAbortable),

        "queued" | "dispatched" => {
            sqlx::query("UPDATE jobs SET status = 'cancelled', queue_reason = NULL WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await
                .map_err(|e| ManagerError::Internal(format!("cancel job: {e}")))?;
            Ok("cancelled".to_string())
        }

        "preparing" => {
            sqlx::query("UPDATE jobs SET status = 'cancelling' WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await
                .map_err(|e| ManagerError::Internal(format!("set cancelling: {e}")))?;
            Ok("cancelling".to_string())
        }

        "running" => {
            sqlx::query("UPDATE jobs SET status = 'cancelling' WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await
                .map_err(|e| ManagerError::Internal(format!("set cancelling: {e}")))?;

            // Notifica orquestrador com retry (até 3 tentativas com backoff).
            if let Some(orch_id) = orchestrator_id {
                if let Ok(orch) = get_orchestrator(pool, orch_id).await {
                    let body = serde_json::json!({"job_id": id.to_string()});
                    let url = format!("{}/internal/abort", orch.endpoint);
                    let mut sent = false;
                    for attempt in 0..3 {
                        if attempt > 0 {
                            tokio::time::sleep(std::time::Duration::from_millis(
                                200 * (1 << attempt),
                            ))
                            .await;
                        }
                        match orch_client.post(&url, &body).await {
                            Ok(_) => {
                                sent = true;
                                break;
                            }
                            Err(e) => {
                                tracing::warn!("tentativa {attempt} abort no orquestrador falhou para job {id}: {e}");
                            }
                        }
                    }
                    if !sent {
                        tracing::error!(
                            "todas tentativas de abort no orquestrador falharam para job {id}"
                        );
                    }
                }
            }

            Ok("cancelling".to_string())
        }

        "cancelling" => Ok("cancelling".to_string()),

        other => Err(ManagerError::Internal(format!(
            "unexpected status: {other}"
        ))),
    }
}
