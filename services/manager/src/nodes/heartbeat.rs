//! Recepção e persistência de heartbeats de orquestradores (ADR-0011, MM-10).

use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use super::cache::TelemetryCache;
use crate::error::ManagerError;
pub use heph_contracts::heartbeat::HeartbeatBody as HeartbeatRequest;

/// Recebe heartbeat do orquestrador.
pub async fn receive_heartbeat(
    pool: &PgPool,
    cache: &TelemetryCache,
    req: HeartbeatRequest,
) -> Result<(), ManagerError> {
    // 1. Resolve endpoint → id.
    let row: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM orchestrators WHERE endpoint = $1")
        .bind(&req.endpoint)
        .fetch_optional(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("resolve heartbeat endpoint: {e}")))?;

    let orch_id = match row {
        Some((id,)) => id,
        None => {
            tracing::warn!(
                endpoint = %req.endpoint,
                "heartbeat de endpoint não registrado (orchestrator não adotado ou ORCH_ADVERTISE_URL errado)"
            );
            return Ok(());
        }
    };

    // 2. Atualiza last_heartbeat e status SOMENTE nesta linha.
    let update_result = sqlx::query(
        "UPDATE orchestrators SET last_heartbeat = now(), status = 'online' \
         WHERE id = $1 AND status <> 'revoked'",
    )
    .bind(orch_id)
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("update heartbeat: {e}")))?;

    // Se a linha é revoked, o UPDATE não modificou nada — retorna cedo
    // (sem UPDATE de gpus/vram, sem cache). Nó revoked não deve ser medido.
    if update_result.rows_affected() == 0 {
        tracing::info!(
            orch_id = %orch_id,
            endpoint = %req.endpoint,
            "heartbeat ignorado: nó revoked"
        );
        return Ok(());
    }

    // 3. Grava gpus/vram_total_gb quando heartbeat carrega VRAM e gpus não-vazio.
    //    vram_total_gb = maior GPU individual (round(max_gpu_mib/1024)) — 1 job = 1 GPU.
    //    Fallback: heartbeat sem max_gpu_mib (orquestrador legado) usa a soma (vram_total).
    if !req.gpus.is_empty() {
        let effective_vram_mib = req.max_gpu_mib.or(req.vram_total);
        if let Some(vram_mib) = effective_vram_mib {
            let vram_total_gb = ((vram_mib as f64) / 1024.0).round() as i32;
            let gpus_json = serde_json::to_value(&req.gpus)
                .map_err(|e| ManagerError::Internal(format!("serialize gpus: {e}")))?;
            sqlx::query("UPDATE orchestrators SET gpus = $1, vram_total_gb = $2 WHERE id = $3")
                .bind(gpus_json)
                .bind(vram_total_gb)
                .bind(orch_id)
                .execute(pool)
                .await
                .map_err(|e| ManagerError::Internal(format!("update orchestrator gpus: {e}")))?;
        }
    }

    // 4. Atualiza cache por nó.
    let mut cache = cache.write().await;
    let state = cache.entry(orch_id).or_default();
    state.endpoint = req.endpoint;
    state.measured = true;
    state.vram_used = req.vram_used;
    state.vram_total = req.vram_total;
    state.cpu = req.cpu;
    state.ram = req.ram;
    state.ram_total = req.ram_total;
    state.gpus = req.gpus;
    state.jobs_active = req.jobs_active;
    state.last_heartbeat = Some(Utc::now());

    Ok(())
}
