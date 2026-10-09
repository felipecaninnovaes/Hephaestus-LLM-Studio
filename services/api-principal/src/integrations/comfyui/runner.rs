//! Execução do envio em background: lê o objeto do storage em streaming
//! (reescrevendo o header safetensors quando a arquitetura tem regra), empurra
//! para o destino e mantém `comfy_exports` atualizada (progresso ≤ 1x/s).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tokio::io::{AsyncRead, AsyncReadExt};
use uuid::Uuid;

use super::client::{Failure, Remote, TransferError};
use super::repository;
use super::safetensors;
use super::source::ResolvedSource;
use crate::storage::StoragePort;

pub const RESTART_ERROR_CODE: &str = "interrupted";
pub const RESTART_ERROR_PT: &str = "interrompido por reinício do servidor";
const PROGRESS_INTERVAL: Duration = Duration::from_secs(1);
/// Recomeços do zero quando o destino perde o `uploadId` (ComfyUI reiniciou).
const MAX_RESTARTS: u32 = 1;

pub struct ExportJob {
    pub export_id: Uuid,
    pub remote: Remote,
    pub source: ResolvedSource,
    pub overwrite: bool,
}

type Source = Box<dyn AsyncRead + Send + Unpin>;

/// Abre a origem e devolve o stream já com o header reescrito (regras por
/// arquitetura + tensores `.alpha`) e o tamanho do arquivo reescrito.
async fn open_source(
    storage: &dyn StoragePort,
    src: &ResolvedSource,
) -> Result<(Source, u64), Failure> {
    let total = u64::try_from(src.bytes).unwrap_or(0);
    if total == 0 {
        return Err(Failure::new(
            "source_invalid",
            "o arquivo de origem está vazio",
        ));
    }
    let mut stream = storage.get_stream(&src.key).await.map_err(|_| {
        Failure::new(
            "source_not_found",
            "o arquivo de origem não está disponível no storage",
        )
    })?;
    let arch = src.arch.as_deref().unwrap_or("");
    let head = safetensors::rewrite_stream_head(&mut stream, total, arch, src.alpha)
        .await
        .map_err(|e| Failure::new("source_invalid", e.message_pt()))?;
    let prefix = std::io::Cursor::new(head.prefix);
    if head.suffix.is_empty() {
        // Sem acréscimo: o stream segue inteiro (excesso de dados é detectado no envio).
        return Ok((Box::new(prefix.chain(stream)), head.total_len));
    }
    let reader = prefix
        .chain(stream.take(head.data_len))
        .chain(std::io::Cursor::new(head.suffix));
    Ok((Box::new(reader), head.total_len))
}

/// Roda o envio até o fim e grava o desfecho; nunca propaga erro (a task é
/// fire-and-forget).
pub async fn run_export(pool: PgPool, storage: Arc<dyn StoragePort>, job: ExportJob) {
    let outcome = send(&pool, storage.as_ref(), &job).await;
    let written = match outcome {
        Ok((total, path)) => {
            repository::finish_done(&pool, job.export_id, total as i64, &path).await
        }
        Err(f) => repository::finish_failed(&pool, job.export_id, &f.code, &f.message).await,
    };
    if let Err(e) = written {
        tracing::error!(export_id = %job.export_id, error = %e, "comfy export: falha ao gravar desfecho");
    }
}

async fn send(
    pool: &PgPool,
    storage: &dyn StoragePort,
    job: &ExportJob,
) -> Result<(u64, String), Failure> {
    let db_err = |e: sqlx::Error| {
        Failure::new(
            "internal",
            format!("falha ao atualizar o status do envio: {e}"),
        )
    };
    let mut restarts = 0;
    loop {
        let (mut reader, total) = open_source(storage, &job.source).await?;
        repository::mark_uploading(pool, job.export_id, total as i64)
            .await
            .map_err(db_err)?;
        let sent = Arc::new(AtomicU64::new(0));
        let ticker = spawn_progress_ticker(pool.clone(), job.export_id, sent.clone());
        let sent_cb = sent.clone();
        let mut progress = move |n: u64| sent_cb.store(n, Ordering::Relaxed);
        let result = job
            .remote
            .transfer(
                &job.source.filename,
                job.overwrite,
                total,
                &mut reader,
                &mut progress,
            )
            .await;
        ticker.abort();
        match result {
            Ok(path) => return Ok((total, path)),
            Err(TransferError::UploadLost) if restarts < MAX_RESTARTS => restarts += 1,
            Err(TransferError::UploadLost) => {
                return Err(Failure::new(
                    "upload_not_found",
                    "o ComfyUI perdeu o upload em andamento (reiniciou?)",
                ))
            }
            Err(TransferError::Failed(f)) => return Err(f),
        }
    }
}

fn spawn_progress_ticker(
    pool: PgPool,
    id: Uuid,
    sent: Arc<AtomicU64>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut last = 0;
        let mut tick = tokio::time::interval(PROGRESS_INTERVAL);
        loop {
            tick.tick().await;
            let now = sent.load(Ordering::Relaxed);
            if now != last {
                last = now;
                let _ = repository::update_progress(&pool, id, now as i64).await;
            }
        }
    })
}

/// Boot: exports `queued`/`uploading` do processo anterior viram `failed`.
pub async fn fail_interrupted(pool: &PgPool) -> Result<u64, sqlx::Error> {
    repository::fail_pending(pool, RESTART_ERROR_CODE, RESTART_ERROR_PT).await
}
