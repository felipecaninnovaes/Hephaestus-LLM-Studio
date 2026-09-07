//! Handlers de jobs (7 rotas de leitura — BFF do manager, ADR-0007 D3/D7).
//!
//! O principal NÃO lê tabelas jobs/orchestrators/job_artifacts — dono é o
//! manager (ADR-0007 D1/D3/D8). Todas as respostas são camelCase.
//!
//! Decisões registradas (coordenador, NÃO redecidir):
//! - 404 `not_found` para `:id`/`:artifactId` não-UUID (D8 — NUNCA 400).
//! - Manager indisponível → 503 `queue_unavailable` em TODAS as 7 rotas.
//! - GET /api/telemetry tem 503 além de 200/401 (delta consciente da D7 :355
//!   — o docs-sync cobre depois; o contrato v1 sempre teve 503 implícito para
//!   manager, agora é explícito).

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{err, MSG_NOT_FOUND, MSG_QUEUE_UNAVAILABLE, MSG_STORAGE_UNAVAILABLE};
use crate::jobs::manager_client::ManagerError;
use crate::state::AppState;
use crate::storage::StorageError;

// ---------------------------------------------------------------------------
// Query params
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct JobsQuery {
    pub status: Option<String>,
    pub engine: Option<String>,
}

// ---------------------------------------------------------------------------
// Response types (camelCase wire)
// ---------------------------------------------------------------------------

/// Metric epoch item (camelCase wire). mAP50-95 → `map5095`.
#[derive(Debug, Serialize)]
pub struct MetricsItem {
    pub epoch: i32,
    #[serde(rename = "boxLoss")]
    pub box_loss: f64,
    #[serde(rename = "clsLoss")]
    pub cls_loss: f64,
    #[serde(rename = "dflLoss")]
    pub dfl_loss: f64,
    #[serde(rename = "map50")]
    pub map50: f64,
    #[serde(rename = "map5095")]
    pub map5095: f64,
}

/// Job response (camelCase wire).
#[derive(Debug, Serialize)]
pub struct JobResponse {
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
    pub metrics: Option<Vec<MetricsItem>>,
    pub vram_min_gb: Option<i32>,
    pub orchestrator_id: Option<String>,
    pub created_at: String,
    pub finished_at: Option<String>,
}

/// Job list response.
#[derive(Debug, Serialize)]
pub struct JobListResponse {
    pub items: Vec<JobResponse>,
    pub total: i32,
}

/// Queue item (camelCase wire).
#[derive(Debug, Serialize)]
pub struct QueueItem {
    #[serde(rename = "jobId")]
    pub job_id: String,
    pub position: i32,
    #[serde(rename = "queueReason")]
    pub queue_reason: Option<String>,
}

/// Queue list response.
#[derive(Debug, Serialize)]
pub struct QueueResponse {
    pub items: Vec<QueueItem>,
}

/// Artifact (camelCase wire).
#[derive(Debug, Serialize)]
pub struct ArtifactResponse {
    pub id: String,
    pub kind: String,
    pub path: String,
    pub md5: String,
    pub bytes: i64,
}

/// Artifact list response.
#[derive(Debug, Serialize)]
pub struct ArtifactListResponse {
    pub items: Vec<ArtifactResponse>,
}

/// Telemetry response (camelCase wire — D9).
#[derive(Debug, Serialize)]
pub struct TelemetryResponse {
    pub measured: bool,
    #[serde(rename = "vramUsed")]
    pub vram_used: Option<i64>,
    #[serde(rename = "vramTotal")]
    pub vram_total: Option<i64>,
    pub cpu: Option<f64>,
    pub ram: Option<i64>,
    pub gpus: Vec<String>,
    #[serde(rename = "jobsActive")]
    pub jobs_active: i32,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_uuid(id: &str) -> Option<Uuid> {
    id.parse::<Uuid>().ok()
}

fn not_found() -> Response {
    err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND)
}

