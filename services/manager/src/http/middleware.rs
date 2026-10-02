//! Middlewares de autenticação e instrumentação de requisições HTTP (MM-06).

use axum::{
    extract::State,
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use tracing::Instrument;

use super::state::AppState;
use crate::error::error_response;

/// Propaga `x-request-id`/`traceparent` (fatia 2b): gera/reusa o contexto de
/// correlação, abre o span `request` (campo `job_id` preenchido pelos
/// handlers que operam sobre um job específico) e devolve ambos os headers
/// na resposta.
pub async fn request_id_middleware(
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    let incoming_request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok());
    let incoming_traceparent = req
        .headers()
        .get("traceparent")
        .and_then(|v| v.to_str().ok());
    let ctx =
        heph_contracts::RequestContext::from_incoming(incoming_request_id, incoming_traceparent);

    let method = req.method().clone();
    let uri = req.uri().clone();

    let span = tracing::info_span!(
        "request",
        request_id = %ctx.request_id,
        trace_id = %ctx.trace_id,
        method = %method,
        path = %uri,
        job_id = tracing::field::Empty,
    );
    heph_contracts::otel::set_request_parent(&span, &ctx, incoming_traceparent);

    let request_id = ctx.request_id.clone();
    let traceparent = ctx.traceparent();
    let start = std::time::Instant::now();
    let mut response = ctx
        .scope(async { next.run(req).await })
        .instrument(span.clone())
        .await;
    let duration = start.elapsed();
    span.in_scope(|| {
        tracing::info!(
            status = response.status().as_u16(),
            duration_ms = duration.as_millis() as u64,
            "request completed"
        );
    });

    if let Ok(val) = request_id.parse() {
        response.headers_mut().insert("x-request-id", val);
    }
    if let Ok(val) = traceparent.parse() {
        response.headers_mut().insert("traceparent", val);
    }
    response
}

pub async fn auth_middleware(
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
