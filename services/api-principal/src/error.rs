//! Construtor único do envelope de erro D6 `{code, message}` para todo domínio.
//!
//! `message` é sempre estática por `code` — nunca contém valor de usuário,
//! path, token ou segredo. O `auth` mantém suas próprias mensagens (já no
//! contrato/ADR-0001) mas usa esta função.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: &'static str,
}

pub fn err(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    (status, Json(ErrorBody { code, message })).into_response()
}

/// 404 de dataset por id/slug inexistente ou id inválido (D8: nunca 400).
pub const MSG_NOT_FOUND: &str = "dataset not found";
/// 409 de slug já existente.
pub const MSG_SLUG_CONFLICT: &str = "dataset slug already exists";
/// 400 genérico de corpo/query inválido.
pub const MSG_INVALID_REQUEST: &str = "invalid request";
/// 401 de sessão ausente/inválida (valor no contrato da Fatia 2, imutável).
pub const MSG_UNAUTHORIZED: &str = "unauthorized";
