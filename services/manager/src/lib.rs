//! Manager service — fila central de jobs (ADR-0007 F4.3).
//!
//! `lib.rs` contém lógica de negócio testável sem axum: criação de jobs,
//! listagem, abort, report, heartbeat, telemetry, recovery, auto-adoção
//! e dispatch ao orquestrador. `main.rs` é a camada fina axum/routes.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Erros
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum ManagerError {
    NotFound,
    NotAbortable,
    InvalidRequest(String),
    PairingInvalid,
    Internal(String),
}

impl std::fmt::Display for ManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "not found"),
            Self::NotAbortable => write!(f, "job not abortable"),
            Self::InvalidRequest(e) => write!(f, "invalid request: {e}"),
            Self::PairingInvalid => write!(f, "pairing_invalid"),
            Self::Internal(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ManagerError {}

// ---------------------------------------------------------------------------
// Tipos de request/response (snake_case interno)
// ---------------------------------------------------------------------------

/// Referência de pesos para fine-tune (ADR-0012 D5 — snake_case interno).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightsRef {
    pub s3_key: String,
    pub md5: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateJobRequest {
    pub kind: String,
    pub engine: String,
    pub model: String,
    pub mode: String,
    pub dataset_id: Option<String>,
    pub dataset_version_id: Option<String>,
    pub package_ref: Option<PackageRef>,
    pub config_yaml: Option<String>,
    pub params: Option<serde_json::Value>,
    pub vram_min_gb: Option<i32>,
    /// UUID de uma row de `models` para fine-tune (ADR-0012 D5).
    pub weights_id: Option<Uuid>,
    /// Hint opcional de orquestrador para despacho (ADR-0015 D2).
    #[serde(default)]
    pub orchestrator_hint: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PackageRef {
    pub version_id: String,
    pub key: String,
    pub md5_zip: String,
    pub bytes: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateJobResponse {
    pub job_id: String,
    pub status: String,
    pub queue_position: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactRow {
    pub id: String,
    pub kind: String,
    pub path: String,
    pub md5: String,
    pub bytes: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReportRequest {
    pub status: String,
    pub progress: Option<f64>,
    pub epoch: Option<i32>,
    pub step: Option<i32>,
    pub metrics: Option<serde_json::Value>,
    pub error: Option<String>,
    pub artifacts: Option<Vec<ArtifactItem>>,
    /// Conteúdo do generation_meta.json (JSONL) — enviado pelo orquestrador
    /// para o hook de generations (D5 — ADR-0023). Campo opcional retrocompat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta_content: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArtifactItem {
    pub kind: String,
    pub path: String,
    pub md5: String,
    pub bytes: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HeartbeatRequest {
    pub endpoint: String,
    pub gpus: Vec<String>,
    pub vram_total: Option<i64>,
    pub vram_used: Option<i64>,
    pub cpu: Option<f64>,
    pub ram: Option<i64>,
    pub ram_total: Option<i64>,
    pub jobs_active: i32,
    /// Maior VRAM individual entre as GPUs (MiB) — capacidade real de 1 job.
    pub max_gpu_mib: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AbortResponse {
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TelemetryResponse {
    pub measured: bool,
    pub vram_used: Option<i64>,
    pub vram_total: Option<i64>,
    pub cpu: Option<f64>,
    pub ram: Option<i64>,
    pub ram_total: Option<i64>,
    pub gpus: Vec<String>,
    pub jobs_active: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListJobsResponse {
    pub items: Vec<JobRow>,
    pub total: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactsListResponse {
    pub items: Vec<ArtifactRow>,
}

// ---------------------------------------------------------------------------
// LoRA / Custom checkpoint resolution (D3/D4 — ADR-0023)
// ---------------------------------------------------------------------------

/// Referência resolvida de LoRA para o dispatch (D3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedLora {
    pub s3_key: String,
    pub md5: String,
    pub scale: f64,
}

/// Referência resolvida de checkpoint custom para o dispatch (D4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedCheckpoint {
    pub s3_key: String,
    pub md5: String,
}

// ---------------------------------------------------------------------------
// Generations (D5 — ADR-0023)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct GenerationRow {
    pub id: String,
    pub job_id: String,
    pub s3_key: String,
    pub thumb_s3_key: Option<String>,
    pub filename: String,
    pub seed: i64,
    pub prompt: String,
    pub negative_prompt: Option<String>,
    pub width: i32,
    pub height: i32,
    pub params: serde_json::Value,
    pub created_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListGenerationsResponse {
    pub items: Vec<GenerationRow>,
    pub total: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeleteGenerationsRequest {
    pub ids: Vec<Uuid>,
}

// ---------------------------------------------------------------------------
// Trait de cliente do orquestrador (mockável para testes)
// ---------------------------------------------------------------------------

#[async_trait]
pub trait OrchestratorClient: Send + Sync {
    async fn post(&self, url: &str, body: &serde_json::Value) -> Result<(), String>;
    async fn post_json(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, String>;
}

/// Cliente HTTP real do orquestrador.
pub struct HttpOrchestratorClient {
    client: reqwest::Client,
    token: Option<String>,
}

impl HttpOrchestratorClient {
    pub fn new(token: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("reqwest client do orchestrator");
        Self { client, token }
    }
}

#[async_trait]
impl OrchestratorClient for HttpOrchestratorClient {
    async fn post(&self, url: &str, body: &serde_json::Value) -> Result<(), String> {
        let mut req = self.client.post(url).json(body);
        if let Some(ref t) = self.token {
            req = req.header("Authorization", format!("Bearer {t}"));
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("orchestrator request: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("orchestrator status: {status}"));
        }
        Ok(())
    }

    async fn post_json(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let mut req = self.client.post(url).json(body);
        if let Some(ref t) = self.token {
            req = req.header("Authorization", format!("Bearer {t}"));
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("orchestrator request: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("orchestrator status: {status}"));
        }
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("orchestrator response body: {e}"))?;
        Ok(json)
    }
}

// ---------------------------------------------------------------------------
// Cache de telemetria
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct TelemetryState {
    pub endpoint: String,
    pub measured: bool,
    pub vram_used: Option<i64>,
    pub vram_total: Option<i64>,
    pub cpu: Option<f64>,
    pub ram: Option<i64>,
    pub ram_total: Option<i64>,
    pub gpus: Vec<String>,
    pub jobs_active: i32,
    pub last_heartbeat: Option<DateTime<Utc>>,
}

impl Default for TelemetryState {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            measured: false,
            vram_used: None,
            vram_total: None,
            cpu: None,
            ram: None,
            ram_total: None,
            gpus: vec![],
            jobs_active: 0,
            last_heartbeat: None,
        }
    }
}

pub type TelemetryCache = Arc<RwLock<std::collections::HashMap<Uuid, TelemetryState>>>;

pub fn new_telemetry_cache() -> TelemetryCache {
    Arc::new(RwLock::new(std::collections::HashMap::new()))
}

// ---------------------------------------------------------------------------
// VRAM table (ADR-0011 D3)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct VramTable {
    pub defaults: VramDefaults,
    pub entries: Vec<VramEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VramDefaults {
    pub headroom_gb: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VramEntry {
    pub engine: String,
    pub model: String,
    pub mode: String,
    pub vram_min_gb: i32,
}

impl VramTable {
    /// Resolve o requisito VRAM para um job: vram_min_gb + headroom.
    /// Entrada faltante ⇒ None (permissivo).
    pub fn resolve_required_gb(&self, engine: &str, model: &str, mode: &str) -> Option<i32> {
        self.entries
            .iter()
            .find(|e| e.engine == engine && e.model == model && e.mode == mode)
            .map(|e| e.vram_min_gb + self.defaults.headroom_gb)
    }

    /// Parse a partir de string YAML. Fail-fast se inválido.
    pub fn parse(yaml: &str) -> Result<Self, String> {
        serde_yaml::from_str(yaml).map_err(|e| format!("vram-table parse: {e}"))
    }
}

// ---------------------------------------------------------------------------
// Funções de banco de dados (business logic)
// ---------------------------------------------------------------------------

/// Valida md5: hex lowercase de 32 chars.
fn is_valid_md5(s: &str) -> bool {
    s.len() == 32
        && s.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// Cria um job. Retorna (job_id, queue_position).
///
/// VRAM policy: `vram_min_gb` é gravado mas ignorado na decisão de fila (no-op
/// sem GPU — D9/R3). Ver `@gpu` para o fluxo real com `waiting_vram`.
pub async fn create_job(
    pool: &PgPool,
    req: CreateJobRequest,
) -> Result<CreateJobResponse, ManagerError> {
    let job_id = Uuid::new_v4();
    let dataset_id: Option<Uuid> = req.dataset_id.as_deref().and_then(|s| s.parse().ok());

    // Monta params: merge package_ref de cima se params não tiver.
    let mut params = req.params.unwrap_or(serde_json::json!({}));
    if params.get("package_ref").is_none() {
        if let Some(ref pr) = req.package_ref {
            if let Ok(v) = serde_json::to_value(pr) {
                params["package_ref"] = v;
            }
        }
    }

    // Resolve weights_id → weights_ref (ADR-0012 D5 — fail-fast no submit).
    // Para predict: também resolve variante do modelo para jobs.model (D5).
    let mut resolved_model = req.model.clone();
    if let Some(weights_id) = req.weights_id {
        let row: Option<(String, String, String, Option<String>)> =
            match sqlx::query_as("SELECT s3_key, hash, engine, model FROM models WHERE id = $1")
                .bind(weights_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| ManagerError::Internal(format!("resolve weights from models: {e}")))?
            {
                Some(r) => Some(r),
                None => {
                    // Fallback: busca em job_artifacts (ex.: checkpoints periódicos por época ou modelos intermediários)
                    let art_row: Option<(Uuid, String, String, String, String)> = sqlx::query_as(
                        "SELECT a.job_id, a.path, a.md5, j.engine, j.model \
                     FROM job_artifacts a \
                     JOIN jobs j ON j.id = a.job_id \
                     WHERE a.id = $1 AND a.kind IN ('checkpoint', 'model')",
                    )
                    .bind(weights_id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| {
                        ManagerError::Internal(format!("resolve weights from artifacts: {e}"))
                    })?;

                    art_row.map(|(job_id, path, md5, engine, model)| {
                        (
                            format!("artifacts/{job_id}/{path}"),
                            md5,
                            engine,
                            Some(model),
                        )
                    })
                }
            };

        match row {
            None => return Err(ManagerError::NotFound),
            Some((s3_key, hash, engine, variant)) => {
                if engine != "yolo" && engine != "world" && engine != "diffusion" {
                    return Err(ManagerError::InvalidRequest(format!(
                        "weights engine must be 'yolo', 'world', or 'diffusion', got '{engine}'"
                    )));
                }
                // Defesa: fine-tune yolo (mode=train) NÃO aceita pesos world nem diffusion.
                if req.engine == "yolo" && req.mode == "train" && engine != "yolo" {
                    return Err(ManagerError::InvalidRequest(format!(
                        "fine-tune weights engine must be 'yolo', got '{engine}'"
                    )));
                }
                // Defesa: treino de difusão exige pesos de difusão.
                if req.engine == "diffusion" && engine != "diffusion" {
                    return Err(ManagerError::InvalidRequest(format!(
                        "diffusion weights engine must be 'diffusion', got '{engine}'"
                    )));
                }
                params["weights_ref"] = serde_json::json!({
                    "s3_key": s3_key,
                    "md5": hash,
                });
                // ADR-0012 D5/I.2b: predict com variant → jobs.model = variante (ex.: yolo11m).
                if req.model == "predict" {
                    if let Some(v) = variant.as_deref() {
                        resolved_model = v.to_string();
                    }
                    // Sem variante (upload/download) → resolved_model = "predict" (literal).
                }
                // ADR-0014 D5: autotracker com engine=world → jobs.model = variante | "world".
                // O req.model é sempre "mock" (D2); a resolução é por engine da row.
                if engine == "world" && req.model != "predict" {
                    if let Some(v) = variant.as_deref() {
                        resolved_model = v.to_string();
                    } else {
                        resolved_model = "world".into();
                    }
                }
            }
        }
    }

    // Validação de orchestrator_hint (ADR-0015 D2).
    if let Some(ref hint_str) = req.orchestrator_hint {
        let hint_uuid = Uuid::parse_str(hint_str).map_err(|_| {
            ManagerError::InvalidRequest("orchestrator_hint must be a valid UUID".into())
        })?;

        let orch_status: Option<(String,)> =
            sqlx::query_as("SELECT status FROM orchestrators WHERE id = $1")
                .bind(hint_uuid)
                .fetch_optional(pool)
                .await
                .map_err(|e| ManagerError::Internal(format!("check orchestrator_hint: {e}")))?;

        match orch_status {
            None => return Err(ManagerError::NotFound),
            Some((status,)) if status != "online" => {
                return Err(ManagerError::InvalidRequest(
                    "nó de execução indisponível (offline ou revogado) — escolha outro ou Automático".into(),
                ));
            }
            _ => {}
        }
        params["orchestrator_hint"] = serde_json::json!(hint_uuid.to_string());
    }

    // -------------------------------------------------------------------------
    // Resolução multi-LoRA + custom checkpoint (D3/D4 — ADR-0023).
    // Params contém `loras: [{modelId, scale}]` e `customModelId` (uuid|null).
    // Lê de forma tolerante: campos ausentes = legado (sem loras/custom).
    // -------------------------------------------------------------------------
    if req.engine == "diffusion" && req.mode == "generate" {
        // Resolve loras.
        if let Some(loras_arr) = params.get("loras").and_then(|v| v.as_array()) {
            if loras_arr.len() > 4 {
                return Err(ManagerError::InvalidRequest(
                    "loras must have at most 4 items".into(),
                ));
            }
            let mut resolved_loras: Vec<ResolvedLora> = Vec::with_capacity(loras_arr.len());
            for (i, lora_entry) in loras_arr.iter().enumerate() {
                let model_id_str = lora_entry
                    .get("modelId")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ManagerError::InvalidRequest(format!("loras[{i}].modelId is required"))
                    })?;
                let model_uuid = Uuid::parse_str(model_id_str).map_err(|_| {
                    ManagerError::InvalidRequest(format!("loras[{i}].modelId must be a valid UUID"))
                })?;
                let scale = lora_entry
                    .get("scale")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(1.0);

                // Resolve model row.
                let row: Option<(String, String, Option<String>, Option<String>)> =
                    sqlx::query_as("SELECT s3_key, hash, kind, arch FROM models WHERE id = $1")
                        .bind(model_uuid)
                        .fetch_optional(pool)
                        .await
                        .map_err(|e| {
                            ManagerError::Internal(format!("resolve lora model {i}: {e}"))
                        })?;

                match row {
                    None => {
                        return Err(ManagerError::InvalidRequest(format!(
                            "lora modelId at index {i} not found"
                        )));
                    }
                    Some((s3_key, hash, kind, _arch)) => {
                        if kind.as_deref() != Some("lora") {
                            return Err(ManagerError::InvalidRequest(format!(
                                "lora modelId at index {i} must have kind='lora', got {:?}",
                                kind
                            )));
                        }
                        resolved_loras.push(ResolvedLora {
                            s3_key,
                            md5: hash,
                            scale,
                        });
                    }
                }
            }
            // Grava loras resolvidos em params.
            if let Ok(v) = serde_json::to_value(&resolved_loras) {
                params["loras"] = v;
            }
        }

        // Resolve customModelId.
        if let Some(custom_id_str) = params.get("customModelId").and_then(|v| v.as_str()) {
            let custom_uuid = Uuid::parse_str(custom_id_str).map_err(|_| {
                ManagerError::InvalidRequest("customModelId must be a valid UUID".into())
            })?;

            let row: Option<(String, String, Option<String>, Option<String>)> =
                sqlx::query_as("SELECT s3_key, hash, kind, arch FROM models WHERE id = $1")
                    .bind(custom_uuid)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| ManagerError::Internal(format!("resolve custom model: {e}")))?;

            match row {
                None => {
                    return Err(ManagerError::InvalidRequest(
                        "customModelId not found".into(),
                    ));
                }
                Some((s3_key, hash, kind, arch)) => {
                    if kind.as_deref() != Some("checkpoint") {
                        return Err(ManagerError::InvalidRequest(format!(
                            "customModelId must have kind='checkpoint', got {:?}",
                            kind
                        )));
                    }
                    let arch_val = arch.as_deref().unwrap_or("");
                    if !matches!(arch_val, "sdxl" | "sd15") {
                        return Err(ManagerError::InvalidRequest(format!(
                            "customModelId arch must be 'sdxl' or 'sd15', got '{arch_val}'"
                        )));
                    }
                    let resolved = ResolvedCheckpoint { s3_key, md5: hash };
                    if let Ok(v) = serde_json::to_value(&resolved) {
                        params["custom_checkpoint"] = v;
                    }
                }
            }
        }
    }

    sqlx::query(
        "INSERT INTO jobs (id, kind, engine, model, mode, dataset_id, params, config_yaml, vram_min_gb, status, queue_reason) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'queued', NULL)",
    )
    .bind(job_id)
    .bind(&req.kind)
    .bind(&req.engine)
    .bind(&resolved_model)
    .bind(&req.mode)
    .bind(dataset_id)
    .bind(&params)
    .bind(&req.config_yaml)
    .bind(req.vram_min_gb)
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("insert job: {e}")))?;

    // Posição na fila: quantos jobs queued têm created_at menor.
    let queue_pos: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM jobs WHERE status = 'queued' AND created_at < (SELECT created_at FROM jobs WHERE id = $1)",
    )
    .bind(job_id)
    .fetch_one(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("queue position: {e}")))?;

    Ok(CreateJobResponse {
        job_id: job_id.to_string(),
        status: "queued".to_string(),
        queue_position: Some((queue_pos.0 + 1) as i32),
    })
}

/// Lista jobs com filtros opcionais.
///
/// Cada item carrega `queue_position` (= posição na fila se status=queued,
/// senão null) + `queue_reason`. A fila deriva dos items (status=queued,
/// ordenados por position) — não existe mais payload separado de fila.
pub async fn list_jobs(
    pool: &PgPool,
    status: Option<&str>,
    engine: Option<&str>,
) -> Result<ListJobsResponse, ManagerError> {
    // Pré-computa posições da fila (inline — list_queue foi removida).
    let queue_rows: Vec<(Uuid,)> =
        sqlx::query_as("SELECT id FROM jobs WHERE status = 'queued' ORDER BY created_at")
            .fetch_all(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("queue positions: {e}")))?;
    let pos_map: std::collections::HashMap<String, i32> = queue_rows
        .into_iter()
        .enumerate()
        .map(|(i, (id,))| (id.to_string(), (i + 1) as i32))
        .collect();

    let mut query = String::from(
        "SELECT j.id, j.kind, j.engine, j.model, j.mode, j.dataset_id, j.status, j.queue_reason, \
         j.progress, j.epoch, j.step, j.metrics, j.vram_min_gb, j.orchestrator_id, j.created_at, j.finished_at, j.params, \
         o.name AS orchestrator_name, o.kind AS orchestrator_kind, \
         COALESCE((j.params->>'orchestrator_fallback') = 'true', false) AS orchestrator_fallback, \
         j.params->>'error' AS error \
         FROM jobs j \
         LEFT JOIN orchestrators o ON o.id = j.orchestrator_id \
         WHERE 1=1",
    );
    let mut count_query = String::from("SELECT COUNT(*) FROM jobs WHERE 1=1");
    let mut bind_idx: u32 = 1;

    if status.is_some() {
        let clause = format!(" AND j.status = ${bind_idx}");
        query.push_str(&clause);
        count_query.push_str(&format!(" AND status = ${bind_idx}"));
        bind_idx += 1;
    }
    if engine.is_some() {
        let clause = format!(" AND j.engine = ${bind_idx}");
        query.push_str(&clause);
        count_query.push_str(&format!(" AND engine = ${bind_idx}"));
    }
    query.push_str(" ORDER BY j.created_at DESC");

    let mut count_q = sqlx::query_as::<_, (i64,)>(&count_query);
    if let Some(s) = status {
        count_q = count_q.bind(s);
    }
    if let Some(e) = engine {
        count_q = count_q.bind(e);
    }
    let total: (i64,) = count_q
        .fetch_one(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("count jobs: {e}")))?;

    let mut main_q = sqlx::query(&query);
    if let Some(s) = status {
        main_q = main_q.bind(s);
    }
    if let Some(e) = engine {
        main_q = main_q.bind(e);
    }
    let rows = main_q
        .fetch_all(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("list jobs: {e}")))?;

    let items = rows
        .into_iter()
        .map(|r| {
            let id: Uuid = r.get("id");
            let id_str = id.to_string();
            let status: String = r.get("status");
            let queue_position = if status == "queued" {
                pos_map.get(&id_str).copied()
            } else {
                None
            };
            let dataset_id: Option<Uuid> = r.get("dataset_id");
            let orchestrator_id: Option<Uuid> = r.get("orchestrator_id");
            let created_at: DateTime<Utc> = r.get("created_at");
            let finished_at: Option<DateTime<Utc>> = r.get("finished_at");
            JobRow {
                id: id_str,
                kind: r.get("kind"),
                engine: r.get("engine"),
                model: r.get("model"),
                mode: r.get("mode"),
                dataset_id: dataset_id.map(|u| u.to_string()),
                status,
                queue_reason: r.get("queue_reason"),
                queue_position,
                progress: r.get("progress"),
                epoch: r.get("epoch"),
                step: r.get("step"),
                metrics: r.get("metrics"),
                vram_min_gb: r.get("vram_min_gb"),
                orchestrator_id: orchestrator_id.map(|u| u.to_string()),
                orchestrator_name: r.get("orchestrator_name"),
                orchestrator_kind: r.get("orchestrator_kind"),
                orchestrator_fallback: r.get("orchestrator_fallback"),
                created_at: created_at.to_rfc3339(),
                finished_at: finished_at.map(|t| t.to_rfc3339()),
                error: r.get("error"),
                params: r.get("params"),
            }
        })
        .collect();

    Ok(ListJobsResponse {
        items,
        total: total.0 as i32,
    })
}

/// Retorna um job por ID.
pub async fn get_job(pool: &PgPool, id: Uuid) -> Result<JobRow, ManagerError> {
    // Pré-computa posições da fila (inline).
    let queue_rows: Vec<(Uuid,)> =
        sqlx::query_as("SELECT id FROM jobs WHERE status = 'queued' ORDER BY created_at")
            .fetch_all(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("queue positions: {e}")))?;
    let pos_map: std::collections::HashMap<String, i32> = queue_rows
        .into_iter()
        .enumerate()
        .map(|(i, (id,))| (id.to_string(), (i + 1) as i32))
        .collect();

    let row = sqlx::query(
        "SELECT j.id, j.kind, j.engine, j.model, j.mode, j.dataset_id, j.status, j.queue_reason, \
         j.progress, j.epoch, j.step, j.metrics, j.vram_min_gb, j.orchestrator_id, j.created_at, j.finished_at, j.params, \
         o.name AS orchestrator_name, o.kind AS orchestrator_kind, \
         COALESCE((j.params->>'orchestrator_fallback') = 'true', false) AS orchestrator_fallback, \
         j.params->>'error' AS error \
         FROM jobs j \
         LEFT JOIN orchestrators o ON o.id = j.orchestrator_id \
         WHERE j.id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("get job: {e}")))?;

    let r = row.ok_or(ManagerError::NotFound)?;
    let job_id: Uuid = r.get("id");
    let id_str = job_id.to_string();
    let status: String = r.get("status");
    let queue_position = if status == "queued" {
        pos_map.get(&id_str).copied()
    } else {
        None
    };
    let dataset_id: Option<Uuid> = r.get("dataset_id");
    let orchestrator_id: Option<Uuid> = r.get("orchestrator_id");
    let created_at: DateTime<Utc> = r.get("created_at");
    let finished_at: Option<DateTime<Utc>> = r.get("finished_at");

    Ok(JobRow {
        id: id_str,
        kind: r.get("kind"),
        engine: r.get("engine"),
        model: r.get("model"),
        mode: r.get("mode"),
        dataset_id: dataset_id.map(|u| u.to_string()),
        status,
        queue_reason: r.get("queue_reason"),
        queue_position,
        progress: r.get("progress"),
        epoch: r.get("epoch"),
        step: r.get("step"),
        metrics: r.get("metrics"),
        vram_min_gb: r.get("vram_min_gb"),
        orchestrator_id: orchestrator_id.map(|u| u.to_string()),
        orchestrator_name: r.get("orchestrator_name"),
        orchestrator_kind: r.get("orchestrator_kind"),
        orchestrator_fallback: r.get("orchestrator_fallback"),
        created_at: created_at.to_rfc3339(),
        finished_at: finished_at.map(|t| t.to_rfc3339()),
        error: r.get("error"),
        params: r.get("params"),
    })
}

/// Lista artefatos de um job.
pub async fn get_job_artifacts(
    pool: &PgPool,
    job_id: Uuid,
) -> Result<Vec<ArtifactRow>, ManagerError> {
    // Verifica que o job existe.
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM jobs WHERE id = $1)")
        .bind(job_id)
        .fetch_one(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("check job: {e}")))?;
    if !exists {
        return Err(ManagerError::NotFound);
    }

    let rows: Vec<(Uuid, String, String, String, i64)> =
        sqlx::query_as("SELECT id, kind, path, md5, bytes FROM job_artifacts WHERE job_id = $1")
            .bind(job_id)
            .fetch_all(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("list artifacts: {e}")))?;

    Ok(rows
        .into_iter()
        .map(|r| ArtifactRow {
            id: r.0.to_string(),
            kind: r.1,
            path: r.2,
            md5: r.3,
            bytes: r.4,
        })
        .collect())
}

