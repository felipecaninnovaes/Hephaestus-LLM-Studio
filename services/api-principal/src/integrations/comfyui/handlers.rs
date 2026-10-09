//! Handlers `/api/integrations/comfyui/*` (finos: validam, delegam ao
//! repositório/runner e montam o wire).

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{de::DeserializeOwned, Deserialize};
use uuid::Uuid;

use super::client::Remote;
use super::crypto;
use super::repository::{self as repo, ExportRow, NewExport, TargetRow};
use super::runner::{run_export, ExportJob};
use super::source::{self, ResolveError};
use super::types::*;
use crate::error::{
    err, MSG_EXPORT_UNSUPPORTED, MSG_INVALID_REQUEST, MSG_NOT_FOUND, MSG_QUEUE_UNAVAILABLE,
    MSG_TARGET_NAME_CONFLICT,
};
use crate::state::AppState;

const MSG_INTERNAL: &str = "internal server error";
const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 100;

fn internal() -> Response {
    err(StatusCode::INTERNAL_SERVER_ERROR, "internal", MSG_INTERNAL)
}
fn bad_request() -> Response {
    err(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        MSG_INVALID_REQUEST,
    )
}
fn not_found() -> Response {
    err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND)
}
fn name_conflict() -> Response {
    err(
        StatusCode::CONFLICT,
        "comfy_target_name_taken",
        MSG_TARGET_NAME_CONFLICT,
    )
}

fn parse_body<T: DeserializeOwned>(body: &Bytes) -> Result<T, Box<Response>> {
    serde_json::from_slice(body).map_err(|_| Box::new(bad_request()))
}

fn parse_id(raw: &str) -> Result<Uuid, Box<Response>> {
    raw.parse().map_err(|_| Box::new(not_found()))
}

pub fn target_wire(t: TargetRow) -> ComfyTargetResponse {
    ComfyTargetResponse {
        id: t.id,
        name: t.name,
        base_url: t.base_url,
        has_token: !t.token_ciphertext.is_empty(),
        last_check_at: t.last_check_at,
        last_check_ok: t.last_check_ok,
        last_check_error: t.last_check_error,
        created_at: t.created_at,
        updated_at: t.updated_at,
    }
}

fn export_wire(e: ExportRow) -> ComfyExportResponse {
    ComfyExportResponse {
        id: e.id,
        target_id: e.target_id,
        target_name: e.target_name,
        filename: e.filename,
        status: e.status,
        bytes_total: e.bytes_total,
        bytes_sent: e.bytes_sent,
        error: e.error,
        error_code: e.error_code,
        remote_path: e.remote_path,
        created_at: e.created_at,
        updated_at: e.updated_at,
    }
}

/// Mapeia erro de escrita do alvo: nome duplicado ⇒ 409, resto ⇒ 500.
fn write_error(e: sqlx::Error) -> Response {
    if repo::is_unique_violation(&e) {
        name_conflict()
    } else {
        internal()
    }
}

pub async fn list_targets(State(state): State<AppState>) -> Response {
    match repo::list_targets(&state.pool).await {
        Ok(rows) => Json(rows.into_iter().map(target_wire).collect::<Vec<_>>()).into_response(),
        Err(_) => internal(),
    }
}

pub async fn create_target(State(state): State<AppState>, body: Bytes) -> Response {
    let req: CreateTargetRequest = match parse_body(&body) {
        Ok(r) => r,
        Err(r) => return *r,
    };
    let (Some(name), Some(url), Some(token)) = (
        valid_name(&req.name),
        normalize_base_url(&req.base_url),
        valid_token(&req.token),
    ) else {
        return bad_request();
    };
    let Ok((ct, nonce)) = crypto::encrypt(&state.jwt_secret, &token) else {
        return internal();
    };
    match repo::insert_target(&state.pool, Uuid::new_v4(), &name, &url, &ct, &nonce).await {
        Ok(row) => (StatusCode::CREATED, Json(target_wire(row))).into_response(),
        Err(e) => write_error(e),
    }
}

pub async fn patch_target(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let id = match parse_id(&id) {
        Ok(i) => i,
        Err(r) => return *r,
    };
    let req: PatchTargetRequest = match parse_body(&body) {
        Ok(r) => r,
        Err(r) => return *r,
    };
    let name = req.name.as_deref().map(valid_name);
    let url = req.base_url.as_deref().map(normalize_base_url);
    let token = req.token.as_deref().map(valid_token);
    if matches!(name, Some(None)) || matches!(url, Some(None)) || matches!(token, Some(None)) {
        return bad_request();
    }
    let enc = match token
        .flatten()
        .map(|t| crypto::encrypt(&state.jwt_secret, &t))
    {
        Some(Ok(pair)) => Some(pair),
        Some(Err(_)) => return internal(),
        None => None,
    };
    let secret = enc.as_ref().map(|(c, n)| (c.as_slice(), n.as_slice()));
    let (name, url) = (name.flatten(), url.flatten());
    match repo::update_target(&state.pool, id, name.as_deref(), url.as_deref(), secret).await {
        Ok(Some(row)) => Json(target_wire(row)).into_response(),
        Ok(None) => not_found(),
        Err(e) => write_error(e),
    }
}

