//! Funções utilitárias compartilhadas entre handlers de jobs.

use axum::{http::StatusCode, response::Response};
use uuid::Uuid;

use super::types::{extract_totals_from_params, JobResponse, MetricsItem};
use crate::error::{
    err, MSG_DATASET_NOT_READY, MSG_INVALID_REQUEST, MSG_JOB_NOT_ABORTABLE, MSG_JOB_NOT_DONE,
    MSG_JOB_NOT_TERMINAL, MSG_NOT_FOUND, MSG_QUEUE_UNAVAILABLE, MSG_STORAGE_UNAVAILABLE,
};
use crate::state::AppState;
use crate::storage::StorageError;

pub fn parse_uuid(id: &str) -> Option<Uuid> {
    id.parse::<Uuid>().ok()
}

pub fn not_found() -> Response {
    err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND)
}

pub fn queue_unavailable() -> Response {
    err(
        StatusCode::SERVICE_UNAVAILABLE,
        "queue_unavailable",
        MSG_QUEUE_UNAVAILABLE,
    )
}

pub fn storage_unavailable() -> Response {
    err(
        StatusCode::SERVICE_UNAVAILABLE,
        "storage_unavailable",
        MSG_STORAGE_UNAVAILABLE,
    )
}

pub fn dataset_not_ready() -> Response {
    err(
        StatusCode::CONFLICT,
        "dataset_not_ready",
        MSG_DATASET_NOT_READY,
    )
}

pub fn job_not_abortable() -> Response {
    err(
        StatusCode::CONFLICT,
        "job_not_abortable",
        MSG_JOB_NOT_ABORTABLE,
    )
}

pub fn job_not_terminal() -> Response {
    err(
        StatusCode::CONFLICT,
        "job_not_terminal",
        MSG_JOB_NOT_TERMINAL,
    )
}

pub fn job_not_done() -> Response {
    err(StatusCode::CONFLICT, "job_not_done", MSG_JOB_NOT_DONE)
}

pub fn invalid_request() -> Response {
    err(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        MSG_INVALID_REQUEST,
    )
}

/// Re-mapeia JSONB snake_case do manager para MetricsItem camelCase.
/// A chave `mAP50-95` do JSONB vira `map5095` no wire.
pub fn remap_metrics(raw: &serde_json::Value) -> Vec<MetricsItem> {
    let items = match raw.get("items").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return vec![],
    };
    items
        .iter()
        .filter_map(|item| {
            let epoch = item.get("epoch").and_then(|v| v.as_i64())? as i32;
            let box_loss = item.get("box_loss").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let cls_loss = item.get("cls_loss").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let dfl_loss = item.get("dfl_loss").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let map50 = item.get("mAP50").and_then(|v| v.as_f64()).unwrap_or(0.0);
            // mAP50-95 → map5095: a chave no JSONB tem hífen/maiúscula.
            let map5095 = item
                .get("mAP50-95")
                .or_else(|| item.get("map50_95"))
                .or_else(|| item.get("map5095"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let loss = item.get("loss").and_then(|v| v.as_f64());
            let lr = item.get("lr").and_then(|v| v.as_f64());
            let step = item.get("step").and_then(|v| v.as_i64());
            let progress = item.get("progress").and_then(|v| v.as_f64());
            let phase = item
                .get("phase")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let message = item
                .get("message")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let vram_used_gb = item
                .get("vramUsedGb")
                .or_else(|| item.get("vram_used_gb"))
                .and_then(|v| v.as_f64());
            Some(MetricsItem {
                epoch,
                box_loss,
                cls_loss,
                dfl_loss,
                map50,
                map5095,
                loss,
                lr,
                step,
                progress,
                phase,
                message,
                vram_used_gb,
            })
        })
        .collect()
}

/// Converte `InternalJob` do manager (snake_case) para `JobResponse` (camelCase).
pub fn to_job_response(job: crate::jobs::manager_client::InternalJob) -> JobResponse {
    let metrics = job.metrics.as_ref().map(remap_metrics);
    let latest_metric = metrics.as_ref().and_then(|m| m.last());
    // AC-006-A D4: phase/phase_message vêm das colunas do job (D3),
    // com fallback de status-para-fase quando job.phase é None.
    let phase = job.phase.clone().or_else(|| match job.status.as_str() {
        "queued" => Some("queued".into()),
        "dispatched" => Some(job.status.clone()),
        "preparing" => Some("preparing".into()),
        "running" => Some("running".into()),
        "cancelling" => Some(job.status.clone()),
        "done" => Some("completed".into()),
        "failed" => Some("error".into()),
        "cancelled" => Some("cancelled".into()),
        // espelha from_job_response: status desconhecido ecoa cru (CHECK fecha o domínio, sem status real fora dos mapeados).
        _ => Some(job.status.clone()),
    });
    let phase_message = job.message.clone();
    // AC-006-A D4: vram_used_gb continua derivado da última métrica.
    let vram_used_gb = latest_metric.and_then(|m| m.vram_used_gb);

    JobResponse {
        id: job.id,
        kind: job.kind,
        engine: job.engine,
        model: job.model,
        mode: job.mode,
        dataset_id: job.dataset_id,
        status: job.status,
        queue_reason: job.queue_reason,
        queue_position: job.queue_position,
        progress: job.progress,
        epoch: job.epoch,
        step: job.step,
        metrics,
        vram_min_gb: job.vram_min_gb,
        orchestrator_id: job.orchestrator_id,
        orchestrator_name: job.orchestrator_name,
        orchestrator_kind: job.orchestrator_kind,
        orchestrator_fallback: job.orchestrator_fallback,
        created_at: job.created_at,
        finished_at: job.finished_at,
        error: job.error,
        phase,
        phase_message,
        vram_used_gb,
        total_steps: {
            let (ts, _) = extract_totals_from_params(job.params.as_ref());
            ts
        },
        total_epochs: {
            let (_, te) = extract_totals_from_params(job.params.as_ref());
            te
        },
        params: job.params,
    }
}

/// Valida que a path do artefato não contém `..` ou prefixo estranho (defesa
/// em profundidade). Retorna `Err` com 400 se inválido.
pub fn validate_artifact_path(path: &str) -> Result<(), Response> {
    if path.contains("..") || path.starts_with('/') || path.starts_with('\\') {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "invalid request",
        ));
    }
    Ok(())
}

