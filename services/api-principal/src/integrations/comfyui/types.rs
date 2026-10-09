//! Wire (camelCase) dos destinos/exportações ComfyUI + validações puras.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComfyTargetResponse {
    pub id: Uuid,
    pub name: String,
    pub base_url: String,
    /// O token nunca volta em resposta; só a indicação de que existe.
    pub has_token: bool,
    pub last_check_at: Option<DateTime<Utc>>,
    pub last_check_ok: Option<bool>,
    pub last_check_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateTargetRequest {
    pub name: String,
    pub base_url: String,
    pub token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PatchTargetRequest {
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateExportRequest {
    pub model_id: Option<Uuid>,
    pub job_id: Option<Uuid>,
    pub artifact_id: Option<Uuid>,
    #[serde(default)]
    pub overwrite: bool,
}

/// Origem pedida: exatamente uma forma (`modelId` OU `jobId`+`artifactId`).
#[derive(Debug, PartialEq, Eq)]
pub enum ExportSourceRef {
    Model(Uuid),
    Artifact { job_id: Uuid, artifact_id: Uuid },
}

impl CreateExportRequest {
    pub fn source(&self) -> Option<ExportSourceRef> {
        match (self.model_id, self.job_id, self.artifact_id) {
            (Some(m), None, None) => Some(ExportSourceRef::Model(m)),
            (None, Some(job_id), Some(artifact_id)) => Some(ExportSourceRef::Artifact {
                job_id,
                artifact_id,
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComfyExportResponse {
    pub id: Uuid,
    pub target_id: Uuid,
    pub target_name: String,
    pub filename: String,
    pub status: String,
    pub bytes_total: i64,
    pub bytes_sent: i64,
    pub error: Option<String>,
    /// Código estável p/ máquina quando `failed` (do custom node ou do BFF).
    pub error_code: Option<String>,
    pub remote_path: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub const MAX_NAME_CHARS: usize = 100;
pub const MAX_TOKEN_CHARS: usize = 512;

pub fn valid_name(raw: &str) -> Option<String> {
    let n = raw.trim();
    (!n.is_empty() && n.chars().count() <= MAX_NAME_CHARS).then(|| n.to_string())
}

pub fn valid_token(raw: &str) -> Option<String> {
    let t = raw.trim();
    (!t.is_empty() && t.chars().count() <= MAX_TOKEN_CHARS && !t.chars().any(char::is_control))
        .then(|| t.to_string())
}

/// `http`/`https`, com host, sem userinfo/query/fragmento; devolve sem barra final.
pub fn normalize_base_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.len() > 2048 {
        return None;
    }
    let url = url::Url::parse(trimmed).ok()?;
    let ok = matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none();
    ok.then(|| trimmed.trim_end_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_aceita_http_e_https_sem_barra_final() {
        assert_eq!(
            normalize_base_url("http://10.0.0.5:8188/").as_deref(),
            Some("http://10.0.0.5:8188")
        );
        assert_eq!(
            normalize_base_url(" https://x.proxy.runpod.net ").as_deref(),
            Some("https://x.proxy.runpod.net")
        );
        assert_eq!(
            normalize_base_url("https://h/comfy//").as_deref(),
            Some("https://h/comfy")
        );
    }

    #[test]
    fn base_url_rejeita_esquema_userinfo_query_e_lixo() {
        for bad in [
            "ftp://h",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "http://u:p@h",
            "http://u@h",
            "http://h?x=1",
            "http://h#f",
            "h:8188",
            "",
            "não é url",
        ] {
            assert_eq!(normalize_base_url(bad), None, "{bad}");
        }
    }

    #[test]
    fn origem_do_export_exige_exatamente_uma_forma() {
        let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let req = |m, j, art| CreateExportRequest {
            model_id: m,
            job_id: j,
            artifact_id: art,
            overwrite: false,
        };
        assert_eq!(
            req(Some(a), None, None).source(),
            Some(ExportSourceRef::Model(a))
        );
        assert_eq!(
            req(None, Some(b), Some(c)).source(),
            Some(ExportSourceRef::Artifact {
                job_id: b,
                artifact_id: c
            })
        );
        assert_eq!(req(Some(a), Some(b), Some(c)).source(), None);
        assert_eq!(req(Some(a), Some(b), None).source(), None);
        assert_eq!(req(None, Some(b), None).source(), None);
        assert_eq!(req(None, None, None).source(), None);
    }
}
