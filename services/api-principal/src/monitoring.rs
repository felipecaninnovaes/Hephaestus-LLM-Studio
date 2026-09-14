//! Handlers de monitoramento (F6.1b — ADR-0009 D1/D2/D3).
//!
//! Rotas públicas BFF:
//! - `GET /api/orchestrators` — lista real da tabela do manager (D1)
//! - `GET /api/models` — pesos derivados de `job_artifacts.kind='model'` (D2)
//! - `GET /api/storage/usage` — soma SQL por dono (D3)
//!
//! O principal NÃO lê tabelas do manager — tudo via `ManagerPort`.
//! `datasetsBytes` é calculado localmente (pool do principal).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::error::{err, MSG_QUEUE_UNAVAILABLE};
use crate::jobs::manager_client::ManagerError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Wire types (camelCase — ADR-0002 D1)
// ---------------------------------------------------------------------------

/// Orquestrador (camelCase wire — D1).
#[derive(Debug, Serialize)]
pub struct OrchestratorResponse {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub endpoint: String,
    pub status: String,
    #[serde(rename = "lastHeartbeat")]
    pub last_heartbeat: Option<String>,
    /// Telemetria por nó (H.4 — ADR-0011 D2).
    pub measured: bool,
    pub cpu: Option<f64>,
    pub ram: Option<i64>,
    #[serde(rename = "ramTotal")]
    pub ram_total: Option<i64>,
    #[serde(rename = "vramUsed")]
    pub vram_used: Option<i64>,
    #[serde(rename = "vramTotal")]
    pub vram_total: Option<i64>,
    #[serde(rename = "vramTotalGb")]
    pub vram_total_gb: Option<i32>,
    pub gpus: Vec<String>,
    #[serde(rename = "jobsActive")]
    pub jobs_active: i32,
}

/// Lista de orquestradores.
#[derive(Debug, Serialize)]
pub struct OrchestratorListResponse {
    pub items: Vec<OrchestratorResponse>,
}

/// Peso/modelo (camelCase wire — D2/D6 ADR-0012).
/// `name` = basename do `path` (s3_key); `model`/`jobId`/`url` nullable.
#[derive(Debug, Serialize)]
pub struct ModelWeightResponse {
    pub id: String,
    pub name: String,
    pub engine: String,
    /// Variante conhecida (treino); null para upload/download (D6).
    pub model: Option<String>,
    pub source: String,
    pub bytes: i64,
    pub md5: String,
    /// Presigned GET URL; null sem `S3_PUBLIC_ENDPOINT_URL` (D6).
    pub url: Option<String>,
    /// PK do job de treino; null para upload/download (D6).
    #[serde(rename = "jobId")]
    pub job_id: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

/// Lista de modelos/pesos.
#[derive(Debug, Serialize)]
pub struct ModelListResponse {
    pub items: Vec<ModelWeightResponse>,
}

/// Uso de storage (camelCase wire — D3/D8 ADR-0012).
#[derive(Debug, Serialize)]
pub struct StorageUsageResponse {
    #[serde(rename = "datasetsBytes")]
    pub datasets_bytes: i64,
    #[serde(rename = "artifactsBytes")]
    pub artifacts_bytes: i64,
    #[serde(rename = "modelsBytes")]
    pub models_bytes: i64,
    #[serde(rename = "totalBytes")]
    pub total_bytes: i64,
    pub measured: bool,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn queue_unavailable() -> Response {
    err(
        StatusCode::SERVICE_UNAVAILABLE,
        "queue_unavailable",
        MSG_QUEUE_UNAVAILABLE,
    )
}

/// Extrai o basename de um path (parte após o último `/`).
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/orchestrators — lista de orquestradores (D1).
pub async fn get_orchestrators(State(state): State<AppState>) -> Response {
    let items = match state.manager.list_orchestrators().await {
        Ok(v) => v,
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let items = items
        .into_iter()
        .map(|o| OrchestratorResponse {
            id: o.id,
            name: o.name,
            kind: o.kind,
            endpoint: o.endpoint,
            status: o.status,
            last_heartbeat: o.last_heartbeat,
            measured: o.measured,
            cpu: o.cpu,
            ram: o.ram,
            ram_total: o.ram_total,
            vram_used: o.vram_used,
            vram_total: o.vram_total,
            vram_total_gb: o.vram_total_gb,
            gpus: o.gpus,
            jobs_active: o.jobs_active,
        })
        .collect();
    (StatusCode::OK, Json(OrchestratorListResponse { items })).into_response()
}

/// GET /api/models — modelos da tabela `models` (D2/D6 ADR-0012).
///
/// `url`: presigned GET do `s3_key` (híbrido — sem `S3_PUBLIC_ENDPOINT_URL`
/// ⇒ `null`). Para checkpoints de treino (`artifacts/`), gera presigned do
/// objeto; para upload/download (`models/`), idem.
pub async fn get_models(State(state): State<AppState>) -> Response {
    let items = match state.manager.list_models().await {
        Ok(v) => v,
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let mut wire_items = Vec::with_capacity(items.len());
    for m in items {
        // Presign URL do s3_key (híbrido D6 — sem endpoint público ⇒ null).
        let url = match state.storage.presign_get(&m.path).await {
            Ok(u) => Some(u),
            Err(_) => None,
        };
        wire_items.push(ModelWeightResponse {
            id: m.id,
            name: if m.name.is_empty() {
                basename(&m.path).to_string()
            } else {
                m.name
            },
            engine: m.engine,
            model: m.model,
            source: m.source,
            bytes: m.bytes,
            md5: m.md5,
            url,
            job_id: m.job_id,
            created_at: m.created_at,
        });
    }
    (
        StatusCode::OK,
        Json(ModelListResponse { items: wire_items }),
    )
        .into_response()
}

/// GET /api/storage/usage — soma atômica de datasets + artifacts + models (D3/D8).
///
/// 200 é atômico: qualquer fonte (pool OU manager) falha ⇒ 503 `queue_unavailable`,
/// sem resposta parcial. `artifactsBytes` exclui `kind='model'` (D8).
pub async fn get_storage_usage(State(state): State<AppState>) -> Response {
    // 1. datasetsBytes: SQL no principal (dono = principal).
    let datasets_bytes: i64 = match sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(SUM(size_bytes), 0)::bigint FROM datasets",
    )
    .fetch_one(&state.pool)
    .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("storage_usage: datasets query failed: {e}");
            return queue_unavailable();
        }
    };

