//! Estado global compartilhado da aplicação HTTP (MM-06).

use std::sync::Arc;
use sqlx::PgPool;
use crate::orchestrator::OrchestratorClient;
use crate::policy::VramTable;
use crate::TelemetryCache;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub token: String,
    pub telemetry_cache: TelemetryCache,
    pub orch_client: Arc<dyn OrchestratorClient>,
    pub exec_mode: String,
    pub orch_workdir: String,
    pub trainer_image: String,
    pub vram_table: VramTable,
}
