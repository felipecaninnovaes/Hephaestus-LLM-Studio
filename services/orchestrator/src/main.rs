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
    self, config::OrchestratorConfig, DispatchRequest, HeartbeatBody, HttpHeartbeatClient,
    HttpReportClient, PairingState, ReportBody, S3Client,
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
    max_concurrent_jobs: usize,
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

async fn metrics_handler(State(state): State<AppState>) -> Response {
    let active_count = state.active_jobs.len();
    let max_concurrency = state.max_concurrent_jobs;
    let s3_ok = if state.s3.ping().await { 1 } else { 0 };

    let body = format!(
        "# HELP hephaestus_orchestrator_up Service liveness\n\
         # TYPE hephaestus_orchestrator_up gauge\n\
         hephaestus_orchestrator_up 1\n\
         # HELP hephaestus_orchestrator_active_jobs Active jobs running on this node\n\
         # TYPE hephaestus_orchestrator_active_jobs gauge\n\
         hephaestus_orchestrator_active_jobs {active_count}\n\
         # HELP hephaestus_orchestrator_max_concurrent_jobs Maximum concurrent jobs allowed on this node\n\
         # TYPE hephaestus_orchestrator_max_concurrent_jobs gauge\n\
         hephaestus_orchestrator_max_concurrent_jobs {max_concurrency}\n\
         # HELP hephaestus_orchestrator_s3_connected S3 accessibility probe\n\
         # TYPE hephaestus_orchestrator_s3_connected gauge\n\
         hephaestus_orchestrator_s3_connected {s3_ok}\n"
    );

    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response()
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

    // Semáforo local de GPU/VRAM: rejeita com HTTP 503 se atingiu capacidade máxima
    if state.active_jobs.len() >= state.max_concurrent_jobs {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "node_busy",
            "GPU node is currently at maximum capacity",
        );
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

/// Re-export de [`orchestrator::config::resolve_daemon_diffusion_image`] para
/// compatibilidade com os testes existentes deste módulo (Fatia 1).
#[cfg(test)]
pub(crate) use orchestrator::config::resolve_daemon_diffusion_image;

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
        .route("/metrics", get(metrics_handler))
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

    // Config tipada — fail-fast sem ecoar valor (config::OrchestratorConfig).
    let cfg = OrchestratorConfig::from_env().unwrap_or_else(|e| panic!("{e}"));

    // D5.1-2 — pairing code: env define, ou gera no boot e loga uma vez.
    let pairing = Arc::new(match cfg.pairing_code.clone() {
        Some(code) => {
            tracing::info!("ORCH_PAIRING_CODE definido via env");
            orchestrator::PairingState::new(code)
        }
        None => {
            let code = orchestrator::generate_pairing_code();
            tracing::info!(
                pairing_code = %code,
                "pairing code gerado — copie para o manager (usado uma única vez)"
            );
            orchestrator::PairingState::new(code)
        }
    });

    tracing::info!(
        "orchestrator boot: exec_mode={}, workdir={}",
        cfg.exec_mode,
        cfg.workdir
    );

    // S3 client (D2 — cliente escopado).
    let s3 = Arc::new(S3Client::new(
        &cfg.s3_endpoint,
        &cfg.s3_access_key,
        &cfg.s3_secret_key,
        &cfg.s3_bucket,
    )) as Arc<dyn orchestrator::S3Port>;

    // Report client (D4 — POST /internal/jobs/:id/report).
    let report_client = Arc::new(HttpReportClient::new(
        &cfg.manager_url,
        cfg.manager_token.as_deref(),
    )) as Arc<dyn orchestrator::ReportClient>;

    // Heartbeat client (D4/D9 — POST /internal/heartbeat).
    let heartbeat_client = Arc::new(HttpHeartbeatClient::new(
        &cfg.manager_url,
        cfg.manager_token.as_deref(),
    )) as Arc<dyn orchestrator::HeartbeatClient>;

    // Executor (D5 — docker CLI via socket do host).
    let executor: Arc<dyn orchestrator::TrainerExecutor> = match cfg.exec_mode.as_str() {
        "subprocess" => Arc::new(orchestrator::SubprocessExecutor),
        _ => Arc::new(orchestrator::DockerExecutor),
    };

    // State.
    let active_jobs = orchestrator::new_active_jobs();

    // GPU config: lida uma vez no boot via cfg (D4/D7).
    if let Some(devices) = &cfg.gpu_devices {
        tracing::info!("ORCH_GPU_DEVICES={devices} — modo GPU habilitado");
    }
    if cfg.gpu_allow_mock {
        tracing::info!("ORCH_GPU_ALLOW_MOCK=1 — guarda anti-mock desabilitada");
    }

    // Daemon (D1 — ADR-0023).
    // Trata string vazia como ausente: compose emite `DIFFUSION_DAEMON_URL=`
    // (presença + valor vazio) e o unwrap_or vê Ok("") → daemon externo errado.
    // (Filtro aplicado em `config::DaemonConfig`.)
    let daemon_state = if cfg.daemon.enabled {
        // Mesmo env do manager/compose (`DIFFUSION_TRAINER_IMAGE`): o nome
        // antigo `TRAINER_IMAGE_DIFFUSION` caía sempre no default :local e a
        // guarda D2 recusava job real no nó GPU.
        let image = cfg.daemon.image.clone();
        let client: Arc<dyn orchestrator::daemon::DaemonClient> =
            if let Some(url) = &cfg.daemon.url_override {
                Arc::new(orchestrator::daemon::HttpDaemonClient::new(url))
            } else {
                Arc::new(orchestrator::daemon::HttpDaemonClient::new(&format!(
                    "http://localhost:{}",
                    cfg.daemon.port
                )))
            };

        // Mesmos volumes/mounts do one-shot (build_docker_run_args).
        let daemon_volumes: Vec<(String, String)> = vec![
            (
                cfg.daemon.vol_datasets.clone(),
                "/data/datasets".to_string(),
            ),
            (cfg.daemon.vol_outputs.clone(), "/data/outputs".to_string()),
        ];

        // Mesmas envs do one-shot: ENGINE_MOCK=0 quando GPU, HF cache paths, etc.
        let mut daemon_env: Vec<(String, String)> = Vec::new();
        if cfg.gpu_devices.is_some() {
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
        if let Some(token) = cfg.daemon.hf_token.clone() {
            daemon_env.push(("HF_TOKEN".to_string(), token.clone()));
            daemon_env.push(("HUGGING_FACE_HUB_TOKEN".to_string(), token));
        }
        if let Some(model_id) = cfg.daemon.flux_model_id.clone() {
            daemon_env.push(("FLUX_MODEL_ID".to_string(), model_id));
        }

        let launcher: Arc<dyn orchestrator::daemon::DaemonLauncher> =
            Arc::new(orchestrator::daemon::DockerDaemonLauncher::new(
                &image,
                "diffusion-daemon",
                daemon_volumes,
                cfg.daemon.port,
                cfg.gpu_devices.clone(),
                daemon_env,
                cfg.daemon.network.clone(),
            ));
        let ds = Arc::new(orchestrator::daemon::DaemonState::new(
            &image,
            cfg.daemon.port,
            cfg.daemon.idle_ttl,
            client,
            launcher,
        ));
        tracing::info!(
            "diffusion daemon habilitado: port={}, idle_ttl={}s",
            cfg.daemon.port,
            cfg.daemon.idle_ttl
        );
        if let Some(url) = &cfg.daemon.url_override {
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
        manager_token: cfg.manager_token.clone(),
        gpu_devices: cfg.gpu_devices.clone(),
        gpu_allow_mock: cfg.gpu_allow_mock,
        pairing,
        daemon_state,
        max_concurrent_jobs: cfg.max_concurrent_jobs,
    };

    // Heartbeat loop (~2s, D4/D9).
    let heartbeat_active_jobs = Arc::clone(&state.active_jobs);
    let heartbeat_advertise_url = cfg.advertise_url.clone();

    // GPU telemetry: tenta nvidia-smi no boot; se falhar, warn único e fallback.
    let gpu_telemetry_boot = orchestrator::try_nvidia_smi().await;
    if gpu_telemetry_boot.is_some() {
        tracing::info!("nvidia-smi disponível — telemetria GPU habilitada");
    } else {
        tracing::warn!(
            "nvidia-smi indisponível — telemetria GPU desabilitada (gpus:[], vram:None)"
        );
    }

    // Sweep de containers órfãos no boot (anti-processos fantasmas pós crash).
    orchestrator::sweep_orphan_trainer_containers().await;
    orchestrator::sweep_orphan_workdirs(
        &std::path::PathBuf::from(&cfg.workdir),
        std::time::Duration::from_secs(86400),
    )
    .await;

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
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", cfg.port))
        .await
        .expect("bind");
    tracing::info!("orchestrator ouvindo em 0.0.0.0:{}", cfg.port);
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
