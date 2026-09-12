//! Manager service — axum routes + boot (F4.3).
//!
//! `main.rs` é a camada fina: tracing, DB pool com retry, boot (adopt + recover),
//! rotas axum, dispatch worker, e servidor HTTP.

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use sqlx::PgPool;
use std::sync::Arc;
use tower_http::trace::TraceLayer;

use manager::{
    self, AbortResponse, AdoptRequest, ArtifactsListResponse, HeartbeatRequest,
    HttpOrchestratorClient, ManagerError, OrchestratorClient, ReportRequest, TelemetryCache,
    VramTable,
};

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct AppState {
    pool: PgPool,
    token: String,
    telemetry_cache: TelemetryCache,
    orch_client: Arc<dyn OrchestratorClient>,
    exec_mode: String,
    orch_workdir: String,
    trainer_image: String,
    vram_table: VramTable,
}

// ---------------------------------------------------------------------------
// Query params
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct ListJobsQuery {
    status: Option<String>,
    engine: Option<String>,
}

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        Json(serde_json::json!({"code": code, "message": message})),
    )
        .into_response()
}

fn not_found() -> Response {
    error_response(StatusCode::NOT_FOUND, "not_found", "resource not found")
}

fn not_abortable() -> Response {
    error_response(
        StatusCode::CONFLICT,
        "job_not_abortable",
        "job is in a terminal state",
    )
}

fn internal_error(msg: &str) -> Response {
    error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal", msg)
}

fn bad_request(msg: &str) -> Response {
    error_response(StatusCode::BAD_REQUEST, "invalid_request", msg)
}

fn pairing_invalid() -> Response {
    error_response(
        StatusCode::CONFLICT,
        "pairing_invalid",
        "código de pareamento inválido ou orquestrador inalcançável",
    )
}

// ---------------------------------------------------------------------------
// Middleware: x-request-id + tracing
// ---------------------------------------------------------------------------

async fn request_id_middleware(req: axum::http::Request<axum::body::Body>, next: Next) -> Response {
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    let method = req.method().clone();
    let uri = req.uri().clone();

    let span = tracing::info_span!(
        "request",
        request_id = %request_id,
        method = %method,
        path = %uri,
    );
    let _guard = span.enter();

    let start = std::time::Instant::now();
    let response = next.run(req).await;
    let duration = start.elapsed();

    tracing::info!(
        status = response.status().as_u16(),
        duration_ms = duration.as_millis() as u64,
        "request completed"
    );

    let mut response = response;
    if let Ok(val) = request_id.parse() {
        response.headers_mut().insert("x-request-id", val);
    }
    response
}

// ---------------------------------------------------------------------------
// Auth middleware: Bearer MANAGER_TOKEN
// ---------------------------------------------------------------------------

async fn auth_middleware(
    State(state): State<AppState>,
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    let auth = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    match auth {
        Some(token) if token == format!("Bearer {}", state.token) => next.run(req).await,
        _ => error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "invalid or missing token",
        ),
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "service": "manager" }))
}

async fn ready(State(state): State<AppState>) -> Response {
    // DB check.
    let db_ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .is_ok();

    if db_ok {
        (StatusCode::OK, Json(serde_json::json!({"status": "ready"}))).into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"status": "not ready"})),
        )
            .into_response()
    }
}

