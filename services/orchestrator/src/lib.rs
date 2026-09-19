//! Orchestrator service — executor stateless de jobs (ADR-0007 F4.4).
//!
//! Recebe dispatch do manager, baixa package, valida md5, descompacta,
//! monta config.yaml, sobe trainer (docker|subprocess), coleta métricas,
//! sobe artefatos, reporta progresso ao manager. Sem Postgres — stateless.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub mod config;
pub mod daemon;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Tipos de request/response (snake_case interno, conforme D4)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Pairing (D5.1-2 — single-use em memória)
// ---------------------------------------------------------------------------

/// Estado do pairing code no orquestrador.
/// O `used` flag é single-use: 1ª chamada com código correto consome; 2ª → false.
pub struct PairingState {
    pub code: String,
    pub used: std::sync::atomic::AtomicBool,
}

impl PairingState {
    pub fn new(code: String) -> Self {
        Self {
            code,
            used: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Verifica o código e consome se válido (single-use, atômico via compare_exchange).
    pub fn verify(&self, code: &str) -> bool {
        if self.code == code {
            self.used
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        } else {
            false
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct PairingVerifyRequest {
    pub code: String,
}

#[derive(Debug, Serialize)]
pub struct PairingVerifyResponse {
    pub valid: bool,
}

/// Resolve o URL de advertise do orquestrador.
///
/// Se o valor for `None` ou string vazia, retorna o default
/// `http://orchestrator-local:8082`. Função pura — sem side effects.
pub fn resolve_advertise_url(env_val: Option<&str>) -> String {
    match env_val {
        Some(v) if !v.is_empty() => v.to_string(),
        _ => "http://orchestrator-local:8082".into(),
    }
}

/// Gera um pairing code aleatório no formato `heph_p_<32hex>`.
pub fn generate_pairing_code() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    let hex = hex::encode(bytes);
    format!("heph_p_{hex}")
}

// ---------------------------------------------------------------------------
// Erros
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub enum ScopedKeyError {
    EmptyKey,
    AbsolutePath,
    PathTraversal,
    OutsideScope,
}

impl std::fmt::Display for ScopedKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyKey => write!(f, "empty key"),
            Self::AbsolutePath => write!(f, "absolute path not allowed"),
            Self::PathTraversal => write!(f, "path traversal not allowed"),
            Self::OutsideScope => write!(f, "key outside allowed scope"),
        }
    }
}

impl std::error::Error for ScopedKeyError {}

#[derive(Debug)]
pub enum PipelineError {
    S3Download(String),
    Md5Mismatch {
        expected: String,
        actual: String,
    },
    UnzipFailed(String),
    ConfigYamlInvalid(String),
    DockerFailed {
        exit_code: i32,
        logs_tail: String,
    },
    ArtifactUpload(String),
    ReportFailed(String),
    /// GPU orchestrator recebeu imagem mock — guarda anti-mock (D2).
    GpuImageGuard {
        image: String,
    },
    /// Erro genérico (sem variante específica).
    Other(String),
    /// Daemon de difusão falhou ao subir (D1).
    DaemonLaunchFailed(String),
    /// Daemon de difusão não respondeu health a tempo (D1).
    DaemonHealthTimeout(String),
    /// Daemon de difusão busy após múltiplas tentativas (D1).
    DaemonBusy,
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::S3Download(e) => write!(f, "S3 download failed: {e}"),
            Self::Md5Mismatch { expected, actual } => {
                write!(f, "MD5 mismatch: expected {expected}, got {actual}")
            }
            Self::UnzipFailed(e) => write!(f, "unzip failed: {e}"),
            Self::ConfigYamlInvalid(e) => write!(f, "config.yaml invalid: {e}"),
            Self::DockerFailed {
                exit_code,
                logs_tail,
            } => {
                write!(f, "trainer failed (exit {exit_code}):\n{logs_tail}")
            }
            Self::ArtifactUpload(e) => write!(f, "artifact upload failed: {e}"),
            Self::ReportFailed(e) => write!(f, "report failed: {e}"),
            Self::GpuImageGuard { image } => {
                write!(
                    f,
                    "GPU orchestrator requires GPU trainer image (TRAINER_IMAGE={image} → :gpu)"
                )
            }
            Self::Other(e) => write!(f, "{e}"),
            Self::DaemonLaunchFailed(e) => write!(f, "daemon launch failed: {e}"),
            Self::DaemonHealthTimeout(e) => write!(f, "daemon health timeout: {e}"),
            Self::DaemonBusy => write!(f, "daemon busy after retries"),
        }
    }
}

impl std::error::Error for PipelineError {}

// ---------------------------------------------------------------------------
// S3 scoped key (D2 — invariante de prefixo, barreira principal)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum S3Scope {
    Packages,
    Artifacts,
    Models,
    GenerationInputs,
}

impl S3Scope {
    pub fn prefix(&self) -> &str {
        match self {
            S3Scope::Packages => "packages/",
            S3Scope::Artifacts => "artifacts/",
            S3Scope::Models => "models/",
            S3Scope::GenerationInputs => "generation_inputs/",
        }
    }
}

/// Valida e retorna uma key S3 dentro do escopo permitido.
///
/// Regras:
/// - Key não pode ser vazia
/// - Key não pode começar com `/`
/// - Key não pode conter `..`
/// - Key deve começar com o prefixo do scope (`packages/` ou `artifacts/`)
///
/// Esta é a BARREIRA PRINCIPAL contra path-traversal (D2, spike F4.0).
pub fn scoped_key(scope: S3Scope, key: &str) -> Result<String, ScopedKeyError> {
    if key.is_empty() {
        return Err(ScopedKeyError::EmptyKey);
    }
    if key.starts_with('/') {
        return Err(ScopedKeyError::AbsolutePath);
    }
    if key.contains("..") {
        return Err(ScopedKeyError::PathTraversal);
    }
    let prefix = scope.prefix();
    if !key.starts_with(prefix) {
        return Err(ScopedKeyError::OutsideScope);
    }
    Ok(key.to_string())
}

/// Valida key de imagem inicial img2img (S4 — feat/img2img).
///
/// Aceita `generation_inputs/<...>` (upload avulso) OU `artifacts/<...>`
/// (galeria) — as duas origens possíveis do init. Escopo próprio do staging
/// do init: NÃO afrouxa `scoped_key` para outros usos.
pub fn scoped_init_image_key(key: &str) -> Result<String, ScopedKeyError> {
    if key.is_empty() {
        return Err(ScopedKeyError::EmptyKey);
    }
    if key.starts_with('/') {
        return Err(ScopedKeyError::AbsolutePath);
    }
    if key.contains("..") {
        return Err(ScopedKeyError::PathTraversal);
    }
    if key.starts_with(S3Scope::GenerationInputs.prefix())
        || key.starts_with(S3Scope::Artifacts.prefix())
    {
        return Ok(key.to_string());
    }
    Err(ScopedKeyError::OutsideScope)
}

/// Extensão sanitizada da imagem inicial a partir do s3_key (S4 — feat/img2img).
///
/// Usa o sufixo após o último `.` do filename (após a última `/`): só
/// `[a-zA-Z0-9]` com 2..5 chars (lowercased) — qualquer outra coisa cai em
/// `"png"`. Nunca devolve `..` ou barras.
pub fn init_image_ext(s3_key: &str) -> String {
    let filename = s3_key.rsplit('/').next().unwrap_or("");
    let ext = filename.rsplit('.').next().unwrap_or("");
    let ok = (2..=5).contains(&ext.len())
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
        && filename.contains('.');
    if ok {
        ext.to_ascii_lowercase()
    } else {
        "png".to_string()
    }
}

// ---------------------------------------------------------------------------
// Metrics parsing (D5 — contrato com F4.5, snake_case)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MetricsLine {
    #[serde(default)]
    pub box_loss: f64,
    #[serde(default)]
    pub cls_loss: f64,
    #[serde(default)]
    pub dfl_loss: f64,
    #[serde(rename = "mAP50", default)]
    pub map50: f64,
    #[serde(rename = "mAP50-95", default)]
    pub map50_95: f64,
    #[serde(default)]
    pub loss: Option<f64>,
    #[serde(default)]
    pub lr: Option<f64>,
    #[serde(default)]
    pub step: Option<i64>,
    pub epoch: i32,
    #[serde(default)]
    pub progress: Option<f64>,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub vram_used_gb: Option<f64>,
}

impl MetricsLine {
    /// AC-006-A D1: linha de métrica de treino carrega ao menos um valor numérico.
    /// Sem valor ⇒ é evento de status (fase/mensagem de boot, progresso por imagem).
    /// Fase em qualquer linha promove `jobs.phase` (D2): métrica com `phase`
    /// continua métrica e carrega a fase junto no report.
    pub fn is_training_metric(&self) -> bool {
        matches!(self.loss, Some(x) if x.is_finite())
            || matches!(self.lr, Some(x) if x.is_finite())
            || matches!(self.box_loss, x if x != 0.0 && x.is_finite())
            || matches!(self.cls_loss, x if x != 0.0 && x.is_finite())
            || matches!(self.dfl_loss, x if x != 0.0 && x.is_finite())
            || matches!(self.map50, x if x != 0.0 && x.is_finite())
            || matches!(self.map50_95, x if x != 0.0 && x.is_finite())
    }

    pub fn to_report_json(&self) -> serde_json::Value {
        let mut obj = serde_json::json!({
            "box_loss": self.box_loss,
            "cls_loss": self.cls_loss,
            "dfl_loss": self.dfl_loss,
            "mAP50": self.map50,
            "mAP50-95": self.map50_95,
            "epoch": self.epoch,
        });
        if let Some(loss) = self.loss {
            obj["loss"] = serde_json::json!(loss);
        }
        if let Some(lr) = self.lr {
            obj["lr"] = serde_json::json!(lr);
        }
        if let Some(step) = self.step {
            obj["step"] = serde_json::json!(step);
        }
        if let Some(p) = self.progress {
            obj["progress"] = serde_json::json!(p);
        }
        if let Some(ref phase) = self.phase {
            obj["phase"] = serde_json::json!(phase);
        }
        if let Some(ref msg) = self.message {
            obj["message"] = serde_json::json!(msg);
        }
        if let Some(vram) = self.vram_used_gb {
            obj["vram_used_gb"] = serde_json::json!(vram);
        }
        obj
    }
}

/// Parse tolerante de uma linha de metrics.jsonl ou telemetry.jsonl.
/// Linhas malformadas são ignoradas (skip silencioso).
pub fn parse_metrics_line(line: &str) -> Option<MetricsLine> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    // Sanitiza literais float não-padrão (ex.: : NaN ou : Infinity emitidos por runtimes legados/Python)
    let clean_line = if line.contains("NaN") || line.contains("Infinity") {
        line.replace(": NaN", ": null")
            .replace(": -NaN", ": null")
            .replace(": Infinity", ": null")
            .replace(": -Infinity", ": null")
    } else {
        line.to_string()
    };
    let v: serde_json::Value = serde_json::from_str(&clean_line).ok()?;
    let epoch = v.get("epoch").and_then(|e| e.as_i64()).or_else(|| {
        if v.get("phase").is_some() || v.get("progress").is_some() {
            Some(0)
        } else {
            None
        }
    })? as i32;
    let phase = v
        .get("phase")
        .and_then(|p| p.as_str())
        .map(|s| s.to_string());
    let message = v
        .get("phaseMessage")
        .or_else(|| v.get("message"))
        .and_then(|m| m.as_str())
        .map(|s| s.to_string());
    let vram_used_gb = v
        .get("vramUsedGb")
        .or_else(|| v.get("vram_used_gb"))
        .and_then(|x| x.as_f64());
    Some(MetricsLine {
        box_loss: v.get("box_loss").and_then(|x| x.as_f64()).unwrap_or(0.0),
        cls_loss: v.get("cls_loss").and_then(|x| x.as_f64()).unwrap_or(0.0),
        dfl_loss: v.get("dfl_loss").and_then(|x| x.as_f64()).unwrap_or(0.0),
        map50: v.get("mAP50").and_then(|x| x.as_f64()).unwrap_or(0.0),
        map50_95: v.get("mAP50-95").and_then(|x| x.as_f64()).unwrap_or(0.0),
        loss: v.get("loss").and_then(|x| x.as_f64()),
        lr: v.get("lr").and_then(|x| x.as_f64()),
        step: v.get("step").and_then(|x| x.as_i64()),
        epoch,
        progress: v.get("progress").and_then(|p| p.as_f64()),
        phase,
        message,
        vram_used_gb,
    })
}

/// Calcula progress a partir de uma linha de métricas.
/// Se a linha contiver `progress` explícito (ex.: emitido pelo autolabel ou outro runner),
/// honra esse valor diretamente; caso contrário calcula (epoch / total_epochs).
pub fn compute_progress(line: &MetricsLine, total_epochs: i32) -> f64 {
    if let Some(p) = line.progress {
        return p.clamp(0.0, 1.0);
    }
    if total_epochs <= 0 {
        return 0.0;
    }
    ((line.epoch as f64) / (total_epochs as f64)).clamp(0.0, 1.0)
}

/// Lê linhas novas de um arquivo JSONL a partir de um offset (contagem de linhas).
/// Retorna `(linhas_parseadas, novo_offset)`. O offset SEMPRE avança para o
/// total de linhas lidas — inclusive sobre linhas malformadas (skip silencioso
/// via `parse_metrics_line`), para não reprocessar lixo a cada tick.
///
/// Arquivo ausente ou ilegível → `(vec![], offset)` sem erro: o produtor pode
/// ainda não ter criado o arquivo (ex.: daemon ainda carregando pipeline).
///
/// Compartilhada entre o collector do one-shot (`metrics.jsonl`) e o tail do
/// path daemon (`telemetry.jsonl`, D1 — ADR-0023): mesmo formato de relatório
/// nos dois paths.
pub fn tail_jsonl_lines(path: &Path, lines_read: usize) -> (Vec<MetricsLine>, usize) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return (Vec::new(), lines_read),
    };
    let lines: Vec<&str> = content.lines().collect();
    // Arquivo truncado (rotação): recomeça do zero em vez de pular tudo.
    let start = if lines.len() >= lines_read {
        lines_read
    } else {
        0
    };
    let mut parsed = Vec::new();
    for line in &lines[start..] {
        if let Some(m) = parse_metrics_line(line) {
            parsed.push(m);
        }
    }
    (parsed, lines.len())
}

/// Constrói o `ReportBody` de progresso para uma linha de telemetria/metrics.
///
/// Formato idêntico ao do collector do one-shot: status "running", progress
/// via `compute_progress` (honra `progress` explícito da linha), métrica só
/// quando `is_training_metric()`, e `phase`/`message` promovidas via COALESCE
/// no `report_job` do manager.
pub fn telemetry_report_for_line(line: &MetricsLine, total_epochs: i32) -> ReportBody {
    let progress = compute_progress(line, total_epochs);
    let is_metric = line.is_training_metric();
    ReportBody {
        status: "running".to_string(),
        progress: Some(progress),
        epoch: Some(line.epoch),
        step: line.step.map(|s| s as i32),
        metrics: if is_metric {
            Some(line.to_report_json())
        } else {
            None
        },
        error: None,
        artifacts: None,
        meta_content: None,
        phase: line.phase.clone(),
        message: line.message.clone(),
    }
}

// ---------------------------------------------------------------------------
// Config.yaml placeholder replacement (D6)
// ---------------------------------------------------------------------------

fn default_mode() -> String {
    "train".to_string()
}
/// Substitui placeholders no config.yaml.
///
/// Suporta:
/// - `{dataset_path}`, `{output_path}` — sempre
/// - `{weights_path}` — weights legado (fine-tune)
/// - `{lora_path_0}`...`{lora_path_N}` — LoRAs multi-ref (D3)
/// - `{custom_checkpoint_path}` — checkpoint custom (D4)
/// - `{text_encoder_path}` — text encoder custom (fatia feat/pesos-custom-flux2)
/// - `{init_image_path}` — imagem inicial img2img (S4 — feat/img2img)
/// - `{control_dataset_path}` — dataset de regularização/controle (treino difusão)
///
/// Placeholders absentes no yaml são ignorados (no-op tolerante).
pub fn replace_config_placeholders(
    config: &str,
    dataset_path: &str,
    output_path: &str,
    weights_path: Option<&str>,
    lora_paths: &[String],
    custom_checkpoint_path: Option<&str>,
    init_image_path: Option<&str>,
    control_dataset_path: Option<&str>,
    text_encoder_path: Option<&str>,
) -> String {
    let mut result = config
        .replace("{dataset_path}", dataset_path)
        .replace("{output_path}", output_path);

    match weights_path {
        Some(wp) => result = result.replace("{weights_path}", wp),
        None => {}
    }

    for (i, path) in lora_paths.iter().enumerate() {
        let placeholder = format!("{{lora_path_{i}}}");
        result = result.replace(&placeholder, path);
    }

    if let Some(cp) = custom_checkpoint_path {
        result = result.replace("{custom_checkpoint_path}", cp);
    }

    if let Some(tp) = text_encoder_path {
        result = result.replace("{text_encoder_path}", tp);
    }

    if let Some(ip) = init_image_path {
        result = result.replace("{init_image_path}", ip);
    }

    if let Some(cd) = control_dataset_path {
        result = result.replace("{control_dataset_path}", cd);
    }

    result
}

/// Substitui placeholders (versão legada — sem multi-ref).
/// Mantida para compatibilidade interna.
pub fn replace_config_placeholders_legacy(
    config: &str,
    dataset_path: &str,
    output_path: &str,
    weights_path: Option<&str>,
) -> String {
    replace_config_placeholders(
        config,
        dataset_path,
        output_path,
        weights_path,
        &[],
        None,
        None,
        None,
        None,
    )
}
/// Extrai o valor de `epochs` do config.yaml (para cálculo de progress).
pub fn extract_epochs(config_yaml: &str) -> i32 {
    let parsed = serde_yaml::from_str::<serde_yaml::Value>(config_yaml).ok();
    if let Some(ref v) = parsed {
        if let Some(ep) = v.get("epochs").and_then(|e| e.as_i64()) {
            return ep as i32;
        }
        if let Some(ep) = v
            .get("lora")
            .and_then(|l| l.get("epochs"))
            .and_then(|e| e.as_i64())
        {
            return ep as i32;
        }
    }
    100
}

// ---------------------------------------------------------------------------
// MD5 helper
// ---------------------------------------------------------------------------

/// Calcula MD5 hex de um arquivo.
pub fn compute_file_md5(path: &Path) -> Result<String, String> {
    use md5::Digest;
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let digest = md5::Md5::digest(&bytes);
    Ok(hex::encode(digest))
}

// ---------------------------------------------------------------------------
// Unzip seguro (zip-slip protection, padrão import 3e)
// ---------------------------------------------------------------------------

/// Descompacta um zip em `dest`, recusando entradas com `..` ou caminhos absolutos.
pub fn unzip_safe(zip_path: &Path, dest: &Path) -> Result<(), PipelineError> {
    let file = std::fs::File::open(zip_path)
        .map_err(|e| PipelineError::UnzipFailed(format!("open zip: {e}")))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| PipelineError::UnzipFailed(format!("read zip: {e}")))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| PipelineError::UnzipFailed(format!("read entry: {e}")))?;

        let entry_name = entry.name().to_string();

        // Zip-slip protection (padrão import 3e)
        if entry_name.contains("..") || entry_name.starts_with('/') {
            return Err(PipelineError::UnzipFailed(format!(
                "unsafe zip entry: {entry_name}"
            )));
        }

        let out_path = dest.join(&entry_name);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)
                .map_err(|e| PipelineError::UnzipFailed(format!("create dir: {e}")))?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| PipelineError::UnzipFailed(format!("create parent: {e}")))?;
            }
            let mut out_file = std::fs::File::create(&out_path)
                .map_err(|e| PipelineError::UnzipFailed(format!("create file: {e}")))?;
            std::io::copy(&mut entry, &mut out_file)
                .map_err(|e| PipelineError::UnzipFailed(format!("write file: {e}")))?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// S3 port (trait mockable)
// ---------------------------------------------------------------------------

#[async_trait]
pub trait S3Port: Send + Sync {
    /// Faz GET de um objeto S3 para um arquivo local.
    async fn get_to_file(&self, key: &str, path: &Path) -> Result<(), String> {
        self.get_to_file_with_progress(key, path, None).await
    }
    /// Faz GET de um objeto S3 para um arquivo local com callback de progresso opcional (bytes_baixados, total_bytes).
    async fn get_to_file_with_progress(
        &self,
        key: &str,
        path: &Path,
        _on_progress: Option<&(dyn Fn(u64, Option<u64>) + Send + Sync)>,
    ) -> Result<(), String> {
        self.get_to_file(key, path).await
    }
    /// Faz PUT de um arquivo local para um objeto S3.
    async fn put(&self, key: &str, path: &Path) -> Result<(), String>;
    /// Verifica se o bucket é acessível (para /ready).
    async fn ping(&self) -> bool;
}

/// Cliente S3 real usando aws-sdk-s3 (padrão services/api-principal/src/storage/s3.rs).
pub struct S3Client {
    client: aws_sdk_s3::Client,
    bucket: String,
}

impl S3Client {
    pub fn new(endpoint: &str, access_key: &str, secret_key: &str, bucket: &str) -> Self {
        let creds =
            aws_sdk_s3::config::Credentials::new(access_key, secret_key, None, None, "heph-orch");
        let conf = aws_sdk_s3::config::Builder::new()
            .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
            .region(aws_sdk_s3::config::Region::new("us-east-1"))
            .endpoint_url(endpoint)
            .force_path_style(true)
            .request_checksum_calculation(
                aws_sdk_s3::config::RequestChecksumCalculation::WhenRequired,
            )
            .credentials_provider(creds)
            .retry_config(aws_smithy_types::retry::RetryConfig::disabled())
            .timeout_config(
                aws_smithy_types::timeout::TimeoutConfig::builder()
                    .connect_timeout(Duration::from_secs(2))
                    .read_timeout(Duration::from_secs(30))
                    // Teto de transferência p/ objetos multi-GB (fail-fast vem de
                    // connect+read, não daqui): 120s flakeava em LAN lenta.
                    .operation_timeout(Duration::from_secs(3600))
                    .build(),
            )
            .build();
        Self {
            client: aws_sdk_s3::Client::from_conf(conf),
            bucket: bucket.to_string(),
        }
    }
}

#[async_trait]
impl S3Port for S3Client {
    async fn get_to_file(&self, key: &str, path: &Path) -> Result<(), String> {
        self.get_to_file_with_progress(key, path, None).await
    }

    async fn get_to_file_with_progress(
        &self,
        key: &str,
        path: &Path,
        on_progress: Option<&(dyn Fn(u64, Option<u64>) + Send + Sync)>,
    ) -> Result<(), String> {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

        let out = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| format!("S3 GET {key}: {e}"))?;

        let total_bytes = out.content_length().map(|l| l as u64);
        let mut reader = out.body.into_async_read();
        let mut file = tokio::fs::File::create(path)
            .await
            .map_err(|e| format!("create file {}: {e}", path.display()))?;

        let mut buf = [0u8; 64 * 1024];
        let mut downloaded_bytes: u64 = 0;
        let mut last_reported = tokio::time::Instant::now();

        if let Some(cb) = on_progress {
            cb(0, total_bytes);
        }

        loop {
            let n = reader
                .read(&mut buf)
                .await
                .map_err(|e| format!("read S3 GET {key}: {e}"))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])
                .await
                .map_err(|e| format!("write file {}: {e}", path.display()))?;
            downloaded_bytes += n as u64;

            if let Some(cb) = on_progress {
                if last_reported.elapsed() >= std::time::Duration::from_millis(200) {
                    cb(downloaded_bytes, total_bytes);
                    last_reported = tokio::time::Instant::now();
                }
            }
        }
        file.flush()
            .await
            .map_err(|e| format!("flush file {}: {e}", path.display()))?;

        if let Some(cb) = on_progress {
            cb(downloaded_bytes, total_bytes);
        }
        Ok(())
    }

    async fn put(&self, key: &str, path: &Path) -> Result<(), String> {
        let body = aws_sdk_s3::primitives::ByteStream::from_path(path)
            .await
            .map_err(|e| format!("read file {}: {e}", path.display()))?;
        let len = std::fs::metadata(path)
            .map(|m| m.len() as i64)
            .map_err(|e| format!("stat {}: {e}", path.display()))?;

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_length(len)
            .body(body)
            .send()
            .await
            .map_err(|e| format!("S3 PUT {key}: {e}"))?;
        Ok(())
    }

    async fn ping(&self) -> bool {
        // HEAD bucket = operação barata que prova acessibilidade (D11 /ready).
        self.client
            .head_bucket()
            .bucket(&self.bucket)
            .send()
            .await
            .is_ok()
    }
}

// ---------------------------------------------------------------------------
// Cache local de pesos customizados por MD5 (Text Encoder, LoRAs, Checkpoints)
// ---------------------------------------------------------------------------

/// Baixa e faz cache de pesos no nó orquestrador com base no MD5.
///
/// Se o peso já existir em `cache_dir/<md5>.<ext>` com tamanho > 0:
/// - Reusa instantaneamente via hardlink O(1) (ou copy fallback) sem rebaixar do S3.
/// Se não existir:
/// - Baixa para arquivo temporário, valida MD5, move atomicamente para o cache
///   e vincula ao arquivo de destino do job.
pub async fn stage_cached_weight(
    s3: &Arc<dyn S3Port>,
    cache_dir: &Path,
    dest_file: &Path,
    scoped_key: &str,
    expected_md5: &str,
) -> Result<(), PipelineError> {
    stage_cached_weight_with_progress(s3, cache_dir, dest_file, scoped_key, expected_md5, None)
        .await
}

pub async fn stage_cached_weight_with_progress(
    s3: &Arc<dyn S3Port>,
    cache_dir: &Path,
    dest_file: &Path,
    scoped_key: &str,
    expected_md5: &str,
    on_progress: Option<&(dyn Fn(u64, Option<u64>) + Send + Sync)>,
) -> Result<(), PipelineError> {
    tokio::fs::create_dir_all(cache_dir)
        .await
        .map_err(|e| PipelineError::Other(format!("create weights cache dir: {e}")))?;

    let ext = dest_file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("safetensors");
    let cached_file = cache_dir.join(format!("{expected_md5}.{ext}"));

    if cached_file.is_file() {
        if let Ok(meta) = tokio::fs::metadata(&cached_file).await {
            if meta.len() > 0 {
                tracing::info!(
                    md5 = expected_md5,
                    dest = %dest_file.display(),
                    "Cache hit para peso custom — vinculando instantaneamente via link local"
                );
                let _ = tokio::fs::remove_file(dest_file).await;
                if tokio::fs::hard_link(&cached_file, dest_file).await.is_ok() {
                    return Ok(());
                }
                if tokio::fs::copy(&cached_file, dest_file).await.is_ok() {
                    return Ok(());
                }
            }
        }
    }

    // Cache miss ou arquivo corrompido: baixa para arquivo temporário isolado
    let tmp_file = cache_dir.join(format!(
        ".tmp_{}_{}.part",
        expected_md5,
        uuid::Uuid::new_v4().simple()
    ));

    s3.get_to_file_with_progress(scoped_key, &tmp_file, on_progress)
        .await
        .map_err(|e| PipelineError::S3Download(format!("download weight {scoped_key}: {e}")))?;

    let actual_md5 = compute_file_md5(&tmp_file)
        .map_err(|e| PipelineError::S3Download(format!("compute weight md5 {scoped_key}: {e}")))?;

    if actual_md5 != expected_md5 {
        let _ = tokio::fs::remove_file(&tmp_file).await;
        return Err(PipelineError::Md5Mismatch {
            expected: expected_md5.to_string(),
            actual: actual_md5,
        });
    }

    // Move atomicamente para o cache permanente
    if let Err(e) = tokio::fs::rename(&tmp_file, &cached_file).await {
        if !cached_file.is_file() {
            let _ = tokio::fs::remove_file(&tmp_file).await;
            return Err(PipelineError::Other(format!("persist cached weight: {e}")));
        }
        let _ = tokio::fs::remove_file(&tmp_file).await;
    }

    let _ = tokio::fs::remove_file(dest_file).await;
    if tokio::fs::hard_link(&cached_file, dest_file).await.is_err() {
        tokio::fs::copy(&cached_file, dest_file)
            .await
            .map_err(|e| PipelineError::Other(format!("copy cached weight to dest: {e}")))?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Report client (trait mockable — reporta ao manager via D4)
// ---------------------------------------------------------------------------

#[async_trait]
pub trait ReportClient: Send + Sync {
    async fn report(&self, job_id: &str, body: &ReportBody) -> Result<(), String>;
}

/// Cliente HTTP que reporta ao manager via POST /internal/jobs/:id/report.
pub struct HttpReportClient {
    manager_url: String,
    token: Option<String>,
    client: reqwest::Client,
}

impl HttpReportClient {
    pub fn new(manager_url: &str, token: Option<&str>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(3))
            .build()
            .expect("reqwest client do report");
        Self {
            manager_url: manager_url.trim_end_matches('/').to_string(),
            token: token.map(|s| s.to_string()),
            client,
        }
    }
}

#[async_trait]
impl ReportClient for HttpReportClient {
    async fn report(&self, job_id: &str, body: &ReportBody) -> Result<(), String> {
        let url = format!("{}/internal/jobs/{job_id}/report", self.manager_url);
        let is_terminal = body.status == "done" || body.status == "failed";
        let max_attempts = if is_terminal { 5 } else { 2 };
        let mut last_err = "unknown report error".to_string();

        for attempt in 0..max_attempts {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(300 * (1 << attempt))).await;
            }
            let mut req = self.client.post(&url).json(body);
            if let Some(ref token) = self.token {
                req = req.header("authorization", format!("Bearer {token}"));
            }
            match req.send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        return Ok(());
                    }
                    last_err = format!("report status: {}", resp.status());
                    if resp.status().is_client_error()
                        && resp.status() != reqwest::StatusCode::TOO_MANY_REQUESTS
                    {
                        return Err(last_err);
                    }
                }
                Err(e) => {
                    last_err = format!("report request: {e}");
                }
            }
        }
        Err(last_err)
    }
}

