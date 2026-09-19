//! Telemetria do orchestrator (Fatia 7): CPU/RAM de host, GPU via nvidia-smi
//! e parsing/agregação de linhas de metrics/telemetry JSONL.
//!
//! Fachada pura de re-exports; compatibilidade preservada via `crate::lib`.

pub mod gpu;
pub mod host;
pub mod metrics;

pub use gpu::*;
pub use host::*;
pub use metrics::*;
