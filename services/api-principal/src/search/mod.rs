//! Busca semântica — porta de embedding (ADR-0004 D1, fatia 3f.2).
//!
//! Análoga a `StoragePort`: o principal consome `EmbeddingPort` via
//! `Arc<dyn>` no `AppState`; `MockEmbedder` (default dev, sem rede) e
//! `HttpEmbedder` (embedder real atrás de `ENGINE_MOCK`) implementam.

pub mod embed;

pub use embed::{EmbedderConfig, EmbeddingError, EmbeddingPort, HttpEmbedder, MockEmbedder, DIM};
