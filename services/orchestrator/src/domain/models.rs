// DTOs de dispatch/reports vindos da crate compartilhada (Wave 1 — RD-010).
pub use heph_contracts::artifacts::ArtifactReport;
pub use heph_contracts::dispatch::{
    DispatchRequest, InitImageRef, LoraRefStage, PackageRef, WeightRef, WeightsRef,
};
pub use heph_contracts::heartbeat::HeartbeatBody;
pub use heph_contracts::report::ReportBody;

pub fn default_mode() -> String {
    "train".to_string()
}
