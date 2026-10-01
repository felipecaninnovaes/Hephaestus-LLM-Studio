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
pub use app::outbox::{
    drain_outbox, is_unrecoverable_error, spawn_outbox_drain_worker, OutboxItem, OutboxReportClient,
};
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
pub use server::{build_router, AdmissionError, AppState};
pub use storage::{
    append_manifest, build_job_view, ensure_dataset_cached, write_terminal_marker,
    MANIFEST_FILENAME, TERMINAL_MARKER_FILENAME,
};
pub use storage::{
    compute_file_md5, evict_dataset_cache, evict_text_embeds_cache, init_image_ext, put_with_retry,
    scoped_init_image_key, scoped_key, stage_cached_weight, stage_cached_weight_with_progress,
    sweep_dataset_cache_tmp, sweep_output_purge, unzip_safe, DatasetCacheLocks, S3Client, S3Scope,
};
pub use telemetry::*;

#[cfg(test)]
mod tests;
