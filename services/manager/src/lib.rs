//! Manager service — fila central de jobs (ADR-0007 F4.3).
//!
//! `lib.rs` contém lógica de negócio testável sem axum: criação de jobs,
//! listagem, abort, report, heartbeat, telemetry, recovery, auto-adoção
//! e dispatch ao orquestrador. `main.rs` é a camada fina axum/routes.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
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
    Internal(String),
}

impl std::fmt::Display for ManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "not found"),
            Self::NotAbortable => write!(f, "job not abortable"),
            Self::Internal(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ManagerError {}

// ---------------------------------------------------------------------------
// Tipos de request/response (snake_case interno)
// ---------------------------------------------------------------------------

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
    pub created_at: String,
    pub finished_at: Option<String>,
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
    pub gpus: Vec<String>,
    pub vram_total: Option<i64>,
    pub vram_used: Option<i64>,
    pub cpu: Option<f64>,
    pub ram: Option<i64>,
    pub jobs_active: i32,
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
// Trait de cliente do orquestrador (mockável para testes)
// ---------------------------------------------------------------------------

#[async_trait]
pub trait OrchestratorClient: Send + Sync {
    async fn post(&self, url: &str, body: &serde_json::Value) -> Result<(), String>;
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
}

// ---------------------------------------------------------------------------
// Cache de telemetria
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct TelemetryState {
    pub measured: bool,
    pub vram_used: Option<i64>,
    pub vram_total: Option<i64>,
    pub cpu: Option<f64>,
    pub ram: Option<i64>,
    pub gpus: Vec<String>,
    pub jobs_active: i32,
    pub last_heartbeat: Option<DateTime<Utc>>,
}

impl Default for TelemetryState {
    fn default() -> Self {
        Self {
            measured: false,
            vram_used: None,
            vram_total: None,
            cpu: None,
            ram: None,
            gpus: vec![],
            jobs_active: 0,
            last_heartbeat: None,
        }
    }
}

pub type TelemetryCache = Arc<RwLock<TelemetryState>>;

pub fn new_telemetry_cache() -> TelemetryCache {
    Arc::new(RwLock::new(TelemetryState::default()))
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

    sqlx::query(
        "INSERT INTO jobs (id, kind, engine, model, mode, dataset_id, params, config_yaml, vram_min_gb, status, queue_reason) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'queued', NULL)",
    )
    .bind(job_id)
    .bind(&req.kind)
    .bind(&req.engine)
    .bind(&req.model)
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

    let mut query = String::from("SELECT id, kind, engine, model, mode, dataset_id, status, queue_reason, progress, epoch, step, metrics, vram_min_gb, orchestrator_id, created_at, finished_at FROM jobs WHERE 1=1");
    let mut count_query = String::from("SELECT COUNT(*) FROM jobs WHERE 1=1");

    if let Some(s) = status {
        query.push_str(&format!(" AND status = '{s}'"));
        count_query.push_str(&format!(" AND status = '{s}'"));
    }
    if let Some(e) = engine {
        query.push_str(&format!(" AND engine = '{e}'"));
        count_query.push_str(&format!(" AND engine = '{e}'"));
    }
    query.push_str(" ORDER BY created_at DESC");

    let total: (i64,) = sqlx::query_as(&count_query)
        .fetch_one(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("count jobs: {e}")))?;

    let rows: Vec<(
        Uuid,
        String,
        String,
        String,
        String,
        Option<Uuid>,
        String,
        Option<String>,
        Option<f64>,
        Option<i32>,
        Option<i32>,
        Option<serde_json::Value>,
        Option<i32>,
        Option<Uuid>,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
    )> = sqlx::query_as(&query)
        .fetch_all(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("list jobs: {e}")))?;

    let items = rows
        .into_iter()
        .map(|r| {
            let id_str = r.0.to_string();
            let queue_position = if r.6 == "queued" {
                pos_map.get(&id_str).copied()
            } else {
                None
            };
            JobRow {
                id: id_str,
                kind: r.1,
                engine: r.2,
                model: r.3,
                mode: r.4,
                dataset_id: r.5.map(|u| u.to_string()),
                status: r.6,
                queue_reason: r.7,
                queue_position,
                progress: r.8,
                epoch: r.9,
                step: r.10,
                metrics: r.11,
                vram_min_gb: r.12,
                orchestrator_id: r.13.map(|u| u.to_string()),
                created_at: r.14.to_rfc3339(),
                finished_at: r.15.map(|t| t.to_rfc3339()),
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

    let row: Option<(
        Uuid, String, String, String, String, Option<Uuid>, String, Option<String>,
        Option<f64>, Option<i32>, Option<i32>, Option<serde_json::Value>,
        Option<i32>, Option<Uuid>, DateTime<Utc>, Option<DateTime<Utc>>,
    )> = sqlx::query_as(
        "SELECT id, kind, engine, model, mode, dataset_id, status, queue_reason, progress, epoch, step, metrics, vram_min_gb, orchestrator_id, created_at, finished_at \
         FROM jobs WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("get job: {e}")))?;

    let r = row.ok_or(ManagerError::NotFound)?;
    let id_str = r.0.to_string();
    let queue_position = if r.6 == "queued" {
        pos_map.get(&id_str).copied()
    } else {
        None
    };

    Ok(JobRow {
        id: id_str,
        kind: r.1,
        engine: r.2,
        model: r.3,
        mode: r.4,
        dataset_id: r.5.map(|u| u.to_string()),
        status: r.6,
        queue_reason: r.7,
        queue_position,
        progress: r.8,
        epoch: r.9,
        step: r.10,
        metrics: r.11,
        vram_min_gb: r.12,
        orchestrator_id: r.13.map(|u| u.to_string()),
        created_at: r.14.to_rfc3339(),
        finished_at: r.15.map(|t| t.to_rfc3339()),
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

/// Extrai o campo `epoch` (i64) de um objeto de métricas.
fn metrics_epoch(value: &serde_json::Value) -> Option<i64> {
    value.get("epoch").and_then(|v| v.as_i64())
}

/// Faz upsert incremental de metrics no banco:
/// lê array existente, normaliza novos, dedup por epoch (R4), grava como
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

    // Mapa epoch → objeto (dedup por chave).
    let mut epoch_map: std::collections::HashMap<i64, serde_json::Value> =
        std::collections::HashMap::new();

    // 1. Itens existentes.
    for item in normalize_metrics_to_array(&existing) {
        if let Some(ep) = metrics_epoch(&item) {
            epoch_map.insert(ep, item);
        }
    }

    // 2. Itens novos (substitui se epoch repetido).
    for item in normalize_metrics_to_array(new_metrics) {
        if let Some(ep) = metrics_epoch(&item) {
            epoch_map.insert(ep, item);
        }
    }

    // 3. Ordena por epoch e grava como {"items": [...]}.
    let mut items: Vec<serde_json::Value> = epoch_map.into_values().collect();
    items.sort_by_key(|v| metrics_epoch(v).unwrap_or(0));

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
        }

        "done" => {
            // Idempotente: se já done, não duplica artifacts.
            if current_status == "done" {
                return Ok(());
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

            sqlx::query(
                "UPDATE jobs SET status = 'done', finished_at = now(), progress = 1.0 WHERE id = $1",
            )
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("set done: {e}")))?;
        }

        "failed" => {
            // Idempotente.
            if current_status == "failed" {
                return Ok(());
            }

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
    // Atualiza last_heartbeat de todos os orchestrators online.
    sqlx::query(
        "UPDATE orchestrators SET last_heartbeat = now() WHERE status IN ('online', 'degraded')",
    )
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("update heartbeat: {e}")))?;

    // Atualiza cache.
    let mut state = cache.write().await;
    state.measured = true;
    state.vram_used = req.vram_used;
    state.vram_total = req.vram_total;
    state.cpu = req.cpu;
    state.ram = req.ram;
    state.gpus = req.gpus;
    state.jobs_active = req.jobs_active;
    state.last_heartbeat = Some(Utc::now());

    Ok(())
}

/// Retorna telemetria do cache.
pub async fn get_telemetry(pool: &PgPool, cache: &TelemetryCache) -> TelemetryResponse {
    let state = cache.read().await;
    let now = Utc::now();

    if let Some(last) = state.last_heartbeat {
        if (now - last).num_seconds() <= 10 {
            return TelemetryResponse {
                measured: true,
                vram_used: state.vram_used,
                vram_total: state.vram_total,
                cpu: state.cpu,
                ram: state.ram,
                gpus: state.gpus.clone(),
                jobs_active: state.jobs_active,
            };
        }
    }

    // Sem heartbeat recente: devolve measured:false e jobs_active da fila.
    let jobs_active: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM jobs WHERE status NOT IN ('done', 'failed', 'cancelled')",
    )
    .fetch_one(pool)
    .await
    .unwrap_or((0,));

    TelemetryResponse {
        measured: false,
        vram_used: None,
        vram_total: None,
        cpu: None,
        ram: None,
        gpus: vec![],
        jobs_active: jobs_active.0 as i32,
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

/// Auto-adoção: insere orchestrator-local se ausente, atualiza status.
pub async fn adopt_orchestrator(pool: &PgPool) -> Result<(), ManagerError> {
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'orchestrator-local', 'http://orchestrator-local:8082', 'local', 'online') \
         ON CONFLICT (endpoint) DO UPDATE SET status = 'online'",
    )
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("adopt orchestrator: {e}")))?;

    Ok(())
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
// Dispatch
// ---------------------------------------------------------------------------