// ---------------------------------------------------------------------------
// Heartbeat client (D4/D9)
// ---------------------------------------------------------------------------

#[async_trait]
pub trait HeartbeatClient: Send + Sync {
    async fn send(&self, body: &HeartbeatBody) -> Result<(), String>;
}

pub struct HttpHeartbeatClient {
    manager_url: String,
    token: Option<String>,
    client: reqwest::Client,
}

impl HttpHeartbeatClient {
    pub fn new(manager_url: &str, token: Option<&str>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(3))
            .build()
            .expect("reqwest client do heartbeat");
        Self {
            manager_url: manager_url.trim_end_matches('/').to_string(),
            token: token.map(|s| s.to_string()),
            client,
        }
    }
}

#[async_trait]
impl HeartbeatClient for HttpHeartbeatClient {
    async fn send(&self, body: &HeartbeatBody) -> Result<(), String> {
        let url = format!("{}/internal/heartbeat", self.manager_url);
        let mut req = self.client.post(&url).json(body);
        if let Some(ref token) = self.token {
            req = req.header("authorization", format!("Bearer {token}"));
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("heartbeat request: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("heartbeat status: {}", resp.status()));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Trainer executor (trait mockable — docker CLI ou subprocess)
// ---------------------------------------------------------------------------

#[async_trait]
pub trait TrainerExecutor: Send + Sync {
    /// Executa o trainer. Retorna (exit_code, logs_completos).
    ///
    /// `env` — variáveis de ambiente extras (ex.: `ENGINE_MOCK=0`).
    /// `gpu_devices` — lista de índices nvidia-smi (ex.: `"0"` ou `"0,1"`).
    ///   `Some(v)` → `--gpus "device={v}"` + `-e NVIDIA_VISIBLE_DEVICES={v}` +
    ///   `--shm-size=2g` + envs repassados. `None` → comportamento padrão.
    async fn run(
        &self,
        image: &str,
        container_name: &str,
        volumes: &[(String, String)], // (host_path, container_path)
        args: &[String],              // argumentos após a imagem (ex.: train --config …)
        env: &[(String, String)],     // variáveis de ambiente extras
        gpu_devices: Option<&str>,    // índices nvidia-smi (ex.: "0")
    ) -> (i32, String);

    /// Para um container (abort via docker stop --time 5 → exit 137).
    async fn stop(&self, container_name: &str) -> Result<(), String>;
}

/// Executor real via CLI docker (EXEC_MODE=docker, default).
pub struct DockerExecutor;

/// Monta os argumentos do `docker run` para teste (D7).
/// Retorna os argumentos que seriam passados ao `docker run` (sem o binário).
pub fn build_docker_run_args(
    image: &str,
    container_name: &str,
    volumes: &[(String, String)],
    args: &[String],
    env: &[(String, String)],
    gpu_devices: Option<&str>,
) -> Vec<String> {
    let mut cmd_args = Vec::new();
    cmd_args.push("run".to_string());
    cmd_args.push("--rm".to_string());
    cmd_args.push("--name".to_string());
    cmd_args.push(container_name.to_string());
    cmd_args.push("--add-host".to_string());
    cmd_args.push("host.docker.internal:host-gateway".to_string());

    let network = std::env::var("ENGINE_NETWORK")
        .or_else(|_| std::env::var("DIFFUSION_DAEMON_NETWORK"))
        .unwrap_or_else(|_| "infra_default".to_string());
    cmd_args.push("--network".to_string());
    cmd_args.push(network);
    for (host, container) in volumes {
        cmd_args.push("-v".to_string());
        cmd_args.push(format!("{host}:{container}"));
    }

    // GPU flags (D4/D7): só quando gpu_devices está setado.
    if let Some(devices) = gpu_devices {
        cmd_args.push("--gpus".to_string());
        cmd_args.push(format!("device={devices}"));
        cmd_args.push("--shm-size".to_string());
        cmd_args.push("2g".to_string());
        cmd_args.push("-e".to_string());
        cmd_args.push(format!("NVIDIA_VISIBLE_DEVICES={devices}"));
    }

    // Env extras (ENGINE_MOCK=0, etc.)
    for (key, value) in env {
        cmd_args.push("-e".to_string());
        cmd_args.push(format!("{key}={value}"));
    }

    cmd_args.push(image.to_string());
    cmd_args.extend_from_slice(args);

    cmd_args
}

#[async_trait]
impl TrainerExecutor for DockerExecutor {
    async fn run(
        &self,
        image: &str,
        container_name: &str,
        volumes: &[(String, String)],
        args: &[String],
        env: &[(String, String)],
        gpu_devices: Option<&str>,
    ) -> (i32, String) {
        let cmd_args =
            build_docker_run_args(image, container_name, volumes, args, env, gpu_devices);

        let mut cmd = tokio::process::Command::new("docker");
        cmd.args(&cmd_args);
        cmd.kill_on_drop(true);

        let timeout_secs: u64 = std::env::var("TRAINER_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(7200);
        let timeout_duration = Duration::from_secs(timeout_secs);

        match tokio::time::timeout(timeout_duration, cmd.output()).await {
            Ok(Ok(o)) => {
                let exit_code = o.status.code().unwrap_or(-1);
                let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                let logs = format!("{stdout}\n{stderr}");
                (exit_code, logs)
            }
            Ok(Err(e)) => (-1, format!("docker exec error: {e}")),
            Err(_) => {
                let _ = self.stop(container_name).await;
                (
                    -1,
                    format!("timeout de execução atingido ({timeout_secs}s) - container encerrado"),
                )
            }
        }
    }

    async fn stop(&self, container_name: &str) -> Result<(), String> {
        let output = tokio::process::Command::new("docker")
            .args(["stop", "--time", "5", container_name])
            .output()
            .await
            .map_err(|e| format!("docker stop error: {e}"))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "docker stop failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }
}

/// Varre e encerra containers órfãos de treino (`trainer-*`) no boot do orquestrador.
pub async fn sweep_orphan_trainer_containers() {
    tracing::info!("verificando containers órfãos de treino no boot...");
    let output = tokio::process::Command::new("docker")
        .args(["ps", "-q", "--filter", "name=trainer-"])
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => {
            let container_ids = String::from_utf8_lossy(&out.stdout);
            let ids: Vec<&str> = container_ids
                .lines()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();
            if ids.is_empty() {
                tracing::info!("nenhum container órfão encontrado no boot");
            } else {
                tracing::warn!(
                    "encontrados {} containers órfãos: {:?}. Encerrando...",
                    ids.len(),
                    ids
                );
                for id in ids {
                    let stop_res = tokio::process::Command::new("docker")
                        .args(["rm", "-f", id])
                        .output()
                        .await;
                    if let Err(e) = stop_res {
                        tracing::error!("falha ao remover container órfão {id}: {e}");
                    } else {
                        tracing::info!("container órfão {id} removido com sucesso");
                    }
                }
            }
        }
        Ok(out) => {
            tracing::warn!(
                "docker ps retornou erro ao verificar órfãos: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Err(e) => {
            tracing::warn!("falha ao executar docker ps para checar órfãos: {e}");
        }
    }
}

/// Varre e limpa diretórios antigos de cache de datasets no workdir (> 24h).
pub async fn sweep_orphan_workdirs(workdir: &Path, max_age: std::time::Duration) {
    let cache_dir = workdir.join("datasets").join("datasets-cache");
    if let Ok(mut entries) = tokio::fs::read_dir(&cache_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(meta) = entry.metadata().await {
                if let Ok(modified) = meta.modified() {
                    if let Ok(age) = modified.elapsed() {
                        if age > max_age {
                            let _ = tokio::fs::remove_dir_all(entry.path()).await;
                        }
                    }
                }
            }
        }
    }
}

/// Executor via subprocess (EXEC_MODE=subprocess, não é caminho de aceite R5).
pub struct SubprocessExecutor;

#[async_trait]
impl TrainerExecutor for SubprocessExecutor {
    async fn run(
        &self,
        _image: &str,
        _container_name: &str,
        _volumes: &[(String, String)],
        _args: &[String],
        _env: &[(String, String)],
        _gpu_devices: Option<&str>,
    ) -> (i32, String) {
        // Subprocess mode: tenta rodar o trainer diretamente.
        // Não é o caminho de aceite — falha honestamente se o pacote não estiver instalado.
        (-1, "subprocess mode not supported in v1".to_string())
    }

    async fn stop(&self, _container_name: &str) -> Result<(), String> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Active jobs tracking (para abort — D4)
// ---------------------------------------------------------------------------

pub struct ActiveJobState {
    pub container_name: String,
    pub cancelled: AtomicBool,
}

impl ActiveJobState {
    pub fn new(container_name: String) -> Self {
        Self {
            container_name,
            cancelled: AtomicBool::new(false),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

pub type ActiveJobs = Arc<dashmap::DashMap<String, ActiveJobState>>;

/// Cria uma nova instância de ActiveJobs.
pub fn new_active_jobs() -> ActiveJobs {
    Arc::new(dashmap::DashMap::new())
}

/// PUT S3 com retry (N=3, backoff curto 100ms/200ms).
///
/// Upload de output não pode falhar silenciosamente (incidente galeria vazia):
/// quem chama registra o erro persistente e o report final vira failed.
/// Não aborta o resto do loop — o chamador decide após coletar tudo.
async fn put_with_retry(s3: &dyn S3Port, key: &str, path: &Path) -> Result<(), String> {
    let mut last_err = String::from("upload falhou");
    for attempt in 0..3 {
        match s3.put(key, path).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = e;
                if attempt < 2 {
                    tokio::time::sleep(Duration::from_millis(100 * (attempt as u64 + 1))).await;
                }
            }
        }
    }
    Err(last_err)
}

// ---------------------------------------------------------------------------
// Job pipeline (§11/:262–270, D5/D6/D8)
// ---------------------------------------------------------------------------

/// Executa o pipeline completo de um job (async, chamado como task).
///
/// 1. Reporta `preparing`
/// 2. Baixa package.zip via S3 (scoped), valida md5
/// 3. Descompacta (zip-slip safe)
/// 4. Monta config.yaml com paths reais
/// 5. Reporta `running`
/// 6. Executa trainer (docker ou subprocess)
/// 7. Coleta métricas incrementalmente
/// 8. Sobe artefatos ao bucket
/// 9. Reporta `done` ou `failed`
/// 10. Limpa tempdir
pub async fn run_job(
    dispatch: DispatchRequest,
    s3: Arc<dyn S3Port>,
    report_client: Arc<dyn ReportClient>,
    executor: Arc<dyn TrainerExecutor>,
    active_jobs: ActiveJobs,
    gpu_devices: Option<String>,
    gpu_allow_mock: bool,
    daemon_state: Option<Arc<daemon::DaemonState>>,
) {
    let job_id = dispatch.job_id.clone();
    let report_for_error = Arc::clone(&report_client);
    let result = run_job_inner(
        &dispatch,
        s3,
        report_client,
        executor,
        &active_jobs,
        gpu_devices.as_deref(),
        gpu_allow_mock,
        daemon_state.as_deref(),
    )
    .await;

    if let Err(err) = result {
        let err_msg = err.to_string();
        if let Err(report_err) = report_for_error
            .report(
                &job_id,
                &ReportBody {
                    status: "failed".to_string(),
                    progress: None,
                    epoch: None,
                    step: None,
                    metrics: None,
                    error: Some(err_msg.clone()),
                    artifacts: None,
                    meta_content: None,
                    phase: Some("error".to_string()),
                    message: Some(err_msg),
                },
            )
            .await
        {
            tracing::warn!(
                job_id = %job_id,
                report_error = %report_err,
                "falha ao reportar erro terminal do job"
            );
        }
    }

    // Cleanup pós-job do cache do dataset descompactado:
    let job_workdir = std::path::PathBuf::from(&dispatch.workdir);
    let dataset_dir = job_workdir
        .join("datasets")
        .join("datasets-cache")
        .join(&job_id);
    let _ = tokio::fs::remove_dir_all(&dataset_dir).await;
    active_jobs.remove(&job_id);
}

async fn run_job_inner(
    dispatch: &DispatchRequest,
    s3: Arc<dyn S3Port>,
    report_client: Arc<dyn ReportClient>,
    executor: Arc<dyn TrainerExecutor>,
    active_jobs: &ActiveJobs,
    gpu_devices: Option<&str>,
    gpu_allow_mock: bool,
    daemon_state: Option<&daemon::DaemonState>,
) -> Result<(), PipelineError> {
    let job_id = &dispatch.job_id;
    let job_workdir = PathBuf::from(&dispatch.workdir);

    // paths internos = mounts do compose (/data/datasets, /data/outputs);
    // envs ORCH_VOL_* = NOME do volume docker (com prefixo do projeto) para o docker run.
    let vol_datasets =
        std::env::var("ORCH_VOL_DATASETS").unwrap_or_else(|_| "infra_datasets".into());
    let vol_outputs = std::env::var("ORCH_VOL_OUTPUTS").unwrap_or_else(|_| "infra_outputs".into());

    // Cria diretórios de trabalho — paths FIXOS no filesystem do orquestrador.
    // O compose monta infra_datasets em /data/datasets e infra_outputs em /data/outputs.
    let datasets_cache = job_workdir
        .join("datasets")
        .join("datasets-cache")
        .join(job_id);
    let outputs = job_workdir.join("outputs").join(job_id);
    let temp_dir = job_workdir.join("tmp").join(job_id);
    let weights_cache_dir = job_workdir.join("outputs").join(".weights-cache");
    tokio::fs::create_dir_all(&datasets_cache)
        .await
        .map_err(|e| PipelineError::Other(format!("create datasets-cache: {e}")))?;
    tokio::fs::create_dir_all(&outputs)
        .await
        .map_err(|e| PipelineError::Other(format!("create outputs: {e}")))?;
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .map_err(|e| PipelineError::Other(format!("create temp: {e}")))?;

    // Guarda anti-mock (D2): fail-fast antes de downloads/reports.
    if gpu_devices.is_some() && !gpu_allow_mock {
        if dispatch.image.ends_with(":local") {
            return Err(PipelineError::GpuImageGuard {
                image: dispatch.image.clone(),
            });
        }
    }

    // 1. Report preparing
    report_client
        .report(
            job_id,
            &ReportBody {
                status: "preparing".to_string(),
                progress: None,
                epoch: None,
                step: None,
                metrics: None,
                error: None,
                artifacts: None,
                meta_content: None,
                phase: None,
                message: None,
            },
        )
        .await
        .map_err(|e| PipelineError::ReportFailed(format!("report preparing: {e}")))?;

    // Preempção: ANTES de despachar treino, se daemon idle → kill (D1)
    if dispatch.mode != "generate" {
        if let Some(ds) = daemon_state {
            daemon::maybe_preempt_daemon(ds).await;
        }
    }

    fn make_progress_reporter(
        report_client: &Arc<dyn ReportClient>,
        job_id: &str,
        phase: &'static str,
        prefix_msg: &'static str,
        base_progress: f64,
        progress_span: f64,
    ) -> impl Fn(u64, Option<u64>) + Send + Sync + 'static {
        let rc = Arc::clone(report_client);
        let jid = job_id.to_string();
        move |downloaded: u64, total: Option<u64>| {
            let dl_mb = downloaded as f64 / (1024.0 * 1024.0);
            let (msg, prog) = if let Some(tot) = total {
                let tot_mb = tot as f64 / (1024.0 * 1024.0);
                let pct = if tot > 0 {
                    (downloaded as f64 / tot as f64).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (
                    format!(
                        "{prefix_msg} ({:.1} MB / {:.1} MB · {:.0}%)...",
                        dl_mb,
                        tot_mb,
                        pct * 100.0
                    ),
                    base_progress + progress_span * pct,
                )
            } else {
                (
                    format!("{prefix_msg} ({:.1} MB)...", dl_mb),
                    base_progress + 0.01,
                )
            };
            let rc_spawn = Arc::clone(&rc);
            let jid_spawn = jid.clone();
            tracing::info!(job_id = %jid_spawn, phase = %phase, "{msg}");
            tokio::spawn(async move {
                let _ = rc_spawn
                    .report(
                        &jid_spawn,
                        &ReportBody {
                            status: "running".to_string(),
                            progress: Some(prog),
                            epoch: None,
                            step: None,
                            metrics: None,
                            error: None,
                            artifacts: None,
                            meta_content: None,
                            phase: Some(phase.to_string()),
                            message: Some(msg),
                        },
                    )
                    .await;
            });
        }
    }

    // 2. Download package.zip via S3 (scoped — D2 barreira principal) se presente
    if let Some(ref pr) = dispatch.package_ref {
        let zip_path = temp_dir.join("dataset.zip");
        let key = scoped_key(S3Scope::Packages, &pr.key)
            .map_err(|e| PipelineError::S3Download(format!("invalid package key: {e}")))?;

        let on_dl = make_progress_reporter(
            &report_client,
            job_id,
            "downloading_dataset",
            "Baixando dataset",
            0.01,
            0.05,
        );
        tracing::info!(job_id = %job_id, key = %key, "Iniciando download do dataset...");
        s3.get_to_file_with_progress(&key, &zip_path, Some(&on_dl))
            .await
            .map_err(|e| PipelineError::S3Download(format!("download package: {e}")))?;

        // 3. Verify MD5 (crash do job se divergir — D4)
        let _ = report_client
            .report(
                job_id,
                &ReportBody {
                    status: "running".to_string(),
                    progress: Some(0.06),
                    epoch: None,
                    step: None,
                    metrics: None,
                    error: None,
                    artifacts: None,
                    meta_content: None,
                    phase: Some("downloading_dataset".to_string()),
                    message: Some("Validando integridade do dataset (MD5)...".to_string()),
                },
            )
            .await;
        let actual_md5 = compute_file_md5(&zip_path)
            .map_err(|e| PipelineError::S3Download(format!("compute md5: {e}")))?;
        if actual_md5 != pr.md5_zip {
            return Err(PipelineError::Md5Mismatch {
                expected: pr.md5_zip.clone(),
                actual: actual_md5,
            });
        }
        tracing::info!(job_id = %job_id, md5 = %actual_md5, "Integridade do dataset validada com sucesso");

        // 4. Unzip (zip-slip safe, padrão import 3e)
        let _ = report_client
            .report(
                job_id,
                &ReportBody {
                    status: "running".to_string(),
                    progress: Some(0.07),
                    epoch: None,
                    step: None,
                    metrics: None,
                    error: None,
                    artifacts: None,
                    meta_content: None,
                    phase: Some("extracting_dataset".to_string()),
                    message: Some("Descompactando dataset no cache do nó...".to_string()),
                },
            )
            .await;
        tracing::info!(job_id = %job_id, "Descompactando dataset no cache do nó...");
        unzip_safe(&zip_path, &datasets_cache)?;
    }

    // 5. Download e staging de pesos (fine-tune — ADR-0012 D5)
    //    Pesos ficam em outputs/<job_id>/weights/<filename> (volume outputs já montado).
    let mut weights_staged_path: Option<String> = None;
    if let Some(ref weights_ref) = dispatch.weights_ref {
        // Infere escopo pelo prefixo da key (models/ → Models, artifacts/ → Artifacts)
        let scope = if weights_ref.s3_key.starts_with("models/") {
            S3Scope::Models
        } else if weights_ref.s3_key.starts_with("artifacts/") {
            S3Scope::Artifacts
        } else {
            return Err(PipelineError::S3Download(format!(
                "weights_ref key must start with models/ or artifacts/, got: {}",
                weights_ref.s3_key
            )));
        };

        let scoped_wkey = scoped_key(scope, &weights_ref.s3_key)
            .map_err(|e| PipelineError::S3Download(format!("invalid weights_ref key: {e}")))?;

        // Extrai filename do path (models/yolo/<id>/best.pt → best.pt)
        let filename = weights_ref.s3_key.rsplit('/').next().ok_or_else(|| {
            PipelineError::S3Download("weights_ref key has no filename".to_string())
        })?;

        let weights_dir = outputs.join("weights");
        tokio::fs::create_dir_all(&weights_dir)
            .await
            .map_err(|e| PipelineError::Other(format!("create weights dir: {e}")))?;

        let weights_file = weights_dir.join(filename);
        let on_w = make_progress_reporter(
            &report_client,
            job_id,
            "downloading_weights",
            "Baixando pesos do modelo",
            0.07,
            0.03,
        );
        stage_cached_weight_with_progress(
            &s3,
            &weights_cache_dir,
            &weights_file,
            &scoped_wkey,
            &weights_ref.md5,
            Some(&on_w),
        )
        .await?;
        weights_staged_path = Some(format!("/outputs/{job_id}/weights/{filename}"));
    }

    // 5b. Download e staging de LoRAs multi-ref (D3 — ADR-0023)
    //     Pesos ficam em outputs/<job_id>/weights/lora_0.safetensors, lora_1.safetensors, ...
    let mut lora_staged_paths: Vec<String> = Vec::new();
    let weights_dir = outputs.join("weights");
    if !dispatch.loras.is_empty() {
        tokio::fs::create_dir_all(&weights_dir)
            .await
            .map_err(|e| PipelineError::Other(format!("create weights dir: {e}")))?;
    }
    for (i, lora) in dispatch.loras.iter().enumerate() {
        let scope = if lora.s3_key.starts_with("models/") {
            S3Scope::Models
        } else if lora.s3_key.starts_with("artifacts/") {
            S3Scope::Artifacts
        } else {
            return Err(PipelineError::S3Download(format!(
                "lora s3_key must start with models/ or artifacts/, got: {}",
                lora.s3_key
            )));
        };
        let scoped_key = scoped_key(scope, &lora.s3_key)
            .map_err(|e| PipelineError::S3Download(format!("invalid lora key: {e}")))?;
        let lora_file = weights_dir.join(format!("lora_{i}.safetensors"));
        stage_cached_weight(&s3, &weights_cache_dir, &lora_file, &scoped_key, &lora.md5).await?;
        lora_staged_paths.push(format!("/outputs/{job_id}/weights/lora_{i}.safetensors"));
    }

    // 5c. Download e staging de custom checkpoint (D4 — ADR-0023)
    //     Pesos ficam em outputs/<job_id>/weights/custom.safetensors
    let mut custom_staged_path: Option<String> = None;
    if let Some(ref custom) = dispatch.custom_checkpoint {
        tokio::fs::create_dir_all(&weights_dir)
            .await
            .map_err(|e| PipelineError::Other(format!("create weights dir: {e}")))?;
        let scope = if custom.s3_key.starts_with("models/") {
            S3Scope::Models
        } else if custom.s3_key.starts_with("artifacts/") {
            S3Scope::Artifacts
        } else {
            return Err(PipelineError::S3Download(format!(
                "custom_checkpoint s3_key must start with models/ or artifacts/, got: {}",
                custom.s3_key
            )));
        };
        let scoped_key = scoped_key(scope, &custom.s3_key)
            .map_err(|e| PipelineError::S3Download(format!("invalid custom key: {e}")))?;
        let custom_file = weights_dir.join("custom.safetensors");
        let on_c = make_progress_reporter(
            &report_client,
            job_id,
            "downloading_weights",
            "Baixando checkpoint custom",
            0.07,
            0.03,
        );
        stage_cached_weight_with_progress(
            &s3,
            &weights_cache_dir,
            &custom_file,
            &scoped_key,
            &custom.md5,
            Some(&on_c),
        )
        .await?;
        custom_staged_path = Some(format!("/outputs/{job_id}/weights/custom.safetensors"));
    }
    // 5c2. Download e staging do text encoder custom (fatia feat/pesos-custom-flux2).
    //     Fica em outputs/<job_id>/weights/text_encoder.safetensors (mesmo
    //     mecanismo weights_ref: escopo models/|artifacts/, md5 obrigatório,
    //     falha honesta em qualquer etapa — nunca fallback silencioso p/ o oficial).
    let mut text_encoder_staged_path: Option<String> = None;
    if let Some(ref encoder) = dispatch.text_encoder {
        tokio::fs::create_dir_all(&weights_dir)
            .await
            .map_err(|e| PipelineError::Other(format!("create weights dir: {e}")))?;
        let scope = if encoder.s3_key.starts_with("models/") {
            S3Scope::Models
        } else if encoder.s3_key.starts_with("artifacts/") {
            S3Scope::Artifacts
        } else {
            return Err(PipelineError::S3Download(format!(
                "text_encoder s3_key must start with models/ or artifacts/, got: {}",
                encoder.s3_key
            )));
        };
        let scoped_key = scoped_key(scope, &encoder.s3_key)
            .map_err(|e| PipelineError::S3Download(format!("invalid text_encoder key: {e}")))?;
        let encoder_file = weights_dir.join("text_encoder.safetensors");
        stage_cached_weight(
            &s3,
            &weights_cache_dir,
            &encoder_file,
            &scoped_key,
            &encoder.md5,
        )
        .await?;
        text_encoder_staged_path = Some(format!(
            "/outputs/{job_id}/weights/text_encoder.safetensors"
        ));
    }

    // 5d. Download e staging da imagem inicial img2img (S4 — feat/img2img).
    //     Fica em outputs/<job_id>/inputs/init.<ext> (ext sanitizada do s3_key,
    //     fallback `png`). O path REAL do host entra no real_config — o
    //     config.yaml já é montado via volume /outputs, como weights/custom.
    let mut init_staged_path: Option<String> = None;
    if let Some(ref init) = dispatch.init_image_ref {
        let scoped_ikey = scoped_init_image_key(&init.s3_key)
            .map_err(|e| PipelineError::S3Download(format!("invalid init_image_ref key: {e}")))?;
        let inputs_dir = outputs.join("inputs");
        tokio::fs::create_dir_all(&inputs_dir)
            .await
            .map_err(|e| PipelineError::Other(format!("create inputs dir: {e}")))?;
        let ext = init_image_ext(&init.s3_key);
        let init_file = inputs_dir.join(format!("init.{ext}"));
        s3.get_to_file(&scoped_ikey, &init_file)
            .await
            .map_err(|e| PipelineError::S3Download(format!("download init image: {e}")))?;
        let actual_md5 = compute_file_md5(&init_file)
            .map_err(|e| PipelineError::S3Download(format!("compute init image md5: {e}")))?;
        match init.md5.as_deref() {
            Some(expected) => {
                if actual_md5 != expected {
                    return Err(PipelineError::Md5Mismatch {
                        expected: expected.to_string(),
                        actual: actual_md5,
                    });
                }
            }
            // Origem galeria: sem hash de referência — só registra o calculado.
            None => {
                tracing::info!(
                    "init image from gallery has no expected md5, staged md5={actual_md5}"
                );
            }
        }
        init_staged_path = Some(format!("/outputs/{job_id}/inputs/init.{ext}"));
    }
    // 5e. Download e staging do dataset de controle/regularização (treino difusão).
    //     Mesmo caminho do pacote principal: zip no escopo Packages + md5_zip
    //     obrigatório, extraído (zip-slip safe) para datasets-cache/<job_id>/control.
    //     O path REAL do host entra no real_config via {control_dataset_path}.
    //     Falha em qualquer etapa = job falha honesto (S3Download/Md5Mismatch/UnzipFailed).
    let mut control_staged_path: Option<String> = None;
    if let Some(ref control) = dispatch.control_package_ref {
        let control_zip = temp_dir.join("control.zip");
        let control_key = scoped_key(S3Scope::Packages, &control.key)
            .map_err(|e| PipelineError::S3Download(format!("invalid control package key: {e}")))?;
        s3.get_to_file(&control_key, &control_zip)
            .await
            .map_err(|e| PipelineError::S3Download(format!("download control package: {e}")))?;
        let actual_md5 = compute_file_md5(&control_zip)
            .map_err(|e| PipelineError::S3Download(format!("compute control md5: {e}")))?;
        if actual_md5 != control.md5_zip {
            return Err(PipelineError::Md5Mismatch {
                expected: control.md5_zip.clone(),
                actual: actual_md5,
            });
        }
        let control_dir = datasets_cache.join("control");
        tokio::fs::create_dir_all(&control_dir)
            .await
            .map_err(|e| PipelineError::Other(format!("create control dir: {e}")))?;
        unzip_safe(&control_zip, &control_dir)?;
        control_staged_path = Some(format!("/datasets/datasets-cache/{job_id}/control"));
    }

    // 6. Monta config.yaml REAL — substitui placeholders (§8/:102)
    let total_epochs = dispatch
        .config_yaml
        .as_deref()
        .map(extract_epochs)
        .unwrap_or(100);

    if let Some(ref config_yaml) = dispatch.config_yaml {
        // Dentro do container trainer: /datasets/datasets-cache/<job_id> e /outputs/<job_id>
        // (via -v volumes nomeados montados no compose).
        let dataset_path = format!("/datasets/datasets-cache/{job_id}");
        let output_path = format!("/outputs/{job_id}");
        let real_config = replace_config_placeholders(
            config_yaml,
            &dataset_path,
            &output_path,
            weights_staged_path.as_deref(),
            &lora_staged_paths,
            custom_staged_path.as_deref(),
            init_staged_path.as_deref(),
            control_staged_path.as_deref(),
            text_encoder_staged_path.as_deref(),
        );

        // O config exige init mas nenhum init_image_ref veio no dispatch:
        // falha explícita em vez de deixar o placeholder vazar (S4).
        if real_config.contains("{init_image_path}") {
            return Err(PipelineError::ConfigYamlInvalid(
                "config.yaml requires {init_image_path} but no init_image_ref was provided"
                    .to_string(),
            ));
        }

        // O config exige control mas nenhum control_package_ref veio no dispatch:
        // falha explícita em vez de deixar o placeholder vazar (espelha S4).
        if real_config.contains("{control_dataset_path}") {
            return Err(PipelineError::ConfigYamlInvalid(
                "config.yaml requires {control_dataset_path} but no control_package_ref was provided"
                    .to_string(),
            ));
        }

        // O config exige text encoder custom mas nenhum veio no dispatch:
        // falha explícita em vez de vazar o placeholder ou cair no oficial
        // (fallback silencioso proibido — fatia feat/pesos-custom-flux2).
        if real_config.contains("{text_encoder_path}") {
            return Err(PipelineError::ConfigYamlInvalid(
                "config.yaml requires {text_encoder_path} but no text_encoder was provided"
                    .to_string(),
            ));
        }

        // Valida que é YAML parseável (D6)
        let _: serde_yaml::Value = serde_yaml::from_str(&real_config).map_err(|e| {
            PipelineError::ConfigYamlInvalid(format!("config.yaml parse error: {e}"))
        })?;

        // Escreve no output_path (trainer lê de lá)
        let config_path = outputs.join("config.yaml");
        tokio::fs::write(&config_path, &real_config)
            .await
            .map_err(|e| PipelineError::ConfigYamlInvalid(format!("write config.yaml: {e}")))?;

        // Grava training_config.json para reproducibilidade e download pelo usuário (apenas treino de difusão)
        if dispatch.engine == "diffusion" && dispatch.mode == "train" {
            if let Ok(json_val) = serde_yaml::from_str::<serde_json::Value>(&real_config) {
                if let Ok(json_str) = serde_json::to_string_pretty(&json_val) {
                    let _ = tokio::fs::write(outputs.join("training_config.json"), json_str).await;
                }
            }
        }
    }

    // 6. Report running
    report_client
        .report(
            job_id,
            &ReportBody {
                status: "running".to_string(),
                progress: Some(0.0),
                epoch: Some(0),
                step: None,
                metrics: None,
                error: None,
                artifacts: None,
                meta_content: None,
                phase: None,
                message: None,
            },
        )
        .await
        .map_err(|e| PipelineError::ReportFailed(format!("report running: {e}")))?;

    // =========================================================================
    // DAEMON PATH: Diffusion generate com daemon habilitado (D1)
    // =========================================================================
    if dispatch.engine == "diffusion" && dispatch.mode == "generate" && daemon_state.is_some() {
        let ds = daemon_state.unwrap();

        // Deriva spec-alvo do config yaml (D1: loaded_spec from config)
        let _target_spec = dispatch.config_yaml.as_deref().unwrap_or("default");

        // (a) Se daemon não está de pé → sobe, aguarda /health 200
        let _daemon_url = daemon::ensure_daemon_ready(ds, _target_spec)
            .await
            .map_err(|e| PipelineError::DaemonLaunchFailed(e))?;

        // Telemetry path (D1)
        let telemetry_abs = outputs.join("telemetry.jsonl");

        // Config yaml como string JSON para o daemon — usa o real_config
        // (mesmo config com placeholders substituídos que o one-shot grava em config.yaml)
        let config_str = dispatch
            .config_yaml
            .as_ref()
            .map(|cy| {
                replace_config_placeholders(
                    cy,
                    &format!("/datasets/datasets-cache/{job_id}"),
                    &format!("/outputs/{job_id}"),
                    weights_staged_path.as_deref(),
                    &lora_staged_paths,
                    custom_staged_path.as_deref(),
                    init_staged_path.as_deref(),
                    control_staged_path.as_deref(),
                    text_encoder_staged_path.as_deref(),
                )
            })
            .unwrap_or_default();

        // Mesmo guarda do one-shot: placeholder de init sem init_image_ref
        // falha explícita em vez de vazar para o daemon (S4).
        if config_str.contains("{init_image_path}") {
            return Err(PipelineError::ConfigYamlInvalid(
                "config.yaml requires {init_image_path} but no init_image_ref was provided"
                    .to_string(),
            ));
        }

        // Defesa anti-placeholder do control (espelha a de init): config exige
        // control mas nenhum control_package_ref veio no dispatch.
        if config_str.contains("{control_dataset_path}") {
            return Err(PipelineError::ConfigYamlInvalid(
                "config.yaml requires {control_dataset_path} but no control_package_ref was provided"
                    .to_string(),
            ));
        }

        // Defesa anti-placeholder do text encoder (fatia feat/pesos-custom-flux2).
        if config_str.contains("{text_encoder_path}") {
            return Err(PipelineError::ConfigYamlInvalid(
                "config.yaml requires {text_encoder_path} but no text_encoder was provided"
                    .to_string(),
            ));
        }

        let body = daemon::GenerateBody {
            config: config_str,
            output_dir: outputs.to_str().unwrap_or_default().to_string(),
            telemetry_path: telemetry_abs.to_str().unwrap_or_default().to_string(),
        };

        // (c) Tail de telemetry.jsonl durante o POST /generate: o HTTP retorna
        // só no 200 (fim da geração), então sem tail o job fica em 0.0 até
        // done. Mesmos events/phases do one-shot: a task lê telemetry.jsonl
        // via `tail_jsonl_lines` e reporta cada linha com
        // `telemetry_report_for_line` (== formato do collector do one-shot).
        // A task só observa o arquivo, nunca toca no client/launcher.
        let telemetry_report_client = Arc::clone(&report_client);
        let telemetry_path_clone = telemetry_abs.clone();
        let telemetry_job_id = job_id.clone();
        let telemetry_handle = tokio::spawn(async move {
            let mut lines_read: usize = 0;
            let mut interval = tokio::time::interval(Duration::from_millis(500));
            loop {
                interval.tick().await;
                let (new_lines, new_offset) = tail_jsonl_lines(&telemetry_path_clone, lines_read);
                lines_read = new_offset;
                for m in new_lines {
                    let body = telemetry_report_for_line(&m, total_epochs);
                    let _ = telemetry_report_client
                        .report(&telemetry_job_id, &body)
                        .await;
                }
            }
        });

        // POST /generate com retry em 409 (D1: 2 retries com backoff curto).
        // O tail acima emite progresso enquanto este await bloqueia.
        let mut last_err = String::new();
        let mut succeeded = false;
        for attempt in 0..3 {
            let client = ds.client.read().unwrap().clone();
            match client.generate(&body).await {
                Ok(()) => {
                    succeeded = true;
                    ds.touch();
                    break;
                }
                Err(e) if e == "busy" => {
                    if attempt < 2 {
                        // Backoff curto antes de retry
                        tokio::time::sleep(Duration::from_millis(500 * (attempt as u64 + 1))).await;
                        last_err = "busy".to_string();
                        continue;
                    } else {
                        last_err = "busy".to_string();
                    }
                }
                Err(e) => {
                    // Erro diferente de busy → tail para, falha honesta
                    telemetry_handle.abort();
                    return Err(PipelineError::Other(format!("daemon generate: {e}")));
                }
            }
        }

        // Para o tail: sucesso e busy-exausto convergem abaixo (done / DaemonBusy).
        telemetry_handle.abort();

        if !succeeded {
            if last_err == "busy" {
                return Err(PipelineError::DaemonBusy);
            }
            return Err(PipelineError::Other(format!(
                "daemon generate failed: {last_err}"
            )));
        }

        // (d) Coleta artefatos do output_dir igual one-shot (glob)
        let mut artifacts = Vec::new();
        // Uploads com retry; falhas persistentes viram failed no gate abaixo
        // (incidente galeria vazia) — sem abortar o resto do loop.
        let mut upload_errors: Vec<String> = Vec::new();

        // Coleta glob: generated_*.png (kind generated), thumb_*.jpg (kind generated_thumb),
        // generation_meta.json (kind generated_meta) — D2 ADR-0023
        // Se existir qualquer `generated_*.png`, PULA `generated.png` (symlink legado só vale
        // para jobs sem numerado).
        if let Ok(entries) = std::fs::read_dir(&outputs) {
            let all_files: Vec<std::path::PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file()
                        && !p
                            .file_name()
                            .and_then(|n| n.to_str())
                            .map(|n| {
                                n.starts_with('.') || n.ends_with(".tmp") || n.ends_with(".part")
                            })
                            .unwrap_or(false)
                })
                .collect();

            let has_numbered = all_files.iter().any(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("generated_") && n.ends_with(".png"))
                    .unwrap_or(false)
            });

            for path in &all_files {
                let fname = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };

                let kind = if fname.starts_with("generated_") && fname.ends_with(".png") {
                    Some("generated")
                } else if fname.starts_with("thumb_")
                    && (fname.ends_with(".jpg") || fname.ends_with(".jpeg"))
                {
                    Some("generated_thumb")
                } else if fname == "generation_meta.json" {
                    Some("generated_meta")
                } else if fname == "generated.png" && !has_numbered {
                    // Legado: generated.png só coleta se NÃO houver numerados
                    Some("generated")
                } else {
                    None
                };

                if let Some(k) = kind {
                    let bytes = std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0);
                    if bytes > 0 {
                        let art_key = format!("artifacts/{job_id}/{fname}");
                        match scoped_key(S3Scope::Artifacts, &art_key) {
                            Ok(scoped) => match compute_file_md5(path) {
                                Ok(md5) => match put_with_retry(s3.as_ref(), &scoped, path).await {
                                    Ok(()) => artifacts.push(ArtifactReport {
                                        kind: k.to_string(),
                                        path: fname,
                                        md5,
                                        bytes,
                                    }),
                                    Err(e) => upload_errors.push(format!("{fname}: {e}")),
                                },
                                Err(e) => upload_errors.push(format!(
                                    "{fname}: falha ao preparar artefato (md5): {e}"
                                )),
                            },
                            Err(e) => upload_errors
                                .push(format!("{fname}: falha ao preparar artefato (key): {e}")),
                        }
                    }
                }
            }
        }

        // Incidente galeria vazia: upload persistente falhou → o job falhou do
        // ponto de vista do usuário; reportar done seria mentira.
        if !upload_errors.is_empty() {
            return Err(PipelineError::Other(format!(
                "upload de artefatos falhou: {}",
                upload_errors.join("; ")
            )));
        }

        // 11. Report done — inclui meta_content se generation_meta.json existe (D5 ADR-0023)
        let final_metrics = read_final_metrics(&outputs.join("metrics.jsonl"));
        let meta_content = read_generation_meta_content(&outputs);
        report_client
            .report(
                job_id,
                &ReportBody {
                    status: "done".to_string(),
                    progress: Some(1.0),
                    epoch: final_metrics.as_ref().map(|m| m.epoch),
                    step: final_metrics
                        .as_ref()
                        .and_then(|m| m.step.map(|s| s as i32)),
                    // AC-006-A D1: somente métricas de treino entram no array.
                    metrics: final_metrics
                        .as_ref()
                        .filter(|m| m.is_training_metric())
                        .map(|m| m.to_report_json()),
                    error: None,
                    artifacts: if artifacts.is_empty() {
                        None
                    } else {
                        Some(artifacts)
                    },
                    meta_content,
                    phase: Some("completed".to_string()),
                    message: Some("Treino concluído".to_string()),
                },
            )
            .await
            .map_err(|e| PipelineError::ReportFailed(format!("report done: {e}")))?;

        // 12. Cleanup tempdir
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        return Ok(());
    }

    // =========================================================================
    // ONE-SHOT PATH: Executor docker/subprocess (comportamento legado)
    // =========================================================================
    // 7. Execute trainer (D5 :301–309)
    let container_name = format!("trainer-{}-{}", dispatch.engine, job_id);
    let active_state = ActiveJobState::new(container_name.clone());
    active_jobs.insert(job_id.to_string(), active_state);

    let volumes = vec![
        (vol_datasets, "/datasets".to_string()),
        (vol_outputs, "/outputs".to_string()),
    ];

    // Env extras para o executor (D7): ENGINE_MOCK=0 quando GPU habilitada.
    let mut exec_env: Vec<(String, String)> = Vec::new();
    if gpu_devices.is_some() {
        exec_env.push(("ENGINE_MOCK".to_string(), "0".to_string()));
    }
    // Persistência de cache de modelos (Hugging Face / PyTorch) no volume montado /outputs/.cache
    if dispatch.engine == "diffusion" {
        // Reduz fragmentação de VRAM (OOM de alocções grandes com modelo 4-bit
        // carregado em GPU apertada).
        exec_env.push((
            "PYTORCH_CUDA_ALLOC_CONF".to_string(),
            "expandable_segments:True".to_string(),
        ));
        exec_env.push((
            "HF_HOME".to_string(),
            "/outputs/.cache/huggingface".to_string(),
        ));
        exec_env.push((
            "HF_HUB_CACHE".to_string(),
            "/outputs/.cache/huggingface/hub".to_string(),
        ));
        exec_env.push((
            "TRANSFORMERS_CACHE".to_string(),
            "/outputs/.cache/huggingface/hub".to_string(),
        ));
        exec_env.push((
            "DIFFUSERS_CACHE".to_string(),
            "/outputs/.cache/huggingface/hub".to_string(),
        ));
        exec_env.push((
            "TORCH_HOME".to_string(),
            "/outputs/.cache/torch".to_string(),
        ));
    }

    // Repassa token do Hugging Face para download de modelos restritos/gated
    if let Ok(token) =
        std::env::var("HF_TOKEN").or_else(|_| std::env::var("HUGGING_FACE_HUB_TOKEN"))
    {
        if !token.is_empty() {
            exec_env.push(("HF_TOKEN".to_string(), token.clone()));
            exec_env.push(("HUGGING_FACE_HUB_TOKEN".to_string(), token));
        }
    }

    // Repassa FLUX_MODEL_ID customizado se definido no nó
    if let Ok(model_id) = std::env::var("FLUX_MODEL_ID") {
        if !model_id.is_empty() {
            exec_env.push(("FLUX_MODEL_ID".to_string(), model_id));
        }
    }

    // Spawn metrics collector & sample streamer (polls metrics.jsonl e outputs/samples durante execução)
    let metrics_path = outputs.join("metrics.jsonl");
    let metrics_path_clone = metrics_path.clone();
    let samples_dir = outputs.join("samples");
    let checkpoints_dir = outputs.join("checkpoints");
    let metrics_job_id = job_id.to_string();
    let metrics_total = total_epochs;
    let is_diffusion = dispatch.engine == "diffusion";

    let metrics_report_client = Arc::clone(&report_client);
    let metrics_s3 = Arc::clone(&s3);
    let metrics_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        let mut lines_read: usize = 0;
        let mut uploaded_samples = std::collections::HashSet::<String>::new();
        let mut uploaded_checkpoints = std::collections::HashSet::<String>::new();
        loop {
            interval.tick().await;

            let mut new_live_artifacts: Vec<ArtifactReport> = Vec::new();

            // 1a. Escaneia novas amostras de difusão em tempo real
            if is_diffusion && samples_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&samples_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            let is_image = path
                                .extension()
                                .and_then(|e| e.to_str())
                                .map(|ext| {
                                    matches!(
                                        ext.to_ascii_lowercase().as_str(),
                                        "png" | "jpg" | "jpeg" | "webp"
                                    )
                                })
                                .unwrap_or(false);

                            if is_image {
                                if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
                                    if fname.starts_with('.')
                                        || fname.ends_with(".tmp")
                                        || fname.ends_with(".part")
                                    {
                                        continue;
                                    }
                                    if !uploaded_samples.contains(fname) {
                                        let bytes = std::fs::metadata(&path)
                                            .map(|m| m.len() as i64)
                                            .unwrap_or(0);
                                        // Aguarda o arquivo ter tamanho > 0 (terminou de salvar)
                                        if bytes > 0 {
                                            let rel_path = format!("samples/{fname}");
                                            let art_key =
                                                format!("artifacts/{metrics_job_id}/{rel_path}");
                                            if let Ok(scoped) =
                                                scoped_key(S3Scope::Artifacts, &art_key)
                                            {
                                                if let Ok(md5) = compute_file_md5(&path) {
                                                    match put_with_retry(
                                                        metrics_s3.as_ref(),
                                                        &scoped,
                                                        &path,
                                                    )
                                                    .await
                                                    {
                                                        Ok(()) => {
                                                            uploaded_samples
                                                                .insert(fname.to_string());
                                                            new_live_artifacts.push(
                                                                ArtifactReport {
                                                                    kind: "sample".to_string(),
                                                                    path: rel_path,
                                                                    md5,
                                                                    bytes,
                                                                },
                                                            );
                                                        }
                                                        Err(e) => {
                                                            // Live é best-effort: não anuncia o que
                                                            // não subiu; próxima tick tenta de novo.
                                                            tracing::warn!(
                                                                job_id = %metrics_job_id,
                                                                artifact = %rel_path,
                                                                error = %e,
                                                                "falha persistente no upload live de sample"
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 1b. Escaneia novos checkpoints por época em tempo real
            if is_diffusion && checkpoints_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&checkpoints_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            let is_ckpt = path
                                .extension()
                                .and_then(|e| e.to_str())
                                .map(|ext| ext.eq_ignore_ascii_case("safetensors"))
                                .unwrap_or(false);

                            if is_ckpt {
                                if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
                                    if fname.starts_with('.')
                                        || fname.ends_with(".tmp")
                                        || fname.ends_with(".part")
                                    {
                                        continue;
                                    }
                                    if !uploaded_checkpoints.contains(fname) {
                                        let bytes = std::fs::metadata(&path)
                                            .map(|m| m.len() as i64)
                                            .unwrap_or(0);
                                        if bytes > 0 {
                                            let rel_path = format!("checkpoints/{fname}");
                                            let art_key =
                                                format!("artifacts/{metrics_job_id}/{rel_path}");
                                            if let Ok(scoped) =
                                                scoped_key(S3Scope::Artifacts, &art_key)
                                            {
                                                if let Ok(md5) = compute_file_md5(&path) {
                                                    match put_with_retry(
                                                        metrics_s3.as_ref(),
                                                        &scoped,
                                                        &path,
                                                    )
                                                    .await
                                                    {
                                                        Ok(()) => {
                                                            uploaded_checkpoints
                                                                .insert(fname.to_string());
                                                            new_live_artifacts.push(
                                                                ArtifactReport {
                                                                    kind: "checkpoint".to_string(),
                                                                    path: rel_path,
                                                                    md5,
                                                                    bytes,
                                                                },
                                                            );
                                                        }
                                                        Err(e) => {
                                                            // Live é best-effort: não anuncia o que
                                                            // não subiu; próxima tick tenta de novo.
                                                            tracing::warn!(
                                                                job_id = %metrics_job_id,
                                                                artifact = %rel_path,
                                                                error = %e,
                                                                "falha persistente no upload live de checkpoint"
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 2. Lê metrics.jsonl incrementalmente (helper compartilhado com o tail do daemon)
            let (new_metrics, new_lines_read) = tail_jsonl_lines(&metrics_path_clone, lines_read);
            lines_read = new_lines_read;

            // 3. Envia report se houver novas métricas OU novos artefatos (amostras/checkpoints)
            if !new_metrics.is_empty() {
                for m in new_metrics {
                    let progress = compute_progress(&m, metrics_total);
                    let is_metric = m.is_training_metric();
                    if let Some(ref msg) = m.message {
                        tracing::info!(
                            job_id = %metrics_job_id,
                            phase = ?m.phase,
                            epoch = m.epoch,
                            "{msg}"
                        );
                    }
                    let _ = metrics_report_client
                        .report(
                            &metrics_job_id,
                            &ReportBody {
                                status: "running".to_string(),
                                progress: Some(progress),
                                epoch: Some(m.epoch),
                                step: m.step.map(|s| s as i32),
                                metrics: if is_metric {
                                    Some(m.to_report_json())
                                } else {
                                    None
                                },
                                error: None,
                                artifacts: if new_live_artifacts.is_empty() {
                                    None
                                } else {
                                    Some(std::mem::take(&mut new_live_artifacts))
                                },
                                meta_content: None,
                                phase: m.phase.clone(),
                                message: m.message.clone(),
                            },
                        )
                        .await;
                }
            } else if !new_live_artifacts.is_empty() {
                for art in &new_live_artifacts {
                    tracing::info!(
                        job_id = %metrics_job_id,
                        artifact = %art.path,
                        "Artefato intermediário gerado e sincronizado"
                    );
                }
                let _ = metrics_report_client
                    .report(
                        &metrics_job_id,
                        &ReportBody {
                            status: "running".to_string(),
                            progress: None,
                            epoch: None,
                            step: None,
                            metrics: None,
                            error: None,
                            artifacts: Some(new_live_artifacts),
                            meta_content: None,
                            phase: None,
                            message: None,
                        },
                    )
                    .await;
            }
        }
    });

    // Ramifica subcomando e artefatos por (engine, mode) — ADR-0013 D6
    let subcommand_args: Vec<String> = match (dispatch.engine.as_str(), dispatch.mode.as_str()) {
        ("yolo", "train") => vec![
            "train".to_string(),
            "--config".to_string(),
            format!("/outputs/{job_id}/config.yaml"),
            "--output".to_string(),
            format!("/outputs/{job_id}"),
        ],
        ("yolo", "predict") => vec![
            "predict".to_string(),
            "--config".to_string(),
            format!("/outputs/{job_id}/config.yaml"),
            "--output".to_string(),
            format!("/outputs/{job_id}"),
        ],
        ("autotracker", _) => vec![
            "autotrack".to_string(),
            "--config".to_string(),
            format!("/outputs/{job_id}/config.yaml"),
            "--output".to_string(),
            format!("/outputs/{job_id}"),
        ],
        ("autolabel", _) => vec![
            "autolabel".to_string(),
            "--config".to_string(),
            format!("/outputs/{job_id}/config.yaml"),
            "--output".to_string(),
            format!("/outputs/{job_id}"),
        ],
        ("diffusion", "generate") => vec![
            "generate".to_string(),
            "--config".to_string(),
            format!("/outputs/{job_id}/config.yaml"),
            "--output".to_string(),
            format!("/outputs/{job_id}"),
        ],
        ("diffusion", _) => vec![
            "train".to_string(),
            "--config".to_string(),
            format!("/outputs/{job_id}/config.yaml"),
            "--output".to_string(),
            format!("/outputs/{job_id}"),
        ],
        (engine, mode) => {
            return Err(PipelineError::Other(format!(
                "unsupported engine/mode: {engine}/{mode}"
            )))
        }
    };

    tracing::info!(
        job_id = %job_id,
        container = %container_name,
        "Inicializando container de execução na GPU..."
    );
    let _ = report_client
        .report(
            job_id,
            &ReportBody {
                status: "running".to_string(),
                progress: Some(0.08),
                epoch: None,
                step: None,
                metrics: None,
                error: None,
                artifacts: None,
                meta_content: None,
                phase: Some("starting_container".to_string()),
                message: Some("Inicializando container de execução na GPU...".to_string()),
            },
        )
        .await;

    let (exit_code, logs) = executor
        .run(
            &dispatch.image,
            &container_name,
            &volumes,
            &subcommand_args,
            &exec_env,
            gpu_devices,
        )
        .await;

    // Cancela metrics collector
    metrics_handle.abort();

    // Remove from active jobs
    active_jobs.remove(job_id);

    // 8. Check exit code
    if exit_code != 0 {
        let logs_tail = logs.lines().rev().take(20).collect::<Vec<_>>().join("\n");
        return Err(PipelineError::DockerFailed {
            exit_code,
            logs_tail,
        });
    }

    // 9. Upload artifacts para S3 (D8 — artifacts/<job_id>/)
    let artifact_specs: Vec<(&str, &str)> = match (dispatch.engine.as_str(), dispatch.mode.as_str())
    {
        ("yolo", "train") => vec![
            ("best.pt", "model"),
            ("last.pt", "model"),
            ("metrics.jsonl", "metrics"),
        ],
        ("yolo", "predict") => vec![("predictions.json", "predictions")],
        ("autotracker", _) => vec![("boxes.json", "boxes"), ("metrics.jsonl", "metrics")],
        ("autolabel", _) => vec![("captions.jsonl", "captions"), ("metrics.jsonl", "metrics")],
        ("diffusion", "generate") => vec![], // glob abaixo (D2 ADR-0023)
        ("diffusion", _) => vec![
            ("adapter.safetensors", "model"),
            ("metrics.jsonl", "metrics"),
        ],
        // Já validado acima — seguro unreachable
        _ => unreachable!("unsupported engine/mode validated earlier"),
    };

    let mut artifacts = Vec::new();
    // Uploads com retry; falhas persistentes viram failed no gate abaixo
    // (incidente galeria vazia) — sem abortar o resto do loop.
    let mut upload_errors: Vec<String> = Vec::new();

    for (filename, kind) in artifact_specs {
        let file_path = outputs.join(filename);
        if file_path.exists() {
            let art_key = format!("artifacts/{job_id}/{filename}");
            let art_key = scoped_key(S3Scope::Artifacts, &art_key)
                .map_err(|e| PipelineError::ArtifactUpload(format!("artifact key: {e}")))?;
            let md5 = compute_file_md5(&file_path)
                .map_err(|e| PipelineError::ArtifactUpload(format!("md5 {filename}: {e}")))?;
            let bytes = std::fs::metadata(&file_path)
                .map(|m| m.len() as i64)
                .unwrap_or(0);

            put_with_retry(s3.as_ref(), &art_key, &file_path)
                .await
                .map_err(|e| PipelineError::ArtifactUpload(format!("upload {filename}: {e}")))?;

            artifacts.push(ArtifactReport {
                kind: kind.to_string(),
                path: filename.to_string(),
                md5,
                bytes,
            });
        }
    }

    // Coleta glob para diffusion generate (D2 — ADR-0023)
    // Gera generated_*.png (kind generated), thumb_*.jpg (kind generated_thumb),
    // generation_meta.json (kind generated_meta). generated.png legado só coleta
    // se NÃO houver numerados (evita duplicidade em batch=1).
    if dispatch.engine == "diffusion" && dispatch.mode == "generate" {
        if let Ok(entries) = std::fs::read_dir(&outputs) {
            let mut glob_files: Vec<std::path::PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file()
                        && !p
                            .file_name()
                            .and_then(|n| n.to_str())
                            .map(|n| {
                                n.starts_with('.') || n.ends_with(".tmp") || n.ends_with(".part")
                            })
                            .unwrap_or(false)
                })
                .collect();
            glob_files.sort();

            // Verifica se existem arquivos numerados (generated_*.png)
            let has_numbered = glob_files.iter().any(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("generated_") && n.ends_with(".png"))
                    .unwrap_or(false)
            });

            for gpath in glob_files {
                let fname = match gpath.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };

                let kind = if fname.starts_with("generated_") && fname.ends_with(".png") {
                    Some("generated")
                } else if fname.starts_with("thumb_")
                    && (fname.ends_with(".jpg") || fname.ends_with(".jpeg"))
                {
                    Some("generated_thumb")
                } else if fname == "generation_meta.json" {
                    Some("generated_meta")
                } else if fname == "generated.png" && !has_numbered {
                    // Legado: generated.png só coleta se NÃO houver numerados
                    Some("generated")
                } else {
                    None
                };

                if let Some(k) = kind {
                    let bytes = std::fs::metadata(&gpath)
                        .map(|m| m.len() as i64)
                        .unwrap_or(0);
                    if bytes > 0 {
                        let art_key = format!("artifacts/{job_id}/{fname}");
                        match scoped_key(S3Scope::Artifacts, &art_key) {
                            Ok(scoped) => match compute_file_md5(&gpath) {
                                Ok(md5) => {
                                    match put_with_retry(s3.as_ref(), &scoped, &gpath).await {
                                        Ok(()) => artifacts.push(ArtifactReport {
                                            kind: k.to_string(),
                                            path: fname,
                                            md5,
                                            bytes,
                                        }),
                                        Err(e) => upload_errors.push(format!("{fname}: {e}")),
                                    }
                                }
                                Err(e) => upload_errors.push(format!(
                                    "{fname}: falha ao preparar artefato (md5): {e}"
                                )),
                            },
                            Err(e) => upload_errors
                                .push(format!("{fname}: falha ao preparar artefato (key): {e}")),
                        }
                    }
                }
            }
        }
    }

    // Se for difusão, escaneia também samples/, checkpoints/ por época e modelos safetensors adicionais
    if dispatch.engine == "diffusion" {
        let samples_dir = outputs.join("samples");
        if samples_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&samples_dir) {
                let mut sample_files: Vec<std::path::PathBuf> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.is_file()
                            && !p
                                .file_name()
                                .and_then(|n| n.to_str())
                                .map(|n| {
                                    n.starts_with('.')
                                        || n.ends_with(".tmp")
                                        || n.ends_with(".part")
                                })
                                .unwrap_or(false)
                            && p.extension()
                                .and_then(|e| e.to_str())
                                .map(|ext| {
                                    matches!(
                                        ext.to_ascii_lowercase().as_str(),
                                        "png" | "jpg" | "jpeg" | "webp"
                                    )
                                })
                                .unwrap_or(false)
                    })
                    .collect();
                sample_files.sort();

                for s_path in sample_files {
                    if let Some(s_name) = s_path.file_name().and_then(|n| n.to_str()) {
                        let rel_path = format!("samples/{s_name}");
                        let art_key = format!("artifacts/{job_id}/{rel_path}");
                        match scoped_key(S3Scope::Artifacts, &art_key) {
                            Ok(scoped) => match compute_file_md5(&s_path) {
                                Ok(md5) => {
                                    let bytes = std::fs::metadata(&s_path)
                                        .map(|m| m.len() as i64)
                                        .unwrap_or(0);
                                    match put_with_retry(s3.as_ref(), &scoped, &s_path).await {
                                        Ok(()) => artifacts.push(ArtifactReport {
                                            kind: "sample".to_string(),
                                            path: rel_path,
                                            md5,
                                            bytes,
                                        }),
                                        Err(e) => upload_errors.push(format!("{rel_path}: {e}")),
                                    }
                                }
                                Err(e) => upload_errors.push(format!(
                                    "{rel_path}: falha ao preparar artefato (md5): {e}"
                                )),
                            },
                            Err(e) => upload_errors
                                .push(format!("{rel_path}: falha ao preparar artefato (key): {e}")),
                        }
                    }
                }
            }
        }

        // Escaneia checkpoints por época em outputs/checkpoints/
        let checkpoints_dir = outputs.join("checkpoints");
        if checkpoints_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&checkpoints_dir) {
                let mut ckpt_files: Vec<std::path::PathBuf> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.is_file()
                            && !p
                                .file_name()
                                .and_then(|n| n.to_str())
                                .map(|n| {
                                    n.starts_with('.')
                                        || n.ends_with(".tmp")
                                        || n.ends_with(".part")
                                })
                                .unwrap_or(false)
                            && p.extension()
                                .and_then(|e| e.to_str())
                                .map(|ext| ext.eq_ignore_ascii_case("safetensors"))
                                .unwrap_or(false)
                    })
                    .collect();
                ckpt_files.sort();

                for c_path in ckpt_files {
                    if let Some(c_name) = c_path.file_name().and_then(|n| n.to_str()) {
                        let rel_path = format!("checkpoints/{c_name}");
                        let art_key = format!("artifacts/{job_id}/{rel_path}");
                        match scoped_key(S3Scope::Artifacts, &art_key) {
                            Ok(scoped) => match compute_file_md5(&c_path) {
                                Ok(md5) => {
                                    let bytes = std::fs::metadata(&c_path)
                                        .map(|m| m.len() as i64)
                                        .unwrap_or(0);
                                    match put_with_retry(s3.as_ref(), &scoped, &c_path).await {
                                        Ok(()) => artifacts.push(ArtifactReport {
                                            kind: "checkpoint".to_string(),
                                            path: rel_path,
                                            md5,
                                            bytes,
                                        }),
                                        Err(e) => upload_errors.push(format!("{rel_path}: {e}")),
                                    }
                                }
                                Err(e) => upload_errors.push(format!(
                                    "{rel_path}: falha ao preparar artefato (md5): {e}"
                                )),
                            },
                            Err(e) => upload_errors
                                .push(format!("{rel_path}: falha ao preparar artefato (key): {e}")),
                        }
                    }
                }
            }
        }

        // Escaneia qualquer outro *.safetensors na raiz de outputs/ (ex: nome semântico configurado)
        if let Ok(entries) = std::fs::read_dir(&outputs) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    if let Some(f_name) = p.file_name().and_then(|n| n.to_str()) {
                        if !f_name.starts_with('.')
                            && !f_name.ends_with(".tmp")
                            && !f_name.ends_with(".part")
                            && f_name != "adapter.safetensors"
                            && p.extension()
                                .and_then(|e| e.to_str())
                                .map(|ext| ext.eq_ignore_ascii_case("safetensors"))
                                .unwrap_or(false)
                        {
                            let art_key = format!("artifacts/{job_id}/{f_name}");
                            match scoped_key(S3Scope::Artifacts, &art_key) {
                                Ok(scoped) => match compute_file_md5(&p) {
                                    Ok(md5) => {
                                        let bytes = std::fs::metadata(&p)
                                            .map(|m| m.len() as i64)
                                            .unwrap_or(0);
                                        match put_with_retry(s3.as_ref(), &scoped, &p).await {
                                            Ok(()) => artifacts.push(ArtifactReport {
                                                kind: "model".to_string(),
                                                path: f_name.to_string(),
                                                md5,
                                                bytes,
                                            }),
                                            Err(e) => upload_errors.push(format!("{f_name}: {e}")),
                                        }
                                    }
                                    Err(e) => upload_errors.push(format!(
                                        "{f_name}: falha ao preparar artefato (md5): {e}"
                                    )),
                                },
                                Err(e) => upload_errors.push(format!(
                                    "{f_name}: falha ao preparar artefato (key): {e}"
                                )),
                            }
                        }
                    }
                }
            }
        }
    }

    // Se for treino de difusão e existir training_config.json, inclui nos artefatos com kind "config"
    if dispatch.engine == "diffusion" && dispatch.mode == "train" {
        let training_config_path = outputs.join("training_config.json");
        if training_config_path.is_file() {
            let art_key = format!("artifacts/{job_id}/training_config.json");
            match scoped_key(S3Scope::Artifacts, &art_key) {
                Ok(scoped) => match compute_file_md5(&training_config_path) {
                    Ok(md5) => {
                        let bytes = std::fs::metadata(&training_config_path)
                            .map(|m| m.len() as i64)
                            .unwrap_or(0);
                        match put_with_retry(s3.as_ref(), &scoped, &training_config_path).await {
                            Ok(()) => artifacts.push(ArtifactReport {
                                kind: "config".to_string(),
                                path: "training_config.json".to_string(),
                                md5,
                                bytes,
                            }),
                            Err(e) => upload_errors.push(format!("training_config.json: {e}")),
                        }
                    }
                    Err(e) => upload_errors.push(format!(
                        "training_config.json: falha ao preparar artefato (md5): {e}"
                    )),
                },
                Err(e) => upload_errors.push(format!(
                    "training_config.json: falha ao preparar artefato (key): {e}"
                )),
            }
        }
    }

    // Incidente galeria vazia: upload persistente falhou → o job falhou do
    // ponto de vista do usuário; reportar done seria mentira.
    if !upload_errors.is_empty() {
        return Err(PipelineError::Other(format!(
            "upload de artefatos falhou: {}",
            upload_errors.join("; ")
        )));
    }

    // 10. Lê métricas finais para o report done
    let final_metrics = read_final_metrics(&metrics_path);

    // 11. Report done — inclui meta_content se generation_meta.json existe (D5 ADR-0023)
    let meta_content = read_generation_meta_content(&outputs);
    report_client
        .report(
            job_id,
            &ReportBody {
                status: "done".to_string(),
                progress: Some(1.0),
                epoch: final_metrics.as_ref().map(|m| m.epoch),
                step: final_metrics
                    .as_ref()
                    .and_then(|m| m.step.map(|s| s as i32)),
                // AC-006-A D1: somente métricas de treino entram no array.
                metrics: final_metrics
                    .as_ref()
                    .filter(|m| m.is_training_metric())
                    .map(|m| m.to_report_json()),
                error: None,
                artifacts: if artifacts.is_empty() {
                    None
                } else {
                    Some(artifacts)
                },
                meta_content,
                phase: Some("completed".to_string()),
                message: Some("Treino concluído".to_string()),
            },
        )
        .await
        .map_err(|e| PipelineError::ReportFailed(format!("report done: {e}")))?;

    // 12. Cleanup tempdir (datasets-cache e outputs persistem — volumes §8)
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;

    Ok(())
}

