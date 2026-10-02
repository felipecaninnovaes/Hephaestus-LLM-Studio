use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<TrainingDiagnostics>,
}

/// Diagnósticos de treino emitidos pelo engine (fatia 3b/3c, spec §3.5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainingDiagnostics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grad_norm_l2: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nan_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inf_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lr_per_group: Vec<Option<f64>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub lora_norms: BTreeMap<String, Option<f64>>,
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

/// GET /internal/jobs/:id/metrics (manager) e GET /api/jobs/:id/metrics?afterSeq=
/// (BFF, com algum parâmetro novo) — fatia 1a §3.4. Ponto bruto de série
/// temporal (linha de `job_metric_points`); `epoch` é nullable: um ponto sem
/// epoch não é descartado (ao contrário do legado `MetricsItem`, que exige
/// epoch).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricPointWithKey {
    pub seq: i64,
    pub epoch: Option<i32>,
    pub step: i64,
    pub key: String,
    pub value: f64,
    pub ts: String,
}

/// Resposta da rota de pontos brutos (fatia 1a §3.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricPointsResponse {
    pub items: Vec<MetricPointWithKey>,
    pub max_seq: i64,
    pub downsampled: bool,
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

    #[test]
    fn test_job_telemetry_event_diagnostics_round_trip() {
        let raw = r#"{
            "timestamp": "2026-10-02T12:00:00Z",
            "phase": "training",
            "progress": 0.5,
            "diagnostics": {
                "gradNormL2": 1.45,
                "nanCount": 2,
                "infCount": 0,
                "lrPerGroup": [0.0001, null],
                "loraNorms": { "layer1": 0.5 }
            }
        }"#;
        let event: JobTelemetryEvent = serde_json::from_str(raw).unwrap();
        assert!(event.diagnostics.is_some());
        let diag = event.diagnostics.unwrap();
        assert_eq!(diag.nan_count, Some(2));
        assert_eq!(diag.inf_count, Some(0));
        assert_eq!(diag.grad_norm_l2, Some(1.45));
        assert_eq!(diag.lr_per_group.len(), 2);
        assert_eq!(diag.lora_norms.get("layer1"), Some(&Some(0.5)));
    }
}
