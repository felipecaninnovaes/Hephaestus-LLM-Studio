//! Coleta unificada de artefatos de difusão (P0-4).
//!
//! Unifica a varredura duplicada entre o caminho do Daemon e o caminho
//! One-shot em `run_job_inner`:
//! - glob `generated_*.png` (kind `generated`)
//! - glob `thumb_*.jpg`/`*.jpeg` (kind `generated_thumb`)
//! - `generation_meta.json` (kind `generated_meta`)
//! - `generated.png` legado (kind `generated`, só se NÃO houver numerados)
//!
//! Também move `read_final_metrics` e `read_generation_meta_content`
//! (usados nos reports `done` de ambos os caminhos).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domain::models::ArtifactReport;
use crate::ports::storage::S3Port;
use crate::storage::{compute_file_md5, put_with_retry, scoped_key, S3Scope};
use crate::{parse_metrics_line, tail_jsonl_lines, MetricsLine};

/// Classifica um filename do diretório de outputs em kind de artefato.
///
/// `has_numbered` indica se existe algum `generated_*.png` — nesse caso o
/// `generated.png` legado é ignorado (evita duplicidade em batch=1).
fn classify_diffusion_file(fname: &str, has_numbered: bool) -> Option<&'static str> {
    if fname.starts_with("generated_") && fname.ends_with(".png") {
        Some("generated")
    } else if fname.starts_with("thumb_") && (fname.ends_with(".jpg") || fname.ends_with(".jpeg")) {
        Some("generated_thumb")
    } else if fname == "generation_meta.json" {
        Some("generated_meta")
    } else if fname == "generated.png" && !has_numbered {
        // Legado: generated.png só coleta se NÃO houver numerados
        Some("generated")
    } else {
        None
    }
}

/// Lista os arquivos regulares de `dir` (excluindo dotfiles e `.tmp`/`.part`),
/// ordenados para determinismo.
fn list_output_files(dir: &Path) -> Vec<PathBuf> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && !p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with('.') || n.ends_with(".tmp") || n.ends_with(".part"))
                    .unwrap_or(false)
        })
        .collect();
    files.sort();
    files
}

/// Sobe um arquivo local como artefato `artifacts/<job_id>/<rel>`.
/// Retorna o `ArtifactReport` em sucesso ou a mensagem de erro coletada.
async fn upload_one(
    s3: &Arc<dyn S3Port>,
    job_id: &str,
    rel: String,
    path: &Path,
    kind: &str,
) -> Result<ArtifactReport, String> {
    let bytes = std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0);
    if bytes <= 0 {
        return Err(format!("{rel}: arquivo vazio, ignorado"));
    }
    let art_key = format!("artifacts/{job_id}/{rel}");
    let scoped = scoped_key(S3Scope::Artifacts, &art_key)
        .map_err(|e| format!("{rel}: falha ao preparar artefato (key): {e}"))?;
    let md5 = compute_file_md5(path)
        .map_err(|e| format!("{rel}: falha ao preparar artefato (md5): {e}"))?;
    put_with_retry(s3.as_ref(), &scoped, path)
        .await
        .map_err(|e| format!("{rel}: {e}"))?;
    Ok(ArtifactReport {
        kind: kind.to_string(),
        path: rel,
        md5,
        bytes,
    })
}

/// Coleta os artefatos de geração de difusão (`generated_*`, `thumb_*`,
/// `generation_meta.json`, `generated.png` legado) com upload via
/// `put_with_retry`.
///
/// Retorna `(artifacts, upload_errors)` — falhas persistentes NÃO abatem o
/// resto do loop; o chamador decide (gate "galeria vazia").
pub async fn collect_diffusion_artifacts(
    s3: &Arc<dyn S3Port>,
    job_id: &str,
    outputs: &Path,
) -> (Vec<ArtifactReport>, Vec<String>) {
    let mut artifacts = Vec::new();
    let mut upload_errors: Vec<String> = Vec::new();

    let all_files = list_output_files(outputs);
    let has_numbered = all_files.iter().any(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("generated_") && n.ends_with(".png"))
            .unwrap_or(false)
    });

    for path in &all_files {
        let fname = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if let Some(kind) = classify_diffusion_file(&fname, has_numbered) {
            match upload_one(s3, job_id, fname.clone(), path, kind).await {
                Ok(rep) => artifacts.push(rep),
                Err(e) => {
                    // Arquivo vazio é skip silencioso do original — preserva sem erro.
                    if !e.ends_with("arquivo vazio, ignorado") {
                        upload_errors.push(e);
                    }
                }
            }
        }
    }

    (artifacts, upload_errors)
}

