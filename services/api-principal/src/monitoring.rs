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
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

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
}

/// Lista de orquestradores.
#[derive(Debug, Serialize)]
pub struct OrchestratorListResponse {
    pub items: Vec<OrchestratorResponse>,
}

/// Peso/modelo (camelCase wire — D2). `name` = basename do `path`.
#[derive(Debug, Serialize)]
pub struct ModelWeightResponse {
    pub id: String,
    pub name: String,
    pub engine: String,
    pub model: String,
    #[serde(rename = "jobId")]
    pub job_id: String,
    pub bytes: i64,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

/// Lista de modelos/pesos.
#[derive(Debug, Serialize)]
pub struct ModelListResponse {
    pub items: Vec<ModelWeightResponse>,
}

/// Uso de storage (camelCase wire — D3).
#[derive(Debug, Serialize)]
pub struct StorageUsageResponse {
    #[serde(rename = "datasetsBytes")]
    pub datasets_bytes: i64,
    #[serde(rename = "artifactsBytes")]
    pub artifacts_bytes: i64,
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
        })
        .collect();
    (StatusCode::OK, Json(OrchestratorListResponse { items })).into_response()
}

/// GET /api/models — pesos derivados de job_artifacts (D2).
pub async fn get_models(State(state): State<AppState>) -> Response {
    let items = match state.manager.list_models().await {
        Ok(v) => v,
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let items = items
        .into_iter()
        .map(|m| ModelWeightResponse {
            id: m.id,
            name: basename(&m.path).to_string(),
            engine: m.engine,
            model: m.model,
            job_id: m.job_id,
            bytes: m.bytes,
            created_at: m.created_at,
        })
        .collect();
    (StatusCode::OK, Json(ModelListResponse { items })).into_response()
}

/// GET /api/storage/usage — soma atômica de datasets + artifacts (D3).
///
/// 200 é atômico: qualquer fonte (pool OU manager) falha ⇒ 503 `queue_unavailable`,
/// sem resposta parcial.
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

    // 2. artifactsBytes: via manager (dono = manager).
    let artifacts_bytes = match state.manager.get_storage_usage().await {
        Ok(v) => v.artifacts_bytes,
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };

    let total_bytes = datasets_bytes + artifacts_bytes;

    (
        StatusCode::OK,
        Json(StorageUsageResponse {
            datasets_bytes,
            artifacts_bytes,
            total_bytes,
            measured: true,
        }),
    )
        .into_response()
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
        }
    }

    fn mock_model() -> InternalModel {
        InternalModel {
            id: "550e8400-e29b-41d4-a716-446655440003".into(),
            job_id: "550e8400-e29b-41d4-a716-446655440004".into(),
            path: "best.pt".into(),
            bytes: 110,
            engine: "yolo".into(),
            model: "yolo11m".into(),
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
        model.path = "outputs/123/best.pt".into();
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
        });
        let state = test_state(mock);
        let resp = get_storage_usage(axum::extract::State(state)).await;
        // The handler queries the pool which will fail (lazy, unreachable),
        // so we expect 503. This test only validates the wire shape when
        // the pool succeeds — see test-db for that.
        // But we can test the camelCase shape via the response type directly.
        let r = StorageUsageResponse {
            datasets_bytes: 100,
            artifacts_bytes: 500,
            total_bytes: 600,
            measured: true,
        };
        let json = serde_json::to_value(&r).unwrap();
        assert!(json.get("datasetsBytes").is_some());
        assert!(json.get("artifactsBytes").is_some());
        assert!(json.get("totalBytes").is_some());
        assert!(json.get("measured").is_some());
        assert!(json.get("datasets_bytes").is_none(), "leaked snake_case");
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