/// Aborta um job.
pub async fn abort_job(
    pool: &PgPool,
    id: Uuid,
    orch_client: &dyn OrchestratorClient,
) -> Result<String, ManagerError> {
    let row: Option<(String, Option<Uuid>)> =
        sqlx::query_as("SELECT status, orchestrator_id FROM jobs WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("get job for abort: {e}")))?;

    let (status, orchestrator_id) = row.ok_or(ManagerError::NotFound)?;

    match status.as_str() {
        "done" | "failed" | "cancelled" => Err(ManagerError::NotAbortable),

        "queued" | "dispatched" => {
            sqlx::query("UPDATE jobs SET status = 'cancelled', queue_reason = NULL WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await
                .map_err(|e| ManagerError::Internal(format!("cancel job: {e}")))?;
            Ok("cancelled".to_string())
        }

        "preparing" | "running" => {
            sqlx::query("UPDATE jobs SET status = 'cancelling' WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await
                .map_err(|e| ManagerError::Internal(format!("set cancelling: {e}")))?;

            // Notifica orquestrador (best-effort).
            if let Some(orch_id) = orchestrator_id {
                if let Ok(orch) = get_orchestrator(pool, orch_id).await {
                    let body = serde_json::json!({"job_id": id.to_string()});
                    let url = format!("{}/internal/abort", orch.endpoint);
                    if let Err(e) = orch_client.post(&url, &body).await {
                        tracing::warn!("failed to notify orchestrator of abort for job {id}: {e}");
                    }
                }
            }

            Ok("cancelling".to_string())
        }

        "cancelling" => Ok("cancelling".to_string()),

        other => Err(ManagerError::Internal(format!(
            "unexpected status: {other}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Metrics helpers — append + dedup por epoch (ADR-0007 D7 :374–376)
// ---------------------------------------------------------------------------

/// Normaliza um valor JSON recebido do orquestrador em um array de objetos
/// de métricas por epoch. Aceita: objeto único, array, ou `{"items":[...]}`.
fn normalize_metrics_to_array(value: &serde_json::Value) -> Vec<serde_json::Value> {
    if let Some(arr) = value.as_array() {
        return arr.clone();
    }
    if let Some(items) = value.get("items").and_then(|v| v.as_array()) {
        return items.clone();
    }
    if value.is_object() {
        return vec![value.clone()];
    }
    vec![]
}

/// Extrai uma chave numérica estável (epoch * 1_000_000 + step) de um objeto de métricas.
/// Permite rastrear múltiplos passos e fases de preparação dentro da mesma época.
fn metrics_key(value: &serde_json::Value) -> Option<i64> {
    let epoch = value.get("epoch").and_then(|v| v.as_i64())?;
    let step = value.get("step").and_then(|v| v.as_i64()).unwrap_or(0);
    Some(epoch * 1_000_000 + step)
}

/// Faz upsert incremental de metrics no banco:
/// lê array existente, normaliza novos, dedup por chave (epoch, step), grava como
/// `{"items": [...]}` (formato esperado por `remap_metrics` no api-principal).
async fn upsert_metrics(
    pool: &PgPool,
    id: Uuid,
    new_metrics: &serde_json::Value,
) -> Result<(), ManagerError> {
    // Lê array existente (NULL → vazio via COALESCE).
    let existing: serde_json::Value =
        sqlx::query_scalar("SELECT COALESCE(metrics, '[]'::jsonb) FROM jobs WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("read metrics: {e}")))?;

    // Mapa chave → objeto (dedup por (epoch, step)).
    let mut metrics_map: std::collections::HashMap<i64, serde_json::Value> =
        std::collections::HashMap::new();

    // 1. Itens existentes.
    for item in normalize_metrics_to_array(&existing) {
        if let Some(k) = metrics_key(&item) {
            metrics_map.insert(k, item);
        }
    }

    // 2. Itens novos (substitui se chave repetida).
    for item in normalize_metrics_to_array(new_metrics) {
        if let Some(k) = metrics_key(&item) {
            metrics_map.insert(k, item);
        }
    }

    // 3. Ordena por chave cronológica e grava como {"items": [...]}.
    let mut items: Vec<serde_json::Value> = metrics_map.into_values().collect();
    items.sort_by_key(|v| metrics_key(v).unwrap_or(0));

    let merged = serde_json::json!({"items": items});
    sqlx::query("UPDATE jobs SET metrics = $2 WHERE id = $1")
        .bind(id)
        .bind(&merged)
        .execute(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("write metrics: {e}")))?;

    Ok(())
}

/// Processa um report do orquestrador.
pub async fn report_job(
    pool: &PgPool,
    id: Uuid,
    report: ReportRequest,
) -> Result<(), ManagerError> {
    // Verifica que o job existe.
    let current: Option<String> = sqlx::query_scalar("SELECT status FROM jobs WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("get job for report: {e}")))?;

    let current_status = current.ok_or(ManagerError::NotFound)?;

    // Guarda de transição: status terminais são imutáveis.
    if current_status == "done" || current_status == "failed" || current_status == "cancelled" {
        tracing::warn!(
            job_id = %id,
            current_status = %current_status,
            report_status = %report.status,
            "report ignorado para job terminal"
        );
        return Ok(());
    }

    // Guarda de transição: cancelling só aceita done/failed/cancelled.
    if current_status == "cancelling"
        && !matches!(report.status.as_str(), "done" | "failed" | "cancelled")
    {
        tracing::warn!(
            job_id = %id,
            current_status = %current_status,
            report_status = %report.status,
            "report ignorado para job em cancelling (aceita apenas done/failed/cancelled)"
        );
        return Ok(());
    }

    match report.status.as_str() {
        "preparing" | "running" => {
            sqlx::query(
                "UPDATE jobs SET status = $2, progress = COALESCE($3, progress), epoch = COALESCE($4, epoch), step = COALESCE($5, step) WHERE id = $1",
            )
            .bind(id)
            .bind(&report.status)
            .bind(report.progress)
            .bind(report.epoch)
            .bind(report.step)
            .execute(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("update job status: {e}")))?;

            // Atualiza metrics se fornecido (append + dedup por epoch).
            if let Some(metrics) = &report.metrics {
                upsert_metrics(pool, id, metrics).await?;
            }

            // Atualiza artifacts intermediários se fornecido (ex.: samples geradas durante o treino).
            if let Some(artifacts) = &report.artifacts {
                for art in artifacts {
                    if is_valid_md5(&art.md5) && art.bytes >= 0 {
                        let exists: bool = sqlx::query_scalar(
                            "SELECT EXISTS (SELECT 1 FROM job_artifacts WHERE job_id = $1 AND path = $2)",
                        )
                        .bind(id)
                        .bind(&art.path)
                        .fetch_one(pool)
                        .await
                        .unwrap_or(false);

                        if !exists {
                            let art_id = Uuid::new_v4();
                            let _ = sqlx::query(
                                "INSERT INTO job_artifacts (id, job_id, kind, path, md5, bytes) VALUES ($1, $2, $3, $4, $5, $6)",
                            )
                            .bind(art_id)
                            .bind(id)
                            .bind(&art.kind)
                            .bind(&art.path)
                            .bind(&art.md5)
                            .bind(art.bytes)
                            .execute(pool)
                            .await;
                        }
                    }
                }
            }
        }

        "done" => {
            // Valida e insere artifacts.
            if let Some(artifacts) = &report.artifacts {
                for art in artifacts {
                    if !is_valid_md5(&art.md5) {
                        return Err(ManagerError::Internal(format!("invalid md5: {}", art.md5)));
                    }
                    if art.bytes < 0 {
                        return Err(ManagerError::Internal(format!(
                            "negative bytes: {}",
                            art.bytes
                        )));
                    }
                }
                // Limpa artifacts existentes (idempotência).
                sqlx::query("DELETE FROM job_artifacts WHERE job_id = $1")
                    .bind(id)
                    .execute(pool)
                    .await
                    .map_err(|e| ManagerError::Internal(format!("delete old artifacts: {e}")))?;

                for art in artifacts {
                    let art_id = Uuid::new_v4();
                    sqlx::query(
                        "INSERT INTO job_artifacts (id, job_id, kind, path, md5, bytes) VALUES ($1, $2, $3, $4, $5, $6)",
                    )
                    .bind(art_id)
                    .bind(id)
                    .bind(&art.kind)
                    .bind(&art.path)
                    .bind(&art.md5)
                    .bind(art.bytes)
                    .execute(pool)
                    .await
                    .map_err(|e| ManagerError::Internal(format!("insert artifact: {e}")))?;
                }
            }

            // Grava metrics (append + dedup por epoch).
            if let Some(metrics) = &report.metrics {
                upsert_metrics(pool, id, metrics).await?;
            }

            // Hook: registra best.pt na tabela models (ADR-0012 D1).
            // Best-effort: falha não impede o report done.
            if let Some(artifacts) = &report.artifacts {
                let best_models: Vec<_> = artifacts
                    .iter()
                    .filter(|a| {
                        a.kind == "model"
                            && (a.path.contains("best")
                                || a.path.contains("adapter")
                                || a.path.ends_with(".safetensors"))
                    })
                    .collect();
                if !best_models.is_empty() {
                    // Lê engine, model, dataset_id, params do job para o INSERT (ADR-0022 D0).
                    let job_info: Option<(String, String, Option<Uuid>, serde_json::Value)> =
                        match sqlx::query_as::<_, (String, String, Option<Uuid>, serde_json::Value)>(
                            "SELECT engine, model, dataset_id, params FROM jobs WHERE id = $1",
                        )
                        .bind(id)
                        .fetch_optional(pool)
                        .await
                        {
                            Ok(opt) => opt,
                            Err(e) => {
                                tracing::warn!(
                                    "hook models: falha ao ler engine/model/dataset_id/params do job {id}: {e}"
                                );
                                None
                            }
                        };
                    if let Some((engine, model, dataset_id, job_params)) = job_info {
                        let dataset_slug: Option<String> = if let Some(ds_id) = dataset_id {
                            match sqlx::query_scalar::<_, String>(
                                "SELECT slug FROM datasets WHERE id = $1",
                            )
                            .bind(ds_id)
                            .fetch_optional(pool)
                            .await
                            {
                                Ok(opt) => opt,
                                Err(e) => {
                                    tracing::warn!(
                                        "hook models: falha ao ler slug do dataset {ds_id}: {e}"
                                    );
                                    None
                                }
                            }
                        } else {
                            None
                        };

                        for art in best_models {
                            let s3_key = format!("artifacts/{id}/{}", art.path);
                            let model_name = compute_model_name(
                                &art.path,
                                &engine,
                                &model,
                                id,
                                dataset_slug.as_deref(),
                                &job_params,
                            );
                            let result = sqlx::query(
                                "INSERT INTO models (id, engine, name, model, s3_key, source, hash, bytes, job_id) \
                                 VALUES ($1, $2, $3, $4, $5, 'train', $6, $7, $8) \
                                 ON CONFLICT (s3_key) DO NOTHING",
                            )
                            .bind(Uuid::new_v4())
                            .bind(&engine)
                            .bind(&model_name)
                            .bind(Some(model.clone()))
                            .bind(&s3_key)
                            .bind(&art.md5)
                            .bind(art.bytes)
                            .bind(id)
                            .execute(pool)
                            .await;
                            if let Err(e) = result {
                                tracing::warn!(
                                    job_id = %id,
                                    s3_key = %s3_key,
                                    error = %e,
                                    "falha ao registrar modelo na tabela models (best-effort)"
                                );
                            }
                        }
                    }
                }
            }

            // Hook generations (D5 — ADR-0023): job diffusion generate done com
            // artefato generated_meta → parse JSONL → INSERT em generations.
            // Best-effort: falha de parse/log não impede o report done.
            {
                let job_meta: Option<(String, String)> =
                    match sqlx::query_as::<_, (String, String)>(
                        "SELECT engine, mode FROM jobs WHERE id = $1",
                    )
                    .bind(id)
                    .fetch_optional(pool)
                    .await
                    {
                        Ok(opt) => opt,
                        Err(e) => {
                            tracing::warn!(
                                "hook generations: falha ao ler engine/mode do job {id}: {e}"
                            );
                            None
                        }
                    };

                if let Some((engine, mode)) = job_meta {
                    if engine == "diffusion" && mode == "generate" {
                        // Procura artefato generated_meta nos artifacts do report.
                        let meta_artifact = report
                            .artifacts
                            .as_ref()
                            .and_then(|arts| arts.iter().find(|a| a.kind == "generated_meta"));

                        if let Some(_meta_art) = meta_artifact {
                            // Usa meta_content enviado pelo orquestrador no report.
                            // Retrocompat: se meta_content não vier, tenta ler de job_artifacts.content.
                            let content: Option<String> = if report.meta_content.is_some() {
                                report.meta_content.clone()
                            } else {
                                sqlx::query_scalar(
                                    "SELECT content FROM job_artifacts WHERE job_id = $1 AND kind = 'generated_meta' LIMIT 1",
                                )
                                .bind(id)
                                .fetch_optional(pool)
                                .await
                                .ok()
                                .flatten()
                            };

                            if let Some(jsonl_content) = content {
                                for line in jsonl_content.lines() {
                                    let line = line.trim();
                                    if line.is_empty() {
                                        continue;
                                    }
                                    match serde_json::from_str::<serde_json::Value>(line) {
                                        Ok(entry) => {
                                            let filename = entry
                                                .get("filename")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            if filename.is_empty() {
                                                continue;
                                            }
                                            let s3_key = format!("artifacts/{id}/{filename}");
                                            let thumb_s3_key = entry
                                                .get("thumb")
                                                .and_then(|v| v.as_str())
                                                .map(|t| format!("artifacts/{id}/{t}"));
                                            let seed = entry
                                                .get("seed")
                                                .and_then(|v| v.as_i64())
                                                .unwrap_or(0);
                                            let prompt = entry
                                                .get("prompt")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            let negative_prompt = entry
                                                .get("negative_prompt")
                                                .and_then(|v| v.as_str());
                                            let width = entry
                                                .get("width")
                                                .and_then(|v| v.as_i64())
                                                .unwrap_or(512)
                                                as i32;
                                            let height = entry
                                                .get("height")
                                                .and_then(|v| v.as_i64())
                                                .unwrap_or(512)
                                                as i32;

                                            // Params = RESTO da linha (batch_index, batch_size, etc.)
                                            let mut gen_params = entry.clone();
                                            // Remove campos já colunas explícitas.
                                            if let Some(obj) = gen_params.as_object_mut() {
                                                obj.remove("filename");
                                                obj.remove("thumb");
                                                obj.remove("seed");
                                                obj.remove("prompt");
                                                obj.remove("negative_prompt");
                                                obj.remove("width");
                                                obj.remove("height");
                                            }

                                            let result = sqlx::query(
                                                "INSERT INTO generations \
                                                 (id, job_id, s3_key, thumb_s3_key, filename, seed, \
                                                  prompt, negative_prompt, width, height, params) \
                                                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
                                                 ON CONFLICT (s3_key) DO NOTHING",
                                            )
                                            .bind(Uuid::new_v4())
                                            .bind(id)
                                            .bind(&s3_key)
                                            .bind(&thumb_s3_key)
                                            .bind(filename)
                                            .bind(seed)
                                            .bind(prompt)
                                            .bind(negative_prompt)
                                            .bind(width)
                                            .bind(height)
                                            .bind(&gen_params)
                                            .execute(pool)
                                            .await;

                                            if let Err(e) = result {
                                                tracing::warn!(
                                                    job_id = %id,
                                                    s3_key = %s3_key,
                                                    error = %e,
                                                    "falha ao inserir generation (best-effort)"
                                                );
                                            }
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                job_id = %id,
                                                error = %e,
                                                "hook generations: falha ao parsear linha do meta (best-effort)"
                                            );
                                        }
                                    }
                                }
                            } else {
                                tracing::warn!(
                                    job_id = %id,
                                    "hook generations: generated_meta sem content em job_artifacts"
                                );
                            }
                        }
                    }
                }
            }

            sqlx::query(
                "UPDATE jobs SET status = 'done', finished_at = now(), progress = 1.0 WHERE id = $1",
            )
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("set done: {e}")))?;
        }

        "failed" => {
            // Merge error into params.
            if let Some(err_msg) = &report.error {
                sqlx::query("UPDATE jobs SET params = params || $2::jsonb WHERE id = $1")
                    .bind(id)
                    .bind(serde_json::json!({"error": err_msg}))
                    .execute(pool)
                    .await
                    .map_err(|e| ManagerError::Internal(format!("merge error: {e}")))?;
            }

            sqlx::query("UPDATE jobs SET status = 'failed', finished_at = now() WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await
                .map_err(|e| ManagerError::Internal(format!("set failed: {e}")))?;
        }

        other => {
            return Err(ManagerError::Internal(format!(
                "invalid report status: {other}"
            )));
        }
    }

    Ok(())
}

/// Recebe heartbeat do orquestrador.
pub async fn receive_heartbeat(
    pool: &PgPool,
    cache: &TelemetryCache,
    req: HeartbeatRequest,
) -> Result<(), ManagerError> {
    // 1. Resolve endpoint → id.
    let row: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM orchestrators WHERE endpoint = $1")
        .bind(&req.endpoint)
        .fetch_optional(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("resolve heartbeat endpoint: {e}")))?;

    let orch_id = match row {
        Some((id,)) => id,
        None => {
            tracing::warn!(
                endpoint = %req.endpoint,
                "heartbeat de endpoint não registrado (orchestrator não adotado ou ORCH_ADVERTISE_URL errado)"
            );
            return Ok(());
        }
    };

    // 2. Atualiza last_heartbeat e status SOMENTE nesta linha.
    let update_result = sqlx::query(
        "UPDATE orchestrators SET last_heartbeat = now(), status = 'online' \
         WHERE id = $1 AND status <> 'revoked'",
    )
    .bind(orch_id)
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("update heartbeat: {e}")))?;

    // Se a linha é revoked, o UPDATE não modificou nada — retorna cedo
    // (sem UPDATE de gpus/vram, sem cache). Nó revoked não deve ser medido.
    if update_result.rows_affected() == 0 {
        tracing::info!(
            orch_id = %orch_id,
            endpoint = %req.endpoint,
            "heartbeat ignorado: nó revoked"
        );
        return Ok(());
    }

    // 3. Grava gpus/vram_total_gb quando heartbeat carrega VRAM e gpus não-vazio.
    //    vram_total_gb = maior GPU individual (round(max_gpu_mib/1024)) — 1 job = 1 GPU.
    //    Fallback: heartbeat sem max_gpu_mib (orquestrador legado) usa a soma (vram_total).
    if !req.gpus.is_empty() {
        let effective_vram_mib = req.max_gpu_mib.or(req.vram_total);
        if let Some(vram_mib) = effective_vram_mib {
            let vram_total_gb = ((vram_mib as f64) / 1024.0).round() as i32;
            let gpus_json = serde_json::to_value(&req.gpus)
                .map_err(|e| ManagerError::Internal(format!("serialize gpus: {e}")))?;
            sqlx::query("UPDATE orchestrators SET gpus = $1, vram_total_gb = $2 WHERE id = $3")
                .bind(gpus_json)
                .bind(vram_total_gb)
                .bind(orch_id)
                .execute(pool)
                .await
                .map_err(|e| ManagerError::Internal(format!("update orchestrator gpus: {e}")))?;
        }
    }

    // 4. Atualiza cache por nó.
    let mut cache = cache.write().await;
    let state = cache.entry(orch_id).or_default();
    state.endpoint = req.endpoint;
    state.measured = true;
    state.vram_used = req.vram_used;
    state.vram_total = req.vram_total;
    state.cpu = req.cpu;
    state.ram = req.ram;
    state.ram_total = req.ram_total;
    state.gpus = req.gpus;
    state.jobs_active = req.jobs_active;
    state.last_heartbeat = Some(Utc::now());

    Ok(())
}

/// Retorna telemetria do cache (agregação global).
pub async fn get_telemetry(pool: &PgPool, cache: &TelemetryCache) -> TelemetryResponse {
    let cache = cache.read().await;
    let now = Utc::now();

    // 0 nós no cache → fallback (comportamento atual: measured:false + jobs da fila).
    if cache.is_empty() {
        let jobs_active: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM jobs WHERE status NOT IN ('done', 'failed', 'cancelled')",
        )
        .fetch_one(pool)
        .await
        .unwrap_or((0,));

        return TelemetryResponse {
            measured: false,
            vram_used: None,
            vram_total: None,
            cpu: None,
            ram: None,
            ram_total: None,
            gpus: vec![],
            jobs_active: jobs_active.0 as i32,
        };
    }

    // 1 nó → exatamente o de hoje (compat total).
    if cache.len() == 1 {
        let state = cache.values().next().unwrap();
        let measured = state
            .last_heartbeat
            .map(|last| (now - last).num_seconds() <= 10)
            .unwrap_or(false);
        // ADR D2.3/R5: nó sem heartbeat fresco → mesmo fallback do 0-nós.
        if !measured {
            let jobs_active: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM jobs WHERE status NOT IN ('done', 'failed', 'cancelled')",
            )
            .fetch_one(pool)
            .await
            .unwrap_or((0,));
            return TelemetryResponse {
                measured: false,
                vram_used: None,
                vram_total: None,
                cpu: None,
                ram: None,
                ram_total: None,
                gpus: vec![],
                jobs_active: jobs_active.0 as i32,
            };
        }
        return TelemetryResponse {
            measured,
            vram_used: state.vram_used,
            vram_total: state.vram_total,
            cpu: state.cpu,
            ram: state.ram,
            ram_total: state.ram_total,
            gpus: state.gpus.clone(),
            jobs_active: state.jobs_active,
        };
    }

    // >1 nós → agregação SOMENTE de entradas frescas (heartbeat ≤ 10s).
    // Entradas stale (offline ou sem heartbeat recente) são ignoradas na soma/união.
    // Se NENHUMA for fresca mas houver entradas → fallback (mesmo do 0-nós).
    let mut vram_used_sum: Option<i64> = Some(0);
    let mut vram_total_sum: Option<i64> = Some(0);
    let mut gpus: Vec<String> = Vec::new();
    let mut jobs_active_sum: i32 = 0;
    let mut measured = false;

    for state in cache.values() {
        let is_fresh = state
            .last_heartbeat
            .map(|last| (now - last).num_seconds() <= 10)
            .unwrap_or(false);

        if is_fresh {
            measured = true;

            // vram_used/vram_total: soma dos Some (None → None global).
            match (vram_used_sum, state.vram_used) {
                (Some(acc), Some(val)) => vram_used_sum = Some(acc + val),
                (Some(_), None) => vram_used_sum = None,
                (None, _) => {}
            }
            match (vram_total_sum, state.vram_total) {
                (Some(acc), Some(val)) => vram_total_sum = Some(acc + val),
                (Some(_), None) => vram_total_sum = None,
                (None, _) => {}
            }

            // gpus: união (ordem estável por nó).
            for gpu in &state.gpus {
                if !gpus.contains(gpu) {
                    gpus.push(gpu.clone());
                }
            }

            // jobs_active: soma.
            jobs_active_sum += state.jobs_active;
        }
    }

    // Nenhuma entrada fresca → fallback (nulls + jobs da fila).
    if !measured {
        let jobs_active: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM jobs WHERE status NOT IN ('done', 'failed', 'cancelled')",
        )
        .fetch_one(pool)
        .await
        .unwrap_or((0,));

        return TelemetryResponse {
            measured: false,
            vram_used: None,
            vram_total: None,
            cpu: None,
            ram: None,
            ram_total: None,
            gpus: vec![],
            jobs_active: jobs_active.0 as i32,
        };
    }

    TelemetryResponse {
        measured,
        vram_used: vram_used_sum,
        vram_total: vram_total_sum,
        cpu: None,
        ram: None,
        ram_total: None,
        gpus,
        jobs_active: jobs_active_sum,
    }
}

