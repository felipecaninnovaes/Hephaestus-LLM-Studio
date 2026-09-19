//! Adaptadores de infraestrutura do orchestrator (Fatia 7): executores de
//! trainer, sweepers de boot e clientes HTTP de report/heartbeat.
//!
//! Fachada pura de re-exports; compatibilidade preservada via `crate::lib`.

pub mod executor_docker;
pub mod executor_subprocess;
pub mod heartbeat_http;
pub mod report_http;
pub mod sweeper;

pub use executor_docker::*;
pub use executor_subprocess::*;
pub use heartbeat_http::*;
pub use report_http::*;
pub use sweeper::*;
