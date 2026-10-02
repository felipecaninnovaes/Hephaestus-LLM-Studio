//! Publicação no canal Postgres `job_events` (fatia 1b, spec
//! `telemetria-observabilidade.md` §3.2/§3.3): substitui o polling de 300 ms
//! por cliente SSE do BFF por pub-sub via `pg_notify` + `PgListener` único.
//!
//! Canal único `job_events` (NÃO `job_metrics`: também carrega mudança de
//! status). Payload ponteiro — NUNCA o dado em si (o BFF refaz uma busca por
//! job ao receber o notice):
//! - `{"jobId":"<uuid>","seq":<maxSeq>}` para novos pontos de métricas;
//! - `{"jobId":"<uuid>","status":true}` quando status/phase/progress mudou.
//!
//! Todo chamador MUST emitir dentro da MESMA transação da escrita que
//! motivou o evento — `pg_notify` só é visível a listeners após o COMMIT, e
//! um rollback descarta o notice junto com a escrita (Postgres garante isso
//! nativamente: NOTIFY é transacional).

use sqlx::{Executor, Postgres};
use uuid::Uuid;

use crate::error::ManagerError;

/// Notifica mudança de pontos de métricas de um job. `max_seq` é o maior
/// `seq` já alocado para o job (ponteiro para o BFF buscar o delta via
/// `afterSeq`).
pub async fn notify_metrics<'e, E>(exec: E, job_id: Uuid, max_seq: i64) -> Result<(), ManagerError>
where
    E: Executor<'e, Database = Postgres>,
{
    let payload = serde_json::json!({"jobId": job_id.to_string(), "seq": max_seq}).to_string();
    sqlx::query("SELECT pg_notify('job_events', $1)")
        .bind(payload)
        .execute(exec)
        .await
        .map_err(|e| ManagerError::Internal(format!("pg_notify metrics: {e}")))?;
    Ok(())
}

/// Notifica mudança de status/phase/progress de um job (sem pontos novos de
/// métricas associados).
pub async fn notify_status_change<'e, E>(exec: E, job_id: Uuid) -> Result<(), ManagerError>
where
    E: Executor<'e, Database = Postgres>,
{
    let payload = serde_json::json!({"jobId": job_id.to_string(), "status": true}).to_string();
    sqlx::query("SELECT pg_notify('job_events', $1)")
        .bind(payload)
        .execute(exec)
        .await
        .map_err(|e| ManagerError::Internal(format!("pg_notify status: {e}")))?;
    Ok(())
}