// ---------------------------------------------------------------------------
// Orchestrators
// ---------------------------------------------------------------------------

struct OrchestratorRow {
    #[allow(dead_code)]
    id: Uuid,
    endpoint: String,
}

async fn get_orchestrator(pool: &PgPool, id: Uuid) -> Result<OrchestratorRow, ManagerError> {
    let row: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, endpoint FROM orchestrators WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("get orchestrator: {e}")))?;

    row.map(|(id, endpoint)| OrchestratorRow { id, endpoint })
        .ok_or(ManagerError::Internal("orchestrator not found".into()))
}

/// AUTO_ADOPT_LOCAL: default "1" (fail-open). "0" desliga a auto-adoção
/// de orchestrator-local no boot (sessão GPU — ADR-0010 D2).
pub fn auto_adopt_enabled(raw: Option<&str>) -> bool {
    raw.map(|v| v != "0").unwrap_or(true)
}

/// Auto-adoção: insere orchestrator-local se ausente, atualiza status.
/// Guarda: auto-adoção NUNCA ressuscita revoked (ADR-0011 D5.7).
pub async fn adopt_orchestrator(pool: &PgPool) -> Result<(), ManagerError> {
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'orchestrator-local', 'http://orchestrator-local:8082', 'local', 'online') \
         ON CONFLICT (endpoint) DO UPDATE SET status = 'online' \
         WHERE orchestrators.status <> 'revoked'",
    )
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("adopt orchestrator: {e}")))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Rotas internas de leitura (F6.1a)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct OrchestratorItem {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub endpoint: String,
    pub status: String,
    pub last_heartbeat: Option<String>,
    pub vram_total_gb: Option<i32>,
    pub measured: bool,
    pub cpu: Option<f64>,
    pub ram: Option<i64>,
    pub ram_total: Option<i64>,
    pub vram_used: Option<i64>,
    pub vram_total: Option<i64>,
    pub gpus: Vec<String>,
    pub jobs_active: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrchestratorsResponse {
    pub items: Vec<OrchestratorItem>,
}

/// Lista todos os orquestradores com telemetria por nó.
pub async fn list_orchestrators(
    pool: &PgPool,
    cache: &TelemetryCache,
) -> Result<OrchestratorsResponse, ManagerError> {
    let rows: Vec<(
        Uuid,
        String,
        String,
        String,
        String,
        Option<DateTime<Utc>>,
        Option<i32>,
    )> = sqlx::query_as(
        "SELECT id, name, kind, endpoint, status, last_heartbeat, vram_total_gb \
             FROM orchestrators ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("list orchestrators: {e}")))?;

    let cache = cache.read().await;
    let now = Utc::now();

    let items = rows
        .into_iter()
        .map(|r| {
            let orch_id = r.0;
            let telemetry = cache.get(&orch_id);
            let (measured, cpu, ram, ram_total, vram_used, vram_total, gpus, jobs_active) =
                match telemetry {
                    Some(state) => {
                        let m = state
                            .last_heartbeat
                            .map(|last| (now - last).num_seconds() <= 10)
                            .unwrap_or(false);
                        (
                            m,
                            state.cpu,
                            state.ram,
                            state.ram_total,
                            state.vram_used,
                            state.vram_total,
                            state.gpus.clone(),
                            state.jobs_active,
                        )
                    }
                    None => (false, None, None, None, None, None, vec![], 0),
                };

            OrchestratorItem {
                id: orch_id.to_string(),
                name: r.1,
                kind: r.2,
                endpoint: r.3,
                status: r.4,
                last_heartbeat: r.5.map(|t| t.to_rfc3339()),
                vram_total_gb: r.6,
                measured,
                cpu,
                ram,
                ram_total,
                vram_used,
                vram_total,
                gpus,
                jobs_active,
            }
        })
        .collect();

    Ok(OrchestratorsResponse { items })
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelItem {
    pub id: String,
    pub name: String,
    pub engine: String,
    pub model: Option<String>,
    pub source: String,
    pub hash: String,
    pub bytes: i64,
    pub path: String,
    pub job_id: Option<String>,
    pub created_at: String,
    /// Tipo do modelo para engine='diffusion': 'lora' ou 'checkpoint' (D4 — ADR-0023).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Arquitetura do modelo para engine='diffusion': 'flux-2-klein-4b', 'sdxl', 'sd15' (D4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelsResponse {
    pub items: Vec<ModelItem>,
}

/// Lista modelos da tabela `models` (catálogo canônico; ADR-0012 D2).
pub async fn list_models(pool: &PgPool) -> Result<ModelsResponse, ManagerError> {
    let rows: Vec<(
        Uuid,
        String,
        String,
        Option<String>,
        String,
        String,
        i64,
        String,
        Option<Uuid>,
        DateTime<Utc>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, name, engine, model, source, hash, bytes, s3_key, job_id, created_at, kind, arch \
         FROM models ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("list models: {e}")))?;

    let items = rows
        .into_iter()
        .map(|r| ModelItem {
            id: r.0.to_string(),
            name: r.1,
            engine: r.2,
            model: r.3,
            source: r.4,
            hash: r.5,
            bytes: r.6,
            path: r.7, // s3_key → path (wire compat)
            job_id: r.8.map(|u| u.to_string()),
            created_at: r.9.to_rfc3339(),
            kind: r.10,
            arch: r.11,
        })
        .collect();

    Ok(ModelsResponse { items })
}

// ---------------------------------------------------------------------------
// POST /internal/models — cria row na tabela models (ADR-0012 D1/I.2b)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct CreateModelRequest {
    pub id: Uuid,
    pub engine: String,
    pub name: String,
    pub model: Option<String>,
    pub s3_key: String,
    pub source: String,
    pub url: Option<String>,
    pub hash: String,
    pub bytes: i64,
    pub job_id: Option<Uuid>,
    /// Tipo do modelo para engine='diffusion': 'lora' ou 'checkpoint'.
    #[serde(default)]
    pub kind: Option<String>,
    /// Arquitetura do modelo para engine='diffusion': 'flux-2-klein-4b', 'sdxl', 'sd15'.
    #[serde(default)]
    pub arch: Option<String>,
}

/// Validação pura do CreateModelRequest (padrão da casa — função testável).
fn validate_create_model(req: &CreateModelRequest) -> Result<(), ManagerError> {
    if req.engine != "yolo"
        && req.engine != "world"
        && req.engine != "diffusion"
        && req.engine != "clip"
    {
        return Err(ManagerError::InvalidRequest(format!(
            "engine must be 'yolo', 'world', 'diffusion', or 'clip', got '{}'",
            req.engine
        )));
    }
    if !matches!(req.source.as_str(), "train" | "upload" | "download") {
        return Err(ManagerError::InvalidRequest(format!(
            "source must be 'train', 'upload', or 'download', got '{}'",
            req.source
        )));
    }
    if !is_valid_md5(&req.hash) {
        return Err(ManagerError::InvalidRequest(format!(
            "hash must be a 32-char lowercase hex md5, got '{}'",
            req.hash
        )));
    }
    if req.bytes < 0 {
        return Err(ManagerError::InvalidRequest(format!(
            "bytes must be >= 0, got {}",
            req.bytes
        )));
    }
    if req.name.is_empty() || req.name.len() > 255 {
        return Err(ManagerError::InvalidRequest(format!(
            "name must be between 1 and 255 chars, got {}",
            req.name.len()
        )));
    }
    // Validação de kind/arch: só aceitos para engine='diffusion'.
    if req.kind.is_some() || req.arch.is_some() {
        if req.engine != "diffusion" {
            return Err(ManagerError::InvalidRequest(format!(
                "kind/arch are only allowed for engine='diffusion', got engine='{}'",
                req.engine
            )));
        }
    }
    if let Some(ref kind) = req.kind {
        if kind != "lora" && kind != "checkpoint" {
            return Err(ManagerError::InvalidRequest(format!(
                "kind must be 'lora' or 'checkpoint', got '{}'",
                kind
            )));
        }
    }
    if let Some(ref arch) = req.arch {
        if arch != "flux-2-klein-4b" && arch != "sdxl" && arch != "sd15" {
            return Err(ManagerError::InvalidRequest(format!(
                "arch must be 'flux-2-klein-4b', 'sdxl', or 'sd15', got '{}'",
                arch
            )));
        }
    }
    // kind=checkpoint exige arch (D4 — flux custom fora da v1 no upload).
    if req.kind.as_deref() == Some("checkpoint") && req.arch.is_none() {
        return Err(ManagerError::InvalidRequest(
            "checkpoint requires arch ('flux-2-klein-4b', 'sdxl', or 'sd15')".into(),
        ));
    }
    Ok(())
}

/// Cria uma row na tabela models (POST /internal/models — ADR-0012 D1).
/// Retorna a row criada (shape = ModelItem).
pub async fn create_model(
    pool: &PgPool,
    req: CreateModelRequest,
) -> Result<ModelItem, ManagerError> {
    validate_create_model(&req)?;

    let row: Result<
        Option<(
            Uuid,
            String,
            String,
            Option<String>,
            String,
            String,
            i64,
            String,
            Option<Uuid>,
            DateTime<Utc>,
            Option<String>,
            Option<String>,
        )>,
        sqlx::Error,
    > = sqlx::query_as(
        "INSERT INTO models (id, engine, name, model, s3_key, source, url, hash, bytes, job_id, kind, arch) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) \
         RETURNING id, name, engine, model, source, hash, bytes, s3_key, job_id, created_at, kind, arch",
    )
    .bind(req.id)
    .bind(&req.engine)
    .bind(&req.name)
    .bind(&req.model)
    .bind(&req.s3_key)
    .bind(&req.source)
    .bind(&req.url)
    .bind(&req.hash)
    .bind(req.bytes)
    .bind(req.job_id)
    .bind(&req.kind)
    .bind(&req.arch)
    .fetch_optional(pool)
    .await;

    match row {
        Ok(Some(r)) => Ok(ModelItem {
            id: r.0.to_string(),
            name: r.1,
            engine: r.2,
            model: r.3,
            source: r.4,
            hash: r.5,
            bytes: r.6,
            path: r.7,
            job_id: r.8.map(|u| u.to_string()),
            created_at: r.9.to_rfc3339(),
            kind: r.10,
            arch: r.11,
        }),
        Ok(None) => Err(ManagerError::Internal(
            "insert model: no row returned".into(),
        )),
        Err(e) => {
            // A3: checagem robusta de violação de unicidade (sqlx code 23505).
            if e.as_database_error()
                .map(|db| db.is_unique_violation())
                .unwrap_or(false)
            {
                Err(ManagerError::Internal("model_exists".to_string()))
            } else {
                Err(ManagerError::Internal(format!("insert model: {e}")))
            }
        }
    }
}

/// Remove uma row da tabela models (DELETE /internal/models/:id).
/// Retorna a row deletada (shape = ModelItem) ou NotFound.
pub async fn delete_model(pool: &PgPool, id: Uuid) -> Result<ModelItem, ManagerError> {
    let row: Option<(
        Uuid,
        String,
        String,
        Option<String>,
        String,
        String,
        i64,
        String,
        Option<Uuid>,
        DateTime<Utc>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "DELETE FROM models WHERE id = $1 \
         RETURNING id, name, engine, model, source, hash, bytes, s3_key, job_id, created_at, kind, arch",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("delete model: {e}")))?;

    match row {
        Some(r) => Ok(ModelItem {
            id: r.0.to_string(),
            name: r.1,
            engine: r.2,
            model: r.3,
            source: r.4,
            hash: r.5,
            bytes: r.6,
            path: r.7,
            job_id: r.8.map(|u| u.to_string()),
            created_at: r.9.to_rfc3339(),
            kind: r.10,
            arch: r.11,
        }),
        None => Err(ManagerError::NotFound),
    }
}

