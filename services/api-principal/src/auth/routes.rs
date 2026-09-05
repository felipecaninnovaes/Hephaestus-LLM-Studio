//! Tabela declarativa de rotas D8 (Fatia 2B): fonte única da montagem do
//! `Router` E do teste de contrato (`tests/contract.rs` compara este conjunto
//! com `packages/contracts/openapi.yaml`, ignorando `x-reserved: true`).
//!
//! Montagem D9: `/health` + `/api/auth/*` públicos (sem gate); sub-router
//! protegido com `require_auth` via `.route_layer()`; `.fallback()` no router
//! raiz (sem cookie válido → 401 `unauthorized`; com sessão válida → 404 sem body).

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json,
};
use serde_json::{json, Value};

use super::{gate, handlers, AppState};

/// `(método, path, status_codes)` — espelho exato do contrato (sem `x-reserved`).
/// Toda rota de negócio nova entra AQUI, montada no sub-router `protected`
/// com o gate — esquecer = contrato vermelho.
pub const PROTECTED_ROUTES: &[(&str, &str, &[u16])] = &[];

/// Rotas públicas (sem gate): `/health` + `/api/auth/*`.
pub const PUBLIC_ROUTES: &[(&str, &str, &[u16])] = &[
    ("GET", "/health", &[200]),
    ("POST", "/api/auth/login", &[200, 400, 401, 503]),
    ("GET", "/api/auth/me", &[200, 401]),
    ("POST", "/api/auth/logout", &[204]),
];

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
    // Será povoado com as rotas de negócio nas fatias 3+ (datasets/jobs/settings).
    let protected = axum::Router::new();
    // DIVERGÊNCIA vs ADR-0001 D9 (letra): o `route_layer(require_auth)` no
    // sub-router vazio causa panic no boot no axum 0.7 ("route_layer before any
    // routes is a no-op") — por isso o gate (`gate::require_auth` via
    // `middleware::from_fn_with_state`) só é plugado junto da 1ª rota de
    // negócio (fatia 3+). O fail-closed p/ caminho desconhecido vale desde já
    // via `gate_fallback` (401 sem cookie).
    axum::Router::new()
        .route("/health", get(health))
        .route("/api/auth/login", post(handlers::login))
        .route("/api/auth/me", get(handlers::me))
        .route("/api/auth/logout", post(handlers::logout))
        .merge(protected)
        .fallback(gate_fallback)
        .with_state(state)
}
