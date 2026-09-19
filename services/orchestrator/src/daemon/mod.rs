//! Daemon de inferência por nó (D1 — ADR-0023).
//!
//! Gerencia o ciclo de vida de um daemon HTTP de difusão:
//! spawn, health check, geração via HTTP, preempção e idle TTL kill.
//!
//! O daemon state vive no processo do orchestrator (1 orchestrator = 1 nó).

pub mod client;
pub mod launcher;
pub mod lifecycle;
pub mod state;
pub mod types;

pub use client::*;
pub use launcher::*;
pub use lifecycle::*;
pub use state::*;
pub use types::*;

#[cfg(test)]
mod tests;
