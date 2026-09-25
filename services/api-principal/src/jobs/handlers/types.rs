//! DTOs wire, queries e respostas de jobs (camelCase wire).

use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct JobsQuery {
    pub status: Option<String>,
    pub engine: Option<String>,
}

// ---------------------------------------------------------------------------
// Response types (camelCase wire)
// ---------------------------------------------------------------------------

pub use heph_contracts::telemetry::{JobTelemetryEvent, MetricsItem};

/// Extrai contadores totais (steps/epochs/imagens) a partir de job.params.
pub fn extract_totals_from_params(
    params: Option<&serde_json::Value>,
) -> (Option<i64>, Option<i32>) {
    let p = match params {
        Some(v) if v.is_object() => v,
        _ => return (None, None),
    };
    let total_epochs = p
        .get("epochs")
        .or_else(|| p.pointer("/lora/epochs"))
        .or_else(|| p.pointer("/yolo/epochs"))
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);
    let total_steps = p
        .get("batchSize")
        .or_else(|| p.get("batch_size"))
        .or_else(|| p.get("totalSteps"))
        .or_else(|| p.get("total_steps"))
        .or_else(|| p.get("imagesCount"))
        .or_else(|| p.get("image_count"))
        .and_then(|v| v.as_i64());
    (total_steps, total_epochs)
}

pub trait JobTelemetryEventExt {
    fn from_job_response(job: &JobResponse) -> Self;
}

impl JobTelemetryEventExt for JobTelemetryEvent {
    fn from_job_response(job: &JobResponse) -> Self {
        let timestamp = chrono::Utc::now().to_rfc3339();
        let phase = job
            .phase
            .clone()
            .unwrap_or_else(|| match job.status.as_str() {
                "queued" => "queued".to_string(),
                "dispatched" => "dispatched".to_string(),
                "preparing" => "preparing".to_string(),
                "running" => "running".to_string(),
                "cancelling" => "cancelling".to_string(),
                "done" => "completed".to_string(),
                "failed" => "error".to_string(),
                "cancelled" => "cancelled".to_string(),
                // espelha to_job_response: status desconhecido ecoa cru.
                _ => job.status.clone(),
            });
        let progress = job
            .progress
            .unwrap_or(if job.status == "done" { 1.0 } else { 0.0 });
        let latest_metric = job.metrics.as_ref().and_then(|m| m.last());
        let mut m_obj = serde_json::Map::new();
        if let Some(m) = latest_metric {
            if let Some(loss) = m.loss {
                m_obj.insert("loss".to_string(), serde_json::json!(loss));
            }
            if let Some(lr) = m.lr {
                m_obj.insert("lr".to_string(), serde_json::json!(lr));
            }
            if m.box_loss > 0.0 {
                m_obj.insert("boxLoss".to_string(), serde_json::json!(m.box_loss));
            }
            if m.map50 > 0.0 {
                m_obj.insert("map50".to_string(), serde_json::json!(m.map50));
            }
        }
        let metrics = if !m_obj.is_empty() {
            Some(serde_json::Value::Object(m_obj))
        } else {
            None
        };

        Self {
            timestamp,
            phase,
            phase_message: job.phase_message.clone(),
            progress,
            step: job.step.map(|s| s as i64),
            total_steps: job.total_steps,
            epoch: job.epoch,
            total_epochs: job.total_epochs,
            vram_used_gb: job.vram_used_gb,
            metrics,
        }
    }
}

/// Job response (camelCase wire).
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JobResponse {
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
    pub metrics: Option<Vec<MetricsItem>>,
    pub vram_min_gb: Option<i32>,
    pub orchestrator_id: Option<String>,
    pub orchestrator_name: Option<String>,
    pub orchestrator_kind: Option<String>,
    pub orchestrator_fallback: bool,
    pub created_at: String,
    pub finished_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(rename = "phaseMessage", skip_serializing_if = "Option::is_none")]
    pub phase_message: Option<String>,
    #[serde(rename = "vramUsedGb", skip_serializing_if = "Option::is_none")]
    pub vram_used_gb: Option<f64>,
    #[serde(rename = "totalSteps", skip_serializing_if = "Option::is_none")]
    pub total_steps: Option<i64>,
    #[serde(rename = "totalEpochs", skip_serializing_if = "Option::is_none")]
    pub total_epochs: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}
