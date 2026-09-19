//! heph-contracts: Contratos, DTOs e tipos de protocolo compartilhados do Hephaestus LLM Studio.

pub mod artifacts;
pub mod dispatch;
pub mod heartbeat;
pub mod report;
pub mod telemetry;

pub use artifacts::{ArtifactItem, ArtifactReport};
pub use dispatch::{
    DispatchRequest, InitImageRef, LoraRefStage, PackageRef, WeightRef, WeightsRef,
};
pub use heartbeat::{HeartbeatBody, HeartbeatRequest};
pub use report::{ReportBody, ReportRequest};
pub use telemetry::{JobTelemetryEvent, MetricsItem};
