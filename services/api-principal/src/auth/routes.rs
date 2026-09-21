//! Tabela declarativa de rotas D8 (Fatia 2B): fonte única da montagem do
//! `Router` E do teste de contrato (`tests/contract.rs` compara este conjunto
//! com `packages/contracts/openapi.yaml`, ignorando `x-reserved: true`).
//!
//! Montagem D9: `/health` + `/api/auth/*` públicos (sem gate); sub-router
//! protegido com `require_auth` via `.route_layer()` (datasets desde a 3a);
//! `.fallback()` no router raiz (sem cookie válido → 401 `unauthorized`;
//! com sessão válida → 404 sem body).

use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json,
};
use serde_json::{json, Value};

use super::{gate, handlers, AppState};
use crate::{datasets, generations, jobs, monitoring, search};

/// `(método, path, status_codes)` — espelho exato do contrato (sem `x-reserved`).
/// Toda rota de negócio nova entra AQUI, montada no sub-router `protected`
/// com o gate — esquecer = contrato vermelho.
/// Path no estilo axum 0.7 (`:id`); a OpenAPI declara `{id}` e o teste de
/// contrato normaliza um lado para comparar.
pub const PROTECTED_ROUTES: &[(&str, &str, &[u16])] = &[
    ("GET", "/api/datasets", &[200, 401]),
    ("POST", "/api/datasets", &[201, 400, 401, 409]),
    ("GET", "/api/datasets/:id", &[200, 401, 404]),
    ("DELETE", "/api/datasets/:id", &[204, 401, 404]),
    (
        "POST",
        "/api/datasets/:id/upload",
        &[200, 400, 401, 404, 503],
    ),
    ("GET", "/api/datasets/:id/images", &[200, 400, 401, 404]),
    ("GET", "/api/datasets/:id/images/:imageId", &[200, 401, 404]),
    (
        "GET",
        "/api/datasets/:id/images/:imageId/data",
        &[200, 401, 404, 503],
    ),
    (
        "PUT",
        "/api/datasets/:id/images/:imageId/boxes",
        &[200, 400, 401, 404],
    ),
    (
        "POST",
        "/api/datasets/:id/boxes/batch",
        &[200, 400, 401, 404],
    ),
    (
        "PUT",
        "/api/datasets/:id/images/:imageId/caption",
        &[200, 400, 401, 404],
    ),
    (
        "PUT",
        "/api/datasets/:id/classes",
        &[200, 400, 401, 404, 409],
    ),
    (
        "DELETE",
        "/api/datasets/:id/images/:imageId",
        &[204, 401, 404],
    ),
    (
        "POST",
        "/api/datasets/:id/images/:imageId/restore",
        &[200, 204, 401, 404, 503],
    ),
    ("DELETE", "/api/datasets/:id/trash", &[204, 401, 404]),
    ("POST", "/api/datasets/:id/search/index", &[202, 401, 404]),
    ("GET", "/api/datasets/:id/search/status", &[200, 401, 404]),
    (
        "GET",
        "/api/datasets/:id/search",
        &[200, 400, 401, 404, 409, 503],
    ),
    (
        "POST",
        "/api/datasets/:id/search/by-image",
        &[200, 400, 401, 404, 409],
    ),
    ("POST", "/api/datasets/:id/export", &[200, 401, 404, 503]),
    (
        "POST",
        "/api/datasets/:id/package",
        &[200, 400, 401, 404, 503],
    ),
    ("POST", "/api/datasets/import", &[201, 400, 401, 409, 503]),
    ("GET", "/api/jobs", &[200, 401, 503]),
    ("GET", "/api/jobs/queue", &[200, 401, 503]),
    ("GET", "/api/jobs/:id", &[200, 401, 404, 503]),
    ("GET", "/api/jobs/:id/events", &[200, 401, 404, 503]),
    ("GET", "/api/jobs/:id/metrics", &[200, 401, 404, 503]),
    ("GET", "/api/jobs/:id/artifacts", &[200, 401, 404, 503]),
    ("GET", "/api/jobs/:id/logs", &[200, 401, 404, 503]),
    (
        "GET",
        "/api/jobs/:id/artifacts/:artifactId/data",
        &[200, 401, 404, 503],
    ),
    ("GET", "/api/jobs/:id/artifacts/zip", &[200, 401, 404, 503]),
    ("GET", "/api/telemetry", &[200, 401, 503]),
    ("POST", "/api/jobs/yolo", &[202, 400, 401, 404, 409, 503]),
    (
        "POST",
        "/api/jobs/diffusion",
        &[202, 400, 401, 404, 409, 503],
    ),
    (
        "POST",
        "/api/jobs/diffusion/generate",
        &[202, 400, 401, 404, 503],
    ),
    ("POST", "/api/jobs/predict", &[202, 400, 401, 404, 409, 503]),
    (
        "POST",
        "/api/jobs/autotracker",
        &[202, 400, 401, 404, 409, 503],
    ),
    (
        "POST",
        "/api/jobs/:id/autotracker/apply",
        &[200, 400, 401, 404, 409, 503],
    ),
    (
        "GET",
        "/api/jobs/:id/autotracker/preview",
        &[200, 401, 404, 409, 503],
    ),
    (
        "POST",
        "/api/jobs/autolabel",
        &[202, 400, 401, 404, 409, 503],
    ),
    (
        "POST",
        "/api/jobs/:id/autolabel/apply",
        &[200, 400, 401, 404, 409, 503],
    ),
    (
        "GET",
        "/api/jobs/:id/autolabel/preview",
        &[200, 401, 404, 409, 503],
    ),
    ("POST", "/api/jobs/:id/abort", &[200, 401, 404, 409, 503]),
    ("DELETE", "/api/jobs/:id", &[200, 401, 404, 409, 503]),
    ("POST", "/api/jobs/cleanup", &[200, 400, 401, 503]),
    ("GET", "/api/orchestrators", &[200, 401, 503]),
    (
        "POST",
        "/api/orchestrators/adopt",
        &[200, 400, 401, 409, 503],
    ),
    (
        "POST",
        "/api/orchestrators/:id/revoke",
        &[204, 401, 404, 503],
    ),
    ("GET", "/api/environments", &[200, 401, 503]),
    (
        "POST",
        "/api/environments/adopt",
        &[200, 400, 401, 409, 503],
    ),
    (
        "POST",
        "/api/environments/:id/revoke",
        &[204, 401, 404, 503],
    ),
    ("GET", "/api/models", &[200, 401, 503]),
    ("DELETE", "/api/models/:id", &[204, 401, 404, 503]),
    ("PATCH", "/api/models/:id", &[200, 400, 401, 404, 503]),
    ("POST", "/api/models/upload", &[201, 400, 401, 413, 503]),
    (
        "POST",
        "/api/models/download",
        &[201, 400, 401, 403, 502, 503],
    ),
    ("POST", "/api/models/uploads/init", &[201, 400, 401]),
    (
        "PUT",
        "/api/models/uploads/:uploadId/part/:partNumber",
        &[204, 400, 401, 404, 413],
    ),
    (
        "POST",
        "/api/models/uploads/:uploadId/complete",
        &[201, 400, 401, 404, 409, 413, 503],
    ),
    ("DELETE", "/api/models/uploads/:uploadId", &[204, 401]),
    ("GET", "/api/storage/usage", &[200, 401, 503]),
    // Galeria de gerações (ADR-0023 D5 — G.1 stubs).
    ("GET", "/api/generations", &[200, 400, 401, 503]),
    ("POST", "/api/generations/inputs", &[201, 400, 401]),
    ("GET", "/api/generations/:id/data", &[200, 401, 404, 503]),
    ("GET", "/api/generations/:id/thumb", &[200, 401, 404, 503]),
    ("POST", "/api/generations/delete", &[204, 400, 401, 503]),
    ("POST", "/api/generations/export", &[200, 400, 401, 503]),
];