fn queue_unavailable() -> Response {
    err(
        StatusCode::SERVICE_UNAVAILABLE,
        "queue_unavailable",
        MSG_QUEUE_UNAVAILABLE,
    )
}

fn storage_unavailable() -> Response {
    err(
        StatusCode::SERVICE_UNAVAILABLE,
        "storage_unavailable",
        MSG_STORAGE_UNAVAILABLE,
    )
}

/// Re-mapeia JSONB snake_case do manager para MetricsItem camelCase.
/// A chave `mAP50-95` do JSONB vira `map5095` no wire.
fn remap_metrics(raw: &serde_json::Value) -> Vec<MetricsItem> {
    let items = match raw.get("items").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return vec![],
    };
    items
        .iter()
        .filter_map(|item| {
            let epoch = item.get("epoch").and_then(|v| v.as_i64())? as i32;
            let box_loss = item.get("box_loss").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let cls_loss = item.get("cls_loss").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let dfl_loss = item.get("dfl_loss").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let map50 = item.get("mAP50").and_then(|v| v.as_f64()).unwrap_or(0.0);
            // mAP50-95 → map5095: a chave no JSONB tem hífen/maiúscula.
            let map5095 = item
                .get("mAP50-95")
                .or_else(|| item.get("map50_95"))
                .or_else(|| item.get("map5095"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            Some(MetricsItem {
                epoch,
                box_loss,
                cls_loss,
                dfl_loss,
                map50,
                map5095,
            })
        })
        .collect()
}

/// Converte `InternalJob` do manager (snake_case) para `JobResponse` (camelCase).
fn to_job_response(job: crate::jobs::manager_client::InternalJob) -> JobResponse {
    let metrics = job.metrics.as_ref().map(remap_metrics);
    JobResponse {
        id: job.id,
        kind: job.kind,
        engine: job.engine,
        model: job.model,
        mode: job.mode,
        dataset_id: job.dataset_id,
        status: job.status,
        queue_reason: job.queue_reason,
        queue_position: job.queue_position,
        progress: job.progress,
        epoch: job.epoch,
        step: job.step,
        metrics,
        vram_min_gb: job.vram_min_gb,
        orchestrator_id: job.orchestrator_id,
        created_at: job.created_at,
        finished_at: job.finished_at,
    }
}

/// Valida que a path do artefato não contém `..` ou prefixo estranho (defesa
/// em profundidade). Retorna `Err` com 400 se inválido.
fn validate_artifact_path(path: &str) -> Result<(), Response> {
    if path.contains("..") || path.starts_with('/') || path.starts_with('\\') {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "invalid request",
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/jobs?status=&engine= — lista jobs via manager.
pub async fn list_jobs(State(state): State<AppState>, Query(q): Query<JobsQuery>) -> Response {
    let status = q.status.as_deref();
    let engine = q.engine.as_deref();
    let (items, total) = match state.manager.list_jobs(status, engine).await {
        Ok(v) => v,
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let items = items.into_iter().map(to_job_response).collect();
    (StatusCode::OK, Json(JobListResponse { items, total })).into_response()
}

/// GET /api/jobs/queue — fila ordenada de jobs.
pub async fn list_queue(State(state): State<AppState>) -> Response {
    let items = match state.manager.list_queue().await {
        Ok(v) => v,
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let items = items
        .into_iter()
        .map(|q| QueueItem {
            job_id: q.job_id,
            position: q.position,
            queue_reason: q.queue_reason,
        })
        .collect();
    (StatusCode::OK, Json(QueueResponse { items })).into_response()
}

/// GET /api/jobs/:id — detalhe de um job.
pub async fn get_job(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if parse_uuid(&id).is_none() {
        return not_found();
    }
    let job = match state.manager.get_job(&id).await {
        Ok(v) => v,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
    };
    (StatusCode::OK, Json(to_job_response(job))).into_response()
}

/// GET /api/jobs/:id/metrics — métricas de um job (re-mapeadas camelCase).
pub async fn get_job_metrics(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if parse_uuid(&id).is_none() {
        return not_found();
    }
    let job = match state.manager.get_job(&id).await {
        Ok(v) => v,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
    };
    let items = match &job.metrics {
        Some(m) => remap_metrics(m),
        None => vec![],
    };
    (StatusCode::OK, Json(serde_json::json!({ "items": items }))).into_response()
}

/// GET /api/jobs/:id/artifacts — lista artefatos de um job.
pub async fn list_artifacts(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if parse_uuid(&id).is_none() {
        return not_found();
    }
    let arts = match state.manager.list_artifacts(&id).await {
        Ok(v) => v,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
    };
    let items: Vec<ArtifactResponse> = arts
        .into_iter()
        .map(|a| ArtifactResponse {
            id: a.id,
            kind: a.kind,
            path: a.path,
            md5: a.md5,
            bytes: a.bytes,
        })
        .collect();
    (StatusCode::OK, Json(ArtifactListResponse { items })).into_response()
}

/// GET /api/jobs/:id/artifacts/:artifactId/data — proxy do objeto do artefato.
///
/// Metadata via manager + objeto via StoragePort (admin).
/// Key = `artifacts/<job_id>/<path>` (D8). Confere `md5` se barato.
pub async fn get_artifact_data(
    State(state): State<AppState>,
    Path((id, artifact_id)): Path<(String, String)>,
) -> Response {
    if parse_uuid(&id).is_none() || parse_uuid(&artifact_id).is_none() {
        return not_found();
    }
    // 1. Busca artefatos do job para encontrar o path/md5 pelo artifactId.
    let arts = match state.manager.list_artifacts(&id).await {
        Ok(v) => v,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
    };
    let art = match arts.iter().find(|a| a.id == artifact_id) {
        Some(a) => a,
        None => return not_found(),
    };
    // Defesa em profundidade: valida path do artefato.
    if let Err(resp) = validate_artifact_path(&art.path) {
        return resp;
    }
    let key = format!("artifacts/{id}/{}", art.path);
    // 2. Busca objeto via StoragePort (admin).
    let bytes = match state.storage.get(&key).await {
        Ok(b) => b,
        Err(StorageError::NotFound) => {
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "storage_unavailable",
                MSG_STORAGE_UNAVAILABLE,
            );
        }
        Err(StorageError::Unavailable(_)) => return storage_unavailable(),
    };
    // 3. Confere md5 se barato (bytes já em RAM).
    let computed = format!(
        "{:x}",
        md5::Digest::finalize({
            use md5::Digest;
            let mut h = md5::Md5::new();
            md5::Digest::update(&mut h, &bytes);
            h
        })
    );
    if computed != art.md5 {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_unavailable",
            MSG_STORAGE_UNAVAILABLE,
        );
    }
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable",
            ),
        ],
        bytes,
    )
        .into_response()
}

