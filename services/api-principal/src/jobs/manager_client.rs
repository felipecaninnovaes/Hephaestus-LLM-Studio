//! Client HTTP do manager (ADR-0007 D3): trait `ManagerPort` + impl real.
//!
//! Padrão igual a `StoragePort`/`EmbeddingPort`: trait para mock nos testes.
//! O principal fala com o manager via `Authorization: Bearer <MANAGER_TOKEN>`.

use async_trait::async_trait;
use serde::Deserialize;

/// Erro do manager client.
///
/// `Unavailable` ⇒ 503 `queue_unavailable` no handler.
#[derive(Debug)]
pub enum ManagerError {
    /// Manager indisponível ou falha de I/O.
    Unavailable(String),
    /// Resposta 404 do manager (job/artefato não existe).
    NotFound,
    /// Resposta 409 do manager (job em estado terminal — abort não possível).
    NotAbortable,
}

impl std::fmt::Display for ManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(_) => write!(f, "manager unavailable"),
            Self::NotFound => write!(f, "manager: not found"),
            Self::NotAbortable => write!(f, "manager: job not abortable"),
        }
    }
}

impl std::error::Error for ManagerError {}

/// Job retornado pelo manager (snake_case interno).
/// O handler do principal re-mapeia para camelCase no wire.
#[derive(Debug, Clone, Deserialize)]
pub struct InternalJob {
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
    pub created_at: String,
    pub finished_at: Option<String>,
}

/// Item da fila (snake_case interno do manager).
#[derive(Debug, Clone, Deserialize)]
pub struct InternalQueueItem {
    pub job_id: String,
    pub position: i32,
    pub queue_reason: Option<String>,
}

/// Artefato (snake_case interno do manager).
#[derive(Debug, Clone, Deserialize)]
pub struct InternalArtifact {
    pub id: String,
    pub kind: String,
    pub path: String,
    pub md5: String,
    pub bytes: i64,
}

/// Telemetria (camelCase direto do manager — D9).
#[derive(Debug, Clone, Deserialize)]
pub struct InternalTelemetry {
    pub measured: bool,
    pub vram_used: Option<i64>,
    pub vram_total: Option<i64>,
    pub cpu: Option<f64>,
    pub ram: Option<i64>,
    #[serde(default)]
    pub ram_total: Option<i64>,
    pub gpus: Vec<String>,
    pub jobs_active: i32,
}

/// Orquestrador retornado pelo manager (snake_case interno).
#[derive(Debug, Clone, Deserialize)]
pub struct InternalOrchestrator {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub endpoint: String,
    pub status: String,
    pub last_heartbeat: Option<String>,
}

/// Peso/modelo retornado pelo manager (snake_case interno).
#[derive(Debug, Clone, Deserialize)]
pub struct InternalModel {
    pub id: String,
    pub job_id: String,
    pub path: String,
    pub bytes: i64,
    pub engine: String,
    pub model: String,
    pub created_at: String,
}

/// Uso de storage retornado pelo manager (snake_case interno).
#[derive(Debug, Clone, Deserialize)]
pub struct InternalStorageUsage {
    pub artifacts_bytes: i64,
}

/// Resposta do manager ao criar job (snake_case interno).
#[derive(Debug, Clone, Deserialize)]
pub struct CreateJobResponse {
    pub job_id: String,
    pub status: String,
    pub queue_position: Option<i32>,
}

/// Resposta do manager ao abortar job (snake_case interno).
#[derive(Debug, Clone, Deserialize)]
pub struct AbortJobResponse {
    pub status: String,
}

/// Trait do client do manager (mockable).
#[async_trait]
pub trait ManagerPort: Send + Sync {
    /// Lista jobs com filtros opcionais.
    async fn list_jobs(
        &self,
        status: Option<&str>,
        engine: Option<&str>,
    ) -> Result<(Vec<InternalJob>, i32), ManagerError>;

    /// Fila ordenada de jobs.
    async fn list_queue(&self) -> Result<Vec<InternalQueueItem>, ManagerError>;

    /// Detalhe de um job por ID.
    async fn get_job(&self, id: &str) -> Result<InternalJob, ManagerError>;

    /// Lista artefatos de um job.
    async fn list_artifacts(&self, job_id: &str) -> Result<Vec<InternalArtifact>, ManagerError>;

