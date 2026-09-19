//! Cache local de pesos customizados por MD5 (Text Encoder, LoRAs, Checkpoints).

use std::path::Path;
use std::sync::Arc;

use crate::domain::errors::PipelineError;
use crate::ports::storage::S3Port;

use super::archive::compute_file_md5;

/// Baixa e faz cache de pesos no nó orquestrador com base no MD5.
///
/// Se o peso já existir em `cache_dir/<md5>.<ext>` com tamanho > 0:
/// - Reusa instantaneamente via hardlink O(1) (ou copy fallback) sem rebaixar do S3.
/// Se não existir:
/// - Baixa para arquivo temporário, valida MD5, move atomicamente para o cache
///   e vincula ao arquivo de destino do job.
pub async fn stage_cached_weight(
    s3: &Arc<dyn S3Port>,
    cache_dir: &Path,
    dest_file: &Path,
    scoped_key: &str,
    expected_md5: &str,
) -> Result<(), PipelineError> {
    stage_cached_weight_with_progress(s3, cache_dir, dest_file, scoped_key, expected_md5, None)
        .await
}

pub async fn stage_cached_weight_with_progress(
    s3: &Arc<dyn S3Port>,
    cache_dir: &Path,
    dest_file: &Path,
    scoped_key: &str,
    expected_md5: &str,
    on_progress: Option<&(dyn Fn(u64, Option<u64>) + Send + Sync)>,
) -> Result<(), PipelineError> {
    tokio::fs::create_dir_all(cache_dir)
        .await
        .map_err(|e| PipelineError::Other(format!("create weights cache dir: {e}")))?;

    let ext = dest_file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("safetensors");
    let cached_file = cache_dir.join(format!("{expected_md5}.{ext}"));

    if cached_file.is_file() {
        if let Ok(meta) = tokio::fs::metadata(&cached_file).await {
            if meta.len() > 0 {
                tracing::info!(
                    md5 = expected_md5,
                    dest = %dest_file.display(),
                    "Cache hit para peso custom — vinculando instantaneamente via link local"
                );
                let _ = tokio::fs::remove_file(dest_file).await;
                if tokio::fs::hard_link(&cached_file, dest_file).await.is_ok() {
                    return Ok(());
                }
                if tokio::fs::copy(&cached_file, dest_file).await.is_ok() {
                    return Ok(());
                }
            }
        }
    }

    // Cache miss ou arquivo corrompido: baixa para arquivo temporário isolado
    let tmp_file = cache_dir.join(format!(
        ".tmp_{}_{}.part",
        expected_md5,
        uuid::Uuid::new_v4().simple()
    ));

    s3.get_to_file_with_progress(scoped_key, &tmp_file, on_progress)
        .await
        .map_err(|e| PipelineError::S3Download(format!("download weight {scoped_key}: {e}")))?;

    let actual_md5 = compute_file_md5(&tmp_file)
        .map_err(|e| PipelineError::S3Download(format!("compute weight md5 {scoped_key}: {e}")))?;

    if actual_md5 != expected_md5 {
        let _ = tokio::fs::remove_file(&tmp_file).await;
        return Err(PipelineError::Md5Mismatch {
            expected: expected_md5.to_string(),
            actual: actual_md5,
        });
    }

    // Move atomicamente para o cache permanente
    if let Err(e) = tokio::fs::rename(&tmp_file, &cached_file).await {
        if !cached_file.is_file() {
            let _ = tokio::fs::remove_file(&tmp_file).await;
            return Err(PipelineError::Other(format!("persist cached weight: {e}")));
        }
        let _ = tokio::fs::remove_file(&tmp_file).await;
    }

    let _ = tokio::fs::remove_file(dest_file).await;
    if tokio::fs::hard_link(&cached_file, dest_file).await.is_err() {
        tokio::fs::copy(&cached_file, dest_file)
            .await
            .map_err(|e| PipelineError::Other(format!("copy cached weight to dest: {e}")))?;
    }

    Ok(())
}
