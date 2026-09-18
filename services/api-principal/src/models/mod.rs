//! Módulo de modelos (I.4a — ADR-0012 D3/D4).
//!
//! Upload de pesos (multipart) e download por URL (server-side no principal).
//! Handlers + validação pura; reutiliza padrões de `src/datasets/` e `package.rs`.

pub mod chunk;
pub mod handlers;
pub mod validate;