/// Job list response.
#[derive(Debug, Serialize)]
pub struct JobListResponse {
    pub items: Vec<JobResponse>,
    pub total: i32,
}

/// Queue item (camelCase wire).
#[derive(Debug, Serialize)]
pub struct QueueItem {
    #[serde(rename = "jobId")]
    pub job_id: String,
    pub position: i32,
    #[serde(rename = "queueReason")]
    pub queue_reason: Option<String>,
}

/// Queue list response.
#[derive(Debug, Serialize)]
pub struct QueueResponse {
    pub items: Vec<QueueItem>,
}

/// Artifact (camelCase wire).
#[derive(Debug, Serialize)]
pub struct ArtifactResponse {
    pub id: String,
    pub kind: String,
    pub path: String,
    pub md5: String,
    pub bytes: i64,
}

/// Artifact list response.
#[derive(Debug, Serialize)]
pub struct ArtifactListResponse {
    pub items: Vec<ArtifactResponse>,
}

/// Telemetry response (camelCase wire — D9).
#[derive(Debug, Serialize)]
pub struct TelemetryResponse {
    pub measured: bool,
    #[serde(rename = "vramUsed")]
    pub vram_used: Option<i64>,
    #[serde(rename = "vramTotal")]
    pub vram_total: Option<i64>,
    pub cpu: Option<f64>,
    pub ram: Option<i64>,
    #[serde(rename = "ramTotal")]
    pub ram_total: Option<i64>,
    pub gpus: Vec<String>,
    #[serde(rename = "jobsActive")]
    pub jobs_active: i32,
}

/// Job submission response (camelCase wire — D7 :362-366).
#[derive(Debug, Serialize)]
pub struct SubmitJobResponse {
    #[serde(rename = "jobId")]
    pub job_id: String,
    pub status: String,
    #[serde(rename = "queuePosition")]
    pub queue_position: Option<i32>,
}

/// Abort response (camelCase wire — D7 :372-373).
#[derive(Debug, Serialize)]
pub struct AbortResponse {
    pub status: String,
}

/// AutoTracker apply response (camelCase wire — ADR-0008 D1).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutotrackerApplyResponse {
    /// Número de boxes gravadas.
    pub applied: i64,
    /// Número de boxes ignoradas (classe/imagem inexistente ou cap).
    pub skipped: i64,
    /// Número de imagens que receberam ao menos uma box.
    pub images: i64,
}

#[derive(Deserialize)]
pub struct JobLogsQuery {
    pub offset: Option<i64>,
    pub limit: Option<i64>,
}

/// Linha de log persistida (wire `JobLogLine`, camelCase). Linhas malformadas
/// do jsonl chegam com `message` = linha bruta e demais campos nulos.
#[derive(Debug, Serialize, Clone)]
pub struct JobLogLine {
    pub timestamp: Option<String>,
    pub phase: Option<String>,
    pub message: Option<String>,
    pub progress: Option<f64>,
    pub epoch: Option<i64>,
    pub step: Option<i64>,
}

/// Página de logs (`JobLogPage`): `offset` conta LINHAS RAW jsonl consumidas.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobLogPage {
    pub lines: Vec<JobLogLine>,
    pub next_offset: i64,
    pub eof: bool,
}

/// Job metrics response (camelCase wire).
#[derive(Debug, Serialize)]
pub struct JobMetricsResponse {
    pub items: Vec<MetricsItem>,
}

// Aliases wire/contrato
pub type ListJobsResponse = JobListResponse;
pub type ArtifactItem = ArtifactResponse;
pub type ListArtifactsResponse = ArtifactListResponse;
