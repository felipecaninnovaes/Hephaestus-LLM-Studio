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
    /// Job não está em estado terminal (done|failed|cancelled) — não pode ser apagado.
    NotDeletable,
    /// Transição guardada recusada (ex.: prepare-complete fora de `preparing`) → 409.
    Conflict(String),
    InvalidRequest(String),
    PairingInvalid,
    Internal(String),
}

impl std::fmt::Display for ManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "not found"),
            Self::NotAbortable => write!(f, "job not abortable"),
            Self::NotDeletable => write!(f, "job_not_terminal"),
            Self::Conflict(e) => write!(f, "{e}"),
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

/// POST /internal/jobs/:id/prepare-complete (ADR-0025 D1).
/// Wire camelCase com aliases snake_case (tolerante ao BFF).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareCompleteRequest {
    #[serde(alias = "dataset_version_id")]
    pub dataset_version_id: String,
    #[serde(alias = "package_ref")]
    pub package_ref: PreparePackageRef,
}

/// Pacote construído pelo worker de preparação (sem version_id: ele é o
/// dataset_version_id do envelope).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparePackageRef {
    pub key: String,
    #[serde(alias = "md5_zip")]
    pub md5_zip: String,
    pub bytes: i64,
}

/// POST /internal/jobs/:id/prepare-fail (ADR-0025 D1).
/// Wire camelCase como o resto de `/api/*` (campos de palavra única: sem
/// efeito no wire, só conformidade de contrato).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareFailRequest {
    pub code: String,
    pub message: String,
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
    /// AC-006-A D3: último status de fase do job (snapshot last-write-wins).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// AC-006-A D3: última mensagem de status do job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
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
    /// AC-006-A D2/D3: fase/status do job (ex.: "loading_model").
    /// Campo opcional retrocompat: ausente em orquestradores antigos.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// AC-006-A D2/D3: mensagem descritiva da fase.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
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

/// Referência resolvida de text encoder custom para o dispatch
/// (fatia feat/pesos-custom-flux2). Mesmo shape do checkpoint: `{s3_key, md5}`
/// em snake_case — casa com `WeightRef` do orquestrador.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedTextEncoder {
    pub s3_key: String,
    pub md5: String,
}

/// Referência resolvida de imagem inicial para img2img (S4 — feat/img2img).
/// `md5` é `Some` quando vem de `generation_inputs` (upload avulso com hash
/// conhecido) e `None` quando vem de `generations` (galeria — hash não
/// persistido na linha; a verificação vira log no orchestrator).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedInitImage {
    pub s3_key: String,
    pub md5: Option<String>,
}

// ---------------------------------------------------------------------------
// Generations (D5 — ADR-0023)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct GenerationRow {
    pub id: String,
    /// `None` após o job de origem ser expurgado (AC-003: geração sobrevive ao
    /// job; `s3_key` continua válido para o proxy de imagem).
    pub job_id: Option<String>,
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

/// Defesa em profundidade (incidente galeria vazia): kinds que exigem artefato
/// não podem transitar para done sem eles.
///
/// Retorna `Some(motivo)` quando o report done viola a exigência; `None` = ok.
///
/// Tabela (decisão documentada):
/// - `diffusion_generate` → exige ≥1 artefato E ≥1 com kind `generated`.
/// - `yolo_train` → lista vazia/ausente é PRESERVADA como done (testes
///   `t7_ac006a_*` e `abort_em_voo_e_terminal` reportam done sem artefatos e
///   asseveram `done`); lista NÃO-vazia sem kind `model` → violação.
/// - `diffusion_train`, `yolo_predict`, `autotracker`, `autolabel` → exigem
///   lista não-vazia (por design todos produzem artefatos; nenhum teste
///   existente faz done vazio para esses kinds).
/// - kinds desconhecidos → permissivo (forward-compat).
pub fn done_artifacts_violation(kind: &str, artifacts: Option<&[ArtifactItem]>) -> Option<String> {
    let arts: &[ArtifactItem] = artifacts.unwrap_or(&[]);
    match kind {
        "diffusion_generate" => {
            if arts.is_empty() {
                return Some("diffusion_generate exige ao menos 1 artefato".into());
            }
            if !arts.iter().any(|a| a.kind == "generated") {
                return Some("diffusion_generate exige artefato kind 'generated'".into());
            }
            None
        }
        // Lista vazia/ausente preservada como done (t7_ac006a_*, abort_em_voo);
        // lista não-vazia sem modelo = treino sem produto → violação.
        "yolo_train" => {
            if !arts.is_empty() && !arts.iter().any(|a| a.kind == "model") {
                return Some("yolo_train exige artefato kind 'model'".into());
            }
            None
        }
        "diffusion_train" | "yolo_predict" | "autotracker" | "autolabel" => {
            if arts.is_empty() {
                return Some(format!("{kind} exige ao menos 1 artefato"));
            }
            None
        }
        _ => None,
    }
}