pub async fn delete_target(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let id = match parse_id(&id) {
        Ok(i) => i,
        Err(r) => return *r,
    };
    match repo::delete_target(&state.pool, id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => not_found(),
        Err(_) => internal(),
    }
}

const UNREADABLE_TOKEN_CODE: &str = "token_undecryptable";
const UNREADABLE_TOKEN_MSG: &str = "token ilegível; edite o destino e informe o token de novo";

/// Decifra o token do destino e monta o cliente remoto. `None` quando o
/// token não decifra (ex.: `jwt_secret` rotacionado).
fn remote_for(state: &AppState, t: &TargetRow) -> Option<Remote> {
    crypto::decrypt(&state.jwt_secret, &t.token_ciphertext, &t.token_nonce)
        .ok()
        .map(|token| Remote::new(&t.base_url, &token))
}

fn resolve_error(e: ResolveError) -> Response {
    match e {
        ResolveError::NotFound => not_found(),
        ResolveError::Unsupported => err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "export_unsupported",
            MSG_EXPORT_UNSUPPORTED,
        ),
        ResolveError::Unavailable => err(
            StatusCode::SERVICE_UNAVAILABLE,
            "queue_unavailable",
            MSG_QUEUE_UNAVAILABLE,
        ),
    }
}

pub async fn test_target(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let id = match parse_id(&id) {
        Ok(i) => i,
        Err(r) => return *r,
    };
    let target = match repo::get_target(&state.pool, id).await {
        Ok(Some(t)) => t,
        Ok(None) => return not_found(),
        Err(_) => return internal(),
    };
    let (ok, error) = match remote_for(&state, &target) {
        None => (false, Some(UNREADABLE_TOKEN_MSG.to_string())),
        Some(remote) => match remote.health().await {
            Ok(_) => (true, None),
            Err(msg) => (false, Some(msg)),
        },
    };
    match repo::set_check(&state.pool, id, ok, error.as_deref()).await {
        Ok(Some(row)) => Json(target_wire(row)).into_response(),
        Ok(None) => not_found(),
        Err(_) => internal(),
    }
}

pub async fn create_export(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let target_id = match parse_id(&id) {
        Ok(i) => i,
        Err(r) => return *r,
    };
    let req: CreateExportRequest = match parse_body(&body) {
        Ok(r) => r,
        Err(r) => return *r,
    };
    let Some(src_ref) = req.source() else {
        return bad_request();
    };
    let target = match repo::get_target(&state.pool, target_id).await {
        Ok(Some(t)) => t,
        Ok(None) => return not_found(),
        Err(_) => return internal(),
    };
    let remote = remote_for(&state, &target);
    let src = match source::resolve(&state.pool, state.manager.as_ref(), &src_ref).await {
        Ok(s) => s,
        Err(e) => return resolve_error(e),
    };
    let export_id = Uuid::new_v4();
    let new = NewExport {
        id: export_id,
        target_id,
        model_id: src.model_id,
        job_id: src.job_id,
        artifact_id: src.artifact_id,
        filename: &src.filename,
        bytes_total: src.bytes,
    };
    if repo::insert_export(&state.pool, &new).await.is_err() {
        return internal();
    }
    match remote {
        Some(remote) => {
            let job = ExportJob {
                export_id,
                remote,
                source: src,
                overwrite: req.overwrite,
            };
            tokio::spawn(run_export(state.pool.clone(), state.storage.clone(), job));
        }
        None => {
            let failed = repo::finish_failed(
                &state.pool,
                export_id,
                UNREADABLE_TOKEN_CODE,
                UNREADABLE_TOKEN_MSG,
            )
            .await;
            if failed.is_err() {
                return internal();
            }
        }
    }
    match repo::get_export(&state.pool, export_id).await {
        Ok(Some(row)) => (StatusCode::ACCEPTED, Json(export_wire(row))).into_response(),
        _ => internal(),
    }
}

pub async fn get_export(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let id = match parse_id(&id) {
        Ok(i) => i,
        Err(r) => return *r,
    };
    match repo::get_export(&state.pool, id).await {
        Ok(Some(row)) => Json(export_wire(row)).into_response(),
        Ok(None) => not_found(),
        Err(_) => internal(),
    }
}

#[derive(Deserialize)]
pub struct ListQuery {
    limit: Option<i64>,
}

pub async fn list_exports(State(state): State<AppState>, Query(q): Query<ListQuery>) -> Response {
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return bad_request();
    }
    match repo::list_exports(&state.pool, limit).await {
        Ok(rows) => Json(rows.into_iter().map(export_wire).collect::<Vec<_>>()).into_response(),
        Err(_) => internal(),
    }
}
