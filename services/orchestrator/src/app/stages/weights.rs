//! Staging unificado de pesos com cache MD5 (P1-3).
//!
//! Unifica os 4 blocos replicados de `run_job_inner` (pesos base, loop de
//! LoRAs, checkpoint custom, text encoder): inferência de escopo
//! `models/`|`artifacts/`, validação via `scoped_key` e staging via
//! `stage_cached_weight_with_progress`.

use std::path::Path;
use std::sync::Arc;

use crate::domain::errors::PipelineError;
use crate::ports::storage::S3Port;
use crate::storage::{scoped_key, stage_cached_weight_with_progress, S3Scope};

/// Resolve o escopo `models/`|`artifacts/` a partir do `s3_key`, valida com
/// `scoped_key` e faz staging via cache MD5.
///
/// `on_progress` é repassado ao staging com progresso (relatórios
/// `downloading_weights`). Falhas de escopo/validação são mapeadas para
/// `PipelineError::S3Download`.
pub async fn resolve_and_stage_weight(
    s3: &Arc<dyn S3Port>,
    cache_dir: &Path,
    dest_file: &Path,
    s3_key: &str,
    expected_md5: &str,
    on_progress: Option<&(dyn Fn(u64, Option<u64>) + Send + Sync)>,
) -> Result<(), PipelineError> {
    // Infere escopo pelo prefixo da key (models/ → Models, artifacts/ → Artifacts)
    let scope = if s3_key.starts_with("models/") {
        S3Scope::Models
    } else if s3_key.starts_with("artifacts/") {
        S3Scope::Artifacts
    } else {
        return Err(PipelineError::S3Download(format!(
            "weight key must start with models/ or artifacts/, got: {s3_key}"
        )));
    };

    let scoped = scoped_key(scope, s3_key)
        .map_err(|e| PipelineError::S3Download(format!("invalid weight key: {e}")))?;

    stage_cached_weight_with_progress(s3, cache_dir, dest_file, &scoped, expected_md5, on_progress)
        .await
}
