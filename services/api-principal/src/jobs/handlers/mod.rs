//! Handlers de jobs (BFF do manager, ADR-0007 D3/D7).
//!
//! O principal NÃO lê tabelas jobs/orchestrators/job_artifacts — dono é o
//! manager (ADR-0007 D1/D3/D8). Todas as respostas são camelCase.

pub mod alerts;
pub mod apply;
pub mod artifacts;
pub mod export;
pub mod helpers;
pub mod lifecycle;
pub mod lineage;
pub mod query;
pub mod stream;
pub mod submit;
pub mod types;

#[cfg(test)]
mod tests;

pub use alerts::*;
pub use apply::*;
pub use artifacts::*;
pub use export::*;
pub use helpers::*;
pub use lifecycle::*;
pub use lineage::*;
pub use query::*;
pub use stream::*;
pub use submit::*;
pub use types::*;
