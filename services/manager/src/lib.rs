//! Manager service — fachada pública e re-exports estáveis (ADR-0007, F4.3).
//!
//! Toda a lógica de negócio está segregada em módulos especializados:
//! - `config`: carregamento estruturado de ambiente e flags
//! - `constants`: constantes de status, normalização de arquiteturas e erros
//! - `error`: ManagerError com impl IntoResponse e helpers
//! - `http`: extractors tipados (JobId, ModelId, AppJson), middleware, rotas e handlers
//! - `jobs`: tipos DTO, repositório SQL, ciclo de vida, criação, abort e expurgo
//! - `nodes`: cache em memória, heartbeat, telemetria agregada e registro/adoção
//! - `models`: serviço e catálogo canônico da tabela `models`
//! - `generations`: serviço e consultas da tabela `generations`
//! - `reporting`: orquestração atômica de reports, artefatos e métricas
//! - `dispatch`: eleição de nós com VRAM, montagem de payload e disparo ao orquestrador
//! - `policy`: resolução de VRAM e headroom
//! - `watchdog`: timeout de preparação, recovery no boot, tick de offline e GC

pub mod config;
pub mod constants;
pub mod dispatch;
pub mod error;
pub mod generations;
pub mod http;
pub mod jobs;
pub mod models;
pub mod nodes;
pub mod orchestrator;
pub mod policy;
pub mod reporting;
pub mod watchdog;

pub use config::*;
pub use constants::*;
pub use dispatch::*;
pub use error::*;
pub use generations::*;
pub use http::*;
pub use jobs::*;
pub use models::*;
pub use nodes::*;
pub use orchestrator::*;
pub use policy::*;
pub use reporting::*;
pub use watchdog::*;

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