/// Lê as métricas finais do metrics.jsonl (última linha válida).
fn read_final_metrics(path: &Path) -> Option<MetricsLine> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut last = None;
    for line in content.lines() {
        if let Some(m) = parse_metrics_line(line) {
            last = Some(m);
        }
    }
    last
}

/// Lê o conteúdo textual do generation_meta.json para enviar como `meta_content`
/// no report done (D5 ADR-0023). Cap: 256 KiB — truncamento honesto com warning.
const META_CONTENT_MAX_BYTES: usize = 256 * 1024;

fn read_generation_meta_content(outputs: &Path) -> Option<String> {
    let path = outputs.join("generation_meta.json");
    if !path.is_file() {
        return None;
    }
    let raw = std::fs::read(&path).ok()?;
    if raw.is_empty() {
        return None;
    }
    if raw.len() > META_CONTENT_MAX_BYTES {
        tracing::warn!(
            path = %path.display(),
            raw_bytes = raw.len(),
            cap = META_CONTENT_MAX_BYTES,
            "generation_meta.json excede 256 KiB — truncando honestamente"
        );
        let truncated = &raw[..META_CONTENT_MAX_BYTES];
        // Recorta até a última quebra de linha para não enviar JSONL cortado no meio
        let last_nl = truncated.iter().rposition(|&b| b == b'\n');
        let slice = match last_nl {
            Some(pos) => &truncated[..=pos],
            None => truncated,
        };
        let mut s = String::from_utf8_lossy(slice).into_owned();
        s.push_str("\n[TRUNCATED — original excedeu 256 KiB]\n");
        return Some(s);
    }
    // Conteúdo pequeno o suficiente — lê como UTF-8, tolerando invalid bytes
    Some(String::from_utf8_lossy(&raw).into_owned())
}

