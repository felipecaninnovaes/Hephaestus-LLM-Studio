//! Orchestrator service — axum routes + boot (F4.4, ADR-0007).
//!
//! `main.rs` é a camada fina: tracing, env loading, rotas axum,
//! dispatch handler (aceita job e spawna task), abort handler,
//! heartbeat loop, e servidor HTTP.

use axum::{
    body::Bytes,
    extract::State,
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use std::sync::Arc;
use tower_http::trace::TraceLayer;

use orchestrator::{
    self, DispatchRequest, HeartbeatBody, HttpHeartbeatClient, HttpReportClient, ReportBody,
    S3Client,
};

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct AppState {
    s3: Arc<dyn orchestrator::S3Port>,
    report_client: Arc<dyn orchestrator::ReportClient>,
    executor: Arc<dyn orchestrator::TrainerExecutor>,
    active_jobs: orchestrator::ActiveJobs,
    manager_token: Option<String>,
    gpu_devices: Option<String>,
    gpu_allow_mock: bool,
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

fn bad_request(msg: &str) -> Response {
    error_response(StatusCode::BAD_REQUEST, "invalid_request", msg)
}

fn conflict(msg: &str) -> Response {
    error_response(StatusCode::CONFLICT, "job_conflict", msg)
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
// Auth middleware: Bearer MANAGER_TOKEN (opcional na v1)
// ---------------------------------------------------------------------------

async fn auth_middleware(
    State(state): State<AppState>,
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    // Se MANAGER_TOKEN não está configurado, auth é bypass (D4: "Bearer OPCIONAL na v1")
    if state.manager_token.is_none() {
        return next.run(req).await;
    }

    let auth = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    match auth {
        Some(token) if token == format!("Bearer {}", state.manager_token.as_deref().unwrap()) => {
            next.run(req).await
        }
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
    Json(serde_json::json!({ "status": "ok", "service": "orchestrator" }))
}

async fn ready(State(state): State<AppState>) -> Response {
    // /ready = S3 acessível (operação barata — head_bucket).
    // db NÃO existe no orquestrador (stateless).
    if state.s3.ping().await {
        (StatusCode::OK, Json(serde_json::json!({"status": "ready"}))).into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"status": "not ready"})),
        )
            .into_response()
    }
}

/// POST /internal/dispatch — recebe job do manager (D4 :263–267).
async fn dispatch_handler(State(state): State<AppState>, body: Bytes) -> Response {
    if body.is_empty() {
        return bad_request("empty body");
    }

    let req: DispatchRequest = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return bad_request(&format!("invalid json: {e}"));
        }
    };

    // Validação básica do body
    if req.job_id.is_empty() {
        return bad_request("job_id is required");
    }
    if req.engine.is_empty() {
        return bad_request("engine is required");
    }
    if req.image.is_empty() {
        return bad_request("image is required");
    }
    if req.workdir.is_empty() {
        return bad_request("workdir is required");
    }
    if req.package_ref.key.is_empty() {
        return bad_request("package_ref.key is required");
    }
    if req.package_ref.md5_zip.is_empty() {
        return bad_request("package_ref.md5_zip is required");
    }

    // Idempotência R4: se já existe job com MESMO job_id em memória → 409
    if state.active_jobs.contains_key(&req.job_id) {
        return conflict("job already dispatched (duplicate job_id)");
    }

    // Registra job ativo (para idempotência)
    state.active_jobs.insert(
        req.job_id.clone(),
        orchestrator::ActiveJobState::new(String::new()),
    );

    let s3 = Arc::clone(&state.s3);
    let report_client = Arc::clone(&state.report_client);
    let executor = Arc::clone(&state.executor);
    let active_jobs = Arc::clone(&state.active_jobs);
    let gpu_devices = state.gpu_devices.clone();
    let gpu_allow_mock = state.gpu_allow_mock;

    // Spawna pipeline assíncrono (D5/D6/D8)
    tokio::spawn(async move {
        orchestrator::run_job(
            req,
            s3,
            report_client,
            executor,
            active_jobs,
            gpu_devices,
            gpu_allow_mock,
        )
        .await;
    });

    (StatusCode::ACCEPTED, Json(serde_json::json!({"ok": true}))).into_response()
}

