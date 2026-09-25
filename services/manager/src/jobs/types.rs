//! Tipos e DTOs de jobs (MM-13).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub struct CreateJobRequest {
    pub kind: String,
    pub engine: String,
    pub model: String,
    pub mode: String,
    pub dataset_id: Option<String>,
    pub dataset_version_id: Option<String>,
    pub package_ref: Option<PackageRef>,
    pub config_yaml: Option<String>,
    pub params: Option<serde_json::Value>,
    pub vram_min_gb: Option<i32>,
    /// UUID de uma row de `models` para fine-tune (ADR-0012 D5).
    pub weights_id: Option<Uuid>,
    /// Hint opcional de orquestrador para despacho (ADR-0015 D2).
    #[serde(default)]
    pub orchestrator_hint: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PackageRef {
    pub version_id: String,
    pub key: String,
    pub md5_zip: String,
    pub bytes: i64,
}

/// POST /internal/jobs/:id/prepare-complete (ADR-0025 D1).
/// Wire camelCase com aliases snake_case (tolerante ao BFF).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareCompleteRequest {
    #[serde(alias = "dataset_version_id")]
    pub dataset_version_id: String,
    #[serde(alias = "package_ref")]
    pub package_ref: PreparePackageRef,
}

/// Pacote construído pelo worker de preparação (sem version_id: ele é o
/// dataset_version_id do envelope).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparePackageRef {
    pub key: String,
    #[serde(alias = "md5_zip")]
    pub md5_zip: String,
    pub bytes: i64,
}

/// POST /internal/jobs/:id/prepare-fail (ADR-0025 D1).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareFailRequest {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateJobResponse {
    pub job_id: String,
    pub status: String,
    pub queue_position: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobRow {
    pub id: String,
    pub kind: String,
    pub engine: String,
    pub model: String,
    pub mode: String,
    pub dataset_id: Option<String>,
    pub status: String,
    pub queue_reason: Option<String>,
    pub queue_position: Option<i32>,
    pub progress: Option<f64>,
    pub epoch: Option<i32>,
    pub step: Option<i32>,
    pub metrics: Option<serde_json::Value>,
    pub vram_min_gb: Option<i32>,
    pub orchestrator_id: Option<String>,
    pub orchestrator_name: Option<String>,
    pub orchestrator_kind: Option<String>,
    #[serde(default)]
    pub orchestrator_fallback: bool,
    pub created_at: String,
    pub finished_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    /// AC-006-A D3: último status de fase do job (snapshot last-write-wins).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// AC-006-A D3: última mensagem de status do job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactRow {
    pub id: String,
    pub kind: String,
    pub path: String,
    pub md5: String,
    pub bytes: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AbortResponse {
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListJobsResponse {
    pub items: Vec<JobRow>,
    pub total: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactsListResponse {
    pub items: Vec<ArtifactRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeletedJob {
    pub id: String,
    pub status: String,
    /// Paths relativos dos artifacts (informativo p/ UI/toast).
    pub artifacts: Vec<String>,
    /// Chaves S3 completas a apagar (exclui gerações preservadas).
    pub object_keys: Vec<String>,
    /// Linhas do catálogo `models` derivadas deste job e expurgadas (D-a).
    pub models_deleted: i64,
    /// Gerações da galeria preservadas pelo SET NULL na FK (0012).
    pub generations_preserved: i64,
}

/// Resultado do `cleanup_jobs` (limpeza em lote).
#[derive(Debug, Clone, Serialize)]
pub struct CleanupResult {
    pub deleted: i64,
    pub jobs: Vec<DeletedJob>,
    /// União das `object_keys` de todos os jobs (conveniência p/ o sweep).
    pub object_keys: Vec<String>,
}
