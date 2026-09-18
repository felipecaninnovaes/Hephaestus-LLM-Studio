//! Upload chunked de modelos (contrato 901ebad).
//!
//! Contorna o buffering de multipart grande no proxy Next (OOM do next-server
//! com 8 GB): o cliente fatia o arquivo em partes cruas de até 96 MiB
//! (`PUT .../part/{n}` com corpo `application/octet-stream`) e o principal
//! remonta em disco no `complete`, reutilizando o fluxo do upload único.
//!
//! Sessões vivem em memória do processo (mapa global sob `Mutex`), com spool
//! das partes em `TempDir` — nunca em RAM além do chunk em trânsito. Cada
//! sessão é destruída no `complete` (sucesso ou erro) ou no `DELETE` (abort);
//! o `TempDir` é removido do disco no `drop`.
//!
//! Dívida conhecida: sem TTL/GC de sessões abandonadas (sem `complete` nem
//! `DELETE` elas ficam até o restart). Single-instance por desenho — com mais
//! de uma réplica do principal seria preciso storage compartilhado.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use axum::{
    body::Body,
    extract::{Json, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures_util::stream::StreamExt;
use serde::{Deserialize, Serialize};

use crate::error::{err, MSG_INVALID_REQUEST};
use crate::models::validate;
use crate::state::AppState;

/// Tamanho canônico da parte: 96 MiB (contrato 901ebad, `partSize`).
pub const CHUNK_PART_SIZE: u64 = 96 * 1024 * 1024;

/// `DefaultBodyLimit` do PUT de parte: 96 MiB + folga (contrato: 104 MiB).
/// Mora na camada de roteamento; o teto exato de 96 MiB mora no handler
/// (413 com envelope) — mesmo padrão do upload de datasets.
pub const CHUNK_PART_BODY_LIMIT_BYTES: usize = 104 * 1024 * 1024;

/// Registro single-instance das sessões (`uploadId → sessão`).
static SESSIONS: std::sync::LazyLock<Mutex<HashMap<String, UploadSession>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

fn lock_sessions() -> MutexGuard<'static, HashMap<String, UploadSession>> {
    SESSIONS.lock().unwrap_or_else(|e| e.into_inner())
}
/// Sessão de upload chunked: metadados do `init` + partes spooladas em disco.
struct UploadSession {
    engine: String,
    name: String,
    kind_hint: Option<String>,
    arch_hint: Option<String>,
    size: u64,
    total_parts: u32,
    parts_dir: tempfile::TempDir,
    parts: std::collections::BTreeMap<u32, u64>,
}

// ---------------------------------------------------------------------------
// Wire types (camelCase — ADR-0002 D1, schemas ModelUploadInitRequest/Response)
// ---------------------------------------------------------------------------

/// Body do `POST /api/models/uploads/init` (contrato 901ebad).
/// `size`/`totalParts` como `i64` para que valores negativos/zero caiam no
/// 400 do handler em vez de 422 de desserialização.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelUploadInitRequest {
    pub name: String,
    pub engine: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
    pub size: i64,
    pub total_parts: i64,
}

/// Resposta do `init` (contrato 901ebad).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUploadInitResponse {
    pub upload_id: String,
    pub part_size: u64,
    pub total_parts: u32,
}

// ---------------------------------------------------------------------------
// Erros (envelope D6; mensagens estáticas — nunca ecoam path/id)
// ---------------------------------------------------------------------------

fn not_found() -> Response {
    err(
        StatusCode::NOT_FOUND,
        "not_found",
        "upload session not found",
    )
}

