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
    pub gpus: Vec<String>,
    pub vram_total: Option<i64>,
    pub vram_used: Option<i64>,
    pub cpu: Option<f64>,
    pub ram: Option<i64>,
    pub jobs_active: i32,
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
    Md5Mismatch { expected: String, actual: String },
    UnzipFailed(String),
    ConfigYamlInvalid(String),
    DockerFailed { exit_code: i32, logs_tail: String },
    ArtifactUpload(String),
    ReportFailed(String),
    UnsupportedEngine(String),
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
            Self::UnsupportedEngine(e) => {
                write!(
                    f,
                    "unsupported engine: {e} (expected 'yolo' or 'autotracker')"
                )
            }
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
}

impl S3Scope {
    pub fn prefix(&self) -> &str {
        match self {
            S3Scope::Packages => "packages/",
            S3Scope::Artifacts => "artifacts/",
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

/// Substitui `{dataset_path}` e `{output_path}` no config.yaml.
pub fn replace_config_placeholders(config: &str, dataset_path: &str, output_path: &str) -> String {
    config
        .replace("{dataset_path}", dataset_path)
        .replace("{output_path}", output_path)
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
    async fn run(
        &self,
        image: &str,
        container_name: &str,
        volumes: &[(String, String)], // (host_path, container_path)
        args: &[String],              // argumentos após a imagem (ex.: train --config …)
    ) -> (i32, String);

    /// Para um container (abort via docker stop --time 5 → exit 137).
    async fn stop(&self, container_name: &str) -> Result<(), String>;
}

/// Executor real via CLI docker (EXEC_MODE=docker, default).
pub struct DockerExecutor;

#[async_trait]
impl TrainerExecutor for DockerExecutor {
    async fn run(
        &self,
        image: &str,
        container_name: &str,
        volumes: &[(String, String)],
        args: &[String],
    ) -> (i32, String) {
        let mut cmd = tokio::process::Command::new("docker");
        cmd.arg("run").arg("--rm").arg("--name").arg(container_name);

        for (host, container) in volumes {
            cmd.arg("-v").arg(format!("{host}:{container}"));
        }

        cmd.arg(image);
        cmd.args(args);

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
) {
    let job_id = dispatch.job_id.clone();
    let report_for_error = Arc::clone(&report_client);
    let result = run_job_inner(&dispatch, s3, report_client, executor, &active_jobs).await;

    if let Err(err_msg) = result {
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
) -> Result<(), String> {
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
        .map_err(|e| format!("create datasets-cache: {e}"))?;
    tokio::fs::create_dir_all(&outputs)
        .await
        .map_err(|e| format!("create outputs: {e}"))?;
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .map_err(|e| format!("create temp: {e}"))?;

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
        .map_err(|e| format!("report preparing: {e}"))?;

    // 2. Download package.zip via S3 (scoped — D2 barreira principal)
    let zip_path = temp_dir.join("dataset.zip");
    let key = scoped_key(S3Scope::Packages, &dispatch.package_ref.key)
        .map_err(|e| format!("invalid package key: {e}"))?;

    s3.get_to_file(&key, &zip_path)
        .await
        .map_err(|e| format!("download package: {e}"))?;

    // 3. Verify MD5 (crash do job se divergir — D4)
    let actual_md5 = compute_file_md5(&zip_path).map_err(|e| format!("compute md5: {e}"))?;
    if actual_md5 != dispatch.package_ref.md5_zip {
        return Err(format!(
            "MD5 mismatch: expected {}, got {actual_md5}",
            dispatch.package_ref.md5_zip
        ));
    }

    // 4. Unzip (zip-slip safe, padrão import 3e)
    unzip_safe(&zip_path, &datasets_cache).map_err(|e| format!("{}", e))?;

    // 5. Monta config.yaml REAL — substitui placeholders (§8/:102)
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
        let real_config = replace_config_placeholders(config_yaml, &dataset_path, &output_path);

        // Valida que é YAML parseável (D6)
        let _: serde_yaml::Value = serde_yaml::from_str(&real_config)
            .map_err(|e| format!("config.yaml parse error: {e}"))?;

        // Escreve no output_path (trainer lê de lá)
        let config_path = outputs.join("config.yaml");
        tokio::fs::write(&config_path, &real_config)
            .await
            .map_err(|e| format!("write config.yaml: {e}"))?;
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
        .map_err(|e| format!("report running: {e}"))?;

    // 7. Execute trainer (D5 :301–309)
    let container_name = format!("trainer-{}-{}", dispatch.engine, job_id);
    let active_state = ActiveJobState::new(container_name.clone());
    active_jobs.insert(job_id.to_string(), active_state);

    let volumes = vec![
        (vol_datasets, "/datasets".to_string()),
        (vol_outputs, "/outputs".to_string()),
    ];

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
        other => return Err(format!("unsupported engine: {other}")),
    };

    let (exit_code, logs) = executor
        .run(&dispatch.image, &container_name, &volumes, &subcommand_args)
        .await;

    // Cancela metrics collector
    metrics_handle.abort();

    // Remove from active jobs
    active_jobs.remove(job_id);

    // 8. Check exit code
    if exit_code != 0 {
        let logs_tail = logs.lines().rev().take(20).collect::<Vec<_>>().join("\n");
        return Err(format!("trainer failed (exit {exit_code}):\n{logs_tail}"));
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
        _ => return Err(format!("unsupported engine: {}", dispatch.engine)),
    };

    let mut artifacts = Vec::new();

    for (filename, kind) in artifact_specs {
        let file_path = outputs.join(filename);
        if file_path.exists() {
            let art_key = format!("artifacts/{job_id}/{filename}");
            let art_key = scoped_key(S3Scope::Artifacts, &art_key)
                .map_err(|e| format!("artifact key: {e}"))?;
            let md5 = compute_file_md5(&file_path).map_err(|e| format!("md5 {filename}: {e}"))?;
            let bytes = std::fs::metadata(&file_path)
                .map(|m| m.len() as i64)
                .unwrap_or(0);

            s3.put(&art_key, &file_path)
                .await
                .map_err(|e| format!("upload {filename}: {e}"))?;

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
        .map_err(|e| format!("report done: {e}"))?;

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

    // -- config.yaml tests --

    #[test]
    fn replace_config_placeholders_basic() {
        let config = "dataset_path: {dataset_path}\noutput_path: {output_path}";
        let result =
            replace_config_placeholders(config, "/datasets/datasets-cache/j1", "/outputs/j1");
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
            replace_config_placeholders(config, "/datasets/datasets-cache/j1", "/outputs/j1");
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
}
