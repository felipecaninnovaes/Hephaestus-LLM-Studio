//! Handlers de visualização e download de artefatos e logs de jobs.

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};

use super::helpers::{
    not_found, parse_uuid, queue_unavailable, storage_unavailable, validate_artifact_path,
};
use super::types::{ArtifactListResponse, ArtifactResponse, JobLogLine, JobLogPage, JobLogsQuery};
use crate::error::{err, MSG_STORAGE_UNAVAILABLE};
use crate::jobs::manager_client::{self, ManagerError};
use crate::state::AppState;
use crate::storage::StorageError;

/// GET /api/jobs/:id/artifacts — lista artefatos de um job.
pub async fn list_artifacts(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if parse_uuid(&id).is_none() {
        return not_found();
    }
    let arts = match state.manager.list_artifacts(&id).await {
        Ok(v) => v,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let items: Vec<ArtifactResponse> = arts
        .into_iter()
        .map(|a| ArtifactResponse {
            id: a.id,
            kind: a.kind,
            path: a.path,
            md5: a.md5,
            bytes: a.bytes,
        })
        .collect();
    (StatusCode::OK, Json(ArtifactListResponse { items })).into_response()
}

/// GET /api/jobs/:id/artifacts/:artifactId/data — proxy do objeto do artefato.
///
/// Metadata via manager + objeto via StoragePort (admin).
/// Key = `artifacts/<job_id>/<path>` (D8). Confere `md5` se barato.
pub async fn get_artifact_data(
    State(state): State<AppState>,
    Path((id, artifact_id)): Path<(String, String)>,
) -> Response {
    if parse_uuid(&id).is_none() || parse_uuid(&artifact_id).is_none() {
        return not_found();
    }
    // 1. Busca artefatos do job para encontrar o path/md5 pelo artifactId.
    let arts = match state.manager.list_artifacts(&id).await {
        Ok(v) => v,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let art = match arts.iter().find(|a| a.id == artifact_id) {
        Some(a) => a,
        None => return not_found(),
    };
    // Defesa em profundidade: valida path do artefato.
    if let Err(resp) = validate_artifact_path(&art.path) {
        return resp;
    }
    let key = format!("artifacts/{id}/{}", art.path);
    // 2. Busca objeto via StoragePort (admin).
    let bytes = match state.storage.get(&key).await {
        Ok(b) => b,
        Err(StorageError::NotFound) => {
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "storage_unavailable",
                MSG_STORAGE_UNAVAILABLE,
            );
        }
        Err(StorageError::Unavailable(_)) => return storage_unavailable(),
    };
    // 3. Confere md5 se barato (bytes já em RAM).
    let computed = format!(
        "{:x}",
        md5::Digest::finalize({
            use md5::Digest;
            let mut h = md5::Md5::new();
            md5::Digest::update(&mut h, &bytes);
            h
        })
    );
    if computed != art.md5 {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_unavailable",
            MSG_STORAGE_UNAVAILABLE,
        );
    }
    let content_type = if art.path.ends_with(".png") {
        "image/png"
    } else if art.path.ends_with(".jpg") || art.path.ends_with(".jpeg") {
        "image/jpeg"
    } else if art.path.ends_with(".webp") {
        "image/webp"
    } else {
        "application/octet-stream"
    };
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable",
            ),
        ],
        bytes,
    )
        .into_response()
}

/// Parse tolerante de uma linha do jsonl de telemetria.
///
/// `message` canônico com fallback `phaseMessage` (engine-kit vs espelho
/// legado); loss/lr aninhados sob `metrics` são irrelevantes para o log.
/// Linha não-JSON/objeto → mensagem bruta (nunca drop silencioso).
pub fn parse_job_log_line(line: &str) -> JobLogLine {
    let s_field = |v: &serde_json::Value, k: &str| -> Option<String> {
        v.get(k).and_then(|x| x.as_str()).map(String::from)
    };
    match serde_json::from_str::<serde_json::Value>(line.trim()) {
        Ok(v) if v.is_object() => {
            let message = s_field(&v, "message").or_else(|| s_field(&v, "phaseMessage"));
            JobLogLine {
                timestamp: s_field(&v, "timestamp"),
                phase: s_field(&v, "phase"),
                message,
                progress: v.get("progress").and_then(|x| x.as_f64()),
                epoch: v.get("epoch").and_then(|x| x.as_i64()),
                step: v.get("step").and_then(|x| x.as_i64()),
            }
        }
        _ => JobLogLine {
            timestamp: None,
            phase: None,
            message: Some(line.to_string()),
            progress: None,
            epoch: None,
            step: None,
        },
    }
}

