//! Estado compartilhado HTTP do orchestrator (Fatia 3 — modularização).
//!
//! `AppState` é injetado via `axum::extract::State` em todos os handlers
//! e middlewares. Extraído de `main.rs` sem mudança de comportamento.

/// Erro de admissão atômica de jobs no orchestrator (P0-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionError {
    CapacityExceeded,
    DuplicateJobId,
}

impl std::fmt::Display for AdmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CapacityExceeded => write!(f, "capacity exceeded"),
            Self::DuplicateJobId => write!(f, "duplicate job id"),
        }
    }
}

impl std::error::Error for AdmissionError {}

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
    pub admission_lock: std::sync::Arc<std::sync::Mutex<()>>,
}

impl AppState {
    /// Tenta admitir um job atomicamente sob o `admission_lock` (P0-3).
    ///
    /// Garante que concorrência e idempotência não sofram race conditions:
    /// 1. Adquire `admission_lock`
    /// 2. Valida se `active_jobs.len() >= max_concurrent_jobs`
    /// 3. Valida se `active_jobs.contains_key(&job_id)`
    /// 4. Insere o job em `active_jobs`
    pub fn try_admit(
        &self,
        job_id: String,
        state: crate::ActiveJobState,
    ) -> Result<(), AdmissionError> {
        let _guard = self
            .admission_lock
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if self.active_jobs.len() >= self.max_concurrent_jobs {
            return Err(AdmissionError::CapacityExceeded);
        }
        if self.active_jobs.contains_key(&job_id) {
            return Err(AdmissionError::DuplicateJobId);
        }
        self.active_jobs.insert(job_id, state);
        Ok(())
    }
}