    // 2. artifactsBytes + modelsBytes: via manager (dono = manager).
    let usage = match state.manager.get_storage_usage().await {
        Ok(v) => v,
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };

    let total_bytes = datasets_bytes + usage.artifacts_bytes + usage.models_bytes;

    (
        StatusCode::OK,
        Json(StorageUsageResponse {
            datasets_bytes,
            artifacts_bytes: usage.artifacts_bytes,
            models_bytes: usage.models_bytes,
            total_bytes,
            measured: true,
        }),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Adopt / Revoke (H.4 — ADR-0011 D5)
// ---------------------------------------------------------------------------

/// Body de adopt no wire (camelCase — ADR-0002 D1).
#[derive(Debug, Deserialize)]
pub struct AdoptRequest {
    pub name: String,
    pub endpoint: String,
    pub kind: String,
    #[serde(rename = "pairingCode")]
    pub pairing_code: String,
}

/// POST /api/orchestrators/adopt — adota orquestrador via manager.
pub async fn adopt_orchestrator(
    State(state): State<AppState>,
    Json(body): Json<AdoptRequest>,
) -> Response {
    // Validação de domínio LOCAL antes de chamar o manager (400).
    if body.name.is_empty() || body.name.len() > 128 {
        return err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            crate::error::MSG_INVALID_REQUEST,
        );
    }
    if !body.endpoint.starts_with("http://") && !body.endpoint.starts_with("https://") {
        return err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            crate::error::MSG_INVALID_REQUEST,
        );
    }
    if body.kind != "local" && body.kind != "remoto" {
        return err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            crate::error::MSG_INVALID_REQUEST,
        );
    }
    if body.pairing_code.is_empty() || body.pairing_code.len() > 128 {
        return err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            crate::error::MSG_INVALID_REQUEST,
        );
    }

    // Delega ao manager.
    let payload = serde_json::json!({
        "name": body.name,
        "endpoint": body.endpoint,
        "kind": body.kind,
        "pairing_code": body.pairing_code,
    });
    match state.manager.adopt_orchestrator(&payload).await {
        Ok(o) => {
            let resp = OrchestratorResponse {
                id: o.id,
                name: o.name,
                kind: o.kind,
                endpoint: o.endpoint,
                status: o.status,
                last_heartbeat: o.last_heartbeat,
                measured: o.measured,
                cpu: o.cpu,
                ram: o.ram,
                ram_total: o.ram_total,
                vram_used: o.vram_used,
                vram_total: o.vram_total,
                vram_total_gb: o.vram_total_gb,
                gpus: o.gpus,
                jobs_active: o.jobs_active,
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(ManagerError::PairingInvalid) => err(
            StatusCode::CONFLICT,
            "pairing_invalid",
            crate::error::MSG_PAIRING_INVALID,
        ),
        Err(ManagerError::Unavailable(_)) => queue_unavailable(),
        Err(_) => queue_unavailable(),
    }
}

/// POST /api/orchestrators/:id/revoke — revoga orquestrador via manager.
pub async fn revoke_orchestrator(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    // Valida UUID: se não parseia → 404 not_found (D8 ADR-0002).
    if uuid::Uuid::parse_str(&id).is_err() {
        return err(
            StatusCode::NOT_FOUND,
            "not_found",
            crate::error::MSG_NOT_FOUND,
        );
    }
    match state.manager.revoke_orchestrator(&id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(ManagerError::NotFound) => err(
            StatusCode::NOT_FOUND,
            "not_found",
            crate::error::MSG_NOT_FOUND,
        ),
        Err(ManagerError::Unavailable(_)) => queue_unavailable(),
        Err(_) => queue_unavailable(),
    }
}

// ---------------------------------------------------------------------------
// Tests unitários
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::manager_client::{
        InternalModel, InternalOrchestrator, InternalStorageUsage, MockManager,
    };

    fn mock_orchestrator() -> InternalOrchestrator {
        InternalOrchestrator {
            id: "550e8400-e29b-41d4-a716-446655440000".into(),
            name: "orchestrator-local".into(),
            kind: "local".into(),
            endpoint: "http://orchestrator-local:8082".into(),
            status: "online".into(),
            last_heartbeat: Some("2026-09-09T12:00:00Z".into()),
            measured: true,
            cpu: Some(42.5),
            ram: Some(4096),
            ram_total: Some(8192),
            vram_used: Some(3072),
            vram_total: Some(6144),
            vram_total_gb: Some(6),
            gpus: vec!["NVIDIA GeForce GTX 1660 SUPER".into()],
            jobs_active: 1,
        }
    }

    fn mock_model() -> InternalModel {
        InternalModel {
            id: "550e8400-e29b-41d4-a716-446655440003".into(),
            name: "best.pt".into(),
            engine: "yolo".into(),
            model: Some("yolo11m".into()),
            source: "train".into(),
            md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
            bytes: 110,
            path: "artifacts/550e8400-e29b-41d4-a716-446655440004/best.pt".into(),
            job_id: Some("550e8400-e29b-41d4-a716-446655440004".into()),
            created_at: "2026-09-09T12:00:00Z".into(),
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
            model_download_allowed_hosts: vec![],
        }
    }

    // --- get_orchestrators ---

    #[tokio::test]
    async fn get_orchestrators_200() {
        let mut mock = MockManager::default();
        mock.list_orchestrators_result = Some(vec![mock_orchestrator()]);
        let state = test_state(mock);
        let resp = get_orchestrators(axum::extract::State(state)).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_orchestrators_503_manager_offline() {
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let resp = get_orchestrators(axum::extract::State(state)).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn get_orchestrators_wire_camel_case() {
        let mut mock = MockManager::default();
        mock.list_orchestrators_result = Some(vec![mock_orchestrator()]);
        let state = test_state(mock);
        let resp = get_orchestrators(axum::extract::State(state)).await;
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let items = json["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert!(item.get("lastHeartbeat").is_some(), "missing lastHeartbeat");
        assert!(
            item.get("last_heartbeat").is_none(),
            "leaked snake_case last_heartbeat"
        );
        assert_eq!(item["kind"], "local");
    }

    // --- get_models ---

    #[tokio::test]
    async fn get_models_200() {
        let mut mock = MockManager::default();
        mock.list_models_result = Some(vec![mock_model()]);
        let state = test_state(mock);
        let resp = get_models(axum::extract::State(state)).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_models_503_manager_offline() {
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let resp = get_models(axum::extract::State(state)).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn get_models_wire_camel_case_and_basename() {
        let mut mock = MockManager::default();
        let mut model = mock_model();
        model.path = "artifacts/123/best.pt".into();
        model.job_id = Some("550e8400-e29b-41d4-a716-446655440004".into());
        mock.list_models_result = Some(vec![model]);
        let state = test_state(mock);
        let resp = get_models(axum::extract::State(state)).await;
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let items = json["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item["name"], "best.pt", "name should be basename");
        assert!(item.get("jobId").is_some(), "missing jobId");
        assert!(item.get("job_id").is_none(), "leaked snake_case job_id");
        assert!(item.get("createdAt").is_some(), "missing createdAt");
        assert!(
            item.get("created_at").is_none(),
            "leaked snake_case created_at"
        );
        // D6 novos campos
        assert_eq!(item["source"], "train");
        assert!(item.get("md5").is_some(), "missing md5");
        assert!(item.get("model").is_some(), "missing model");
        // url: MockStorage não tem presign real → null
        assert!(item.get("url").is_some(), "missing url key");
    }

    #[tokio::test]
    async fn get_models_empty_items() {
        let mut mock = MockManager::default();
        mock.list_models_result = Some(vec![]);
        let state = test_state(mock);
        let resp = get_models(axum::extract::State(state)).await;
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["items"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn get_models_nullable_model_and_job_id() {
        // Upload/download: model=null, jobId=null (D6 ADR-0012).
        let mut mock = MockManager::default();
        let model = InternalModel {
            id: "550e8400-e29b-41d4-a716-446655440005".into(),
            name: "custom.pt".into(),
            engine: "yolo".into(),
            model: None,
            source: "upload".into(),
            md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
            bytes: 2048,
            path: "models/yolo/550e8400-e29b-41d4-a716-446655440005/custom.pt".into(),
            job_id: None,
            created_at: "2026-09-10T12:00:00Z".into(),
        };
        mock.list_models_result = Some(vec![model]);
        let state = test_state(mock);
        let resp = get_models(axum::extract::State(state)).await;
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let items = json["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item["source"], "upload");
        // model e jobId nullable — devem ser null, não ausentes.
        assert!(
            item.get("model").is_some(),
            "model key missing (should be null)"
        );
        assert!(item["model"].is_null(), "model should be null for upload");
        assert!(
            item.get("jobId").is_some(),
            "jobId key missing (should be null)"
        );
        assert!(item["jobId"].is_null(), "jobId should be null for upload");
    }

    // --- get_storage_usage ---

    #[tokio::test]
    async fn get_storage_usage_503_manager_offline() {
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let _resp = get_storage_usage(axum::extract::State(state)).await;
        assert_eq!(_resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn get_storage_usage_wire_camel_case() {
        let mut mock = MockManager::default();
        mock.get_storage_usage_result = Some(InternalStorageUsage {
            artifacts_bytes: 500,
            models_bytes: 200,
        });
        let state = test_state(mock);
        let _resp = get_storage_usage(axum::extract::State(state)).await;
        // The handler queries the pool which will fail (lazy, unreachable),
        // so we expect 503. This test only validates the wire shape when
        // the pool succeeds — see test-db for that.
        // But we can test the camelCase shape via the response type directly.
        let r = StorageUsageResponse {
            datasets_bytes: 100,
            artifacts_bytes: 500,
            models_bytes: 200,
            total_bytes: 800,
            measured: true,
        };
        let json = serde_json::to_value(&r).unwrap();
        assert!(json.get("datasetsBytes").is_some());
        assert!(json.get("artifactsBytes").is_some());
        assert!(json.get("modelsBytes").is_some());
        assert!(json.get("totalBytes").is_some());
        assert!(json.get("measured").is_some());
        assert!(json.get("datasets_bytes").is_none(), "leaked snake_case");
        assert!(json.get("models_bytes").is_none(), "leaked snake_case");
    }

    // --- get_orchestrators enriched ---

    #[tokio::test]
    async fn get_orchestrators_200_enriched() {
        let mut mock = MockManager::default();
        mock.list_orchestrators_result = Some(vec![mock_orchestrator()]);
        let state = test_state(mock);
        let resp = get_orchestrators(axum::extract::State(state)).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let items = json["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item["measured"], true);
        assert_eq!(item["cpu"], 42.5);
        assert_eq!(item["ram"], 4096);
        assert_eq!(item["ramTotal"], 8192);
        assert_eq!(item["vramUsed"], 3072);
        assert_eq!(item["vramTotal"], 6144);
        assert_eq!(item["vramTotalGb"], 6);
        assert!(item["gpus"].as_array().unwrap().len() > 0);
        assert_eq!(item["jobsActive"], 1);
    }

    // --- adopt_orchestrator ---

    #[tokio::test]
    async fn adopt_orchestrator_200() {
        let mut mock = MockManager::default();
        mock.adopt_orchestrator_result = Some(mock_orchestrator());
        let state = test_state(mock);
        let resp = adopt_orchestrator(
            axum::extract::State(state),
            axum::extract::Json(AdoptRequest {
                name: "orchestrator-remote".into(),
                endpoint: "http://10.0.0.1:8082".into(),
                kind: "remoto".into(),
                pairing_code: "heph_p_test123".into(),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn adopt_orchestrator_400_empty_name() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = adopt_orchestrator(
            axum::extract::State(state),
            axum::extract::Json(AdoptRequest {
                name: "".into(),
                endpoint: "http://10.0.0.1:8082".into(),
                kind: "remoto".into(),
                pairing_code: "heph_p_test123".into(),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn adopt_orchestrator_400_invalid_endpoint() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = adopt_orchestrator(
            axum::extract::State(state),
            axum::extract::Json(AdoptRequest {
                name: "test".into(),
                endpoint: "ftp://invalid".into(),
                kind: "remoto".into(),
                pairing_code: "heph_p_test123".into(),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn adopt_orchestrator_400_invalid_kind() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = adopt_orchestrator(
            axum::extract::State(state),
            axum::extract::Json(AdoptRequest {
                name: "test".into(),
                endpoint: "http://10.0.0.1:8082".into(),
                kind: "gpu".into(),
                pairing_code: "heph_p_test123".into(),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn adopt_orchestrator_400_empty_pairing_code() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = adopt_orchestrator(
            axum::extract::State(state),
            axum::extract::Json(AdoptRequest {
                name: "test".into(),
                endpoint: "http://10.0.0.1:8082".into(),
                kind: "remoto".into(),
                pairing_code: "".into(),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn adopt_orchestrator_409_pairing_invalid() {
        let mut mock = MockManager::default();
        mock.fail_adopt_pairing = true;
        let state = test_state(mock);
        let resp = adopt_orchestrator(
            axum::extract::State(state),
            axum::extract::Json(AdoptRequest {
                name: "test".into(),
                endpoint: "http://10.0.0.1:8082".into(),
                kind: "remoto".into(),
                pairing_code: "wrong_code".into(),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "pairing_invalid");
    }

    #[tokio::test]
    async fn adopt_orchestrator_503_manager_offline() {
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let resp = adopt_orchestrator(
            axum::extract::State(state),
            axum::extract::Json(AdoptRequest {
                name: "test".into(),
                endpoint: "http://10.0.0.1:8082".into(),
                kind: "remoto".into(),
                pairing_code: "heph_p_test123".into(),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // --- revoke_orchestrator ---

    #[tokio::test]
    async fn revoke_orchestrator_204() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = revoke_orchestrator(
            axum::extract::State(state),
            axum::extract::Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn revoke_orchestrator_404_non_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = revoke_orchestrator(
            axum::extract::State(state),
            axum::extract::Path("not-a-uuid".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn revoke_orchestrator_404_not_found() {
        let mut mock = MockManager::default();
        mock.revoke_not_found = true;
        let state = test_state(mock);
        let resp = revoke_orchestrator(
            axum::extract::State(state),
            axum::extract::Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn revoke_orchestrator_503_manager_offline() {
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let resp = revoke_orchestrator(
            axum::extract::State(state),
            axum::extract::Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // --- basename ---

    #[test]
    fn basename_simple() {
        assert_eq!(basename("best.pt"), "best.pt");
    }

    #[test]
    fn basename_with_path() {
        assert_eq!(basename("outputs/123/best.pt"), "best.pt");
    }

    #[test]
    fn basename_nested() {
        assert_eq!(basename("a/b/c/last.pt"), "last.pt");
    }
}