/// POST /internal/abort — aborta um job (D4 — manager chama em cancelling).
async fn abort_handler(State(state): State<AppState>, body: Bytes) -> Response {
    if body.is_empty() {
        return bad_request("empty body");
    }

    #[derive(serde::Deserialize)]
    struct AbortReq {
        job_id: String,
    }

    let req: AbortReq = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return bad_request(&format!("invalid json: {e}"));
        }
    };

    // Procura container ativo
    if let Some(entry) = state.active_jobs.get(&req.job_id) {
        entry.cancel();
        if !entry.container_name.is_empty() {
            if let Err(e) = state.executor.stop(&entry.container_name).await {
                tracing::warn!("abort: docker stop failed for {}: {e}", req.job_id);
            }
        }
        tracing::info!("abort: job {} stopped", req.job_id);
    } else {
        // Sem container vivo — reporta falha ou nada (decisão honesta)
        tracing::info!(
            "abort: job {} not found in active jobs (already finished?)",
            req.job_id
        );
        // Reporta failed se o job não está mais ativo
        let _ = state
            .report_client
            .report(
                &req.job_id,
                &ReportBody {
                    status: "failed".to_string(),
                    progress: None,
                    epoch: None,
                    step: None,
                    metrics: None,
                    error: Some("job not found or already finished".to_string()),
                    artifacts: None,
                },
            )
            .await;
    }

    (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response()
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

fn build_router(state: AppState) -> Router {
    let api = Router::new()
        .route("/internal/dispatch", post(dispatch_handler))
        .route("/internal/abort", post(abort_handler))
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
    // Tracing (JSON + env filter, padrão manager).
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "orchestrator=info,tower_http=info".into()),
        )
        .json()
        .init();

    // Config — fail-fast sem ecoar valor (padrão load_storage do principal).
    let s3_endpoint =
        std::env::var("S3_ORCH_ENDPOINT_URL").expect("S3_ORCH_ENDPOINT_URL obrigatório");
    let s3_bucket = std::env::var("S3_ORCH_BUCKET").unwrap_or_else(|_| "heph-data".into());
    let s3_access_key =
        std::env::var("S3_ORCH_ACCESS_KEY").expect("S3_ORCH_ACCESS_KEY obrigatório");
    let s3_secret_key =
        std::env::var("S3_ORCH_SECRET_KEY").expect("S3_ORCH_SECRET_KEY obrigatório");
    let workdir = std::env::var("ORCH_WORKDIR").unwrap_or_else(|_| "/data".into());
    let exec_mode = std::env::var("EXEC_MODE").unwrap_or_else(|_| "docker".into());
    let manager_url = std::env::var("MANAGER_URL").unwrap_or_else(|_| "http://manager:8081".into());
    let manager_token = std::env::var("MANAGER_TOKEN").ok();
    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "8082".into())
        .parse()
        .expect("PORT deve ser um número");

    tracing::info!("orchestrator boot: exec_mode={exec_mode}, workdir={workdir}");

    // S3 client (D2 — cliente escopado).
    let s3 = Arc::new(S3Client::new(
        &s3_endpoint,
        &s3_access_key,
        &s3_secret_key,
        &s3_bucket,
    )) as Arc<dyn orchestrator::S3Port>;

    // Report client (D4 — POST /internal/jobs/:id/report).
    let report_client = Arc::new(HttpReportClient::new(
        &manager_url,
        manager_token.as_deref(),
    )) as Arc<dyn orchestrator::ReportClient>;

    // Heartbeat client (D4/D9 — POST /internal/heartbeat).
    let heartbeat_client = Arc::new(HttpHeartbeatClient::new(
        &manager_url,
        manager_token.as_deref(),
    )) as Arc<dyn orchestrator::HeartbeatClient>;

    // Executor (D5 — docker CLI via socket do host).
    let executor: Arc<dyn orchestrator::TrainerExecutor> = match exec_mode.as_str() {
        "subprocess" => Arc::new(orchestrator::SubprocessExecutor),
        _ => Arc::new(orchestrator::DockerExecutor),
    };

    // State.
    let active_jobs = orchestrator::new_active_jobs();

    // GPU config: lê uma vez no boot (D4/D7).
    let gpu_devices_boot = std::env::var("ORCH_GPU_DEVICES")
        .ok()
        .filter(|s| !s.is_empty());
    let gpu_allow_mock_boot = std::env::var("ORCH_GPU_ALLOW_MOCK").eq(&Ok("1".to_string()));
    if let Some(ref devices) = gpu_devices_boot {
        tracing::info!("ORCH_GPU_DEVICES={devices} — modo GPU habilitado");
    }
    if gpu_allow_mock_boot {
        tracing::info!("ORCH_GPU_ALLOW_MOCK=1 — guarda anti-mock desabilitada");
    }

    let state = AppState {
        s3: Arc::clone(&s3),
        report_client: Arc::clone(&report_client),
        executor,
        active_jobs,
        manager_token,
        gpu_devices: gpu_devices_boot.clone(),
        gpu_allow_mock: gpu_allow_mock_boot,
    };

    // Heartbeat loop (~2s, D4/D9).
    let heartbeat_active_jobs = Arc::clone(&state.active_jobs);

    // GPU telemetry: tenta nvidia-smi no boot; se falhar, warn único e fallback.
    let gpu_telemetry_boot = orchestrator::try_nvidia_smi().await;
    if gpu_telemetry_boot.is_some() {
        tracing::info!("nvidia-smi disponível — telemetria GPU habilitada");
    } else {
        tracing::warn!(
            "nvidia-smi indisponível — telemetria GPU desabilitada (gpus:[], vram:None)"
        );
    }

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        loop {
            interval.tick().await;

            // Tenta nvidia-smi a cada tick; fallback silencioso.
            let (gpus, vram_total, vram_used) = match orchestrator::try_nvidia_smi().await {
                Some(telemetry) => (
                    telemetry.gpus,
                    Some(telemetry.vram_total),
                    Some(telemetry.vram_used),
                ),
                None => (vec![], None, None),
            };

            let body = HeartbeatBody {
                gpus,
                vram_total,
                vram_used,
                cpu: Some(orchestrator::read_cpu()),
                ram: Some(orchestrator::read_ram()),
                ram_total: orchestrator::read_ram_total(),
                jobs_active: heartbeat_active_jobs.len() as i32,
            };

            if let Err(e) = heartbeat_client.send(&body).await {
                tracing::warn!("heartbeat failed: {e}");
            }
        }
    });

    // Server.
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("bind");
    tracing::info!("orchestrator ouvindo em 0.0.0.0:{port}");
    axum::serve(listener, app).await.unwrap();
}