    /// Telemetria do manager (cache de heartbeat).
    async fn get_telemetry(&self) -> Result<InternalTelemetry, ManagerError>;

    /// Cria um job via manager (ADR-0007 D3/D7).
    async fn create_job(&self, body: &serde_json::Value)
        -> Result<CreateJobResponse, ManagerError>;

    /// Aborta um job via manager (ADR-0007 D7).
    async fn abort_job(&self, id: &str) -> Result<AbortJobResponse, ManagerError>;

    /// Lista orquestradores registrados no manager.
    async fn list_orchestrators(&self) -> Result<Vec<InternalOrchestrator>, ManagerError>;

    /// Lista pesos/modelos derivados de job_artifacts (kind='model', jobs done).
    async fn list_models(&self) -> Result<Vec<InternalModel>, ManagerError>;

    /// Retorna uso de storage (artifacts bytes) do manager.
    async fn get_storage_usage(&self) -> Result<InternalStorageUsage, ManagerError>;
}

/// Implementação HTTP real do manager client.
pub struct HttpManager {
    base_url: String,
    token: String,
    client: reqwest::Client,
}

impl HttpManager {
    pub fn new(base_url: String, token: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("reqwest client do HttpManager");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
            client,
        }
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.token)
    }

    /// Helper genérico que faz GET e retorna JSON.
    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, ManagerError> {
        let url = format!("{}{path}", self.base_url);
        let resp = self
            .client
            .get(&url)
            .header("authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| ManagerError::Unavailable(format!("manager request: {e}")))?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(ManagerError::NotFound);
        }
        if !status.is_success() {
            return Err(ManagerError::Unavailable(format!(
                "manager status: {status}"
            )));
        }
        resp.json()
            .await
            .map_err(|e| ManagerError::Unavailable(format!("manager body: {e}")))
    }
}

#[async_trait]
impl ManagerPort for HttpManager {
    async fn list_jobs(
        &self,
        status: Option<&str>,
        engine: Option<&str>,
    ) -> Result<(Vec<InternalJob>, i32), ManagerError> {
        let mut url = format!("{}/internal/jobs", self.base_url);
        let mut params = Vec::new();
        if let Some(s) = status {
            params.push(format!("status={s}"));
        }
        if let Some(e) = engine {
            params.push(format!("engine={e}"));
        }
        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }
        let resp = self
            .client
            .get(&url)
            .header("authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| ManagerError::Unavailable(format!("manager request: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(ManagerError::Unavailable(format!(
                "manager status: {status}"
            )));
        }
        #[derive(Deserialize)]
        struct ListResponse {
            items: Vec<InternalJob>,
            total: i32,
        }
        let body: ListResponse = resp
            .json()
            .await
            .map_err(|e| ManagerError::Unavailable(format!("manager body: {e}")))?;
        Ok((body.items, body.total))
    }

    async fn list_queue(&self) -> Result<Vec<InternalQueueItem>, ManagerError> {
        // GET /internal/jobs (sem filtros) → {items, total}.
        // Deriva a fila: filtra status=queued, ordena por queue_position.
        #[derive(Deserialize)]
        struct ListResponse {
            items: Vec<InternalJob>,
        }
        let body: ListResponse = self.get_json("/internal/jobs").await?;
        let mut queue: Vec<InternalQueueItem> = body
            .items
            .into_iter()
            .filter(|j| j.status == "queued")
            .map(|j| InternalQueueItem {
                job_id: j.id,
                position: j.queue_position.unwrap_or(0),
                queue_reason: j.queue_reason,
            })
            .collect();
        queue.sort_by_key(|q| q.position);
        Ok(queue)
    }

    async fn get_job(&self, id: &str) -> Result<InternalJob, ManagerError> {
        self.get_json(&format!("/internal/jobs/{id}")).await
    }

    async fn list_artifacts(&self, job_id: &str) -> Result<Vec<InternalArtifact>, ManagerError> {
        #[derive(Deserialize)]
        struct ArtifactsResponse {
            items: Vec<InternalArtifact>,
        }
        let body: ArtifactsResponse = self
            .get_json(&format!("/internal/jobs/{job_id}/artifacts"))
            .await?;
        Ok(body.items)
    }

    async fn get_telemetry(&self) -> Result<InternalTelemetry, ManagerError> {
        self.get_json("/internal/telemetry").await
    }

    async fn create_job(
        &self,
        body: &serde_json::Value,
    ) -> Result<CreateJobResponse, ManagerError> {
        let url = format!("{}/internal/jobs", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("authorization", self.auth_header())
            .json(body)
            .send()
            .await
            .map_err(|e| ManagerError::Unavailable(format!("manager request: {e}")))?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(ManagerError::NotFound);
        }
        if !status.is_success() {
            return Err(ManagerError::Unavailable(format!(
                "manager status: {status}"
            )));
        }
        resp.json()
            .await
            .map_err(|e| ManagerError::Unavailable(format!("manager body: {e}")))
    }

    async fn abort_job(&self, id: &str) -> Result<AbortJobResponse, ManagerError> {
        let url = format!("{}/internal/jobs/{id}/abort", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| ManagerError::Unavailable(format!("manager request: {e}")))?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(ManagerError::NotFound);
        }
        if status == reqwest::StatusCode::CONFLICT {
            return Err(ManagerError::NotAbortable);
        }
        if !status.is_success() {
            return Err(ManagerError::Unavailable(format!(
                "manager status: {status}"
            )));
        }
        resp.json()
            .await
            .map_err(|e| ManagerError::Unavailable(format!("manager body: {e}")))
    }

    async fn list_orchestrators(&self) -> Result<Vec<InternalOrchestrator>, ManagerError> {
        #[derive(Deserialize)]
        struct ListResponse {
            items: Vec<InternalOrchestrator>,
        }
        let body: ListResponse = self.get_json("/internal/orchestrators").await?;
        Ok(body.items)
    }

    async fn list_models(&self) -> Result<Vec<InternalModel>, ManagerError> {
        #[derive(Deserialize)]
        struct ListResponse {
            items: Vec<InternalModel>,
        }
        let body: ListResponse = self.get_json("/internal/models").await?;
        Ok(body.items)
    }

    async fn get_storage_usage(&self) -> Result<InternalStorageUsage, ManagerError> {
        self.get_json("/internal/storage/usage").await
    }
}

