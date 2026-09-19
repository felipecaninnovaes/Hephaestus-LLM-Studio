//! Ramificação de subcomando por (engine, mode) — ADR-0013 D6.
//!
//! Movida verbatim de `run_job_inner`: cada engine/mode mapeia para o
//! subcomando do trainer + `--config`/`--output` sob `/outputs/<job_id>`.

use crate::domain::errors::PipelineError;

/// Resolve os argumentos de subcomando do trainer para `(engine, mode)`.
///
/// Retorna `Err(PipelineError::Other)` para combinação desconhecida — o
/// chamador converte em falha honesta do job.
pub fn resolve_subcommand_args(
    engine: &str,
    mode: &str,
    job_id: &str,
) -> Result<Vec<String>, PipelineError> {
    match (engine, mode) {
        ("yolo", "train") => Ok(vec![
            "train".to_string(),
            "--config".to_string(),
            format!("/outputs/{job_id}/config.yaml"),
            "--output".to_string(),
            format!("/outputs/{job_id}"),
        ]),
        ("yolo", "predict") => Ok(vec![
            "predict".to_string(),
            "--config".to_string(),
            format!("/outputs/{job_id}/config.yaml"),
            "--output".to_string(),
            format!("/outputs/{job_id}"),
        ]),
        ("autotracker", _) => Ok(vec![
            "autotrack".to_string(),
            "--config".to_string(),
            format!("/outputs/{job_id}/config.yaml"),
            "--output".to_string(),
            format!("/outputs/{job_id}"),
        ]),
        ("autolabel", _) => Ok(vec![
            "autolabel".to_string(),
            "--config".to_string(),
            format!("/outputs/{job_id}/config.yaml"),
            "--output".to_string(),
            format!("/outputs/{job_id}"),
        ]),
        ("diffusion", "generate") => Ok(vec![
            "generate".to_string(),
            "--config".to_string(),
            format!("/outputs/{job_id}/config.yaml"),
            "--output".to_string(),
            format!("/outputs/{job_id}"),
        ]),
        ("diffusion", _) => Ok(vec![
            "train".to_string(),
            "--config".to_string(),
            format!("/outputs/{job_id}/config.yaml"),
            "--output".to_string(),
            format!("/outputs/{job_id}"),
        ]),
        (engine, mode) => Err(PipelineError::Other(format!(
            "unsupported engine/mode: {engine}/{mode}"
        ))),
    }
}
