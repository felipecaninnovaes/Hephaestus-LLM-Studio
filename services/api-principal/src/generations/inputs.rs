//! Upload de imagem inicial efêmera p/ img2img (`POST /api/generations/inputs`).
//!
//! Campo único `file` em `multipart/form-data`; sniff do tipo real pelo
//! conteúdo (png/jpeg/webp — `storage::sniff`), dimensões via `image`,
//! md5 hex do conteúdo, spool em tempfile + `StoragePort::put` (mesmo padrão
//! do upload de datasets, D1/D7) e INSERT em `generation_inputs`
//! (migration 0017). 201 `{id, filename, mimeType, width, height}` camelCase.
//!
//! Erros: ausente, >20 MiB ou não-imagem ⇒ 400 `invalid_request` (mensagens
//! estáticas estilo MSG_*). O contrato desta rota declara só 201/400/401 —
//! storage ou banco fora ⇒ 500 `internal` (permitido globalmente), nunca
//! `storage_unavailable` (não declarado aqui).

use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use uuid::Uuid;

use crate::error::{err, MSG_INVALID_REQUEST};
use crate::state::AppState;
use crate::storage::{keys, sniff};

/// Teto por-arquivo: 20 MiB (contrato). Acima ⇒ 400 `invalid_request`.
pub const MAX_INPUT_BYTES: i64 = 20 * 1024 * 1024;

/// Chave S3 do input: `generation_inputs/{id}/{canonical}` — `canonical` já é
/// stem sanitizado + extensão do sniff (nunca a do form).
pub fn input_object_key(id: Uuid, canonical: &str) -> String {
    format!("generation_inputs/{id}/{canonical}")
}

fn invalid_request() -> Response {
    err(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        MSG_INVALID_REQUEST,
    )
}

fn internal() -> Response {
    err(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal",
        "internal server error",
    )
}

/// Erro opaco do axum: `LengthLimitError` no debug (estouro do
/// `DefaultBodyLimit` do CORPO TOTAL — backstop da rota).
fn is_too_large(err: &axum::extract::multipart::MultipartError) -> bool {
    format!("{err:?}").contains("LengthLimit")
}

