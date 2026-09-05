//! api-principal — biblioteca (Fatias 2B + 3a).
//!
//! Expõe `auth` e `datasets` para o binário (`src/main.rs`) e para
//! `tests/contract.rs` (inventário D8 contra `packages/contracts/openapi.yaml`).

pub mod auth;
pub mod datasets;
pub mod error;
pub mod state;
