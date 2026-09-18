//! Handlers de upload e download de modelos (I.4a — ADR-0012 D3/D4).
//!
//! Upload: multipart `file` + form `engine` + `name?` → PUT S3 → POST /internal/models → 201.
//! Download: body `{url, engine, name?}` → reqwest stream → PUT S3 → POST /internal/models → 201.
//!
//! Padrão 3b: DefaultBodyLimit dedicado, spool em tempfile, magic bytes, md5,
//! compensação delete do objeto se INSERT falhar.

use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use futures_util::stream::StreamExt;
use serde::Deserialize;

use crate::error::{
    err, MSG_INVALID_REQUEST, MSG_MODEL_DOWNLOAD_DISABLED, MSG_MODEL_DOWNLOAD_FAILED,
    MSG_QUEUE_UNAVAILABLE, MSG_STORAGE_UNAVAILABLE,
};
use crate::jobs::manager_client::ManagerError;
use crate::models::validate;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Wire types (camelCase — ADR-0002 D1)
// ---------------------------------------------------------------------------

/// Resposta de upload/download de modelo (D6 ADR-0012, ADR-0023 D4).
#[derive(Debug, serde::Serialize)]
pub struct ModelResponse {
    pub id: String,
    pub name: String,
    pub engine: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub source: String,
    pub bytes: i64,
    pub md5: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(rename = "jobId", skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
}

/// Body do download (camelCase, deny_unknown_fields — D4).
#[derive(Debug, Deserialize)]
pub struct DownloadRequest {
    pub url: String,
    pub engine: String,
    #[serde(default)]
    pub name: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(crate) fn queue_unavailable() -> Response {
    err(
        StatusCode::SERVICE_UNAVAILABLE,
        "queue_unavailable",
        MSG_QUEUE_UNAVAILABLE,
    )
}

pub(crate) fn storage_unavailable() -> Response {
    err(
        StatusCode::SERVICE_UNAVAILABLE,
        "storage_unavailable",
        MSG_STORAGE_UNAVAILABLE,
    )
}

pub(crate) fn invalid_request() -> Response {
    err(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        MSG_INVALID_REQUEST,
    )
}

/// Erro opaco do axum: `LengthLimitError` no debug ⇒ 413.
fn is_too_large(err: &axum::extract::multipart::MultipartError) -> bool {
    format!("{err:?}").contains("LengthLimit")
}

/// Compute md5 hex de um arquivo.
pub(crate) async fn compute_md5(path: &std::path::Path) -> Result<String, String> {
    use md5::Digest;
    use tokio::io::AsyncReadExt;

    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| format!("open file for md5: {e}"))?;
    let mut hasher = md5::Md5::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file
            .read(&mut buf)
            .await
            .map_err(|e| format!("read for md5: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Extrai nome do arquivo de um path URL.
fn basename_from_url(url_str: &str) -> Option<String> {
    let parsed = url::Url::parse(url_str).ok()?;
    let path = parsed.path();
    let raw = path.rsplit('/').next()?;
    let base = raw.split('?').next()?;
    if base.is_empty() {
        return None;
    }
    Some(validate::sanitize_model_name(base))
}

/// Resolve host via DNS e verifica se é privado (D4 — deny ranges).
async fn resolve_and_check_private(hostname: &str) -> Result<(), Response> {
    use std::net::ToSocketAddrs;

    // DNS resolve.
    let addrs = format!("{hostname}:443")
        .to_socket_addrs()
        .map_err(|_| invalid_request())?;

    for addr in addrs {
        if validate::is_private_ip(addr.ip()) {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                MSG_INVALID_REQUEST,
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Finalização compartilhada upload único × chunked
// ---------------------------------------------------------------------------

/// Entrada canônica de um binário de modelo pronto para S3+manager.
pub(crate) struct FinalizeInput {
    pub(crate) engine: String,
    pub(crate) final_name: String,
    pub(crate) kind_hint: Option<String>,
    pub(crate) arch_hint: Option<String>,
}

/// Magic + sniff safetensors (diffusion) idênticos ao fluxo do upload único.
/// Retorna `(resolved_kind, resolved_arch)` ou uma `Response` de erro pronta.
async fn resolve_final_kind_arch(
    file_path: &std::path::Path,
    input: &FinalizeInput,
) -> Result<(Option<String>, Option<String>), Response> {
    use tokio::io::AsyncReadExt;

    let mut head = [0u8; 16];
    let mut n = 0usize;
    match tokio::fs::File::open(file_path).await {
        Ok(mut f) => {
            while n < head.len() {
                match f.read(&mut head[n..]).await {
                    Ok(0) => break,
                    Ok(k) => n += k,
                    Err(_) => break,
                }
            }
        }
        Err(_) => return Err(invalid_request()),
    };
    if !validate::validate_magic(&head[..n], &input.final_name) {
        return Err(invalid_request());
    }

    let mut resolved_kind: Option<String> = None;
    let mut resolved_arch: Option<String> = None;
    let lower_final = input.final_name.to_lowercase();
    if lower_final.ends_with(".safetensors") && input.engine == "diffusion" {
        let mut buf = vec![0u8; 8 + validate::SAFETENSORS_HEADER_MAX];
        let mut m = 0usize;
        match tokio::fs::File::open(file_path).await {
            Ok(mut f) => {
                while m < buf.len() {
                    match f.read(&mut buf[m..]).await {
                        Ok(0) => break,
                        Ok(k) => m += k,
                        Err(_) => break,
                    }
                }
            }
            Err(_) => return Err(invalid_request()),
        }
        let sniff_result = match validate::parse_safetensors_header(&buf[..m]) {
            Ok(map) => validate::sniff_safetensors(&map),
            Err(e) => Err(e),
        };
        match validate::resolve_kind_arch(
            sniff_result,
            input.kind_hint.as_deref(),
            input.arch_hint.as_deref(),
        ) {
            Ok((kind, arch)) => {
                resolved_kind = Some(kind);
                resolved_arch = if arch.is_empty() { None } else { Some(arch) };
            }
            Err(msg) => return Err(err(StatusCode::BAD_REQUEST, "invalid_request", msg)),
        }
    } else if input.engine == "diffusion" {
        if let (Some(k), Some(a)) = (&input.kind_hint, &input.arch_hint) {
            if !validate::ALLOWED_KINDS.contains(&k.as_str())
                || !validate::ALLOWED_ARCHS.contains(&a.as_str())
            {
                return Err(err(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "invalid kind or arch",
                ));
            }
            resolved_kind = Some(k.clone());
            resolved_arch = Some(a.clone());
        }
    }
    Ok((resolved_kind, resolved_arch))
}

/// S3 PUT + POST /internal/models + presign + compensações delete (D1).
/// Fluxo EXATO do upload único, reutilizado pelo complete chunked.
pub(crate) async fn finalize_model_file(
    state: &AppState,
    file_path: &std::path::Path,
    input: FinalizeInput,
) -> Response {
    let (resolved_kind, resolved_arch) = match resolve_final_kind_arch(file_path, &input).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    let md5_hex = match compute_md5(file_path).await {
        Ok(h) => h,
        Err(_) => return invalid_request(),
    };
    let bytes = match tokio::fs::metadata(file_path).await {
        Ok(m) => m.len() as i64,
        Err(_) => return invalid_request(),
    };

    let model_id = uuid::Uuid::new_v4().to_string();
    let s3_key = format!("models/{}/{}/{}", input.engine, model_id, input.final_name);

    if let Err(e) = state.storage.put(&s3_key, file_path).await {
        tracing::error!("finalize_model_file: S3 put failed: {e}");
        return storage_unavailable();
    }

    let mut payload = serde_json::json!({
        "id": model_id,
        "engine": input.engine,
        "name": input.final_name,
        "model": null,
        "source": "upload",
        "s3_key": s3_key,
        "hash": md5_hex,
        "bytes": bytes,
        "job_id": null,
    });
    if let Some(k) = &resolved_kind {
        payload["kind"] = serde_json::json!(k);
    }
    if let Some(a) = &resolved_arch {
        payload["arch"] = serde_json::json!(a);
    }

    match state.manager.create_model(&payload).await {
        Ok(resp) => {
            let url = match state.storage.presign_get(&s3_key).await {
                Ok(u) => Some(u),
                Err(_) => None,
            };
            (
                StatusCode::CREATED,
                Json(ModelResponse {
                    id: resp.id,
                    name: resp.name,
                    engine: resp.engine,
                    model: resp.model,
                    source: resp.source,
                    bytes: resp.bytes,
                    md5: resp.md5,
                    url,
                    job_id: resp.job_id,
                    created_at: resp.created_at,
                    kind: resp.kind,
                    arch: resp.arch,
                }),
            )
                .into_response()
        }
        Err(ManagerError::Conflict) => {
            let _ = state.storage.delete(&s3_key).await;
            err(StatusCode::CONFLICT, "conflict", "model already exists")
        }
        Err(ManagerError::InvalidRequest(msg)) => {
            let _ = state.storage.delete(&s3_key).await;
            tracing::warn!("finalize_model_file: manager invalid_request: {msg}");
            #[derive(serde::Serialize)]
            struct ErrorBody {
                code: &'static str,
                message: String,
            }
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorBody {
                    code: "invalid_request",
                    message: msg,
                }),
            )
                .into_response()
        }
        Err(ManagerError::Unavailable(_)) => {
            let _ = state.storage.delete(&s3_key).await;
            queue_unavailable()
        }
        Err(_) => {
            let _ = state.storage.delete(&s3_key).await;
            queue_unavailable()
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/models/upload — multipart `file` + form `engine` + `name?` + `kind?` + `arch?` (D3/D4).
///
/// Padrão 3b: DefaultBodyLimit 8 GiB+8 MiB, spool em tempfile, magic PK,
/// md5 hex, PUT S3, POST /internal/models, compensação delete se INSERT falhar.
/// Para `.safetensors` com engine=diffusion: sniff do header para classificar
/// kind+arch (ADR-0023 D4).
pub async fn upload_model(State(state): State<AppState>, mut multipart: Multipart) -> Response {
    let mut engine: Option<String> = None;
    let mut name: Option<String> = None;
    let mut kind_hint: Option<String> = None;
    let mut arch_hint: Option<String> = None;
    let mut file_field: Option<(String, tempfile::NamedTempFile)> = None;

    // Loop de fields do multipart.
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                if is_too_large(&e) {
                    return err(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "invalid_request",
                        MSG_INVALID_REQUEST,
                    );
                }
                return invalid_request();
            }
        };

        let field_name = field.name().unwrap_or("").to_string();

        match field_name.as_str() {
            "file" => {
                let raw_name = field.file_name().unwrap_or("model.pt").to_string();
                // Spool em tempfile.
                let tmp = match tempfile::NamedTempFile::new() {
                    Ok(t) => t,
                    Err(_) => return invalid_request(),
                };
                let tmp_path = tmp.path().to_path_buf();
                let mut out = match tokio::fs::File::create(&tmp_path).await {
                    Ok(f) => f,
                    Err(_) => return invalid_request(),
                };

                // Spool com teto por arquivo (validate::MODEL_MAX_FILE_BYTES).
                {
                    use tokio::io::AsyncWriteExt;
                    let mut field = field;
                    let mut total: u64 = 0;
                    let mut over = false;
                    loop {
                        match field.chunk().await {
                            Ok(Some(bytes)) => {
                                total += bytes.len() as u64;
                                if total > validate::MODEL_MAX_FILE_BYTES {
                                    over = true;
                                    continue;
                                }
                                if out.write_all(&bytes).await.is_err() {
                                    return invalid_request();
                                }
                            }
                            Ok(None) => break,
                            Err(e) => {
                                if is_too_large(&e) {
                                    return err(
                                        StatusCode::PAYLOAD_TOO_LARGE,
                                        "invalid_request",
                                        MSG_INVALID_REQUEST,
                                    );
                                }
                                return invalid_request();
                            }
                        }
                    }
                    let _ = out.flush().await;
                    if over {
                        return err(
                            StatusCode::PAYLOAD_TOO_LARGE,
                            "invalid_request",
                            MSG_INVALID_REQUEST,
                        );
                    }
                }

                file_field = Some((raw_name, tmp));
            }
            "engine" => {
                let val = field.text().await.unwrap_or_default();
                engine = Some(val);
            }
            "name" => {
                let val = field.text().await.unwrap_or_default();
                name = Some(val);
            }
            "kind" => {
                let val = field.text().await.unwrap_or_default();
                if !val.is_empty() {
                    kind_hint = Some(val);
                }
            }
            "arch" => {
                let val = field.text().await.unwrap_or_default();
                if !val.is_empty() {
                    arch_hint = Some(val);
                }
            }
            _ => {
                // Ignora campos desconhecidos.
            }
        }
    }

    // Validações.
    let engine = match engine {
        Some(e) => e,
        None => return invalid_request(),
    };

    let (raw_filename, tmp_file) = match file_field {
        Some(f) => f,
        None => return invalid_request(),
    };

    let validation = match validate::validate_upload(&engine, name.as_deref()) {
        Ok(v) => v,
        Err(validate::UploadError::InvalidEngine) => return invalid_request(),
        Err(validate::UploadError::InvalidExtension) => return invalid_request(),
        Err(validate::UploadError::InvalidMagic) => return invalid_request(),
        Err(validate::UploadError::InvalidName) => return invalid_request(),
    };

    // Validação de extensão: vem do arquivo bruto (multipart filename), não do display name.
    let file_ext = match validate::validate_raw_filename(&raw_filename) {
        Ok(ext) => ext,
        Err(_) => return invalid_request(),
    };

    // Validação de nome fornecido pelo usuário (se diferente do default).
    // A extensão do arquivo é anexada automaticamente; se o usuário já digitou
    // a extensão, não duplica.
    let final_name = {
        let sanitized = validate::sanitize_model_name(name.as_deref().unwrap_or(&validation.name));
        if sanitized.to_lowercase().ends_with(&file_ext) {
            sanitized
        } else {
            format!("{sanitized}{file_ext}")
        }
    };

    if final_name.is_empty() {
        return invalid_request();
    }

    // Magic + sniff + S3 + manager: fluxo compartilhado com o complete chunked.
    finalize_model_file(
        &state,
        tmp_file.path(),
        FinalizeInput {
            engine: validation.engine,
            final_name,
            kind_hint,
            arch_hint,
        },
    )
    .await
}

/// POST /api/models/download — body `{url, engine, name?}` (D4/E1 ADR-0012).
///
/// Server-side no principal: reqwest stream → tempdir → PUT S3 → POST /internal/models.
/// SSRF: allow-list via `AppState.model_download_allowed_hosts` (E1 — boot-loaded).
/// Redirects: `Policy::custom` com re-validação por hop contra allow-list + deny de
/// ranges privados/metadata. Cada redirect que aponta para host não autorizado ou
/// privado → 502 `model_download_failed` (falha dentro do client).
pub async fn download_model(
    State(state): State<AppState>,
    Json(body): Json<DownloadRequest>,
) -> Response {
    // Validação de domínio LOCAL antes de qualquer I/O (D4).
    let validation = match validate::validate_upload(&body.engine, body.name.as_deref()) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // Validação de URL (scheme http/https).
    let parsed_url = match validate::validate_download_url(&body.url) {
        Ok(u) => u,
        Err(_) => return invalid_request(),
    };

    // E1: allow-list via AppState (carregada no boot — não std::env::get por request).
    let allowed_hosts = &state.model_download_allowed_hosts;
    if allowed_hosts.is_empty() {
        return err(
            StatusCode::FORBIDDEN,
            "model_download_disabled",
            MSG_MODEL_DOWNLOAD_DISABLED,
        );
    }

    let hostname = match parsed_url.host_str() {
        Some(h) => h.to_lowercase(),
        None => return invalid_request(),
    };

    if !validate::host_allowed(&hostname, allowed_hosts) {
        return err(
            StatusCode::FORBIDDEN,
            "model_download_disabled",
            MSG_MODEL_DOWNLOAD_DISABLED,
        );
    }

    // Resolve DNS + deny de IP privado.
    if let Err(resp) = resolve_and_check_private(&hostname).await {
        return resp;
    }

    // Nome final: extensão vem do basename da URL; display name do body.name é opcional.
    let url_basename = basename_from_url(&body.url).unwrap_or_else(|| "model.pt".to_string());
    let file_ext = match validate::validate_raw_filename(&url_basename) {
        Ok(ext) => ext,
        Err(_) => return invalid_request(),
    };
    let final_name = {
        let sanitized =
            validate::sanitize_model_name(body.name.as_deref().unwrap_or(&url_basename));
        if sanitized.to_lowercase().ends_with(&file_ext) {
            sanitized
        } else {
            format!("{sanitized}{file_ext}")
        }
    };
    if final_name.is_empty() {
        return invalid_request();
    }

    // Baixar com reqwest: stream para tempdir, cap 2 GiB, timeouts.
    let tmp_dir = match tempfile::tempdir() {
        Ok(d) => d,
        Err(_) => return invalid_request(),
    };
    let tmp_path = tmp_dir.path().join(&final_name);

    // E1b: redirect policy com re-validação por hop.
    // Cada redirect é verificado contra a allow-list + deny de privados antes
    // de seguir; host não autorizado → 502 (falha dentro do client).
    let allowed_hosts_clone = allowed_hosts.clone();
    let redirect_policy = reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= validate::MAX_REDIRECTS as usize {
            return attempt.error("too many redirects");
        }
        let url = attempt.url();
        let hop_host = match url.host_str() {
            Some(h) => h.to_lowercase(),
            None => return attempt.error("no hostname in redirect"),
        };
        if !validate::host_allowed(&hop_host, &allowed_hosts_clone) {
            return attempt.error("redirect to non-allowed host");
        }
        // DNS + deny de privados no hop.
        use std::net::ToSocketAddrs;
        if let Ok(addrs) = format!("{hop_host}:443").to_socket_addrs() {
            for addr in addrs {
                if validate::is_private_ip(addr.ip()) {
                    return attempt.error("redirect to private IP");
                }
            }
        }
        attempt.follow()
    });

    let client = match reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .timeout(std::time::Duration::from_secs(120))
        .redirect(redirect_policy)
        .build()
    {
        Ok(c) => c,
        Err(_) => return invalid_request(),
    };

    let resp = match client.get(&body.url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("download_model: request failed: {e}");
            return err(
                StatusCode::BAD_GATEWAY,
                "model_download_failed",
                MSG_MODEL_DOWNLOAD_FAILED,
            );
        }
    };

    // Checagem de status HTTP da fonte (A1 — review Fatia I).
    if !resp.status().is_success() {
        tracing::warn!(
            "download_model: upstream returned {} {}",
            resp.status().as_u16(),
            resp.status().canonical_reason().unwrap_or(""),
        );
        return err(
            StatusCode::BAD_GATEWAY,
            "model_download_failed",
            MSG_MODEL_DOWNLOAD_FAILED,
        );
    }

    // Verificação final pós-redirect (redundante com Policy mas segura).
    let final_url = resp.url().clone();
    let final_hostname = final_url.host_str().unwrap_or("").to_lowercase();
    if !validate::host_allowed(&final_hostname, allowed_hosts) {
        return err(
            StatusCode::BAD_GATEWAY,
            "model_download_failed",
            MSG_MODEL_DOWNLOAD_FAILED,
        );
    }
    if let Err(_resp) = resolve_and_check_private(&final_hostname).await {
        return err(
            StatusCode::BAD_GATEWAY,
            "model_download_failed",
            MSG_MODEL_DOWNLOAD_FAILED,
        );
    }

    // Stream para disco com cap 2 GiB.
    let content_length = resp.content_length().unwrap_or(0);
    if content_length > validate::MODEL_MAX_FILE_BYTES {
        return err(
            StatusCode::PAYLOAD_TOO_LARGE,
            "model_download_failed",
            MSG_MODEL_DOWNLOAD_FAILED,
        );
    }

    let mut file = match tokio::fs::File::create(&tmp_path).await {
        Ok(f) => f,
        Err(_) => {
            return err(
                StatusCode::BAD_GATEWAY,
                "model_download_failed",
                MSG_MODEL_DOWNLOAD_FAILED,
            );
        }
    };

    let mut downloaded: u64 = 0;
    let mut stream = resp.bytes_stream();
    use tokio::io::AsyncWriteExt;

    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("download_model: stream error: {e}");
                return err(
                    StatusCode::BAD_GATEWAY,
                    "model_download_failed",
                    MSG_MODEL_DOWNLOAD_FAILED,
                );
            }
        };
        downloaded += chunk.len() as u64;
        if downloaded > validate::MODEL_MAX_FILE_BYTES {
            return err(
                StatusCode::PAYLOAD_TOO_LARGE,
                "model_download_failed",
                MSG_MODEL_DOWNLOAD_FAILED,
            );
        }
        if file.write_all(&chunk).await.is_err() {
            return err(
                StatusCode::BAD_GATEWAY,
                "model_download_failed",
                MSG_MODEL_DOWNLOAD_FAILED,
            );
        }
    }
    let _ = file.flush().await;

    // Magic bytes check.
    let head = {
        use tokio::io::AsyncReadExt;
        let mut f = match tokio::fs::File::open(&tmp_path).await {
            Ok(f) => f,
            Err(_) => {
                return err(
                    StatusCode::BAD_GATEWAY,
                    "model_download_failed",
                    MSG_MODEL_DOWNLOAD_FAILED,
                );
            }
        };
        let mut buf = [0u8; 16];
        let mut n = 0usize;
        while n < 16 {
            match f.read(&mut buf[n..]).await {
                Ok(0) => break,
                Ok(k) => n += k,
                Err(_) => break,
            }
        }
        buf[..n].to_vec()
    };

    if !validate::validate_magic(&head, &final_name) {
        return invalid_request();
    }

    // md5.
    let md5_hex = match compute_md5(&tmp_path).await {
        Ok(h) => h,
        Err(_) => {
            return err(
                StatusCode::BAD_GATEWAY,
                "model_download_failed",
                MSG_MODEL_DOWNLOAD_FAILED,
            );
        }
    };

    let bytes = downloaded as i64;

    // UUID gerado pelo principal.
    let model_id = uuid::Uuid::new_v4().to_string();

    // Chave S3: models/<engine>/<id>/<name>
    let s3_key = format!("models/{}/{}/{}", validation.engine, model_id, final_name);

    // PUT no S3.
    if let Err(e) = state.storage.put(&s3_key, &tmp_path).await {
        tracing::error!("download_model: S3 put failed: {e}");
        return storage_unavailable();
    }

    // POST /internal/models.
    let payload = serde_json::json!({
        "id": model_id,
        "engine": validation.engine,
        "name": final_name,
        "model": null,
        "source": "download",
        "s3_key": s3_key,
        "hash": md5_hex,
        "bytes": bytes,
        "url": body.url,
        "job_id": null,
    });

    match state.manager.create_model(&payload).await {
        Ok(resp) => {
            let url = match state.storage.presign_get(&s3_key).await {
                Ok(u) => Some(u),
                Err(_) => None,
            };
            (
                StatusCode::CREATED,
                Json(ModelResponse {
                    id: resp.id,
                    name: resp.name,
                    engine: resp.engine,
                    model: resp.model,
                    source: resp.source,
                    bytes: resp.bytes,
                    md5: resp.md5,
                    url,
                    job_id: resp.job_id,
                    created_at: resp.created_at,
                    kind: resp.kind,
                    arch: resp.arch,
                }),
            )
                .into_response()
        }
        Err(ManagerError::Conflict) => {
            let _ = state.storage.delete(&s3_key).await;
            err(StatusCode::CONFLICT, "conflict", "model already exists")
        }
        Err(ManagerError::InvalidRequest(msg)) => {
            let _ = state.storage.delete(&s3_key).await;
            tracing::warn!("download_model: manager invalid_request: {msg}");
            #[derive(serde::Serialize)]
            struct ErrorBody {
                code: &'static str,
                message: String,
            }
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorBody {
                    code: "invalid_request",
                    message: msg,
                }),
            )
                .into_response()
        }
        Err(ManagerError::Unavailable(_)) => {
            let _ = state.storage.delete(&s3_key).await;
            queue_unavailable()
        }
        Err(_) => {
            let _ = state.storage.delete(&s3_key).await;
            queue_unavailable()
        }
    }
}