/// POST /api/generations/inputs — upload avulso de imagem inicial p/ img2img.
///
/// Ordem objeto→linha→compensação (D7): o PUT precede o INSERT; falha no
/// INSERT ⇒ delete best-effort do objeto recém-enviado. `tmp` morre no fim
/// (drop apaga o spool — D1, disco só efêmero).
pub async fn upload_generation_input(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Response {
    // 1. Lê UM campo `file` com spool em tempfile (teto 20 MiB por-arquivo:
    // ao exceder, ⇒ 400 sem acumular em RAM).
    let mut raw_name: Option<String> = None;
    let mut tmp: Option<tempfile::NamedTempFile> = None;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                // Backstop do DefaultBodyLimit OU stream morto: o contrato só
                // prevê 400 p/ oversize/ausente nesta rota.
                if is_too_large(&e) {
                    tracing::warn!("generation_inputs: corpo multipart excede o limite global");
                } else {
                    tracing::warn!(error = ?e, "generation_inputs: erro ao ler campo multipart");
                }
                return invalid_request();
            }
        };
        if field.name() != Some("file") {
            continue;
        }
        if tmp.is_some() {
            // Só o primeiro `file` vale; demais fields `file` ignorados.
            continue;
        }
        let name = match field.file_name() {
            Some(n) => n.to_string(),
            None => return invalid_request(),
        };
        // Spool chunk → tempfile com teto (padrão do upload de datasets).
        let spool_file = match tempfile::NamedTempFile::new() {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(error = ?e, "generation_inputs: falha ao criar tempfile");
                return internal();
            }
        };
        let spool_path = spool_file.path().to_path_buf();
        let mut out = match tokio::fs::File::create(&spool_path).await {
            Ok(f) => f,
            Err(e) => {
                tracing::error!(error = ?e, "generation_inputs: falha ao abrir tempfile");
                return internal();
            }
        };
        {
            use tokio::io::AsyncWriteExt;
            let mut field = field;
            let mut total: i64 = 0;
            let mut over = false;
            let mut broken = false;
            loop {
                match field.chunk().await {
                    Ok(Some(bytes)) => {
                        total += bytes.len() as i64;
                        if total > MAX_INPUT_BYTES {
                            over = true;
                            continue;
                        }
                        if out.write_all(&bytes).await.is_err() {
                            broken = true;
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        if is_too_large(&e) {
                            tracing::warn!(
                                "generation_inputs: limite global excedido durante chunk"
                            );
                        } else {
                            tracing::warn!(error = ?e, "generation_inputs: chunk corrompido");
                        }
                        return invalid_request();
                    }
                }
            }
            let _ = out.flush().await;
            if broken {
                tracing::error!("generation_inputs: erro de I/O no spool temporario");
                return internal();
            }
            if over {
                tracing::warn!(
                    limit_bytes = MAX_INPUT_BYTES,
                    "generation_inputs: arquivo rejeitado (too_large)"
                );
                return invalid_request();
            }
        }
        raw_name = Some(name);
        tmp = Some(spool_file);
    }
    let (raw_name, tmp) = match (raw_name, tmp) {
        (Some(n), Some(t)) => (n, t),
        _ => return invalid_request(),
    };

    // 2. Bytes do spool p/ sniff/dims/hash.
    let bytes = match tokio::fs::read(tmp.path()).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(error = ?e, "generation_inputs: falha ao ler bytes do tempfile");
            return internal();
        }
    };

    // 3. Sniff do conteúdo real (nunca nome/content-type do form).
    let media = match sniff::sniff(&bytes) {
        Some(m) => m,
        None => {
            tracing::warn!("generation_inputs: conteúdo não é png/jpeg/webp");
            return invalid_request();
        }
    };

    // 4. Dimensões via `image` (decode real — não-imagem ⇒ 400).
    let (width, height) = match image::load_from_memory(&bytes) {
        Ok(img) => (img.width() as i32, img.height() as i32),
        Err(_) => {
            tracing::warn!("generation_inputs: bytes não decodificam como imagem");
            return invalid_request();
        }
    };

    // 5. md5 hex minúsculo do conteúdo.
    let md5hex = {
        use md5::Digest;
        let mut h = md5::Md5::new();
        md5::Digest::update(&mut h, &bytes);
        hex::encode(md5::Digest::finalize(h))
    };

    // 6. PUT objeto→linha→compensação (D7).
    let id = Uuid::new_v4();
    let canonical = format!(
        "{}.{}",
        keys::sanitize_filename(&raw_name),
        media.extension()
    );
    let key = input_object_key(id, &canonical);
    if let Err(e) = state.storage.put(&key, tmp.path()).await {
        tracing::error!(key = %key, error = %e, "generation_inputs: falha ao persistir no storage");
        return internal();
    }
    let mime = media.content_type();
    if let Err(e) = sqlx::query(
        "INSERT INTO generation_inputs (id, s3_key, filename, mime_type, width, height, md5) \
         VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(id)
    .bind(&key)
    .bind(&raw_name)
    .bind(mime)
    .bind(width)
    .bind(height)
    .bind(&md5hex)
    .execute(&state.pool)
    .await
    {
        tracing::error!(key = %key, error = %e, "generation_inputs: erro no banco ao registrar input");
        let _ = state.storage.delete(&key).await;
        return internal();
    }

    // 7. 201 camelCase exato do contrato (`filename` = nome original enviado).
    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": id.to_string(),
            "filename": raw_name,
            "mimeType": mime,
            "width": width,
            "height": height,
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chave_formato_exato() {
        let id = Uuid::nil();
        assert_eq!(
            input_object_key(id, "foto.png"),
            format!("generation_inputs/{id}/foto.png")
        );
    }

    #[test]
    fn teto_20mib() {
        assert_eq!(MAX_INPUT_BYTES, 20 * 1024 * 1024);
    }

    #[test]
    fn canonica_usa_sniff_nao_nome() {
        let canonical = format!(
            "{}.{}",
            keys::sanitize_filename("foto.png"),
            sniff::MediaType::Jpeg.extension()
        );
        assert_eq!(canonical, "foto.jpg");
    }
}
