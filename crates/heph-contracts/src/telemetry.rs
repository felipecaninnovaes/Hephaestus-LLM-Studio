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

/// Métrica individual por epoch (camelCase wire; mAP50-95 → map5095).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsItem {
    pub epoch: i32,
    pub box_loss: f64,
    pub cls_loss: f64,
    pub dfl_loss: f64,
    pub map50: f64,
    pub map5095: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loss: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lr: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<f64>,
    // legado pré-0013: só jobs antigos têm fase dentro das métricas; Job.phase não lê mais daqui.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vram_used_gb: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_item_camel_case() {
        let item = MetricsItem {
            epoch: 1,
            box_loss: 0.1,
            cls_loss: 0.2,
            dfl_loss: 0.3,
            map50: 0.8,
            map5095: 0.6,
            loss: Some(0.5),
            lr: Some(0.001),
            step: Some(100),
            progress: Some(0.25),
            phase: Some("train".into()),
            message: Some("epoch 1".into()),
            vram_used_gb: Some(4.2),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"boxLoss\":0.1"));
        assert!(json.contains("\"clsLoss\":0.2"));
        assert!(json.contains("\"dflLoss\":0.3"));
        assert!(json.contains("\"map50\":0.8"));
        assert!(json.contains("\"map5095\":0.6"));
        assert!(json.contains("\"vramUsedGb\":4.2"));
    }
}
