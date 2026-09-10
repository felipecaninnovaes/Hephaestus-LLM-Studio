//! Estado compartilhado do boot (D3).
//!
//! A definição vive aqui; `auth` re-exporta o path histórico.
//! Nenhum campo novo entra na 3a — o `storage_dir` de datasets
//! chega na 3b com o upload.

use sqlx::PgPool;

/// Estado construído no boot: pool → migrations → segredo → bootstrap.
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub jwt_secret: [u8; 32],
    pub secure_cookie: bool,
    /// `true` quando `users` está vazia (STUDIO_PASSWORD ausente no 1º boot).
    pub setup_required: bool,
    pub storage: std::sync::Arc<dyn crate::storage::StoragePort>,
    pub storage_config: crate::storage::StorageConfig,
    pub embedder: std::sync::Arc<dyn crate::search::EmbeddingPort>,
    pub embedding_model: String,
    /// Client do manager (BFF, ADR-0007 D3).
    pub manager: std::sync::Arc<dyn crate::jobs::manager_client::ManagerPort>,
    /// Allow-list de hosts para download por URL (E1 — ADR-0012 D4).
    /// Carregada no boot; vazia = download desabilitado (fail-closed).
    pub model_download_allowed_hosts: Vec<String>,
}
