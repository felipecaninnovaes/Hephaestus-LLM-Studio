//! Cache de telemetria em memória por nó (ADR-0011, MM-10).

use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
pub struct TelemetryState {
    pub endpoint: String,
    pub measured: bool,
    pub vram_used: Option<i64>,
    pub vram_total: Option<i64>,
    pub cpu: Option<f64>,
    pub ram: Option<i64>,
    pub ram_total: Option<i64>,
    pub gpus: Vec<String>,
    pub jobs_active: i32,
    pub last_heartbeat: Option<DateTime<Utc>>,
}


pub type TelemetryCache = Arc<RwLock<HashMap<Uuid, TelemetryState>>>;

pub fn new_telemetry_cache() -> TelemetryCache {
    Arc::new(RwLock::new(HashMap::new()))
}

pub fn node_stale_timeout_secs() -> i64 {
    std::env::var("NODE_STALE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10)
}
