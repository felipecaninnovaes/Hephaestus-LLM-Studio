//! heph-contracts: Contratos, DTOs e tipos de protocolo compartilhados do Hephaestus LLM Studio.

pub mod artifacts;
pub mod dispatch;
pub mod heartbeat;
pub mod job_status;
pub mod jobs;
pub mod models;
pub mod nodes;
pub mod report;
pub mod telemetry;

pub use artifacts::{ArtifactItem, ArtifactReport};
pub use dispatch::{
    DispatchRequest, InitImageRef, LoraRefStage, PackageRef, WeightRef, WeightsRef,
};
pub use heartbeat::{HeartbeatBody, HeartbeatRequest};
pub use job_status::JobStatus;
pub use jobs::{
    AbortJobResponse, ArtifactRow, CreateJobResponse, JobRow, PrepareCompleteRequest,
    PrepareFailRequest, PreparePackageRef, QueueItem,
};
pub use models::{GenerationItem, GenerationRow, ModelItem, ModelResponse, StorageUsageResponse};
pub use nodes::{OrchestratorItem, TelemetryResponse};
pub use report::{ReportBody, ReportRequest};
pub use telemetry::{JobTelemetryEvent, MetricsItem};