/// POST /internal/jobs — cria job.
async fn create_job_handler(State(state): State<AppState>, body: Bytes) -> Response {
    if body.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request", "empty body");
    }
    let req: manager::CreateJobRequest = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("invalid json: {e}"),
            )
        }
    };

    match manager::create_job(&state.pool, req).await {
        Ok(resp) => (StatusCode::ACCEPTED, Json(resp)).into_response(),
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::InvalidRequest(ref msg)) => {
            error_response(StatusCode::BAD_REQUEST, "invalid_request", msg)
        }
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// GET /internal/jobs — lista jobs.
///
/// SEMPRE retorna `{items: [...], total: N}` onde cada item carrega os campos
/// do job + `queue_position` (posição na fila se status=queued, senão null) +
/// `queue_reason`. Query params opcionais filtram por status/engine.
async fn list_jobs_handler(
    State(state): State<AppState>,
    Query(params): Query<ListJobsQuery>,
) -> Response {
    match manager::list_jobs(
        &state.pool,
        params.status.as_deref(),
        params.engine.as_deref(),
    )
    .await
    {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// GET /internal/jobs/:id — detalhe de um job.
async fn get_job_handler(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let uuid = match id.parse::<uuid::Uuid>() {
        Ok(u) => u,
        Err(_) => return not_found(),
    };

    match manager::get_job(&state.pool, uuid).await {
        Ok(job) => (StatusCode::OK, Json(job)).into_response(),
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// GET /internal/jobs/:id/artifacts — lista artefatos de um job.
async fn list_artifacts_handler(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let uuid = match id.parse::<uuid::Uuid>() {
        Ok(u) => u,
        Err(_) => return not_found(),
    };

    match manager::get_job_artifacts(&state.pool, uuid).await {
        Ok(arts) => (StatusCode::OK, Json(ArtifactsListResponse { items: arts })).into_response(),
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// POST /internal/jobs/:id/abort — aborta um job.
async fn abort_job_handler(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let uuid = match id.parse::<uuid::Uuid>() {
        Ok(u) => u,
        Err(_) => return not_found(),
    };

    match manager::abort_job(&state.pool, uuid, state.orch_client.as_ref()).await {
        Ok(status) => (StatusCode::OK, Json(AbortResponse { status })).into_response(),
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::NotAbortable) => not_abortable(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(ManagerError::InvalidRequest(msg)) => bad_request(&msg),
        Err(ManagerError::PairingInvalid) => internal_error("unexpected pairing_invalid"),
    }
}

/// POST /internal/jobs/:id/report — report do orquestrador.
async fn report_job_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let uuid = match id.parse::<uuid::Uuid>() {
        Ok(u) => u,
        Err(_) => return not_found(),
    };

    if body.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request", "empty body");
    }
    let req: ReportRequest = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("invalid json: {e}"),
            )
        }
    };

    match manager::report_job(&state.pool, uuid, req).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// POST /internal/heartbeat — heartbeat do orquestrador.
async fn heartbeat_handler(State(state): State<AppState>, body: Bytes) -> Response {
    if body.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request", "empty body");
    }
    let req: HeartbeatRequest = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("invalid json: {e}"),
            )
        }
    };

    match manager::receive_heartbeat(&state.pool, &state.telemetry_cache, req).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// GET /internal/telemetry — telemetria do cache de heartbeat.
async fn telemetry_handler(State(state): State<AppState>) -> Response {
    let resp = manager::get_telemetry(&state.pool, &state.telemetry_cache).await;
    (StatusCode::OK, Json(resp)).into_response()
}

