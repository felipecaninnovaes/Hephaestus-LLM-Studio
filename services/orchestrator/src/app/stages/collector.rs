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
    outputs: &Path,
) -> Result<ArtifactReport, String> {
    let bytes = std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0);
    if bytes <= 0 {
        return Err(format!("{rel}: arquivo vazio, ignorado"));
    }
    let art_key = format!("artifacts/{job_id}/{rel}");
    let scoped = scoped_key(S3Scope::Artifacts, &art_key)
        .map_err(|e| format!("{rel}: falha ao preparar artefato (key): {e}"))?;
    let path_buf = path.to_path_buf();
    let md5 = tokio::task::spawn_blocking(move || compute_file_md5(&path_buf))
        .await
        .map_err(|e| format!("{rel}: falha ao preparar artefato (spawn md5): {e}"))?
        .map_err(|e| format!("{rel}: falha ao preparar artefato (md5): {e}"))?;
    put_with_retry(s3.as_ref(), &scoped, path)
        .await
        .map_err(|e| format!("{rel}: {e}"))?;
    crate::storage::append_manifest(outputs, &rel).await;
    Ok(ArtifactReport {
        kind: kind.to_string(),
        path: rel,
        md5,
        bytes,
    })
}

/// C2a: sobe (ou ressobe, overwrite) o snapshot atual do `telemetry.jsonl`
/// como artefato `logs/telemetry.jsonl` (kind `logs`).
///
/// Objeto MÚTVEL por design: cada ciclo reenvia o arquivo inteiro sob a mesma
/// key. O manager deduplica reports por `(job_id, path)` (md5/bytes do ROW
/// ficam do primeiro report — honesto: consumidores do log — BFF `/logs` e
/// `/artifacts/zip` — leem o objeto, nunca confiam no md5 deste kind).
/// Best-effort: falha loga warn e não abate o job.
pub async fn upload_telemetry_snapshot(
    s3: &Arc<dyn S3Port>,
    job_id: &str,
    telemetry_path: &Path,
    outputs: &Path,
) -> Option<ArtifactReport> {
    let size = std::fs::metadata(telemetry_path)
        .map(|m| m.len())
        .unwrap_or(0);
    if size == 0 {
        return None;
    }
    match upload_one(
        s3,
        job_id,
        "logs/telemetry.jsonl".to_string(),
        telemetry_path,
        "logs",
        outputs,
    )
    .await
    {
        Ok(rep) => Some(rep),
        Err(e) => {
            tracing::warn!(
                job_id = %job_id,
                error = %e,
                "falha best-effort no upload live de telemetry.jsonl (C2a)"
            );
            None
        }
    }
}

/// Fatia 1c: sobe (ou ressobe, overwrite) o snapshot atual de `logs/run.log`
/// (stdout+stderr do container, gravado em streaming pelo executor) como
/// artefato `logs/run.log` (kind `logs`). Mesmo padrão growth-gated/mutável
/// de `upload_telemetry_snapshot` (C2a); best-effort: falha loga warn e não
/// abate o job.
pub async fn upload_run_log_snapshot(
    s3: &Arc<dyn S3Port>,
    job_id: &str,
    run_log_path: &Path,
    outputs: &Path,
) -> Option<ArtifactReport> {
    let size = std::fs::metadata(run_log_path)
        .map(|m| m.len())
        .unwrap_or(0);
    if size == 0 {
        return None;
    }
    match upload_one(
        s3,
        job_id,
        "logs/run.log".to_string(),
        run_log_path,
        "logs",
        outputs,
    )
    .await
    {
        Ok(rep) => Some(rep),
        Err(e) => {
            tracing::warn!(
                job_id = %job_id,
                error = %e,
                "falha best-effort no upload live de run.log (Fatia 1c)"
            );
            None
        }
    }
}

