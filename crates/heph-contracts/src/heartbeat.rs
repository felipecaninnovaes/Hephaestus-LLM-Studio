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
}

/// Alias para compatibilidade com o manager.
pub type HeartbeatRequest = HeartbeatBody;