/// Elege o artefato de log do job: `logs/telemetry.jsonl` > qualquer path
/// terminado em `telemetry.jsonl` > `metrics.jsonl` (legado, jobs antigos).
pub fn pick_log_artifact<'a>(
    arts: &'a [manager_client::InternalArtifact],
) -> Option<&'a manager_client::InternalArtifact> {
    let by = |pred: &dyn Fn(&str) -> bool| {
        arts.iter()
            .find(|a| a.kind != "model" && pred(&a.path) && validate_artifact_path(&a.path).is_ok())
    };
    by(&|p| p == "logs/telemetry.jsonl")
        .or_else(|| by(&|p| p.ends_with("telemetry.jsonl")))
        .or_else(|| by(&|p| p == "metrics.jsonl" || p.ends_with("/metrics.jsonl")))
}

fn empty_log_page() -> Response {
    (
        StatusCode::OK,
        Json(JobLogPage {
            lines: vec![],
            next_offset: 0,
            eof: true,
        }),
    )
        .into_response()
}

/// GET /api/jobs/:id/logs?offset=&limit= — C2a.
///
/// Fonte: artefato de log do job (telemetry.jsonl persistido incrementalmente
/// pelo orquestrador; métrica legado `metrics.jsonl` como fallback), lido via
/// StoragePort. Job sem artefato de log → 200 `{lines:[], eof:true}` (não 404
/// — ausência de log é estado válido, o job existe).
pub async fn get_job_logs(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<JobLogsQuery>,
) -> Response {
    if parse_uuid(&id).is_none() {
        return not_found();
    }
    match state.manager.get_job(&id).await {
        Ok(_) => {}
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let arts = match state.manager.list_artifacts(&id).await {
        Ok(v) => v,
        Err(ManagerError::NotFound) => return not_found(),
        Err(_) => return queue_unavailable(),
    };
    let Some(art) = pick_log_artifact(&arts) else {
        return empty_log_page();
    };
    let key = format!("artifacts/{id}/{}", art.path);
    let bytes = match state.storage.get(&key).await {
        Ok(b) => b,
        // Row existe mas o objeto sumiu (GC/backup): página vazia honesta.
        Err(StorageError::NotFound) => return empty_log_page(),
        Err(StorageError::Unavailable(_)) => return storage_unavailable(),
    };
    let text = String::from_utf8_lossy(&bytes);
    let all: Vec<&str> = text.lines().collect();
    let offset = q.offset.unwrap_or(0).max(0);
    let limit = q.limit.unwrap_or(500).clamp(1, 2000);
    let start = (offset as usize).min(all.len());
    let end = (start + limit as usize).min(all.len());
    let lines: Vec<JobLogLine> = all[start..end]
        .iter()
        .map(|l| parse_job_log_line(l))
        .collect();
    let page = JobLogPage {
        lines,
        next_offset: end as i64,
        eof: end >= all.len(),
    };
    (StatusCode::OK, Json(page)).into_response()
}

// ---------------------------------------------------------------------------
// F2: GET /api/jobs/:id/artifacts/zip — ZIP `stored` de todos os artefatos
// ---------------------------------------------------------------------------

/// Nome de arquivo do zip: `params.outputName` sanitizado, senão o id do job.
pub fn zip_filename_for_job(id: &str, params: Option<&serde_json::Value>) -> String {
    let raw = params
        .and_then(|p| p.get("outputName"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let clean: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let stem = clean.trim_matches('_').trim_matches('.');
    if stem.is_empty() {
        format!("job-{id}")
    } else {
        format!("{stem}-artifacts")
    }
}

/// GET /api/jobs/:id/artifacts/zip — F2 (streaming no BFF, zip `stored`:
/// safetensors/png já comprimem; compressão própria seria CPU por nada).
///
/// Padrão de produção idêntico ao zip de gerações (ADR-0006): spool por
/// objeto em tempdir (`get_to_file`, um de cada vez — nunca o job inteiro
/// em RAM), `ZipWriter` em `spawn_blocking`, `ReaderStream` no body (o fd
/// sobrevive ao unlink do TempDir em Unix). Zero artefatos → 404.
/// Órfão (row sem objeto) → skip + warn honesto, o zip segue com o resto.
pub async fn download_artifacts_zip(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    if parse_uuid(&id).is_none() {
        return not_found();
    }
    let job = match state.manager.get_job(&id).await {
        Ok(v) => v,
        Err(ManagerError::NotFound) => return not_found(),
        Err(_) => return queue_unavailable(),
    };
    let arts = match state.manager.list_artifacts(&id).await {
        Ok(v) => v,
        Err(ManagerError::NotFound) => return not_found(),
        Err(_) => return queue_unavailable(),
    };
    if arts.is_empty() {
        return not_found();
    }
    let tmp = match tempfile::TempDir::new() {
        Ok(d) => d,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };
    let mut zip_entries: Vec<(String, std::path::PathBuf)> = Vec::new();
    let mut used_names: Vec<String> = Vec::new();
    for (i, art) in arts.iter().enumerate() {
        if validate_artifact_path(&art.path).is_err() {
            tracing::warn!(job_id=%id, artifact=%art.path, "zip: artefato com path suspeito, ignorado");
            continue;
        }
        let key = format!("artifacts/{id}/{}", art.path);
        let dest = tmp.path().join(format!("entry_{i}"));
        match state.storage.get_to_file(&key, &dest).await {
            Ok(()) => {
                let mut arcname = art.path.clone();
                if used_names.contains(&arcname) {
                    arcname = format!("{}/{}", art.kind, art.path);
                }
                used_names.push(arcname.clone());
                zip_entries.push((arcname, dest));
            }
            Err(StorageError::NotFound) => {
                tracing::warn!(job_id=%id, artifact=%art.path, "zip: objeto ausente, entrada ignorada");
            }
            Err(StorageError::Unavailable(_)) => return storage_unavailable(),
        }
    }
    if zip_entries.is_empty() {
        return err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "no artifacts could be exported",
        );
    }
    let zip_path = tmp.path().join("artifacts.zip");
    let zip_path_blocking = zip_path.clone();
    let blocking: Result<Result<(), std::io::Error>, tokio::task::JoinError> =
        tokio::task::spawn_blocking(move || {
            let file = std::fs::File::create(&zip_path_blocking)?;
            let mut zip = zip::ZipWriter::new(file);
            for (arcname, fs_path) in &zip_entries {
                let options = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored);
                zip.start_file(arcname, options)?;
                let mut src = std::fs::File::open(fs_path)?;
                std::io::copy(&mut src, &mut zip)?;
            }
            zip.finish()?;
            Ok(())
        })
        .await;
    if !matches!(blocking, Ok(Ok(()))) {
        return err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "internal server error",
        );
    }
    let len = match tokio::fs::metadata(&zip_path).await {
        Ok(m) => m.len(),
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };
    let file = match tokio::fs::File::open(&zip_path).await {
        Ok(f) => f,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };
    let filename = format!("{}.zip", zip_filename_for_job(&id, job.params.as_ref()));
    let stream = tokio_util::io::ReaderStream::new(file);
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
            (header::CONTENT_LENGTH, len.to_string()),
        ],
        axum::body::Body::from_stream(stream),
    )
        .into_response()
}