/// Mock do manager para testes unitários e de integração.
///
/// Dois níveis de configuração:
/// - **fixo** (`get_job_result`, `list_artifacts_result`): retrocompatível,
///   usado por todos os testes unitários existentes (176+ verdes).
/// - **por-ID** (`jobs_by_id`, `artifacts_by_id`): para testes de integração
///   que simulam um fluxo completo com IDs reais; `get_job`/`list_artifacts`
///   consultam o mapa por-ID antes de cair no resultado fixo.
pub struct MockManager {
    pub list_jobs_result: Option<(Vec<InternalJob>, i32)>,
    pub list_queue_result: Option<Vec<InternalQueueItem>>,
    pub get_job_result: Option<InternalJob>,
    pub list_artifacts_result: Option<Vec<InternalArtifact>>,
    pub get_telemetry_result: Option<InternalTelemetry>,
    pub create_job_result: Option<CreateJobResponse>,
    pub abort_job_result: Option<AbortJobResponse>,
    pub list_orchestrators_result: Option<Vec<InternalOrchestrator>>,
    pub list_models_result: Option<Vec<InternalModel>>,
    pub get_storage_usage_result: Option<InternalStorageUsage>,
    /// Se `true`, todas as chamadas retornam `Unavailable`.
    pub fail: bool,
    /// Se `true`, `abort_job` retorna `NotAbortable` (para testar 409).
    pub abort_not_abortable: bool,
    /// Body capturado na última chamada a `create_job` (para asserts de teste).
    last_create_job_body: std::sync::Mutex<Option<serde_json::Value>>,
    /// Jobs indexados por ID — `get_job` consulta aqui antes do resultado fixo.
    pub jobs_by_id: std::collections::HashMap<String, InternalJob>,
    /// Artefatos indexados por job_id — `list_artifacts` consulta aqui antes do
    /// resultado fixo.
    pub artifacts_by_id: std::collections::HashMap<String, Vec<InternalArtifact>>,
}

impl MockManager {
    /// Retorna o body capturado na última chamada a `create_job`.
    pub fn last_create_job_body(&self) -> Option<serde_json::Value> {
        self.last_create_job_body
            .try_lock()
            .ok()
            .and_then(|m| m.clone())
    }
}