/// Cria um job. Retorna (job_id, status, queue_position).
///
/// Fluxo assíncrono (ADR-0025 D0): sem package_ref + `params.prepare` (objeto)
/// → `preparing` (queue_position NULL); com package_ref → `queued` (legado).
/// Omissão total (sem package_ref nem prepare) → `queued` legado
/// (retrocompat); intenção async malformada (package_ref null sem prepare, ou
/// prepare não-objeto) → `InvalidRequest` (400).
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
    // Resolução multi-LoRA + custom checkpoint + text encoder
    // (D3/D4 — ADR-0023; encoder — fatia feat/pesos-custom-flux2).
    // Params contém `loras: [{modelId, scale}]`, `customModelId` (uuid|null)
    // e `textEncoderModelId` (uuid|null).
    // Lê de forma tolerante: campos ausentes = legado (sem loras/custom/encoder).
    // Loras/img2img só existem no generate; custom + encoder valem p/ train e generate.
    // -------------------------------------------------------------------------
    if req.engine == "diffusion" && (req.mode == "generate" || req.mode == "train") {
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
                    // Fatia feat/pesos-custom-flux2: +flux-2-klein-4b (alias
                    // legado "flux" normalizado). sdxl/sd15 inalterados.
                    let arch_norm = if arch_val == "flux" {
                        "flux-2-klein-4b"
                    } else {
                        arch_val
                    };
                    if !matches!(arch_norm, "sdxl" | "sd15" | "flux-2-klein-4b") {
                        return Err(ManagerError::InvalidRequest(format!(
                            "customModelId arch must be 'sdxl', 'sd15' or 'flux-2-klein-4b', got '{arch_val}'"
                        )));
                    }
                    let resolved = ResolvedCheckpoint { s3_key, md5: hash };
                    if let Ok(v) = serde_json::to_value(&resolved) {
                        params["custom_checkpoint"] = v;
                    }
                }
            }
        }

        // Resolve textEncoderModelId (fatia feat/pesos-custom-flux2).
        // kind=text_encoder; arch EFETIVO deve ser flux-2-klein-4b.
        // O campo original camelCase é mantido (rastreabilidade); o resolvido
        // vai em snake_case `text_encoder_ref` ({s3_key, md5}).
        // Defesa em profundidade: o BFF já valida, mas params pode vir de outra origem.
        if let Some(encoder_id_str) = params.get("textEncoderModelId").and_then(|v| v.as_str()) {
            let encoder_uuid = Uuid::parse_str(encoder_id_str).map_err(|_| {
                ManagerError::InvalidRequest("textEncoderModelId must be a valid UUID".into())
            })?;
            let row: Option<(String, String, Option<String>, Option<String>)> =
                sqlx::query_as("SELECT s3_key, hash, kind, arch FROM models WHERE id = $1")
                    .bind(encoder_uuid)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| ManagerError::Internal(format!("resolve text encoder: {e}")))?;
            match row {
                None => {
                    return Err(ManagerError::InvalidRequest(
                        "textEncoderModelId not found".into(),
                    ));
                }
                Some((s3_key, hash, kind, _arch)) => {
                    if kind.as_deref() != Some("text_encoder") {
                        return Err(ManagerError::InvalidRequest(format!(
                            "textEncoderModelId must have kind='text_encoder', got {:?}",
                            kind
                        )));
                    }
                    let resolved = ResolvedTextEncoder { s3_key, md5: hash };
                    if let Ok(v) = serde_json::to_value(&resolved) {
                        params["text_encoder_ref"] = v;
                    }
                }
            }
            // Arch efetivo: o BFF envia baseModel já resolvido (generate:
            // arch do custom ou base; train: effective_base). Normaliza "flux".
            let effective = params
                .get("baseModel")
                .or_else(|| params.get("base_model"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let effective_norm = if effective == "flux" {
                "flux-2-klein-4b"
            } else {
                effective
            };
            if effective_norm != "flux-2-klein-4b" {
                return Err(ManagerError::InvalidRequest(
                    "textEncoderModelId requires arch 'flux-2-klein-4b'".into(),
                ));
            }
        }

        // initImageId/initGenerationId (img2img — S4): só no generate.
        // Treino nunca envia esses campos — o guarda evita tocar jobs de
        // treino que porventura carreguem chaves homônimas.
        if req.mode == "generate" {
            // Resolve initImageId (img2img — S4 feat/img2img).
            // O BFF envia camelCase; o campo original é MANTIDO em params
            // (rastreabilidade em generations.params — padrão da casa) e o
            // resolvido é gravado snake_case em `init_image_ref`.
            // Defesa em profundidade: BFF valida XOR, mas params pode vir de outra origem.
            if params.get("initImageId").and_then(|v| v.as_str()).is_some()
                && params
                    .get("initGenerationId")
                    .and_then(|v| v.as_str())
                    .is_some()
            {
                return Err(ManagerError::InvalidRequest(
                    "use either initImageId or initGenerationId, not both".into(),
                ));
            }
            if let Some(init_id_str) = params.get("initImageId").and_then(|v| v.as_str()) {
                let init_uuid = Uuid::parse_str(init_id_str).map_err(|_| {
                    ManagerError::InvalidRequest("initImageId must be a valid UUID".into())
                })?;

                let row: Option<(String, String)> =
                    sqlx::query_as("SELECT s3_key, md5 FROM generation_inputs WHERE id = $1")
                        .bind(init_uuid)
                        .fetch_optional(pool)
                        .await
                        .map_err(|e| ManagerError::Internal(format!("resolve init image: {e}")))?;

                match row {
                    None => {
                        return Err(ManagerError::InvalidRequest("initImageId not found".into()));
                    }
                    Some((s3_key, md5)) => {
                        // Marca consumo (best-effort: falha aqui não aborta o job).
                        let _ = sqlx::query(
                            "UPDATE generation_inputs SET used_at = now() WHERE id = $1",
                        )
                        .bind(init_uuid)
                        .execute(pool)
                        .await;
                        let resolved = ResolvedInitImage {
                            s3_key,
                            md5: Some(md5),
                        };
                        if let Ok(v) = serde_json::to_value(&resolved) {
                            params["init_image_ref"] = v;
                        }
                    }
                }
            }

            // Resolve initGenerationId (img2img via galeria — S4 feat/img2img).
            // Linha da galeria não persiste hash: `md5: null` (o orchestrator
            // só registra o md5 calculado, sem falhar).
            if let Some(gen_id_str) = params.get("initGenerationId").and_then(|v| v.as_str()) {
                let gen_uuid = Uuid::parse_str(gen_id_str).map_err(|_| {
                    ManagerError::InvalidRequest("initGenerationId must be a valid UUID".into())
                })?;

                let row: Option<(String,)> = sqlx::query_as(
                    "SELECT s3_key FROM generations WHERE id = $1 AND deleted_at IS NULL",
                )
                .bind(gen_uuid)
                .fetch_optional(pool)
                .await
                .map_err(|e| ManagerError::Internal(format!("resolve init generation: {e}")))?;

                match row {
                    None => {
                        return Err(ManagerError::InvalidRequest(
                            "initGenerationId not found".into(),
                        ));
                    }
                    Some((s3_key,)) => {
                        let resolved = ResolvedInitImage { s3_key, md5: None };
                        if let Ok(v) = serde_json::to_value(&resolved) {
                            params["init_image_ref"] = v;
                        }
                    }
                }
            }
        } // fecha `if req.mode == "generate"` (img2img só no generate)
    } // fecha `if diffusion generate|train`
      // Fluxo assíncrono (ADR-0025 D0): package_ref ausente + params.prepare
      // (objeto opaco do BFF) → `preparing`; package_ref presente → `queued`
      // (legado, retrocompat). Intenção async explícita porém malformada
      // (package_ref:null sem prepare, ou prepare não-objeto) → 400: job
      // ficaria eternamente `preparing` sem dono.
      // Omissão total (sem package E sem prepare, como os callers legados e os
      // testes de roteamento/generations) → `queued` legado (retrocompat total).
      // Posição tardia de propósito: resoluções fail-fast (weights_id,
      // orchestrator_hint, LoRA) mantêm precedência legada (404/400 próprios).
    let has_package = req.package_ref.is_some()
        || params
            .get("package_ref")
            .map(|v| {
                v.is_object()
                    && v.get("key")
                        .and_then(|k| k.as_str())
                        .map(|s| !s.is_empty())
                        .unwrap_or(false)
            })
            .unwrap_or(false);
    let prepare_field = params.get("prepare");
    let has_prepare = prepare_field.map(|v| v.is_object()).unwrap_or(false);
    if !has_package && !has_prepare {
        let package_null = params
            .get("package_ref")
            .map(|v| v.is_null())
            .unwrap_or(false);
        if package_null || prepare_field.is_some() {
            if prepare_field.is_some() {
                return Err(ManagerError::InvalidRequest(
                    "params.prepare deve ser um objeto".into(),
                ));
            }
            return Err(ManagerError::InvalidRequest(
                "job com package_ref null exige params.prepare (fluxo assíncrono ADR-0025)".into(),
            ));
        }
    }
    // package presente sempre vence → `queued`; só vai a `preparing` com
    // intenção async genuína (prepare objeto, sem package).
    let initial_status = if !has_package && has_prepare {
        "preparing"
    } else {
        "queued"
    };

    sqlx::query(
        "INSERT INTO jobs (id, kind, engine, model, mode, dataset_id, params, config_yaml, vram_min_gb, status, queue_reason) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NULL)",
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
    .bind(initial_status)
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("insert job: {e}")))?;

    // Posição na fila: só jobs `queued` contam; `preparing` → NULL.
    // (dispatch_next/list/get usam o mesmo predicado status='queued'.)
    let queue_position = if initial_status == "queued" {
        let queue_pos: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM jobs WHERE status = 'queued' AND created_at < (SELECT created_at FROM jobs WHERE id = $1)",
        )
        .bind(job_id)
        .fetch_one(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("queue position: {e}")))?;
        Some((queue_pos.0 + 1) as i32)
    } else {
        None
    };

    Ok(CreateJobResponse {
        job_id: job_id.to_string(),
        status: initial_status.to_string(),
        queue_position,
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
         j.phase, j.message, \
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
                phase: r.get("phase"),
                message: r.get("message"),
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
         j.phase, j.message, \
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
        phase: r.get("phase"),
        message: r.get("message"),
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

    let rows: Vec<(Uuid, String, String, String, i64)> = sqlx::query_as(
        "SELECT id, kind, path, md5, bytes FROM job_artifacts WHERE job_id = $1 \
             ORDER BY path, id",
    )
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

        "preparing" => {
            sqlx::query("UPDATE jobs SET status = 'cancelling' WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await
                .map_err(|e| ManagerError::Internal(format!("set cancelling: {e}")))?;
            Ok("cancelling".to_string())
        }

        "running" => {
            sqlx::query("UPDATE jobs SET status = 'cancelling' WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await
                .map_err(|e| ManagerError::Internal(format!("set cancelling: {e}")))?;

            // Notifica orquestrador com retry (até 3 tentativas com backoff).
            if let Some(orch_id) = orchestrator_id {
                if let Ok(orch) = get_orchestrator(pool, orch_id).await {
                    let body = serde_json::json!({"job_id": id.to_string()});
                    let url = format!("{}/internal/abort", orch.endpoint);
                    let mut sent = false;
                    for attempt in 0..3 {
                        if attempt > 0 {
                            tokio::time::sleep(std::time::Duration::from_millis(
                                200 * (1 << attempt),
                            ))
                            .await;
                        }
                        match orch_client.post(&url, &body).await {
                            Ok(_) => {
                                sent = true;
                                break;
                            }
                            Err(e) => {
                                tracing::warn!("tentativa {attempt} abort no orquestrador falhou para job {id}: {e}");
                            }
                        }
                    }
                    if !sent {
                        tracing::error!(
                            "todas tentativas de abort no orquestrador falharam para job {id}"
                        );
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
// Preparação assíncrona (ADR-0025 D0/D1/D3) — dono do estado `preparing`.
// Representação: SEM coluna nova em jobs (sem migration — CHECK da 0006 já
// cobre `preparing`); package_ref vive em `params->'package_ref'`
// {version_id,key,md5_zip,bytes} e o dataset_version_id espelhado em
// `params->'dataset_version_id'`; o spec opaco do BFF em `params->'prepare'`.
// Erros seguem a convenção do manager: `params->'error'` (exposto como
// `JobRow.error` via `params->>'error'`).
// ---------------------------------------------------------------------------

/// POST /internal/jobs/:id/prepare-complete: `preparing` → `queued`.
///
/// Transição guardada (0 linhas ⇒ 404 se inexistente, 409 se fora de
/// `preparing`). Persiste package_ref + dataset_version_id em params e renova
/// `dataset_versions.created_at` (touch anti-GC: versão reutilizada por
/// fingerprint sobrevive mais 7 dias). O chamador (handler) dispara
/// `dispatch_next` em seguida (best-effort).
pub async fn prepare_complete(
    pool: &PgPool,
    id: Uuid,
    req: PrepareCompleteRequest,
) -> Result<(), ManagerError> {
    let dv_id = Uuid::parse_str(&req.dataset_version_id).map_err(|_| {
        ManagerError::InvalidRequest("dataset_version_id must be a valid UUID".into())
    })?;
    if req.package_ref.key.is_empty() {
        return Err(ManagerError::InvalidRequest(
            "package_ref.key must not be empty".into(),
        ));
    }
    if !is_valid_md5(&req.package_ref.md5_zip) {
        return Err(ManagerError::InvalidRequest(format!(
            "invalid md5_zip: {}",
            req.package_ref.md5_zip
        )));
    }
    if req.package_ref.bytes < 0 {
        return Err(ManagerError::InvalidRequest(format!(
            "negative bytes: {}",
            req.package_ref.bytes
        )));
    }
    // Fail-fast: versão precisa existir (dono lógico: principal; DB físico
    // compartilhado no dev) — evita referência pendurada.
    let version_exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM dataset_versions WHERE id = $1)")
            .bind(dv_id)
            .fetch_one(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("check dataset version: {e}")))?;
    if !version_exists {
        return Err(ManagerError::NotFound);
    }

    let package_json = serde_json::json!({
        "version_id": dv_id.to_string(),
        "key": req.package_ref.key,
        "md5_zip": req.package_ref.md5_zip,
        "bytes": req.package_ref.bytes,
    });
    let result = sqlx::query(
        "UPDATE jobs SET status = 'queued', queue_reason = NULL, \
          params = params || jsonb_build_object('package_ref', $2::jsonb, 'dataset_version_id', $3) \
          WHERE id = $1 AND status = 'preparing'",
    )
    .bind(id)
    .bind(&package_json)
    .bind(dv_id.to_string())
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("prepare complete: {e}")))?;

    if result.rows_affected() == 0 {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM jobs WHERE id = $1)")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("check job: {e}")))?;
        if !exists {
            return Err(ManagerError::NotFound);
        }
        return Err(ManagerError::Conflict("job_not_preparing".into()));
    }
    // Touch anti-GC (ADR-0025 D4): versão reutilizada por fingerprint (D1)
    // renova o prazo de 7 dias. Sem coluna updated_at em dataset_versions, o
    // relógio do GC ancora em created_at. Só após transição aplicada: fora de
    // `preparing` (409/404 acima) nada é renovado.
    sqlx::query("UPDATE dataset_versions SET created_at = now() WHERE id = $1")
        .bind(dv_id)
        .execute(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("touch dataset version: {e}")))?;
    Ok(())
}

/// POST /internal/jobs/:id/prepare-fail: `preparing` → `failed` com
/// `params.error = 'prepare_failed:<code>:<message>'`.
pub async fn prepare_fail(
    pool: &PgPool,
    id: Uuid,
    req: PrepareFailRequest,
) -> Result<(), ManagerError> {
    if req.code.is_empty() || req.code.len() > 128 {
        return Err(ManagerError::InvalidRequest(
            "code must be 1-128 characters".into(),
        ));
    }
    if req.message.is_empty() {
        return Err(ManagerError::InvalidRequest(
            "message must not be empty".into(),
        ));
    }
    let error = format!("prepare_failed:{}:{}", req.code, req.message);
    let result = sqlx::query(
        "UPDATE jobs SET status = 'failed', finished_at = now(), \
          params = params || jsonb_build_object('error', $2) \
         WHERE id = $1 AND status = 'preparing'",
    )
    .bind(id)
    .bind(&error)
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("prepare fail: {e}")))?;

    if result.rows_affected() == 0 {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM jobs WHERE id = $1)")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("check job: {e}")))?;
        if !exists {
            return Err(ManagerError::NotFound);
        }
        return Err(ManagerError::Conflict("job_not_preparing".into()));
    }
    Ok(())
}