/// Lê as métricas finais do metrics.jsonl (última linha válida).
pub fn read_final_metrics(path: &Path) -> Option<MetricsLine> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut last = None;
    for line in content.lines() {
        if let Some(m) = parse_metrics_line(line) {
            last = Some(m);
        }
    }
    last
}

/// Cap de `meta_content` no report done (D5 ADR-0023) — truncamento honesto.
pub const META_CONTENT_MAX_BYTES: usize = 256 * 1024;

/// Lê o conteúdo textual do generation_meta.json para enviar como `meta_content`
/// no report done (D5 ADR-0023). Cap: 256 KiB — truncamento honesto com warning.
pub fn read_generation_meta_content(outputs: &Path) -> Option<String> {
    let path = outputs.join("generation_meta.json");
    if !path.is_file() {
        return None;
    }
    let raw = std::fs::read(&path).ok()?;
    if raw.is_empty() {
        return None;
    }
    if raw.len() > META_CONTENT_MAX_BYTES {
        tracing::warn!(
            path = %path.display(),
            raw_bytes = raw.len(),
            cap = META_CONTENT_MAX_BYTES,
            "generation_meta.json excede 256 KiB — truncando honestamente"
        );
        let truncated = &raw[..META_CONTENT_MAX_BYTES];
        // Recorta até a última quebra de linha para não enviar JSONL cortado no meio
        let last_nl = truncated.iter().rposition(|&b| b == b'\n');
        let slice = match last_nl {
            Some(pos) => &truncated[..=pos],
            None => truncated,
        };
        let mut s = String::from_utf8_lossy(slice).into_owned();
        s.push_str("\n[TRUNCATED — original excedeu 256 KiB]\n");
        return Some(s);
    }
    // Conteúdo pequeno o suficiente — lê como UTF-8, tolerando invalid bytes
    Some(String::from_utf8_lossy(&raw).into_owned())
}

