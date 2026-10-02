use serde::{Deserialize, Serialize};

/// Alerta de job persistido em `job_alerts` (fatia 3c).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobAlert {
    pub id: String,
    pub job_id: String,
    pub rule_id: String,
    pub severity: String,
    pub message: String,
    pub fired_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
}

/// Resposta de `GET /api/jobs/:id/alerts` e `GET /internal/jobs/:id/alerts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobAlertsResponse {
    pub items: Vec<JobAlert>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_alert_roundtrip() {
        let alert = JobAlert {
            id: "11111111-1111-1111-1111-111111111111".into(),
            job_id: "22222222-2222-2222-2222-222222222222".into(),
            rule_id: "nan_detected".into(),
            severity: "critical".into(),
            message: "NaN detectado nos gradientes/loss".into(),
            fired_at: "2026-10-02T12:00:00Z".into(),
            resolved_at: None,
        };

        let json = serde_json::to_string(&alert).unwrap();
        assert!(json.contains("\"ruleId\":\"nan_detected\""));
        assert!(!json.contains("resolvedAt"));

        let deserialized: JobAlert = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, alert);
    }
}
