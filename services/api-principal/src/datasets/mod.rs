//! Domínio datasets (principal, Postgres §10).
//!
//! Blobs em disco e a tabela `images` chegam na 3b; a 3a é
//! metadados + classes, sem escrita em filesystem.

pub mod export;
pub mod handlers;
pub mod models;
