use serde::{Deserialize, Serialize};

pub fn default_mode() -> String {
    "train".to_string()
}

/// Referência a pesos de modelo no S3 (fine-tune — ADR-0012 D5).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WeightsRef {
    /// S3 key do peso: `models/<engine>/<id>/<name>` ou `artifacts/<job_id>/<path>`.
    pub s3_key: String,
    /// MD5 hash esperado (hex 32).
    pub md5: String,
}

/// Referência a um LoRA para staging multi-ref (D3 — ADR-0023).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoraRefStage {
    /// S3 key do LoRA (.safetensors).
    pub s3_key: String,
    /// MD5 hash esperado (hex 32).
    pub md5: String,
    /// Escala do LoRA (0..2).
    pub scale: f64,
}

/// Referência a um checkpoint custom para staging multi-ref (D4 — ADR-0023).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WeightRef {
    /// S3 key do checkpoint (.safetensors).
    pub s3_key: String,
    /// MD5 hash esperado (hex 32).
    pub md5: String,
}

/// Referência à imagem inicial para img2img (S4 — feat/img2img).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InitImageRef {
    /// S3 key da imagem: `generation_inputs/<...>` ou `artifacts/<job_id>/<path>`.
    pub s3_key: String,
    /// MD5 hash esperado (hex 32). `None` = origem galeria (hash não
    /// persistido na linha) — a verificação vira log, sem falha.
    #[serde(default)]
    pub md5: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DispatchRequest {
    pub job_id: String,
    pub engine: String,
    pub image: String,
    pub exec_mode: String,
    #[serde(default)]
    pub package_ref: Option<PackageRef>,
    pub config_yaml: Option<String>,
    pub dataset_version_id: Option<String>,
    pub workdir: String,
    /// Modo de operação: `train` (default) ou `predict` (ADR-0013 D6).
    #[serde(default = "default_mode")]
    pub mode: String,
    /// Pesos de modelo para fine-tune (ADR-0012 D5). `None` = treino do zero.
    #[serde(default)]
    pub weights_ref: Option<WeightsRef>,
    /// LoRAs para staging multi-ref (D3 — ADR-0023). Empty = sem LoRAs.
    #[serde(default)]
    pub loras: Vec<LoraRefStage>,
    /// Checkpoint custom para staging multi-ref (D4 — ADR-0023).
    #[serde(default)]
    pub custom_checkpoint: Option<WeightRef>,
    /// Text encoder custom para staging (fatia feat/pesos-custom-flux2).
    /// `None` = encoder oficial do repo BFL. Mesmo shape do checkpoint
    /// (`{s3_key, md5}` — casa com `text_encoder_ref`/`text_encoder` do manager).
    #[serde(default)]
    pub text_encoder: Option<WeightRef>,
    /// Imagem inicial para img2img (S4 — feat/img2img). `None` = txt2img.
    #[serde(default)]
    pub init_image_ref: Option<InitImageRef>,
    /// Dataset de regularização/controle para treino de difusão. `None` = sem controle.
    /// Mesmo shape do `package_ref` (zip no escopo Packages + md5_zip).
    #[serde(default)]
    pub control_package_ref: Option<PackageRef>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PackageRef {
    pub key: String,
    pub md5_zip: String,
    pub bytes: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportBody {
    pub status: String,
    pub progress: Option<f64>,
    pub epoch: Option<i32>,
    pub step: Option<i32>,
    pub metrics: Option<serde_json::Value>,
    pub error: Option<String>,
    pub artifacts: Option<Vec<ArtifactReport>>,
    /// Conteúdo textual do generation_meta.json (JSONL) — D5 ADR-0023.
    /// Campo opcional retrocompat: ausente em jobs legados.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta_content: Option<String>,
    /// AC-006-A D2: fase/status do job (ex.: "loading_model", "quantizing").
    /// Eventos de status → phase/message; métricas de treino → None, EXCETO
    /// que fase em qualquer linha promove `jobs.phase` (P2-1: métrica com
    /// `phase` carrega a fase junto no report via COALESCE).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// AC-006-A D2: mensagem descritiva da fase (ex.: "Carregando FLUX").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactReport {
    pub kind: String,
    pub path: String,
    pub md5: String,
    pub bytes: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeartbeatBody {
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
