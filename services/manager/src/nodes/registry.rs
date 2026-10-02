//! Registro, adoção e revogação de orquestradores (ADR-0010, ADR-0011, MM-10).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use super::cache::TelemetryCache;
use crate::error::ManagerError;
use crate::orchestrator::OrchestratorClient;

pub use heph_contracts::OrchestratorItem;

#[derive(Debug, Clone, Serialize)]
pub struct OrchestratorsResponse {
    pub items: Vec<OrchestratorItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AdoptRequest {
    pub name: String,
    pub endpoint: String,
    pub kind: String,
    pub pairing_code: String,
}

pub struct OrchestratorRow {
    #[allow(dead_code)]
    pub id: Uuid,
    pub endpoint: String,
}

pub async fn get_orchestrator(pool: &PgPool, id: Uuid) -> Result<OrchestratorRow, ManagerError> {
    let row: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, endpoint FROM orchestrators WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("get orchestrator: {e}")))?;

    row.map(|(id, endpoint)| OrchestratorRow { id, endpoint })
        .ok_or(ManagerError::Internal("orchestrator not found".into()))
}

/// Auto-adoção: insere orchestrator-local se ausente, atualiza status.
/// Guarda: auto-adoção NUNCA ressuscita revoked (ADR-0011 D5.7).
pub async fn adopt_orchestrator(pool: &PgPool) -> Result<(), ManagerError> {
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'orchestrator-local', 'http://orchestrator-local:8082', 'local', 'online') \
         ON CONFLICT (endpoint) DO UPDATE SET status = 'online' \
         WHERE orchestrators.status <> 'revoked'",
    )
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("adopt orchestrator: {e}")))?;

    Ok(())
}
type OrchestratorDbRow = (
    Uuid,
    String,
    String,
    String,
    String,
    Option<DateTime<Utc>>,
    Option<i32>,
    Option<serde_json::Value>,
);

