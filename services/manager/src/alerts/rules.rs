use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::ManagerError;

/// Regras suportadas pelo sistema de alertas.
pub const RULE_NAN_DETECTED: &str = "nan_detected";
pub const RULE_TELEMETRY_STALE: &str = "telemetry_stale";
pub const RULE_VRAM_HIGH: &str = "vram_high";
pub const RULE_DISK_HIGH: &str = "disk_high";

/// Severity de alertas.
pub const SEVERITY_WARNING: &str = "warning";
pub const SEVERITY_CRITICAL: &str = "critical";

/// Limiar padrão de job running sem telemetria em segundos (ALERT_STALE_SECS).
pub const DEFAULT_ALERT_STALE_SECS: i64 = 300;

/// Limiar padrão de uso de disco para warning (ALERT_DISK_RATIO).
pub const DEFAULT_ALERT_DISK_RATIO: f64 = 0.85;
/// Limiar de uso de disco para critical.
pub const CRITICAL_ALERT_DISK_RATIO: f64 = 0.95;

/// Histerese padrão para resolução de alertas de disco (ALERT_DISK_HYSTERESIS).
pub const DEFAULT_ALERT_DISK_HYSTERESIS: f64 = 0.05;

pub fn get_alert_disk_hysteresis() -> f64 {
    std::env::var("ALERT_DISK_HYSTERESIS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_ALERT_DISK_HYSTERESIS)
}

pub fn get_alert_disk_ratio() -> f64 {
    std::env::var("ALERT_DISK_RATIO")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_ALERT_DISK_RATIO)
}

pub fn get_alert_stale_secs() -> i64 {
    std::env::var("ALERT_STALE_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_ALERT_STALE_SECS)
}

/// Dispara um alerta se não houver um ativo para o mesmo (job_id, rule_id).
/// Usa a constraint `job_alerts_active_uniq` com ON CONFLICT DO NOTHING.
/// Retorna `true` se inseriu um novo alerta ativo.
pub async fn fire_alert(
    conn: &mut PgConnection,
    job_id: Uuid,
    rule_id: &str,
    severity: &str,
    message: &str,
) -> Result<bool, ManagerError> {
    let result = sqlx::query(
        "INSERT INTO job_alerts (job_id, rule_id, severity, message, fired_at) \
         VALUES ($1, $2, $3, $4, now()) \
         ON CONFLICT (job_id, rule_id) WHERE resolved_at IS NULL \
         DO NOTHING",
    )
    .bind(job_id)
    .bind(rule_id)
    .bind(severity)
    .bind(message)
    .execute(conn)
    .await
    .map_err(|e| ManagerError::Internal(format!("fire alert {rule_id}: {e}")))?;

    Ok(result.rows_affected() > 0)
}

/// Resolve alertas ativos para uma regra específica (ou todas as regras ativas se rule_id=None).
/// Retorna a quantidade de alertas resolvidos.
pub async fn resolve_alerts(
    conn: &mut PgConnection,
    job_id: Uuid,
    rule_id: Option<&str>,
) -> Result<u64, ManagerError> {
    let result = match rule_id {
        Some(rule) => sqlx::query(
            "UPDATE job_alerts SET resolved_at = now() \
                 WHERE job_id = $1 AND rule_id = $2 AND resolved_at IS NULL",
        )
        .bind(job_id)
        .bind(rule)
        .execute(conn)
        .await
        .map_err(|e| ManagerError::Internal(format!("resolve alert {rule}: {e}")))?,
        None => sqlx::query(
            "UPDATE job_alerts SET resolved_at = now() \
                 WHERE job_id = $1 AND resolved_at IS NULL",
        )
        .bind(job_id)
        .execute(conn)
        .await
        .map_err(|e| ManagerError::Internal(format!("resolve all alerts: {e}")))?,
    };

    Ok(result.rows_affected())
}

/// Avalia a regra `nan_detected` a partir das métricas brutas recebidas no report.
/// Dispara quando algum ponto possui `nan_count > 0` ou `inf_count > 0`.
pub async fn check_nan_detected_on_metrics(
    conn: &mut PgConnection,
    job_id: Uuid,
    metrics: &serde_json::Value,
) -> Result<bool, ManagerError> {
    let mut triggered = false;
    let items = crate::reporting::metrics::normalize_metrics_to_array(metrics);
    for item in &items {
        if let Some(obj) = item.as_object() {
            // Verifica nan_count ou inf_count
            let nan_count = obj.get("nan_count").and_then(|v| v.as_i64()).unwrap_or(0);
            let inf_count = obj.get("inf_count").and_then(|v| v.as_i64()).unwrap_or(0);
            if nan_count > 0 || inf_count > 0 {
                triggered = true;
                break;
            }
        }
    }

    if triggered {
        let fired = fire_alert(
            conn,
            job_id,
            RULE_NAN_DETECTED,
            SEVERITY_CRITICAL,
            "NaN ou Inf detectado nos gradientes/loss do modelo",
        )
        .await?;
        return Ok(fired);
    }

    Ok(false)
}
