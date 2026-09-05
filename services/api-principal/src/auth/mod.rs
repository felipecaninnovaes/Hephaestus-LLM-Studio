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

// Re-export preserva o path histórico `auth::AppState` usado por
// `tests/contract.rs` e `main.rs`; a definição vive em `crate::state`.
pub use crate::state::AppState;
