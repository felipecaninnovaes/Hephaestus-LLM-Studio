//! Estado compartilhado HTTP do orchestrator (Fatia 3 — modularização).
//!
//! `AppState` é injetado via `axum::extract::State` em todos os handlers
//! e middlewares. Extraído de `main.rs` sem mudança de comportamento.

/// Estado compartilhado do servidor HTTP.
#[derive(Clone)]
pub struct AppState {
    pub s3: std::sync::Arc<dyn crate::ports::S3Port>,
    pub report_client: std::sync::Arc<dyn crate::ports::ReportClient>,
    pub executor: std::sync::Arc<dyn crate::ports::TrainerExecutor>,
    pub active_jobs: crate::ActiveJobs,
    pub manager_token: Option<String>,
    pub gpu_devices: Option<String>,
    pub gpu_allow_mock: bool,
    pub pairing: std::sync::Arc<crate::PairingState>,
    pub daemon_state: Option<std::sync::Arc<crate::daemon::DaemonState>>,
    pub max_concurrent_jobs: usize,
}