/// GET /api/telemetry — telemetria do manager (proxy puro do cache de heartbeat).
///
/// Delta consciente: 503 além de 200/401 — a D7 original (:355) não listava
/// explicitamente o 503 para telemetry; o docs-sync cobre depois.
pub async fn get_telemetry(State(state): State<AppState>) -> Response {
    let t = match state.manager.get_telemetry().await {
        Ok(v) => v,
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let resp = TelemetryResponse {
        measured: t.measured,
        vram_used: t.vram_used,
        vram_total: t.vram_total,
        cpu: t.cpu,
        ram: t.ram,
        gpus: t.gpus,
        jobs_active: t.jobs_active,
    };
    (StatusCode::OK, Json(resp)).into_response()
}

// ---------------------------------------------------------------------------
// Tests unitários
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::manager_client::{InternalArtifact, InternalJob, MockManager};

    #[test]
    fn remap_metrics_camel_case() {
        let raw = serde_json::json!({
            "items": [
                {
                    "epoch": 1,
                    "box_loss": 0.5,
                    "cls_loss": 0.3,
                    "dfl_loss": 0.2,
                    "mAP50": 0.8,
                    "mAP50-95": 0.6
                },
                {
                    "epoch": 2,
                    "box_loss": 0.4,
                    "cls_loss": 0.2,
                    "dfl_loss": 0.1,
                    "mAP50": 0.9,
                    "mAP50-95": 0.7
                }
            ]
        });
        let items = remap_metrics(&raw);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].epoch, 1);
        assert_eq!(items[0].box_loss, 0.5);
        assert_eq!(items[0].cls_loss, 0.3);
        assert_eq!(items[0].dfl_loss, 0.2);
        assert_eq!(items[0].map50, 0.8);
        assert_eq!(items[0].map5095, 0.6);
        assert_eq!(items[1].epoch, 2);
        assert_eq!(items[1].map5095, 0.7);
    }

    #[test]
    fn remap_metrics_empty_items() {
        let raw = serde_json::json!({ "items": [] });
        let items = remap_metrics(&raw);
        assert!(items.is_empty());
    }

    #[test]
    fn remap_metrics_no_items_key() {
        let raw = serde_json::json!({});
        let items = remap_metrics(&raw);
        assert!(items.is_empty());
    }

    #[test]
    fn remap_metrics_missing_epoch_skipped() {
        let raw = serde_json::json!({
            "items": [
                { "box_loss": 0.5, "cls_loss": 0.3, "dfl_loss": 0.2, "mAP50": 0.8, "mAP50-95": 0.6 }
            ]
        });
        let items = remap_metrics(&raw);
        assert!(items.is_empty());
    }

    #[test]
    fn to_job_response_snake_to_camel() {
        let job = InternalJob {
            id: "550e8400-e29b-41d4-a716-446655440000".into(),
            kind: "yolo_train".into(),
            engine: "yolo".into(),
            model: "yolo11m".into(),
            mode: "train".into(),
            dataset_id: Some("550e8400-e29b-41d4-a716-446655440001".into()),
            status: "running".into(),
            queue_reason: None,
            queue_position: None,
            progress: Some(0.5),
            epoch: Some(5),
            step: Some(100),
            metrics: None,
            vram_min_gb: Some(4),
            orchestrator_id: Some("550e8400-e29b-41d4-a716-446655440002".into()),
            created_at: "2026-01-01T00:00:00Z".into(),
            finished_at: None,
        };
        let resp = to_job_response(job);
        assert_eq!(resp.id, "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(resp.kind, "yolo_train");
        assert_eq!(resp.status, "running");
        assert_eq!(resp.progress, Some(0.5));
        assert_eq!(resp.epoch, Some(5));
        assert!(resp.metrics.is_none());
    }

    #[test]
    fn validate_artifact_path_rejects_dotdot() {
        assert!(validate_artifact_path("../etc/passwd").is_err());
    }

    #[test]
    fn validate_artifact_path_rejects_absolute() {
        assert!(validate_artifact_path("/etc/passwd").is_err());
    }

    #[test]
    fn validate_artifact_path_rejects_backslash() {
        assert!(validate_artifact_path("..\\windows").is_err());
    }

    #[test]
    fn validate_artifact_path_accepts_normal() {
        assert!(validate_artifact_path("best.pt").is_ok());
        assert!(validate_artifact_path("subdir/best.pt").is_ok());
    }

    #[tokio::test]
    async fn list_jobs_handler_200() {
        let mut mock = MockManager::default();
        mock.list_jobs_result = Some((vec![mock_job()], 1));
        let state = test_state(mock);
        let resp = list_jobs(
            axum::extract::State(state),
            Query(JobsQuery {
                status: None,
                engine: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn list_jobs_handler_503_manager_offline() {
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let resp = list_jobs(
            axum::extract::State(state),
            Query(JobsQuery {
                status: None,
                engine: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn list_queue_handler_200() {
        let mut mock = MockManager::default();
        mock.list_queue_result = Some(vec![crate::jobs::manager_client::InternalQueueItem {
            job_id: "550e8400-e29b-41d4-a716-446655440000".into(),
            position: 1,
            queue_reason: None,
        }]);
        let state = test_state(mock);
        let resp = list_queue(axum::extract::State(state)).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_job_handler_200() {
        let mut mock = MockManager::default();
        mock.get_job_result = Some(mock_job());
        let state = test_state(mock);
        let resp = get_job(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_job_handler_404_non_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = get_job(axum::extract::State(state), Path("not-a-uuid".to_string())).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_job_handler_404_not_found() {
        let mut mock = MockManager::default();
        mock.get_job_result = None; // NotFound
        let state = test_state(mock);
        let resp = get_job(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_job_handler_503_manager_offline() {
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let resp = get_job(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn get_job_metrics_200_empty() {
        let mut mock = MockManager::default();
        let mut job = mock_job();
        job.metrics = None;
        mock.get_job_result = Some(job);
        let state = test_state(mock);
        let resp = get_job_metrics(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_job_metrics_200_with_data() {
        let mut mock = MockManager::default();
        let mut job = mock_job();
        job.metrics = Some(serde_json::json!({
            "items": [{
                "epoch": 1,
                "box_loss": 0.5,
                "cls_loss": 0.3,
                "dfl_loss": 0.2,
                "mAP50": 0.8,
                "mAP50-95": 0.6
            }]
        }));
        mock.get_job_result = Some(job);
        let state = test_state(mock);
        let resp = get_job_metrics(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn list_artifacts_handler_200() {
        let mut mock = MockManager::default();
        mock.list_artifacts_result = Some(vec![InternalArtifact {
            id: "550e8400-e29b-41d4-a716-446655440003".into(),
            kind: "model".into(),
            path: "best.pt".into(),
            md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
            bytes: 1024,
        }]);
        let state = test_state(mock);
        let resp = list_artifacts(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_telemetry_handler_200() {
        let mut mock = MockManager::default();
        mock.get_telemetry_result = Some(crate::jobs::manager_client::InternalTelemetry {
            measured: false,
            vram_used: None,
            vram_total: None,
            cpu: Some(0.5),
            ram: Some(1024),
            gpus: vec![],
            jobs_active: 0,
        });
        let state = test_state(mock);
        let resp = get_telemetry(axum::extract::State(state)).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_telemetry_handler_503_manager_offline() {
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let resp = get_telemetry(axum::extract::State(state)).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // --- helpers ---

    fn mock_job() -> InternalJob {
        InternalJob {
            id: "550e8400-e29b-41d4-a716-446655440000".into(),
            kind: "yolo_train".into(),
            engine: "yolo".into(),
            model: "yolo11m".into(),
            mode: "train".into(),
            dataset_id: None,
            status: "queued".into(),
            queue_reason: None,
            queue_position: None,
            progress: None,
            epoch: None,
            step: None,
            metrics: None,
            vram_min_gb: None,
            orchestrator_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            finished_at: None,
        }
    }

    fn test_state(manager: MockManager) -> crate::state::AppState {
        crate::state::AppState {
            pool: sqlx::PgPool::connect_lazy("postgres://n/n").expect("lazy"),
            jwt_secret: [0x42; 32],
            secure_cookie: false,
            setup_required: false,
            storage: std::sync::Arc::new(crate::storage::MockStorage::new()),
            storage_config: crate::storage::StorageConfig {
                bucket: "heph-test".into(),
                public_endpoint: None,
                url_ttl_secs: 60,
            },
            embedder: std::sync::Arc::new(crate::search::MockEmbedder::new()),
            embedding_model: "ViT-B-32".to_string(),
            manager: std::sync::Arc::new(manager),
        }
    }
}