/// Coleta incremental de métricas + streaming de samples/checkpoints durante a
/// execução one-shot (poll a cada 2s).
///
/// Movida verbatim do corpo de `run_job_inner`: lê `metrics.jsonl` via
/// `tail_jsonl_lines`, escaneia `samples/` e `checkpoints/`, reporta progresso
/// via `report_client` e faz upload live best-effort via `s3`.
#[allow(clippy::too_many_arguments)]
pub async fn stream_metrics_and_samples(
    s3: Arc<dyn S3Port>,
    report_client: Arc<dyn crate::ports::reporter::ReportClient>,
    job_id: String,
    metrics_path: PathBuf,
    samples_dir: PathBuf,
    checkpoints_dir: PathBuf,
    total_epochs: i32,
    is_diffusion: bool,
) {
    use crate::compute_progress;
    use crate::domain::models::ReportBody;

    let metrics_path_clone = metrics_path.clone();
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    let mut lines_read: usize = 0;
    let mut uploaded_samples = std::collections::HashSet::<String>::new();
    let mut uploaded_checkpoints = std::collections::HashSet::<String>::new();
    loop {
        interval.tick().await;

        let mut new_live_artifacts: Vec<ArtifactReport> = Vec::new();

        // 1a. Escaneia novas amostras de difusão em tempo real
        if is_diffusion && samples_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&samples_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        let is_image = path
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(|ext| {
                                matches!(
                                    ext.to_ascii_lowercase().as_str(),
                                    "png" | "jpg" | "jpeg" | "webp"
                                )
                            })
                            .unwrap_or(false);

                        if is_image {
                            if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
                                if fname.starts_with('.')
                                    || fname.ends_with(".tmp")
                                    || fname.ends_with(".part")
                                {
                                    continue;
                                }
                                if !uploaded_samples.contains(fname) {
                                    let bytes = std::fs::metadata(&path)
                                        .map(|m| m.len() as i64)
                                        .unwrap_or(0);
                                    // Aguarda o arquivo ter tamanho > 0 (terminou de salvar)
                                    if bytes > 0 {
                                        let rel_path = format!("samples/{fname}");
                                        let art_key = format!("artifacts/{job_id}/{rel_path}");
                                        if let Ok(scoped) = scoped_key(S3Scope::Artifacts, &art_key)
                                        {
                                            if let Ok(md5) = compute_file_md5(&path) {
                                                match put_with_retry(s3.as_ref(), &scoped, &path)
                                                    .await
                                                {
                                                    Ok(()) => {
                                                        uploaded_samples.insert(fname.to_string());
                                                        new_live_artifacts.push(ArtifactReport {
                                                            kind: "sample".to_string(),
                                                            path: rel_path,
                                                            md5,
                                                            bytes,
                                                        });
                                                    }
                                                    Err(e) => {
                                                        // Live é best-effort: não anuncia o que
                                                        // não subiu; próxima tick tenta de novo.
                                                        tracing::warn!(
                                                            job_id = %job_id,
                                                            artifact = %rel_path,
                                                            error = %e,
                                                            "falha persistente no upload live de sample"
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 1b. Escaneia novos checkpoints por época em tempo real
        if is_diffusion && checkpoints_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&checkpoints_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        let is_ckpt = path
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(|ext| ext.eq_ignore_ascii_case("safetensors"))
                            .unwrap_or(false);

                        if is_ckpt {
                            if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
                                if fname.starts_with('.')
                                    || fname.ends_with(".tmp")
                                    || fname.ends_with(".part")
                                {
                                    continue;
                                }
                                if !uploaded_checkpoints.contains(fname) {
                                    let bytes = std::fs::metadata(&path)
                                        .map(|m| m.len() as i64)
                                        .unwrap_or(0);
                                    if bytes > 0 {
                                        let rel_path = format!("checkpoints/{fname}");
                                        let art_key = format!("artifacts/{job_id}/{rel_path}");
                                        if let Ok(scoped) = scoped_key(S3Scope::Artifacts, &art_key)
                                        {
                                            if let Ok(md5) = compute_file_md5(&path) {
                                                match put_with_retry(s3.as_ref(), &scoped, &path)
                                                    .await
                                                {
                                                    Ok(()) => {
                                                        uploaded_checkpoints
                                                            .insert(fname.to_string());
                                                        new_live_artifacts.push(ArtifactReport {
                                                            kind: "checkpoint".to_string(),
                                                            path: rel_path,
                                                            md5,
                                                            bytes,
                                                        });
                                                    }
                                                    Err(e) => {
                                                        // Live é best-effort: não anuncia o que
                                                        // não subiu; próxima tick tenta de novo.
                                                        tracing::warn!(
                                                            job_id = %job_id,
                                                            artifact = %rel_path,
                                                            error = %e,
                                                            "falha persistente no upload live de checkpoint"
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 2. Lê metrics.jsonl incrementalmente (helper compartilhado com o tail do daemon)
        let (new_metrics, new_lines_read) = tail_jsonl_lines(&metrics_path_clone, lines_read);
        lines_read = new_lines_read;

        // 3. Envia report se houver novas métricas OU novos artefatos (amostras/checkpoints)
        if !new_metrics.is_empty() {
            for m in new_metrics {
                let progress = compute_progress(&m, total_epochs);
                let is_metric = m.is_training_metric();
                if let Some(ref msg) = m.message {
                    tracing::info!(
                        job_id = %job_id,
                        phase = ?m.phase,
                        epoch = m.epoch,
                        "{msg}"
                    );
                }
                let _ = report_client
                    .report(
                        &job_id,
                        &ReportBody {
                            status: "running".to_string(),
                            progress: Some(progress),
                            epoch: Some(m.epoch),
                            step: m.step.map(|s| s as i32),
                            metrics: if is_metric {
                                Some(m.to_report_json())
                            } else {
                                None
                            },
                            error: None,
                            artifacts: if new_live_artifacts.is_empty() {
                                None
                            } else {
                                Some(std::mem::take(&mut new_live_artifacts))
                            },
                            meta_content: None,
                            phase: m.phase.clone(),
                            message: m.message.clone(),
                        },
                    )
                    .await;
            }
        } else if !new_live_artifacts.is_empty() {
            for art in &new_live_artifacts {
                tracing::info!(
                    job_id = %job_id,
                    artifact = %art.path,
                    "Artefato intermediário gerado e sincronizado"
                );
            }
            let _ = report_client
                .report(
                    &job_id,
                    &ReportBody {
                        status: "running".to_string(),
                        progress: None,
                        epoch: None,
                        step: None,
                        metrics: None,
                        error: None,
                        artifacts: Some(new_live_artifacts),
                        meta_content: None,
                        phase: None,
                        message: None,
                    },
                )
                .await;
        }
    }
}
