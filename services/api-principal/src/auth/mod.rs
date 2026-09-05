//! Auth single-user (ADR-0001, Fatias 2A + 2B).
//!
//! `AppState` é o estado compartilhado do boot D3. `gate`/`routes` (D9/D8)
//! vivem aqui desde a 2B; handlers + helpers vieram da 2A.

pub mod gate;
pub mod handlers;
pub mod password;
pub mod routes;
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
