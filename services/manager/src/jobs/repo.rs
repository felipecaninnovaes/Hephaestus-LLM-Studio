//! Consultas e mapeamentos SQL compartilhados de jobs (MM-13).

use std::collections::HashMap;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::error::ManagerError;
use super::types::JobRow;

pub const SELECT_JOB_FIELDS: &str =
    "SELECT j.id, j.kind, j.engine, j.model, j.mode, j.dataset_id, j.status, j.queue_reason, \
     j.progress, j.epoch, j.step, j.metrics, j.vram_min_gb, j.orchestrator_id, j.created_at, j.finished_at, j.params, \
     j.phase, j.message, \
     o.name AS orchestrator_name, o.kind AS orchestrator_kind, \
     COALESCE((j.params->>'orchestrator_fallback') = 'true', false) AS orchestrator_fallback, \
     j.params->>'error' AS error \
     FROM jobs j \
     LEFT JOIN orchestrators o ON o.id = j.orchestrator_id";

/// Constrói o mapa de posições na fila (1-indexed por created_at) para jobs queued.
pub async fn fetch_queue_positions(
    pool: &PgPool,
) -> Result<HashMap<String, i32>, ManagerError> {
    let queue_rows: Vec<(Uuid,)> =
        sqlx::query_as("SELECT id FROM jobs WHERE status = 'queued' ORDER BY created_at")
            .fetch_all(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("queue positions: {e}")))?;
    Ok(queue_rows
        .into_iter()
        .enumerate()
        .map(|(i, (id,))| (id.to_string(), (i + 1) as i32))
        .collect())
}

/// Mapeia uma linha PostgreSQL (PgRow) para o DTO JobRow com posições calculadas.
pub fn row_to_job_row(
    r: &PgRow,
    pos_map: &HashMap<String, i32>,
) -> JobRow {
    let id: Uuid = r.get("id");
    let id_str = id.to_string();
    let status: String = r.get("status");
    let queue_position = if status == "queued" {
        pos_map.get(&id_str).copied()
    } else {
        None
    };
    let dataset_id: Option<Uuid> = r.get("dataset_id");
    let orchestrator_id: Option<Uuid> = r.get("orchestrator_id");
    let created_at: DateTime<Utc> = r.get("created_at");
    let finished_at: Option<DateTime<Utc>> = r.get("finished_at");
    JobRow {
        id: id_str,
        kind: r.get("kind"),
        engine: r.get("engine"),
        model: r.get("model"),
        mode: r.get("mode"),
        dataset_id: dataset_id.map(|u| u.to_string()),
        status,
        queue_reason: r.get("queue_reason"),
        queue_position,
        progress: r.get("progress"),
        epoch: r.get("epoch"),
        step: r.get("step"),
        metrics: r.get("metrics"),
        vram_min_gb: r.get("vram_min_gb"),
        orchestrator_id: orchestrator_id.map(|u| u.to_string()),
        orchestrator_name: r.get("orchestrator_name"),
        orchestrator_kind: r.get("orchestrator_kind"),
        orchestrator_fallback: r.get("orchestrator_fallback"),
        created_at: created_at.to_rfc3339(),
        finished_at: finished_at.map(|t| t.to_rfc3339()),
        error: r.get("error"),
        params: r.get("params"),
        phase: r.get("phase"),
        message: r.get("message"),
    }
}
