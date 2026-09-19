use std::path::Path;

use async_trait::async_trait;

#[async_trait]
pub trait S3Port: Send + Sync {
    /// Faz GET de um objeto S3 para um arquivo local.
    async fn get_to_file(&self, key: &str, path: &Path) -> Result<(), String> {
        self.get_to_file_with_progress(key, path, None).await
    }
    /// Faz GET de um objeto S3 para um arquivo local com callback de progresso opcional (bytes_baixados, total_bytes).
    async fn get_to_file_with_progress(
        &self,
        key: &str,
        path: &Path,
        _on_progress: Option<&(dyn Fn(u64, Option<u64>) + Send + Sync)>,
    ) -> Result<(), String> {
        self.get_to_file(key, path).await
    }
    /// Faz PUT de um arquivo local para um objeto S3.
    async fn put(&self, key: &str, path: &Path) -> Result<(), String>;
    /// Verifica se o bucket é acessível (para /ready).
    async fn ping(&self) -> bool;
}
