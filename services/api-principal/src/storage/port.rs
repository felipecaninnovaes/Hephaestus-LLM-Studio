//! Porta de storage de objetos (ADR-0003 D8/D10).

use std::path::Path;

/// Erro da porta de storage.
///
/// Doc: mapeia a 503 `storage_unavailable` no handler (ADR-0003 D10).
#[derive(Debug)]
pub enum StorageError {
    /// Backend indisponível ou falha de I/O (mensagem estática; nunca credencial/URL).
    Unavailable(String),
    /// Chave inexistente.
    NotFound,
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(_) => write!(f, "storage unavailable"),
            Self::NotFound => write!(f, "storage object not found"),
        }
    }
}

impl std::error::Error for StorageError {}

/// Config do storage: bucket entra no `source` derivado (D5) e no presigned;
/// `public_endpoint: None` → modo proxy (flag D3); `url_ttl_secs` default 3600.
#[derive(Debug, Clone)]
pub struct StorageConfig {
    /// Bucket S3 (entra no `source` derivado e no presigned).
    pub bucket: String,
    /// Endpoint público p/ URLs assinadas; `None` → modo proxy.
    pub public_endpoint: Option<String>,
    /// TTL das URLs assinadas em segundos (default 3600).
    pub url_ttl_secs: u64,
}

/// Porta de storage (D8): 5 métodos.
#[async_trait::async_trait]
pub trait StoragePort: Send + Sync {
    /// D2/spool: o handler grava tempfile e faz PUT com content-length exato;
    /// a porta recebe CAMINHO, nunca stream de tamanho desconhecido nem bytes
    /// no heap do principal.
    async fn put(&self, key: &str, path: &Path) -> Result<(), StorageError>;
    /// Fallback /data (D3); corpo total ≤ 200 MB (limite da rota).
    async fn get(&self, key: &str) -> Result<Vec<u8>, StorageError>;
    /// TTL/bucket vêm do StorageConfig na implementação; assinado no endpoint
    /// PÚBLICO quando configurado (D3, gotcha SigV4 do Host).
    async fn presign_get(&self, key: &str) -> Result<String, StorageError>;
    /// Compensação D7 (objeto→linha→compensação); best-effort: falha loga,
    /// não estoura 500.
    async fn delete(&self, key: &str) -> Result<(), StorageError>;
    /// Sweep pós-commit do DELETE dataset (D7), paginado 1000 por lote na
    /// impl real; retorna contagem de objetos removidos.
    async fn delete_prefix(&self, prefix: &str) -> Result<u32, StorageError>;
}