/// POST /internal/jobs/:id/prepare-cancel: `preparing` | `cancelling` → `cancelled`.
pub async fn prepare_cancel(pool: &PgPool, id: Uuid) -> Result<(), ManagerError> {
    let result = sqlx::query(
        "UPDATE jobs SET status = 'cancelled', finished_at = now() \
         WHERE id = $1 AND status IN ('preparing', 'cancelling')",
    )
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("prepare cancel: {e}")))?;

    if result.rows_affected() == 0 {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM jobs WHERE id = $1)")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("check job: {e}")))?;
        if !exists {
            return Err(ManagerError::NotFound);
        }
        let is_cancelled: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM jobs WHERE id = $1 AND status = 'cancelled')",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("check job cancelled: {e}")))?;
        if is_cancelled {
            return Ok(());
        }
        return Err(ManagerError::Conflict("job_not_cancelling".into()));
    }
    Ok(())
}

/// Watchdog de preparação (ADR-0025 D3): `preparing` com created_at > 60min
/// → `failed` (`params.error = 'prepare_timeout'`).
///
/// Sem coluna `updated_at` em jobs (e sem migration nesta fatia): o relógio
/// ancora em `created_at` — reports de progresso não estendem o prazo.
/// Prepares duram minutos; 60min desde a criação é limite seguro.
pub async fn watchdog_prepare_timeout(pool: &PgPool) -> Result<u64, ManagerError> {
    let result = sqlx::query(
        "UPDATE jobs SET status = 'failed', finished_at = now(), \
          params = params || jsonb_build_object('error', 'prepare_timeout') \
         WHERE status = 'preparing' AND created_at < now() - interval '60 minutes'",
    )
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("watchdog prepare timeout: {e}")))?;

    Ok(result.rows_affected())
}

/// GC de dataset_versions (ADR-0025 D4): apaga versões com >7 dias NUNCA
/// referenciadas por job aceito (referência = params.package_ref.version_id;
/// NUNCA apaga pacote referenciado). Query defensiva: IS NOT NULL no
/// version_id para NULL não virar match.
///
/// Corrida fingerprint (D1): o worker do principal pode REUSAR uma versão
/// antiga (D1) enquanto ela é alvo do GC — por isso versões cujo dataset
/// possui job não-terminal com `params.prepare.datasetId` são puladas. A
/// chave é camelCase porque o aceite do principal grava exatamente assim
/// (`accept_job_preparing`: `params.prepare = {kind, datasetId,
/// fingerprint}` — ver services/api-principal/src/jobs/prepare.rs).
pub async fn gc_dataset_versions(pool: &PgPool) -> Result<u64, ManagerError> {
    let result = sqlx::query(
        "DELETE FROM dataset_versions dv \
         WHERE dv.created_at < now() - interval '7 days' \
           AND NOT EXISTS (SELECT 1 FROM jobs j \
             WHERE (j.params->'package_ref'->>'version_id') IS NOT NULL \
               AND j.params->'package_ref'->>'version_id' = dv.id::text) \
           AND NOT EXISTS (SELECT 1 FROM jobs j \
             WHERE j.status NOT IN ('done','failed','cancelled') \
               AND j.params->'prepare'->>'datasetId' = dv.dataset_id::text)",
    )
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("gc dataset versions: {e}")))?;

    Ok(result.rows_affected())
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
async fn upsert_metrics_conn(
    conn: &mut sqlx::PgConnection,
    id: Uuid,
    new_metrics: &serde_json::Value,
) -> Result<(), ManagerError> {
    // Lê array existente (NULL → vazio via COALESCE).
    let existing: serde_json::Value =
        sqlx::query_scalar("SELECT COALESCE(metrics, '[]'::jsonb) FROM jobs WHERE id = $1")
            .bind(id)
            .fetch_one(&mut *conn)
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
        .execute(&mut *conn)
        .await
        .map_err(|e| ManagerError::Internal(format!("write metrics: {e}")))?;

    Ok(())
}