/// Sobe o `captions.jsonl` parcial de um job autolabel que terminou
/// `failed`/`cancelled` após o container rodar (o engine escreve uma linha
/// flushada por imagem). Só sobe se o arquivo existir e não for vazio.
/// Best-effort: falha loga warn e devolve `None` — o report terminal segue.
pub async fn upload_partial_captions(
    s3: &Arc<dyn S3Port>,
    job_id: &str,
    outputs: &Path,
) -> Option<ArtifactReport> {
    let path = outputs.join("captions.jsonl");
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if size == 0 {
        return None;
    }
    match upload_one(
        s3,
        job_id,
        "captions.jsonl".to_string(),
        &path,
        "captions",
        outputs,
    )
    .await
    {
        Ok(rep) => Some(rep),
        Err(e) => {
            tracing::warn!(
                job_id = %job_id,
                error = %e,
                "falha best-effort no upload de captions parciais (autolabel)"
            );
            None
        }
    }
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
    let all_files = list_output_files(outputs);
    let has_numbered = all_files.iter().any(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("generated_") && n.ends_with(".png"))
            .unwrap_or(false)
    });

    let concurrency: usize = std::env::var("ARTIFACT_UPLOAD_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3)
        .max(1);

    let mut items = Vec::new();
    for (orig_idx, path) in all_files.iter().enumerate() {
        let fname = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if let Some(kind) = classify_diffusion_file(&fname, has_numbered) {
            items.push((orig_idx, fname, path.clone(), kind));
        }
    }

    let mut join_set = tokio::task::JoinSet::new();
    let mut next_idx = 0;
    let mut results: Vec<(usize, Result<ArtifactReport, String>)> = Vec::with_capacity(items.len());

    while next_idx < items.len() || !join_set.is_empty() {
        while next_idx < items.len() && join_set.len() < concurrency {
            let (orig_idx, fname, path, kind) = items[next_idx].clone();
            next_idx += 1;
            let s3_clone = Arc::clone(s3);
            let job_id_string = job_id.to_string();
            let outputs_buf = outputs.to_path_buf();
            join_set.spawn(async move {
                let res =
                    upload_one(&s3_clone, &job_id_string, fname, &path, kind, &outputs_buf).await;
                (orig_idx, res)
            });
        }

        if let Some(res) = join_set.join_next().await {
            match res {
                Ok((idx, upload_res)) => results.push((idx, upload_res)),
                Err(join_err) => {
                    results.push((usize::MAX, Err(format!("task join error: {join_err}"))))
                }
            }
        }
    }

    // Ordenação determinística igual à ordem de listagem original
    results.sort_by_key(|(idx, _)| *idx);

    let mut artifacts = Vec::new();
    let mut upload_errors = Vec::new();
    for (_, res) in results {
        match res {
            Ok(rep) => artifacts.push(rep),
            Err(e) => {
                if !e.ends_with("arquivo vazio, ignorado") {
                    upload_errors.push(e);
                }
            }
        }
    }

    (artifacts, upload_errors)
}