/// GET /internal/orchestrators — lista orquestradores com telemetria por nó.
async fn list_orchestrators_handler(State(state): State<AppState>) -> Response {
    match manager::list_orchestrators(&state.pool, &state.telemetry_cache).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// POST /internal/adopt — adopt de orquestrador remoto/local via pairing code.
async fn adopt_handler(State(state): State<AppState>, body: Bytes) -> Response {
    if body.is_empty() {
        return bad_request("empty body");
    }
    let req: AdoptRequest = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return bad_request(&format!("invalid json: {e}")),
    };

    match manager::adopt_internal(&state.pool, state.orch_client.as_ref(), &req).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(ManagerError::InvalidRequest(msg)) => bad_request(&msg),
        Err(ManagerError::PairingInvalid) => pairing_invalid(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// POST /internal/orchestrators/:id/revoke — revoke orquestrador.
async fn revoke_handler(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let uuid = match id.parse::<uuid::Uuid>() {
        Ok(u) => u,
        Err(_) => return not_found(),
    };

    match manager::revoke_orchestrator(&state.pool, uuid).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// GET /internal/models — modelos derivados de job_artifacts.kind='model'.
async fn list_models_handler(State(state): State<AppState>) -> Response {
    match manager::list_models(&state.pool).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// POST /internal/models — cria row na tabela models (ADR-0012 D1/I.2b).
async fn create_model_handler(State(state): State<AppState>, body: Bytes) -> Response {
    if body.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request", "empty body");
    }
    let req: manager::CreateModelRequest = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("invalid json: {e}"),
            )
        }
    };

    match manager::create_model(&state.pool, req).await {
        Ok(item) => (StatusCode::CREATED, Json(item)).into_response(),
        Err(ManagerError::InvalidRequest(msg)) => bad_request(&msg),
        Err(ManagerError::Internal(ref msg)) if msg == "model_exists" => error_response(
            StatusCode::CONFLICT,
            "model_exists",
            "model with this s3_key already exists",
        ),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// GET /internal/storage/usage — soma de bytes de job_artifacts.
async fn get_storage_usage_handler(State(state): State<AppState>) -> Response {
    match manager::get_storage_usage(&state.pool).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// DELETE /internal/models/:id — remove modelo da tabela models.
async fn delete_model_handler(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let uid = match id.parse::<uuid::Uuid>() {
        Ok(u) => u,
        Err(_) => return bad_request("invalid uuid"),
    };
    match manager::delete_model(&state.pool, uid).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::Internal(e)) => internal_error(&e),
        Err(e) => internal_error(&e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

fn build_router(state: AppState) -> Router {
    let api = Router::new()
        .route(
            "/internal/jobs",
            post(create_job_handler).get(list_jobs_handler),
        )
        .route("/internal/jobs/:id", get(get_job_handler))
        .route("/internal/jobs/:id/artifacts", get(list_artifacts_handler))
        .route("/internal/jobs/:id/abort", post(abort_job_handler))
        .route("/internal/jobs/:id/report", post(report_job_handler))
        .route("/internal/heartbeat", post(heartbeat_handler))
        .route("/internal/telemetry", get(telemetry_handler))
        .route("/internal/orchestrators", get(list_orchestrators_handler))
        .route("/internal/adopt", post(adopt_handler))
        .route("/internal/orchestrators/:id/revoke", post(revoke_handler))
        .route(
            "/internal/models",
            get(list_models_handler).post(create_model_handler),
        )
        .route("/internal/models/:id", delete(delete_model_handler))
        .route("/internal/storage/usage", get(get_storage_usage_handler))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .merge(api)
        .layer(middleware::from_fn(request_id_middleware))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    // Tracing.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "manager=info,tower_http=info".into()),
        )
        .json()
        .init();

    // Config.
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL obrigatório");
    let token = std::env::var("MANAGER_TOKEN").unwrap_or_else(|_| "manager-dev-token".into());
    let exec_mode = std::env::var("EXEC_MODE").unwrap_or_else(|_| "docker".into());
    let orch_workdir = std::env::var("ORCH_WORKDIR").unwrap_or_else(|_| "/data".into());
    let trainer_image =
        std::env::var("TRAINER_IMAGE").unwrap_or_else(|_| "hephaestus/trainer-yolo:local".into());
    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "8081".into())
        .parse()
        .expect("PORT deve ser um número");

    // DB pool com retry.
    let pool = loop {
        match PgPool::connect(&database_url).await {
            Ok(p) => {
                tracing::info!("conectado ao Postgres");
                break p;
            }
            Err(e) => {
                tracing::warn!("falha ao conectar no Postgres: {e}, tentando em 2s...");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    };

    // Boot: adopt (AUTO_ADOPT_LOCAL=0 → skip) + recovery.
    if manager::auto_adopt_enabled(std::env::var("AUTO_ADOPT_LOCAL").ok().as_deref()) {
        if let Err(e) = manager::adopt_orchestrator(&pool).await {
            tracing::error!("falha ao auto-adotar orchestrator: {e}");
        } else {
            tracing::info!("orchestrator-local auto-adotado");
        }
    } else {
        tracing::info!("AUTO_ADOPT_LOCAL=0: pulando auto-adoção de orchestrator-local");
    }

    match manager::recover_jobs(&pool).await {
        Ok(n) if n > 0 => tracing::info!("recovery: {n} jobs recuperados para queued"),
        Ok(_) => tracing::info!("recovery: nenhum job órfão"),
        Err(e) => tracing::error!("falha no recovery: {e}"),
    }

    // VRAM table (fail-fast no boot).
    let vram_table_raw = match std::env::var("VRAM_TABLE_PATH") {
        Ok(path) => std::fs::read_to_string(&path).unwrap_or_else(|e| {
            tracing::error!("falha ao ler VRAM_TABLE_PATH={path}: {e}");
            panic!("VRAM_TABLE_PATH legível: {e}");
        }),
        Err(_) => include_str!("../../../packages/policies/vram-table.yaml").to_string(),
    };
    let vram_table: VramTable = VramTable::parse(&vram_table_raw).unwrap_or_else(|e| {
        tracing::error!("falha ao parsear vram-table: {e}");
        panic!("vram-table inválido: {e}");
    });
    tracing::info!(
        "vram-table: {} entradas, headroom={}GB",
        vram_table.entries.len(),
        vram_table.defaults.headroom_gb
    );

    // State.
    let orch_client = Arc::new(HttpOrchestratorClient::new(Some(token.clone())));
    let state = AppState {
        pool: pool.clone(),
        token,
        telemetry_cache: manager::new_telemetry_cache(),
        orch_client,
        exec_mode,
        orch_workdir,
        trainer_image,
        vram_table,
    };

    // Dispatch worker (tokio task).
    let dispatch_pool = pool.clone();
    let dispatch_client: Arc<dyn OrchestratorClient> = state.orch_client.clone();
    let dispatch_mode = state.exec_mode.clone();
    let dispatch_workdir = state.orch_workdir.clone();
    let dispatch_image = state.trainer_image.clone();
    let dispatch_vram = state.vram_table.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        loop {
            interval.tick().await;

            // Watchdog tick (ADR-0011 D4).
            if let Err(e) = manager::watchdog_tick(&dispatch_pool).await {
                tracing::error!("watchdog error: {e}");
            }

            match manager::dispatch_next(
                &dispatch_pool,
                dispatch_client.as_ref(),
                &dispatch_mode,
                &dispatch_workdir,
                &dispatch_image,
                &dispatch_vram,
            )
            .await
            {
                Ok(true) => tracing::info!("dispatch: job despachado"),
                Ok(false) => {} // Nenhum job na fila.
                Err(e) => tracing::error!("dispatch error: {e}"),
            }
        }
    });

    // Server.
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("bind");
    tracing::info!("manager ouvindo em 0.0.0.0:{port}");
    axum::serve(listener, app).await.unwrap();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    struct NoopOrch;

    #[async_trait::async_trait]
    impl OrchestratorClient for NoopOrch {
        async fn post(&self, _url: &str, _body: &serde_json::Value) -> Result<(), String> {
            Ok(())
        }
        async fn post_json(
            &self,
            _url: &str,
            _body: &serde_json::Value,
        ) -> Result<serde_json::Value, String> {
            Ok(serde_json::json!({}))
        }
    }

    fn test_state(pool: PgPool) -> AppState {
        let vram_table = VramTable::parse(
            "defaults:\n  headroom_gb: 2\nentries:\n  - { engine: yolo, model: yolo11n, mode: train, vram_min_gb: 6 }\n",
        )
        .unwrap();
        AppState {
            pool,
            token: "test-token".into(),
            telemetry_cache: manager::new_telemetry_cache(),
            orch_client: Arc::new(NoopOrch),
            exec_mode: "local".into(),
            orch_workdir: "/tmp".into(),
            trainer_image: "trainer:latest".into(),
            vram_table,
        }
    }

    /// POST /internal/jobs com weights_id inexistente → 404 not_found.
    #[sqlx::test(migrations = "../api-principal/migrations")]
    #[ignore = "requer Postgres (bash scripts/test-db.sh)"]
    async fn create_job_handler_not_found(pool: PgPool) {
        let app = build_router(test_state(pool));

        let fake_id = uuid::Uuid::new_v4();
        let body = serde_json::json!({
            "kind": "yolo_train",
            "engine": "yolo",
            "model": "yolo11m",
            "mode": "train",
            "weights_id": fake_id,
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/internal/jobs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(json["code"], "not_found");
    }

    /// POST /internal/jobs com JSON inválido → 400 invalid_request.
    #[sqlx::test(migrations = "../api-principal/migrations")]
    #[ignore = "requer Postgres (bash scripts/test-db.sh)"]
    async fn create_job_handler_invalid_json(pool: PgPool) {
        let app = build_router(test_state(pool));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/internal/jobs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from("not json"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(json["code"], "invalid_request");
    }
}
