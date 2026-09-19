use serde::{Deserialize, Serialize};

/// Evento estruturado de telemetria emitido durante a execução de um job (ADR-0021).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobTelemetryEvent {
    pub timestamp: String,
    pub phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_message: Option<String>,
    pub progress: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_steps: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epoch: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_epochs: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vram_used_gb: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<serde_json::Value>,
}

/// Métrica individual por epoch (camelCase wire).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsItem {
    pub epoch: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub box_loss: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cls_loss: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dfl_loss: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub map50: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub map5095: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loss: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lr: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<i32>,
}
