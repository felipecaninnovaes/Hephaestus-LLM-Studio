//! Tipos de wire do daemon de inferência (D1 — ADR-0023).

use serde::{Deserialize, Serialize};

/// Body do `POST /generate`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateBody {
    pub config: String,
    pub output_dir: String,
    pub telemetry_path: String,
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
