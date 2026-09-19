//! Camada de storage do orchestrator (Fatia 5): chaves S3 escopadas,
//! arquivo seguro (md5/zip), cliente S3 real e cache local de pesos.
//!
//! Compatibilidade pública preservada via re-exports em `crate::lib`.

pub mod archive;
pub mod cache;
pub mod s3;
pub mod scope;

pub use archive::*;
pub use cache::*;
pub use s3::*;
pub use scope::*;