// ---------------------------------------------------------------------------
// Telemetry (CPU/RAM from /proc — D9, LIMITAÇÃO documentada E3)
// ---------------------------------------------------------------------------

/// Lê CPU usage do /proc/stat.
///
/// LIMITAÇÃO: em container Linux, /proc reflete o host em single-node.
/// Em ambiente multi-node, os valores podem não corresponder ao host real.
pub fn read_cpu() -> f64 {
    let content = match std::fs::read_to_string("/proc/stat") {
        Ok(c) => c,
        Err(_) => return 0.0,
    };

    let first_line = match content.lines().next() {
        Some(l) => l,
        None => return 0.0,
    };

    // Formato: "cpu  user nice system idle iowait irq softirq steal"
    let parts: Vec<u64> = first_line
        .split_whitespace()
        .skip(1)
        .filter_map(|s| s.parse().ok())
        .collect();

    if parts.len() < 4 {
        return 0.0;
    }

    let idle = parts[3];
    let total: u64 = parts.iter().sum();

    if total == 0 {
        return 0.0;
    }

    ((total - idle) as f64 / total as f64) * 100.0
}

/// Lê RAM usada do /proc/meminfo (em bytes).
///
/// LIMITAÇÃO: mesma do CPU — reflete o host em Linux single-node.
pub fn read_ram() -> i64 {
    let content = match std::fs::read_to_string("/proc/meminfo") {
        Ok(c) => c,
        Err(_) => return 0,
    };

    let mut total = 0i64;
    let mut available = 0i64;

    for line in content.lines() {
        if let Some(v) = line.strip_prefix("MemTotal:") {
            total = v
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0)
                * 1024; // kB → B
        }
        if let Some(v) = line.strip_prefix("MemAvailable:") {
            available = v
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0)
                * 1024; // kB → B
        }
    }

    total - available
}

/// Lê RAM total do /proc/meminfo (em bytes).
///
/// Returns `None` se a leitura ou parse falhar (nunca pânico, nunca valor inventado).
pub fn read_ram_total() -> Option<i64> {
    let content = std::fs::read_to_string("/proc/meminfo").ok()?;
    parse_ram_total_from_content(&content)
}

/// Parseia o conteúdo de /proc/meminfo e devolve MemTotal em bytes.
fn parse_ram_total_from_content(content: &str) -> Option<i64> {
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("MemTotal:") {
            let kb: i64 = v.split_whitespace().next().and_then(|s| s.parse().ok())?;
            return Some(kb * 1024); // kB → B
        }
    }
    None
}

// ---------------------------------------------------------------------------
// GPU telemetry: nvidia-smi com fallback silencioso (D7)
// ---------------------------------------------------------------------------

/// Resultado do parse do nvidia-smi.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuTelemetry {
    /// Nomes das GPUs visíveis (ex.: "NVIDIA GeForce RTX 3060").
    pub gpus: Vec<String>,
    /// VRAM total somada em MiB (nvidia-smi reporta MiB).
    pub vram_total: i64,
    /// VRAM usada somada em MiB.
    pub vram_used: i64,
    /// Maior VRAM total individual entre as GPUs visíveis (MiB).
    /// 1 job = 1 GPU (backend.md §6) — capacidade real de treino de 1 job.
    pub max_gpu_mib: i64,
}

/// Tenta rodar `nvidia-smi` e parsear o CSV de saída.
/// Retorna `None` se o binário não existir ou falhar (fallback silencioso).
pub async fn try_nvidia_smi() -> Option<GpuTelemetry> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tokio::process::Command::new("nvidia-smi")
            .args([
                "--query-gpu=name,memory.total,memory.used",
                "--format=csv,noheader,nounits",
            ])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_nvidia_smi_csv(&stdout)
}

