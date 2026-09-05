//! Storage de objetos (ADR-0003 D8). Na 3b SOMENTE o principal fala S3;
//! s3.rs chega na 3b.4, keys.rs/sniff.rs na 3b.3.
pub mod keys;
pub mod mock;
pub mod port;
pub mod s3;
pub mod sniff;

pub use mock::MockStorage;
pub use port::{StorageConfig, StorageError, StoragePort};
pub use s3::S3Storage;