fn invalid_request() -> Response {
    err(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        MSG_INVALID_REQUEST,
    )
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/models/uploads/init — valida tudo antecipadamente, cria a sessão
/// em memória + tempdir e devolve `{uploadId, partSize, totalParts}` (201).
/// `totalParts` precisa ser exatamente `ceil(size / 96MiB)`, senão 400.
pub async fn init_upload(Json(req): Json<ModelUploadInitRequest>) -> Response {
    // Contrato 901ebad restringe o chunked a 3 engines (o upload único
    // também serve `clip`; aqui vale o enum da spec).
    if !matches!(req.engine.as_str(), "yolo" | "world" | "diffusion") {
        return invalid_request();
    }

    // `name` é o filename cru COM extensão (o web envia `file.name`).
    let file_ext = match validate::validate_raw_filename(&req.name) {
        Ok(ext) => ext,
        Err(_) => return invalid_request(),
    };
    let mut final_name = validate::sanitize_model_name(&req.name);
    if final_name.is_empty() || final_name.len() > 255 {
        return invalid_request();
    }
    if !final_name.to_lowercase().ends_with(&file_ext) {
        final_name = format!("{final_name}{file_ext}");
        if final_name.len() > 255 {
            return invalid_request();
        }
    }

    if let Some(k) = &req.kind {
        if !validate::ALLOWED_KINDS.contains(&k.as_str()) {
            return invalid_request();
        }
    }
    if let Some(a) = &req.arch {
        if !validate::ALLOWED_ARCHS.contains(&a.as_str()) {
            return invalid_request();
        }
    }

    if req.size < 1 || req.size as u64 > validate::MODEL_MAX_FILE_BYTES {
        return invalid_request();
    }
    let size = req.size as u64;

    if req.total_parts < 1 || req.total_parts > 100_000 {
        return invalid_request();
    }
    let total_parts = req.total_parts as u32;
    let expected = (size + CHUNK_PART_SIZE - 1) / CHUNK_PART_SIZE;
    if total_parts as u64 != expected {
        return invalid_request();
    }

    let parts_dir = match tempfile::TempDir::new() {
        Ok(d) => d,
        Err(_) => return invalid_request(),
    };
    let upload_id = uuid::Uuid::new_v4().to_string();
    lock_sessions().insert(
        upload_id.clone(),
        UploadSession {
            engine: req.engine,
            name: final_name,
            kind_hint: req.kind.filter(|s| !s.is_empty()),
            arch_hint: req.arch.filter(|s| !s.is_empty()),
            size,
            total_parts,
            parts_dir,
            parts: Default::default(),
        },
    );

    (
        StatusCode::CREATED,
        Json(ModelUploadInitResponse {
            upload_id,
            part_size: CHUNK_PART_SIZE,
            total_parts,
        }),
    )
        .into_response()
}

/// PUT /api/models/uploads/{id}/part/{n} — corpo cru streamado para arquivo
/// em `parts_dir` com teto de 96 MiB (413 se estourar, 400 se vazio).
/// 404 sessão inexistente; 204 ok.
/// Reenvio da mesma parte substitui (escreve em `.tmp` + rename atômico para
/// nunca corromper a parte anterior em caso de falha no meio do stream).
pub async fn put_part(
    Path((upload_id, part_number_raw)): Path<(String, String)>,
    body: Body,
) -> Response {
    let uid = match uuid::Uuid::parse_str(&upload_id) {
        Ok(u) => u.to_string(),
        Err(_) => return not_found(),
    };
    let part_number: u32 = match part_number_raw.parse() {
        Ok(n) => n,
        Err(_) => return invalid_request(),
    };

    let parts_dir_path = {
        let guard = lock_sessions();
        match guard.get(&uid) {
            None => return not_found(),
            Some(s) => {
                if part_number >= s.total_parts {
                    return invalid_request();
                }
                s.parts_dir.path().to_path_buf()
            }
        }
    };

    let part_path = parts_dir_path.join(format!("part-{part_number:06}"));
    let tmp_path = parts_dir_path.join(format!("part-{part_number:06}.tmp"));
    let mut out = match tokio::fs::File::create(&tmp_path).await {
        Ok(f) => f,
        Err(_) => return invalid_request(),
    };

    let mut total: u64 = 0;
    let mut stream = body.into_data_stream();
    loop {
        match stream.next().await {
            Some(Ok(bytes)) => {
                total += bytes.len() as u64;
                if total > CHUNK_PART_SIZE {
                    drop(out);
                    let _ = tokio::fs::remove_file(&tmp_path).await;
                    return err(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "invalid_request",
                        MSG_INVALID_REQUEST,
                    );
                }
                if tokio::io::AsyncWriteExt::write_all(&mut out, &bytes)
                    .await
                    .is_err()
                {
                    drop(out);
                    let _ = tokio::fs::remove_file(&tmp_path).await;
                    return invalid_request();
                }
            }
            Some(Err(e)) => {
                drop(out);
                let _ = tokio::fs::remove_file(&tmp_path).await;
                // Erro opaco do axum: `LengthLimitError` no debug ⇒ estourou o
                // DefaultBodyLimit de 104 MiB da rota (mesmo padrão do
                // `is_too_large` do upload único).
                if format!("{e:?}").contains("LengthLimit") {
                    return err(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "invalid_request",
                        MSG_INVALID_REQUEST,
                    );
                }
                return invalid_request();
            }
            None => break,
        }
    }
    drop(out);
    if total == 0 {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return invalid_request();
    }
    if tokio::fs::rename(&tmp_path, &part_path).await.is_err() {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return invalid_request();
    }

    let mut guard = lock_sessions();
    match guard.get_mut(&uid) {
        None => {
            let _ = std::fs::remove_file(&part_path);
            return not_found();
        }
        Some(s) => {
            s.parts.insert(part_number, total);
        }
    }

    StatusCode::NO_CONTENT.into_response()
}

/// POST /api/models/uploads/{id}/complete — exige todas as partes
/// `0..total_parts` (400 se faltando) e soma dos sizes igual ao `size` do
/// `init` (400 se divergente, 413 se acima de 8 GiB); monta o arquivo final
/// por append sequencial num `NamedTempFile` dentro do próprio tempdir e
/// executa o fluxo compartilhado (`finalize_model_file`: magic, sniff, md5,
/// PUT S3, POST /internal/models, compensações). A sessão é destruída ao
/// final em qualquer caminho (o `TempDir` sai do disco no `drop`).
pub async fn complete_upload(
    State(state): State<AppState>,
    Path(upload_id): Path<String>,
) -> Response {
    let uid = match uuid::Uuid::parse_str(&upload_id) {
        Ok(u) => u.to_string(),
        Err(_) => return not_found(),
    };
    let session = match lock_sessions().remove(&uid) {
        Some(s) => s,
        None => return not_found(),
    };

    for n in 0..session.total_parts {
        if !session.parts.contains_key(&n) {
            return invalid_request();
        }
    }
    let sum: u64 = session.parts.values().sum();
    if sum > validate::MODEL_MAX_FILE_BYTES {
        return err(
            StatusCode::PAYLOAD_TOO_LARGE,
            "invalid_request",
            MSG_INVALID_REQUEST,
        );
    }
    if sum != session.size {
        return invalid_request();
    }

    let final_tmp = match tempfile::NamedTempFile::new_in(session.parts_dir.path()) {
        Ok(f) => f,
        Err(_) => return invalid_request(),
    };
    let mut out = match tokio::fs::File::create(final_tmp.path()).await {
        Ok(f) => f,
        Err(_) => return invalid_request(),
    };
    for n in 0..session.total_parts {
        let part_path = session.parts_dir.path().join(format!("part-{n:06}"));
        let mut inp = match tokio::fs::File::open(&part_path).await {
            Ok(f) => f,
            Err(_) => return invalid_request(),
        };
        if tokio::io::copy(&mut inp, &mut out).await.is_err() {
            return invalid_request();
        }
    }
    drop(out);

    crate::models::handlers::finalize_model_file(
        &state,
        final_tmp.path(),
        crate::models::handlers::FinalizeInput {
            engine: session.engine,
            final_name: session.name,
            kind_hint: session.kind_hint,
            arch_hint: session.arch_hint,
        },
    )
    .await
}

/// DELETE /api/models/uploads/{id} — remove tempdir + sessão; 204 idempotente
/// (também para sessão inexistente ou id malformado).
pub async fn abort_upload(Path(upload_id): Path<String>) -> Response {
    if let Ok(u) = uuid::Uuid::parse_str(&upload_id) {
        lock_sessions().remove(&u.to_string());
    }
    StatusCode::NO_CONTENT.into_response()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::manager_client::{InternalModelResponse, MockManager};
    use crate::state::AppState;
    use crate::storage::MockStorage;

    fn test_state(manager: MockManager) -> AppState {
        AppState {
            pool: sqlx::PgPool::connect_lazy("postgres://n/n").expect("lazy"),
            jwt_secret: [0x42; 32],
            secure_cookie: false,
            setup_required: false,
            storage: std::sync::Arc::new(MockStorage::new()),
            storage_config: crate::storage::StorageConfig {
                bucket: "heph-test".into(),
                public_endpoint: None,
                url_ttl_secs: 60,
            },
            embedder: std::sync::Arc::new(crate::search::MockEmbedder::new()),
            embedding_model: "ViT-B-32".to_string(),
            manager: std::sync::Arc::new(manager),
            model_download_allowed_hosts: vec![],
        }
    }

    fn mock_model_response() -> InternalModelResponse {
        InternalModelResponse {
            id: "550e8400-e29b-41d4-a716-446655440000".into(),
            name: "tiny.pt".into(),
            engine: "yolo".into(),
            model: None,
            source: "upload".into(),
            bytes: 8,
            md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
            url: None,
            job_id: None,
            created_at: "2026-09-10T12:00:00Z".into(),
            kind: None,
            arch: None,
        }
    }

    fn init_req(name: &str, engine: &str, size: i64, total_parts: i64) -> ModelUploadInitRequest {
        ModelUploadInitRequest {
            name: name.to_string(),
            engine: engine.to_string(),
            kind: None,
            arch: None,
            size,
            total_parts,
        }
    }

    async fn init_ok(name: &str, engine: &str, size: i64, total_parts: i64) -> String {
        let resp = init_upload(Json(init_req(name, engine, size, total_parts))).await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["partSize"], 100_663_296);
        assert_eq!(json["totalParts"], total_parts);
        assert!(json.get("uploadId").is_some(), "missing uploadId");
        assert!(json.get("upload_id").is_none(), "leaked snake_case");
        assert!(json.get("part_size").is_none(), "leaked snake_case");
        json["uploadId"].as_str().unwrap().to_string()
    }

    async fn put_bytes(upload_id: &str, n: u32, data: Vec<u8>) -> Response {
        put_part(
            Path((upload_id.to_string(), n.to_string())),
            Body::from(data),
        )
        .await
    }

    // --- init: validações 400 ---

    #[tokio::test]
    async fn init_400_bad_engine() {
        let resp = init_upload(Json(init_req("m.pt", "clip", 8, 1))).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn init_400_bad_extension() {
        let resp = init_upload(Json(init_req("model.pth", "yolo", 8, 1))).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn init_400_size_zero() {
        let resp = init_upload(Json(init_req("m.pt", "yolo", 0, 1))).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn init_400_size_over_8gib() {
        let resp = init_upload(Json(init_req(
            "m.pt",
            "yolo",
            8 * 1024 * 1024 * 1024 + 1,
            87,
        )))
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn init_400_total_parts_mismatch() {
        // ceil(10 / 96MiB) = 1, não 2.
        let resp = init_upload(Json(init_req("m.pt", "yolo", 10, 2))).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn init_400_bad_kind_hint() {
        let mut req = init_req("m.safetensors", "diffusion", 8, 1);
        req.kind = Some("nope".to_string());
        let resp = init_upload(Json(req)).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn init_201_then_abort() {
        let id = init_ok("m.pt", "yolo", 8, 1).await;
        let resp = abort_upload(Path(id)).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    // --- part: 204/404/413/400 ---

    #[tokio::test]
    async fn part_204_ok_and_resend_replaces() {
        let id = init_ok("m.pt", "yolo", 8, 1).await;
        let resp = put_bytes(&id, 0, b"PK\x03\x04abcd".to_vec()).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        // Reenvio substitui sem erro.
        let resp = put_bytes(&id, 0, b"PK\x03\x04efgh".to_vec()).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let resp = abort_upload(Path(id)).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn part_404_unknown_session() {
        let id = uuid::Uuid::new_v4().to_string();
        let resp = put_bytes(&id, 0, b"PK\x03\x04abcd".to_vec()).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        // Sem vazamento do id na resposta.
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(!String::from_utf8_lossy(&body).contains(&id));
    }

    #[tokio::test]
    async fn part_404_malformed_uuid() {
        let resp = put_bytes("not-a-uuid", 0, b"xx".to_vec()).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn part_400_empty_body() {
        let id = init_ok("m.pt", "yolo", 8, 1).await;
        let resp = put_bytes(&id, 0, vec![]).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let resp = abort_upload(Path(id)).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn part_400_number_out_of_range() {
        let id = init_ok("m.pt", "yolo", 8, 1).await;
        let resp = put_bytes(&id, 1, b"xx".to_vec()).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let resp = abort_upload(Path(id)).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn part_404_after_complete_session_gone() {
        // Pós-complete a sessão não existe ⇒ 404 (410 morto removido).
        let state = test_state(MockManager::default());
        let id = init_ok("m.pt", "yolo", 8, 1).await;
        let resp = complete_upload(State(state), Path(id.clone())).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let resp = put_bytes(&id, 0, b"xx".to_vec()).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn part_413_over_96mib() {
        let size = (CHUNK_PART_SIZE + 1) as i64;
        let total_parts = ((size as u64 + CHUNK_PART_SIZE - 1) / CHUNK_PART_SIZE) as i64;
        let id = init_ok("big.pt", "yolo", size, total_parts).await;
        // Stream de 97×1 MiB sem materializar os 96 MiB de uma vez.
        let stream = futures_util::stream::unfold(0u32, |n| async move {
            if n < 97 {
                Some((
                    Ok::<_, std::io::Error>(axum::body::Bytes::from(vec![0u8; 1024 * 1024])),
                    n + 1,
                ))
            } else {
                None
            }
        });
        let resp = put_part(
            Path((id.clone(), "0".to_string())),
            Body::from_stream(stream),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let resp = abort_upload(Path(id)).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    // --- complete: 400 faltando/divergente + 201 feliz ---

    #[tokio::test]
    async fn complete_400_missing_part() {
        let state = test_state(MockManager::default());
        let id = init_ok("m.pt", "yolo", 8, 1).await;
        let resp = complete_upload(State(state), Path(id.clone())).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // Sessão destruída mesmo no erro: segundo complete ⇒ 404.
        let state2 = test_state(MockManager::default());
        let resp = complete_upload(State(state2), Path(id)).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn complete_400_size_mismatch() {
        let state = test_state(MockManager::default());
        let id = init_ok("m.pt", "yolo", 8, 1).await;
        let resp = put_bytes(&id, 0, b"short".to_vec()).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let resp = complete_upload(State(state), Path(id)).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn complete_404_unknown_session() {
        let state = test_state(MockManager::default());
        let id = uuid::Uuid::new_v4().to_string();
        let resp = complete_upload(State(state), Path(id.clone())).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(!String::from_utf8_lossy(&body).contains(&id));
    }

    #[tokio::test]
    async fn complete_201_happy_path() {
        let mut mock = MockManager::default();
        mock.create_model_result = Some(mock_model_response());
        let state = test_state(mock);
        let id = init_ok("tiny.pt", "yolo", 8, 1).await;
        let resp = put_bytes(&id, 0, b"PK\x03\x04abcd".to_vec()).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let resp = complete_upload(State(state), Path(id.clone())).await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["name"], "tiny.pt");
        assert_eq!(json["engine"], "yolo");
        // Sessão destruída no sucesso: segundo complete ⇒ 404.
        let state2 = test_state(MockManager::default());
        let resp = complete_upload(State(state2), Path(id)).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // --- abort: 204 idempotente ---

    #[tokio::test]
    async fn abort_204_idempotent() {
        let id = init_ok("m.pt", "yolo", 8, 1).await;
        let resp = abort_upload(Path(id.clone())).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let resp = abort_upload(Path(id)).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let resp = abort_upload(Path(uuid::Uuid::new_v4().to_string())).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let resp = abort_upload(Path("not-a-uuid".to_string())).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
}