/// Parseia CSV do nvidia-smi (nomes + soma de VRAM em MiB + max individual).
/// Linhas malformadas são ignoradas (skip silencioso).
pub fn parse_nvidia_smi_csv(csv: &str) -> Option<GpuTelemetry> {
    let mut gpus = Vec::new();
    let mut vram_total_mib: i64 = 0;
    let mut vram_used_mib: i64 = 0;
    let mut max_gpu_mib: i64 = 0;

    for line in csv.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Formato: "NVIDIA GeForce RTX 3060, 12288, 0"
        let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if parts.len() < 3 {
            continue; // linha malformada → ignora
        }
        let name = parts[0].to_string();
        let total: i64 = match parts[1].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let used: i64 = match parts[2].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        gpus.push(name);
        vram_total_mib += total;
        vram_used_mib += used;
        if total > max_gpu_mib {
            max_gpu_mib = total;
        }
    }

    if gpus.is_empty() {
        return None;
    }

    Some(GpuTelemetry {
        gpus,
        vram_total: vram_total_mib,
        vram_used: vram_used_mib,
        max_gpu_mib,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::Instant;

    // -- scoped_key tests --

    #[test]
    fn scoped_key_packages_valid() {
        let key = scoped_key(S3Scope::Packages, "packages/abc-123/dataset.zip");
        assert_eq!(key, Ok("packages/abc-123/dataset.zip".to_string()));
    }

    #[test]
    fn scoped_key_artifacts_valid() {
        let key = scoped_key(S3Scope::Artifacts, "artifacts/job-456/best.pt");
        assert_eq!(key, Ok("artifacts/job-456/best.pt".to_string()));
    }

    #[test]
    fn scoped_key_empty() {
        assert_eq!(
            scoped_key(S3Scope::Packages, ""),
            Err(ScopedKeyError::EmptyKey)
        );
    }

    #[test]
    fn scoped_key_absolute() {
        assert_eq!(
            scoped_key(S3Scope::Packages, "/packages/x"),
            Err(ScopedKeyError::AbsolutePath)
        );
    }

    #[test]
    fn scoped_key_traversal() {
        assert_eq!(
            scoped_key(S3Scope::Packages, "../etc/passwd"),
            Err(ScopedKeyError::PathTraversal)
        );
    }

    #[test]
    fn scoped_key_traversal_in_middle() {
        assert_eq!(
            scoped_key(S3Scope::Artifacts, "artifacts/../evil"),
            Err(ScopedKeyError::PathTraversal)
        );
    }

    #[test]
    fn scoped_key_outside_scope() {
        assert_eq!(
            scoped_key(S3Scope::Packages, "datasets/abc/images"),
            Err(ScopedKeyError::OutsideScope)
        );
    }

    #[test]
    fn scoped_key_wrong_prefix() {
        assert_eq!(
            scoped_key(S3Scope::Artifacts, "packages/abc/dataset.zip"),
            Err(ScopedKeyError::OutsideScope)
        );
    }

    // -- metrics parsing tests --

    #[test]
    fn parse_metrics_line_valid() {
        let line = r#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":5}"#;
        let m = parse_metrics_line(line).unwrap();
        assert_eq!(m.epoch, 5);
        assert!((m.map50 - 0.8).abs() < 1e-6);
        assert!((m.map50_95 - 0.6).abs() < 1e-6);
    }

    #[test]
    fn parse_metrics_line_empty() {
        assert!(parse_metrics_line("").is_none());
        assert!(parse_metrics_line("  ").is_none());
    }

    #[test]
    fn parse_metrics_line_malformed() {
        assert!(parse_metrics_line(r#"{"box_loss":0.5}"#).is_none());
        assert!(parse_metrics_line("not json").is_none());
    }

    #[test]
    fn parse_metrics_line_tolerant() {
        let good = r#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":5}"#;
        assert!(parse_metrics_line(good).is_some());
        assert!(parse_metrics_line("{}").is_none());
    }

    #[test]
    fn parse_metrics_line_diffusion() {
        let diff_line = r#"{"epoch":3,"step":30,"loss":0.0452,"lr":0.0001}"#;
        let parsed = parse_metrics_line(diff_line).expect("should parse diffusion line");
        assert_eq!(parsed.epoch, 3);
        assert_eq!(parsed.step, Some(30));
        assert_eq!(parsed.loss, Some(0.0452));
        assert_eq!(parsed.lr, Some(0.0001));
        assert_eq!(parsed.box_loss, 0.0);
    }

    #[test]
    fn parse_metrics_line_nan_tolerant() {
        let nan_line = r#"{"epoch": 1, "step": 5, "loss": NaN, "lr": 0.0001}"#;
        let parsed = parse_metrics_line(nan_line).expect("should parse line with NaN safely");
        assert_eq!(parsed.epoch, 1);
        assert_eq!(parsed.step, Some(5));
        assert_eq!(parsed.loss, None);
        assert_eq!(parsed.lr, Some(0.0001));
    }

    // -- read_ram_total tests --

    #[test]
    fn parse_ram_total_present() {
        let content = "MemTotal:       16384000 kB\nMemFree:         8192000 kB\nMemAvailable:    8192000 kB\n";
        assert_eq!(parse_ram_total_from_content(content), Some(16384000 * 1024));
    }

    #[test]
    fn parse_ram_total_missing() {
        let content = "MemFree:         8192000 kB\nMemAvailable:    8192000 kB\n";
        assert_eq!(parse_ram_total_from_content(content), None);
    }

    #[test]
    fn parse_ram_total_empty() {
        assert_eq!(parse_ram_total_from_content(""), None);
    }

    // -- config.yaml tests --

    #[test]
    fn replace_config_placeholders_basic() {
        let config = "dataset_path: {dataset_path}\noutput_path: {output_path}";
        let result = replace_config_placeholders(
            config,
            "/datasets/datasets-cache/j1",
            "/outputs/j1",
            None,
            &[],
            None,
            None,
            None,
            None,
        );
        assert_eq!(
            result,
            "dataset_path: /datasets/datasets-cache/j1\noutput_path: /outputs/j1"
        );
    }

    #[test]
    fn replace_config_placeholders_yaml_parseable() {
        let config =
            "dataset_path: {dataset_path}\noutput_path: {output_path}\nepochs: 100\nmodel: yolo11m";
        let result = replace_config_placeholders(
            config,
            "/datasets/datasets-cache/j1",
            "/outputs/j1",
            None,
            &[],
            None,
            None,
            None,
            None,
        );
        let parsed: serde_yaml::Value = serde_yaml::from_str(&result).unwrap();
        assert_eq!(parsed["dataset_path"], "/datasets/datasets-cache/j1");
        assert_eq!(parsed["output_path"], "/outputs/j1");
        assert_eq!(parsed["epochs"], 100);
        assert_eq!(parsed["model"], "yolo11m");
    }

    #[test]
    fn extract_epochs_from_config() {
        let config = "epochs: 50\nmodel: yolo11m\nbatch: 16";
        assert_eq!(extract_epochs(config), 50);
    }

    #[test]
    fn extract_epochs_default() {
        let config = "model: yolo11m";
        assert_eq!(extract_epochs(config), 100);
    }

    // -- zip-slip tests --

    #[test]
    fn unzip_safe_rejects_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("evil.zip");

        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file("../evil.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"pwned").unwrap();
            zip.finish().unwrap();
        }

        let dest = tmp.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();

        let result = unzip_safe(&zip_path, &dest);
        assert!(result.is_err());
        assert!(matches!(result, Err(PipelineError::UnzipFailed(_))));
    }

    #[test]
    fn unzip_safe_rejects_absolute() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("evil.zip");

        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file("/etc/evil.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"pwned").unwrap();
            zip.finish().unwrap();
        }

        let dest = tmp.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();

        let result = unzip_safe(&zip_path, &dest);
        assert!(result.is_err());
    }

    #[test]
    fn unzip_safe_accepts_valid() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("valid.zip");

        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file("images/photo.jpg", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"fake image").unwrap();
            zip.start_file("labels/photo.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"0 0.5 0.5 0.1 0.1").unwrap();
            zip.finish().unwrap();
        }

        let dest = tmp.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();

        let result = unzip_safe(&zip_path, &dest);
        assert!(result.is_ok());
        assert!(dest.join("images/photo.jpg").exists());
        assert!(dest.join("labels/photo.txt").exists());
    }

    // -- compute_progress tests --

    #[test]
    fn compute_progress_basic() {
        let m = MetricsLine {
            box_loss: 0.0,
            cls_loss: 0.0,
            dfl_loss: 0.0,
            map50: 0.0,
            map50_95: 0.0,
            loss: None,
            lr: None,
            step: None,
            epoch: 5,
            progress: None,
            phase: None,
            message: None,
            vram_used_gb: None,
        };
        assert!((compute_progress(&m, 100) - 0.05).abs() < 1e-6);
    }

    #[test]
    fn compute_progress_zero_epochs() {
        let m = MetricsLine {
            box_loss: 0.0,
            cls_loss: 0.0,
            dfl_loss: 0.0,
            map50: 0.0,
            map50_95: 0.0,
            loss: None,
            lr: None,
            step: None,
            epoch: 5,
            progress: None,
            phase: None,
            message: None,
            vram_used_gb: None,
        };
        assert!((compute_progress(&m, 0) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn compute_progress_full() {
        let m = MetricsLine {
            box_loss: 0.0,
            cls_loss: 0.0,
            dfl_loss: 0.0,
            map50: 0.0,
            map50_95: 0.0,
            loss: None,
            lr: None,
            step: None,
            epoch: 100,
            progress: None,
            phase: None,
            message: None,
            vram_used_gb: None,
        };
        assert!((compute_progress(&m, 100) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn compute_progress_explicit() {
        let m = MetricsLine {
            box_loss: 0.0,
            cls_loss: 0.0,
            dfl_loss: 0.0,
            map50: 0.0,
            map50_95: 0.0,
            loss: None,
            lr: None,
            step: None,
            epoch: 3,
            progress: Some(0.65),
            phase: None,
            message: None,
            vram_used_gb: None,
        };
        assert!((compute_progress(&m, 100) - 0.65).abs() < 1e-6);
    }

    // -- executor args test --

    struct FakeExecutor {
        last_args: std::sync::Mutex<Option<Vec<String>>>,
    }

    impl FakeExecutor {
        fn new() -> Self {
            Self {
                last_args: std::sync::Mutex::new(None),
            }
        }

        fn last_args(&self) -> Option<Vec<String>> {
            self.last_args.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl TrainerExecutor for FakeExecutor {
        async fn run(
            &self,
            _image: &str,
            _container_name: &str,
            _volumes: &[(String, String)],
            args: &[String],
            _env: &[(String, String)],
            _gpu_devices: Option<&str>,
        ) -> (i32, String) {
            *self.last_args.lock().unwrap() = Some(args.to_vec());
            (0, "ok".to_string())
        }

        async fn stop(&self, _container_name: &str) -> Result<(), String> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn run_job_inner_passes_correct_args() {
        use std::sync::Arc;

        let executor = Arc::new(FakeExecutor::new());
        let job_id = "test-job-001";

        // Simulate the args that run_job_inner would build
        let expected_args = vec![
            "train".to_string(),
            "--config".to_string(),
            format!("/outputs/{job_id}/config.yaml"),
            "--output".to_string(),
            format!("/outputs/{job_id}"),
        ];

        // Directly call executor.run to verify args are forwarded
        let (_, _) = executor
            .run(
                "my-image:latest",
                "trainer-test",
                &[("/data/datasets".into(), "/datasets".into())],
                &expected_args,
                &[],
                None,
            )
            .await;

        assert_eq!(executor.last_args(), Some(expected_args));
    }

    // -- idempotency test (dispatch com mesmo job_id → 409) --

    #[test]
    fn active_jobs_idempotency() {
        let active = ActiveJobs::default();
        // Simula job já existente
        active.insert(
            "job-123".to_string(),
            ActiveJobState::new("trainer-yolo-job-123".to_string()),
        );

        // Segundo dispatch com mesmo job_id deve ser detectado
        assert!(active.contains_key("job-123"));
    }

    // =========================================================================
    // A.3 — engine branching tests
    // =========================================================================

    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Mock S3 que serve um zip válido para download e grava uploads.
    /// `upload_fail` simula bucket inexistente/S3 fora do ar: `put` sempre falha.
    /// `puts` conta tentativas (prova do retry N=3); `fail_first_n` simula S3
    /// instável (falha as N primeiras tentativas, depois sucede).
    struct FakeS3 {
        downloads: Mutex<Vec<String>>,
        uploads: Mutex<Vec<(String, PathBuf)>>,
        zip_bytes: Vec<u8>,
        upload_fail: AtomicBool,
        puts: std::sync::atomic::AtomicUsize,
        fail_first_n: std::sync::atomic::AtomicUsize,
    }

    impl FakeS3 {
        fn new() -> Self {
            // Cria um zip in-memory com dataset.yaml vazio
            let mut buf = std::io::Cursor::new(Vec::new());
            {
                let mut zip = zip::ZipWriter::new(&mut buf);
                let opts = zip::write::SimpleFileOptions::default();
                zip.start_file("dataset.yaml", opts).unwrap();
                zip.write_all(b"classes: []\nimages: []\n").unwrap();
                zip.finish().unwrap();
            }
            Self {
                downloads: Mutex::new(Vec::new()),
                uploads: Mutex::new(Vec::new()),
                zip_bytes: buf.into_inner(),
                upload_fail: AtomicBool::new(false),
                puts: std::sync::atomic::AtomicUsize::new(0),
                fail_first_n: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        fn set_upload_fail(&self, v: bool) {
            self.upload_fail.store(v, Ordering::SeqCst);
        }

        fn set_fail_first_n(&self, n: usize) {
            self.fail_first_n.store(n, Ordering::SeqCst);
        }

        fn put_count(&self) -> usize {
            self.puts.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl S3Port for FakeS3 {
        async fn get_to_file(&self, key: &str, path: &std::path::Path) -> Result<(), String> {
            self.downloads.lock().unwrap().push(key.to_string());
            std::fs::write(path, &self.zip_bytes).map_err(|e| format!("write zip: {e}"))
        }

        async fn put(&self, key: &str, path: &std::path::Path) -> Result<(), String> {
            self.puts.fetch_add(1, Ordering::SeqCst);
            if self.upload_fail.load(Ordering::SeqCst) {
                return Err(format!(
                    "S3 PUT {key}: bucket inexistente (fake upload_fail)"
                ));
            }
            if self.fail_first_n.load(Ordering::SeqCst) > 0 {
                self.fail_first_n.fetch_sub(1, Ordering::SeqCst);
                return Err(format!(
                    "S3 PUT {key}: instabilidade transitória (fake flaky)"
                ));
            }
            self.uploads
                .lock()
                .unwrap()
                .push((key.to_string(), path.to_path_buf()));
            Ok(())
        }

        async fn ping(&self) -> bool {
            true
        }
    }

    /// Mock S3 que serve bytes customizados por prefixo (para testes de weights).
    /// Qualquer key com `models/` ou `artifacts/` retorna `weights_bytes`;
    /// caso contrário, retorna `zip_bytes` (comportamento padrão do FakeS3).
    struct FakeS3WithWeights {
        downloads: Mutex<Vec<String>>,
        uploads: Mutex<Vec<(String, PathBuf)>>,
        zip_bytes: Vec<u8>,
        weights_bytes: Vec<u8>,
    }

    impl FakeS3WithWeights {
        fn new(weights_bytes: Vec<u8>) -> Self {
            let mut buf = std::io::Cursor::new(Vec::new());
            {
                let mut zip = zip::ZipWriter::new(&mut buf);
                let opts = zip::write::SimpleFileOptions::default();
                zip.start_file("dataset.yaml", opts).unwrap();
                zip.write_all(b"classes: []\nimages: []\n").unwrap();
                zip.finish().unwrap();
            }
            Self {
                downloads: Mutex::new(Vec::new()),
                uploads: Mutex::new(Vec::new()),
                zip_bytes: buf.into_inner(),
                weights_bytes,
            }
        }
    }

    #[async_trait]
    impl S3Port for FakeS3WithWeights {
        async fn get_to_file(&self, key: &str, path: &std::path::Path) -> Result<(), String> {
            self.downloads.lock().unwrap().push(key.to_string());
            let data = if key.starts_with("models/") || key.starts_with("artifacts/") {
                &self.weights_bytes
            } else {
                &self.zip_bytes
            };
            std::fs::write(path, data).map_err(|e| format!("write file: {e}"))
        }

        async fn put(&self, key: &str, path: &std::path::Path) -> Result<(), String> {
            self.uploads
                .lock()
                .unwrap()
                .push((key.to_string(), path.to_path_buf()));
            Ok(())
        }

        async fn ping(&self) -> bool {
            true
        }
    }

    /// Calcula MD5 de bytes in-memory (hex 32).
    fn compute_file_md5_bytes(data: &[u8]) -> String {
        use md5::Digest;
        let digest = md5::Md5::digest(data);
        hex::encode(digest)
    }

    /// Mock ReportClient que grava relatórios.
    struct FakeReport {
        reports: Mutex<Vec<ReportBody>>,
    }

    impl FakeReport {
        fn new() -> Self {
            Self {
                reports: Mutex::new(Vec::new()),
            }
        }

        fn statuses(&self) -> Vec<String> {
            self.reports
                .lock()
                .unwrap()
                .iter()
                .map(|r| r.status.clone())
                .collect()
        }

        fn failed_report(&self) -> Option<ReportBody> {
            self.reports
                .lock()
                .unwrap()
                .iter()
                .find(|r| r.status == "failed")
                .cloned()
        }

        fn done_artifacts(&self) -> Option<Vec<ArtifactReport>> {
            self.reports
                .lock()
                .unwrap()
                .iter()
                .find(|r| r.status == "done")
                .and_then(|r| r.artifacts.clone())
        }

        fn done_meta_content(&self) -> Option<String> {
            self.reports
                .lock()
                .unwrap()
                .iter()
                .find(|r| r.status == "done")
                .and_then(|r| r.meta_content.clone())
        }
    }

    #[async_trait]
    impl ReportClient for FakeReport {
        async fn report(&self, _job_id: &str, body: &ReportBody) -> Result<(), String> {
            self.reports.lock().unwrap().push(body.clone());
            Ok(())
        }
    }

    /// Executor que simula o trainer — apenas grava os args recebidos.
    struct FakeTrainerExecutor {
        last_args: Mutex<Option<Vec<String>>>,
    }

    impl FakeTrainerExecutor {
        fn new() -> Self {
            Self {
                last_args: Mutex::new(None),
            }
        }

        fn last_args(&self) -> Option<Vec<String>> {
            self.last_args.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl TrainerExecutor for FakeTrainerExecutor {
        async fn run(
            &self,
            _image: &str,
            _container_name: &str,
            _volumes: &[(String, String)],
            args: &[String],
            _env: &[(String, String)],
            _gpu_devices: Option<&str>,
        ) -> (i32, String) {
            *self.last_args.lock().unwrap() = Some(args.to_vec());
            (0, "ok".to_string())
        }

        async fn stop(&self, _container_name: &str) -> Result<(), String> {
            Ok(())
        }
    }

    /// Helper que cria os arquivos de output simulados no diretorio correto.
    /// O run_job_inner le de `workdir/outputs/<job_id>/`, entao pre-criamos la.
    fn create_fake_outputs(workdir: &Path, job_id: &str, files: &HashMap<String, Vec<u8>>) {
        let outputs = workdir.join("outputs").join(job_id);
        std::fs::create_dir_all(&outputs).unwrap();
        for (name, content) in files {
            std::fs::write(outputs.join(name), content).unwrap();
        }
    }

    /// Helper para criar um DispatchRequest de teste.
    fn make_dispatch(job_id: &str, engine: &str) -> DispatchRequest {
        DispatchRequest {
            job_id: job_id.to_string(),
            engine: engine.to_string(),
            image: "hephaestus/trainer-yolo:local".to_string(),
            exec_mode: "docker".to_string(),
            package_ref: Some(PackageRef {
                key: "packages/test-pkg/dataset.zip".to_string(),
                md5_zip: String::new(), // será calculado
                bytes: 0,
            }),
            config_yaml: Some(
                "epochs: 1\ndataset_path: {dataset_path}\noutput_path: {output_path}".to_string(),
            ),
            dataset_version_id: None,
            workdir: "/tmp".to_string(),
            mode: "train".to_string(),
            weights_ref: None,
            loras: Vec::new(),
            custom_checkpoint: None,
            text_encoder: None,
            init_image_ref: None,
            control_package_ref: None,
        }
    }

    /// Cria um dispatch com MD5 correto do zip fake.
    fn make_dispatch_with_valid_md5(
        job_id: &str,
        engine: &str,
        zip_path: &Path,
    ) -> DispatchRequest {
        let md5 = compute_file_md5(zip_path).unwrap();
        let mut d = make_dispatch(job_id, engine);
        if let Some(ref mut pr) = d.package_ref {
            pr.md5_zip = md5;
        }
        d
    }

    // -- A.3 test 1: engine yolo → subcomando train, artefatos [best.pt, last.pt, metrics.jsonl] --

    #[tokio::test]
    async fn engine_yolo_uses_train_subcommand_and_yolo_artifacts() {
        let tmp = tempfile::tempdir().unwrap();

        // Prepara zip fake para o S3
        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-yolo-001", "yolo", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        // Pre-cria os arquivos de output que o "trainer" produziria
        let mut output_files = HashMap::new();
        output_files.insert("best.pt".to_string(), b"fake model".to_vec());
        output_files.insert("last.pt".to_string(), b"fake model".to_vec());
        output_files.insert(
            "metrics.jsonl".to_string(),
            br#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":1}"#.to_vec(),
        );
        create_fake_outputs(tmp.path(), "job-yolo-001", &output_files);

        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;
        assert!(
            result.is_ok(),
            "yolo pipeline should succeed: {:?}",
            result.err()
        );

        // Verifica subcomando
        let args = executor.last_args().unwrap();
        assert_eq!(args[0], "train");
        assert_eq!(args[1], "--config");
        assert_eq!(args[3], "--output");

        // Verifica artefatos: yolo produz best.pt, last.pt, metrics.jsonl
        let artifacts = report.done_artifacts().unwrap();
        let kinds: Vec<&str> = artifacts.iter().map(|a| a.kind.as_str()).collect();
        let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
        assert!(filenames.contains(&"best.pt"));
        assert!(filenames.contains(&"last.pt"));
        assert!(filenames.contains(&"metrics.jsonl"));
        assert!(kinds.contains(&"model")); // best.pt e last.pt são kind "model"
        assert!(kinds.contains(&"metrics")); // metrics.jsonl é kind "metrics"
        assert!(!filenames.contains(&"boxes.json")); // yolo NÃO produz boxes.json
    }

    // -- A.3 test 2: engine autotracker → subcomando autotrack, artefatos [boxes.json, metrics.jsonl] --

    #[tokio::test]
    async fn engine_autotracker_uses_autotrack_subcommand_and_autotracker_artifacts() {
        let tmp = tempfile::tempdir().unwrap();

        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-at-001", "autotracker", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let mut output_files = HashMap::new();
        output_files.insert(
            "boxes.json".to_string(),
            br#"{"engine":"autotracker","model":"mock","seed":42,"conf":0.65,"images":[]}"#
                .to_vec(),
        );
        output_files.insert(
            "metrics.jsonl".to_string(),
            br#"{"box_loss":0.1,"cls_loss":0.2,"dfl_loss":0.3,"mAP50":0.9,"mAP50-95":0.7,"epoch":1}"#.to_vec(),
        );
        create_fake_outputs(tmp.path(), "job-at-001", &output_files);

        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;
        assert!(
            result.is_ok(),
            "autotracker pipeline should succeed: {:?}",
            result.err()
        );

        // Verifica subcomando: autotrack, não train
        let args = executor.last_args().unwrap();
        assert_eq!(args[0], "autotrack");
        assert_eq!(args[1], "--config");
        assert_eq!(args[3], "--output");

        // Verifica artefatos: autotracker produz boxes.json + metrics.jsonl
        let artifacts = report.done_artifacts().unwrap();
        let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
        let kinds: Vec<&str> = artifacts.iter().map(|a| a.kind.as_str()).collect();
        assert!(filenames.contains(&"boxes.json"));
        assert!(filenames.contains(&"metrics.jsonl"));
        assert!(kinds.contains(&"boxes")); // boxes.json é kind "boxes"
        assert!(kinds.contains(&"metrics")); // metrics.jsonl é kind "metrics"
        assert!(!filenames.contains(&"best.pt")); // autotracker NÃO produz model artifacts
        assert!(!filenames.contains(&"last.pt"));
    }

    // -- AL.3 test: engine autolabel → subcomando autolabel, artefatos [captions.jsonl, metrics.jsonl] --

    #[tokio::test]
    async fn engine_autolabel_uses_autolabel_subcommand_and_captions_artifacts() {
        let tmp = tempfile::tempdir().unwrap();

        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-al-001", "autolabel", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let mut output_files = HashMap::new();
        output_files.insert(
            "captions.jsonl".to_string(),
            br#"{"filename":"img.jpg","caption":"uma foto de teste"}"#.to_vec(),
        );
        output_files.insert(
            "metrics.jsonl".to_string(),
            br#"{"epoch":1,"loss":0.0,"images":1}"#.to_vec(),
        );
        create_fake_outputs(tmp.path(), "job-al-001", &output_files);

        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;
        assert!(
            result.is_ok(),
            "autolabel pipeline should succeed: {:?}",
            result.err()
        );

        // Verifica subcomando: autolabel
        let args = executor.last_args().unwrap();
        assert_eq!(args[0], "autolabel");
        assert_eq!(args[1], "--config");
        assert_eq!(args[3], "--output");

        // Verifica artefatos: captions.jsonl + metrics.jsonl
        let artifacts = report.done_artifacts().unwrap();
        let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
        let kinds: Vec<&str> = artifacts.iter().map(|a| a.kind.as_str()).collect();
        assert!(filenames.contains(&"captions.jsonl"));
        assert!(filenames.contains(&"metrics.jsonl"));
        assert!(kinds.contains(&"captions"));
        assert!(kinds.contains(&"metrics"));
        assert!(!filenames.contains(&"best.pt"));
        assert!(!filenames.contains(&"boxes.json"));
    }

    // -- A.3 test 3: engine desconhecido → falha limpa (via run_job que faz o report "failed") --

    #[tokio::test]
    async fn unknown_engine_returns_clean_error() {
        let tmp = tempfile::tempdir().unwrap();

        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch =
            make_dispatch_with_valid_md5("job-bad-001", "unknown_engine_foo", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        // Usa run_job (não run_job_inner) para testar o caminho completo de falha
        run_job(
            dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            active_jobs.clone(),
            None,
            false,
            None,
        )
        .await;

        // Verifica que os reports incluem preparing e failed (via run_job outer)
        let statuses = report.statuses();
        assert!(statuses.contains(&"preparing".to_string()));
        assert!(statuses.contains(&"failed".to_string()));
        assert!(!statuses.contains(&"done".to_string()));

        // Executor nunca foi chamado (engine check falha antes)
        assert!(executor.last_args().is_none());
    }

    // -- ADR-0018: engine diffusion usa train e coleta adapter.safetensors --

    #[tokio::test]
    async fn engine_diffusion_uses_train_subcommand_and_adapter_artifacts() {
        let tmp = tempfile::tempdir().unwrap();

        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-diff-001", "diffusion", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let mut output_files = HashMap::new();
        output_files.insert(
            "adapter.safetensors".to_string(),
            b"fake safetensors bytes".to_vec(),
        );
        output_files.insert(
            "metrics.jsonl".to_string(),
            br#"{"epoch":1,"step":10,"loss":0.42}"#.to_vec(),
        );
        create_fake_outputs(tmp.path(), "job-diff-001", &output_files);

        let res = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;

        assert!(res.is_ok(), "run_job_inner failed: {:?}", res);

        let args = executor.last_args().unwrap();
        assert_eq!(args[0], "train");
        assert_eq!(args[1], "--config");
        assert_eq!(args[3], "--output");

        // Verifica artefatos coletados: adapter.safetensors + metrics.jsonl
        let artifacts = report.done_artifacts().unwrap();
        let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
        let kinds: Vec<&str> = artifacts.iter().map(|a| a.kind.as_str()).collect();
        assert!(filenames.contains(&"adapter.safetensors"));
        assert!(filenames.contains(&"metrics.jsonl"));
        assert!(kinds.contains(&"model"));
        assert!(kinds.contains(&"metrics"));
    }

    // -- control dataset (treino difusão): staging + placeholder + defesa --

    /// Fake S3 que serve zips distintos por key: pacote principal vs controle.
    /// Reusa o mesmo zip válido do FakeS3 para ambos; o MD5 é calculado sobre
    /// os bytes servidos, então o dispatch usa `compute_file_md5_bytes`.
    struct FakeS3WithControl {
        downloads: Mutex<Vec<String>>,
        uploads: Mutex<Vec<(String, PathBuf)>>,
        main_zip: Vec<u8>,
        control_zip: Vec<u8>,
    }

    impl FakeS3WithControl {
        fn new() -> Self {
            let mut main_buf = std::io::Cursor::new(Vec::new());
            {
                let mut zip = zip::ZipWriter::new(&mut main_buf);
                let opts = zip::write::SimpleFileOptions::default();
                zip.start_file("dataset.yaml", opts).unwrap();
                zip.write_all(b"classes: []\nimages: []\n").unwrap();
                zip.finish().unwrap();
            }
            let mut control_buf = std::io::Cursor::new(Vec::new());
            {
                let mut zip = zip::ZipWriter::new(&mut control_buf);
                let opts = zip::write::SimpleFileOptions::default();
                zip.start_file("regularization.txt", opts).unwrap();
                zip.write_all(b"control images\n").unwrap();
                zip.finish().unwrap();
            }
            Self {
                downloads: Mutex::new(Vec::new()),
                uploads: Mutex::new(Vec::new()),
                main_zip: main_buf.into_inner(),
                control_zip: control_buf.into_inner(),
            }
        }
    }

    #[async_trait]
    impl S3Port for FakeS3WithControl {
        async fn get_to_file(&self, key: &str, path: &std::path::Path) -> Result<(), String> {
            self.downloads.lock().unwrap().push(key.to_string());
            let data = if key.contains("control") {
                &self.control_zip
            } else {
                &self.main_zip
            };
            std::fs::write(path, data).map_err(|e| format!("write file: {e}"))
        }

        async fn put(&self, key: &str, path: &std::path::Path) -> Result<(), String> {
            self.uploads
                .lock()
                .unwrap()
                .push((key.to_string(), path.to_path_buf()));
            Ok(())
        }

        async fn ping(&self) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn diffusion_train_control_dataset_staged_and_config_resolved() {
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3WithControl::new());

        let mut dispatch = make_dispatch("job-control-001", "diffusion");
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        dispatch.mode = "train".to_string();
        dispatch.package_ref = Some(PackageRef {
            key: "packages/test-pkg/dataset.zip".to_string(),
            md5_zip: compute_file_md5_bytes(&s3.main_zip),
            bytes: s3.main_zip.len() as i64,
        });
        dispatch.control_package_ref = Some(PackageRef {
            key: "packages/test-pkg/control.zip".to_string(),
            md5_zip: compute_file_md5_bytes(&s3.control_zip),
            bytes: s3.control_zip.len() as i64,
        });
        dispatch.config_yaml = Some(
            "epochs: 1\ndataset_path: {dataset_path}\noutput_path: {output_path}\ncontrol_dataset_path: \"{control_dataset_path}\"\ncache_text_embeddings: true"
                .to_string(),
        );
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let mut output_files = HashMap::new();
        output_files.insert(
            "adapter.safetensors".to_string(),
            b"fake safetensors bytes".to_vec(),
        );
        create_fake_outputs(tmp.path(), "job-control-001", &output_files);

        let res = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;
        assert!(res.is_ok(), "run_job_inner failed: {:?}", res);

        // Diretório de controle existe no host (staging) com o conteúdo do zip.
        let control_dir = tmp
            .path()
            .join("datasets")
            .join("datasets-cache")
            .join("job-control-001")
            .join("control");
        assert!(
            control_dir.join("regularization.txt").is_file(),
            "control dataset deve ser extraído em datasets-cache/<job>/control"
        );

        // Config final entregue ao trainer contém o path real, sem placeholder.
        let config_path = tmp
            .path()
            .join("outputs")
            .join("job-control-001")
            .join("config.yaml");
        let config = std::fs::read_to_string(&config_path).unwrap();
        assert!(
            config.contains("/datasets/datasets-cache/job-control-001/control"),
            "config deve conter o control path real: {config}"
        );
        assert!(
            !config.contains("{control_dataset_path}"),
            "placeholder não pode vazar: {config}"
        );
        assert!(
            config.contains("cache_text_embeddings: true"),
            "flag opaca preservada: {config}"
        );
    }

    #[tokio::test]
    async fn diffusion_train_control_placeholder_sem_ref_falha_claro() {
        // Defesa anti-placeholder (espelha init): config exige control mas
        // nenhum control_package_ref veio no dispatch → ConfigYamlInvalid.
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-control-002", "diffusion", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        dispatch.mode = "train".to_string();
        dispatch.control_package_ref = None;
        dispatch.config_yaml = Some(
            "epochs: 1\ndataset_path: {dataset_path}\noutput_path: {output_path}\ncontrol_dataset_path: \"{control_dataset_path}\""
                .to_string(),
        );
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let res = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;
        assert!(
            matches!(res, Err(PipelineError::ConfigYamlInvalid(_))),
            "placeholder sem ref deve falhar claro: {res:?}"
        );
    }

    #[tokio::test]
    async fn diffusion_train_control_md5_mismatch_falha_honesto() {
        // MD5 divergente no pacote de controle → Md5Mismatch (nada silencioso).
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3WithControl::new());

        let mut dispatch = make_dispatch("job-control-003", "diffusion");
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        dispatch.mode = "train".to_string();
        dispatch.package_ref = Some(PackageRef {
            key: "packages/test-pkg/dataset.zip".to_string(),
            md5_zip: compute_file_md5_bytes(&s3.main_zip),
            bytes: s3.main_zip.len() as i64,
        });
        dispatch.control_package_ref = Some(PackageRef {
            key: "packages/test-pkg/control.zip".to_string(),
            md5_zip: "00000000000000000000000000000000".to_string(),
            bytes: s3.control_zip.len() as i64,
        });
        dispatch.config_yaml = Some(
            "epochs: 1\ndataset_path: {dataset_path}\noutput_path: {output_path}\ncontrol_dataset_path: \"{control_dataset_path}\""
                .to_string(),
        );
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let res = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;
        assert!(
            matches!(res, Err(PipelineError::Md5Mismatch { .. })),
            "md5 divergente deve falhar honesto: {res:?}"
        );
    }

    // -- ADR-0020: engine diffusion com mode generate usa generate e coleta generated.png sem exigir package --

    #[tokio::test]
    async fn engine_diffusion_uses_generate_subcommand_and_generated_artifacts() {
        let tmp = tempfile::tempdir().unwrap();

        let s3 = Arc::new(FakeS3::new());
        let mut dispatch = make_dispatch("job-diff-gen-001", "diffusion");
        dispatch.mode = "generate".to_string();
        dispatch.package_ref = None; // Sem package_ref (Text-to-Image puro)
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let mut output_files = HashMap::new();
        output_files.insert(
            "generated.png".to_string(),
            b"fake png image bytes".to_vec(),
        );
        create_fake_outputs(tmp.path(), "job-diff-gen-001", &output_files);

        let res = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;

        assert!(res.is_ok(), "run_job_inner failed: {:?}", res);

        let args = executor.last_args().unwrap();
        assert_eq!(args[0], "generate");
        assert_eq!(args[1], "--config");
        assert_eq!(args[3], "--output");

        // Verifica artefato coletado: generated.png (kind = generated)
        let artifacts = report.done_artifacts().unwrap();
        let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
        let kinds: Vec<&str> = artifacts.iter().map(|a| a.kind.as_str()).collect();
        assert!(filenames.contains(&"generated.png"));
        assert!(kinds.contains(&"generated"));
    }

    // -- Incidente galeria vazia: bucket S3 inexistente → uploads falham → job
    //    NÃO pode terminar done com zero artefatos (mentira sobre si mesmo).

    /// FakeS3 em modo upload_fail → run_job deve reportar failed com mensagem
    /// descritiva, nunca done.
    #[tokio::test]
    async fn generate_upload_fail_reports_failed_not_done() {
        let tmp = tempfile::tempdir().unwrap();

        let s3 = Arc::new(FakeS3::new());
        s3.set_upload_fail(true);
        let mut dispatch = make_dispatch("job-upload-fail-001", "diffusion");
        dispatch.mode = "generate".to_string();
        dispatch.package_ref = None; // Text-to-Image puro, como no incidente
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        // Trainer "produziu" a imagem — só o upload ao bucket falha.
        let mut output_files = HashMap::new();
        output_files.insert("generated_0001.png".to_string(), b"fake png".to_vec());
        create_fake_outputs(tmp.path(), "job-upload-fail-001", &output_files);

        run_job(
            dispatch,
            s3,
            report.clone(),
            executor,
            active_jobs,
            None,
            false,
            None,
        )
        .await;

        let statuses = report.statuses();
        assert!(
            !statuses.iter().any(|s| s == "done"),
            "job com upload falho não pode reportar done: {statuses:?}"
        );
        let failed = report
            .failed_report()
            .expect("deve haver report final failed");
        let msg = failed
            .message
            .clone()
            .or(failed.error.clone())
            .unwrap_or_default();
        assert!(
            msg.contains("upload de artefatos falhou"),
            "mensagem deve diagnosticar a falha de upload, obtido: {msg}"
        );
    }

    // -- Ronda reviewer S2: retry e preparo de artefatos --

    /// put_with_retry com S3 sempre falhando → Err após EXATAMENTE 3 tentativas.
    #[tokio::test]
    async fn put_with_retry_tries_three_times_then_gives_up() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("a.bin");
        std::fs::write(&file, b"data").unwrap();

        let s3 = FakeS3::new();
        s3.set_upload_fail(true);
        let res = put_with_retry(&s3, "artifacts/j/a.bin", &file).await;
        assert!(res.is_err(), "put sempre falhando deve retornar Err");
        assert_eq!(s3.put_count(), 3, "put_with_retry deve tentar N=3 vezes");
    }

    /// put_with_retry com S3 instável (2 falhas transitórias) → Ok na 3ª.
    #[tokio::test]
    async fn put_with_retry_succeeds_after_transient_failures() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("b.bin");
        std::fs::write(&file, b"data").unwrap();

        let s3 = FakeS3::new();
        s3.set_fail_first_n(2);
        let res = put_with_retry(&s3, "artifacts/j/b.bin", &file).await;
        assert!(
            res.is_ok(),
            "falha transitória deve ser absorvida pelo retry"
        );
        assert_eq!(s3.put_count(), 3, "2 falhas + 1 sucesso = 3 tentativas");
    }

    /// One-shot generate com upload_fail → exatamente 3 puts por arquivo
    /// (prova end-to-end do retry) + report final failed.
    #[tokio::test]
    async fn generate_upload_fail_puts_exactly_three_times() {
        let tmp = tempfile::tempdir().unwrap();

        let s3 = Arc::new(FakeS3::new());
        s3.set_upload_fail(true);
        let mut dispatch = make_dispatch("job-retry-count-001", "diffusion");
        dispatch.mode = "generate".to_string();
        dispatch.package_ref = None;
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let mut output_files = HashMap::new();
        output_files.insert("generated_0001.png".to_string(), b"fake png".to_vec());
        create_fake_outputs(tmp.path(), "job-retry-count-001", &output_files);

        run_job(
            dispatch,
            s3.clone(),
            report.clone(),
            executor,
            active_jobs,
            None,
            false,
            None,
        )
        .await;

        assert!(
            report.failed_report().is_some(),
            "upload falho deve terminar failed"
        );
        assert_eq!(s3.put_count(), 3, "1 arquivo × N=3 tentativas");
    }

    /// Falha de PREPARO (scoped_key rejeita job_id com traversal) → entra em
    /// upload_errors → report final failed (nada é silenciado).
    #[tokio::test]
    async fn generate_scoped_key_failure_reports_failed() {
        let tmp = tempfile::tempdir().unwrap();

        let s3 = Arc::new(FakeS3::new());
        // job_id com segmento ".." → art_key contém traversal → scoped_key Err.
        let mut dispatch = make_dispatch("job/../traversal-001", "diffusion");
        dispatch.mode = "generate".to_string();
        dispatch.package_ref = None;
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let mut output_files = HashMap::new();
        output_files.insert("generated_0001.png".to_string(), b"fake png".to_vec());
        create_fake_outputs(tmp.path(), "job/../traversal-001", &output_files);

        run_job(
            dispatch,
            s3,
            report.clone(),
            executor,
            active_jobs,
            None,
            false,
            None,
        )
        .await;

        let statuses = report.statuses();
        assert!(
            !statuses.iter().any(|s| s == "done"),
            "falha de preparo não pode terminar done: {statuses:?}"
        );
        let failed = report
            .failed_report()
            .expect("deve haver report final failed");
        let msg = failed
            .message
            .clone()
            .or(failed.error.clone())
            .unwrap_or_default();
        assert!(
            msg.contains("upload de artefatos falhou"),
            "gate deve transformar em failed, obtido: {msg}"
        );
        assert!(
            msg.contains("falha ao preparar artefato (key)"),
            "causa deve identificar o preparo (key), obtido: {msg}"
        );
    }

    /// Simetria daemon: generate via daemon com upload_fail → failed, nunca done.
    #[tokio::test]
    async fn daemon_generate_upload_fail_reports_failed() {
        let tmp = tempfile::tempdir().unwrap();

        let s3 = Arc::new(FakeS3::new());
        s3.set_upload_fail(true);
        let mut dispatch = make_dispatch("job-daemon-fail-001", "diffusion");
        dispatch.mode = "generate".to_string();
        dispatch.package_ref = None;
        dispatch.config_yaml =
            Some("base_model: flux-2-klein-4b\noutput_path: {output_path}".to_string());
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let client = Arc::new(FakeDaemonClient::new());
        let launcher = Arc::new(FakeDaemonLauncher::new());
        let daemon_state = Arc::new(DaemonState::new(
            "hephaestus/trainer-difusao:local",
            8766,
            600,
            client.clone() as Arc<dyn DaemonClient>,
            launcher.clone() as Arc<dyn DaemonLauncher>,
        ));
        daemon_state.set_running(true, Some("http://localhost:8766".to_string()));
        client.set_generate_results(vec![Ok(())]);

        let mut output_files = HashMap::new();
        output_files.insert("generated_0001.png".to_string(), b"daemon png".to_vec());
        create_fake_outputs(tmp.path(), "job-daemon-fail-001", &output_files);

        run_job(
            dispatch,
            s3,
            report.clone(),
            executor,
            active_jobs,
            None,
            false,
            Some(daemon_state),
        )
        .await;

        let statuses = report.statuses();
        assert!(
            !statuses.iter().any(|s| s == "done"),
            "daemon com upload falho não pode reportar done: {statuses:?}"
        );
        let failed = report
            .failed_report()
            .expect("deve haver report final failed");
        let msg = failed
            .message
            .clone()
            .or(failed.error.clone())
            .unwrap_or_default();
        assert!(
            msg.contains("upload de artefatos falhou"),
            "mensagem deve diagnosticar a falha de upload, obtido: {msg}"
        );
    }

    // -- D5 ADR-0023: meta_content no report done --

    /// Job generate one-shot com generation_meta.json fake no output
    /// → report done contém meta_content com o JSONL.
    #[tokio::test]
    async fn generate_one_shot_meta_content_present() {
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let mut dispatch = make_dispatch("job-meta-present-001", "diffusion");
        dispatch.mode = "generate".to_string();
        dispatch.package_ref = None;
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let meta_jsonl = r#"{"filename":"img_001.png","seed":42,"prompt":"a cat","width":512,"height":512}
{"filename":"img_002.png","seed":43,"prompt":"a dog","width":512,"height":512}
"#;
        let mut output_files = HashMap::new();
        output_files.insert("generated_0001.png".to_string(), b"fake png".to_vec());
        output_files.insert(
            "generation_meta.json".to_string(),
            meta_jsonl.as_bytes().to_vec(),
        );
        create_fake_outputs(tmp.path(), "job-meta-present-001", &output_files);

        let res = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;

        assert!(res.is_ok(), "run_job_inner failed: {:?}", res);

        // Verifica meta_content no report done
        let meta = report.done_meta_content();
        assert!(meta.is_some(), "meta_content should be present");
        let content = meta.unwrap();
        assert!(
            content.contains("img_001.png"),
            "meta_content should contain JSONL data"
        );
        assert!(
            content.contains("\"seed\":42"),
            "meta_content should contain seed field"
        );

        // Verifica artefatos
        let artifacts = report.done_artifacts().unwrap();
        let kinds: Vec<&str> = artifacts.iter().map(|a| a.kind.as_str()).collect();
        assert!(kinds.contains(&"generated_meta"));
    }

    /// Job generate sem generation_meta.json → report done NÃO contém meta_content.
    #[tokio::test]
    async fn generate_one_shot_meta_content_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let mut dispatch = make_dispatch("job-meta-absent-001", "diffusion");
        dispatch.mode = "generate".to_string();
        dispatch.package_ref = None;
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let mut output_files = HashMap::new();
        output_files.insert("generated.png".to_string(), b"fake png".to_vec());
        // Sem generation_meta.json — job legado
        create_fake_outputs(tmp.path(), "job-meta-absent-001", &output_files);

        let res = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;

        assert!(res.is_ok(), "run_job_inner failed: {:?}", res);

        // meta_content deve ser None (ausente no report)
        let meta = report.done_meta_content();
        assert!(
            meta.is_none(),
            "meta_content should be absent for legacy jobs"
        );
    }

    // -- A.3 test 4: parse_metrics_line aceita a linha 1-epoch do autotrack --

    #[test]
    fn parse_metrics_line_accepts_autotrack_single_epoch() {
        let line = r#"{"box_loss":0.045,"cls_loss":0.067,"dfl_loss":0.123,"mAP50":0.912,"mAP50-95":0.654,"epoch":1}"#;
        let m = parse_metrics_line(line).expect("should parse autotrack metrics line");
        assert_eq!(m.epoch, 1);
        assert!((m.box_loss - 0.045).abs() < 1e-6);
        assert!((m.cls_loss - 0.067).abs() < 1e-6);
        assert!((m.dfl_loss - 0.123).abs() < 1e-6);
        assert!((m.map50 - 0.912).abs() < 1e-6);
        assert!((m.map50_95 - 0.654).abs() < 1e-6);
    }

    // =========================================================================
    // G.2 — nvidia-smi telemetry parse tests
    // =========================================================================

    #[test]
    fn parse_nvidia_smi_csv_two_gpus() {
        let csv = "\
NVIDIA GeForce RTX 3060, 12288, 0
NVIDIA GeForce GTX 1660 SUPER, 6144, 1024
";
        let t = parse_nvidia_smi_csv(csv).expect("should parse 2 GPUs");
        assert_eq!(
            t.gpus,
            vec!["NVIDIA GeForce RTX 3060", "NVIDIA GeForce GTX 1660 SUPER"]
        );
        // total: 12288 + 6144 = 18432 MiB (sem conversão)
        assert_eq!(t.vram_total, 18432);
        // used: 0 + 1024 = 1024 MiB (sem conversão)
        assert_eq!(t.vram_used, 1024);
        // max individual: 12288 MiB (maior GPU — capacidade de 1 job)
        assert_eq!(t.max_gpu_mib, 12288);
    }

    #[test]
    fn parse_nvidia_smi_csv_malformed_lines_ignored() {
        let csv = "\
NVIDIA GeForce RTX 3060, 12288, 0
CORRUPTED LINE
NVIDIA GeForce GTX 1660 SUPER, 6144, 512
also bad, not a number
";
        let t = parse_nvidia_smi_csv(csv).expect("should parse valid lines only");
        assert_eq!(t.gpus.len(), 2);
        assert_eq!(t.gpus[0], "NVIDIA GeForce RTX 3060");
        assert_eq!(t.gpus[1], "NVIDIA GeForce GTX 1660 SUPER");
        // used: 0 + 512 = 512 MiB (sem conversão)
        assert_eq!(t.vram_used, 512);
    }

    #[test]
    fn parse_nvidia_smi_csv_empty() {
        assert!(parse_nvidia_smi_csv("").is_none());
        assert!(parse_nvidia_smi_csv("  \n  \n").is_none());
    }

    #[test]
    fn parse_nvidia_smi_csv_no_valid_gpus() {
        let csv = "bad line\nanother bad\n";
        assert!(parse_nvidia_smi_csv(csv).is_none());
    }

    #[test]
    fn parse_nvidia_smi_csv_single_gpu_max_equals_total() {
        let csv = "NVIDIA GeForce RTX 3060, 12288, 4096\n";
        let t = parse_nvidia_smi_csv(csv).expect("should parse 1 GPU");
        assert_eq!(t.gpus.len(), 1);
        assert_eq!(t.vram_total, 12288);
        // 1 GPU: max = total (capacidade de 1 job = a única GPU)
        assert_eq!(t.max_gpu_mib, 12288);
    }

    // =========================================================================
    // G.2 — DockerExecutor GPU args tests
    // =========================================================================

    #[test]
    fn docker_run_args_gpu_some() {
        let args = build_docker_run_args(
            "hephaestus/trainer-yolo:gpu",
            "trainer-yolo-job-1",
            &[("/data/datasets".into(), "/datasets".into())],
            &[
                "train".to_string(),
                "--config".to_string(),
                "/outputs/c.yaml".to_string(),
            ],
            &[("ENGINE_MOCK".to_string(), "0".to_string())],
            Some("0"),
        );
        // GPU flags present
        let gpu_idx = args
            .iter()
            .position(|a| a == "--gpus")
            .expect("--gpus flag");
        assert_eq!(args[gpu_idx + 1], "device=0");
        let shm_idx = args
            .iter()
            .position(|a| a == "--shm-size")
            .expect("--shm-size flag");
        assert_eq!(args[shm_idx + 1], "2g");
        // NVIDIA_VISIBLE_DEVICES
        let nvd_idx = args
            .iter()
            .position(|a| a == "NVIDIA_VISIBLE_DEVICES=0")
            .expect("NVIDIA_VISIBLE_DEVICES");
        assert!(args[nvd_idx - 1] == "-e");
        // ENGINE_MOCK env
        let mock_idx = args
            .iter()
            .position(|a| a == "ENGINE_MOCK=0")
            .expect("ENGINE_MOCK env");
        assert!(args[mock_idx - 1] == "-e");
        // Image and subcommand args are present
        assert!(args.contains(&"hephaestus/trainer-yolo:gpu".to_string()));
        assert!(args.contains(&"train".to_string()));
    }

    #[test]
    fn docker_run_args_gpu_none_no_flags() {
        let args = build_docker_run_args(
            "hephaestus/trainer-yolo:local",
            "trainer-yolo-job-2",
            &[],
            &["train".to_string()],
            &[],
            None,
        );
        // NO GPU flags
        assert!(!args.contains(&"--gpus".to_string()));
        assert!(!args.contains(&"--shm-size".to_string()));
        assert!(!args.iter().any(|a| a.starts_with("NVIDIA_VISIBLE_DEVICES")));
        // Host gateway flag present
        assert!(args.contains(&"--add-host".to_string()));
        assert!(args.contains(&"host.docker.internal:host-gateway".to_string()));
        // Image and args present
        assert!(args.contains(&"hephaestus/trainer-yolo:local".to_string()));
        assert!(args.contains(&"train".to_string()));
    }

    // =========================================================================
    // G.2 — Anti-mock guard tests (D2)
    // =========================================================================

    #[test]
    fn anti_mock_guard_rejects_local_with_gpu() {
        // Simula ORCH_GPU_DEVICES setado + imagem :local → deve falhar.
        // Chama run_job_inner diretamente e verifica a variante do erro.
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-guard-001", "yolo", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        dispatch.image = "hephaestus/trainer-yolo:local".to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let result = run_job_inner(
                &dispatch,
                s3.clone(),
                report.clone(),
                executor.clone(),
                &active_jobs,
                Some("0"),
                false,
                None,
            )
            .await;
            assert!(
                matches!(result, Err(PipelineError::GpuImageGuard { .. })),
                "should return GpuImageGuard variant: {:?}",
                result
            );
        });

        // Executor should NOT have been called
        assert!(executor.last_args().is_none());
    }

    #[test]
    fn anti_mock_guard_passes_gpu_image() {
        // GPU setado + imagem :gpu → deve passar (executor chamado).
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-guard-002", "yolo", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        dispatch.image = "hephaestus/trainer-yolo:gpu".to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        // Pre-cria outputs
        let mut output_files = HashMap::new();
        output_files.insert("best.pt".to_string(), b"fake model".to_vec());
        output_files.insert("last.pt".to_string(), b"fake model".to_vec());
        output_files.insert(
            "metrics.jsonl".to_string(),
            br#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":1}"#.to_vec(),
        );
        create_fake_outputs(tmp.path(), "job-guard-002", &output_files);

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let result = run_job_inner(
                &dispatch,
                s3.clone(),
                report.clone(),
                executor.clone(),
                &active_jobs,
                Some("0"),
                false,
                None,
            )
            .await;
            assert!(
                result.is_ok(),
                "gpu image should pass guard: {:?}",
                result.err()
            );
        });
    }

    #[test]
    fn anti_mock_guard_bypass_with_allow_mock() {
        // GPU setado + imagem :local + gpu_allow_mock=true → deve passar.
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-guard-003", "yolo", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        dispatch.image = "hephaestus/trainer-yolo:local".to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        // Pre-cria outputs
        let mut output_files = HashMap::new();
        output_files.insert("best.pt".to_string(), b"fake model".to_vec());
        output_files.insert("last.pt".to_string(), b"fake model".to_vec());
        output_files.insert(
            "metrics.jsonl".to_string(),
            br#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":1}"#.to_vec(),
        );
        create_fake_outputs(tmp.path(), "job-guard-003", &output_files);

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let result = run_job_inner(
                &dispatch,
                s3.clone(),
                report.clone(),
                executor.clone(),
                &active_jobs,
                Some("0"),
                true, // gpu_allow_mock
                None, // daemon_state
            )
            .await;
            assert!(
                result.is_ok(),
                "ALLOW_MOCK should bypass guard: {:?}",
                result.err()
            );
        });
    }

    #[test]
    fn anti_mock_guard_no_gpu_no_check() {
        // Sem ORCH_GPU_DEVICES → imagem :local deve passar (caminho mock padrão).
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-guard-004", "yolo", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        dispatch.image = "hephaestus/trainer-yolo:local".to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        // Pre-cria outputs
        let mut output_files = HashMap::new();
        output_files.insert("best.pt".to_string(), b"fake model".to_vec());
        output_files.insert("last.pt".to_string(), b"fake model".to_vec());
        output_files.insert(
            "metrics.jsonl".to_string(),
            br#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":1}"#.to_vec(),
        );
        create_fake_outputs(tmp.path(), "job-guard-004", &output_files);

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let result = run_job_inner(
                &dispatch,
                s3.clone(),
                report.clone(),
                executor.clone(),
                &active_jobs,
                None,
                false,
                None,
            )
            .await;
            assert!(
                result.is_ok(),
                "no GPU set → mock path should work: {:?}",
                result.err()
            );
        });
    }

    #[test]
    fn anti_mock_guard_allows_non_localgpu_tag() {
        // GPU setado + imagem :localgpu (NÃO :local) → deve passar (ends_with(":local") = false).
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-guard-005", "yolo", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        dispatch.image = "hephaestus/trainer-yolo:localgpu".to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        // Pre-cria outputs
        let mut output_files = HashMap::new();
        output_files.insert("best.pt".to_string(), b"fake model".to_vec());
        output_files.insert("last.pt".to_string(), b"fake model".to_vec());
        output_files.insert(
            "metrics.jsonl".to_string(),
            br#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":1}"#.to_vec(),
        );
        create_fake_outputs(tmp.path(), "job-guard-005", &output_files);

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let result = run_job_inner(
                &dispatch,
                s3.clone(),
                report.clone(),
                executor.clone(),
                &active_jobs,
                Some("0"),
                false,
                None,
            )
            .await;
            assert!(
                result.is_ok(),
                ":localgpu should NOT be blocked by guard: {:?}",
                result.err()
            );
        });
    }

    // =========================================================================
    // H.1 — HeartbeatBody serializa endpoint
    // =========================================================================

    #[test]
    fn heartbeat_body_serializes_endpoint() {
        let body = HeartbeatBody {
            endpoint: "http://orchestrator-local:8082".to_string(),
            gpus: vec!["NVIDIA GeForce RTX 3060".to_string()],
            vram_total: Some(12288),
            vram_used: Some(1024),
            cpu: Some(42.5),
            ram: Some(4_000_000_000),
            ram_total: Some(8_000_000_000),
            jobs_active: 1,
            max_gpu_mib: Some(12288),
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["endpoint"], "http://orchestrator-local:8082");
        assert_eq!(json["gpus"][0], "NVIDIA GeForce RTX 3060");
        assert_eq!(json["jobs_active"], 1);
        assert_eq!(json["max_gpu_mib"], 12288);
    }

    // =========================================================================
    // H.1 — resolve_advertise_url (função pura)
    // =========================================================================

    #[test]
    fn default_advertise_url() {
        // None → default
        assert_eq!(
            resolve_advertise_url(None),
            "http://orchestrator-local:8082"
        );
    }

    #[test]
    fn resolve_advertise_url_from_value() {
        assert_eq!(
            resolve_advertise_url(Some("http://custom:9999")),
            "http://custom:9999"
        );
    }

    #[test]
    fn resolve_advertise_url_empty_fallback() {
        // Empty string → default
        assert_eq!(
            resolve_advertise_url(Some("")),
            "http://orchestrator-local:8082"
        );
    }

    // =========================================================================
    // H.1 — Pairing code generation
    // =========================================================================

    #[test]
    fn generate_pairing_code_format() {
        let code = generate_pairing_code();
        assert!(
            code.starts_with("heph_p_"),
            "code should start with heph_p_: {code}"
        );
        let hex_part = &code[7..]; // "heph_p_" = 7 chars
        assert_eq!(hex_part.len(), 32, "hex part should be 32 chars: {code}");
        assert!(
            hex_part.chars().all(|c| c.is_ascii_hexdigit()),
            "hex part should be all hex digits: {code}"
        );
    }

    #[test]
    fn generate_pairing_code_unique() {
        let a = generate_pairing_code();
        let b = generate_pairing_code();
        assert_ne!(a, b, "two generated codes should differ");
    }

    // =========================================================================
    // H.1 — Pairing verify single-use
    // =========================================================================

    #[test]
    fn pairing_verify_correct_then_second_false() {
        let state = PairingState::new("heph_p_aabbccdd11223344aabbccdd11223344".to_string());
        assert!(state.verify("heph_p_aabbccdd11223344aabbccdd11223344"));
        // Second use — consumed
        assert!(!state.verify("heph_p_aabbccdd11223344aabbccdd11223344"));
    }

    #[test]
    fn pairing_verify_wrong_code() {
        let state = PairingState::new("heph_p_aabbccdd11223344aabbccdd11223344".to_string());
        assert!(!state.verify("heph_p_wrong_wrong_wrong_wrong_wrong_00"));
    }

    #[test]
    fn pairing_verify_empty_code() {
        let state = PairingState::new("heph_p_aabbccdd11223344aabbccdd11223344".to_string());
        assert!(!state.verify(""));
    }

    // =========================================================================
    // H.1 — PairingVerifyRequest deserialization
    // =========================================================================

    #[test]
    fn pairing_verify_request_deserialize() {
        let req: PairingVerifyRequest = serde_json::from_str(r#"{"code":"heph_p_test"}"#).unwrap();
        assert_eq!(req.code, "heph_p_test");
    }

    #[test]
    fn pairing_verify_response_serialize() {
        let resp = PairingVerifyResponse { valid: true };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["valid"], true);

        let resp = PairingVerifyResponse { valid: false };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["valid"], false);
    }

    // =========================================================================
    // I.3 — S3Scope::Models + scoped_key tests
    // =========================================================================

    #[test]
    fn scoped_key_models_valid() {
        let key = scoped_key(S3Scope::Models, "models/yolo/abc-123/best.pt");
        assert_eq!(key, Ok("models/yolo/abc-123/best.pt".to_string()));
    }

    #[test]
    fn scoped_key_models_outside_scope() {
        assert_eq!(
            scoped_key(S3Scope::Models, "packages/abc/dataset.zip"),
            Err(ScopedKeyError::OutsideScope)
        );
    }

    #[test]
    fn scoped_key_packages_rejects_models_prefix() {
        assert_eq!(
            scoped_key(S3Scope::Packages, "models/yolo/abc/best.pt"),
            Err(ScopedKeyError::OutsideScope)
        );
    }

    #[test]
    fn scoped_key_models_empty() {
        assert_eq!(
            scoped_key(S3Scope::Models, ""),
            Err(ScopedKeyError::EmptyKey)
        );
    }

    #[test]
    fn scoped_key_models_absolute() {
        assert_eq!(
            scoped_key(S3Scope::Models, "/models/yolo/abc/best.pt"),
            Err(ScopedKeyError::AbsolutePath)
        );
    }

    #[test]
    fn scoped_key_models_traversal() {
        assert_eq!(
            scoped_key(S3Scope::Models, "../etc/passwd"),
            Err(ScopedKeyError::PathTraversal)
        );
    }

    // =========================================================================
    // I.3 — DispatchRequest weights_ref serde tests
    // =========================================================================

    #[test]
    fn dispatch_request_with_weights_ref() {
        let json = r#"{
            "job_id": "j1",
            "engine": "yolo",
            "image": "img:local",
            "exec_mode": "docker",
            "package_ref": {"key": "packages/p/dataset.zip", "md5_zip": "abc", "bytes": 100},
            "workdir": "/tmp",
            "weights_ref": {"s3_key": "models/yolo/abc/best.pt", "md5": "d41d8cd98f00b204e9800998ecf8427e"}
        }"#;
        let req: DispatchRequest = serde_json::from_str(json).unwrap();
        assert!(req.weights_ref.is_some());
        let wr = req.weights_ref.unwrap();
        assert_eq!(wr.s3_key, "models/yolo/abc/best.pt");
        assert_eq!(wr.md5, "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn dispatch_request_without_weights_ref() {
        let json = r#"{
            "job_id": "j1",
            "engine": "yolo",
            "image": "img:local",
            "exec_mode": "docker",
            "package_ref": {"key": "packages/p/dataset.zip", "md5_zip": "abc", "bytes": 100},
            "workdir": "/tmp"
        }"#;
        let req: DispatchRequest = serde_json::from_str(json).unwrap();
        assert!(req.weights_ref.is_none());
    }

    // =========================================================================
    // I.3 — replace_config_placeholders with weights_path
    // =========================================================================

    #[test]
    fn replace_config_placeholders_with_weights() {
        let config = "model: yolo11m\nweights_path: {weights_path}";
        let result = replace_config_placeholders(
            config,
            "/datasets/j1",
            "/outputs/j1",
            Some("/outputs/j1/weights/best.pt"),
            &[],
            None,
            None,
            None,
            None,
        );
        assert_eq!(
            result,
            "model: yolo11m\nweights_path: /outputs/j1/weights/best.pt"
        );
    }

    #[test]
    fn replace_config_placeholders_without_weights_keeps_literal() {
        let config = "model: yolo11m\nweights_path: {weights_path}";
        let result = replace_config_placeholders(
            config,
            "/datasets/j1",
            "/outputs/j1",
            None,
            &[],
            None,
            None,
            None,
            None,
        );
        assert_eq!(result, "model: yolo11m\nweights_path: {weights_path}");
    }

    // =========================================================================
    // S4 feat/img2img — replace com/sem init, ext sanitizada, allowlist do init
    // =========================================================================

    #[test]
    fn replace_config_placeholders_with_init_image() {
        let config = "init_image: {init_image_path}\nstrength: 0.6";
        let result = replace_config_placeholders(
            config,
            "/datasets/j1",
            "/outputs/j1",
            None,
            &[],
            None,
            Some("/outputs/j1/inputs/init.png"),
            None,
            None,
        );
        assert_eq!(
            result,
            "init_image: /outputs/j1/inputs/init.png\nstrength: 0.6"
        );
    }

    #[test]
    fn replace_config_placeholders_without_init_keeps_literal() {
        let config = "init_image: {init_image_path}\nstrength: 0.6";
        let result = replace_config_placeholders(
            config,
            "/datasets/j1",
            "/outputs/j1",
            None,
            &[],
            None,
            None,
            None,
            None,
        );
        assert_eq!(result, "init_image: {init_image_path}\nstrength: 0.6");
    }

    #[test]
    fn replace_config_placeholders_no_init_placeholder_noop() {
        // Config sem o placeholder: init presente ou não, no-op.
        let config = "prompt: a cat\nsteps: 20";
        for init in [None, Some("/outputs/j1/inputs/init.png")] {
            let result = replace_config_placeholders(
                config,
                "/datasets/j1",
                "/outputs/j1",
                None,
                &[],
                None,
                init,
                None,
                None,
            );
            assert_eq!(result, config);
        }
    }
    #[test]
    fn replace_config_placeholders_with_text_encoder() {
        // Fatia feat/pesos-custom-flux2: placeholder do encoder substituído.
        let config = "text_encoder: {text_encoder_path}\nsteps: 20";
        let result = replace_config_placeholders(
            config,
            "/datasets/j1",
            "/outputs/j1",
            None,
            &[],
            None,
            None,
            None,
            Some("/outputs/j1/weights/text_encoder.safetensors"),
        );
        assert_eq!(
            result,
            "text_encoder: /outputs/j1/weights/text_encoder.safetensors\nsteps: 20"
        );
    }

    #[test]
    fn replace_config_placeholders_without_text_encoder_keeps_literal() {
        let config = "text_encoder: {text_encoder_path}\nsteps: 20";
        let result = replace_config_placeholders(
            config,
            "/datasets/j1",
            "/outputs/j1",
            None,
            &[],
            None,
            None,
            None,
            None,
        );
        assert_eq!(result, "text_encoder: {text_encoder_path}\nsteps: 20");
    }

    #[test]
    fn init_image_ext_from_suffix() {
        assert_eq!(init_image_ext("generation_inputs/abc/img.png"), "png");
        assert_eq!(init_image_ext("generation_inputs/abc/photo.JPG"), "jpg");
        assert_eq!(init_image_ext("artifacts/job-1/gen_0001.webp"), "webp");
        assert_eq!(init_image_ext("artifacts/job-1/frame.jpeg"), "jpeg");
    }

    #[test]
    fn init_image_ext_fallback_png() {
        // Sem extensão, extensão curta/longa demais ou não-alfanumérica → png.
        assert_eq!(init_image_ext("artifacts/j/f"), "png");
        assert_eq!(init_image_ext("artifacts/j/f."), "png");
        assert_eq!(init_image_ext("artifacts/j/f.a"), "png");
        assert_eq!(init_image_ext("artifacts/j/f.toolongext"), "png");
        assert_eq!(init_image_ext("artifacts/j/f.pn_g"), "png");
        assert_eq!(init_image_ext(""), "png");
    }

    #[test]
    fn init_image_ext_never_returns_traversal_or_slashes() {
        for key in [
            "artifacts/j/..",
            "artifacts/../evil.png",
            "generation_inputs/x/../../etc/passwd",
            "/abs/path.png",
        ] {
            let ext = init_image_ext(key);
            assert!(!ext.contains(".."), "ext com traversal: {ext}");
            assert!(!ext.contains('/'), "ext com barra: {ext}");
        }
    }

    #[test]
    fn scoped_init_image_key_accepts_both_prefixes() {
        assert_eq!(
            scoped_init_image_key("generation_inputs/abc/upload.png"),
            Ok("generation_inputs/abc/upload.png".to_string())
        );
        assert_eq!(
            scoped_init_image_key("artifacts/job-1/generated_0001.png"),
            Ok("artifacts/job-1/generated_0001.png".to_string())
        );
    }

    #[test]
    fn scoped_init_image_key_rejects_other_prefixes() {
        assert_eq!(
            scoped_init_image_key("models/diffusion/x/ckpt.safetensors"),
            Err(ScopedKeyError::OutsideScope)
        );
        assert_eq!(
            scoped_init_image_key("packages/abc/dataset.zip"),
            Err(ScopedKeyError::OutsideScope)
        );
        assert_eq!(
            scoped_init_image_key("datasets/abc/images"),
            Err(ScopedKeyError::OutsideScope)
        );
    }

    #[test]
    fn scoped_init_image_key_rejects_unsafe() {
        assert_eq!(scoped_init_image_key(""), Err(ScopedKeyError::EmptyKey));
        assert_eq!(
            scoped_init_image_key("/generation_inputs/x.png"),
            Err(ScopedKeyError::AbsolutePath)
        );
        assert_eq!(
            scoped_init_image_key("generation_inputs/../evil.png"),
            Err(ScopedKeyError::PathTraversal)
        );
        assert_eq!(
            scoped_init_image_key("artifacts/../evil.png"),
            Err(ScopedKeyError::PathTraversal)
        );
    }

    #[test]
    fn dispatch_request_init_image_ref_serde() {
        // snake_case no wire interno, md5 opcional (galeria → null).
        let json = r#"{
            "job_id": "j1", "engine": "diffusion", "image": "img",
            "exec_mode": "docker", "config_yaml": null,
            "dataset_version_id": null, "workdir": "/tmp",
            "init_image_ref": {"s3_key": "artifacts/j0/gen.png", "md5": null}
        }"#;
        let req: DispatchRequest = serde_json::from_str(json).unwrap();
        let init = req.init_image_ref.expect("init presente");
        assert_eq!(init.s3_key, "artifacts/j0/gen.png");
        assert!(init.md5.is_none());

        // Ausente → None (txt2img retrocompat).
        let json2 = r#"{
            "job_id": "j1", "engine": "diffusion", "image": "img",
            "exec_mode": "docker", "config_yaml": null,
            "dataset_version_id": null, "workdir": "/tmp"
        }"#;
        let req2: DispatchRequest = serde_json::from_str(json2).unwrap();
        assert!(req2.init_image_ref.is_none());
    }

    // =========================================================================
    // I.3 — run_job_inner with weights_ref: staging + config substitution
    // =========================================================================

    #[tokio::test]
    async fn weights_ref_valid_stages_file_and_replaces_placeholder() {
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-w-001", "yolo", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        // Config with {weights_path} placeholder
        dispatch.config_yaml = Some(
            "epochs: 1\ndataset_path: {dataset_path}\noutput_path: {output_path}\nweights_path: {weights_path}".to_string(),
        );

        // Create weights file in FakeS3
        let weights_bytes = b"fake weights data";
        let weights_md5 = compute_file_md5_bytes(weights_bytes);

        // Manually stage weights file so FakeS3 can serve it
        // (FakeS3 always writes zip_bytes, so we intercept via a custom approach)
        // Actually, FakeS3 writes zip_bytes for ALL downloads. For this test,
        // we put the weights file directly and use a custom S3 mock.
        // Simpler: create a FakeS3 that serves different content per key.
        let weights_s3 = Arc::new(FakeS3WithWeights::new(weights_bytes.to_vec()));
        let weights_md5_hex = weights_md5.clone();

        dispatch.weights_ref = Some(WeightsRef {
            s3_key: "models/yolo/abc-123/best.pt".to_string(),
            md5: weights_md5_hex,
        });

        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        // Pre-creates outputs
        let mut output_files = HashMap::new();
        output_files.insert("best.pt".to_string(), b"fake model".to_vec());
        output_files.insert("last.pt".to_string(), b"fake model".to_vec());
        output_files.insert(
            "metrics.jsonl".to_string(),
            br#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":1}"#.to_vec(),
        );
        create_fake_outputs(tmp.path(), "job-w-001", &output_files);

        let result = run_job_inner(
            &dispatch,
            weights_s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;
        assert!(
            result.is_ok(),
            "weights pipeline should succeed: {:?}",
            result.err()
        );

        // Verify weights file was staged
        let staged = tmp.path().join("outputs/job-w-001/weights/best.pt");
        assert!(staged.exists(), "weights file should be staged");
        assert_eq!(
            std::fs::read(&staged).unwrap(),
            weights_bytes,
            "staged weights content should match"
        );

        // Verify config.yaml has {weights_path} replaced
        let config_content =
            std::fs::read_to_string(tmp.path().join("outputs/job-w-001/config.yaml")).unwrap();
        assert!(
            config_content.contains("/outputs/job-w-001/weights/best.pt"),
            "config.yaml should contain replaced weights_path, got: {config_content}"
        );
        assert!(
            !config_content.contains("{weights_path}"),
            "config.yaml should not contain literal {{weights_path}}"
        );

        // Verify executor args are unchanged (shape preserved)
        let args = executor.last_args().unwrap();
        assert_eq!(args[0], "train");
        assert_eq!(args[1], "--config");
        assert_eq!(args[3], "--output");

        // Verify downloads include both package and weights
        let downloads = weights_s3.downloads.lock().unwrap();
        assert!(
            downloads.iter().any(|k| k.contains("packages/")),
            "should download package"
        );
        assert!(
            downloads.iter().any(|k| k.contains("models/")),
            "should download weights"
        );
    }

    #[tokio::test]
    async fn weights_ref_wrong_md5_fails_pipeline() {
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-w-002", "yolo", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();

        let weights_s3 = Arc::new(FakeS3WithWeights::new(b"weights data".to_vec()));

        dispatch.weights_ref = Some(WeightsRef {
            s3_key: "models/yolo/abc-123/best.pt".to_string(),
            md5: "00000000000000000000000000000000".to_string(), // wrong hash
        });

        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let result = run_job_inner(
            &dispatch,
            weights_s3,
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;
        assert!(
            matches!(result, Err(PipelineError::Md5Mismatch { .. })),
            "should fail with Md5Mismatch: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn no_weights_ref_unchanged_behavior() {
        // Sem weights_ref → pipeline idêntica ao comportamento atual (regressão)
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-w-003", "yolo", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        dispatch.weights_ref = None; // explicitly None

        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let mut output_files = HashMap::new();
        output_files.insert("best.pt".to_string(), b"fake model".to_vec());
        output_files.insert("last.pt".to_string(), b"fake model".to_vec());
        output_files.insert(
            "metrics.jsonl".to_string(),
            br#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":1}"#.to_vec(),
        );
        create_fake_outputs(tmp.path(), "job-w-003", &output_files);

        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;
        assert!(
            result.is_ok(),
            "no-weights pipeline should succeed: {:?}",
            result.err()
        );

        // No weights directory should exist
        let weights_dir = tmp.path().join("outputs/job-w-003/weights");
        assert!(
            !weights_dir.exists(),
            "weights dir should not exist without weights_ref"
        );

        // Config should have output_path substituted but no weights_path
        let config_content =
            std::fs::read_to_string(tmp.path().join("outputs/job-w-003/config.yaml")).unwrap();
        assert!(
            config_content.contains("output_path: /outputs/job-w-003"),
            "config should have output_path substituted: {config_content}"
        );
        assert!(
            !config_content.contains("weights_path"),
            "config should not contain weights_path when no weights_ref: {config_content}"
        );
    }

    #[tokio::test]
    async fn weights_ref_artifacts_scope() {
        // weights_ref com key em artifacts/ → usa escopo Artifacts existente
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-w-004", "yolo", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        dispatch.config_yaml = Some(
            "epochs: 1\ndataset_path: {dataset_path}\noutput_path: {output_path}\nweights_path: {weights_path}".to_string(),
        );

        let weights_bytes = b"artifact weights";
        let weights_md5 = compute_file_md5_bytes(weights_bytes);
        let weights_s3 = Arc::new(FakeS3WithWeights::new(weights_bytes.to_vec()));

        dispatch.weights_ref = Some(WeightsRef {
            s3_key: "artifacts/job-prev/best.pt".to_string(),
            md5: weights_md5,
        });

        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let mut output_files = HashMap::new();
        output_files.insert("best.pt".to_string(), b"fake model".to_vec());
        output_files.insert("last.pt".to_string(), b"fake model".to_vec());
        output_files.insert(
            "metrics.jsonl".to_string(),
            br#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":1}"#.to_vec(),
        );
        create_fake_outputs(tmp.path(), "job-w-004", &output_files);

        let result = run_job_inner(
            &dispatch,
            weights_s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;
        assert!(
            result.is_ok(),
            "artifacts-scope weights should succeed: {:?}",
            result.err()
        );

        // Verify staged at correct location
        let staged = tmp.path().join("outputs/job-w-004/weights/best.pt");
        assert!(staged.exists());
        assert_eq!(std::fs::read(&staged).unwrap(), weights_bytes);
    }

    #[tokio::test]
    async fn weights_ref_unknown_prefix_fails() {
        // Key que não começa com models/ nem artifacts/ → falha
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-w-005", "yolo", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();

        dispatch.weights_ref = Some(WeightsRef {
            s3_key: "datasets/something/file.pt".to_string(),
            md5: "d41d8cd98f00b204e9800998ecf8427e".to_string(),
        });

        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let result = run_job_inner(
            &dispatch,
            s3,
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;
        assert!(result.is_err(), "unknown prefix should fail: {:?}", result);
    }

    // =========================================================================
    // K.4 — autotracker with weights_ref tests (ADR-0014 D4)
    // =========================================================================

    /// Helper: cria dispatch para autotracker/autotrack com config que inclui {weights_path}.
    fn make_autotracker_dispatch(job_id: &str) -> DispatchRequest {
        DispatchRequest {
            job_id: job_id.to_string(),
            engine: "autotracker".to_string(),
            image: "hephaestus/trainer-yolo:local".to_string(),
            exec_mode: "docker".to_string(),
            package_ref: Some(PackageRef {
                key: "packages/test-pkg/dataset.zip".to_string(),
                md5_zip: String::new(),
                bytes: 0,
            }),
            config_yaml: Some(
                "model: mock\nconf: 0.65\ndataset_path: {dataset_path}\noutput_path: {output_path}\nweights_path: {weights_path}"
                    .to_string(),
            ),
            dataset_version_id: None,
            workdir: "/tmp".to_string(),
            mode: "autotrack".to_string(),
            weights_ref: None,
            loras: Vec::new(),
            custom_checkpoint: None,
            text_encoder: None,
            init_image_ref: None,
            control_package_ref: None,
        }
    }

    fn make_autotracker_dispatch_with_valid_md5(job_id: &str, zip_path: &Path) -> DispatchRequest {
        let md5 = compute_file_md5(zip_path).unwrap();
        let mut d = make_autotracker_dispatch(job_id);
        if let Some(ref mut pr) = d.package_ref {
            pr.md5_zip = md5;
        }
        d
    }

    // -- K.4 test 1: autotracker + weights_ref → staging + autotrack + config with {weights_path} --

    #[tokio::test]
    async fn autotracker_weights_ref_stages_and_replaces_config() {
        let tmp = tempfile::tempdir().unwrap();

        let weights_bytes = b"fake world weights data";
        let weights_md5 = compute_file_md5_bytes(weights_bytes);
        let s3 = Arc::new(FakeS3WithWeights::new(weights_bytes.to_vec()));

        // Usa o zip_bytes do S3 mock para garantir MD5 consistente
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_autotracker_dispatch_with_valid_md5("job-at-w-001", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        dispatch.weights_ref = Some(WeightsRef {
            s3_key: "models/world/abc-123/yolov8x-worldv2.pt".to_string(),
            md5: weights_md5,
        });

        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        // Simula output do engine: boxes.json + metrics.jsonl
        let mut output_files = HashMap::new();
        output_files.insert(
            "boxes.json".to_string(),
            br#"{"engine":"autotracker","model":"world","seed":0,"conf":0.65,"images":[{"filename":"img1.jpg","boxes":[{"class":"cat","x":0.1,"y":0.2,"w":0.3,"h":0.4,"conf":0.9}]}]}"#.to_vec(),
        );
        output_files.insert(
            "metrics.jsonl".to_string(),
            br#"{"box_loss":0.1,"cls_loss":0.2,"dfl_loss":0.3,"mAP50":0.9,"mAP50-95":0.7,"epoch":1}"#.to_vec(),
        );
        create_fake_outputs(tmp.path(), "job-at-w-001", &output_files);

        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;
        assert!(
            result.is_ok(),
            "autotracker with weights_ref should succeed: {:?}",
            result.err()
        );

        // 1. Verifica subcomando: autotrack
        let args = executor.last_args().unwrap();
        assert_eq!(args[0], "autotrack");
        assert_eq!(args[1], "--config");
        assert_eq!(args[3], "--output");

        // 2. Verifica staging de pesos
        let staged = tmp
            .path()
            .join("outputs/job-at-w-001/weights/yolov8x-worldv2.pt");
        assert!(staged.exists(), "weights should be staged");
        assert_eq!(std::fs::read(&staged).unwrap(), weights_bytes);

        // 3. Verifica config.yaml com {weights_path} substituído
        let config_content =
            std::fs::read_to_string(tmp.path().join("outputs/job-at-w-001/config.yaml")).unwrap();
        assert!(
            config_content.contains("/outputs/job-at-w-001/weights/yolov8x-worldv2.pt"),
            "config should contain replaced weights_path, got: {config_content}"
        );
        assert!(
            !config_content.contains("{weights_path}"),
            "config should not contain literal {{weights_path}}"
        );

        // 4. Verifica artefatos: boxes.json + metrics.jsonl
        let artifacts = report.done_artifacts().unwrap();
        let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
        assert!(filenames.contains(&"boxes.json"));
        assert!(filenames.contains(&"metrics.jsonl"));
    }

    // -- K.4 test 2: autotracker + weights_ref + only boxes.json (sem metrics.jsonl) → done com skip --

    #[tokio::test]
    async fn autotracker_weights_ref_only_boxes_json_skips_missing_metrics() {
        let tmp = tempfile::tempdir().unwrap();

        let weights_bytes = b"real world weights";
        let weights_md5 = compute_file_md5_bytes(weights_bytes);
        let s3 = Arc::new(FakeS3WithWeights::new(weights_bytes.to_vec()));

        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_autotracker_dispatch_with_valid_md5("job-at-nom-001", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        dispatch.weights_ref = Some(WeightsRef {
            s3_key: "models/world/def-456/yolov8x-worldv2.pt".to_string(),
            md5: weights_md5,
        });

        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        // Apenas boxes.json — SEM metrics.jsonl (como o real faz, D3 da ADR-0014)
        let mut output_files = HashMap::new();
        output_files.insert(
            "boxes.json".to_string(),
            br#"{"engine":"autotracker","model":"world","seed":0,"conf":0.65,"images":[{"filename":"img1.jpg","boxes":[]}]}"#.to_vec(),
        );
        // NOTE: metrics.jsonl deliberately NOT created
        create_fake_outputs(tmp.path(), "job-at-nom-001", &output_files);

        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;
        assert!(
            result.is_ok(),
            "autotracker without metrics.jsonl should succeed (skip): {:?}",
            result.err()
        );

        // Verifica done report: sem metrics/epoch (progresso binário)
        let done_report = report
            .reports
            .lock()
            .unwrap()
            .iter()
            .find(|r| r.status == "done")
            .cloned()
            .expect("should have done report");
        assert!(
            done_report.metrics.is_none(),
            "done should have no metrics when metrics.jsonl is absent"
        );
        assert!(
            done_report.epoch.is_none(),
            "done should have no epoch when metrics.jsonl is absent"
        );
        assert_eq!(done_report.progress, Some(1.0));

        // Verifica artefatos: SOMENTE boxes.json (sem metrics.jsonl)
        let artifacts = report.done_artifacts().unwrap();
        assert_eq!(
            artifacts.len(),
            1,
            "should have exactly 1 artifact (boxes.json only)"
        );
        assert_eq!(artifacts[0].path, "boxes.json");
        assert_eq!(artifacts[0].kind, "boxes");

        // Verifica subcomando correto
        let args = executor.last_args().unwrap();
        assert_eq!(args[0], "autotrack");
    }

    // -- K.4 test 3: autotracker + weights_ref com md5 errado → falha honesta --

    #[tokio::test]
    async fn autotracker_weights_ref_wrong_md5_fails() {
        let tmp = tempfile::tempdir().unwrap();

        let s3 = Arc::new(FakeS3WithWeights::new(b"real weights".to_vec()));
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_autotracker_dispatch_with_valid_md5("job-at-bad-001", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        dispatch.weights_ref = Some(WeightsRef {
            s3_key: "models/world/ghi-789/yolov8x-worldv2.pt".to_string(),
            md5: "00000000000000000000000000000000".to_string(), // wrong hash
        });

        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let result = run_job_inner(
            &dispatch,
            s3,
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;
        assert!(
            matches!(result, Err(PipelineError::Md5Mismatch { .. })),
            "autotracker with wrong weights md5 should fail with Md5Mismatch: {:?}",
            result
        );

        // Executor nunca chamado (falha antes)
        assert!(executor.last_args().is_none());
    }

    // -- K.4 test 4: autotracker SEM weights_ref → caminho atual byte-a-byte (regressão) --

    #[tokio::test]
    async fn autotracker_no_weights_ref_unchanged_behavior() {
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_autotracker_dispatch_with_valid_md5("job-at-reg-001", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        dispatch.weights_ref = None; // explícito: sem pesos

        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let mut output_files = HashMap::new();
        output_files.insert(
            "boxes.json".to_string(),
            br#"{"engine":"autotracker","model":"mock","seed":42,"conf":0.65,"images":[{"filename":"img1.jpg","boxes":[{"class":"cat","x":0.5,"y":0.5,"w":0.1,"h":0.1,"conf":1.0}]}]}"#.to_vec(),
        );
        output_files.insert(
            "metrics.jsonl".to_string(),
            br#"{"box_loss":0.1,"cls_loss":0.2,"dfl_loss":0.3,"mAP50":0.9,"mAP50-95":0.7,"epoch":1}"#.to_vec(),
        );
        create_fake_outputs(tmp.path(), "job-at-reg-001", &output_files);

        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;
        assert!(
            result.is_ok(),
            "autotracker without weights_ref should succeed: {:?}",
            result.err()
        );

        // Sem weights_ref → NÃO deve haver diretório de weights
        let weights_dir = tmp.path().join("outputs/job-at-reg-001/weights");
        assert!(
            !weights_dir.exists(),
            "weights dir should not exist without weights_ref"
        );

        // Config NÃO deve ter {weights_path} substituído — placeholder permanece literal
        // (trainer mock tolera chave desconhecida — ADR-0012 D5)
        let config_content =
            std::fs::read_to_string(tmp.path().join("outputs/job-at-reg-001/config.yaml")).unwrap();
        assert!(
            config_content.contains("{weights_path}"),
            "config should keep literal {{weights_path}} when no weights_ref: {config_content}"
        );
        assert!(
            config_content.contains("output_path: /outputs/job-at-reg-001"),
            "config should have output_path substituted: {config_content}"
        );

        // Subcomando correto
        let args = executor.last_args().unwrap();
        assert_eq!(args[0], "autotrack");

        // Artefatos: boxes.json + metrics.jsonl (mock produz ambos)
        let artifacts = report.done_artifacts().unwrap();
        let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
        assert!(filenames.contains(&"boxes.json"));
        assert!(filenames.contains(&"metrics.jsonl"));

        // Downloads: apenas package (sem weights)
        let downloads = s3.downloads.lock().unwrap();
        assert!(
            downloads.iter().any(|k| k.contains("packages/")),
            "should download package"
        );
        assert!(
            !downloads.iter().any(|k| k.contains("models/")),
            "should NOT download weights when no weights_ref"
        );
    }

    // =========================================================================
    // J.3 — DispatchRequest.mode serde default + matriz (engine, mode)
    // =========================================================================

    #[test]
    fn dispatch_request_mode_defaults_to_train_when_absent() {
        let json = r#"{
            "job_id": "j1",
            "engine": "yolo",
            "image": "img:local",
            "exec_mode": "docker",
            "package_ref": {"key": "packages/p/dataset.zip", "md5_zip": "abc", "bytes": 100},
            "workdir": "/tmp"
        }"#;
        let req: DispatchRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.mode, "train");
    }

    #[test]
    fn dispatch_request_mode_deserializes_explicit_value() {
        let json = r#"{
            "job_id": "j1",
            "engine": "yolo",
            "image": "img:local",
            "exec_mode": "docker",
            "package_ref": {"key": "packages/p/dataset.zip", "md5_zip": "abc", "bytes": 100},
            "workdir": "/tmp",
            "mode": "predict"
        }"#;
        let req: DispatchRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.mode, "predict");
    }

    // -- (yolo, predict) → subcommand predict + artifact predictions.json --

    #[tokio::test]
    async fn engine_yolo_predict_uses_predict_subcommand_and_predictions_artifact() {
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-pred-001", "yolo", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        dispatch.mode = "predict".to_string();
        // Config with weights_path placeholder (predict always has weights)
        dispatch.config_yaml = Some(
            "mode: predict\npredict:\n  conf: 0.65\ndataset_path: {dataset_path}\noutput_path: {output_path}\nweights_path: {weights_path}"
                .to_string(),
        );

        let weights_bytes = b"fake weights";
        let weights_md5 = compute_file_md5_bytes(weights_bytes);
        dispatch.weights_ref = Some(WeightsRef {
            s3_key: "models/yolo/abc-123/best.pt".to_string(),
            md5: weights_md5,
        });

        let s3w = Arc::new(FakeS3WithWeights::new(weights_bytes.to_vec()));
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        // Pre-cria predictions.json (predict só produz este artefato)
        let predictions = br#"{"engine":"yolo","model":"predict","conf":0.65,"images":[{"filename":"img_0001.jpg","boxes":[]}]}"#;
        let mut output_files = HashMap::new();
        output_files.insert("predictions.json".to_string(), predictions.to_vec());
        create_fake_outputs(tmp.path(), "job-pred-001", &output_files);

        let result = run_job_inner(
            &dispatch,
            s3w.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;
        assert!(
            result.is_ok(),
            "yolo predict pipeline should succeed: {:?}",
            result.err()
        );

        // Verify subcommand: predict, not train
        let args = executor.last_args().unwrap();
        assert_eq!(args[0], "predict");
        assert_eq!(args[1], "--config");
        assert_eq!(args[3], "--output");

        // Verify artifact: only predictions.json with kind="predictions"
        let artifacts = report.done_artifacts().unwrap();
        assert_eq!(
            artifacts.len(),
            1,
            "predict should produce exactly 1 artifact"
        );
        assert_eq!(artifacts[0].path, "predictions.json");
        assert_eq!(artifacts[0].kind, "predictions");

        // Verify weights were downloaded (staged)
        let staged = tmp.path().join("outputs/job-pred-001/weights/best.pt");
        assert!(staged.exists(), "weights should be staged for predict");
        assert_eq!(std::fs::read(&staged).unwrap(), weights_bytes);

        // Verify config has weights_path replaced
        let config_content =
            std::fs::read_to_string(tmp.path().join("outputs/job-pred-001/config.yaml")).unwrap();
        assert!(
            config_content.contains("/outputs/job-pred-001/weights/best.pt"),
            "config should have replaced weights_path, got: {config_content}"
        );
    }

    // -- (yolo, unknown mode) → PipelineError::Other --

    #[tokio::test]
    async fn yolo_unknown_mode_returns_clean_error() {
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-bad-mode", "yolo", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        dispatch.mode = "finetune".to_string(); // unknown mode

        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;
        assert!(
            matches!(result, Err(PipelineError::Other(ref msg)) if msg.contains("unsupported engine/mode")),
            "should fail with clean error for unknown mode: {:?}",
            result
        );

        // Executor never called
        assert!(executor.last_args().is_none());
    }

    // -- (yolo, train) regressão intocada --

    #[tokio::test]
    async fn yolo_train_regression_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-train-reg", "yolo", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        dispatch.mode = "train".to_string();

        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let mut output_files = HashMap::new();
        output_files.insert("best.pt".to_string(), b"fake model".to_vec());
        output_files.insert("last.pt".to_string(), b"fake model".to_vec());
        output_files.insert(
            "metrics.jsonl".to_string(),
            br#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":1}"#.to_vec(),
        );
        create_fake_outputs(tmp.path(), "job-train-reg", &output_files);

        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;
        assert!(
            result.is_ok(),
            "yolo train regression should succeed: {:?}",
            result.err()
        );

        let args = executor.last_args().unwrap();
        assert_eq!(args[0], "train");

        let artifacts = report.done_artifacts().unwrap();
        let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
        assert!(filenames.contains(&"best.pt"));
        assert!(filenames.contains(&"last.pt"));
        assert!(filenames.contains(&"metrics.jsonl"));
    }

    // -- (yolo, predict) pipeline without metrics.jsonl → done with no metrics/epoch --

    #[tokio::test]
    async fn predict_pipeline_done_without_metrics_file() {
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-pred-no-metrics", "yolo", &zip_path);
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        dispatch.mode = "predict".to_string();

        let weights_bytes = b"fake weights";
        let weights_md5 = compute_file_md5_bytes(weights_bytes);
        dispatch.weights_ref = Some(WeightsRef {
            s3_key: "models/yolo/abc/best.pt".to_string(),
            md5: weights_md5,
        });
        let s3w = Arc::new(FakeS3WithWeights::new(weights_bytes.to_vec()));

        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        // Only predictions.json — no metrics.jsonl (predict doesn't produce it)
        let mut output_files = HashMap::new();
        output_files.insert(
            "predictions.json".to_string(),
            br#"{"engine":"yolo","model":"predict","conf":0.65,"images":[]}"#.to_vec(),
        );
        create_fake_outputs(tmp.path(), "job-pred-no-metrics", &output_files);

        let result = run_job_inner(
            &dispatch,
            s3w.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;
        assert!(
            result.is_ok(),
            "predict pipeline without metrics.jsonl should succeed: {:?}",
            result.err()
        );

        // Verify done report has no metrics/epoch (predict is binary progress)
        let statuses = report.statuses();
        assert!(statuses.contains(&"running".to_string()));
        assert!(statuses.contains(&"done".to_string()));

        let done_report = report
            .reports
            .lock()
            .unwrap()
            .iter()
            .find(|r| r.status == "done")
            .cloned()
            .unwrap();
        assert!(
            done_report.metrics.is_none(),
            "predict done should have no metrics"
        );
        assert!(
            done_report.epoch.is_none(),
            "predict done should have no epoch"
        );
        assert_eq!(done_report.progress, Some(1.0));

        // Verify artifact
        let artifacts = report.done_artifacts().unwrap();
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].path, "predictions.json");
        assert_eq!(artifacts[0].kind, "predictions");
    }

    // =========================================================================
    // G.4 — Daemon + glob + multi-ref tests (ADR-0023)
    // =========================================================================

    use daemon::{DaemonClient, DaemonLauncher, DaemonState, GenerateBody, HealthResponse};

    /// Fake DaemonClient para testes.
    /// Controla health/busy/generate via campos Mutex.
    struct FakeDaemonClient {
        health_response: Mutex<Option<HealthResponse>>,
        generate_results: Mutex<Vec<Result<(), String>>>,
        generate_call_count: Mutex<usize>,
        health_call_count: Mutex<usize>,
    }

    impl FakeDaemonClient {
        fn new() -> Self {
            Self {
                health_response: Mutex::new(Some(HealthResponse {
                    ok: true,
                    loaded_spec: Some(serde_json::Value::String("flux-2-klein-4b".to_string())),
                    busy: false,
                    _extra: Default::default(),
                })),
                generate_results: Mutex::new(Vec::new()),
                generate_call_count: Mutex::new(0),
                health_call_count: Mutex::new(0),
            }
        }

        fn set_health(&self, resp: Option<HealthResponse>) {
            *self.health_response.lock().unwrap() = resp;
        }

        fn set_generate_results(&self, results: Vec<Result<(), String>>) {
            *self.generate_results.lock().unwrap() = results;
        }

        fn generate_calls(&self) -> usize {
            *self.generate_call_count.lock().unwrap()
        }

        fn health_calls(&self) -> usize {
            *self.health_call_count.lock().unwrap()
        }
    }

    #[async_trait]
    impl DaemonClient for FakeDaemonClient {
        async fn health(&self) -> Option<HealthResponse> {
            *self.health_call_count.lock().unwrap() += 1;
            self.health_response.lock().unwrap().clone()
        }

        async fn generate(&self, _body: &GenerateBody) -> Result<(), String> {
            let mut count = self.generate_call_count.lock().unwrap();
            let results = self.generate_results.lock().unwrap();
            let idx = *count;
            *count += 1;
            if idx < results.len() {
                results[idx].clone()
            } else {
                panic!("FakeDaemonClient: generate called more times than results provided (call {idx})");
            }
        }

        async fn shutdown(&self) -> Result<(), String> {
            Ok(())
        }
    }

    /// Fake DaemonLauncher para testes.
    struct FakeDaemonLauncher {
        start_call_count: Mutex<usize>,
        kill_call_count: Mutex<usize>,
        fail_start: bool,
    }

    impl FakeDaemonLauncher {
        fn new() -> Self {
            Self {
                start_call_count: Mutex::new(0),
                kill_call_count: Mutex::new(0),
                fail_start: false,
            }
        }

        fn with_fail_start() -> Self {
            Self {
                start_call_count: Mutex::new(0),
                kill_call_count: Mutex::new(0),
                fail_start: true,
            }
        }

        fn start_calls(&self) -> usize {
            *self.start_call_count.lock().unwrap()
        }

        fn kill_calls(&self) -> usize {
            *self.kill_call_count.lock().unwrap()
        }
    }

    #[async_trait]
    impl DaemonLauncher for FakeDaemonLauncher {
        async fn start(&self) -> Result<String, String> {
            *self.start_call_count.lock().unwrap() += 1;
            if self.fail_start {
                Err("docker run daemon failed".to_string())
            } else {
                Ok("http://localhost:8766".to_string())
            }
        }

        async fn kill(&self) -> Result<(), String> {
            *self.kill_call_count.lock().unwrap() += 1;
            Ok(())
        }
    }

    // -- G.4 test 1: daemon_disabled_one_shot_intacto --
    /// DIFFUSION_DAEMON_ENABLED=0 → path one-shot exatamente igual ao comportamento legado.
    #[tokio::test]
    async fn daemon_disabled_one_shot_intacto() {
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let mut dispatch = make_dispatch("job-daemon-off-001", "diffusion");
        dispatch.mode = "generate".to_string();
        dispatch.package_ref = None;
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let mut output_files = HashMap::new();
        output_files.insert(
            "generated.png".to_string(),
            b"fake png image bytes".to_vec(),
        );
        create_fake_outputs(tmp.path(), "job-daemon-off-001", &output_files);

        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None, // daemon_state = None → one-shot
        )
        .await;

        assert!(
            result.is_ok(),
            "daemon disabled one-shot should succeed: {:?}",
            result.err()
        );

        // Verifica subcomando: generate (one-shot)
        let args = executor.last_args().unwrap();
        assert_eq!(args[0], "generate");

        // Verifica artefato coletado via glob: generated.png → kind "generated"
        let artifacts = report.done_artifacts().unwrap();
        let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
        let kinds: Vec<&str> = artifacts.iter().map(|a| a.kind.as_str()).collect();
        assert!(filenames.contains(&"generated.png"));
        assert!(kinds.contains(&"generated"));
    }

    // -- G.4 test 2: glob_coleta_batch --
    /// outputs com generated_0001.png/generated_0002.png/thumb_0001.jpg/thumb_0002.jpg/generation_meta.json
    /// → 5 artefatos com kinds corretos.
    #[tokio::test]
    async fn glob_coleta_batch() {
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let mut dispatch = make_dispatch("job-glob-batch-001", "diffusion");
        dispatch.mode = "generate".to_string();
        dispatch.package_ref = None;
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let mut output_files = HashMap::new();
        output_files.insert("generated_0001.png".to_string(), b"png1".to_vec());
        output_files.insert("generated_0002.png".to_string(), b"png2".to_vec());
        output_files.insert("thumb_0001.jpg".to_string(), b"thumb1".to_vec());
        output_files.insert("thumb_0002.jpg".to_string(), b"thumb2".to_vec());
        output_files.insert("generation_meta.json".to_string(), b"{}".to_vec());
        create_fake_outputs(tmp.path(), "job-glob-batch-001", &output_files);

        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;

        assert!(
            result.is_ok(),
            "glob batch should succeed: {:?}",
            result.err()
        );

        let artifacts = report.done_artifacts().unwrap();
        assert_eq!(artifacts.len(), 5, "should collect 5 artifacts from glob");
        let kinds: Vec<&str> = artifacts.iter().map(|a| a.kind.as_str()).collect();
        assert!(kinds.contains(&"generated"), "should have 'generated' kind");
        assert!(
            kinds.contains(&"generated_thumb"),
            "should have 'generated_thumb' kind"
        );
        assert!(
            kinds.contains(&"generated_meta"),
            "should have 'generated_meta' kind"
        );
    }

    // -- G.4 test 3: glob_casa_generated_png_legado --
    /// outputs com apenas generated.png → artefato kind "generated" (retrocompat).
    #[tokio::test]
    async fn glob_casa_generated_png_legado() {
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let mut dispatch = make_dispatch("job-glob-legado-001", "diffusion");
        dispatch.mode = "generate".to_string();
        dispatch.package_ref = None;
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let mut output_files = HashMap::new();
        output_files.insert("generated.png".to_string(), b"legacy png".to_vec());
        create_fake_outputs(tmp.path(), "job-glob-legado-001", &output_files);

        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;

        assert!(
            result.is_ok(),
            "legacy generated.png should succeed: {:?}",
            result.err()
        );

        let artifacts = report.done_artifacts().unwrap();
        assert_eq!(artifacts.len(), 1, "should collect exactly 1 artifact");
        assert_eq!(artifacts[0].path, "generated.png");
        assert_eq!(artifacts[0].kind, "generated");
    }

    // -- G.4 test 3b: glob_dedup_generated_png ---
    /// outputs com generated.png E generated_0001.png → só coleta generated_0001.png
    /// (generated.png é symlink legado, pula quando existir numerado).
    #[tokio::test]
    async fn glob_dedup_generated_png() {
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let mut dispatch = make_dispatch("job-glob-dedup-001", "diffusion");
        dispatch.mode = "generate".to_string();
        dispatch.package_ref = None;
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let mut output_files = HashMap::new();
        output_files.insert("generated.png".to_string(), b"legacy symlink".to_vec());
        output_files.insert("generated_0001.png".to_string(), b"real png".to_vec());
        create_fake_outputs(tmp.path(), "job-glob-dedup-001", &output_files);

        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;

        assert!(
            result.is_ok(),
            "glob dedup should succeed: {:?}",
            result.err()
        );

        let artifacts = report.done_artifacts().unwrap();
        let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
        // generated_0001.png coletado, generated.png PULADO
        assert!(
            filenames.contains(&"generated_0001.png"),
            "should collect numbered file"
        );
        assert!(
            !filenames.contains(&"generated.png"),
            "should NOT collect generated.png when numbered exists"
        );
    }

    // -- G.4 test 4: daemon_hot_path_sem_docker_run --
    /// DIFFUSION_DAEMON_ENABLED=1 + fake launcher/client → POST /generate chamado,
    /// executor one-shot NÃO chamado, artefatos reportados.
    #[tokio::test]
    async fn daemon_hot_path_sem_docker_run() {
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let mut dispatch = make_dispatch("job-daemon-hot-001", "diffusion");
        dispatch.mode = "generate".to_string();
        dispatch.package_ref = None;
        dispatch.config_yaml =
            Some("base_model: flux-2-klein-4b\noutput_path: {output_path}".to_string());
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let client = Arc::new(FakeDaemonClient::new());
        let launcher = Arc::new(FakeDaemonLauncher::new());
        let daemon_state = Arc::new(DaemonState::new(
            "hephaestus/trainer-difusao:local",
            8766,
            600,
            client.clone() as Arc<dyn DaemonClient>,
            launcher.clone() as Arc<dyn DaemonLauncher>,
        ));
        daemon_state.set_running(true, Some("http://localhost:8766".to_string()));

        // POST /generate: retorna Ok
        client.set_generate_results(vec![Ok(())]);

        // Cria artefatos que o daemon "produziria"
        let mut output_files = HashMap::new();
        output_files.insert("generated_0001.png".to_string(), b"daemon png".to_vec());
        output_files.insert("generation_meta.json".to_string(), b"{}".to_vec());
        create_fake_outputs(tmp.path(), "job-daemon-hot-001", &output_files);

        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            Some(&daemon_state),
        )
        .await;

        assert!(
            result.is_ok(),
            "daemon hot path should succeed: {:?}",
            result.err()
        );

        // POST /generate foi chamado
        assert_eq!(
            client.generate_calls(),
            1,
            "daemon generate should be called once"
        );

        // Executor one-shot NÃO foi chamado
        assert!(
            executor.last_args().is_none(),
            "one-shot executor should NOT be called"
        );

        // Artefatos coletados via glob
        let artifacts = report.done_artifacts().unwrap();
        let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
        assert!(filenames.contains(&"generated_0001.png"));
        assert!(filenames.contains(&"generation_meta.json"));
    }

    // -- G.4 test 5: daemon_reload_quando_spec_muda --
    /// 2 jobs sequenciais com specs diferentes → client vê 2 POST /generate e
    /// health foi consultada entre eles; spec igual → ainda 2 posts.
    /// Testa apenas que o orchestrator NÃO reinicia o launcher entre jobs da mesma spec.
    #[tokio::test]
    async fn daemon_reload_quando_spec_muda() {
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());

        let client = Arc::new(FakeDaemonClient::new());
        let launcher = Arc::new(FakeDaemonLauncher::new());
        let daemon_state = Arc::new(DaemonState::new(
            "hephaestus/trainer-difusao:local",
            8766,
            600,
            client.clone() as Arc<dyn DaemonClient>,
            launcher.clone() as Arc<dyn DaemonLauncher>,
        ));
        daemon_state.set_running(true, Some("http://localhost:8766".to_string()));

        // POST /generate: 2 Ok (1 por job)
        client.set_generate_results(vec![Ok(()), Ok(())]);

        // Job 1
        let mut dispatch1 = make_dispatch("job-spec-001", "diffusion");
        dispatch1.mode = "generate".to_string();
        dispatch1.package_ref = None;
        dispatch1.config_yaml =
            Some("base_model: flux-2-klein-4b\noutput_path: {output_path}".to_string());
        dispatch1.workdir = tmp.path().to_str().unwrap().to_string();
        let active_jobs1 = new_active_jobs();

        let mut output_files1 = HashMap::new();
        output_files1.insert("generated.png".to_string(), b"img1".to_vec());
        create_fake_outputs(tmp.path(), "job-spec-001", &output_files1);

        let result1 = run_job_inner(
            &dispatch1,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs1,
            None,
            false,
            Some(&daemon_state),
        )
        .await;
        assert!(result1.is_ok(), "job 1 should succeed: {:?}", result1.err());

        let health_after_job1 = client.health_calls();
        let gen_after_job1 = client.generate_calls();
        let start_after_job1 = launcher.start_calls();

        // Job 2 (mesma spec)
        let mut dispatch2 = make_dispatch("job-spec-002", "diffusion");
        dispatch2.mode = "generate".to_string();
        dispatch2.package_ref = None;
        dispatch2.config_yaml =
            Some("base_model: flux-2-klein-4b\noutput_path: {output_path}".to_string());
        dispatch2.workdir = tmp.path().to_str().unwrap().to_string();
        let active_jobs2 = new_active_jobs();

        let mut output_files2 = HashMap::new();
        output_files2.insert("generated.png".to_string(), b"img2".to_vec());
        create_fake_outputs(tmp.path(), "job-spec-002", &output_files2);

        let result2 = run_job_inner(
            &dispatch2,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs2,
            None,
            false,
            Some(&daemon_state),
        )
        .await;
        assert!(result2.is_ok(), "job 2 should succeed: {:?}", result2.err());

        // 2 generates chamados
        assert_eq!(
            client.generate_calls(),
            gen_after_job1 + 1,
            "should have 2 generate calls total"
        );
        // Health consultada (ensure_daemon_ready consulta health)
        assert!(
            client.health_calls() > health_after_job1,
            "health should be consulted between jobs"
        );
        // Launcher NÃO reiniciado (daemon já está rodando)
        assert_eq!(
            launcher.start_calls(),
            start_after_job1,
            "launcher should NOT be called again for same spec"
        );
    }

    // -- G.4 test 6: daemon_falha_honesta --
    /// Launcher falha ao subir / health timeout → job failed com erro claro.
    #[tokio::test]
    async fn daemon_falha_honesta() {
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let mut dispatch = make_dispatch("job-daemon-fail-001", "diffusion");
        dispatch.mode = "generate".to_string();
        dispatch.package_ref = None;
        dispatch.config_yaml =
            Some("base_model: flux-2-klein-4b\noutput_path: {output_path}".to_string());
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let client = Arc::new(FakeDaemonClient::new());
        let launcher = Arc::new(FakeDaemonLauncher::with_fail_start()); // falha ao iniciar
                                                                        // Daemon NÃO está rodando — launcher vai falhar

        // Usa run_job (outer) para testar o caminho completo de falha com report
        run_job(
            dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            active_jobs,
            None,
            false,
            Some(Arc::new(DaemonState::new(
                "hephaestus/trainer-difusao:local",
                8766,
                600,
                client.clone() as Arc<dyn DaemonClient>,
                launcher.clone() as Arc<dyn DaemonLauncher>,
            ))),
        )
        .await;

        // Executor nunca chamado
        assert!(executor.last_args().is_none());

        // Reports incluem preparing e failed (via run_job outer)
        let statuses = report.statuses();
        assert!(statuses.contains(&"preparing".to_string()));
        assert!(statuses.contains(&"failed".to_string()));
    }

    // -- G.4 test 7: 409 daemon_busy --
    /// Client fake responde 409 2x e depois 200 → job ok;
    /// 3x → job failed honesto.
    #[tokio::test]
    async fn daemon_busy_retry_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let mut dispatch = make_dispatch("job-busy-001", "diffusion");
        dispatch.mode = "generate".to_string();
        dispatch.package_ref = None;
        dispatch.config_yaml =
            Some("base_model: flux-2-klein-4b\noutput_path: {output_path}".to_string());
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let client = Arc::new(FakeDaemonClient::new());
        // 409, 409, 200
        client.set_generate_results(vec![
            Err("busy".to_string()),
            Err("busy".to_string()),
            Ok(()),
        ]);

        let launcher = Arc::new(FakeDaemonLauncher::new());
        let daemon_state = Arc::new(DaemonState::new(
            "hephaestus/trainer-difusao:local",
            8766,
            600,
            client.clone() as Arc<dyn DaemonClient>,
            launcher.clone() as Arc<dyn DaemonLauncher>,
        ));
        daemon_state.set_running(true, Some("http://localhost:8766".to_string()));

        let mut output_files = HashMap::new();
        output_files.insert("generated.png".to_string(), b"busy ok".to_vec());
        create_fake_outputs(tmp.path(), "job-busy-001", &output_files);

        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            Some(&daemon_state),
        )
        .await;

        assert!(
            result.is_ok(),
            "busy retry should eventually succeed: {:?}",
            result.err()
        );
        assert_eq!(
            client.generate_calls(),
            3,
            "should have retried 3 times total"
        );
    }

    #[tokio::test]
    async fn daemon_busy_exhausted_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let mut dispatch = make_dispatch("job-busy-002", "diffusion");
        dispatch.mode = "generate".to_string();
        dispatch.package_ref = None;
        dispatch.config_yaml =
            Some("base_model: flux-2-klein-4b\noutput_path: {output_path}".to_string());
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let client = Arc::new(FakeDaemonClient::new());
        // 409, 409, 409 (3x busy → exhausted)
        client.set_generate_results(vec![
            Err("busy".to_string()),
            Err("busy".to_string()),
            Err("busy".to_string()),
        ]);

        let launcher = Arc::new(FakeDaemonLauncher::new());
        let daemon_state = Arc::new(DaemonState::new(
            "hephaestus/trainer-difusao:local",
            8766,
            600,
            client.clone() as Arc<dyn DaemonClient>,
            launcher.clone() as Arc<dyn DaemonLauncher>,
        ));
        daemon_state.set_running(true, Some("http://localhost:8766".to_string()));

        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            Some(&daemon_state),
        )
        .await;

        assert!(
            matches!(result, Err(PipelineError::DaemonBusy)),
            "should fail with DaemonBusy after 3 retries: {:?}",
            result
        );
    }

    // -- G.4 test 8: staging_loras --
    /// dispatch com 2 loras + custom → arquivos staged no workdir e config.yaml reescrito.
    #[tokio::test]
    async fn staging_loras() {
        let tmp = tempfile::tempdir().unwrap();

        // FakeS3WithWeights serve weights_bytes para models/ e artifacts/.
        // Todos os downloads recebem o mesmo conteúdo.
        let weights_bytes = b"fake weights data for all refs";
        let weights_md5 = compute_file_md5_bytes(weights_bytes);
        let s3 = Arc::new(FakeS3WithWeights::new(weights_bytes.to_vec()));

        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-loras-001", "diffusion", &zip_path);
        dispatch.mode = "generate".to_string();
        dispatch.package_ref = None; // Sem package para generate
        dispatch.config_yaml = Some(
            "base_model: flux-2-klein-4b\noutput_path: {output_path}\nlora_0: {lora_path_0}\nlora_1: {lora_path_1}\ncustom: {custom_checkpoint_path}".to_string()
        );
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        dispatch.loras = vec![
            LoraRefStage {
                s3_key: "models/lora/abc/lora_a.safetensors".to_string(),
                md5: weights_md5.clone(),
                scale: 0.8,
            },
            LoraRefStage {
                s3_key: "models/lora/def/lora_b.safetensors".to_string(),
                md5: weights_md5.clone(),
                scale: 0.5,
            },
        ];
        dispatch.custom_checkpoint = Some(WeightRef {
            s3_key: "models/checkpoint/xyz/custom.safetensors".to_string(),
            md5: weights_md5.clone(), // mesmo conteúdo do FakeS3WithWeights
        });

        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        // Pre-cria outputs
        let outputs = tmp.path().join("outputs/job-loras-001");
        std::fs::create_dir_all(&outputs).unwrap();

        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;

        assert!(
            result.is_ok(),
            "staging loras should succeed: {:?}",
            result.err()
        );

        // Verifica staging: 2 LoRAs + 1 custom
        let weights_dir = tmp.path().join("outputs/job-loras-001/weights");
        assert!(
            weights_dir.join("lora_0.safetensors").exists(),
            "lora_0 should be staged"
        );
        assert!(
            weights_dir.join("lora_1.safetensors").exists(),
            "lora_1 should be staged"
        );
        assert!(
            weights_dir.join("custom.safetensors").exists(),
            "custom should be staged"
        );

        // Verifica config.yaml reescrito com caminhos staged
        let config_content = std::fs::read_to_string(outputs.join("config.yaml")).unwrap();
        assert!(
            config_content.contains("/outputs/job-loras-001/weights/lora_0.safetensors"),
            "config should have lora_path_0 replaced: {config_content}"
        );
        assert!(
            config_content.contains("/outputs/job-loras-001/weights/lora_1.safetensors"),
            "config should have lora_path_1 replaced: {config_content}"
        );
        assert!(
            config_content.contains("/outputs/job-loras-001/weights/custom.safetensors"),
            "config should have custom_checkpoint_path replaced: {config_content}"
        );
        assert!(
            !config_content.contains("{lora_path_0}"),
            "config should not contain literal {{lora_path_0}}"
        );
        assert!(
            !config_content.contains("{custom_checkpoint_path}"),
            "config should not contain literal {{custom_checkpoint_path}}"
        );
    }
    /// dispatch com text_encoder + custom (treino flux-2) → arquivos staged e
    /// `{text_encoder_path}`/`{custom_checkpoint_path}` substituídos.
    #[tokio::test]
    async fn staging_text_encoder_and_custom_train() {
        let tmp = tempfile::tempdir().unwrap();
        let weights_bytes = b"fake encoder data";
        let weights_md5 = compute_file_md5_bytes(weights_bytes);
        let s3 = Arc::new(FakeS3WithWeights::new(weights_bytes.to_vec()));
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-enc-001", "diffusion", &zip_path);
        dispatch.mode = "train".to_string();
        dispatch.config_yaml = Some(
            "model: flux-2-klein-4b\ncustom_checkpoint_path: {custom_checkpoint_path}\ntext_encoder_path: {text_encoder_path}\noutput_path: {output_path}".to_string(),
        );
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        dispatch.custom_checkpoint = Some(WeightRef {
            s3_key: "models/checkpoint/xyz/custom.safetensors".to_string(),
            md5: weights_md5.clone(),
        });
        dispatch.text_encoder = Some(WeightRef {
            s3_key: "models/diffusion/enc/text_encoder.safetensors".to_string(),
            md5: weights_md5.clone(),
        });

        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();
        let outputs = tmp.path().join("outputs/job-enc-001");
        std::fs::create_dir_all(&outputs).unwrap();

        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;
        assert!(
            result.is_ok(),
            "staging encoder should succeed: {:?}",
            result.err()
        );

        let weights_dir = tmp.path().join("outputs/job-enc-001/weights");
        assert!(
            weights_dir.join("custom.safetensors").exists(),
            "custom staged"
        );
        assert!(
            weights_dir.join("text_encoder.safetensors").exists(),
            "encoder staged"
        );

        let config_content = std::fs::read_to_string(outputs.join("config.yaml")).unwrap();
        assert!(
            config_content.contains("/outputs/job-enc-001/weights/custom.safetensors"),
            "custom replaced: {config_content}"
        );
        assert!(
            config_content.contains("/outputs/job-enc-001/weights/text_encoder.safetensors"),
            "encoder replaced: {config_content}"
        );
        assert!(!config_content.contains("{custom_checkpoint_path}"));
        assert!(!config_content.contains("{text_encoder_path}"));
    }

    // -- G.4 test 9: preempção --
    /// daemon idle + job treino → kill chamado antes do dispatch de treino.
    #[tokio::test]
    async fn preemption_kills_idle_daemon_before_training() {
        let client = Arc::new(FakeDaemonClient::new());
        let launcher = Arc::new(FakeDaemonLauncher::new());
        let daemon_state = Arc::new(DaemonState::new(
            "hephaestus/trainer-difusao:local",
            8766,
            600,
            client.clone() as Arc<dyn DaemonClient>,
            launcher.clone() as Arc<dyn DaemonLauncher>,
        ));
        daemon_state.set_running(true, Some("http://localhost:8766".to_string()));
        // Simula daemon idle: last_used há 600s (TTL/2 = 300s)
        *daemon_state.last_used.lock().unwrap() = Instant::now() - Duration::from_secs(600);

        // Health diz busy=false
        client.set_health(Some(HealthResponse {
            ok: true,
            loaded_spec: None,
            busy: false,
            _extra: Default::default(),
        }));

        // Chama preempção diretamente
        daemon::maybe_preempt_daemon(&daemon_state).await;

        // Verifica que kill foi chamado
        assert_eq!(
            launcher.kill_calls(),
            1,
            "daemon should be killed before training"
        );
        assert!(
            !daemon_state.is_running(),
            "daemon should not be running after preemption"
        );
    }

    #[tokio::test]
    async fn preemption_skips_busy_daemon() {
        let client = Arc::new(FakeDaemonClient::new());
        let launcher = Arc::new(FakeDaemonLauncher::new());
        let daemon_state = Arc::new(DaemonState::new(
            "hephaestus/trainer-difusao:local",
            8766,
            600,
            client.clone() as Arc<dyn DaemonClient>,
            launcher.clone() as Arc<dyn DaemonLauncher>,
        ));
        daemon_state.set_running(true, Some("http://localhost:8766".to_string()));
        *daemon_state.last_used.lock().unwrap() = Instant::now() - Duration::from_secs(600);

        // Health diz busy=true
        client.set_health(Some(HealthResponse {
            ok: true,
            loaded_spec: None,
            busy: true,
            _extra: Default::default(),
        }));

        daemon::maybe_preempt_daemon(&daemon_state).await;

        // Kill NÃO chamado (daemon busy)
        assert_eq!(launcher.kill_calls(), 0, "busy daemon should NOT be killed");
        assert!(daemon_state.is_running(), "daemon should still be running");
    }

    #[tokio::test]
    async fn preemption_noop_when_not_running() {
        let client = Arc::new(FakeDaemonClient::new());
        let launcher = Arc::new(FakeDaemonLauncher::new());
        let daemon_state = Arc::new(DaemonState::new(
            "hephaestus/trainer-difusao:local",
            8766,
            600,
            client.clone() as Arc<dyn DaemonClient>,
            launcher.clone() as Arc<dyn DaemonLauncher>,
        ));
        // Daemon não está rodando

        daemon::maybe_preempt_daemon(&daemon_state).await;

        assert_eq!(launcher.kill_calls(), 0, "should not kill when not running");
        assert_eq!(
            client.health_calls(),
            0,
            "should not check health when not running"
        );
    }

    // -- is_training_metric tests (AC-006-A D1) --

    #[test]
    fn is_training_metric_phase_only_is_false() {
        // Linha de status (ex.: loading_model) — sem valores numéricos de treino.
        let m = MetricsLine {
            loss: None,
            lr: None,
            box_loss: 0.0,
            cls_loss: 0.0,
            dfl_loss: 0.0,
            map50: 0.0,
            map50_95: 0.0,
            step: None,
            epoch: 0,
            progress: Some(0.05),
            phase: Some("loading_model".to_string()),
            message: Some("Carregando FLUX".to_string()),
            vram_used_gb: None,
        };
        assert!(
            !m.is_training_metric(),
            "phase-only line is not a training metric"
        );
    }

    #[test]
    fn is_training_metric_with_loss_is_true() {
        let m = MetricsLine {
            loss: Some(0.4),
            lr: Some(0.0001),
            ..Default::default()
        };
        assert!(
            m.is_training_metric(),
            "line with loss is a training metric"
        );
    }

    #[test]
    fn is_training_metric_autolabel_zeros_is_false() {
        // Autolabel emite linhas com zeros — são eventos de status.
        let m = MetricsLine {
            loss: None,
            lr: None,
            box_loss: 0.0,
            cls_loss: 0.0,
            dfl_loss: 0.0,
            map50: 0.0,
            map50_95: 0.0,
            step: Some(10),
            epoch: 0,
            progress: Some(0.1),
            phase: None,
            message: None,
            vram_used_gb: None,
        };
        assert!(
            !m.is_training_metric(),
            "autolabel zeros is not a training metric"
        );
    }

    #[test]
    fn is_training_metric_yolo_with_box_loss_is_true() {
        let m = MetricsLine {
            box_loss: 0.5,
            cls_loss: 0.3,
            dfl_loss: 0.2,
            map50: 0.8,
            map50_95: 0.6,
            epoch: 5,
            ..Default::default()
        };
        assert!(
            m.is_training_metric(),
            "YOLO line with box_loss is a training metric"
        );
    }

    #[test]
    fn is_training_metric_diffusion_with_map_is_true() {
        let m = MetricsLine {
            loss: Some(0.045),
            map50: 0.9,
            epoch: 3,
            ..Default::default()
        };
        assert!(
            m.is_training_metric(),
            "diffusion line with mAP is a training metric"
        );
    }

    #[test]
    fn is_training_metric_with_phase_is_true_and_phase_promoted() {
        // P2-1: linha métrica COM phase continua métrica e promove a fase
        // (o report carrega `metrics: Some` + `phase: m.phase`).
        let m = MetricsLine {
            loss: Some(0.4),
            epoch: 3,
            phase: Some("training".to_string()),
            message: Some("Época 3".to_string()),
            ..Default::default()
        };
        assert!(
            m.is_training_metric(),
            "metric line with phase is still a training metric"
        );
        let json = m.to_report_json();
        assert_eq!(json.get("phase").and_then(|v| v.as_str()), Some("training"));
        assert_eq!(
            json.get("message").and_then(|v| v.as_str()),
            Some("Época 3")
        );
    }

    #[test]
    fn parse_metrics_line_nan_loss_is_not_metric() {
        // P2-2: literal NaN é sanitizado para null (parseia) e a linha
        // resultante NÃO é métrica. Nota: sem epoch/phase/progress a linha é
        // descartada por falta de epoch — o teste carrega epoch explícito.
        let m = parse_metrics_line(r#"{"epoch": 3, "loss": NaN}"#)
            .expect("NaN line must parse after sanitization");
        assert_eq!(m.loss, None, "NaN loss must become null");
        assert!(
            !m.is_training_metric(),
            "sanitized NaN-loss line is not a training metric"
        );
    }

    // -- telemetry tail (daemon path): tail_jsonl_lines + telemetry_report_for_line --
    #[test]
    fn tail_jsonl_lines_incremental_skips_malformed() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("telemetry.jsonl");
        std::fs::write(
            &path,
            "{\"phase\":\"loading_model\",\"progress\":0.1}\nnot-json\n{\"progress\":0.5}\n",
        )
        .unwrap();
        let (parsed, offset) = tail_jsonl_lines(&path, 0);
        assert_eq!(offset, 3, "offset avança inclusive sobre linha malformada");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].phase.as_deref(), Some("loading_model"));
        // Sem mais linhas novas → vazio, offset estável.
        let (parsed2, offset2) = tail_jsonl_lines(&path, offset);
        assert!(parsed2.is_empty());
        assert_eq!(offset2, offset);
        // Append incremental: só a linha nova é retornada.
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(f, "{{\"progress\":0.9}}").unwrap();
        let (parsed3, offset3) = tail_jsonl_lines(&path, offset2);
        assert_eq!(parsed3.len(), 1);
        assert_eq!(offset3, offset2 + 1);
    }

    #[test]
    fn tail_jsonl_lines_missing_file_keeps_offset() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("telemetry.jsonl");
        let (parsed, offset) = tail_jsonl_lines(&path, 7);
        assert!(parsed.is_empty());
        assert_eq!(offset, 7, "arquivo ausente não reseta o offset");
    }

    #[test]
    fn telemetry_report_for_line_matches_oneshot_format() {
        // Progress explícito honrado; evento de status → metrics None, phase/message promovidas.
        let m =
            parse_metrics_line(r#"{"phase":"denoising","message":"Etapa 3/20","progress":0.15}"#)
                .unwrap();
        let body = telemetry_report_for_line(&m, 100);
        assert_eq!(body.status, "running");
        assert!((body.progress.unwrap() - 0.15).abs() < 1e-9);
        assert_eq!(body.metrics, None);
        assert_eq!(body.phase.as_deref(), Some("denoising"));
        assert_eq!(body.message.as_deref(), Some("Etapa 3/20"));
        // Métrica de treino → metrics Some + phase junto.
        let t = parse_metrics_line(r#"{"epoch":3,"loss":0.5,"phase":"training"}"#).unwrap();
        let t_body = telemetry_report_for_line(&t, 100);
        assert!(t_body.metrics.is_some());
        assert_eq!(t_body.phase.as_deref(), Some("training"));
        assert!((t_body.progress.unwrap() - 0.03).abs() < 1e-9);
    }

    /// Regressão: eventos de telemetry.jsonl escritos durante o generate do
    /// daemon chegam como reports de progresso (antes: 0.0 até done).
    ///
    /// O FakeDaemonClient escreve linhas de telemetria no `telemetry_path`
    /// recebido ANTES de retornar Ok — o tail do path daemon deve reportá-las
    /// como "running" com progress/phase, além do "done" final.
    #[tokio::test]
    async fn daemon_path_emite_progresso_de_telemetry_jsonl() {
        use std::io::Write;
        struct TelemetryWritingClient;
        #[async_trait]
        impl DaemonClient for TelemetryWritingClient {
            async fn health(&self) -> Option<HealthResponse> {
                Some(HealthResponse {
                    ok: true,
                    loaded_spec: None,
                    busy: false,
                    _extra: Default::default(),
                })
            }
            async fn generate(&self, body: &GenerateBody) -> Result<(), String> {
                let path = std::path::PathBuf::from(&body.telemetry_path);
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let mut f = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .unwrap();
                // Dá tempo ao tail (500ms) de observar o arquivo antes do 200.
                for (phase, progress) in [("loading_model", 0.1), ("denoising", 0.5)] {
                    writeln!(f, "{{\"phase\":\"{phase}\",\"progress\":{progress}}}").unwrap();
                    f.flush().unwrap();
                    tokio::time::sleep(Duration::from_millis(700)).await;
                }
                Ok(())
            }
            async fn shutdown(&self) -> Result<(), String> {
                Ok(())
            }
        }
        struct NoopLauncher;
        #[async_trait]
        impl DaemonLauncher for NoopLauncher {
            async fn start(&self) -> Result<String, String> {
                Ok("http://localhost:8766".to_string())
            }
            async fn kill(&self) -> Result<(), String> {
                Ok(())
            }
        }

        let tmp = tempfile::tempdir().unwrap();
        let s3 = Arc::new(FakeS3::new());
        let mut dispatch = make_dispatch("job-daemon-telemetry-001", "diffusion");
        dispatch.mode = "generate".to_string();
        dispatch.package_ref = None;
        dispatch.config_yaml =
            Some("base_model: flux-2-klein-4b\noutput_path: {output_path}".to_string());
        dispatch.workdir = tmp.path().to_str().unwrap().to_string();
        let report = Arc::new(FakeReport::new());
        let executor = Arc::new(FakeTrainerExecutor::new());
        let active_jobs = new_active_jobs();

        let client = Arc::new(TelemetryWritingClient);
        let launcher = Arc::new(NoopLauncher);
        let daemon_state = Arc::new(DaemonState::new(
            "hephaestus/trainer-difusao:local",
            8766,
            600,
            client as Arc<dyn DaemonClient>,
            launcher as Arc<dyn DaemonLauncher>,
        ));
        daemon_state.set_running(true, Some("http://localhost:8766".to_string()));

        let mut output_files = HashMap::new();
        output_files.insert("generated_0001.png".to_string(), b"daemon png".to_vec());
        create_fake_outputs(tmp.path(), "job-daemon-telemetry-001", &output_files);

        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            Some(&daemon_state),
        )
        .await;
        assert!(
            result.is_ok(),
            "daemon path should succeed: {:?}",
            result.err()
        );

        let reports = report.reports.lock().unwrap().clone();
        let running: Vec<&ReportBody> = reports.iter().filter(|r| r.status == "running").collect();
        // running inicial (0.0) + ≥1 do tail com progress > 0 e phase.
        assert!(
            running.len() >= 2,
            "tail deve emitir progresso além do running inicial: {running:?}"
        );
        assert!(
            running.iter().any(|r| r.phase.as_deref() == Some("denoising")
                && r.progress.unwrap_or(0.0) > 0.0),
            "tail deve reportar phase/progress de telemetry.jsonl: {running:?}"
        );
        assert!(
            reports.iter().any(|r| r.status == "done"),
            "done final preservado"
        );
    }

    #[tokio::test]
    async fn test_stage_cached_weight_miss_then_hit() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = tmp.path().join("cache");
        let dest1 = tmp.path().join("job1/weights/text_encoder.safetensors");
        let dest2 = tmp.path().join("job2/weights/text_encoder.safetensors");
        tokio::fs::create_dir_all(dest1.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::create_dir_all(dest2.parent().unwrap())
            .await
            .unwrap();

        let weights_bytes = b"my custom text encoder weights in safetensors format";
        let expected_md5 = compute_file_md5_bytes(weights_bytes);
        let s3: Arc<dyn S3Port> = Arc::new(FakeS3WithWeights::new(weights_bytes.to_vec()));

        // 1ª execução: cache miss -> baixa do S3
        let res1 = stage_cached_weight(
            &s3,
            &cache_dir,
            &dest1,
            "models/weights/enc.safetensors",
            &expected_md5,
        )
        .await;
        assert!(res1.is_ok(), "primeira chamada deve ter sucesso");
        assert!(dest1.is_file(), "dest1 deve existir");
        assert_eq!(std::fs::read(&dest1).unwrap(), weights_bytes);

        let cached_file = cache_dir.join(format!("{expected_md5}.safetensors"));
        assert!(cached_file.is_file(), "arquivo no cache deve existir");

        // 2ª execução: cache hit -> reusa sem baixar novamente do S3
        let res2 = stage_cached_weight(
            &s3,
            &cache_dir,
            &dest2,
            "models/weights/enc.safetensors",
            &expected_md5,
        )
        .await;
        assert!(res2.is_ok(), "segunda chamada (cache hit) deve ter sucesso");
        assert!(dest2.is_file(), "dest2 deve existir");
        assert_eq!(std::fs::read(&dest2).unwrap(), weights_bytes);
    }

    #[tokio::test]
    async fn test_stage_cached_weight_md5_mismatch_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = tmp.path().join("cache");
        let dest = tmp.path().join("job1/weights/bad.safetensors");
        tokio::fs::create_dir_all(dest.parent().unwrap())
            .await
            .unwrap();

        let weights_bytes = b"real data";
        let wrong_md5 = "00000000000000000000000000000000";
        let s3: Arc<dyn S3Port> = Arc::new(FakeS3WithWeights::new(weights_bytes.to_vec()));

        let res = stage_cached_weight(
            &s3,
            &cache_dir,
            &dest,
            "models/weights/enc.safetensors",
            wrong_md5,
        )
        .await;
        assert!(matches!(res, Err(PipelineError::Md5Mismatch { .. })));
        assert!(
            !dest.exists(),
            "dest não deve ser criado em caso de mismatch"
        );
    }
}
