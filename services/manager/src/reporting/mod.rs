//! Módulo de relatórios, persistência de métricas e catalogação de artefatos (MM-11).

pub mod artifacts;
pub mod generations;
pub mod metrics;
pub mod models;
pub mod report;

pub use artifacts::*;
pub use generations::*;
pub use metrics::*;
pub use models::*;
pub use report::*;
