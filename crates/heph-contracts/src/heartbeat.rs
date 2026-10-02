use serde::{Deserialize, Serialize};

use crate::nodes::GpuDeviceTelemetry;

/// Payload de heartbeat enviado do orchestrator ao manager (POST /internal/heartbeat).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeartbeatBody {
    pub endpoint: String,
    pub gpus: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gpu_devices: Vec<GpuDeviceTelemetry>,
    pub vram_total: Option<i64>,
    pub vram_used: Option<i64>,
    pub cpu: Option<f64>,
    pub ram: Option<i64>,
    pub ram_total: Option<i64>,
    pub jobs_active: i32,
    /// Maior VRAM individual entre as GPUs (MiB) — capacidade real de 1 job.
    pub max_gpu_mib: Option<i64>,
    /// Capacidade total do filesystem do workdir do nó (GB).
    #[serde(default)]
    pub disk_total_gb: Option<f64>,
    /// Espaço em disco usado no filesystem do workdir do nó (GB).
    #[serde(default)]
    pub disk_used_gb: Option<f64>,
}

/// Alias para compatibilidade com o manager.
pub type HeartbeatRequest = HeartbeatBody;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heartbeat_roundtrip_sem_gpu_devices() {
        let json = serde_json::json!({
            "endpoint": "http://127.0.0.1:8082",
            "gpus": ["NVIDIA GeForce RTX 3060"],
            "vram_total": 12288,
            "vram_used": 1024,
            "cpu": 10.5,
            "ram": 2048,
            "ram_total": 16384,
            "jobs_active": 0,
            "max_gpu_mib": 12288
        });

        let body: HeartbeatBody = serde_json::from_value(json).unwrap();
        assert!(body.gpu_devices.is_empty());
        let serialized = serde_json::to_string(&body).unwrap();
        assert!(!serialized.contains("gpu_devices"));
    }

    #[test]
    fn test_heartbeat_roundtrip_com_gpu_devices() {
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
        let body = HeartbeatBody {
            endpoint: "http://127.0.0.1:8082".into(),
            gpus: vec!["NVIDIA GeForce RTX 3060".into()],
            gpu_devices: vec![dev.clone()],
            vram_total: Some(12288),
            vram_used: Some(6961),
            cpu: Some(5.0),
            ram: Some(4096),
            ram_total: Some(16384),
            jobs_active: 0,
            max_gpu_mib: Some(12288),
            disk_total_gb: Some(100.0),
            disk_used_gb: Some(20.0),
        };

        let val = serde_json::to_value(&body).unwrap();
        assert!(val.get("gpu_devices").is_some());
        let de: HeartbeatBody = serde_json::from_value(val).unwrap();
        assert_eq!(de.gpu_devices, vec![dev]);
    }
}