/// Sanitiza uma string para slug seguro (apenas a-z, 0-9 e hífen).
pub fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_dash = true; // evita dash inicial
    for c in s.chars() {
        let normalized = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => {
                'a'
            }
            'è' | 'é' | 'ê' | 'ë' | 'È' | 'É' | 'Ê' | 'Ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' => 'i',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' => 'o',
            'ù' | 'ú' | 'û' | 'ü' | 'Ù' | 'Ú' | 'Û' | 'Ü' => 'u',
            'ç' | 'Ç' => 'c',
            'ñ' | 'Ñ' => 'n',
            other => other.to_ascii_lowercase(),
        };
        if normalized.is_ascii_alphanumeric() {
            out.push(normalized);
            last_dash = false;
        } else if (normalized == '-' || normalized == '_' || normalized == ' ') && !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    if out.ends_with('-') {
        out.pop();
    }
    out
}

/// Deriva o nome do modelo registrado na tabela models (ADR-0022 D0).
/// - Se `output_name` estiver presente nos params, usa-o garantindo a extensão apropriada.
/// - Caso contrário, deriva semanticamente:
///   - Difusão: `{dataset_slug}-{model_slug}-{trigger_or_short_id}.safetensors`
///   - YOLO: `{dataset_slug}-{model_slug}-best.pt`
/// - Fallback se sem dataset ou outro artefato: `{art_filename}`.
pub fn compute_model_name(
    art_path: &str,
    engine: &str,
    model: &str,
    job_id: Uuid,
    dataset_slug: Option<&str>,
    params: &serde_json::Value,
) -> String {
    let default_filename = art_path.rsplit('/').next().unwrap_or(art_path);
    let ext = default_filename.rsplit('.').next().unwrap_or("");

    // 1. Se o usuário forneceu output_name explicitamente (D1)
    if let Some(out_name) = params
        .get("output_name")
        .or_else(|| params.get("outputName"))
        .and_then(|v| v.as_str())
    {
        let clean = out_name.trim();
        if !clean.is_empty() {
            let (base_name, user_ext) = if let Some((base, user_ext)) = clean.rsplit_once('.') {
                if user_ext.eq_ignore_ascii_case("safetensors")
                    || user_ext.eq_ignore_ascii_case("pt")
                {
                    (base, Some(user_ext))
                } else {
                    (clean, None)
                }
            } else {
                (clean, None)
            };
            let slugged_base = slugify(base_name);
            if !slugged_base.is_empty() {
                let final_ext = user_ext.unwrap_or(ext);
                if !final_ext.is_empty() {
                    return format!("{slugged_base}.{final_ext}");
                }
                return slugged_base;
            }
        }
    }

    // 2. Derivação semântica inteligente (D0)
    let job_hex = job_id.to_string();
    let short_id = &job_hex[..8.min(job_hex.len())];

    let ds_slug = dataset_slug.map(slugify).filter(|s| !s.is_empty());

    let clean_model = match model.to_ascii_lowercase().as_str() {
        "flux" | "flux-2-klein-4b" => "flux2".to_string(),
        "sdxl" => "sdxl".to_string(),
        "sd15" => "sd15".to_string(),
        other => slugify(other),
    };

    if engine == "diffusion" {
        let trigger = params
            .get("trigger_word")
            .or_else(|| params.get("triggerWord"))
            .and_then(|v| v.as_str())
            .map(slugify)
            .filter(|s| !s.is_empty());

        let suffix = trigger.as_deref().unwrap_or(short_id);
        let ext_str = if ext.is_empty() { "safetensors" } else { ext };

        if let Some(ds) = ds_slug {
            format!("{ds}-{clean_model}-{suffix}.{ext_str}")
        } else {
            format!("{clean_model}-{suffix}.{ext_str}")
        }
    } else if engine == "yolo" {
        let ext_str = if ext.is_empty() { "pt" } else { ext };
        if let Some(ds) = ds_slug {
            format!("{ds}-{clean_model}-best.{ext_str}")
        } else {
            format!("{clean_model}-{short_id}-best.{ext_str}")
        }
    } else {
        default_filename.to_string()
    }
}

