use serde::{Deserialize, Serialize};

/// Payload de heartbeat enviado do orchestrator ao manager (POST /internal/heartbeat).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeartbeatBody {
    pub endpoint: String,
    pub gpus: Vec<String>,
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