#[cfg(test)]
mod job_logs_zip_tests {
    use super::*;
    use uuid::Uuid;

    fn art(kind: &str, path: &str) -> manager_client::InternalArtifact {
        manager_client::InternalArtifact {
            id: Uuid::new_v4().to_string(),
            kind: kind.to_string(),
            path: path.to_string(),
            md5: "d41d8cd98f00b204e9800998ecf8427e".to_string(),
            bytes: 1,
        }
    }

    #[test]
    fn parses_canonical_telemetry_line() {
        let l = parse_job_log_line(
            r#"{"timestamp":"2026-09-20T00:00:00Z","phase":"training","phaseMessage":"Época 1","message":"Época 1/10 · Step 30","progress":0.5,"step":30,"epoch":3,"metrics":{"loss":0.0452,"lr":0.0001}}"#,
        );
        assert_eq!(l.timestamp.as_deref(), Some("2026-09-20T00:00:00Z"));
        assert_eq!(l.phase.as_deref(), Some("training"));
        assert_eq!(l.message.as_deref(), Some("Época 1/10 · Step 30"));
        assert_eq!(l.step, Some(30));
    }

    #[test]
    fn falls_back_to_phase_message() {
        let l = parse_job_log_line(r#"{"phase":"preparing","phaseMessage":"Prep"}"#);
        assert_eq!(l.message.as_deref(), Some("Prep"));
    }

    #[test]
    fn garbage_line_kept_as_raw_message() {
        let l = parse_job_log_line("not-json");
        assert_eq!(l.message.as_deref(), Some("not-json"));
        assert!(l.timestamp.is_none() && l.phase.is_none());
    }

    #[test]
    fn log_artifact_preference_telemetry_over_metrics() {
        let arts = vec![
            art("metrics", "metrics.jsonl"),
            art("logs", "logs/telemetry.jsonl"),
        ];
        assert_eq!(
            pick_log_artifact(&arts).unwrap().path,
            "logs/telemetry.jsonl"
        );
        let legacy = vec![art("metrics", "metrics.jsonl")];
        assert_eq!(pick_log_artifact(&legacy).unwrap().path, "metrics.jsonl");
        let none = vec![art("model", "best.safetensors")];
        assert!(pick_log_artifact(&none).is_none());
    }

    #[test]
    fn zip_filename_sanitized_with_fallback() {
        // A função devolve o STEM; o chamador acrescenta ".zip".
        assert_eq!(
            zip_filename_for_job(
                "abc",
                Some(&serde_json::json!({"outputName": "meu/lora 2"}))
            ),
            "meu_lora_2-artifacts"
        );
        assert_eq!(
            zip_filename_for_job("abc", Some(&serde_json::json!({"outputName": "###"}))),
            "job-abc"
        );
        assert_eq!(zip_filename_for_job("abc", None), "job-abc");
    }
}
