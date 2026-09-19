//! Router HTTP do orchestrator (Fatia 3 — modularização).
//!
//! Extraído de `main.rs` sem mudança de comportamento: rotas públicas
//! (`/health`, `/ready`, `/metrics`), rotas internas autenticadas
//! (`/internal/*`) e camadas de `request-id` + tracing.

use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use tower_http::trace::TraceLayer;

use super::handlers::{
    abort_handler, dispatch_handler, health, metrics_handler, pairing_verify_handler, ready,
};
use super::middleware::{auth_middleware, request_id_middleware};
use super::state::AppState;

/// Constrói o `axum::Router` do orchestrator a partir do estado compartilhado.
pub fn build_router(state: AppState) -> Router {
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
