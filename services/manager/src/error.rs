use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagerError {
    NotFound,
    NotAbortable,
    /// Job não está em estado terminal (done|failed|cancelled) — não pode ser apagado.
    NotDeletable,
    /// Transição guardada recusada (ex.: prepare-complete fora de `preparing`) → 409.
    Conflict(String),
    InvalidRequest(String),
    PairingInvalid,
    Internal(String),
}

impl std::fmt::Display for ManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "not found"),
            Self::NotAbortable => write!(f, "job not abortable"),
            Self::NotDeletable => write!(f, "job_not_terminal"),
            Self::Conflict(e) => write!(f, "{e}"),
            Self::InvalidRequest(e) => write!(f, "invalid request: {e}"),
            Self::PairingInvalid => write!(f, "pairing_invalid"),
            Self::Internal(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ManagerError {}

impl From<sqlx::Error> for ManagerError {
    fn from(err: sqlx::Error) -> Self {
        Self::Internal(err.to_string())
    }
}

pub fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        [("content-type", "application/json")],
        Json(serde_json::json!({
            "code": code,
            "message": message,
        })),
    )
        .into_response()
}

pub fn not_found() -> Response {
    error_response(StatusCode::NOT_FOUND, "not_found", "resource not found")
}

pub fn bad_request(msg: &str) -> Response {
    error_response(StatusCode::BAD_REQUEST, "invalid_request", msg)
}

pub fn conflict(code: &str, msg: &str) -> Response {
    error_response(StatusCode::CONFLICT, code, msg)
}

pub fn internal_error(msg: &str) -> Response {
    error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal", msg)
}

pub fn not_abortable() -> Response {
    error_response(
        StatusCode::CONFLICT,
        "job_not_abortable",
        "job is in a terminal state",
    )
}

pub fn not_deletable() -> Response {
    error_response(
        StatusCode::CONFLICT,
        "job_not_terminal",
        "only terminal jobs (done|failed|cancelled) can be deleted",
    )
}

pub fn pairing_invalid() -> Response {
    error_response(
        StatusCode::CONFLICT,
        "pairing_invalid",
        "código de pareamento inválido ou orquestrador inalcançável",
    )
}

impl IntoResponse for ManagerError {
    fn into_response(self) -> Response {
        match self {
            Self::NotFound => not_found(),
            Self::NotAbortable => not_abortable(),
            Self::NotDeletable => not_deletable(),
            Self::Conflict(ref code) => {
                let msg = match code.as_str() {
                    "job_not_preparing" => "job is not in preparing state",
                    "job_not_cancelling" => "job is not in preparing or cancelling state",
                    _ => "conflict",
                };
                conflict(code, msg)
            }
            Self::InvalidRequest(ref msg) => bad_request(msg),
            Self::PairingInvalid => pairing_invalid(),
            Self::Internal(ref msg) if msg == "model_exists" => error_response(
                StatusCode::CONFLICT,
                "model_exists",
                "model with this s3_key already exists",
            ),
            Self::Internal(ref msg) => internal_error(msg),
        }
    }
}
