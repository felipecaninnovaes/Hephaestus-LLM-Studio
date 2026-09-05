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
}
