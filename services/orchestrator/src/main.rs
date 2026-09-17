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
    self, DispatchRequest, HeartbeatBody, HttpHeartbeatClient, HttpReportClient, PairingState,
    ReportBody, S3Client,
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
    pairing: Arc<PairingState>,
    daemon_state: Option<Arc<orchestrator::daemon::DaemonState>>,
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
    if let Some(ref pr) = req.package_ref {
        if pr.key.is_empty() {
            return bad_request("package_ref.key is required");
        }
        if pr.md5_zip.is_empty() {
            return bad_request("package_ref.md5_zip is required");
        }
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
    let daemon_state = state.daemon_state.clone();

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
            daemon_state,
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
                    meta_content: None,
                    phase: None,
                    message: None,
                },
            )
            .await;
    }

    (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response()
}

/// POST /internal/pairing/verify — verifica pairing code (D5.1-2, single-use).
async fn pairing_verify_handler(State(state): State<AppState>, body: Bytes) -> Response {
    if body.is_empty() {
        return bad_request("empty body");
    }

    let req: orchestrator::PairingVerifyRequest = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return bad_request(&format!("invalid json: {e}"));
        }
    };

    if req.code.is_empty() {
        return bad_request("code is required");
    }

    let valid = state.pairing.verify(&req.code);
    (
        StatusCode::OK,
        Json(orchestrator::PairingVerifyResponse { valid }),
    )
        .into_response()
}

