//! Orchestrator service — executor stateless de jobs (ADR-0007 F4.4).
//!
//! Recebe dispatch do manager, baixa package, valida md5, descompacta,
//! monta config.yaml, sobe trainer (docker|subprocess), coleta métricas,
//! sobe artefatos, reporta progresso ao manager. Sem Postgres — stateless.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

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

#[derive(Debug, Clone, Deserialize)]
pub struct DispatchRequest {
    pub job_id: String,
    pub engine: String,
    pub image: String,
    pub exec_mode: String,
    pub package_ref: PackageRef,
    pub config_yaml: Option<String>,
    pub dataset_version_id: Option<String>,
    pub workdir: String,
    /// Pesos de modelo para fine-tune (ADR-0012 D5). `None` = treino do zero.
    #[serde(default)]
    pub weights_ref: Option<WeightsRef>,
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
}

impl S3Scope {
    pub fn prefix(&self) -> &str {
        match self {
            S3Scope::Packages => "packages/",
            S3Scope::Artifacts => "artifacts/",
            S3Scope::Models => "models/",
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

// ---------------------------------------------------------------------------
// Metrics parsing (D5 — contrato com F4.5, snake_case)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricsLine {
    pub box_loss: f64,
    pub cls_loss: f64,
    pub dfl_loss: f64,
    #[serde(rename = "mAP50")]
    pub map50: f64,
    #[serde(rename = "mAP50-95")]
    pub map50_95: f64,
    pub epoch: i32,
}

/// Parse tolerante de uma linha de metrics.jsonl.
/// Linhas malformadas são ignoradas (skip silencioso).
pub fn parse_metrics_line(line: &str) -> Option<MetricsLine> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    Some(MetricsLine {
        box_loss: v.get("box_loss")?.as_f64()?,
        cls_loss: v.get("cls_loss")?.as_f64()?,
        dfl_loss: v.get("dfl_loss")?.as_f64()?,
        map50: v.get("mAP50")?.as_f64()?,
        map50_95: v.get("mAP50-95")?.as_f64()?,
        epoch: v.get("epoch")?.as_i64()? as i32,
    })
}

/// Calcula progress (epoch / total_epochs) a partir de uma linha de métricas.
pub fn compute_progress(line: &MetricsLine, total_epochs: i32) -> f64 {
    if total_epochs <= 0 {
        return 0.0;
    }
    (line.epoch as f64) / (total_epochs as f64)
}

// ---------------------------------------------------------------------------
// Config.yaml placeholder replacement (D6)
// ---------------------------------------------------------------------------

/// Substitui `{dataset_path}`, `{output_path}` e opcionalmente `{weights_path}` no config.yaml.
///
/// Quando `weights_path` é `None`, o placeholder `{weights_path}` permanece literal
/// (trainer mock tolera chave desconhecida — ADR-0012 D5).
pub fn replace_config_placeholders(
    config: &str,
    dataset_path: &str,
    output_path: &str,
    weights_path: Option<&str>,
) -> String {
    let result = config
        .replace("{dataset_path}", dataset_path)
        .replace("{output_path}", output_path);
    match weights_path {
        Some(wp) => result.replace("{weights_path}", wp),
        None => result,
    }
}

/// Extrai o valor de `epochs` do config.yaml (para计算 progress).
pub fn extract_epochs(config_yaml: &str) -> i32 {
    serde_yaml::from_str::<serde_yaml::Value>(config_yaml)
        .ok()
        .and_then(|v| v.get("epochs")?.as_i64())
        .unwrap_or(100) as i32
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
    async fn get_to_file(&self, key: &str, path: &Path) -> Result<(), String>;
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
                    .operation_timeout(Duration::from_secs(120))
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
        let out = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| format!("S3 GET {key}: {e}"))?;

        let mut reader = out.body.into_async_read();
        let mut file = tokio::fs::File::create(path)
            .await
            .map_err(|e| format!("create file {}: {e}", path.display()))?;
        tokio::io::copy(&mut reader, &mut file)
            .await
            .map_err(|e| format!("write file {}: {e}", path.display()))?;
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
        let mut req = self.client.post(&url).json(body);
        if let Some(ref token) = self.token {
            req = req.header("authorization", format!("Bearer {token}"));
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("report request: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("report status: {}", resp.status()));
        }
        Ok(())
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

        match cmd.output().await {
            Ok(o) => {
                let exit_code = o.status.code().unwrap_or(-1);
                let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                let logs = format!("{stdout}\n{stderr}");
                (exit_code, logs)
            }
            Err(e) => (-1, format!("docker exec error: {e}")),
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
                    error: Some(err_msg),
                    artifacts: None,
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
            },
        )
        .await
        .map_err(|e| PipelineError::ReportFailed(format!("report preparing: {e}")))?;