/// Rotas públicas (sem gate): `/health` + `/ready` + `/api/auth/*`.
pub const PUBLIC_ROUTES: &[(&str, &str, &[u16])] = &[
    ("GET", "/health", &[200]),
    ("GET", "/ready", &[200, 503]),
    ("POST", "/api/auth/login", &[200, 400, 401, 503]),
    ("GET", "/api/auth/me", &[200, 401]),
    ("POST", "/api/auth/logout", &[204]),
];

/// Limite do CORPO TOTAL do lote: 200 MiB de arquivos + 8 MiB de folga p/
/// envelope multipart (D2). NÃO é por field (axum embrulha a stream toda —
/// descoberta da revisão 3b.3).
pub const UPLOAD_BODY_LIMIT_BYTES: usize = 200 * 1024 * 1024 + 8 * 1024 * 1024;

/// Limite do CORPO TOTAL do zip de import: 8 GiB de pacote + 8 MiB de
/// folga p/ envelope multipart (P4, mesmo padrão do upload 3b — o limite
/// mora na camada de roteamento, nunca no handler; teto alinhado com
/// `MAX_GLOBAL_BYTES`/`MAX_DECLARED_TOTAL` do reader do import).
pub const IMPORT_BODY_LIMIT_BYTES: usize = 8 * 1024 * 1024 * 1024 + 8 * 1024 * 1024;

