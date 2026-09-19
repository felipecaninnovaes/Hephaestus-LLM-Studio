//! Camada HTTP do orchestrator (Fatia 3 — modularização).
//!
//! Handlers, middlewares, estado compartilhado e construção do
//! `axum::Router`, extraídos de `main.rs` sem mudança de comportamento.
//! `main.rs` mantém apenas boot, wiring e heartbeat loop.

pub mod handlers;
pub mod middleware;
pub mod router;
pub mod state;

pub use router::build_router;
pub use state::AppState;