/// Resolve a imagem do daemon de difusão a partir do env `DIFFUSION_TRAINER_IMAGE`
/// (mesmo nome usado pelo manager e pelo compose).
///
/// Env ausente ou vazio (só whitespace) → default `"hephaestus/trainer-difusao:local"`.
fn resolve_daemon_diffusion_image(env_value: Option<&str>) -> String {
    match env_value.map(str::trim) {
        Some(v) if !v.is_empty() => v.to_string(),
        _ => "hephaestus/trainer-difusao:local".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------
fn build_router(state: AppState) -> Router {
    let api = Router::new()
        .route("/internal/dispatch", post(dispatch_handler))
        .route("/internal/abort", post(abort_handler))
        .route("/internal/pairing/verify", post(pairing_verify_handler))
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

    // D1 — identidade no heartbeat.
    let advertise_url =
        orchestrator::resolve_advertise_url(std::env::var("ORCH_ADVERTISE_URL").ok().as_deref());

    // D5.1-2 — pairing code: env define, ou gera no boot e loga uma vez.
    let pairing = Arc::new(match std::env::var("ORCH_PAIRING_CODE") {
        Ok(code) if !code.is_empty() => {
            tracing::info!("ORCH_PAIRING_CODE definido via env");
            orchestrator::PairingState::new(code)
        }
        _ => {
            let code = orchestrator::generate_pairing_code();
            tracing::info!(
                pairing_code = %code,
                "pairing code gerado — copie para o manager (usado uma única vez)"
            );
            orchestrator::PairingState::new(code)
        }
    });

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

    // Daemon config (D1 — ADR-0023).
    // Trata string vazia como ausente: compose emite `DIFFUSION_DAEMON_URL=`
    // (presença + valor vazio) e o unwrap_or vê Ok("") → daemon externo errado.
    let daemon_enabled = std::env::var("DIFFUSION_DAEMON_ENABLED")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .as_deref()
        == Some("1");
    let daemon_port: u16 = std::env::var("DIFFUSION_DAEMON_PORT")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "8766".into())
        .parse()
        .unwrap_or(8766);
    let daemon_idle_ttl: u64 = std::env::var("DIFFUSION_DAEMON_IDLE_TTL_S")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "600".into())
        .parse()
        .unwrap_or(600);
    let daemon_url_override = std::env::var("DIFFUSION_DAEMON_URL")
        .ok()
        .filter(|v| !v.trim().is_empty());

    let daemon_state = if daemon_enabled {
        // Mesmo env do manager/compose (`DIFFUSION_TRAINER_IMAGE`): o nome
        // antigo `TRAINER_IMAGE_DIFFUSION` caía sempre no default :local e a
        // guarda D2 recusava job real no nó GPU.
        let image = resolve_daemon_diffusion_image(
            std::env::var("DIFFUSION_TRAINER_IMAGE").ok().as_deref(),
        );
        let client: Arc<dyn orchestrator::daemon::DaemonClient> =
            if let Some(ref url) = daemon_url_override {
                Arc::new(orchestrator::daemon::HttpDaemonClient::new(url))
            } else {
                Arc::new(orchestrator::daemon::HttpDaemonClient::new(&format!(
                    "http://localhost:{daemon_port}"
                )))
            };

        // Mesmos volumes/mounts do one-shot (build_docker_run_args).
        let vol_datasets_daemon =
            std::env::var("ORCH_VOL_DATASETS").unwrap_or_else(|_| "infra_datasets".into());
        let vol_outputs_daemon =
            std::env::var("ORCH_VOL_OUTPUTS").unwrap_or_else(|_| "infra_outputs".into());
        let daemon_volumes: Vec<(String, String)> = vec![
            (vol_datasets_daemon, "/data/datasets".to_string()),
            (vol_outputs_daemon, "/data/outputs".to_string()),
        ];

        // Mesmas envs do one-shot: ENGINE_MOCK=0 quando GPU, HF cache paths, etc.
        let mut daemon_env: Vec<(String, String)> = Vec::new();
        if gpu_devices_boot.is_some() {
            daemon_env.push(("ENGINE_MOCK".to_string(), "0".to_string()));
        }
        // HF cache paths para diffusion (igual one-shot L1473-1493)
        daemon_env.push((
            "HF_HOME".to_string(),
            "/data/outputs/.cache/huggingface".to_string(),
        ));
        daemon_env.push((
            "HF_HUB_CACHE".to_string(),
            "/data/outputs/.cache/huggingface/hub".to_string(),
        ));
        daemon_env.push((
            "TRANSFORMERS_CACHE".to_string(),
            "/data/outputs/.cache/huggingface/hub".to_string(),
        ));
        daemon_env.push((
            "DIFFUSERS_CACHE".to_string(),
            "/data/outputs/.cache/huggingface/hub".to_string(),
        ));
        daemon_env.push((
            "TORCH_HOME".to_string(),
            "/data/outputs/.cache/torch".to_string(),
        ));
        if let Ok(token) =
            std::env::var("HF_TOKEN").or_else(|_| std::env::var("HUGGING_FACE_HUB_TOKEN"))
        {
            if !token.is_empty() {
                daemon_env.push(("HF_TOKEN".to_string(), token.clone()));
                daemon_env.push(("HUGGING_FACE_HUB_TOKEN".to_string(), token));
            }
        }
        if let Ok(model_id) = std::env::var("FLUX_MODEL_ID") {
            if !model_id.is_empty() {
                daemon_env.push(("FLUX_MODEL_ID".to_string(), model_id));
            }
        }

        let daemon_network = std::env::var("DIFFUSION_DAEMON_NETWORK")
            .ok()
            .filter(|v| !v.trim().is_empty());

        let launcher: Arc<dyn orchestrator::daemon::DaemonLauncher> =
            Arc::new(orchestrator::daemon::DockerDaemonLauncher::new(
                &image,
                "diffusion-daemon",
                daemon_volumes,
                daemon_port,
                gpu_devices_boot.clone(),
                daemon_env,
                daemon_network,
            ));
        let ds = Arc::new(orchestrator::daemon::DaemonState::new(
            &image,
            daemon_port,
            daemon_idle_ttl,
            client,
            launcher,
        ));
        tracing::info!(
            "diffusion daemon habilitado: port={daemon_port}, idle_ttl={daemon_idle_ttl}s"
        );
        if let Some(ref url) = daemon_url_override {
            tracing::info!("DIFFUSION_DAEMON_URL={url} — daemon externo, spawn desabilitado");
        }

        // Spawn idle TTL housekeeping task (D1)
        let ds_clone = Arc::clone(&ds);
        tokio::spawn(async move {
            orchestrator::daemon::idle_ttl_housekeeping(ds_clone).await;
        });

        Some(ds)
    } else {
        tracing::info!("diffusion daemon desabilitado (DIFFUSION_DAEMON_ENABLED=0) — one-shot");
        None
    };

    let state = AppState {
        s3: Arc::clone(&s3),
        report_client: Arc::clone(&report_client),
        executor,
        active_jobs,
        manager_token,
        gpu_devices: gpu_devices_boot.clone(),
        gpu_allow_mock: gpu_allow_mock_boot,
        pairing,
        daemon_state,
    };

    // Heartbeat loop (~2s, D4/D9).
    let heartbeat_active_jobs = Arc::clone(&state.active_jobs);
    let heartbeat_advertise_url = advertise_url.clone();

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
            let (gpus, vram_total, vram_used, max_gpu_mib) =
                match orchestrator::try_nvidia_smi().await {
                    Some(telemetry) => (
                        telemetry.gpus,
                        Some(telemetry.vram_total),
                        Some(telemetry.vram_used),
                        Some(telemetry.max_gpu_mib),
                    ),
                    None => (vec![], None, None, None),
                };

            let body = HeartbeatBody {
                endpoint: heartbeat_advertise_url.clone(),
                gpus,
                vram_total,
                vram_used,
                cpu: Some(orchestrator::read_cpu()),
                ram: Some(orchestrator::read_ram()),
                ram_total: orchestrator::read_ram_total(),
                jobs_active: heartbeat_active_jobs.len() as i32,
                max_gpu_mib,
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

#[cfg(test)]
mod tests {
    use super::resolve_daemon_diffusion_image;

    #[test]
    fn daemon_image_env_explicito_vence() {
        assert_eq!(
            resolve_daemon_diffusion_image(Some("meu-registry/trainer-difusao:gpu")),
            "meu-registry/trainer-difusao:gpu"
        );
    }

    #[test]
    fn daemon_image_default_quando_ausente_ou_vazio() {
        // Env ausente, vazio ou só whitespace → default :local (guarda D2 só
        // recusa :local em nó GPU; CPU-only continua mock).
        for v in [None, Some(""), Some("   ")] {
            assert_eq!(
                resolve_daemon_diffusion_image(v),
                "hephaestus/trainer-difusao:local",
                "env={v:?}"
            );
        }
    }

    #[test]
    fn daemon_image_preserva_whitespace_externo() {
        // Trim: compose nunca deve injetar espaços no nome da imagem.
        assert_eq!(
            resolve_daemon_diffusion_image(Some("  hephaestus/trainer-difusao:gpu  ")),
            "hephaestus/trainer-difusao:gpu"
        );
    }
}
