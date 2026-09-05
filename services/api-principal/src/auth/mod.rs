//! Auth single-user (ADR-0001, Fatia 2A): boot D3 + segmentos de credencial.
//!
//! `AppState` é o estado compartilhado do boot D3 (pool, segredo, flags).
//! Gate/middleware e inventário declarativo (D9/D8) entram na 2B.

pub mod handlers;
pub mod password;
pub mod secret;
pub mod session;

use sqlx::PgPool;

/// Estado construído no boot (D3): pool → migrations → segredo → bootstrap.
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub jwt_secret: [u8; 32],
    pub secure_cookie: bool,
    /// `true` quando `users` está vazia (STUDIO_PASSWORD ausente no 1º boot).
    pub setup_required: bool,
}
