use serde::{Deserialize, Serialize};

/// Telemetria detalhada por GPU individual (nvidia-smi).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuDeviceTelemetry {
    pub index: u32,
    pub uuid: String,
    pub name: String,
    /// VRAM total da GPU em MiB.
    pub vram_total: i64,
    /// VRAM usada da GPU em MiB.
    pub vram_used: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_watts: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_utilization_pct: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature_c: Option<i32>,
}

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gpu_devices: Vec<GpuDeviceTelemetry>,
    #[serde(default)]
    pub jobs_active: i32,
    #[serde(default)]
    pub disk_total_gb: Option<f64>,
    #[serde(default)]
    pub disk_used_gb: Option<f64>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gpu_devices: Vec<GpuDeviceTelemetry>,
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
        assert!(item.gpu_devices.is_empty());
    }

    #[test]
    fn test_orchestrator_item_gpu_devices_empty_skip_serializing() {
        let item = OrchestratorItem {
            id: "node-1".into(),
            name: "box".into(),
            kind: "remote".into(),
            endpoint: "http://127.0.0.1:8080".into(),
            status: "ready".into(),
            last_heartbeat: None,
            measured: false,
            cpu: None,
            ram: None,
            ram_total: None,
            vram_used: None,
            vram_total: None,
            vram_total_gb: None,
            gpus: vec![],
            gpu_devices: vec![],
            jobs_active: 0,
            disk_total_gb: None,
            disk_used_gb: None,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(!json.contains("gpu_devices"));
    }

    #[test]
    fn test_orchestrator_item_with_gpu_devices() {
        let dev = GpuDeviceTelemetry {
            index: 0,
            uuid: "GPU-1c1e01c2-4192-8f38-1a8a-33fb78b06f17".into(),
            name: "NVIDIA GeForce RTX 3060".into(),
            vram_total: 12288,
            vram_used: 6961,
            power_watts: Some(18.03),
            gpu_utilization_pct: Some(0.0),
            temperature_c: Some(50),
        };
        let item = OrchestratorItem {
            id: "node-1".into(),
            name: "box".into(),
            kind: "remote".into(),
            endpoint: "http://127.0.0.1:8080".into(),
            status: "ready".into(),
            last_heartbeat: None,
            measured: true,
            cpu: None,
            ram: None,
            ram_total: None,
            vram_used: Some(6961),
            vram_total: Some(12288),
            vram_total_gb: Some(12),
            gpus: vec!["NVIDIA GeForce RTX 3060".into()],
            gpu_devices: vec![dev.clone()],
            jobs_active: 0,
            disk_total_gb: None,
            disk_used_gb: None,
        };
        let json_val = serde_json::to_value(&item).unwrap();
        assert!(json_val.get("gpu_devices").is_some());
        let de: OrchestratorItem = serde_json::from_value(json_val).unwrap();
        assert_eq!(de.gpu_devices, vec![dev]);
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
        assert!(resp.gpu_devices.is_empty());
    }
}