/// Limite do CORPO TOTAL do input img2img: 20 MiB de imagem + 8 MiB de folga
/// p/ envelope multipart (mesmo padrão do upload de datasets — o limite mora
/// na camada de roteamento; o teto por-arquivo de 20 MiB mora no handler e
/// responde 400 `invalid_request`, nunca 413 — o contrato desta rota não
/// declara 413).
pub const GENERATION_INPUT_BODY_LIMIT_BYTES: usize =
    crate::generations::inputs::MAX_INPUT_BYTES as usize + 8 * 1024 * 1024;

/// Contrato total (inventário D8): união REAL de `PUBLIC_ROUTES` +
/// `PROTECTED_ROUTES`. É função justamente para não existir alias esquecido —
/// rotas novas entram só nas duas listas-fonte e aparecem aqui sozinhas.
pub fn routes_all() -> Vec<(&'static str, &'static str, &'static [u16])> {
    PUBLIC_ROUTES
        .iter()
        .chain(PROTECTED_ROUTES.iter())
        .copied()
        .collect()
}

async fn health(State(state): State<AppState>) -> Json<Value> {
    let auth = if state.setup_required {
        "setup_required"
    } else {
        "ready"
    };
    Json(
        json!({ "status": "ok", "service": "api-principal", "auth": auth, "version": env!("CARGO_PKG_VERSION") }),
    )
}

/// GET /ready — readiness check (D11 :459-460). 200 se db saudável; 503 senão.
async fn ready(State(state): State<AppState>) -> Response {
    // Check db: SELECT 1.
    let db_ok: bool = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .is_ok();

    if db_ok {
        (StatusCode::OK, Json(json!({ "status": "ok" }))).into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "unavailable", "reason": "database" })),
        )
            .into_response()
    }
}

/// GET /metrics — métricas no formato padrão Prometheus (OpenMetrics)
async fn metrics_handler(State(state): State<AppState>) -> Response {
    let pool_size = state.pool.size();
    let pool_idle = state.pool.num_idle();
    let setup_val = if state.setup_required { 1 } else { 0 };

    let body = format!(
        "# HELP hephaestus_api_principal_up Service liveness\n\
         # TYPE hephaestus_api_principal_up gauge\n\
         hephaestus_api_principal_up 1\n\
         # HELP hephaestus_db_pool_connections_total Total connections in DB pool\n\
         # TYPE hephaestus_db_pool_connections_total gauge\n\
         hephaestus_db_pool_connections_total {pool_size}\n\
         # HELP hephaestus_db_pool_connections_idle Idle connections in DB pool\n\
         # TYPE hephaestus_db_pool_connections_idle gauge\n\
         hephaestus_db_pool_connections_idle {pool_idle}\n\
         # HELP hephaestus_auth_setup_required Setup required flag\n\
         # TYPE hephaestus_auth_setup_required gauge\n\
         hephaestus_auth_setup_required {setup_val}\n"
    );

    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response()
}

