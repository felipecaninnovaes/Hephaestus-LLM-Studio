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
/// 503 de backend de objetos indisponível (ADR-0003 D10).
pub const MSG_STORAGE_UNAVAILABLE: &str = "storage unavailable";
/// 409 de remoção de classe em uso por anotações (fatia 3g.1).
pub const MSG_CLASSES_IN_USE: &str = "class in use by annotations";
/// 409 de busca sem embeddings do modelo ativo (fatia 3f.5, ADR-0004 D5).
pub const MSG_INDEX_NOT_READY: &str = "index not ready";
/// 503 de embedder inalcançável (fatia 3f.5, ADR-0004 D1).
pub const MSG_EMBEDDING_UNAVAILABLE: &str = "embedding unavailable";
/// 400 de pacote de import inválido (fatia 3e.2, ADR-0006 D7).
pub const MSG_IMPORT_INVALID: &str = "invalid import package";
/// 400 de engine não suportado (fatia 4, ADR-0007 D7).
pub const MSG_ENGINE_UNSUPPORTED: &str = "engine not supported";
/// 503 de manager indisponível (fatia 4, ADR-0007 D7 — `queue_unavailable`).
pub const MSG_QUEUE_UNAVAILABLE: &str = "queue unavailable";
