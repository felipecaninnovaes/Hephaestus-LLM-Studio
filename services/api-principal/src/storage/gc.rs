//! Garbage Collection e purga periódica de storage e metadados.
//!
//! Executa em background a cada N minutos:
//! 1. Expirar sessões de upload chunked inativas (>1h).
//! 2. Expurgo físico no S3 de dataset_versions órfãs (>7 dias) e deleção no SQL.
//! 3. Expurgo físico de imagens na lixeira (>30 dias) e deleção no SQL.
//! 4. Expurgo de generation_inputs temporários (>7 dias).

use crate::state::AppState;
use std::time::Duration;
use uuid::Uuid;

/// Executa todos os sweeps de GC do storage. Retorna resumo de itens purgados.
pub async fn run_storage_gc(state: &AppState) {
    // 1. Sessões chunked em memória/disco
    let chunked = crate::models::chunk::sweep_expired_upload_sessions(Duration::from_secs(3600));
    if chunked > 0 {
        tracing::info!(count = chunked, "gc: sessões chunked expiradas limpas");
    }

    // 2. Versões de datasets órfãs (>7 dias sem job associado)
    match sweep_orphan_dataset_versions(state).await {
        Ok(n) if n > 0 => tracing::info!(count = n, "gc: dataset_versions órfãs purgadas"),
        Ok(_) => {}
        Err(e) => tracing::warn!("gc: erro ao purgar dataset_versions órfãs: {e}"),
    }

    // 3. Imagens na lixeira há mais de 30 dias
    match sweep_expired_trash(state).await {
        Ok(n) if n > 0 => tracing::info!(count = n, "gc: imagens da lixeira expiradas purgadas"),
        Ok(_) => {}
        Err(e) => tracing::warn!("gc: erro ao purgar lixeira: {e}"),
    }

    // 4. Inputs efêmeros de generation_inputs (>7 dias)
    match sweep_old_generation_inputs(state).await {
        Ok(n) if n > 0 => tracing::info!(count = n, "gc: generation_inputs antigos purgados"),
        Ok(_) => {}
        Err(e) => tracing::warn!("gc: erro ao purgar generation_inputs: {e}"),
    }
}

/// Purga dataset_versions órfãs (>7 dias) e deleta seus arquivos packages/{version_id}/ no S3.
pub async fn sweep_orphan_dataset_versions(state: &AppState) -> Result<u64, sqlx::Error> {
    let old_versions: Vec<Uuid> = sqlx::query_scalar(
        "DELETE FROM dataset_versions dv \
         WHERE dv.created_at < now() - interval '7 days' \
           AND NOT EXISTS (SELECT 1 FROM jobs j \
             WHERE (j.params->'package_ref'->>'version_id') IS NOT NULL \
               AND j.params->'package_ref'->>'version_id' = dv.id::text) \
           AND NOT EXISTS (SELECT 1 FROM jobs j \
             WHERE j.status NOT IN ('done','failed','cancelled') \
               AND j.params->'prepare'->>'datasetId' = dv.dataset_id::text) \
         RETURNING dv.id",
    )
    .fetch_all(&state.pool)
    .await?;

    let count = old_versions.len() as u64;
    for version_id in old_versions {
        let prefix = format!("packages/{version_id}/");
        let _ = state.storage.delete_prefix(&prefix).await;
    }
    Ok(count)
}

/// Purga imagens com soft-delete há mais de 30 dias e deleta objetos no S3.
pub async fn sweep_expired_trash(state: &AppState) -> Result<u64, sqlx::Error> {
    let old_images: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "DELETE FROM images WHERE deleted_at IS NOT NULL AND deleted_at < now() - interval '30 days' RETURNING id, dataset_id",
    )
    .fetch_all(&state.pool)
    .await?;

    let count = old_images.len() as u64;
    for (img_id, ds_id) in old_images {
        let prefix = format!("datasets/{ds_id}/images/{img_id}/");
        let _ = state.storage.delete_prefix(&prefix).await;
    }
    Ok(count)
}

/// Purga generation_inputs consumidos ou criados há mais de 7 dias e deleta objetos no S3.
pub async fn sweep_old_generation_inputs(state: &AppState) -> Result<u64, sqlx::Error> {
    let old_inputs: Vec<Uuid> = sqlx::query_scalar(
        "DELETE FROM generation_inputs \
         WHERE created_at < now() - interval '7 days' \
         RETURNING id",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let count = old_inputs.len() as u64;
    for id in old_inputs {
        let prefix = format!("generation_inputs/{id}/");
        let _ = state.storage.delete_prefix(&prefix).await;
    }
    Ok(count)
}
