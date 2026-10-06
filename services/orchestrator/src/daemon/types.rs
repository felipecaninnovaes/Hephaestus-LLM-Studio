//! Tipos de wire do daemon de inferência (D1 — ADR-0023).

use serde::{Deserialize, Serialize};

/// Mounts do container do daemon — IDÊNTICOS aos do one-shot (`/datasets`,
/// `/outputs`), para que todo path staged pelo orquestrador (loras, init,
/// custom checkpoint, text encoder, control, config.yaml) valha no daemon sem
/// tradução. Não mudar sem mudar o staging em `app/mod.rs`.
pub const DAEMON_DATASETS_MOUNT: &str = "/datasets";
pub const DAEMON_OUTPUTS_MOUNT: &str = "/outputs";

/// Body do `POST /generate`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateBody {
    pub config: String,
    pub output_dir: String,
    pub telemetry_path: String,
}

impl GenerateBody {
    /// Monta o body com `output_dir`/`telemetry_path` no namespace do container
    /// do daemon (`/outputs/<job>`), NÃO no path do orquestrador.
    pub fn for_job(config: String, job_id: &str) -> Self {
        let output_dir = format!("{DAEMON_OUTPUTS_MOUNT}/{job_id}");
        Self {
            telemetry_path: format!("{output_dir}/telemetry.jsonl"),
            output_dir,
            config,
        }
    }
}

/// Resposta do `GET /health` do daemon.
/// Contrato real do serve.py: `{"ok": bool, "loaded_spec": dict|null, "busy": bool, ...}`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub ok: bool,
    #[serde(default)]
    pub loaded_spec: Option<serde_json::Value>,
    #[serde(default)]
    pub busy: bool,
    /// Ignora campos extras que o engine possa adicionar no futuro.
    #[serde(flatten)]
    pub _extra: std::collections::HashMap<String, serde_json::Value>,
}
