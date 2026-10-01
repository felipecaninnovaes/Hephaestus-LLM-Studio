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
    pub vram_reserved_gb: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_time_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_formatted: Option<String>,
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

    #[test]
    fn test_job_telemetry_event_v2_round_trip() {
        // Linha real do formato emitido por `TelemetryEmitter.emit()`
        // (engines/engine-kit/src/engine_kit/telemetry.py:111-144).
        let raw = r#"{
            "timestamp": "2026-10-01T12:00:00+00:00",
            "phase": "training",
            "phaseMessage": "step 10/100",
            "progress": 0.1,
            "step": 10,
            "totalSteps": 100,
            "epoch": 1,
            "totalEpochs": 5,
            "vramUsedGb": 8.5,
            "vramReservedGb": 9.2,
            "stepTimeSeconds": 1.23,
            "speed": "1.2s/step",
            "etaSeconds": 110,
            "etaFormatted": "1m 50s"
        }"#;
        let event: JobTelemetryEvent = serde_json::from_str(raw).unwrap();
        assert_eq!(event.vram_reserved_gb, Some(9.2));
        assert_eq!(event.step_time_seconds, Some(1.23));
        assert_eq!(event.eta_seconds, Some(110));
        assert_eq!(event.eta_formatted.as_deref(), Some("1m 50s"));

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"vramReservedGb\":9.2"));
        assert!(json.contains("\"stepTimeSeconds\":1.23"));
        assert!(json.contains("\"etaSeconds\":110"));
        assert!(json.contains("\"etaFormatted\":\"1m 50s\""));

        let round_tripped: JobTelemetryEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, event);
    }
}