/// Sweep best-effort de uma lista EXATA de chaves S3 (vieram do manager em
/// `DeletedJob.object_keys`, já sem as chaves das gerações preservadas).
/// Usar a lista exata (e não `delete_prefix(artifacts/{job}/)`) é o que
/// garante que a galeria sobreviva ao job, pois os bytes das gerações vivem
/// sob o MESMO prefixo. Nunca retorna erro — cada falha é logada (idiom D7).
pub async fn sweep_object_keys(state: &AppState, keys: &[String]) {
    for key in keys {
        match state.storage.delete(key).await {
            Ok(()) => {}
            Err(StorageError::NotFound) => {} // já não existia — ok
            Err(e) => {
                tracing::warn!("sweep {key} falhou ({e}) — objeto reaproveitável");
            }
        }
    }
}

/// Extrai `object_keys` (string[]) de um `DeletedJob` ou `CleanupResult` JSON.
/// Ambos expõem `object_keys` no topo (por job / agregado, já sem gerações).
pub fn extract_object_keys(v: &serde_json::Value) -> Vec<String> {
    v.get("object_keys")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|k| k.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Remapeia um `DeletedJob` snake_case (manager) → wire camelCase (ADR-0002 D1).
pub fn job_deleted_to_wire(v: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "id": v.get("id").cloned().unwrap_or(serde_json::Value::Null),
        "status": v.get("status").cloned().unwrap_or(serde_json::Value::Null),
        "artifacts": v.get("artifacts").cloned().unwrap_or_else(|| serde_json::json!([])),
        "objectKeys": v.get("object_keys").cloned().unwrap_or_else(|| serde_json::json!([])),
        "modelsDeleted": v.get("models_deleted").cloned().unwrap_or_else(|| serde_json::json!(0)),
        "generationsPreserved": v.get("generations_preserved").cloned().unwrap_or_else(|| serde_json::json!(0)),
    })
}

/// Remapeia um `CleanupResult` snake_case → wire camelCase (jobs aninhados incl.).
pub fn cleanup_result_to_wire(v: &serde_json::Value) -> serde_json::Value {
    let jobs = v
        .get("jobs")
        .and_then(|x| x.as_array())
        .map(|arr| arr.iter().map(job_deleted_to_wire).collect::<Vec<_>>())
        .unwrap_or_default();
    serde_json::json!({
        "deleted": v.get("deleted").cloned().unwrap_or_else(|| serde_json::json!(0)),
        "jobs": jobs,
        "objectKeys": v.get("object_keys").cloned().unwrap_or_else(|| serde_json::json!([])),
    })
}

pub use sweep_object_keys as sweep_s3_keys;
