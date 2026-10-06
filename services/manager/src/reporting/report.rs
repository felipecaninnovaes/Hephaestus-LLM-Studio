//! Orquestração do processamento de reports de execução (MM-11).

use sqlx::PgPool;
use uuid::Uuid;

use super::artifacts::{
    done_artifacts_violation, save_intermediate_artifacts, validate_and_save_done_artifacts,
    ReportRequest,
};
use super::generations::hook_generations_on_done;
use super::metrics::insert_metrics_points_conn;
use super::models::hook_models_on_done;
use crate::error::ManagerError;
use crate::notify::notify_status_change;

/// Processa um report do orquestrador.
///
/// Mantém estritamente:
/// - Transação única no bloco `done` (`&mut Transaction<'_, Postgres>`);
/// - Semântica best-effort nos hooks de models e generations (falhas logadas como warn sem rollback);
/// - Guardas silenciosas de transições anômalas (retornam `Ok(())` preservando idempotência).
pub async fn report_job(
    pool: &PgPool,
    id: Uuid,
    report: ReportRequest,
) -> Result<(), ManagerError> {
    // 1. Verifica que o job existe.
    let current: Option<String> = sqlx::query_scalar("SELECT status FROM jobs WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("get job for report: {e}")))?;

    let current_status = current.ok_or(ManagerError::NotFound)?;

    // Transição terminal (ou report redundante pra job já terminal, guarda
    // abaixo): o job nunca mais será despachado, então o contexto de
    // correlação lembrado (se houver) não serve mais pra nada.
    if matches!(report.status.as_str(), "done" | "failed" | "cancelled") {
        heph_contracts::request_context::forget_for_job(&id.to_string());
    }
    // Guarda de transição: status terminais são imutáveis.
    if current_status == "done" || current_status == "failed" || current_status == "cancelled" {
        tracing::warn!(
            job_id = %id,
            current_status = %current_status,
            report_status = %report.status,
            "report ignorado para job terminal"
        );
        return Ok(());
    }

    // Guarda de transição: cancelling só aceita done/failed/cancelled.
    if current_status == "cancelling"
        && !matches!(report.status.as_str(), "done" | "failed" | "cancelled")
    {
        tracing::warn!(
            job_id = %id,
            current_status = %current_status,
            report_status = %report.status,
            "report ignorado para job em cancelling (aceita apenas done/failed/cancelled)"
        );
        return Ok(());
    }

    match report.status.as_str() {
        // `preparing` via report só existe dentro do ciclo async (ADR-0025).
        "preparing" if current_status != "preparing" => {
            tracing::warn!(
                job_id = %id,
                current_status = %current_status,
                report_status = %report.status,
                "report preparing ignorado fora de preparing (sem regressão de ciclo)"
            );
            Ok(())
        }
        "preparing" | "running" => {
            let mut tx = pool
                .begin()
                .await
                .map_err(|e| ManagerError::Internal(format!("begin running/preparing tx: {e}")))?;

            sqlx::query(
                "UPDATE jobs SET status = $2, progress = COALESCE($3, progress), epoch = COALESCE($4, epoch), step = COALESCE($5, step), phase = COALESCE($6, phase), message = COALESCE($7, message), started_at = CASE WHEN $2 = 'running' THEN COALESCE(started_at, now()) ELSE started_at END WHERE id = $1",
            )
            .bind(id)
            .bind(&report.status)
            .bind(report.progress)
            .bind(report.epoch)
            .bind(report.step)
            .bind(&report.phase)
            .bind(&report.message)
            .execute(&mut *tx)
            .await
            .map_err(|e| ManagerError::Internal(format!("update job status: {e}")))?;

            notify_status_change(&mut *tx, id).await?;

            if let Some(metrics) = &report.metrics {
                insert_metrics_points_conn(&mut tx, id, metrics).await?;
                // Resolve telemetry_stale quando chegam novas métricas
                let resolved_stale = crate::alerts::resolve_alerts(
                    &mut tx,
                    id,
                    Some(crate::alerts::RULE_TELEMETRY_STALE),
                )
                .await?;
                // Avalia nan_detected
                let fired_nan =
                    crate::alerts::check_nan_detected_on_metrics(&mut tx, id, metrics).await?;
                if fired_nan || resolved_stale > 0 {
                    crate::notify::notify_alert(&mut *tx, id).await?;
                }
            }
            tx.commit()
                .await
                .map_err(|e| ManagerError::Internal(format!("commit running/preparing tx: {e}")))?;

            if let Some(artifacts) = &report.artifacts {
                save_intermediate_artifacts(pool, id, artifacts).await?;
            }

            Ok(())
        }

        "done" => {
            let mut tx = pool
                .begin()
                .await
                .map_err(|e| ManagerError::Internal(format!("begin report done tx: {e}")))?;

            // Defesa em profundidade (incidente galeria vazia): recusa done sem artefatos exigidos.
            let job_kind: Option<String> =
                sqlx::query_scalar("SELECT kind FROM jobs WHERE id = $1")
                    .bind(id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|e| ManagerError::Internal(format!("get kind for done guard: {e}")))?;
            if let Some(kind) = job_kind {
                if let Some(reason) =
                    done_artifacts_violation(kind.as_str(), report.artifacts.as_deref())
                {
                    let err_code = format!("no_artifacts: {reason}");
                    let msg_pt = format!(
                        "Job finalizado sem os artefatos exigidos ({err_code}). Verifique o bucket S3 e os logs do orquestrador."
                    );
                    tracing::warn!(
                        job_id = %id,
                        kind = %kind,
                        reason = %reason,
                        "done recusado sem artefatos exigidos → failed/no_artifacts"
                    );
                    sqlx::query(
                        "UPDATE jobs SET params = params || $2::jsonb, status = 'failed', \
                         finished_at = now(), phase = COALESCE($3, phase), message = $4 \
                         WHERE id = $1",
                    )
                    .bind(id)
                    .bind(serde_json::json!({"error": err_code}))
                    .bind(&report.phase)
                    .bind(&msg_pt)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| ManagerError::Internal(format!("set failed no_artifacts: {e}")))?;
                    notify_status_change(&mut *tx, id).await?;
                    tx.commit().await.map_err(|e| {
                        ManagerError::Internal(format!("commit failed no_artifacts tx: {e}"))
                    })?;
                    return Ok(());
                }
            }

            // Valida e insere artifacts.
            if let Some(artifacts) = &report.artifacts {
                validate_and_save_done_artifacts(&mut tx, id, artifacts).await?;
            }

            // Grava pontos de métricas (append-only, job_metric_points).
            if let Some(metrics) = &report.metrics {
                insert_metrics_points_conn(&mut tx, id, metrics).await?;
            }

            // Hook: models.
            if let Some(artifacts) = &report.artifacts {
                hook_models_on_done(&mut tx, id, artifacts).await?;
            }

            // Hook: generations.
            hook_generations_on_done(&mut tx, id, &report).await?;

            sqlx::query(
                "UPDATE jobs SET status = 'done', finished_at = now(), progress = 1.0, phase = COALESCE($2, phase), message = COALESCE($3, message) WHERE id = $1",
            )
            .bind(id)
            .bind(&report.phase)
            .bind(&report.message)
            .execute(&mut *tx)
            .await
            .map_err(|e| ManagerError::Internal(format!("set done: {e}")))?;

            let resolved = crate::alerts::resolve_alerts(&mut tx, id, None).await?;
            if resolved > 0 {
                crate::notify::notify_alert(&mut *tx, id).await?;
            }

            notify_status_change(&mut *tx, id).await?;
            tx.commit()
                .await
                .map_err(|e| ManagerError::Internal(format!("commit done tx: {e}")))?;

            Ok(())
        }

        "failed" => {
            // Artefatos parciais (ex.: captions de autolabel) sobrevivem ao término
            // anormal; persistidos ANTES do status/notify para que o cliente que reage
            // à mudança de status já os encontre.
            if let Some(artifacts) = &report.artifacts {
                save_intermediate_artifacts(pool, id, artifacts).await?;
            }

            let mut tx = pool
                .begin()
                .await
                .map_err(|e| ManagerError::Internal(format!("begin report failed tx: {e}")))?;

            if let Some(err_msg) = &report.error {
                sqlx::query("UPDATE jobs SET params = params || $2::jsonb WHERE id = $1")
                    .bind(id)
                    .bind(serde_json::json!({"error": err_msg}))
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| ManagerError::Internal(format!("merge error: {e}")))?;
            }

            sqlx::query("UPDATE jobs SET status = 'failed', finished_at = now(), phase = COALESCE($2, phase), message = COALESCE($3, message) WHERE id = $1")
                .bind(id)
                .bind(&report.phase)
                .bind(&report.message)
                .execute(&mut *tx)
                .await
                .map_err(|e| ManagerError::Internal(format!("set failed: {e}")))?;

            let resolved = crate::alerts::resolve_alerts(&mut tx, id, None).await?;
            if resolved > 0 {
                crate::notify::notify_alert(&mut *tx, id).await?;
            }

            notify_status_change(&mut *tx, id).await?;
            tx.commit()
                .await
                .map_err(|e| ManagerError::Internal(format!("commit report failed tx: {e}")))?;

            Ok(())
        }

        "cancelled" => {
            // Artefatos parciais (ex.: captions de autolabel) sobrevivem ao término
            // anormal; persistidos ANTES do status/notify para que o cliente que reage
            // à mudança de status já os encontre.
            if let Some(artifacts) = &report.artifacts {
                save_intermediate_artifacts(pool, id, artifacts).await?;
            }

            let mut tx = pool
                .begin()
                .await
                .map_err(|e| ManagerError::Internal(format!("begin report cancelled tx: {e}")))?;

            if let Some(err_msg) = &report.error {
                sqlx::query("UPDATE jobs SET params = params || $2::jsonb WHERE id = $1")
                    .bind(id)
                    .bind(serde_json::json!({"error": err_msg}))
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| ManagerError::Internal(format!("merge error: {e}")))?;
            }

            sqlx::query("UPDATE jobs SET status = 'cancelled', finished_at = now(), phase = COALESCE($2, phase), message = COALESCE($3, message) WHERE id = $1")
                .bind(id)
                .bind(&report.phase)
                .bind(&report.message)
                .execute(&mut *tx)
                .await
                .map_err(|e| ManagerError::Internal(format!("set cancelled: {e}")))?;

            let resolved = crate::alerts::resolve_alerts(&mut tx, id, None).await?;
            if resolved > 0 {
                crate::notify::notify_alert(&mut *tx, id).await?;
            }

            notify_status_change(&mut *tx, id).await?;
            tx.commit()
                .await
                .map_err(|e| ManagerError::Internal(format!("commit report cancelled tx: {e}")))?;

            Ok(())
        }

        other => Err(ManagerError::Internal(format!(
            "invalid report status: {other}"
        ))),
    }
}
