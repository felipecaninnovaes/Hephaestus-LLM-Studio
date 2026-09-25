//! Spool outbox durável de reports ao manager (P0-2).
//!
//! Garante entrega atômica e resiliente de relatórios de progresso do orchestrator.
//! Cada report é persistido em disco antes de tentar o envio HTTP ao manager.
//! Falhas transientes de rede mantêm o report no spool, e um worker em background
//! drena os arquivos periodicamente via `drain_outbox`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::domain::models::ReportBody;
use crate::ports::reporter::ReportClient;

/// Item persistido no spool outbox em disco.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxItem {
    pub job_id: String,
    pub body: ReportBody,
    pub created_at: u64,
    pub attempts: u32,
}

/// Identifica se um erro retornado pelo `ReportClient` é não-recuperável (ex.: 400/404).
pub fn is_unrecoverable_error(err: &str) -> bool {
    let lower = err.trim().to_ascii_lowercase();
    // 408 (Request Timeout) e 429 (Too Many Requests) são recuperáveis
    if lower.starts_with("report status: 408")
        || lower.starts_with("report status: 429")
        || lower.contains("too many requests")
    {
        return false;
    }
    // Erros estritamente não-recuperáveis retornados por HttpReportClient (ou marcados unrecoverable)
    if lower.starts_with("report status: 400")
        || lower.starts_with("report status: 401")
        || lower.starts_with("report status: 403")
        || lower.starts_with("report status: 404")
        || lower.starts_with("report status: 422")
        || lower.contains("unrecoverable")
    {
        return true;
    }
    // Formato geral "report status: 4xx" (excluindo 408 e 429)
    if let Some(after) = lower.strip_prefix("report status: 4") {
        let first_two: String = after
            .chars()
            .take(2)
            .filter(|c| c.is_ascii_digit())
            .collect();
        if first_two.len() == 2 {
            let code = format!("4{first_two}");
            if code != "408" && code != "429" {
                return true;
            }
        }
    }
    false
}

/// Decorator de `ReportClient` com persistência outbox em disco.
pub struct OutboxReportClient {
    pub dir: PathBuf,
    pub inner: Arc<dyn ReportClient>,
}

impl OutboxReportClient {
    pub fn new(dir: PathBuf, inner: Arc<dyn ReportClient>) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        Self { dir, inner }
    }
}

#[async_trait]
impl ReportClient for OutboxReportClient {
    async fn report(&self, job_id: &str, body: &ReportBody) -> Result<(), String> {
        let _ = tokio::fs::create_dir_all(&self.dir).await;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let id = uuid::Uuid::new_v4();
        let final_filename = format!("{timestamp}_{id}.json");
        let temp_filename = format!(".tmp_{timestamp}_{id}.json");
        let final_path = self.dir.join(&final_filename);
        let temp_path = self.dir.join(&temp_filename);

        let item = OutboxItem {
            job_id: job_id.to_string(),
            body: body.clone(),
            created_at: timestamp,
            attempts: 0,
        };

        let file_written = match serde_json::to_vec_pretty(&item) {
            Ok(bytes) => {
                if let Err(e) = tokio::fs::write(&temp_path, &bytes).await {
                    tracing::error!(%e, "failed to write temp outbox item");
                    false
                } else if let Err(e) = tokio::fs::rename(&temp_path, &final_path).await {
                    tracing::error!(%e, "failed to rename temp outbox item");
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    false
                } else {
                    true
                }
            }
            Err(e) => {
                tracing::error!(%e, "failed to serialize outbox item");
                false
            }
        };

        match self.inner.report(job_id, body).await {
            Ok(()) => {
                if file_written {
                    let _ = tokio::fs::remove_file(&final_path).await;
                }
                Ok(())
            }
            Err(err) => {
                if is_unrecoverable_error(&err) {
                    if file_written {
                        let _ = tokio::fs::remove_file(&final_path).await;
                    }
                    Err(err)
                } else if file_written {
                    tracing::warn!(job_id, %err, "report delivery failed; queued in outbox spool");
                    Ok(())
                } else {
                    Err(err)
                }
            }
        }
    }
}

/// Lê arquivos `.json` no diretório de spool outbox e tenta reenviar.
///
/// Retorna a quantidade de itens drenados com sucesso.
pub async fn drain_outbox(dir: &Path, client: &dyn ReportClient) -> usize {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(rd) => rd,
        Err(_) => return 0,
    };

    let mut files = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with(".json") && !name.starts_with('.') {
                    files.push(path);
                }
            }
        }
    }

    files.sort();

    let mut drained_count = 0;
    for file_path in files {
        let content = match tokio::fs::read_to_string(&file_path).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(path = ?file_path, %e, "failed to read outbox file");
                continue;
            }
        };

        let mut item: OutboxItem = match serde_json::from_str(&content) {
            Ok(item) => item,
            Err(e) => {
                tracing::error!(path = ?file_path, %e, "corrupt outbox file, discarding");
                let _ = tokio::fs::remove_file(&file_path).await;
                continue;
            }
        };

        match client.report(&item.job_id, &item.body).await {
            Ok(()) => {
                let _ = tokio::fs::remove_file(&file_path).await;
                drained_count += 1;
            }
            Err(err) => {
                if is_unrecoverable_error(&err) {
                    tracing::warn!(job_id = %item.job_id, %err, "unrecoverable error in outbox drain; discarding file");
                    let _ = tokio::fs::remove_file(&file_path).await;
                } else {
                    tracing::warn!(job_id = %item.job_id, %err, "recoverable error in outbox drain; will retry later");
                    item.attempts += 1;
                    if let Ok(updated) = serde_json::to_vec_pretty(&item) {
                        let _ = tokio::fs::write(&file_path, &updated).await;
                    }
                    continue;
                }
            }
        }
    }

    drained_count
}

/// Spawna um worker periódico para drenar relatórios pendentes no spool outbox (§P0-2, §P2-5).
pub fn spawn_outbox_drain_worker(
    dir: PathBuf,
    client: Arc<dyn ReportClient>,
    interval: Duration,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if *shutdown_rx.borrow() {
            return;
        }
        let mut ticker = tokio::time::interval(interval);
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let count = drain_outbox(&dir, client.as_ref()).await;
                    if count > 0 {
                        tracing::debug!(drained = count, "drained outbox reports");
                    }
                }
                res = shutdown_rx.changed() => {
                    if res.is_err() || *shutdown_rx.borrow() {
                        tracing::info!("outbox drain worker: shutdown signal recebido, encerrando");
                        break;
                    }
                }
            }
        }
    })
}
