//! Servidor HTTP fake do custom node (protocolo §1 do contrato).
#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const TOKEN: &str = "segredo-do-destino";

#[derive(Default)]
pub struct Fake {
    pub chunk_size: u64,
    pub declared_size: u64,
    pub part: Vec<u8>,
    pub puts: Vec<(u64, usize)>,
    pub deleted: bool,
    pub committed: Option<Vec<u8>>,
    pub init_calls: u32,
    pub file_exists: bool,
    /// Primeiro PUT grava só metade e responde 500 (conexão caiu no meio).
    pub partial_then_500_once: bool,
    /// Todo PUT responde esse status/código.
    pub put_always: Option<(u16, &'static str)>,
    /// Corrompe 1 byte ao gravar (commit deve acusar checksum).
    pub corrupt: bool,
    /// PUT responde 404 upload_not_found (ComfyUI reiniciou).
    pub forget_uploads: bool,
}

pub type Shared = Arc<Mutex<Fake>>;

fn authorized(h: &HeaderMap) -> bool {
    h.get("authorization").and_then(|v| v.to_str().ok()) == Some(&format!("Bearer {TOKEN}"))
}

fn error(status: u16, code: &str, extra: Value) -> Response {
    let mut body = json!({"error": code, "message": "x"});
    if let (Some(b), Some(e)) = (body.as_object_mut(), extra.as_object()) {
        b.extend(e.clone());
    }
    (StatusCode::from_u16(status).unwrap(), Json(body)).into_response()
}

async fn health(h: HeaderMap) -> Response {
    if !authorized(&h) {
        return error(401, "unauthorized", json!({}));
    }
    Json(json!({"version": "1", "chunkSize": 33554432, "loraDir": "/x"})).into_response()
}

async fn init(State(s): State<Shared>, h: HeaderMap, Json(b): Json<Value>) -> Response {
    if !authorized(&h) {
        return error(401, "unauthorized", json!({}));
    }
    let mut f = s.lock().unwrap();
    f.init_calls += 1;
    if f.file_exists && b["overwrite"] != json!(true) {
        return error(409, "file_exists", json!({}));
    }
    f.declared_size = b["size"].as_u64().unwrap();
    f.part.clear();
    (
        StatusCode::CREATED,
        Json(json!({"uploadId": "up-1", "chunkSize": f.chunk_size})),
    )
        .into_response()
}

#[derive(serde::Deserialize)]
struct Off {
    offset: u64,
}

async fn put_chunk(
    State(s): State<Shared>,
    Path(_id): Path<String>,
    Query(q): Query<Off>,
    body: Bytes,
) -> Response {
    let mut f = s.lock().unwrap();
    f.puts.push((q.offset, body.len()));
    if f.forget_uploads {
        return error(404, "upload_not_found", json!({}));
    }
    if let Some((st, code)) = f.put_always {
        return error(st, code, json!({}));
    }
    if q.offset != f.part.len() as u64 {
        let expected = f.part.len();
        return error(409, "offset_mismatch", json!({"expectedOffset": expected}));
    }
    if f.partial_then_500_once {
        f.partial_then_500_once = false;
        f.part.extend_from_slice(&body[..body.len() / 2]);
        return error(500, "boom", json!({}));
    }
    f.part.extend_from_slice(&body);
    if f.corrupt && f.part.len() == body.len() {
        f.part[0] ^= 0xff;
    }
    Json(json!({"received": f.part.len()})).into_response()
}

async fn commit(State(s): State<Shared>, Json(b): Json<Value>) -> Response {
    let mut f = s.lock().unwrap();
    if f.part.len() as u64 != f.declared_size {
        return error(422, "size_mismatch", json!({}));
    }
    if hex::encode(Sha256::digest(&f.part)) != b["sha256"].as_str().unwrap() {
        f.deleted = true;
        return error(422, "checksum_mismatch", json!({}));
    }
    f.committed = Some(f.part.clone());
    Json(json!({"path": "/models/loras/hephaestus/x.safetensors", "filename": "x.safetensors"}))
        .into_response()
}

async fn abort(State(s): State<Shared>) -> StatusCode {
    s.lock().unwrap().deleted = true;
    StatusCode::NO_CONTENT
}

pub async fn serve(fake: Fake) -> (String, Shared) {
    let shared: Shared = Arc::new(Mutex::new(fake));
    let app = Router::new()
        .route("/hephaestus/health", get(health))
        .route("/hephaestus/lora/uploads", post(init))
        .route("/hephaestus/lora/uploads/:id", put(put_chunk).delete(abort))
        .route("/hephaestus/lora/uploads/:id/commit", post(commit))
        .with_state(shared.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, shared)
}
