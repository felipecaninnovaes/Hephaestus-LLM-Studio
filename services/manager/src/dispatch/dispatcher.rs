//! Despacho transacional de jobs para orquestradores (MM-14).

use sqlx::PgPool;
use uuid::Uuid;

use super::election::select_eligible_orchestrator;
use super::payload::{build_dispatch_payload, BuildPayloadInput};
use crate::error::ManagerError;
use crate::orchestrator::client::OrchestratorClient;
use crate::policy::vram::VramTable;

type QueuedJobRow = (
    Uuid,
    Option<String>,
    String,
    String,
    String,
    Option<Uuid>,
    Option<serde_json::Value>,
    Option<String>,
);

/// Despacha o próximo job queued para um orquestrador elegível.
///
/// Invariante #4 (Locking e Ordem do dispatch_next):
/// 1. `SELECT ... FROM jobs WHERE status = 'queued' ORDER BY created_at LIMIT 1 FOR UPDATE SKIP LOCKED`
/// 2. Eleição de nó orquestrador com lock exclusivo `FOR UPDATE OF o`.
/// 3. Atualização do job para status `dispatched`, `orchestrator_id = $2`, `queue_reason = NULL`.
/// 4. `tx.commit().await` rigorosamente ANTES do POST HTTP ao orquestrador.
/// 5. POST HTTP para `{orch_endpoint}/internal/dispatch`.
/// 6. Compensação best-effort em caso de erro no POST HTTP: reverte o job para `queued` com `orchestrator_id = NULL`.
pub async fn dispatch_next(
    pool: &PgPool,
    orch_client: &dyn OrchestratorClient,
    exec_mode: &str,
    orch_workdir: &str,
    image: &str,
    vram_table: &VramTable,
) -> Result<bool, ManagerError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| ManagerError::Internal(format!("begin dispatch tx: {e}")))?;

    // 1. Seleciona próximo job queued (FIFO) com lock exclusivo SKIP LOCKED.
    let row: Option<QueuedJobRow> = sqlx::query_as(
        "SELECT id, kind, engine, model, mode, dataset_id, params, config_yaml \
         FROM jobs WHERE status = 'queued' ORDER BY created_at LIMIT 1 \
         FOR UPDATE SKIP LOCKED",
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| ManagerError::Internal(format!("select next job: {e}")))?;

    let (job_id, kind, engine, model, mode, dataset_id, params, config_yaml) = match row {
        Some(r) => r,
        None => return Ok(false),
    };

    // 2. Resolve requisito VRAM da vram-table.
    let required_gb: Option<i32> = vram_table.resolve_required_gb(&engine, &model, &mode);

    // 3. Extrai hint e seleciona nó orquestrador (ADR-0015 D3) com lock FOR UPDATE OF o.
    let hint: Option<Uuid> = params
        .as_ref()
        .and_then(|p| p.get("orchestrator_hint"))
        .and_then(|h| h.as_str())
        .and_then(|s| s.parse().ok());

    let orch = match select_eligible_orchestrator(&mut tx, job_id, hint, required_gb).await? {
        Some(o) => o,
        None => {
            tx.commit()
                .await
                .map_err(|e| ManagerError::Internal(format!("commit queue reason tx: {e}")))?;
            return Ok(false);
        }
    };

    // 4. Marca dispatched e atualiza flag de fallback em params dentro da transação.
    if orch.fallback_used {
        sqlx::query(
            "UPDATE jobs SET status = 'dispatched', queue_reason = NULL, orchestrator_id = $2, \
             params = jsonb_set(params, '{orchestrator_fallback}', 'true'::jsonb) \
             WHERE id = $1 AND status = 'queued'",
        )
        .bind(job_id)
        .bind(orch.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ManagerError::Internal(format!("set dispatched (fallback): {e}")))?;
    } else {
        sqlx::query(
            "UPDATE jobs SET status = 'dispatched', queue_reason = NULL, orchestrator_id = $2, \
             params = params - 'orchestrator_fallback' \
             WHERE id = $1 AND status = 'queued'",
        )
        .bind(job_id)
        .bind(orch.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ManagerError::Internal(format!("set dispatched: {e}")))?;
    }

    // Invariante #4: tx.commit() ANTES do POST HTTP ao orquestrador.
    tx.commit()
        .await
        .map_err(|e| ManagerError::Internal(format!("commit dispatch tx: {e}")))?;

    // 5. Monta payload do dispatch tipado em snake_case.
    let payload = build_dispatch_payload(BuildPayloadInput {
        job_id,
        kind: kind.as_deref(),
        engine: &engine,
        model: &model,
        mode: &mode,
        dataset_id,
        params: params.as_ref(),
        config_yaml: config_yaml.as_deref(),
        exec_mode,
        orch_workdir,
        image,
    });

    let dispatch_body = serde_json::to_value(&payload)
        .map_err(|e| ManagerError::Internal(format!("serialize dispatch payload: {e}")))?;

    // 6. POST HTTP para {orch_endpoint}/internal/dispatch.
    let url = format!("{}/internal/dispatch", orch.endpoint);

    // 7. Compensação best-effort em caso de erro no POST HTTP.
    if let Err(e) = orch_client.post(&url, &dispatch_body).await {
        tracing::warn!("dispatch failed for job {job_id}: {e}");
        sqlx::query(
            "UPDATE jobs SET status = 'queued', queue_reason = 'waiting_slot', orchestrator_id = NULL WHERE id = $1",
        )
        .bind(job_id)
        .execute(pool)
        .await
        .map_err(|e2| ManagerError::Internal(format!("revert job: {e2}")))?;
    }

    Ok(true)
}
