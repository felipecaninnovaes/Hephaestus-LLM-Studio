//! Middlewares e helpers de erro HTTP do orchestrator (Fatia 3).
//!
//! Extraídos de `main.rs` sem mudança de comportamento: respostas de erro
//! padronizadas, rastreabilidade `x-request-id` e autenticação Bearer
//! opcional via `MANAGER_TOKEN`.

use axum::{
    extract::State,
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};

use super::state::AppState;

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

pub(crate) fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        Json(serde_json::json!({"code": code, "message": message})),
    )
        .into_response()
}

pub(crate) fn bad_request(msg: &str) -> Response {
    error_response(StatusCode::BAD_REQUEST, "invalid_request", msg)
}

pub(crate) fn conflict(msg: &str) -> Response {
    error_response(StatusCode::CONFLICT, "job_conflict", msg)
}

// ---------------------------------------------------------------------------
// Middleware: x-request-id + tracing
// ---------------------------------------------------------------------------

pub(crate) async fn request_id_middleware(
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
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

pub(crate) async fn auth_middleware(
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