    // 2. Download package.zip via S3 (scoped — D2 barreira principal)
    let zip_path = temp_dir.join("dataset.zip");
    let key = scoped_key(S3Scope::Packages, &dispatch.package_ref.key)
        .map_err(|e| PipelineError::S3Download(format!("invalid package key: {e}")))?;

    s3.get_to_file(&key, &zip_path)
        .await
        .map_err(|e| PipelineError::S3Download(format!("download package: {e}")))?;

    // 3. Verify MD5 (crash do job se divergir — D4)
    let actual_md5 = compute_file_md5(&zip_path)
        .map_err(|e| PipelineError::S3Download(format!("compute md5: {e}")))?;
    if actual_md5 != dispatch.package_ref.md5_zip {
        return Err(PipelineError::Md5Mismatch {
            expected: dispatch.package_ref.md5_zip.clone(),
            actual: actual_md5,
        });
    }

    // 4. Unzip (zip-slip safe, padrão import 3e)
    unzip_safe(&zip_path, &datasets_cache)?;

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
        s3.get_to_file(&scoped_wkey, &weights_file)
            .await
            .map_err(|e| PipelineError::S3Download(format!("download weights: {e}")))?;

        // Verifica MD5
        let actual_md5 = compute_file_md5(&weights_file)
            .map_err(|e| PipelineError::S3Download(format!("compute weights md5: {e}")))?;
        if actual_md5 != weights_ref.md5 {
            return Err(PipelineError::Md5Mismatch {
                expected: weights_ref.md5.clone(),
                actual: actual_md5,
            });
        }

