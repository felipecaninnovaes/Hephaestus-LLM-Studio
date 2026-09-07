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
    pub gpus: Vec<String>,
    pub jobs_active: i32,
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
            .timeout(std::time::Duration::from_secs(30))
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
        self.get_json("/internal/jobs").await
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
}

/// Mock do manager para testes unitários.
pub struct MockManager {
    pub list_jobs_result: Option<(Vec<InternalJob>, i32)>,
    pub list_queue_result: Option<Vec<InternalQueueItem>>,
    pub get_job_result: Option<InternalJob>,
    pub list_artifacts_result: Option<Vec<InternalArtifact>>,
    pub get_telemetry_result: Option<InternalTelemetry>,
    pub create_job_result: Option<CreateJobResponse>,
    pub abort_job_result: Option<AbortJobResponse>,
    /// Se `true`, todas as chamadas retornam `Unavailable`.
    pub fail: bool,
    /// Se `true`, `abort_job` retorna `NotAbortable` (para testar 409).
    pub abort_not_abortable: bool,
    /// Body capturado na última chamada a `create_job` (para asserts de teste).
    last_create_job_body: std::sync::Mutex<Option<serde_json::Value>>,
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
            fail: false,
            abort_not_abortable: false,
            last_create_job_body: std::sync::Mutex::new(None),
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

    async fn get_job(&self, _id: &str) -> Result<InternalJob, ManagerError> {
        if self.fail {
            return Err(ManagerError::Unavailable("mock fail".into()));
        }
        self.get_job_result.clone().ok_or(ManagerError::NotFound)
    }

    async fn list_artifacts(&self, _job_id: &str) -> Result<Vec<InternalArtifact>, ManagerError> {
        if self.fail {
            return Err(ManagerError::Unavailable("mock fail".into()));
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
}