// ---------------------------------------------------------------------------
// PATCH /internal/models/:id — renomeia modelo na tabela models (ADR-0022 D2)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateModelRequest {
    pub name: String,
}

pub fn validate_update_model(req: &UpdateModelRequest) -> Result<(), ManagerError> {
    let clean = req.name.trim();
    if clean.is_empty() || clean.len() > 255 {
        return Err(ManagerError::InvalidRequest(format!(
            "name must be between 1 and 255 chars, got {}",
            clean.len()
        )));
    }
    Ok(())
}

/// Atualiza o nome de um modelo na tabela models (PATCH /internal/models/:id — ADR-0022 D2).
/// Retorna a row atualizada (shape = ModelItem) ou NotFound.
pub async fn update_model(
    pool: &PgPool,
    id: Uuid,
    req: UpdateModelRequest,
) -> Result<ModelItem, ManagerError> {
    validate_update_model(&req)?;
    let clean_name = req.name.trim();

    let row: Option<(
        Uuid,
        String,
        String,
        Option<String>,
        String,
        String,
        i64,
        String,
        Option<Uuid>,
        DateTime<Utc>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "UPDATE models SET name = $1 WHERE id = $2 \
         RETURNING id, name, engine, model, source, hash, bytes, s3_key, job_id, created_at, kind, arch",
    )
    .bind(clean_name)
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("update model: {e}")))?;

    match row {
        Some(r) => Ok(ModelItem {
            id: r.0.to_string(),
            name: r.1,
            engine: r.2,
            model: r.3,
            source: r.4,
            hash: r.5,
            bytes: r.6,
            path: r.7,
            job_id: r.8.map(|u| u.to_string()),
            created_at: r.9.to_rfc3339(),
            kind: r.10,
            arch: r.11,
        }),
        None => Err(ManagerError::NotFound),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StorageUsageResponse {
    pub artifacts_bytes: i64,
    pub models_bytes: i64,
}

/// Retorna soma de bytes de job_artifacts (excluindo kind='model') e models.
pub async fn get_storage_usage(pool: &PgPool) -> Result<StorageUsageResponse, ManagerError> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(bytes), 0)::bigint AS artifacts_bytes FROM job_artifacts WHERE kind <> 'model'",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("storage usage artifacts: {e}")))?;

    let models_row: (i64,) =
        sqlx::query_as("SELECT COALESCE(SUM(bytes), 0)::bigint AS models_bytes FROM models")
            .fetch_one(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("storage usage models: {e}")))?;

    Ok(StorageUsageResponse {
        artifacts_bytes: row.0,
        models_bytes: models_row.0,
    })
}

