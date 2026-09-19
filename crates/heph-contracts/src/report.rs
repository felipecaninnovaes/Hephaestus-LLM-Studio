use crate::artifacts::ArtifactItem;
use serde::{Deserialize, Serialize};

/// Relatório de progresso/estado emitido pelo executor ou BFF (POST /internal/jobs/:id/report).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportBody {
    pub status: String,
    pub progress: Option<f64>,
    pub epoch: Option<i32>,
    pub step: Option<i32>,
    pub metrics: Option<serde_json::Value>,
    pub error: Option<String>,
    pub artifacts: Option<Vec<ArtifactItem>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta_content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Alias para compatibilidade com o manager.
pub type ReportRequest = ReportBody;