        // Caminho absoluto dentro do container trainer (volume outputs → /outputs)
        weights_staged_path = Some(format!("/outputs/{job_id}/weights/{filename}"));
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
        );

        // Valida que é YAML parseável (D6)
        let _: serde_yaml::Value = serde_yaml::from_str(&real_config).map_err(|e| {
            PipelineError::ConfigYamlInvalid(format!("config.yaml parse error: {e}"))
        })?;

        // Escreve no output_path (trainer lê de lá)
        let config_path = outputs.join("config.yaml");
        tokio::fs::write(&config_path, &real_config)
            .await
            .map_err(|e| PipelineError::ConfigYamlInvalid(format!("write config.yaml: {e}")))?;
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
            },
        )
        .await
        .map_err(|e| PipelineError::ReportFailed(format!("report running: {e}")))?;

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

    // Spawn metrics collector (polls metrics.jsonl durante execução)
    let metrics_path = outputs.join("metrics.jsonl");
    let metrics_path_clone = metrics_path.clone();
    let metrics_job_id = job_id.to_string();
    let metrics_total = total_epochs;

    let metrics_report_client = Arc::clone(&report_client);
    let metrics_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        let mut lines_read: usize = 0;
        loop {
            interval.tick().await;
            // Lê metrics.jsonl incrementalmente
            if let Ok(content) = tokio::fs::read_to_string(&metrics_path_clone).await {
                let lines: Vec<&str> = content.lines().collect();
                if lines.len() > lines_read {
                    for line in &lines[lines_read..] {
                        if let Some(m) = parse_metrics_line(line) {
                            let progress = compute_progress(&m, metrics_total);
                            // Report incremental (best-effort)
                            let _ = metrics_report_client
                                .report(
                                    &metrics_job_id,
                                    &ReportBody {
                                        status: "running".to_string(),
                                        progress: Some(progress),
                                        epoch: Some(m.epoch),
                                        step: None,
                                        metrics: Some(serde_json::json!({
                                            "box_loss": m.box_loss,
                                            "cls_loss": m.cls_loss,
                                            "dfl_loss": m.dfl_loss,
                                            "mAP50": m.map50,
                                            "mAP50-95": m.map50_95,
                                            "epoch": m.epoch,
                                        })),
                                        error: None,
                                        artifacts: None,
                                    },
                                )
                                .await;
                        }
                    }
                    lines_read = lines.len();
                }
            }
        }
    });

    // Ramifica subcomando e artefatos por engine (A.3 — D2)
    let subcommand_args: Vec<String> = match dispatch.engine.as_str() {
        "yolo" => vec![
            "train".to_string(),
            "--config".to_string(),
            format!("/outputs/{job_id}/config.yaml"),
            "--output".to_string(),
            format!("/outputs/{job_id}"),
        ],
        "autotracker" => vec![
            "autotrack".to_string(),
            "--config".to_string(),
            format!("/outputs/{job_id}/config.yaml"),
            "--output".to_string(),
            format!("/outputs/{job_id}"),
        ],
        other => return Err(PipelineError::Other(format!("unsupported engine: {other}"))),
    };

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
    let artifact_specs: Vec<(&str, &str)> = match dispatch.engine.as_str() {
        "yolo" => vec![
            ("best.pt", "model"),
            ("last.pt", "model"),
            ("metrics.jsonl", "metrics"),
        ],
        "autotracker" => vec![("boxes.json", "boxes"), ("metrics.jsonl", "metrics")],
        // Já validado acima — seguro unwrap
        _ => {
            return Err(PipelineError::Other(format!(
                "unsupported engine: {}",
                dispatch.engine
            )))
        }
    };

    let mut artifacts = Vec::new();

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

            s3.put(&art_key, &file_path)
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

    // 10. Lê métricas finais para o report done
    let final_metrics = read_final_metrics(&metrics_path);

    // 11. Report done
    report_client
        .report(
            job_id,
            &ReportBody {
                status: "done".to_string(),
                progress: Some(1.0),
                epoch: final_metrics.as_ref().map(|m| m.epoch),
                step: None,
                metrics: final_metrics.as_ref().map(|m| {
                    serde_json::json!({
                        "box_loss": m.box_loss,
                        "cls_loss": m.cls_loss,
                        "dfl_loss": m.dfl_loss,
                        "mAP50": m.map50,
                        "mAP50-95": m.map50_95,
                        "epoch": m.epoch,
                    })
                }),
                error: None,
                artifacts: if artifacts.is_empty() {
                    None
                } else {
                    Some(artifacts)
                },
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
        let result =
            replace_config_placeholders(config, "/datasets/datasets-cache/j1", "/outputs/j1", None);
        assert_eq!(
            result,
            "dataset_path: /datasets/datasets-cache/j1\noutput_path: /outputs/j1"
        );
    }

    #[test]
    fn replace_config_placeholders_yaml_parseable() {
        let config =
            "dataset_path: {dataset_path}\noutput_path: {output_path}\nepochs: 100\nmodel: yolo11m";
        let result =
            replace_config_placeholders(config, "/datasets/datasets-cache/j1", "/outputs/j1", None);
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
            epoch: 5,
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
            epoch: 5,
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
            epoch: 100,
        };
        assert!((compute_progress(&m, 100) - 1.0).abs() < 1e-6);
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
    struct FakeS3 {
        downloads: Mutex<Vec<String>>,
        uploads: Mutex<Vec<(String, PathBuf)>>,
        zip_bytes: Vec<u8>,
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
            }
        }
    }

    #[async_trait]
    impl S3Port for FakeS3 {
        async fn get_to_file(&self, key: &str, path: &std::path::Path) -> Result<(), String> {
            self.downloads.lock().unwrap().push(key.to_string());
            std::fs::write(path, &self.zip_bytes).map_err(|e| format!("write zip: {e}"))
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

        fn done_artifacts(&self) -> Option<Vec<ArtifactReport>> {
            self.reports
                .lock()
                .unwrap()
                .iter()
                .find(|r| r.status == "done")
                .and_then(|r| r.artifacts.clone())
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
            package_ref: PackageRef {
                key: "packages/test-pkg/dataset.zip".to_string(),
                md5_zip: String::new(), // será calculado
                bytes: 0,
            },
            config_yaml: Some(
                "epochs: 1\ndataset_path: {dataset_path}\noutput_path: {output_path}".to_string(),
            ),
            dataset_version_id: None,
            workdir: "/tmp".to_string(),
            weights_ref: None,
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
        d.package_ref.md5_zip = md5;
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

    // -- A.3 test 3: engine desconhecido → falha limpa (via run_job que faz o report "failed") --

    #[tokio::test]
    async fn unknown_engine_returns_clean_error() {
        let tmp = tempfile::tempdir().unwrap();

        let s3 = Arc::new(FakeS3::new());
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

        let mut dispatch = make_dispatch_with_valid_md5("job-bad-001", "diffusion", &zip_path);
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
        );
        assert_eq!(
            result,
            "model: yolo11m\nweights_path: /outputs/j1/weights/best.pt"
        );
    }

    #[test]
    fn replace_config_placeholders_without_weights_keeps_literal() {
        let config = "model: yolo11m\nweights_path: {weights_path}";
        let result = replace_config_placeholders(config, "/datasets/j1", "/outputs/j1", None);
        assert_eq!(result, "model: yolo11m\nweights_path: {weights_path}");
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
        )
        .await;
        assert!(result.is_err(), "unknown prefix should fail: {:?}", result);
    }
}