/// Recovery: marca jobs órfãos como queued com queue_reason='recovered'.
pub async fn recover_jobs(pool: &PgPool) -> Result<u64, ManagerError> {
    let result = sqlx::query(
        "UPDATE jobs SET status = 'queued', queue_reason = 'recovered', orchestrator_id = NULL \
         WHERE status IN ('dispatched', 'preparing', 'running', 'cancelling')",
    )
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("recover jobs: {e}")))?;

    Ok(result.rows_affected())
}

// ---------------------------------------------------------------------------
// Generations (D5 — ADR-0023) — rotas internas
// ---------------------------------------------------------------------------

/// Lista generations com paginação e filtros (GET /internal/generations).
pub async fn list_generations(
    pool: &PgPool,
    limit: i64,
    offset: i64,
    deleted: bool,
    base_model: Option<&str>,
) -> Result<ListGenerationsResponse, ManagerError> {
    let mut where_clauses = Vec::new();
    let mut bind_idx: u32 = 1;

    if deleted {
        where_clauses.push("g.deleted_at IS NOT NULL".to_string());
    } else {
        where_clauses.push("g.deleted_at IS NULL".to_string());
    }

    if base_model.is_some() {
        where_clauses.push(format!("g.params->>'base_model' = ${bind_idx}"));
        bind_idx += 1;
    }

    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };

    // Count query.
    let count_sql = format!("SELECT COUNT(*) FROM generations g {where_sql}");
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    if let Some(bm) = base_model {
        count_q = count_q.bind(bm);
    }
    let total: i64 = count_q
        .fetch_one(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("count generations: {e}")))?;

    // Main query.
    let main_sql = format!(
        "SELECT g.id, g.job_id, g.s3_key, g.thumb_s3_key, g.filename, g.seed, \
         g.prompt, g.negative_prompt, g.width, g.height, g.params, g.created_at, g.deleted_at \
         FROM generations g {where_sql} \
         ORDER BY g.created_at DESC LIMIT ${bind_idx} OFFSET {}",
        bind_idx + 1
    );
    let mut main_q = sqlx::query(&main_sql);
    if let Some(bm) = base_model {
        main_q = main_q.bind(bm);
    }
    main_q = main_q.bind(limit).bind(offset);

    let rows = main_q
        .fetch_all(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("list generations: {e}")))?;

    let items = rows
        .into_iter()
        .map(|r| {
            let id: Uuid = r.get("id");
            let job_id: Uuid = r.get("job_id");
            let created_at: DateTime<Utc> = r.get("created_at");
            let deleted_at: Option<DateTime<Utc>> = r.get("deleted_at");
            GenerationRow {
                id: id.to_string(),
                job_id: job_id.to_string(),
                s3_key: r.get("s3_key"),
                thumb_s3_key: r.get("thumb_s3_key"),
                filename: r.get("filename"),
                seed: r.get("seed"),
                prompt: r.get("prompt"),
                negative_prompt: r.get("negative_prompt"),
                width: r.get("width"),
                height: r.get("height"),
                params: r.get("params"),
                created_at: created_at.to_rfc3339(),
                deleted_at: deleted_at.map(|t| t.to_rfc3339()),
            }
        })
        .collect();

    Ok(ListGenerationsResponse { items, total })
}

/// Busca uma generation por ID — exclui soft-deletadas (deleted_at IS NULL).
/// Usado pelo proxy de imagem da api-principal: GET /internal/generations/:id.
pub async fn get_generation(
    pool: &PgPool,
    id: Uuid,
) -> Result<Option<GenerationRow>, ManagerError> {
    let row = sqlx::query(
        "SELECT id, job_id, s3_key, thumb_s3_key, filename, seed, prompt, negative_prompt, \
         width, height, params, created_at, deleted_at \
         FROM generations WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("get generation: {e}")))?;

    Ok(row.map(|r| {
        let id: Uuid = r.get("id");
        let job_id: Uuid = r.get("job_id");
        let created_at: DateTime<Utc> = r.get("created_at");
        let deleted_at: Option<DateTime<Utc>> = r.get("deleted_at");
        GenerationRow {
            id: id.to_string(),
            job_id: job_id.to_string(),
            s3_key: r.get("s3_key"),
            thumb_s3_key: r.get("thumb_s3_key"),
            filename: r.get("filename"),
            seed: r.get("seed"),
            prompt: r.get("prompt"),
            negative_prompt: r.get("negative_prompt"),
            width: r.get("width"),
            height: r.get("height"),
            params: r.get("params"),
            created_at: created_at.to_rfc3339(),
            deleted_at: deleted_at.map(|t| t.to_rfc3339()),
        }
    }))
}

/// Soft delete de generations por IDs (POST /internal/generations/delete).
/// Idempotente: IDs inexistentes são ignorados.
pub async fn soft_delete_generations(pool: &PgPool, ids: &[Uuid]) -> Result<(), ManagerError> {
    if ids.is_empty() || ids.len() > 100 {
        return Err(ManagerError::InvalidRequest(
            "ids must have 1..100 items".into(),
        ));
    }
    sqlx::query(
        "UPDATE generations SET deleted_at = now() WHERE id = ANY($1) AND deleted_at IS NULL",
    )
    .bind(ids)
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("soft delete generations: {e}")))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Watchdog offline (ADR-0011 D4)
// ---------------------------------------------------------------------------