/// DELETE /api/models/:id — remove modelo da tabela models e storage se upload/download (ADR-0012 R5).
pub async fn delete_model(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    let uid = match uuid::Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return err(StatusCode::NOT_FOUND, "not_found", "model not found"),
    };

    let model = match state.manager.delete_model(&uid.to_string()).await {
        Ok(m) => m,
        Err(ManagerError::NotFound) => {
            return err(StatusCode::NOT_FOUND, "not_found", "model not found");
        }
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };

    // Se a origem for upload ou download, remove também do S3/SeaweedFS (D1/R5).
    // Para checkpoints de treino ('train'), o binário pertence ao histórico de jobs
    // em artifacts/<job_id>/... e não é destruído.
    if matches!(model.source.as_str(), "upload" | "download") {
        if let Err(e) = state.storage.delete(&model.path).await {
            tracing::warn!(
                "delete_model: falha ao remover {} do storage: {e}",
                model.path
            );
        }
    }

    StatusCode::NO_CONTENT.into_response()
}

/// Body para atualização de modelo (ADR-0022 D2).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateModelRequest {
    pub name: String,
}

/// PATCH /api/models/:id — renomeia modelo no catálogo (ADR-0022 D2).
pub async fn update_model(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::extract::Json(req): axum::extract::Json<UpdateModelRequest>,
) -> Response {
    let uid = match uuid::Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return err(StatusCode::NOT_FOUND, "not_found", "model not found"),
    };

    let clean_name = req.name.trim();
    if clean_name.is_empty() || clean_name.chars().count() > 255 {
        return err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "name must be between 1 and 255 characters",
        );
    }

    let m = match state
        .manager
        .update_model(&uid.to_string(), clean_name)
        .await
    {
        Ok(m) => m,
        Err(ManagerError::NotFound) => {
            return err(StatusCode::NOT_FOUND, "not_found", "model not found");
        }
        Err(ManagerError::InvalidRequest(_)) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                MSG_INVALID_REQUEST,
            );
        }
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };

    let url = match state.storage.presign_get(&m.path).await {
        Ok(u) => Some(u),
        Err(_) => None,
    };

    let name = if m.name.is_empty() {
        m.path.rsplit('/').next().unwrap_or(&m.path).to_string()
    } else {
        m.name
    };

    let resp = ModelResponse {
        id: m.id,
        name,
        engine: m.engine,
        model: m.model,
        source: m.source,
        bytes: m.bytes,
        md5: m.md5,
        url,
        job_id: m.job_id,
        created_at: m.created_at,
        kind: m.kind,
        arch: m.arch,
    };

    (StatusCode::OK, Json(resp)).into_response()
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

    fn test_state_with_hosts(manager: MockManager, hosts: Vec<String>) -> AppState {
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
            model_download_allowed_hosts: hosts,
        }
    }

    #[allow(dead_code)]
    fn mock_model_response() -> InternalModelResponse {
        InternalModelResponse {
            id: "550e8400-e29b-41d4-a716-446655440000".into(),
            name: "best.pt".into(),
            engine: "yolo".into(),
            model: None,
            source: "upload".into(),
            bytes: 1024,
            md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
            url: None,
            job_id: None,
            created_at: "2026-09-10T12:00:00Z".into(),
            kind: None,
            arch: None,
        }
    }

    // --- Upload tests (validation only — multipart handler tested via integration) ---

    #[test]
    fn validate_upload_engine_unsupported_400() {
        assert_eq!(
            validate::validate_upload("unsupported", Some("best.pt")),
            Err(validate::UploadError::InvalidEngine)
        );
    }

    #[test]
    fn validate_upload_engine_diffusion_ok() {
        assert!(validate::validate_upload("diffusion", Some("model.safetensors")).is_ok());
    }

    #[test]
    fn validate_upload_name_without_ext_ok() {
        // Display name sem extensão é aceito — extensão vem do arquivo.
        let v = validate::validate_upload("yolo", Some("meu lora v2")).unwrap();
        assert_eq!(v.engine, "yolo");
        assert_eq!(v.name, "meu_lora_v2");
    }

    #[test]
    fn validate_raw_filename_ext_ok() {
        assert_eq!(validate::validate_raw_filename("model.pt").unwrap(), ".pt");
        assert_eq!(
            validate::validate_raw_filename("model.safetensors").unwrap(),
            ".safetensors"
        );
    }

    #[test]
    fn validate_raw_filename_pth_400() {
        assert_eq!(
            validate::validate_raw_filename("model.pth"),
            Err(validate::UploadError::InvalidExtension)
        );
    }

    #[test]
    fn validate_magic_pk_ok() {
        assert!(validate::validate_magic(b"PK\x03\x04rest", "model.pt"));
    }

    #[test]
    fn validate_magic_png_400() {
        assert!(!validate::validate_magic(b"\x89PNG", "model.pt"));
    }

    #[test]
    fn host_allowed_exact() {
        let allowed = vec!["huggingface.co".to_string()];
        assert!(validate::host_allowed("huggingface.co", &allowed));
        assert!(!validate::host_allowed("evil.com", &allowed));
    }

    #[test]
    fn host_allowed_wildcard() {
        let allowed = vec!["*.githubusercontent.com".to_string()];
        assert!(validate::host_allowed(
            "raw.githubusercontent.com",
            &allowed
        ));
        assert!(!validate::host_allowed("github.com", &allowed));
    }

    #[test]
    fn is_private_ip_ranges() {
        assert!(validate::is_private_ip("127.0.0.1".parse().unwrap()));
        assert!(validate::is_private_ip("10.0.0.1".parse().unwrap()));
        assert!(validate::is_private_ip("172.16.0.1".parse().unwrap()));
        assert!(validate::is_private_ip("192.168.0.1".parse().unwrap()));
        assert!(validate::is_private_ip("169.254.1.1".parse().unwrap()));
        assert!(!validate::is_private_ip("8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn is_private_ip_ipv6() {
        assert!(validate::is_private_ip("::1".parse().unwrap()));
        assert!(validate::is_private_ip("fc00::1".parse().unwrap()));
        assert!(validate::is_private_ip("fe80::1".parse().unwrap()));
        assert!(!validate::is_private_ip("2001:db8::1".parse().unwrap()));
    }

    #[test]
    fn url_basename_extraction() {
        let u = url::Url::parse("https://example.com/models/best.pt").unwrap();
        assert_eq!(validate::url_basename(&u), "best.pt");
    }

    #[test]
    fn sanitize_model_name_clean() {
        assert_eq!(validate::sanitize_model_name("best.pt"), "best.pt");
    }

    #[test]
    fn sanitize_model_name_dirty() {
        let s = validate::sanitize_model_name("best model (1).pt");
        assert!(s.ends_with(".pt"));
        assert!(!s.contains(' '));
    }

    #[test]
    fn sanitize_model_name_empty_after() {
        assert_eq!(validate::sanitize_model_name("..."), "");
    }

    // --- Download 400 tests ---

    #[test]
    fn download_validate_url_ftp_400() {
        assert!(validate::validate_download_url("ftp://example.com/model.pt").is_err());
    }

    #[test]
    fn download_validate_url_ok() {
        assert!(validate::validate_download_url("https://example.com/model.pt").is_ok());
        assert!(validate::validate_download_url("http://example.com/model.pt").is_ok());
    }

    #[test]
    fn download_name_not_pt_400() {
        // raw filename sem extensão aceita deve falhar.
        assert_eq!(
            validate::validate_raw_filename("model.pth"),
            Err(validate::UploadError::InvalidExtension)
        );
        assert_eq!(
            validate::validate_raw_filename("model"),
            Err(validate::UploadError::InvalidExtension)
        );
    }

    // --- final_name logic tests (extensão sempre vem do arquivo) ---

    #[test]
    fn final_name_from_file_ext() {
        // name "meu-lora" + arquivo x.safetensors → "meu-lora.safetensors"
        let sanitized = validate::sanitize_model_name("meu-lora");
        let file_ext = ".safetensors";
        let final_name = if sanitized.to_lowercase().ends_with(file_ext) {
            sanitized
        } else {
            format!("{sanitized}{file_ext}")
        };
        assert_eq!(final_name, "meu-lora.safetensors");
    }

    #[test]
    fn final_name_no_duplicate_ext() {
        // name "lora.safetensors" + arquivo y.safetensors → "lora.safetensors"
        let sanitized = validate::sanitize_model_name("lora.safetensors");
        let file_ext = ".safetensors";
        let final_name = if sanitized.to_lowercase().ends_with(file_ext) {
            sanitized
        } else {
            format!("{sanitized}{file_ext}")
        };
        assert_eq!(final_name, "lora.safetensors");
    }

    #[test]
    fn final_name_empty_user_uses_file_ext() {
        // name vazio (default "model") + arquivo model.pt → "model.pt"
        let sanitized = validate::sanitize_model_name("model");
        let file_ext = ".pt";
        let final_name = if sanitized.to_lowercase().ends_with(file_ext) {
            sanitized
        } else {
            format!("{sanitized}{file_ext}")
        };
        assert_eq!(final_name, "model.pt");
    }

    // --- Download handler tests (E1: allow-list via state) ---

    #[tokio::test]
    async fn download_400_invalid_scheme() {
        let mock = MockManager::default();
        let state = test_state_with_hosts(mock, vec!["example.com".into()]);

        let body = DownloadRequest {
            url: "ftp://example.com/model.pt".to_string(),
            engine: "yolo".to_string(),
            name: None,
        };

        let resp = download_model(axum::extract::State(state), Json(body)).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn download_400_invalid_engine() {
        let mock = MockManager::default();
        let state = test_state_with_hosts(mock, vec!["example.com".into()]);

        let body = DownloadRequest {
            url: "https://example.com/model.pt".to_string(),
            engine: "unsupported".to_string(),
            name: None,
        };

        let resp = download_model(axum::extract::State(state), Json(body)).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn download_403_disabled_when_no_hosts() {
        let mock = MockManager::default();
        let state = test_state(mock); // model_download_allowed_hosts: vec![]

        let body = DownloadRequest {
            url: "https://example.com/model.pt".to_string(),
            engine: "yolo".to_string(),
            name: None,
        };

        let resp = download_model(axum::extract::State(state), Json(body)).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(json["code"], "model_download_disabled");
    }

    #[tokio::test]
    async fn download_403_host_not_in_allow_list() {
        let mock = MockManager::default();
        let state = test_state_with_hosts(mock, vec!["huggingface.co".into()]);

        let body = DownloadRequest {
            url: "https://evil.com/model.pt".to_string(),
            engine: "yolo".to_string(),
            name: None,
        };

        let resp = download_model(axum::extract::State(state), Json(body)).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn download_status_check_code_path() {
        let err_msg = crate::error::MSG_MODEL_DOWNLOAD_FAILED;
        assert_eq!(err_msg, "model download failed");
    }

    #[tokio::test]
    async fn delete_model_204_ok() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let id = uuid::Uuid::new_v4().to_string();
        let resp = delete_model(axum::extract::State(state), axum::extract::Path(id)).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn delete_model_404_invalid_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let resp = delete_model(
            axum::extract::State(state),
            axum::extract::Path("not-a-uuid".into()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_model_404_not_found() {
        let mut mock = MockManager::default();
        mock.delete_model_not_found = true;
        let state = test_state(mock);
        let id = uuid::Uuid::new_v4().to_string();
        let resp = delete_model(axum::extract::State(state), axum::extract::Path(id)).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_model_503_manager_offline() {
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let id = uuid::Uuid::new_v4().to_string();
        let resp = delete_model(axum::extract::State(state), axum::extract::Path(id)).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn update_model_200_ok() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let id = uuid::Uuid::new_v4().to_string();
        let req = UpdateModelRequest {
            name: "cyberpunk-flux2-v1.safetensors".to_string(),
        };
        let resp = update_model(
            axum::extract::State(state),
            axum::extract::Path(id),
            axum::Json(req),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["name"], "cyberpunk-flux2-v1.safetensors");
    }

    #[tokio::test]
    async fn update_model_400_empty_name() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let id = uuid::Uuid::new_v4().to_string();
        let req = UpdateModelRequest {
            name: "   ".to_string(),
        };
        let resp = update_model(
            axum::extract::State(state),
            axum::extract::Path(id),
            axum::Json(req),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn update_model_400_too_long() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let id = uuid::Uuid::new_v4().to_string();
        let req = UpdateModelRequest {
            name: "a".repeat(256),
        };
        let resp = update_model(
            axum::extract::State(state),
            axum::extract::Path(id),
            axum::Json(req),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn update_model_404_invalid_uuid() {
        let mock = MockManager::default();
        let state = test_state(mock);
        let req = UpdateModelRequest {
            name: "novo-nome.safetensors".to_string(),
        };
        let resp = update_model(
            axum::extract::State(state),
            axum::extract::Path("not-a-uuid".into()),
            axum::Json(req),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn update_model_404_not_found() {
        let mut mock = MockManager::default();
        mock.update_model_not_found = true;
        let state = test_state(mock);
        let id = uuid::Uuid::new_v4().to_string();
        let req = UpdateModelRequest {
            name: "novo-nome.safetensors".to_string(),
        };
        let resp = update_model(
            axum::extract::State(state),
            axum::extract::Path(id),
            axum::Json(req),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn update_model_503_manager_offline() {
        let mut mock = MockManager::default();
        mock.fail = true;
        let state = test_state(mock);
        let id = uuid::Uuid::new_v4().to_string();
        let req = UpdateModelRequest {
            name: "novo-nome.safetensors".to_string(),
        };
        let resp = update_model(
            axum::extract::State(state),
            axum::extract::Path(id),
            axum::Json(req),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
