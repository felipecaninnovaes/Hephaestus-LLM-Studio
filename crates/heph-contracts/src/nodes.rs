use serde::{Deserialize, Serialize};

/// Orquestrador retornado pelo manager (snake_case interno).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrchestratorItem {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub endpoint: String,
    pub status: String,
    pub last_heartbeat: Option<String>,
    /// Telemetria por nó (H.4 — ADR-0011 D2).
    #[serde(default)]
    pub measured: bool,
    #[serde(default)]
    pub cpu: Option<f64>,
    #[serde(default)]
    pub ram: Option<i64>,
    #[serde(default)]
    pub ram_total: Option<i64>,
    #[serde(default)]
    pub vram_used: Option<i64>,
    #[serde(default)]
    pub vram_total: Option<i64>,
    #[serde(default)]
    pub vram_total_gb: Option<i32>,
    #[serde(default)]
    pub gpus: Vec<String>,
    #[serde(default)]
    pub jobs_active: i32,
}

/// Telemetria global agregada (camelCase wire direto do manager — D9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelemetryResponse {
    pub measured: bool,
    #[serde(default)]
    pub vram_used: Option<i64>,
    #[serde(default)]
    pub vram_total: Option<i64>,
    #[serde(default)]
    pub cpu: Option<f64>,
    #[serde(default)]
    pub ram: Option<i64>,
    #[serde(default)]
    pub ram_total: Option<i64>,
    #[serde(default)]
    pub gpus: Vec<String>,
    #[serde(default)]
    pub jobs_active: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orchestrator_item_serde() {
        let json = serde_json::json!({
            "id": "node-1",
            "name": "gpu-box",
            "kind": "remote",
            "endpoint": "http://127.0.0.1:8080",
            "status": "ready",
            "last_heartbeat": null,
        });

        let item: OrchestratorItem = serde_json::from_value(json).unwrap();
        assert_eq!(item.id, "node-1");
        assert!(!item.measured);
        assert_eq!(item.jobs_active, 0);
        assert!(item.gpus.is_empty());
    }

    #[test]
    fn test_telemetry_response_serde() {
        let json = serde_json::json!({
            "measured": true,
            "vram_used": 1024,
            "vram_total": 8192,
            "cpu": 12.5,
            "ram": 2048,
            "ram_total": 16384,
            "gpus": ["RTX 4090"],
            "jobs_active": 1,
        });

        let resp: TelemetryResponse = serde_json::from_value(json).unwrap();
        assert!(resp.measured);
        assert_eq!(resp.vram_used, Some(1024));
        assert_eq!(resp.jobs_active, 1);
    }
}