/// Tick do watchdog: transições online→degraded→offline com re-queue dos jobs.
/// Chamada pelo worker loop (~2s).
pub async fn watchdog_tick(pool: &PgPool) -> Result<(), ManagerError> {
    let degraded_s: i64 = std::env::var("ORCH_WATCHDOG_DEGRADED_S")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15);
    let offline_s: i64 = std::env::var("ORCH_WATCHDOG_OFFLINE_S")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);

    // online → degraded (last_heartbeat mais velho que degraded_s OU NULL).
    let _ = sqlx::query(
        "UPDATE orchestrators SET status = 'degraded' \
         WHERE status = 'online' \
           AND (last_heartbeat IS NULL OR last_heartbeat < now() - make_interval(secs => $1::float))",
    )
    .bind(degraded_s as f64)
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("watchdog degraded: {e}")))?;

    // degraded → offline + re-queue dos jobs do nó morto (CTE espelho de recover_jobs).
    let result = sqlx::query(
        "WITH morto AS ( \
             UPDATE orchestrators SET status = 'offline' \
             WHERE status = 'degraded' \
               AND (last_heartbeat IS NULL OR last_heartbeat < now() - make_interval(secs => $1::float)) \
             RETURNING id \
         ) \
         UPDATE jobs SET status = 'queued', queue_reason = 'recovered', orchestrator_id = NULL \
         WHERE orchestrator_id IN (SELECT id FROM morto) \
           AND status IN ('dispatched','preparing','running','cancelling')",
    )
    .bind(offline_s as f64)
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("watchdog offline: {e}")))?;

    if result.rows_affected() > 0 {
        tracing::info!(
            "watchdog: {} jobs re-queued de nós offline",
            result.rows_affected()
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Adopt interno (ADR-0011 D5)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct AdoptRequest {
    pub name: String,
    pub endpoint: String,
    pub kind: String,
    pub pairing_code: String,
}

/// Adopt interno: valida, verifica pairing no orquestrador, upsert.
///
/// Retorna o item completo (`OrchestratorItem`) no mesmo shape de
/// `list_orchestrators` — enriquecido com telemetria do cache (que estará
/// vazia na criação: `measured:false` + nulls). BFF já deserializa esse
/// shape — nada muda nele.
pub async fn adopt_internal(
    pool: &PgPool,
    orch_client: &dyn OrchestratorClient,
    req: &AdoptRequest,
) -> Result<OrchestratorItem, ManagerError> {
    // Validação de domínio.
    if req.kind != "local" && req.kind != "remoto" {
        return Err(ManagerError::InvalidRequest(
            "kind must be 'local' or 'remoto'".into(),
        ));
    }
    if !req.endpoint.starts_with("http://") && !req.endpoint.starts_with("https://") {
        return Err(ManagerError::InvalidRequest(
            "endpoint must start with http:// or https://".into(),
        ));
    }
    if req.name.is_empty() || req.name.len() > 128 {
        return Err(ManagerError::InvalidRequest(
            "name must be 1-128 characters".into(),
        ));
    }
    if req.pairing_code.is_empty() || req.pairing_code.len() > 128 {
        return Err(ManagerError::InvalidRequest(
            "pairing_code must be 1-128 characters".into(),
        ));
    }

    // Verifica pairing code no orquestrador.
    let verify_url = format!("{}/internal/pairing/verify", req.endpoint);
    let verify_body = serde_json::json!({"code": &req.pairing_code});

    match orch_client.post_json(&verify_url, &verify_body).await {
        Ok(json) => {
            let valid = json.get("valid").and_then(|v| v.as_bool()).unwrap_or(false);
            if !valid {
                return Err(ManagerError::PairingInvalid);
            }
        }
        Err(_) => {
            return Err(ManagerError::PairingInvalid);
        }
    }

    // Upsert: cria ou revive (revoked incluído — intenção explícita do operador).
    let orch_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status, token_hash, fingerprint) \
         VALUES ($1, $2, $3, $4, 'online', NULL, NULL) \
         ON CONFLICT (endpoint) DO UPDATE SET name = EXCLUDED.name, kind = EXCLUDED.kind, status = 'online'",
    )
    .bind(orch_id)
    .bind(&req.name)
    .bind(&req.endpoint)
    .bind(&req.kind)
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("adopt orchestrator: {e}")))?;

    // Resolve o id real (upsert pode ter usado linha existente).
    let real_id: (Uuid,) = sqlx::query_as("SELECT id FROM orchestrators WHERE endpoint = $1")
        .bind(&req.endpoint)
        .fetch_one(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("resolve adopted id: {e}")))?;

    // Retorna item completo (shape idêntico a list_orchestrators).
    Ok(OrchestratorItem {
        id: real_id.0.to_string(),
        name: req.name.clone(),
        kind: req.kind.clone(),
        endpoint: req.endpoint.clone(),
        status: "online".to_string(),
        last_heartbeat: None,
        vram_total_gb: None,
        measured: false,
        cpu: None,
        ram: None,
        ram_total: None,
        vram_used: None,
        vram_total: None,
        gpus: vec![],
        jobs_active: 0,
    })
}

// ---------------------------------------------------------------------------
// Revoke interno (ADR-0011 D5)
// ---------------------------------------------------------------------------