/// Lista todos os orquestradores com telemetria por nó.
pub async fn list_orchestrators(
    pool: &PgPool,
    cache: &TelemetryCache,
) -> Result<OrchestratorsResponse, ManagerError> {
    let rows: Vec<OrchestratorDbRow> = sqlx::query_as(
        "SELECT id, name, kind, endpoint, status, last_heartbeat, vram_total_gb, gpu_devices \
             FROM orchestrators ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("list orchestrators: {e}")))?;

    let cache = cache.read().await;
    let now = Utc::now();

    let items = rows
        .into_iter()
        .map(|r| {
            let orch_id = r.0;
            let telemetry = cache.get(&orch_id);
            let (
                measured,
                cpu,
                ram,
                ram_total,
                vram_used,
                vram_total,
                gpus,
                gpu_devices,
                jobs_active,
                disk_total_gb,
                disk_used_gb,
            ) = match telemetry {
                Some(state) => {
                    let m = state
                        .last_heartbeat
                        .map(|last| (now - last).num_seconds() <= 10)
                        .unwrap_or(false);
                    (
                        m,
                        state.cpu,
                        state.ram,
                        state.ram_total,
                        state.vram_used,
                        state.vram_total,
                        state.gpus.clone(),
                        state.gpu_devices.clone(),
                        state.jobs_active,
                        state.disk_total_gb,
                        state.disk_used_gb,
                    )
                }
                None => {
                    let db_gpu_devices: Vec<heph_contracts::GpuDeviceTelemetry> =
                        r.7.as_ref()
                            .and_then(|val| serde_json::from_value(val.clone()).ok())
                            .unwrap_or_default();
                    (
                        false,
                        None,
                        None,
                        None,
                        None,
                        None,
                        vec![],
                        db_gpu_devices,
                        0,
                        None,
                        None,
                    )
                }
            };

            // Se o cache não tinha gpu_devices mas o banco tem, usa o do banco como fallback
            let effective_gpu_devices = if gpu_devices.is_empty() {
                r.7.as_ref()
                    .and_then(|val| serde_json::from_value(val.clone()).ok())
                    .unwrap_or_default()
            } else {
                gpu_devices
            };
            OrchestratorItem {
                id: orch_id.to_string(),
                name: r.1,
                kind: r.2,
                endpoint: r.3,
                status: r.4,
                last_heartbeat: r.5.map(|t| t.to_rfc3339()),
                vram_total_gb: r.6,
                measured,
                cpu,
                ram,
                ram_total,
                vram_used,
                vram_total,
                gpus,
                gpu_devices: effective_gpu_devices,
                jobs_active,
                disk_total_gb,
                disk_used_gb,
            }
        })
        .collect();

    Ok(OrchestratorsResponse { items })
}

/// Adopt interno: valida, verifica pairing no orquestrador, upsert.
pub async fn adopt_internal(
    pool: &PgPool,
    orch_client: &dyn OrchestratorClient,
    req: &AdoptRequest,
) -> Result<OrchestratorItem, ManagerError> {
    // Validação de domínio.
    if req.kind != "local" && req.kind != "remoto" {
        return Err(ManagerError::InvalidRequest(
            "kind must be 'local' or 'remoto'".into(),
        ));
    }
    if !req.endpoint.starts_with("http://") && !req.endpoint.starts_with("https://") {
        return Err(ManagerError::InvalidRequest(
            "endpoint must start with http:// or https://".into(),
        ));
    }
    if req.name.is_empty() || req.name.len() > 128 {
        return Err(ManagerError::InvalidRequest(
            "name must be 1-128 characters".into(),
        ));
    }
    if req.pairing_code.is_empty() || req.pairing_code.len() > 128 {
        return Err(ManagerError::InvalidRequest(
            "pairing_code must be 1-128 characters".into(),
        ));
    }

    // Verifica pairing code no orquestrador.
    let verify_url = format!("{}/internal/pairing/verify", req.endpoint);
    let verify_body = serde_json::json!({"code": &req.pairing_code});

    match orch_client.post_json(&verify_url, &verify_body).await {
        Ok(json) => {
            let valid = json.get("valid").and_then(|v| v.as_bool()).unwrap_or(false);
            if !valid {
                return Err(ManagerError::PairingInvalid);
            }
        }
        Err(_) => {
            return Err(ManagerError::PairingInvalid);
        }
    }

    // Upsert: cria ou revive (revoked incluído — intenção explícita do operador).
    let orch_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status, token_hash, fingerprint) \
         VALUES ($1, $2, $3, $4, 'online', NULL, NULL) \
         ON CONFLICT (endpoint) DO UPDATE SET name = EXCLUDED.name, kind = EXCLUDED.kind, status = 'online'",
    )
    .bind(orch_id)
    .bind(&req.name)
    .bind(&req.endpoint)
    .bind(&req.kind)
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("adopt orchestrator: {e}")))?;

    // Resolve o id real (upsert pode ter usado linha existente).
    let real_id: (Uuid,) = sqlx::query_as("SELECT id FROM orchestrators WHERE endpoint = $1")
        .bind(&req.endpoint)
        .fetch_one(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("resolve adopted id: {e}")))?;

    // Retorna item completo (shape idêntico a list_orchestrators).
    Ok(OrchestratorItem {
        id: real_id.0.to_string(),
        name: req.name.clone(),
        kind: req.kind.clone(),
        endpoint: req.endpoint.clone(),
        status: "online".to_string(),
        last_heartbeat: None,
        vram_total_gb: None,
        measured: false,
        cpu: None,
        ram: None,
        ram_total: None,
        vram_used: None,
        vram_total: None,
        gpus: vec![],
        gpu_devices: vec![],
        jobs_active: 0,
        disk_total_gb: None,
        disk_used_gb: None,
    })
}

/// Revoke: status → 'revoked' (tombstone, não DELETE).
pub async fn revoke_orchestrator(pool: &PgPool, id: Uuid) -> Result<(), ManagerError> {
    let result = sqlx::query("UPDATE orchestrators SET status = 'revoked' WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("revoke orchestrator: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(ManagerError::NotFound);
    }
    Ok(())
}
