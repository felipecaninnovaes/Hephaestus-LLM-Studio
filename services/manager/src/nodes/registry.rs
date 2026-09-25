//! Registro, adoção e revogação de orquestradores (ADR-0010, ADR-0011, MM-10).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use super::cache::TelemetryCache;
use crate::error::ManagerError;
use crate::orchestrator::OrchestratorClient;

#[derive(Debug, Clone, Serialize)]
pub struct OrchestratorItem {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub endpoint: String,
    pub status: String,
    pub last_heartbeat: Option<String>,
    pub vram_total_gb: Option<i32>,
    pub measured: bool,
    pub cpu: Option<f64>,
    pub ram: Option<i64>,
    pub ram_total: Option<i64>,
    pub vram_used: Option<i64>,
    pub vram_total: Option<i64>,
    pub gpus: Vec<String>,
    pub jobs_active: i32,
}

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
);

/// Lista todos os orquestradores com telemetria por nó.
pub async fn list_orchestrators(
    pool: &PgPool,
    cache: &TelemetryCache,
) -> Result<OrchestratorsResponse, ManagerError> {
    let rows: Vec<OrchestratorDbRow> = sqlx::query_as(
        "SELECT id, name, kind, endpoint, status, last_heartbeat, vram_total_gb \
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
            let (measured, cpu, ram, ram_total, vram_used, vram_total, gpus, jobs_active) =
                match telemetry {
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
                            state.jobs_active,
                        )
                    }
                    None => (false, None, None, None, None, None, vec![], 0),
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
                jobs_active,
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
        jobs_active: 0,
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
