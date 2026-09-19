//! Handlers HTTP do orchestrator (Fatia 3 — modularização).
//!
//! Extraídos de `main.rs` sem mudança de comportamento.

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};

use super::middleware::{bad_request, conflict, error_response};
use super::state::AppState;
use crate::{
    run_job, ActiveJobState, DispatchRequest, PairingVerifyRequest, PairingVerifyResponse,
    ReportBody,
};

pub(crate) async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "service": "orchestrator" }))
}

pub(crate) async fn ready(State(state): State<AppState>) -> Response {
    // /ready = S3 acessível (operação barata — head_bucket).
    // db NÃO existe no orquestrador (stateless).
    if state.s3.ping().await {
        (StatusCode::OK, Json(serde_json::json!({"status": "ready"}))).into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"status": "not ready"})),
        )
            .into_response()
    }
}

pub(crate) async fn metrics_handler(State(state): State<AppState>) -> Response {
    let active_count = state.active_jobs.len();
    let max_concurrency = state.max_concurrent_jobs;
    let s3_ok = if state.s3.ping().await { 1 } else { 0 };

    let body = format!(
        "# HELP hephaestus_orchestrator_up Service liveness\n\
         # TYPE hephaestus_orchestrator_up gauge\n\
         hephaestus_orchestrator_up 1\n\
         # HELP hephaestus_orchestrator_active_jobs Active jobs running on this node\n\
         # TYPE hephaestus_orchestrator_active_jobs gauge\n\
         hephaestus_orchestrator_active_jobs {active_count}\n\
         # HELP hephaestus_orchestrator_max_concurrent_jobs Maximum concurrent jobs allowed on this node\n\
         # TYPE hephaestus_orchestrator_max_concurrent_jobs gauge\n\
         hephaestus_orchestrator_max_concurrent_jobs {max_concurrency}\n\
         # HELP hephaestus_orchestrator_s3_connected S3 accessibility probe\n\
         # TYPE hephaestus_orchestrator_s3_connected gauge\n\
         hephaestus_orchestrator_s3_connected {s3_ok}\n"
    );

    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response()
}

/// POST /internal/dispatch — recebe job do manager (D4 :263–267).
pub(crate) async fn dispatch_handler(State(state): State<AppState>, body: Bytes) -> Response {
    if body.is_empty() {
        return bad_request("empty body");
    }

    let req: DispatchRequest = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return bad_request(&format!("invalid json: {e}"));
        }
    };

    // Validação básica do body
    if req.job_id.is_empty() {
        return bad_request("job_id is required");
    }
    if req.engine.is_empty() {
        return bad_request("engine is required");
    }
    if req.image.is_empty() {
        return bad_request("image is required");
    }
    if req.workdir.is_empty() {
        return bad_request("workdir is required");
    }
    if let Some(pr) = &req.package_ref {
        if pr.key.is_empty() {
            return bad_request("package_ref.key is required");
        }
        if pr.md5_zip.is_empty() {
            return bad_request("package_ref.md5_zip is required");
        }
    }

    // Semáforo local de GPU/VRAM: rejeita com HTTP 503 se atingiu capacidade máxima
    if state.active_jobs.len() >= state.max_concurrent_jobs {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "node_busy",
            "GPU node is currently at maximum capacity",
        );
    }

    // Idempotência R4: se já existe job com MESMO job_id em memória → 409
    if state.active_jobs.contains_key(&req.job_id) {
        return conflict("job already dispatched (duplicate job_id)");
    }

    // Registra job ativo (para idempotência)
    state
        .active_jobs
        .insert(req.job_id.clone(), ActiveJobState::new(String::new()));

    let s3 = Arc::clone(&state.s3);
    let report_client = Arc::clone(&state.report_client);
    let executor = Arc::clone(&state.executor);
    let active_jobs = Arc::clone(&state.active_jobs);
    let gpu_devices = state.gpu_devices.clone();
    let gpu_allow_mock = state.gpu_allow_mock;
    let daemon_state = state.daemon_state.clone();

    // Spawna pipeline assíncrono (D5/D6/D8)
    tokio::spawn(async move {
        run_job(
            req,
            s3,
            report_client,
            executor,
            active_jobs,
            gpu_devices,
            gpu_allow_mock,
            daemon_state,
        )
        .await;
    });

    (StatusCode::ACCEPTED, Json(serde_json::json!({"ok": true}))).into_response()
}

/// POST /internal/abort — aborta um job (D4 — manager chama em cancelling).
pub(crate) async fn abort_handler(State(state): State<AppState>, body: Bytes) -> Response {
    if body.is_empty() {
        return bad_request("empty body");
    }

    #[derive(serde::Deserialize)]
    struct AbortReq {
        job_id: String,
    }

    let req: AbortReq = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return bad_request(&format!("invalid json: {e}"));
        }
    };

    // Procura container ativo
    if let Some(entry) = state.active_jobs.get(&req.job_id) {
        entry.cancel();
        if !entry.container_name.is_empty() {
            if let Err(e) = state.executor.stop(&entry.container_name).await {
                tracing::warn!("abort: docker stop failed for {}: {e}", req.job_id);
            }
        }
        tracing::info!("abort: job {} stopped", req.job_id);
    } else {
        // Sem container vivo — reporta falha ou nada (decisão honesta)
        tracing::info!(
            "abort: job {} not found in active jobs (already finished?)",
            req.job_id
        );
        // Reporta failed se o job não está mais ativo
        let _ = state
            .report_client
            .report(
                &req.job_id,
                &ReportBody {
                    status: "failed".to_string(),
                    progress: None,
                    epoch: None,
                    step: None,
                    metrics: None,
                    error: Some("job not found or already finished".to_string()),
                    artifacts: None,
                    meta_content: None,
                    phase: None,
                    message: None,
                },
            )
            .await;
    }

    (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response()
}

/// POST /internal/pairing/verify — verifica pairing code (D5.1-2, single-use).
pub(crate) async fn pairing_verify_handler(State(state): State<AppState>, body: Bytes) -> Response {
    if body.is_empty() {
        return bad_request("empty body");
    }

    let req: PairingVerifyRequest = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return bad_request(&format!("invalid json: {e}"));
        }
    };

    if req.code.is_empty() {
        return bad_request("code is required");
    }

    let valid = state.pairing.verify(&req.code);
    (StatusCode::OK, Json(PairingVerifyResponse { valid })).into_response()
}