async fn upsert_metrics(
    pool: &PgPool,
    id: Uuid,
    new_metrics: &serde_json::Value,
) -> Result<(), ManagerError> {
    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| ManagerError::Internal(format!("acquire conn for metrics: {e}")))?;
    upsert_metrics_conn(&mut conn, id, new_metrics).await
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
        // `preparing` via report só existe dentro do ciclo async (ADR-0025):
        // job já fora de `preparing` (queued/dispatched/...) nunca regride
        // para `preparing` sem pacote — ignora mantendo o estado, no mesmo
        // estilo das guardas de terminal/cancelling acima (sem 409: o handler
        // mapeia Conflict para 500, e regressão de ciclo não é erro de
        // concorrência do chamador). Phase/progress/message de prepares
        // continuam fluindo normalmente enquanto o job está em `preparing`.
        "preparing" if current_status != "preparing" => {
            tracing::warn!(
                job_id = %id,
                current_status = %current_status,
                report_status = %report.status,
                "report preparing ignorado fora de preparing (sem regressão de ciclo)"
            );
            return Ok(());
        }
        "preparing" | "running" => {
            sqlx::query(
                "UPDATE jobs SET status = $2, progress = COALESCE($3, progress), epoch = COALESCE($4, epoch), step = COALESCE($5, step), phase = COALESCE($6, phase), message = COALESCE($7, message) WHERE id = $1",
            )
            .bind(id)
            .bind(&report.status)
            .bind(report.progress)
            .bind(report.epoch)
            .bind(report.step)
            .bind(&report.phase)
            .bind(&report.message)
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
            let mut tx = pool
                .begin()
                .await
                .map_err(|e| ManagerError::Internal(format!("begin report done tx: {e}")))?;

            // Defesa em profundidade (incidente galeria vazia): recusa o done
            // de kind que exige artefato quando a lista chega vazia/ausente ou
            // sem o artefato-chave — registra failed com erro no_artifacts e
            // mensagem PT (sem status novo, sem mexer no enum de wire).
            let job_kind: Option<String> =
                sqlx::query_scalar("SELECT kind FROM jobs WHERE id = $1")
                    .bind(id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|e| ManagerError::Internal(format!("get kind for done guard: {e}")))?;
            if let Some(kind) = job_kind {
                if let Some(reason) =
                    done_artifacts_violation(kind.as_str(), report.artifacts.as_deref())
                {
                    let err_code = format!("no_artifacts: {reason}");
                    let msg_pt = format!(
                        "Job finalizado sem os artefatos exigidos ({err_code}). Verifique o bucket S3 e os logs do orquestrador."
                    );
                    tracing::warn!(
                        job_id = %id,
                        kind = %kind,
                        reason = %reason,
                        "done recusado sem artefatos exigidos → failed/no_artifacts"
                    );
                    // UPDATE único: merge do erro em params + transição failed.
                    sqlx::query(
                        "UPDATE jobs SET params = params || $2::jsonb, status = 'failed', \
                         finished_at = now(), phase = COALESCE($3, phase), message = $4 \
                         WHERE id = $1",
                    )
                    .bind(id)
                    .bind(serde_json::json!({"error": err_code}))
                    .bind(&report.phase)
                    .bind(&msg_pt)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| ManagerError::Internal(format!("set failed no_artifacts: {e}")))?;
                    tx.commit().await.map_err(|e| {
                        ManagerError::Internal(format!("commit failed no_artifacts tx: {e}"))
                    })?;
                    return Ok(());
                }
            }

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
                    .execute(&mut *tx)
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
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| ManagerError::Internal(format!("insert artifact: {e}")))?;
                }
            }

            // Grava metrics (append + dedup por epoch).
            if let Some(metrics) = &report.metrics {
                upsert_metrics_conn(&mut tx, id, metrics).await?;
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
                    // Lê engine/model/mode/kind/dataset_id/params/config_yaml do job (ADR-0022 D0 + Bug 009:
                    // mode/kind/config_yaml alimentam a derivação de kind/arch do artefato).
                    let job_info: Option<(
                        String,
                        String,
                        String,
                        String,
                        Option<Uuid>,
                        serde_json::Value,
                        Option<String>,
                    )> = match sqlx::query_as::<_, (String, String, String, String, Option<Uuid>, serde_json::Value, Option<String>)>(
                            "SELECT engine, model, mode, kind, dataset_id, params, config_yaml FROM jobs WHERE id = $1",
                        )
                        .bind(id)
                        .fetch_optional(&mut *tx)
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
                    if let Some((engine, model, mode, kind, dataset_id, job_params, config_yaml)) =
                        job_info
                    {
                        let dataset_slug: Option<String> = if let Some(ds_id) = dataset_id {
                            match sqlx::query_scalar::<_, String>(
                                "SELECT slug FROM datasets WHERE id = $1",
                            )
                            .bind(ds_id)
                            .fetch_optional(&mut *tx)
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

                        // Bug 009: treinos de difusão emitem adapters LoRA, mas o INSERT
                        // abaixo não preenchia kind/arch → a geração rejeitava o modelo
                        // ("argumentos inválidos"). Classifica kind/arch só para treino
                        // de difusão; outras engines mantêm NULL (comportamento legado).
                        // Arch indeterminável → NULL + warn (nunca chuta).
                        let is_diffusion_train =
                            engine == "diffusion" && (mode == "train" || kind == "diffusion_train");
                        let train_arch: Option<String> = if is_diffusion_train {
                            match derive_diffusion_arch(&model, &job_params, config_yaml.as_deref())
                            {
                                Some(a) => Some(a),
                                None => {
                                    tracing::warn!(
                                        job_id = %id,
                                        "hook models: arch indeterminável p/ treino de difusão (kind será 'lora', arch NULL)"
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
                            // Deriva kind/arch por artefato (difusão-train) ou NULL (legado).
                            let (art_kind, art_arch): (Option<String>, Option<String>) =
                                if is_diffusion_train {
                                    (
                                        Some(classify_diffusion_model_kind(&art.path).to_string()),
                                        train_arch.clone(),
                                    )
                                } else {
                                    (None, None)
                                };
                            // ON CONFLICT DO UPDATE com COALESCE: corrige rows já
                            // quebradas (kind/arch NULL do Bug 009) sem sobrescrever
                            // valores definidos manualmente (ex.: upload via create_model).
                            let result = sqlx::query(
                                "INSERT INTO models (id, engine, name, model, s3_key, source, hash, bytes, job_id, kind, arch) \
                                 VALUES ($1, $2, $3, $4, $5, 'train', $6, $7, $8, $9, $10) \
                                 ON CONFLICT (s3_key) DO UPDATE SET kind = COALESCE(models.kind, EXCLUDED.kind), arch = COALESCE(models.arch, EXCLUDED.arch) WHERE models.kind IS NULL OR models.arch IS NULL",
                            )
                            .bind(Uuid::new_v4())
                            .bind(&engine)
                            .bind(&model_name)
                            .bind(Some(model.clone()))
                            .bind(&s3_key)
                            .bind(&art.md5)
                            .bind(art.bytes)
                            .bind(id)
                            .bind(&art_kind)
                            .bind(&art_arch)
                            .execute(&mut *tx)
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
                    .fetch_optional(&mut *tx)
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
                                .fetch_optional(&mut *tx)
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
                                                .get("thumb_filename")
                                                .or_else(|| entry.get("thumb"))
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
                                                obj.remove("thumb_filename");
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
                                            .execute(&mut *tx)
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
                "UPDATE jobs SET status = 'done', finished_at = now(), progress = 1.0, phase = COALESCE($2, phase), message = COALESCE($3, message) WHERE id = $1",
            )
            .bind(id)
            .bind(&report.phase)
            .bind(&report.message)
            .execute(&mut *tx)
            .await
            .map_err(|e| ManagerError::Internal(format!("set done: {e}")))?;

            tx.commit()
                .await
                .map_err(|e| ManagerError::Internal(format!("commit done tx: {e}")))?;
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

            sqlx::query("UPDATE jobs SET status = 'failed', finished_at = now(), phase = COALESCE($2, phase), message = COALESCE($3, message) WHERE id = $1")
                .bind(id)
                .bind(&report.phase)
                .bind(&report.message)
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
        if kind != "lora" && kind != "checkpoint" && kind != "text_encoder" {
            return Err(ManagerError::InvalidRequest(format!(
                "kind must be 'lora', 'checkpoint' or 'text_encoder', got '{}'",
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
    // kind=text_encoder só admite arch flux-2-klein-4b (encoder swap do Qwen3).
    if req.kind.as_deref() == Some("text_encoder") && req.arch.as_deref() != Some("flux-2-klein-4b")
    {
        return Err(ManagerError::InvalidRequest(
            "text_encoder requires arch 'flux-2-klein-4b'".into(),
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

// ---------------------------------------------------------------------------
// Exclusão de jobs (AC-003) — dono da fila/linhas é o manager; o sweep S3 é
// compensação best-effort do principal (ADR-0003 D6/D7: linha primeiro, objeto
// depois). Estados terminais são os únicos apagáveis (D-a do plano).
// ---------------------------------------------------------------------------

/// Linha apagada por `delete_job`/`cleanup_jobs` (AC-003).
///
/// `object_keys` é a lista EXATA de chaves S3 a varrer no sweep do principal —
/// já exclui as chaves das gerações preservadas (a galeria sobrevive ao job;
/// seus bytes vivem sob `artifacts/{job_id}/...`, o mesmo prefixo dos demais
/// artifacts, daí a necessidade de lista exata em vez de sweep por prefixo).
#[derive(Debug, Clone, Serialize)]
pub struct DeletedJob {
    pub id: String,
    pub status: String,
    /// Paths relativos dos artifacts (informativo p/ UI/toast).
    pub artifacts: Vec<String>,
    /// Chaves S3 completas a apagar (exclui gerações preservadas).
    pub object_keys: Vec<String>,
    /// Linhas do catálogo `models` derivadas deste job e expurgadas (D-a).
    pub models_deleted: i64,
    /// Gerações da galeria preservadas pelo SET NULL na FK (0012).
    pub generations_preserved: i64,
}

/// Resultado do `cleanup_jobs` (limpeza em lote).
#[derive(Debug, Clone, Serialize)]
pub struct CleanupResult {
    pub deleted: i64,
    pub jobs: Vec<DeletedJob>,
    /// União das `object_keys` de todos os jobs (conveniência p/ o sweep).
    pub object_keys: Vec<String>,
}

/// Estados terminais — os únicos apagáveis.
pub const TERMINAL_STATUSES: [&str; 3] = ["done", "failed", "cancelled"];

/// Monta a lista exata de chaves S3 a varrer num job (origem dupla:
/// artifacts + `models.s3_key` de órfãos de bytes, excluindo chaves de
/// gerações de todas as linhas do job, incl. trash) + conta modelos
/// expurgados/gerações preservadas. Deve rodar DENTRO da transação,
/// ANTES do `DELETE FROM jobs`.
async fn plan_job_sweep(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
) -> Result<(Vec<String>, Vec<String>, i64, i64), ManagerError> {
    // Chaves de gerações a PRESERVAR (s3_key + thumb_s3_key) — todas as linhas
    // do job, incl. trash (deleted_at preenchido; as linhas sobrevivem via
    // SET NULL da 0012 e continuam referenciando s3_key/thumb_s3_key).
    let gen_rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT s3_key, thumb_s3_key FROM generations \
         WHERE job_id = $1",
    )
    .bind(id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| ManagerError::Internal(format!("list generations to preserve: {e}")))?;
    let preserved: std::collections::HashSet<String> = gen_rows
        .iter()
        .flat_map(|(k, t)| {
            let mut v = vec![k.clone()];
            if let Some(th) = t {
                v.push(th.clone());
            }
            v
        })
        .collect();
    let generations_preserved = gen_rows.len() as i64;

    // Chaves dos artifacts, excluindo as preservadas.
    let paths: Vec<String> =
        sqlx::query_scalar("SELECT path FROM job_artifacts WHERE job_id = $1 ORDER BY path, id")
            .bind(id)
            .fetch_all(&mut **tx)
            .await
            .map_err(|e| ManagerError::Internal(format!("list artifacts before delete: {e}")))?;

    // Chaves órfãs de bytes do catálogo `models` (s3_key NOT NULL, 0007) —
    // capturadas ANTES do DELETE, pois vivem fora do prefixo artifacts/{job}/.
    let model_keys: Vec<String> = sqlx::query_scalar("SELECT s3_key FROM models WHERE job_id = $1")
        .bind(id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| ManagerError::Internal(format!("list models before delete: {e}")))?;

    // União deduplicada e ordenada (artifacts + models), sem as preservadas.
    let object_keys: Vec<String> = {
        let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for p in &paths {
            let k = format!("artifacts/{id}/{p}");
            if !preserved.contains(&k) {
                set.insert(k);
            }
        }
        for k in &model_keys {
            if !preserved.contains(k) {
                set.insert(k.clone());
            }
        }
        set.into_iter().collect()
    };

    // Expurga linhas do catálogo de modelos derivadas deste job (D-a: sim).
    let models_deleted = sqlx::query("DELETE FROM models WHERE job_id = $1")
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(|e| ManagerError::Internal(format!("delete job models: {e}")))?
        .rows_affected() as i64;

    Ok((paths, object_keys, models_deleted, generations_preserved))
}

/// Apaga um job terminal e retorna a linha + as chaves S3 a varrer.
///
/// Guarda: `done|failed|cancelled` → apaga (FK `job_artifacts ON DELETE
/// CASCADE`; `generations` ficam com `job_id` NULL — galeria preservada);
/// qualquer outro status → [`ManagerError::NotDeletable`]; inexistente →
/// [`ManagerError::NotFound`].
pub async fn delete_job(pool: &PgPool, id: Uuid) -> Result<DeletedJob, ManagerError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| ManagerError::Internal(format!("begin delete job: {e}")))?;

    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM jobs WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| ManagerError::Internal(format!("lock job for delete: {e}")))?;

    let status = status.ok_or(ManagerError::NotFound)?;
    if !TERMINAL_STATUSES.contains(&status.as_str()) {
        return Err(ManagerError::NotDeletable);
    }

    let (artifacts, object_keys, models_deleted, generations_preserved) =
        plan_job_sweep(&mut tx, id).await?;

    sqlx::query("DELETE FROM jobs WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ManagerError::Internal(format!("delete job: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| ManagerError::Internal(format!("commit delete job: {e}")))?;

    Ok(DeletedJob {
        id: id.to_string(),
        status,
        artifacts,
        object_keys,
        models_deleted,
        generations_preserved,
    })
}

/// Normaliza e valida os statuses pedidos numa limpeza em lote.
/// `None`/vazio → todos os terminais; qualquer status não-terminal → erro.
pub fn normalize_cleanup_statuses(
    statuses: Option<&Vec<String>>,
) -> Result<Vec<String>, ManagerError> {
    let list = match statuses {
        None => Vec::new(),
        Some(v) => v.clone(),
    };
    let list = if list.is_empty() {
        TERMINAL_STATUSES.iter().map(|s| s.to_string()).collect()
    } else {
        for s in &list {
            if !TERMINAL_STATUSES.contains(&s.as_str()) {
                return Err(ManagerError::InvalidRequest(format!(
                    "cleanup só aceita estados terminais (done|failed|cancelled), recebido: {s}"
                )));
            }
        }
        list
    };
    Ok(list)
}

/// Limpeza em lote de jobs terminais (AC-003).
///
/// `older_than_days`: apaga apenas jobs terminais há mais de N dias
/// (`COALESCE(finished_at, created_at)`); `None` = sem recorte temporal.
/// Exige pelo menos um critério (dias ou statuses) para não apagar o
/// histórico inteiro por omissão. Retorna as linhas apagadas com as chaves
/// S3 a varrer (exclui gerações preservadas).
pub async fn cleanup_jobs(
    pool: &PgPool,
    older_than_days: Option<i64>,
    statuses: Option<Vec<String>>,
) -> Result<CleanupResult, ManagerError> {
    if older_than_days.is_none() && statuses.as_ref().map_or(true, |s| s.is_empty()) {
        return Err(ManagerError::InvalidRequest(
            "cleanup exige pelo menos um critério (olderThanDays ou statuses)".into(),
        ));
    }
    if let Some(d) = older_than_days {
        if d < 0 {
            return Err(ManagerError::InvalidRequest(
                "olderThanDays deve ser >= 0".into(),
            ));
        }
    }
    let statuses = normalize_cleanup_statuses(statuses.as_ref())?;

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| ManagerError::Internal(format!("begin cleanup: {e}")))?;

    // Seleciona os candidatos (lock pessimista p/ não corrida com report/dispatch).
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, status FROM jobs \
         WHERE status = ANY($1) \
           AND ($2::bigint IS NULL OR COALESCE(finished_at, created_at) < NOW() - ($2::bigint * INTERVAL '1 day')) \
         FOR UPDATE SKIP LOCKED",
    )
    .bind(&statuses)
    .bind(older_than_days)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| ManagerError::Internal(format!("select jobs to cleanup: {e}")))?;

    let mut jobs = Vec::with_capacity(rows.len());
    let mut all_keys: Vec<String> = Vec::new();
    for (id, status) in rows {
        let (artifacts, object_keys, models_deleted, generations_preserved) =
            plan_job_sweep(&mut tx, id).await?;

        sqlx::query("DELETE FROM jobs WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| ManagerError::Internal(format!("delete job in cleanup: {e}")))?;

        all_keys.extend(object_keys.iter().cloned());
        jobs.push(DeletedJob {
            id: id.to_string(),
            status,
            artifacts,
            object_keys,
            models_deleted,
            generations_preserved,
        });
    }

    tx.commit()
        .await
        .map_err(|e| ManagerError::Internal(format!("commit cleanup: {e}")))?;

    let deleted = jobs.len() as i64;
    Ok(CleanupResult {
        deleted,
        jobs,
        object_keys: all_keys,
    })
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

// ---------------------------------------------------------------------------
// Classificação kind/arch de artefatos de treino de difusão (Bug 009)
// ---------------------------------------------------------------------------

/// Normaliza um nome de modelo base para o `arch` canônico do catálogo
/// (`sdxl` | `sd15` | `flux-2-klein-4b` — CHECK da migration 0011).
/// Retorna None quando impossível derivar com confiança (o chamador deixa
/// NULL + log warn em vez de chutar).
pub fn normalize_diffusion_arch(raw: &str) -> Option<String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "sdxl" => Some("sdxl".to_string()),
        "sd15" | "sd1.5" | "sd_15" | "sd-15" | "sd 15" => Some("sd15".to_string()),
        "flux" | "flux2" | "flux-2-klein" | "flux-2-klein-4b" | "flux2-klein-4b" => {
            Some("flux-2-klein-4b".to_string())
        }
        _ => None,
    }
}

/// Deriva o `arch` de um job de treino de difusão, por prioridade:
/// 1. `params.baseModel` (wire camelCase do BFF) / `params.base_model` (legado);
/// 2. coluna `jobs.model` (= base_model do treino);
/// 3. linha `model: "x"` do `config_yaml` gerado pelo api-principal.
/// Retorna None se nenhuma fonte normalizar (não chuta).
pub fn derive_diffusion_arch(
    job_model: &str,
    params: &serde_json::Value,
    config_yaml: Option<&str>,
) -> Option<String> {
    for key in ["baseModel", "base_model"] {
        if let Some(s) = params.get(key).and_then(|v| v.as_str()) {
            if let Some(arch) = normalize_diffusion_arch(s) {
                return Some(arch);
            }
        }
    }
    if let Some(arch) = normalize_diffusion_arch(job_model) {
        return Some(arch);
    }
    if let Some(yaml) = config_yaml {
        for line in yaml.lines() {
            // Match ancorado no início da linha aparada: não confunde
            // `model:` com `openai_model:` (config de autolabel).
            if let Some(rest) = line.trim().strip_prefix("model:") {
                let v = rest.trim().trim_matches('"').trim_matches('\'').trim();
                if let Some(arch) = normalize_diffusion_arch(v) {
                    return Some(arch);
                }
            }
        }
    }
    None
}

/// Classifica o `kind` de um artefato final de treino de difusão.
/// O trainer-difusao emite adapters LoRA (`adapter.safetensors` ou
/// `<output_name>.safetensors` — ver `common.py:resolve_lora_basename` e
/// `models/{flux,sdxl,sd15,mock}.py`); um checkpoint completo fundido
/// (ex.: `best.safetensors`/`merged.safetensors`) vira `checkpoint`.
pub fn classify_diffusion_model_kind(art_path: &str) -> &'static str {
    let lower = art_path.to_ascii_lowercase();
    if lower.contains("adapter") || lower.contains("lora") {
        "lora"
    } else {
        "checkpoint"
    }
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
///
/// `preparing` é EXCLUÍDO de propósito: recuperação de prepares pertence ao
/// principal (`job_prepares`/`recover_stale_prepares`, ADR-0025 D3) — um
/// preparing órfão nunca vira `queued` sem pacote; morre pelo watchdog de
/// 60min (`watchdog_prepare_timeout`) se o worker não voltar.
pub async fn recover_jobs(pool: &PgPool) -> Result<u64, ManagerError> {
    // 1. Jobs que estavam em 'cancelling' no boot passam para 'cancelled':
    sqlx::query(
        "UPDATE jobs SET status = 'cancelled', finished_at = now(), queue_reason = 'recovered_cancel' \
         WHERE status = 'cancelling'",
    )
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("recover cancelling jobs: {e}")))?;

    // 2. Apenas jobs com pacote pronto (package_ref presente em params) voltam para queued:
    let result = sqlx::query(
        "UPDATE jobs SET status = 'queued', queue_reason = 'recovered', orchestrator_id = NULL \
         WHERE status IN ('dispatched', 'running') \
           AND (params->>'package_ref' IS NOT NULL OR params->>'dataset_version_id' IS NOT NULL)",
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
    let limit_idx = bind_idx;
    let offset_idx = bind_idx + 1;
    let main_sql = format!(
        "SELECT g.id, g.job_id, g.s3_key, g.thumb_s3_key, g.filename, g.seed, \
         g.prompt, g.negative_prompt, g.width, g.height, g.params, g.created_at, g.deleted_at \
         FROM generations g {where_sql} \
         ORDER BY g.created_at DESC LIMIT ${limit_idx} OFFSET ${offset_idx}"
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
            let job_id: Option<Uuid> = r.get("job_id");
            let created_at: DateTime<Utc> = r.get("created_at");
            let deleted_at: Option<DateTime<Utc>> = r.get("deleted_at");
            GenerationRow {
                id: id.to_string(),
                job_id: job_id.map(|u| u.to_string()),
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
        let job_id: Option<Uuid> = r.get("job_id");
        let created_at: DateTime<Utc> = r.get("created_at");
        let deleted_at: Option<DateTime<Utc>> = r.get("deleted_at");
        GenerationRow {
            id: id.to_string(),
            job_id: job_id.map(|u| u.to_string()),
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
    // `preparing` excluído como no recover: jobs em preparação caem no caminho
    // prepare-timeout/fail, nunca viram queued sem pacote.
    // 1. Jobs do nó morto em 'cancelling' passam para 'cancelled':
    let _ = sqlx::query(
        "WITH morto AS ( \
             SELECT id FROM orchestrators \
             WHERE status = 'degraded' \
               AND (last_heartbeat IS NULL OR last_heartbeat < now() - make_interval(secs => $1::float)) \
         ) \
         UPDATE jobs SET status = 'cancelled', finished_at = now(), orchestrator_id = NULL \
         WHERE orchestrator_id IN (SELECT id FROM morto) \
           AND status = 'cancelling'",
    )
    .bind(offline_s as f64)
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("watchdog cancel cancelling jobs: {e}")))?;

    // 2. degraded → offline + re-queue dos jobs com pacote do nó morto:
    let result = sqlx::query(
        "WITH morto AS ( \
             UPDATE orchestrators SET status = 'offline' \
             WHERE status = 'degraded' \
               AND (last_heartbeat IS NULL OR last_heartbeat < now() - make_interval(secs => $1::float)) \
             RETURNING id \
         ) \
         UPDATE jobs SET status = 'queued', queue_reason = 'recovered', orchestrator_id = NULL \
         WHERE orchestrator_id IN (SELECT id FROM morto) \
           AND status IN ('dispatched','running') \
           AND (params->>'package_ref' IS NOT NULL OR params->>'dataset_version_id' IS NOT NULL)",
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

    // Preparação travada (ADR-0025 D3): `preparing` > 60min → failed.
    match watchdog_prepare_timeout(pool).await {
        Ok(n) if n > 0 => tracing::info!("watchdog: {n} jobs preparing expirados → failed"),
        Ok(_) => {}
        Err(e) => tracing::warn!("watchdog prepare-timeout error: {e}"),
    }

    // GC de dataset_versions órfãs >7 dias (ADR-0025 D4) — mesmo loop.
    match gc_dataset_versions(pool).await {
        Ok(n) if n > 0 => tracing::info!("gc: {n} dataset_versions órfãs removidas"),
        Ok(_) => {}
        Err(e) => tracing::warn!("gc dataset_versions error: {e}"),
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

/// Resolve imagem do container para engine "diffusion".
///
/// Regra: env `DIFFUSION_TRAINER_IMAGE` explícito SEMPRE vence.
/// Quando o env está ausente, herda a tag (:gpu/:local) da `image` base
/// (TRAINER_IMAGE) — preservando o comportamento TrueNAS com :gpu.
pub fn resolve_diffusion_image(image: &str) -> String {
    let env_diff = std::env::var("DIFFUSION_TRAINER_IMAGE").unwrap_or_default();
    if !env_diff.is_empty() {
        env_diff
    } else if image.ends_with(":gpu") || image.contains(":gpu") {
        "hephaestus/trainer-difusao:gpu".to_string()
    } else if image.contains("trainer-yolo") {
        image.replace("trainer-yolo", "trainer-difusao")
    } else {
        image.to_string()
    }
}

pub async fn dispatch_next(
    pool: &PgPool,
    orch_client: &dyn OrchestratorClient,
    exec_mode: &str,
    orch_workdir: &str,
    image: &str,
    vram_table: &VramTable,
) -> Result<bool, ManagerError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| ManagerError::Internal(format!("begin dispatch tx: {e}")))?;

    // 1. Seleciona próximo job queued (FIFO) com lock exclusivo SKIP LOCKED.
    let row: Option<(
        Uuid,
        String,
        String,
        String,
        Option<serde_json::Value>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, engine, model, mode, params, config_yaml \
         FROM jobs WHERE status = 'queued' ORDER BY created_at LIMIT 1 \
         FOR UPDATE SKIP LOCKED",
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| ManagerError::Internal(format!("select next job: {e}")))?;

    let (job_id, engine, model, mode, params, config_yaml) = match row {
        Some(r) => r,
        None => return Ok(false),
    };

    // 2. Resolve requisito VRAM da vram-table.
    let required_gb: Option<i32> = vram_table.resolve_required_gb(&engine, &model, &mode);

    // 3. Seleciona orquestrador (ADR-0015 D3) com lock FOR UPDATE OF o.
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
                                 AND j.status IN ('dispatched','running','cancelling')) \
               AND ($2::int IS NULL OR o.vram_total_gb IS NULL OR o.vram_total_gb >= $2) \
             FOR UPDATE OF o",
        )
        .bind(hint_id)
        .bind(required_gb)
        .fetch_optional(&mut *tx)
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
                                 AND j.status IN ('dispatched','running','cancelling')) \
               AND ($1::int IS NULL OR o.vram_total_gb IS NULL OR o.vram_total_gb >= $1) \
             ORDER BY (o.vram_total_gb IS NULL) ASC, \
                      o.vram_total_gb DESC NULLS LAST, \
                      o.name ASC \
             LIMIT 1 \
             FOR UPDATE OF o",
        )
        .bind(required_gb)
        .fetch_optional(&mut *tx)
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
                .execute(&mut *tx)
                .await
                .map_err(|e| ManagerError::Internal(format!("set queue reason: {e}")))?;
            tx.commit()
                .await
                .map_err(|e| ManagerError::Internal(format!("commit queue reason tx: {e}")))?;
            return Ok(false);
        }
    };

    // 4. Marca dispatched e atualiza flag de fallback em params dentro da transação.
    if fallback_used {
        sqlx::query(
            "UPDATE jobs SET status = 'dispatched', queue_reason = NULL, orchestrator_id = $2, \
             params = jsonb_set(params, '{orchestrator_fallback}', 'true'::jsonb) \
             WHERE id = $1 AND status = 'queued'",
        )
        .bind(job_id)
        .bind(orch_id)
        .execute(&mut *tx)
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
        .execute(&mut *tx)
        .await
        .map_err(|e| ManagerError::Internal(format!("set dispatched: {e}")))?;
    }

    tx.commit()
        .await
        .map_err(|e| ManagerError::Internal(format!("commit dispatch tx: {e}")))?;

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

    // Extrai init_image_ref resolvido do params (img2img — S4 feat/img2img).
    let init_image_ref = params
        .as_ref()
        .and_then(|p| p.get("init_image_ref"))
        .cloned();

    // Extrai text_encoder_ref resolvido do params (fatia feat/pesos-custom-flux2).
    let text_encoder_ref = params
        .as_ref()
        .and_then(|p| p.get("text_encoder_ref"))
        .cloned();
    // Resolve imagem do container: se engine for diffusion, usa DIFFUSION_TRAINER_IMAGE
    // (env explícito SEMPRE vence) ou herda tag de TRAINER_IMAGE (fallback p/ TrueNAS :gpu).
    let job_image = match engine.as_str() {
        "diffusion" => resolve_diffusion_image(image),
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

    // Adiciona init_image_ref ao dispatch quando presente (S4 — feat/img2img).
    // snake_case: `init_image_ref: {s3_key, md5|null}` — casa com InitImageRef do orquestrador.
    if let Some(iir) = init_image_ref {
        dispatch_body["init_image_ref"] = iir;
    }

    // Adiciona text_encoder ao dispatch quando presente (fatia feat/pesos-custom-flux2).
    // snake_case: `text_encoder: {s3_key, md5}` — casa com WeightRef do orquestrador.
    if let Some(te) = text_encoder_ref {
        dispatch_body["text_encoder"] = te;
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

    // -- done_artifacts_violation: defesa no_artifacts (incidente galeria vazia) --

    fn art(kind: &str) -> ArtifactItem {
        ArtifactItem {
            kind: kind.into(),
            path: format!("{kind}.bin"),
            md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
            bytes: 10,
        }
    }

    #[test]
    fn done_violation_diffusion_generate_sem_artifacts() {
        assert!(done_artifacts_violation("diffusion_generate", None).is_some());
        assert!(done_artifacts_violation("diffusion_generate", Some(&[])).is_some());
    }

    #[test]
    fn done_violation_diffusion_generate_sem_generated() {
        let arts = vec![art("generated_meta"), art("generated_thumb")];
        assert!(done_artifacts_violation("diffusion_generate", Some(&arts)).is_some());
    }

    #[test]
    fn done_violation_diffusion_generate_ok() {
        let arts = vec![art("generated"), art("generated_meta")];
        assert!(done_artifacts_violation("diffusion_generate", Some(&arts)).is_none());
    }

    #[test]
    fn done_violation_yolo_train_vazio_preservado() {
        // Preservação (t7_ac006a_*, abort_em_voo_e_terminal): done vazio segue done.
        assert!(done_artifacts_violation("yolo_train", None).is_none());
        assert!(done_artifacts_violation("yolo_train", Some(&[])).is_none());
    }

    #[test]
    fn done_violation_yolo_train_exige_modelo() {
        assert!(done_artifacts_violation("yolo_train", Some(&[art("model")])).is_none());
        let arts = vec![art("metrics")];
        assert!(done_artifacts_violation("yolo_train", Some(&arts)).is_some());
    }

    #[test]
    fn done_violation_treinos_e_predicao_exigem_lista() {
        for kind in [
            "diffusion_train",
            "yolo_predict",
            "autotracker",
            "autolabel",
        ] {
            assert!(
                done_artifacts_violation(kind, None).is_some(),
                "{kind} vazio deve violar"
            );
            assert!(
                done_artifacts_violation(kind, Some(&[])).is_some(),
                "{kind} vazio deve violar"
            );
            assert!(
                done_artifacts_violation(kind, Some(&[art("model")])).is_none(),
                "{kind} com artefato deve passar"
            );
        }
    }

    #[test]
    fn done_violation_kind_desconhecido_permissivo() {
        assert!(done_artifacts_violation("futura_engine_x", None).is_none());
    }

    #[test]
    fn normalize_cleanup_statuses_default_sao_terminais() {
        let got = normalize_cleanup_statuses(None).unwrap();
        assert_eq!(
            got,
            vec!["done".to_string(), "failed".into(), "cancelled".into()]
        );
        // Lista vazia → mesmo default.
        let got2 = normalize_cleanup_statuses(Some(&Vec::new())).unwrap();
        assert_eq!(got2, got);
    }

    #[test]
    fn normalize_cleanup_statuses_rejeita_nao_terminal() {
        let bad = vec!["running".to_string()];
        let err = normalize_cleanup_statuses(Some(&bad)).unwrap_err();
        assert!(matches!(err, ManagerError::InvalidRequest(_)));
        // Subset válido passa.
        let ok = vec!["done".to_string()];
        assert_eq!(
            normalize_cleanup_statuses(Some(&ok)).unwrap(),
            vec!["done".to_string()]
        );
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
    #[test]
    fn test_validate_create_model_text_encoder() {
        // Helper: request base válido diffusion.
        let base = |kind: Option<&str>, arch: Option<&str>| CreateModelRequest {
            id: Uuid::new_v4(),
            engine: "diffusion".to_string(),
            name: "enc.safetensors".to_string(),
            model: None,
            s3_key: "models/diffusion/x/enc.safetensors".to_string(),
            source: "upload".to_string(),
            url: None,
            hash: "d41d8cd98f00b204e9800998ecf8427e".to_string(),
            bytes: 100,
            job_id: None,
            kind: kind.map(|s| s.to_string()),
            arch: arch.map(|s| s.to_string()),
        };
        // text_encoder + flux-2 ⇒ ok.
        assert!(
            validate_create_model(&base(Some("text_encoder"), Some("flux-2-klein-4b"))).is_ok()
        );
        // text_encoder + sdxl/sd15/ausente ⇒ 400.
        assert!(validate_create_model(&base(Some("text_encoder"), Some("sdxl"))).is_err());
        assert!(validate_create_model(&base(Some("text_encoder"), Some("sd15"))).is_err());
        assert!(validate_create_model(&base(Some("text_encoder"), None)).is_err());
        // checkpoint segue exigindo arch (inalterado).
        assert!(validate_create_model(&base(Some("checkpoint"), None)).is_err());
        assert!(validate_create_model(&base(Some("checkpoint"), Some("sdxl"))).is_ok());
    }

    // ── Bug 009: classificação kind/arch de treino de difusão ─────────────

    #[test]
    fn normalize_diffusion_arch_casos_suportados() {
        assert_eq!(normalize_diffusion_arch("sdxl"), Some("sdxl".into()));
        assert_eq!(normalize_diffusion_arch(" SDXL "), Some("sdxl".into()));
        assert_eq!(normalize_diffusion_arch("sd15"), Some("sd15".into()));
        assert_eq!(normalize_diffusion_arch("SD1.5"), Some("sd15".into()));
        assert_eq!(
            normalize_diffusion_arch("flux"),
            Some("flux-2-klein-4b".into())
        );
        assert_eq!(
            normalize_diffusion_arch("flux-2-klein-4b"),
            Some("flux-2-klein-4b".into())
        );
    }

    #[test]
    fn normalize_diffusion_arch_desconhecido_e_none() {
        assert_eq!(normalize_diffusion_arch("unsupported"), None);
        assert_eq!(normalize_diffusion_arch(""), None);
        assert_eq!(normalize_diffusion_arch("yolo11m"), None);
    }

    #[test]
    fn derive_diffusion_arch_prioridade_params_model_yaml() {
        // params.baseModel (camelCase do BFF) vence jobs.model.
        let p = serde_json::json!({"baseModel": "sd15"});
        assert_eq!(
            derive_diffusion_arch("sdxl", &p, Some("model: \"sdxl\"")),
            Some("sd15".into())
        );
        // snake_case legado também vale.
        let p2 = serde_json::json!({"base_model": "sdxl"});
        assert_eq!(
            derive_diffusion_arch("sd15", &p2, None),
            Some("sdxl".into())
        );
        // Sem params: cai para jobs.model.
        let p3 = serde_json::json!({});
        assert_eq!(
            derive_diffusion_arch("sdxl", &p3, None),
            Some("sdxl".into())
        );
        // Sem params nem model válido: extrai do config_yaml.
        assert_eq!(
            derive_diffusion_arch("", &p3, Some("job_id: \"x\"\nmodel: \"sd15\"\n")),
            Some("sd15".into())
        );
        // Nada derivável: None (não chuta).
        assert_eq!(derive_diffusion_arch("", &p3, None), None);
        assert_eq!(
            derive_diffusion_arch("???", &p3, Some("model: \"???\"")),
            None
        );
        // `openai_model:` (autolabel) não contamina a extração do yaml.
        assert_eq!(
            derive_diffusion_arch(
                "",
                &p3,
                Some("openai_model: \"gpt-4o\"\nmode: \"autolabel\"\n")
            ),
            None
        );
    }

    #[test]
    fn classify_diffusion_model_kind_adapter_vs_checkpoint() {
        assert_eq!(classify_diffusion_model_kind("adapter.safetensors"), "lora");
        assert_eq!(
            classify_diffusion_model_kind("checkpoints/adapter_final.safetensors"),
            "lora"
        );
        assert_eq!(
            classify_diffusion_model_kind("meu-lora-v1.safetensors"),
            "lora"
        );
        assert_eq!(
            classify_diffusion_model_kind("best.safetensors"),
            "checkpoint"
        );
        assert_eq!(
            classify_diffusion_model_kind("merged-model.safetensors"),
            "checkpoint"
        );
    }

    // ── resolve_diffusion_image ──────────────────────────────────────────

    /// Serializa testes que manipulam env global (DIFFUSION_TRAINER_IMAGE).
    use std::sync::Mutex;
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Teste 1: env explícito `:local` com image `:gpu` → env vence.
    #[test]
    fn resolve_diffusion_image_env_local_vence_sobre_gpu() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(
            "DIFFUSION_TRAINER_IMAGE",
            "hephaestus/trainer-difusao:local",
        );
        let result = super::resolve_diffusion_image("hephaestus/trainer-yolo:gpu");
        assert_eq!(result, "hephaestus/trainer-difusao:local");
        std::env::remove_var("DIFFUSION_TRAINER_IMAGE");
    }

    /// Teste 2: env ausente + image `:gpu` → herança :gpu (TrueNAS preservado).
    #[test]
    fn resolve_diffusion_image_sem_env_herda_gpu() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("DIFFUSION_TRAINER_IMAGE");
        let result = super::resolve_diffusion_image("hephaestus/trainer-yolo:gpu");
        assert_eq!(result, "hephaestus/trainer-difusao:gpu");
    }

    /// Teste 3: env ausente + image `:local` → herança :local.
    #[test]
    fn resolve_diffusion_image_sem_env_herda_local() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("DIFFUSION_TRAINER_IMAGE");
        let result = super::resolve_diffusion_image("hephaestus/trainer-yolo:local");
        assert_eq!(result, "hephaestus/trainer-difusao:local");
    }

    /// Teste 4: env customizado → usa exatamente o env.
    #[test]
    fn resolve_diffusion_image_env_custom() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("DIFFUSION_TRAINER_IMAGE", "meu-registry/exemplo:tag");
        let result = super::resolve_diffusion_image("hephaestus/trainer-yolo:gpu");
        assert_eq!(result, "meu-registry/exemplo:tag");
        std::env::remove_var("DIFFUSION_TRAINER_IMAGE");
    }
}
