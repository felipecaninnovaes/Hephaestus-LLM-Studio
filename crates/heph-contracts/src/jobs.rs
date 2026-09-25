use serde::{Deserialize, Serialize};

/// Job persistido e gerenciado pelo manager (snake_case interno).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
    /// AC-006-A D3: último status de fase do job (coluna jobs.phase).
    #[serde(default)]
    pub phase: Option<String>,
    /// AC-006-A D3: última mensagem de status do job (coluna jobs.message).
    #[serde(default)]
    pub message: Option<String>,
}

/// Item da fila de execução (snake_case interno).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueueItem {
    pub job_id: String,
    pub position: i32,
    #[serde(default)]
    pub queue_reason: Option<String>,
}

/// Artefato registrado de um job (snake_case interno).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactRow {
    pub id: String,
    pub kind: String,
    pub path: String,
    pub md5: String,
    pub bytes: i64,
}

/// Resposta do manager ao criar job (snake_case interno).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateJobResponse {
    pub job_id: String,
    pub status: String,
    #[serde(default)]
    pub queue_position: Option<i32>,
}

/// Resposta do manager ao abortar job (snake_case interno).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AbortJobResponse {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
}

impl AbortJobResponse {
    pub fn new(status: impl Into<String>) -> Self {
        Self {
            status: status.into(),
            job_id: None,
        }
    }
}

/// POST /internal/jobs/:id/prepare-complete (ADR-0025 D1).
/// Wire camelCase com aliases snake_case (tolerante ao BFF).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareCompleteRequest {
    #[serde(alias = "dataset_version_id")]
    pub dataset_version_id: String,
    #[serde(alias = "package_ref")]
    pub package_ref: PreparePackageRef,
}

/// Pacote construído pelo worker de preparação (sem version_id: ele é o
/// dataset_version_id do envelope).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparePackageRef {
    pub key: String,
    #[serde(alias = "md5_zip")]
    pub md5_zip: String,
    pub bytes: i64,
}

/// POST /internal/jobs/:id/prepare-fail (ADR-0025 D1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareFailRequest {
    pub code: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_row_serde_defaults() {
        let json = serde_json::json!({
            "id": "job-1",
            "kind": "train",
            "engine": "yolo",
            "model": "yolov8n",
            "mode": "train",
            "dataset_id": null,
            "status": "queued",
            "queue_reason": null,
            "queue_position": 1,
            "progress": null,
            "epoch": null,
            "step": null,
            "metrics": null,
            "vram_min_gb": 4,
            "orchestrator_id": null,
            "orchestrator_name": null,
            "orchestrator_kind": null,
            "created_at": "2026-03-01T00:00:00Z",
            "finished_at": null,
        });

        let row: JobRow = serde_json::from_value(json).unwrap();
        assert_eq!(row.id, "job-1");
        assert!(!row.orchestrator_fallback);
        assert_eq!(row.error, None);
        assert_eq!(row.params, None);
        assert_eq!(row.phase, None);
        assert_eq!(row.message, None);
    }

    #[test]
    fn test_prepare_complete_request_alias() {
        let json = serde_json::json!({
            "dataset_version_id": "v-1",
            "package_ref": {
                "key": "packages/pkg.zip",
                "md5_zip": "abc",
                "bytes": 100
            }
        });
        let req: PrepareCompleteRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.dataset_version_id, "v-1");
        assert_eq!(req.package_ref.key, "packages/pkg.zip");
        assert_eq!(req.package_ref.md5_zip, "abc");
        assert_eq!(req.package_ref.bytes, 100);
    }
}