impl Default for MockManager {
    fn default() -> Self {
        Self {
            list_jobs_result: Some((vec![], 0)),
            list_queue_result: Some(vec![]),
            get_job_result: None,
            list_artifacts_result: Some(vec![]),
            get_telemetry_result: None,
            create_job_result: None,
            abort_job_result: None,
            list_orchestrators_result: None,
            list_models_result: None,
            get_storage_usage_result: None,
            fail: false,
            abort_not_abortable: false,
            last_create_job_body: std::sync::Mutex::new(None),
            jobs_by_id: std::collections::HashMap::new(),
            artifacts_by_id: std::collections::HashMap::new(),
        }
    }
}

#[async_trait]
impl ManagerPort for MockManager {
    async fn list_jobs(
        &self,
        _status: Option<&str>,
        _engine: Option<&str>,
    ) -> Result<(Vec<InternalJob>, i32), ManagerError> {
        if self.fail {
            return Err(ManagerError::Unavailable("mock fail".into()));
        }
        Ok(self.list_jobs_result.clone().unwrap_or_default())
    }

    async fn list_queue(&self) -> Result<Vec<InternalQueueItem>, ManagerError> {
        if self.fail {
            return Err(ManagerError::Unavailable("mock fail".into()));
        }
        Ok(self.list_queue_result.clone().unwrap_or_default())
    }

    async fn get_job(&self, id: &str) -> Result<InternalJob, ManagerError> {
        if self.fail {
            return Err(ManagerError::Unavailable("mock fail".into()));
        }
        // Consulta por-ID primeiro (integração), depois resultado fixo (unit).
        if let Some(job) = self.jobs_by_id.get(id) {
            return Ok(job.clone());
        }
        self.get_job_result.clone().ok_or(ManagerError::NotFound)
    }

    async fn list_artifacts(&self, job_id: &str) -> Result<Vec<InternalArtifact>, ManagerError> {
        if self.fail {
            return Err(ManagerError::Unavailable("mock fail".into()));
        }
        // Consulta por-ID primeiro (integração), depois resultado fixo (unit).
        if let Some(arts) = self.artifacts_by_id.get(job_id) {
            return Ok(arts.clone());
        }
        Ok(self.list_artifacts_result.clone().unwrap_or_default())
    }

    async fn get_telemetry(&self) -> Result<InternalTelemetry, ManagerError> {
        if self.fail {
            return Err(ManagerError::Unavailable("mock fail".into()));
        }
        self.get_telemetry_result
            .clone()
            .ok_or(ManagerError::Unavailable("no telemetry".into()))
    }

    async fn create_job(
        &self,
        body: &serde_json::Value,
    ) -> Result<CreateJobResponse, ManagerError> {
        if self.fail {
            return Err(ManagerError::Unavailable("mock fail".into()));
        }
        // Captura body para asserts de teste (try_lock: non-blocking, testes
        // rodam serializados pelo SERIAL.lock).
        if let Ok(mut guard) = self.last_create_job_body.try_lock() {
            *guard = Some(body.clone());
        }
        self.create_job_result
            .clone()
            .ok_or(ManagerError::Unavailable("no create_job result".into()))
    }

    async fn abort_job(&self, _id: &str) -> Result<AbortJobResponse, ManagerError> {
        if self.fail {
            return Err(ManagerError::Unavailable("mock fail".into()));
        }
        if self.abort_not_abortable {
            return Err(ManagerError::NotAbortable);
        }
        self.abort_job_result.clone().ok_or(ManagerError::NotFound)
    }

    async fn list_orchestrators(&self) -> Result<Vec<InternalOrchestrator>, ManagerError> {
        if self.fail {
            return Err(ManagerError::Unavailable("mock fail".into()));
        }
        Ok(self.list_orchestrators_result.clone().unwrap_or_default())
    }

    async fn list_models(&self) -> Result<Vec<InternalModel>, ManagerError> {
        if self.fail {
            return Err(ManagerError::Unavailable("mock fail".into()));
        }
        Ok(self.list_models_result.clone().unwrap_or_default())
    }

    async fn get_storage_usage(&self) -> Result<InternalStorageUsage, ManagerError> {
        if self.fail {
            return Err(ManagerError::Unavailable("mock fail".into()));
        }
        self.get_storage_usage_result
            .clone()
            .ok_or(ManagerError::Unavailable("no storage_usage".into()))
    }
}
