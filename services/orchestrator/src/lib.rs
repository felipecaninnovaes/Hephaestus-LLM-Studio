//! Orchestrator service — executor stateless de jobs (ADR-0007 F4.4).
//!
//! Recebe dispatch do manager, baixa package, valida md5, descompacta,
//! monta config.yaml, sobe trainer (docker|subprocess), coleta métricas,
//! sobe artefatos, reporta progresso ao manager. Sem Postgres — stateless.

pub mod adapters;
pub mod app;
pub mod config;
pub mod daemon;
pub mod domain;
pub mod ports;
pub mod security;
pub mod server;
pub mod storage;
pub mod telemetry;

pub use adapters::*;
pub use app::stages::collector::{
    collect_diffusion_artifacts, read_final_metrics, read_generation_meta_content,
};
pub use app::stages::config::{
    extract_epochs, replace_config_placeholders, replace_config_placeholders_legacy,
};
pub use app::stages::execute::resolve_subcommand_args;
pub use app::stages::weights::resolve_and_stage_weight;
pub use app::{new_active_jobs, run_job, run_job_inner, ActiveJobState, ActiveJobs};
pub use domain::errors::{PipelineError, ScopedKeyError};
pub use domain::models::*;
pub use ports::executor::TrainerExecutor;
pub use ports::heartbeat::HeartbeatClient;
pub use ports::reporter::ReportClient;
pub use ports::storage::S3Port;
pub use security::*;
pub use server::{build_router, AppState};
pub use storage::{
    compute_file_md5, init_image_ext, put_with_retry, scoped_init_image_key, scoped_key,
    stage_cached_weight, stage_cached_weight_with_progress, unzip_safe, S3Client, S3Scope,
};
pub use telemetry::*;

#[cfg(test)]
mod tests;