/// Revoke: status → 'revoked' (tombstone, não DELETE).
pub async fn revoke_orchestrator(pool: &PgPool, id: Uuid) -> Result<(), ManagerError> {
    let result = sqlx::query("UPDATE orchestrators SET status = 'revoked' WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("revoke orchestrator: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(ManagerError::NotFound);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Pega o próximo job queued e despacha ao orquestrador.
///
/// Roteamento por capacidade (ADR-0011 D3): seleciona nó online sem job
/// ativo, com capacidade VRAM suficiente (ou NULL permissivo), na ordem
/// declarada primeiro, maior GPU primeiro, tie-break por nome.
///
/// Retorna `true` se um job foi despachado, `false` se não havia job na fila.
pub async fn dispatch_next(
    pool: &PgPool,
    orch_client: &dyn OrchestratorClient,
    exec_mode: &str,
    orch_workdir: &str,
    image: &str,
    vram_table: &VramTable,
) -> Result<bool, ManagerError> {
    // 1. Seleciona próximo job queued (FIFO).
    let row: Option<(
        Uuid,
        String,
        String,
        String,
        Option<serde_json::Value>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, engine, model, mode, params, config_yaml \
         FROM jobs WHERE status = 'queued' ORDER BY created_at LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("select next job: {e}")))?;

    let (job_id, engine, model, mode, params, config_yaml) = match row {
        Some(r) => r,
        None => return Ok(false),
    };

    // 2. Resolve requisito VRAM da vram-table.
    let required_gb: Option<i32> = vram_table.resolve_required_gb(&engine, &model, &mode);

    // 3. Seleciona orquestrador (ADR-0015 D3).
    // Se houver orchestrator_hint em params, tenta despachar para ele (D3.2).
    let hint: Option<Uuid> = params
        .as_ref()
        .and_then(|p| p.get("orchestrator_hint"))
        .and_then(|h| h.as_str())
        .and_then(|s| s.parse().ok());

    let mut selected_orch: Option<(Uuid, String)> = None;
    let mut fallback_used = false;

    if let Some(hint_id) = hint {
        let hinted: Option<(Uuid, String)> = sqlx::query_as(
            "SELECT o.id, o.endpoint FROM orchestrators o \
             WHERE o.id = $1 AND o.status = 'online' \
               AND NOT EXISTS (SELECT 1 FROM jobs j \
                               WHERE j.orchestrator_id = o.id \
                                 AND j.status IN ('dispatched','preparing','running','cancelling')) \
               AND ($2::int IS NULL OR o.vram_total_gb IS NULL OR o.vram_total_gb >= $2)",
        )
        .bind(hint_id)
        .bind(required_gb)
        .fetch_optional(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("find hinted orchestrator: {e}")))?;

        if let Some(o) = hinted {
            selected_orch = Some(o);
        } else {
            fallback_used = true;
        }
    }

    if selected_orch.is_none() {
        let eligible: Option<(Uuid, String)> = sqlx::query_as(
            "SELECT o.id, o.endpoint FROM orchestrators o \
             WHERE o.status = 'online' \
               AND NOT EXISTS (SELECT 1 FROM jobs j \
                               WHERE j.orchestrator_id = o.id \
                                 AND j.status IN ('dispatched','preparing','running','cancelling')) \
               AND ($1::int IS NULL OR o.vram_total_gb IS NULL OR o.vram_total_gb >= $1) \
             ORDER BY (o.vram_total_gb IS NULL) ASC, \
                      o.vram_total_gb DESC NULLS LAST, \
                      o.name ASC \
             LIMIT 1",
        )
        .bind(required_gb)
        .fetch_optional(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("find orchestrator: {e}")))?;

        selected_orch = eligible;
    }

    let (orch_id, orch_endpoint) = match selected_orch {
        Some(o) => o,
        None => {
            // Sem nó elegível: waiting_vram (se requisito) ou waiting_slot.
            let reason = if required_gb.is_some() {
                "waiting_vram"
            } else {
                "waiting_slot"
            };
            sqlx::query("UPDATE jobs SET queue_reason = $2 WHERE id = $1 AND status = 'queued'")
                .bind(job_id)
                .bind(reason)
                .execute(pool)
                .await
                .map_err(|e| ManagerError::Internal(format!("set queue reason: {e}")))?;
            return Ok(false);
        }
    };

    // 4. Marca dispatched e atualiza flag de fallback em params (ADR-0015 D3.3, D3.5).
    if fallback_used {
        sqlx::query(
            "UPDATE jobs SET status = 'dispatched', queue_reason = NULL, orchestrator_id = $2, \
             params = jsonb_set(params, '{orchestrator_fallback}', 'true'::jsonb) \
             WHERE id = $1 AND status = 'queued'",
        )
        .bind(job_id)
        .bind(orch_id)
        .execute(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("set dispatched (fallback): {e}")))?;
    } else {
        sqlx::query(
            "UPDATE jobs SET status = 'dispatched', queue_reason = NULL, orchestrator_id = $2, \
             params = params - 'orchestrator_fallback' \
             WHERE id = $1 AND status = 'queued'",
        )
        .bind(job_id)
        .bind(orch_id)
        .execute(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("set dispatched: {e}")))?;
    }

    // 5. Monta payload do dispatch (idêntico ao anterior).
    let package_ref = params
        .as_ref()
        .and_then(|p| p.get("package_ref"))
        .filter(|p| !p.is_null() && p.get("key").is_some())
        .cloned();

    let dataset_version_id = params
        .as_ref()
        .and_then(|p| p.get("package_ref"))
        .and_then(|pr| pr.get("version_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Extrai weights_ref do params se presente (ADR-0012 D5/I.2b).
    let weights_ref = params.as_ref().and_then(|p| p.get("weights_ref")).cloned();

    // Extrai loras resolvidos do params (D3 — ADR-0023).
    let resolved_loras = params
        .as_ref()
        .and_then(|p| p.get("loras"))
        .cloned()
        .filter(|v| v.is_array() && !v.as_array().map_or(true, |a| a.is_empty()));

    // Extrai custom_checkpoint resolvido do params (D4 — ADR-0023).
    let custom_checkpoint = params
        .as_ref()
        .and_then(|p| p.get("custom_checkpoint"))
        .cloned();

    // Resolve imagem do container: se engine for diffusion, usa DIFFUSION_TRAINER_IMAGE
    // ou substitui trainer-yolo por trainer-difusao mantendo tag (:local ou :gpu).
    let job_image = match engine.as_str() {
        "diffusion" => {
            let env_diff = std::env::var("DIFFUSION_TRAINER_IMAGE").unwrap_or_default();
            if !env_diff.is_empty() && env_diff != "hephaestus/trainer-difusao:local" {
                env_diff
            } else if image.ends_with(":gpu") || image.contains(":gpu") {
                "hephaestus/trainer-difusao:gpu".to_string()
            } else if !env_diff.is_empty() {
                env_diff
            } else if image.contains("trainer-yolo") {
                image.replace("trainer-yolo", "trainer-difusao")
            } else {
                image.to_string()
            }
        }
        _ => image.to_string(),
    };

    let mut dispatch_body = serde_json::json!({
        "job_id": job_id.to_string(),
        "engine": engine,
        "image": job_image,
        "exec_mode": exec_mode,
        "config_yaml": config_yaml,
        "dataset_version_id": dataset_version_id,
        "workdir": orch_workdir,
        "mode": mode,
    });
    if let Some(pr) = package_ref {
        dispatch_body["package_ref"] = pr;
    }

    // Adiciona weights_ref ao dispatch quando presente (snake_case — casa com WeightsRef do orquestrador).
    if let Some(wr) = weights_ref {
        dispatch_body["weights_ref"] = wr;
    }

    // Adiciona loras ao dispatch quando presente (D3 — ADR-0023).
    // snake_case: `loras: [{s3_key, md5, scale}]` — casa com LoraRefStage do orquestrador.
    if let Some(loras) = resolved_loras {
        dispatch_body["loras"] = loras;
    }

    // Adiciona custom_checkpoint ao dispatch quando presente (D4 — ADR-0023).
    // snake_case: `custom_checkpoint: {s3_key, md5}` — casa com WeightRef do orquestrador.
    if let Some(cc) = custom_checkpoint {
        dispatch_body["custom_checkpoint"] = cc;
    }

    let url = format!("{}/internal/dispatch", orch_endpoint);

    if let Err(e) = orch_client.post(&url, &dispatch_body).await {
        tracing::warn!("dispatch failed for job {job_id}: {e}");
        // Volta para queued.
        sqlx::query(
            "UPDATE jobs SET status = 'queued', queue_reason = 'waiting_slot', orchestrator_id = NULL WHERE id = $1",
        )
        .bind(job_id)
        .execute(pool)
        .await
        .map_err(|e2| ManagerError::Internal(format!("revert job: {e2}")))?;
    }

    Ok(true)
}

// ---------------------------------------------------------------------------
// Tests unitários
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_valid_md5_correct() {
        assert!(is_valid_md5("d41d8cd98f00b204e9800998ecf8427e"));
    }

    #[test]
    fn is_valid_md5_uppercase() {
        assert!(!is_valid_md5("D41D8CD98F00B204E9800998ECF8427E"));
    }

    #[test]
    fn is_valid_md5_wrong_length() {
        assert!(!is_valid_md5("d41d8cd98f00b204e9800998ecf8427"));
    }

    #[test]
    fn is_valid_md5_non_hex() {
        assert!(!is_valid_md5("d41d8cd98f00b204e9800998ecf8427g"));
    }

    #[test]
    fn http_orchestrator_client_stores_token() {
        let client = HttpOrchestratorClient::new(Some("tok_test".into()));
        assert!(client.token.as_deref() == Some("tok_test"));
    }

    #[test]
    fn http_orchestrator_client_none_token() {
        let client = HttpOrchestratorClient::new(None);
        assert!(client.token.is_none());
    }

    #[test]
    fn auto_adopt_enabled_none_is_true() {
        assert!(auto_adopt_enabled(None));
    }

    #[test]
    fn auto_adopt_enabled_one_is_true() {
        assert!(auto_adopt_enabled(Some("1")));
    }

    #[test]
    fn auto_adopt_enabled_zero_is_false() {
        assert!(!auto_adopt_enabled(Some("0")));
    }

    #[test]
    fn auto_adopt_enabled_empty_is_true() {
        assert!(auto_adopt_enabled(Some("")));
    }

    /// Agregação: 2 nós, 1 stale (>10s) → somente o fresco conta.
    #[test]
    fn agregacao_filtro_stale() {
        use super::TelemetryState;
        use chrono::{Duration, Utc};
        use std::collections::HashMap;

        let mut cache: HashMap<Uuid, TelemetryState> = HashMap::new();
        let now = Utc::now();

        // Nó fresco (heartbeat agora).
        cache.insert(
            Uuid::new_v4(),
            TelemetryState {
                endpoint: "http://fresh:8082".into(),
                measured: true,
                vram_used: Some(3000),
                vram_total: Some(12000),
                cpu: Some(0.4),
                ram: Some(4096),
                ram_total: Some(8192),
                gpus: vec!["RTX 3060".into()],
                jobs_active: 1,
                last_heartbeat: Some(now),
            },
        );

        // Nó stale (heartbeat 60s atrás).
        cache.insert(
            Uuid::new_v4(),
            TelemetryState {
                endpoint: "http://stale:8082".into(),
                measured: true,
                vram_used: Some(8000),
                vram_total: Some(24000),
                cpu: Some(0.9),
                ram: Some(16384),
                ram_total: Some(67108864000),
                gpus: vec!["RTX 4090".into()],
                jobs_active: 5,
                last_heartbeat: Some(now - Duration::seconds(60)),
            },
        );

        // Simula a lógica de filtro do get_telemetry (>1 nós).
        let mut vram_used_sum: Option<i64> = Some(0);
        let mut vram_total_sum: Option<i64> = Some(0);
        let mut gpus: Vec<String> = Vec::new();
        let mut jobs_active_sum: i32 = 0;
        let mut measured = false;

        for state in cache.values() {
            let is_fresh = state
                .last_heartbeat
                .map(|last| (now - last).num_seconds() <= 10)
                .unwrap_or(false);
            if is_fresh {
                measured = true;
                match (vram_used_sum, state.vram_used) {
                    (Some(acc), Some(val)) => vram_used_sum = Some(acc + val),
                    (Some(_), None) => vram_used_sum = None,
                    (None, _) => {}
                }
                match (vram_total_sum, state.vram_total) {
                    (Some(acc), Some(val)) => vram_total_sum = Some(acc + val),
                    (Some(_), None) => vram_total_sum = None,
                    (None, _) => {}
                }
                for gpu in &state.gpus {
                    if !gpus.contains(gpu) {
                        gpus.push(gpu.clone());
                    }
                }
                jobs_active_sum += state.jobs_active;
            }
        }

        assert!(measured);
        // Só o nó fresh conta.
        assert_eq!(vram_used_sum, Some(3000));
        assert_eq!(vram_total_sum, Some(12000));
        assert_eq!(jobs_active_sum, 1);
        assert_eq!(gpus, vec!["RTX 3060"]);
        // Nó stale NÃO entra na soma.
        assert!(!gpus.contains(&"RTX 4090".to_string()));
    }

    /// Agregação: 2 nós ambos stale → fallback (measured:false).
    #[test]
    fn agregacao_todos_stale_fallback() {
        use super::TelemetryState;
        use chrono::{Duration, Utc};
        use std::collections::HashMap;

        let mut cache: HashMap<Uuid, TelemetryState> = HashMap::new();
        let now = Utc::now();

        cache.insert(
            Uuid::new_v4(),
            TelemetryState {
                endpoint: "http://a:8082".into(),
                measured: true,
                vram_used: Some(3000),
                vram_total: Some(12000),
                gpus: vec!["GPU_A".into()],
                jobs_active: 1,
                last_heartbeat: Some(now - Duration::seconds(30)),
                ..Default::default()
            },
        );
        cache.insert(
            Uuid::new_v4(),
            TelemetryState {
                endpoint: "http://b:8082".into(),
                measured: true,
                vram_used: Some(2000),
                vram_total: Some(6000),
                gpus: vec!["GPU_B".into()],
                jobs_active: 2,
                last_heartbeat: Some(now - Duration::seconds(60)),
                ..Default::default()
            },
        );

        let mut measured = false;
        for state in cache.values() {
            let is_fresh = state
                .last_heartbeat
                .map(|last| (now - last).num_seconds() <= 10)
                .unwrap_or(false);
            if is_fresh {
                measured = true;
            }
        }

        assert!(!measured, "ambos stale → measured deve ser false");
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Meu Dataset Incrível!"), "meu-dataset-incrivel");
        assert_eq!(slugify("test__model--v1"), "test-model-v1");
        assert_eq!(
            slugify("   leading and trailing   "),
            "leading-and-trailing"
        );
        assert_eq!(slugify("cbr_pnk-123"), "cbr-pnk-123");
    }

    #[test]
    fn test_compute_model_name_custom_output_name() {
        let job_id = Uuid::new_v4();
        let params = serde_json::json!({
            "output_name": "meu-personagem-v1"
        });
        let name = compute_model_name(
            "adapter.safetensors",
            "diffusion",
            "flux",
            job_id,
            Some("retratos"),
            &params,
        );
        assert_eq!(name, "meu-personagem-v1.safetensors");

        // Já vem com a extensão
        let params2 = serde_json::json!({
            "output_name": "meu-personagem-v1.safetensors"
        });
        let name2 = compute_model_name(
            "adapter.safetensors",
            "diffusion",
            "flux",
            job_id,
            Some("retratos"),
            &params2,
        );
        assert_eq!(name2, "meu-personagem-v1.safetensors");

        // Custom output para yolo com espaços
        let params3 = serde_json::json!({
            "output_name": "detector de pragas v2"
        });
        let name3 = compute_model_name(
            "weights/best.pt",
            "yolo",
            "yolo11m",
            job_id,
            Some("insetos"),
            &params3,
        );
        assert_eq!(name3, "detector-de-pragas-v2.pt");
    }

    #[test]
    fn test_compute_model_name_semantic_defaults() {
        let job_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();

        // Diffusion com trigger word e dataset
        let params = serde_json::json!({
            "trigger_word": "cbrpnk"
        });
        let name = compute_model_name(
            "adapter.safetensors",
            "diffusion",
            "flux",
            job_id,
            Some("cyberpunk-city"),
            &params,
        );
        assert_eq!(name, "cyberpunk-city-flux2-cbrpnk.safetensors");

        // Diffusion sem trigger word (usa prefixo do job id)
        let params_no_trigger = serde_json::json!({});
        let name2 = compute_model_name(
            "adapter.safetensors",
            "diffusion",
            "sdxl",
            job_id,
            Some("cyberpunk-city"),
            &params_no_trigger,
        );
        assert_eq!(name2, "cyberpunk-city-sdxl-550e8400.safetensors");

        // YOLO com dataset
        let name_yolo = compute_model_name(
            "weights/best.pt",
            "yolo",
            "yolo11m",
            job_id,
            Some("veiculos-urbanos"),
            &serde_json::json!({}),
        );
        assert_eq!(name_yolo, "veiculos-urbanos-yolo11m-best.pt");

        // Fallback sem dataset
        let name_no_ds = compute_model_name(
            "adapter.safetensors",
            "diffusion",
            "sd15",
            job_id,
            None,
            &serde_json::json!({ "trigger_word": "estilo" }),
        );
        assert_eq!(name_no_ds, "sd15-estilo.safetensors");
    }

    #[test]
    fn test_validate_update_model() {
        let ok = UpdateModelRequest {
            name: "novo-nome.safetensors".to_string(),
        };
        assert!(validate_update_model(&ok).is_ok());

        let empty = UpdateModelRequest {
            name: "   ".to_string(),
        };
        assert!(validate_update_model(&empty).is_err());

        let too_long = UpdateModelRequest {
            name: "a".repeat(256),
        };
        assert!(validate_update_model(&too_long).is_err());
    }
}