/// Lê as métricas finais de telemetry.jsonl (primário) ou metrics.jsonl (fallback).
pub fn read_final_metrics(path: &Path) -> Option<MetricsLine> {
    let telemetry_path = path.with_file_name("telemetry.jsonl");
    let target = if telemetry_path.exists() {
        &telemetry_path
    } else {
        path
    };
    let content = std::fs::read_to_string(target).ok()?;
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

/// Anexa métricas de GPU (`sys.gpu.*`) a um mapa de métricas para um report com step (fatia B3).
///
/// Lê os sensores do `GpuSampler` compartilhado para a GPU do job.
/// Apenas reports com `epoch` e `step` recebem essas chaves.
/// Sensor `None` → chave ausente (nunca 0 — PITFALLS:39).
pub async fn attach_gpu_metrics_if_applicable(
    metrics: &mut serde_json::Value,
    step: Option<i64>,
    gpu_sampler: Option<&crate::telemetry::gpu::GpuSampler>,
    target_gpu_uuid: Option<&str>,
) {
    // Reports sem step não recebem métricas de GPU (UNIQUE (job_id, key, epoch, step) descartaria)
    if step.is_none() {
        return;
    }
    let (Some(sampler), Some(uuid)) = (gpu_sampler, target_gpu_uuid) else {
        return;
    };
    let Some(device) = sampler.get_device_by_uuid(uuid).await else {
        return;
    };

    if let Some(obj) = metrics.as_object_mut() {
        if let Some(util) = device.gpu_utilization_pct {
            if util.is_finite() {
                obj.insert("sys.gpu.util_pct".to_string(), serde_json::json!(util));
            }
        }
        if let Some(temp) = device.temperature_c {
            obj.insert("sys.gpu.temp_c".to_string(), serde_json::json!(temp));
        }
        if let Some(power) = device.power_watts {
            if power.is_finite() {
                obj.insert("sys.gpu.power_w".to_string(), serde_json::json!(power));
            }
        }
        obj.insert(
            "sys.gpu.vram_used_mb".to_string(),
            serde_json::json!(device.vram_used),
        );
    }
}

/// Resolve o UUID da GPU para um job de acordo com as regras da fatia B3:
/// 1. `dispatch.gpu_device` se presente.
/// 2. Sem ele, a única GPU de `ORCH_GPU_DEVICES` se for exatamente uma (resolve índice→UUID pelo sampler).
/// 3. Senão retorna `None` (não emite métricas de GPU).
pub async fn resolve_job_gpu_uuid(
    dispatch_gpu: Option<&str>,
    orch_gpu_devices: Option<&str>,
    sampler: Option<&crate::telemetry::gpu::GpuSampler>,
) -> Option<String> {
    let target = match dispatch_gpu {
        Some(g) if !g.trim().is_empty() => Some(g.trim()),
        _ => match orch_gpu_devices {
            Some(devices) => {
                let list: Vec<&str> = devices
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .collect();
                if list.len() == 1 {
                    Some(list[0])
                } else {
                    None
                }
            }
            None => None,
        },
    }?;

    if target.starts_with("GPU-") {
        return Some(target.to_string());
    }

    if let Ok(idx) = target.parse::<u32>() {
        if let Some(s) = sampler {
            if let Some(sample) = s.sample().await {
                if let Some(dev) = sample.devices.iter().find(|d| d.index == idx) {
                    return Some(dev.uuid.clone());
                }
            }
        }
    }

    None
}

/// Coleta incremental de métricas + streaming de samples/checkpoints durante a
/// execução one-shot (poll a cada 2s).
///
/// Movida verbatim do corpo de `run_job_inner`: lê `metrics.jsonl` via
/// `tail_jsonl_lines`, escaneia `samples/` e `checkpoints/`, reporta progresso
/// via `report_client` e faz upload live best-effort via `s3`.
#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip_all, fields(job_id = %job_id))]
pub async fn stream_metrics_and_samples(
    s3: Arc<dyn S3Port>,
    report_client: Arc<dyn crate::ports::reporter::ReportClient>,
    job_id: String,
    metrics_path: PathBuf,
    samples_dir: PathBuf,
    checkpoints_dir: PathBuf,
    total_epochs: i32,
    is_diffusion: bool,
    run_log_path: PathBuf,
    gpu_sampler: Option<crate::telemetry::gpu::GpuSampler>,
    target_gpu_uuid: Option<String>,
) {
    use crate::compute_progress;
    use crate::domain::models::ReportBody;

    let metrics_path_clone = metrics_path.clone();
    let outputs_dir = metrics_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    let mut byte_offset: usize = 0;
    let mut uploaded_samples = std::collections::HashSet::<String>::new();
    let mut uploaded_checkpoints = std::collections::HashSet::<String>::new();
    // C2a: bytes do último snapshot de telemetry.jsonl enviado (growth-gate).
    let mut telemetry_uploaded_bytes: i64 = 0;
    // Fatia 1c: idem para logs/run.log.
    let mut run_log_uploaded_bytes: i64 = 0;
    // Artefatos cujo report falhou num tick anterior: reenviados no próximo
    // tick junto com os novos (o manager deduplica report por path, idempotente).
    let mut pending_artifacts: Vec<ArtifactReport> = Vec::new();
    loop {
        interval.tick().await;

        let mut new_live_artifacts: Vec<ArtifactReport> = std::mem::take(&mut pending_artifacts);

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
                                            let path_for_md5 = path.clone();
                                            let md5_res = tokio::task::spawn_blocking(move || {
                                                compute_file_md5(&path_for_md5)
                                            })
                                            .await
                                            .unwrap_or_else(|e| Err(e.to_string()));
                                            if let Ok(md5) = md5_res {
                                                match put_with_retry(s3.as_ref(), &scoped, &path)
                                                    .await
                                                {
                                                    Ok(()) => {
                                                        uploaded_samples.insert(fname.to_string());
                                                        crate::storage::append_manifest(
                                                            &outputs_dir,
                                                            &rel_path,
                                                        )
                                                        .await;
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
                        let fname_opt = path.file_name().and_then(|n| n.to_str());
                        let is_ckpt = path
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(|ext| ext.eq_ignore_ascii_case("safetensors"))
                            .unwrap_or(false)
                            || fname_opt
                                .map(|n| n.ends_with("_optimizer.pt"))
                                .unwrap_or(false);

                        if is_ckpt {
                            if let Some(fname) = fname_opt {
                                if fname.starts_with('.')
                                    || fname.ends_with(".tmp")
                                    || fname.ends_with(".part")
                                {
                                    continue;
                                }
                                let kind = if fname.ends_with("_optimizer.pt") {
                                    "optimizer_state"
                                } else {
                                    "checkpoint"
                                };
                                if !uploaded_checkpoints.contains(fname) {
                                    let bytes = std::fs::metadata(&path)
                                        .map(|m| m.len() as i64)
                                        .unwrap_or(0);
                                    if bytes > 0 {
                                        let rel_path = format!("checkpoints/{fname}");
                                        let art_key = format!("artifacts/{job_id}/{rel_path}");
                                        if let Ok(scoped) = scoped_key(S3Scope::Artifacts, &art_key)
                                        {
                                            let path_for_md5 = path.clone();
                                            let md5_res = tokio::task::spawn_blocking(move || {
                                                compute_file_md5(&path_for_md5)
                                            })
                                            .await
                                            .unwrap_or_else(|e| Err(e.to_string()));
                                            if let Ok(md5) = md5_res {
                                                match put_with_retry(s3.as_ref(), &scoped, &path)
                                                    .await
                                                {
                                                    Ok(()) => {
                                                        uploaded_checkpoints
                                                            .insert(fname.to_string());
                                                        crate::storage::append_manifest(
                                                            &outputs_dir,
                                                            &rel_path,
                                                        )
                                                        .await;
                                                        new_live_artifacts.push(ArtifactReport {
                                                            kind: kind.to_string(),
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

        // 2. Lê telemetry.jsonl primariamente; fallback para metrics.jsonl (RD-022)
        let telemetry_path = metrics_path_clone.with_file_name("telemetry.jsonl");
        let active_path = if telemetry_path.exists() {
            &telemetry_path
        } else {
            &metrics_path_clone
        };
        let active_path_buf = active_path.clone();
        let (new_metrics, new_byte_offset) =
            tokio::task::spawn_blocking(move || tail_jsonl_lines(&active_path_buf, byte_offset))
                .await
                .unwrap_or_else(|_| (Vec::new(), byte_offset));

        // C2a: re-upload do snapshot do telemetry.jsonl quando ele cresce —
        // logs de treino ficam persistidos em S3 DURANTE a execução (o report
        // de artefato é idempotente por path no manager; só interessa anunciar
        // enquanto não anunciado — o growth-gate já cobre a parte do objeto).
        if active_path == telemetry_path.as_path() {
            if let Ok(size) = std::fs::metadata(active_path) {
                let size = size.len() as i64;
                if size > telemetry_uploaded_bytes {
                    if let Some(rep) =
                        upload_telemetry_snapshot(&s3, &job_id, active_path, &outputs_dir).await
                    {
                        telemetry_uploaded_bytes = size;
                        new_live_artifacts.push(rep);
                    }
                }
            }
        }

        // Fatia 1c: re-upload do snapshot de logs/run.log quando ele cresce —
        // mesmo growth-gate de telemetry.jsonl acima.
        if let Ok(size) = std::fs::metadata(&run_log_path) {
            let size = size.len() as i64;
            if size > run_log_uploaded_bytes {
                if let Some(rep) =
                    upload_run_log_snapshot(&s3, &job_id, &run_log_path, &outputs_dir).await
                {
                    run_log_uploaded_bytes = size;
                    new_live_artifacts.push(rep);
                }
            }
        }

        // 3. Envia report se houver novas métricas OU novos artefatos (amostras/checkpoints)
        if !new_metrics.is_empty() {
            let mut report_failed = false;
            for m in new_metrics {
                let progress = compute_progress(&m, total_epochs);
                let is_metric = m.is_training_metric();
                if let Some(msg) = &m.message {
                    tracing::info!(
                        job_id = %job_id,
                        phase = ?m.phase,
                        epoch = m.epoch,
                        "{msg}"
                    );
                }
                let artifacts_for_report = if new_live_artifacts.is_empty() {
                    None
                } else {
                    Some(std::mem::take(&mut new_live_artifacts))
                };
                let result = report_client
                    .report(
                        &job_id,
                        &ReportBody {
                            status: "running".to_string(),
                            progress: Some(progress),
                            epoch: Some(m.epoch),
                            step: m.step.map(|s| s as i32),
                            metrics: if is_metric {
                                let mut j = m.to_report_json();
                                attach_gpu_metrics_if_applicable(
                                    &mut j,
                                    m.step,
                                    gpu_sampler.as_ref(),
                                    target_gpu_uuid.as_deref(),
                                )
                                .await;
                                Some(j)
                            } else {
                                None
                            },
                            error: None,
                            artifacts: artifacts_for_report.clone(),
                            meta_content: None,
                            phase: m.phase.clone(),
                            message: m.message.clone(),
                        },
                    )
                    .await;
                if let Err(e) = result {
                    tracing::warn!(
                        job_id = %job_id,
                        error = %e,
                        "falha ao reportar métricas ao manager; reenviando no próximo tick"
                    );
                    report_failed = true;
                    if let Some(arts) = artifacts_for_report {
                        pending_artifacts.extend(arts);
                    }
                    break;
                }
            }
            // Escrita do manager é idempotente por (job_id, key, epoch, step): só
            // avança o offset de leitura quando TODO o lote deste tick foi
            // reportado com sucesso; falha reenvia o lote inteiro no próximo tick.
            if !report_failed {
                byte_offset = new_byte_offset;
            }
        } else {
            byte_offset = new_byte_offset;
            if !new_live_artifacts.is_empty() {
                for art in &new_live_artifacts {
                    tracing::info!(
                        job_id = %job_id,
                        artifact = %art.path,
                        "Artefato intermediário gerado e sincronizado"
                    );
                }
                let arts = std::mem::take(&mut new_live_artifacts);
                let result = report_client
                    .report(
                        &job_id,
                        &ReportBody {
                            status: "running".to_string(),
                            progress: None,
                            epoch: None,
                            step: None,
                            metrics: None,
                            error: None,
                            artifacts: Some(arts.clone()),
                            meta_content: None,
                            phase: None,
                            message: None,
                        },
                    )
                    .await;
                if let Err(e) = result {
                    tracing::warn!(
                        job_id = %job_id,
                        error = %e,
                        "falha ao reportar artefatos intermediários ao manager; reenviando no próximo tick"
                    );
                    pending_artifacts.extend(arts);
                }
            }
        }
    }
}
