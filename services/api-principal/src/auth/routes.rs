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
    routing::{get, post},
    Json,
};
use serde_json::{json, Value};

use super::{gate, handlers, AppState};
use crate::datasets;

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
    ("POST", "/api/datasets/:id/upload", &[200, 400, 401, 404, 503]),
    ("GET", "/api/datasets/:id/images", &[200, 400, 401, 404]),
    ("GET", "/api/datasets/:id/images/:imageId", &[200, 401, 404]),
    ("GET", "/api/datasets/:id/images/:imageId/data", &[200, 401, 404, 503]),
];

/// Rotas públicas (sem gate): `/health` + `/api/auth/*`.
pub const PUBLIC_ROUTES: &[(&str, &str, &[u16])] = &[
    ("GET", "/health", &[200]),
    ("POST", "/api/auth/login", &[200, 400, 401, 503]),
    ("GET", "/api/auth/me", &[200, 401]),
    ("POST", "/api/auth/logout", &[204]),
];

/// Limite do CORPO TOTAL do lote: 200 MiB de arquivos + 8 MiB de folga p/
/// envelope multipart (D2). NÃO é por field (axum embrulha a stream toda —
/// descoberta da revisão 3b.3).
pub const UPLOAD_BODY_LIMIT_BYTES: usize = 200 * 1024 * 1024 + 8 * 1024 * 1024;

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
    Json(json!({ "status": "ok", "service": "api-principal", "auth": auth }))
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
            post(datasets::handlers::upload)
                .layer(DefaultBodyLimit::max(UPLOAD_BODY_LIMIT_BYTES)),
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
        // route_layer DEPOIS dos .route(): aplicado a um router vazio o axum 0.7 panic
        // no boot (path_router.rs, `routes.is_empty()`). Só cobre as rotas deste
        // sub-router — /health e /api/auth/* seguem fora do gate, e o .fallback()
        // da raiz permanece cobrindo caminho NÃO roteado (D9).
        .route_layer(middleware::from_fn_with_state(state.clone(), gate::require_auth));
    axum::Router::new()
        .route("/health", get(health))
        .route("/api/auth/login", post(handlers::login))
        .route("/api/auth/me", get(handlers::me))
        .route("/api/auth/logout", post(handlers::logout))
        .merge(protected)
        .fallback(gate_fallback)
        .with_state(state)
}