/// Pega o próximo job queued e despacha ao orquestrador.
///
/// Retorna `true` se um job foi despachado, `false` se não havia job na fila.
pub async fn dispatch_next(
    pool: &PgPool,
    orch_client: &dyn OrchestratorClient,
    exec_mode: &str,
    orch_workdir: &str,
    image: &str,
) -> Result<bool, ManagerError> {
    // Seleciona próximo job queued (FIFO).
    let row: Option<(
        Uuid, String, Option<serde_json::Value>, Option<String>,
    )> = sqlx::query_as(
        "SELECT id, engine, params, config_yaml FROM jobs WHERE status = 'queued' ORDER BY created_at LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("select next job: {e}")))?;

    let (job_id, engine, params, config_yaml) = match row {
        Some(r) => r,
        None => return Ok(false),
    };

    // Busca endpoint do orchestrator online.
    let orch: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, endpoint FROM orchestrators WHERE status = 'online' LIMIT 1")
            .fetch_optional(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("find orchestrator: {e}")))?;

    let (orch_id, orch_endpoint) = match orch {
        Some(o) => o,
        None => {
            // Nenhum orchestrator online: volta para queued.
            sqlx::query(
                "UPDATE jobs SET queue_reason = 'waiting_slot' WHERE id = $1 AND status = 'queued'",
            )
            .bind(job_id)
            .execute(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("set waiting_slot: {e}")))?;
            return Ok(false);
        }
    };

    // Marca dispatched.
    sqlx::query(
        "UPDATE jobs SET status = 'dispatched', queue_reason = NULL, orchestrator_id = $2 WHERE id = $1 AND status = 'queued'",
    )
    .bind(job_id)
    .bind(orch_id)
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("set dispatched: {e}")))?;

    // Extrai package_ref do params.
    let package_ref = params
        .as_ref()
        .and_then(|p| p.get("package_ref"))
        .cloned()
        .unwrap_or(serde_json::json!({}));

    let dataset_version_id = params
        .as_ref()
        .and_then(|p| p.get("package_ref"))
        .and_then(|pr| pr.get("version_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let dispatch_body = serde_json::json!({
        "job_id": job_id.to_string(),
        "engine": engine,
        "image": image,
        "exec_mode": exec_mode,
        "package_ref": package_ref,
        "config_yaml": config_yaml,
        "dataset_version_id": dataset_version_id,
        "workdir": orch_workdir,
    });

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
}