/// Middleware de propagação de x-request-id
async fn request_id_middleware(
    mut req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Response {
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    if let Ok(val) = request_id.parse() {
        req.headers_mut().insert("x-request-id", val);
    }

    let mut response = next.run(req).await;
    if let Ok(val) = request_id.parse() {
        response.headers_mut().insert("x-request-id", val);
    }
    response
}

/// Fallback D9: caminho não roteado — sem sessão válida → 401 `unauthorized`;
/// com sessão válida → 404 sem body (fora do envelope, fora da OpenAPI).
async fn gate_fallback(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let token = gate::parse_session_cookie(
        headers
            .get(axum::http::header::COOKIE)
            .and_then(|v| v.to_str().ok()),
    );
    match token
        .as_deref()
        .and_then(|t| super::session::verify_jwt(t, &state.jwt_secret).ok())
    {
        Some(_) => StatusCode::NOT_FOUND.into_response(),
        None => gate::unauthorized(),
    }
}

/// Monta o router: rotas públicas + sub-router protegido + fallback D9.
pub fn build(state: AppState) -> axum::Router {
    let protected = axum::Router::new()
        .route(
            "/api/datasets",
            get(datasets::handlers::list).post(datasets::handlers::create),
        )
        .route(
            "/api/datasets/:id",
            get(datasets::handlers::get_one).delete(datasets::handlers::delete),
        )
        .route(
            "/api/datasets/:id/upload",
            post(datasets::handlers::upload).layer(DefaultBodyLimit::max(UPLOAD_BODY_LIMIT_BYTES)),
        )
        .route(
            "/api/datasets/:id/images",
            get(datasets::handlers::list_images),
        )
        .route(
            "/api/datasets/:id/images/:imageId",
            get(datasets::handlers::get_image),
        )
        .route(
            "/api/datasets/:id/images/:imageId/data",
            get(datasets::handlers::get_data),
        )
        .route(
            "/api/datasets/:id/images/:imageId/boxes",
            put(datasets::handlers::put_boxes),
        )
        .route(
            "/api/datasets/:id/boxes/batch",
            post(datasets::handlers::batch_update_boxes),
        )
        .route(
            "/api/datasets/:id/images/:imageId/caption",
            put(datasets::handlers::put_caption),
        )
        .route(
            "/api/datasets/:id/classes",
            put(datasets::handlers::put_classes),
        )
        .route(
            "/api/datasets/:id/images/:imageId",
            delete(datasets::handlers::delete_image),
        )
        .route(
            "/api/datasets/:id/images/:imageId/restore",
            post(datasets::handlers::restore_image),
        )
        .route(
            "/api/datasets/:id/trash",
            delete(datasets::handlers::delete_trash),
        )
        .route(
            "/api/datasets/:id/search/index",
            post(search::handlers::post_index),
        )
        .route(
            "/api/datasets/:id/search/status",
            get(search::handlers::get_status),
        )
        .route(
            "/api/datasets/:id/search",
            get(search::handlers::get_search),
        )
        .route(
            "/api/datasets/:id/search/by-image",
            post(search::handlers::post_search_by_image),
        )
        .route(
            "/api/datasets/:id/export",
            post(datasets::export::export_dataset),
        )
        .route(
            "/api/datasets/:id/package",
            post(datasets::package::package_dataset),
        )
        .route(
            "/api/datasets/import",
            post(datasets::import::import_dataset)
                .layer(DefaultBodyLimit::max(IMPORT_BODY_LIMIT_BYTES)),
        )
        // Jobs (ADR-0007 D3: BFF do manager, 7 rotas de leitura).
        .route("/api/jobs", get(jobs::handlers::list_jobs))
        .route("/api/jobs/queue", get(jobs::handlers::list_queue))
        .route(
            "/api/jobs/:id",
            get(jobs::handlers::get_job).delete(jobs::handlers::delete_job),
        )
        .route(
            "/api/jobs/:id/events",
            get(jobs::handlers::stream_job_events),
        )
        .route(
            "/api/jobs/:id/metrics",
            get(jobs::handlers::get_job_metrics),
        )
        .route(
            "/api/jobs/:id/artifacts",
            get(jobs::handlers::list_artifacts),
        )
        .route(
            "/api/jobs/:id/artifacts/:artifactId/data",
            get(jobs::handlers::get_artifact_data),
        )
        .route("/api/jobs/:id/logs", get(jobs::handlers::get_job_logs))
        .route(
            "/api/jobs/:id/artifacts/zip",
            get(jobs::handlers::download_artifacts_zip),
        )
        .route("/api/telemetry", get(jobs::handlers::get_telemetry))
        .route("/api/jobs/yolo", post(jobs::handlers::submit_yolo_job))
        .route(
            "/api/jobs/diffusion",
            post(jobs::handlers::submit_diffusion_job),
        )
        .route(
            "/api/jobs/diffusion/generate",
            post(jobs::handlers::submit_diffusion_generate_job),
        )
        .route(
            "/api/jobs/predict",
            post(jobs::handlers::submit_predict_job),
        )
        .route(
            "/api/jobs/autotracker",
            post(jobs::handlers::submit_autotracker_job),
        )
        .route(
            "/api/jobs/:id/autotracker/apply",
            post(jobs::handlers::apply_autotracker_boxes),
        )
        .route(
            "/api/jobs/:id/autotracker/preview",
            get(jobs::handlers::preview_autotracker_boxes),
        )
        .route(
            "/api/jobs/autolabel",
            post(jobs::handlers::submit_autolabel_job),
        )
        .route(
            "/api/jobs/:id/autolabel/apply",
            post(jobs::handlers::apply_autolabel_captions),
        )
        .route(
            "/api/jobs/:id/autolabel/preview",
            get(jobs::handlers::preview_autolabel_captions),
        )
        .route("/api/jobs/:id/abort", post(jobs::handlers::abort_job))
        .route("/api/jobs/cleanup", post(jobs::handlers::cleanup_jobs))
        // Monitoramento (F6.1b — ADR-0009 D1/D2/D3).
        .route("/api/orchestrators", get(monitoring::get_orchestrators))
        .route(
            "/api/orchestrators/adopt",
            post(monitoring::adopt_orchestrator),
        )
        .route(
            "/api/orchestrators/:id/revoke",
            post(monitoring::revoke_orchestrator),
        )
        // Alias /api/environments (H.4 — ADR-0011 D5, mesmos handlers).
        .route("/api/environments", get(monitoring::get_orchestrators))
        .route(
            "/api/environments/adopt",
            post(monitoring::adopt_orchestrator),
        )
        .route(
            "/api/environments/:id/revoke",
            post(monitoring::revoke_orchestrator),
        )
        .route("/api/models", get(monitoring::get_models))
        .route(
            "/api/models/:id",
            delete(crate::models::handlers::delete_model)
                .patch(crate::models::handlers::update_model),
        )
        .route(
            "/api/models/upload",
            post(crate::models::handlers::upload_model).layer(DefaultBodyLimit::max(
                crate::models::validate::MODEL_UPLOAD_BODY_LIMIT_BYTES,
            )),
        )
        .route(
            "/api/models/download",
            post(crate::models::handlers::download_model),
        )
        .route(
            "/api/models/uploads/init",
            post(crate::models::chunk::init_upload),
        )
        .route(
            "/api/models/uploads/:uploadId/part/:partNumber",
            put(crate::models::chunk::put_part).layer(DefaultBodyLimit::max(
                crate::models::chunk::CHUNK_PART_BODY_LIMIT_BYTES,
            )),
        )
        .route(
            "/api/models/uploads/:uploadId/complete",
            post(crate::models::chunk::complete_upload),
        )
        .route(
            "/api/models/uploads/:uploadId",
            delete(crate::models::chunk::abort_upload),
        )
        .route("/api/storage/usage", get(monitoring::get_storage_usage))
        // Galeria de gerações (ADR-0023 D5 — G.1 stubs).
        .route(
            "/api/generations",
            get(generations::handlers::list_generations),
        )
        .route(
            "/api/generations/inputs",
            post(generations::inputs::upload_generation_input)
                .layer(DefaultBodyLimit::max(GENERATION_INPUT_BODY_LIMIT_BYTES)),
        )
        .route(
            "/api/generations/:id/data",
            get(generations::handlers::get_generation_data),
        )
        .route(
            "/api/generations/:id/thumb",
            get(generations::handlers::get_generation_thumb),
        )
        .route(
            "/api/generations/delete",
            post(generations::handlers::delete_generations),
        )
        .route(
            "/api/generations/export",
            post(generations::handlers::export_generations),
        )
        // route_layer DEPOIS dos .route(): aplicado a um router vazio o axum 0.7 panic
        // no boot (path_router.rs, `routes.is_empty()`). Só cobre as rotas deste
        // sub-router — /health e /api/auth/* seguem fora do gate, e o .fallback()
        // da raiz permanece cobrindo caminho NÃO roteado (D9).
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            gate::require_auth,
        ));
    axum::Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/metrics", get(metrics_handler))
        .route("/api/auth/login", post(handlers::login))
        .route("/api/auth/me", get(handlers::me))
        .route("/api/auth/logout", post(handlers::logout))
        .merge(protected)
        .fallback(gate_fallback)
        .with_state(state)
        .layer(middleware::from_fn(request_id_middleware))
}
