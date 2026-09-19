//! Estágios cirúrgicos do pipeline de execução (Fatia 6, P1-2/P0-4/P1-3).
//!
//! Cada estágio é uma unidade testável isolada do pipeline monolítico
//! `run_job_inner`; `super::run_job_inner` apenas os compõe.

pub mod collector;
pub mod config;
pub mod execute;
pub mod weights;

pub use collector::{
    collect_diffusion_artifacts, read_final_metrics, read_generation_meta_content,
    stream_metrics_and_samples, META_CONTENT_MAX_BYTES,
};
pub use config::{extract_epochs, replace_config_placeholders, replace_config_placeholders_legacy};
pub use execute::resolve_subcommand_args;
pub use weights::resolve_and_stage_weight;
