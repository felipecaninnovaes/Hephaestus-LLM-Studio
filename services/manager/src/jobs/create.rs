//! Criação e enfileiramento de jobs (MM-13).

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::ManagerError;
use super::resolve::*;
use super::types::{CreateJobRequest, CreateJobResponse};

/// Cria um job. Retorna (job_id, status, queue_position).
///
/// Fluxo assíncrono (ADR-0025 D0): sem package_ref + `params.prepare` (objeto)
/// → `preparing` (queue_position NULL); com package_ref → `queued` (legado).
/// Omissão total (sem package_ref nem prepare) → `queued` legado
/// (retrocompat); intenção async malformada (package_ref null sem prepare, ou
/// prepare não-objeto) → `InvalidRequest` (400).
///
/// VRAM policy: `vram_min_gb` é gravado mas ignorado na decisão de fila (no-op
/// sem GPU — D9/R3). Ver `@gpu` para o fluxo real com `waiting_vram`.
pub async fn create_job(
    pool: &PgPool,
    req: CreateJobRequest,
) -> Result<CreateJobResponse, ManagerError> {
    let job_id = Uuid::new_v4();
    let dataset_id: Option<Uuid> = req.dataset_id.as_deref().and_then(|s| s.parse().ok());

    // Monta params: merge package_ref de cima se params não tiver.
    let mut params = req.params.unwrap_or(serde_json::json!({}));
    if params.get("package_ref").is_none() {
        if let Some(ref pr) = req.package_ref {
            if let Ok(v) = serde_json::to_value(pr) {
                params["package_ref"] = v;
            }
        }
    }

    // Resolve weights_id → weights_ref (ADR-0012 D5 — fail-fast no submit).
    // Para predict: também resolve variante do modelo para jobs.model (D5).
    let mut resolved_model = req.model.clone();
    if let Some(weights_id) = req.weights_id {
        let (weights_ref, model) =
            resolve_weights_ref(pool, weights_id, &req.engine, &req.mode, &req.model).await?;
        params["weights_ref"] = serde_json::json!({
            "s3_key": weights_ref.s3_key,
            "md5": weights_ref.md5,
        });
        resolved_model = model;
    }

    // Validação de orchestrator_hint (ADR-0015 D2).
    if let Some(ref hint_str) = req.orchestrator_hint {
        let hint_uuid = resolve_orchestrator_hint(pool, hint_str).await?;
        params["orchestrator_hint"] = serde_json::json!(hint_uuid.to_string());
    }

    // Resolução multi-LoRA + custom checkpoint + text encoder + init_image (ADR-0023, S4)
    if req.engine == "diffusion" && (req.mode == "generate" || req.mode == "train") {
        if let Some(loras_arr) = params.get("loras").and_then(|v| v.as_array()) {
            let resolved_loras = resolve_loras(pool, loras_arr).await?;
            if let Ok(v) = serde_json::to_value(&resolved_loras) {
                params["loras"] = v;
            }
        }

        if let Some(custom_id_str) = params.get("customModelId").and_then(|v| v.as_str()) {
            let resolved = resolve_custom_checkpoint(pool, custom_id_str).await?;
            if let Ok(v) = serde_json::to_value(&resolved) {
                params["custom_checkpoint"] = v;
            }
        }

        if let Some(encoder_id_str) = params.get("textEncoderModelId").and_then(|v| v.as_str()) {
            let resolved = resolve_text_encoder(pool, encoder_id_str, &params).await?;
            if let Ok(v) = serde_json::to_value(&resolved) {
                params["text_encoder_ref"] = v;
            }
        }

        if req.mode == "generate" {
            if let Some(resolved) = resolve_init_image(pool, &params).await? {
                if let Ok(v) = serde_json::to_value(&resolved) {
                    params["init_image_ref"] = v;
                }
            }
        }
    }

    // Fluxo assíncrono (ADR-0025 D0): package_ref ausente + params.prepare
    let has_package = req.package_ref.is_some()
        || params
            .get("package_ref")
            .map(|v| {
                v.is_object()
                    && v.get("key")
                        .and_then(|k| k.as_str())
                        .map(|s| !s.is_empty())
                        .unwrap_or(false)
            })
            .unwrap_or(false);
    let prepare_field = params.get("prepare");
    let has_prepare = prepare_field.map(|v| v.is_object()).unwrap_or(false);
    if !has_package && !has_prepare {
        let package_null = params
            .get("package_ref")
            .map(|v| v.is_null())
            .unwrap_or(false);
        if package_null || prepare_field.is_some() {
            if prepare_field.is_some() {
                return Err(ManagerError::InvalidRequest(
                    "params.prepare deve ser um objeto".into(),
                ));
            }
            return Err(ManagerError::InvalidRequest(
                "job com package_ref null exige params.prepare (fluxo assíncrono ADR-0025)".into(),
            ));
        }
    }

    let initial_status = if !has_package && has_prepare {
        "preparing"
    } else {
        "queued"
    };

    sqlx::query(
        "INSERT INTO jobs (id, kind, engine, model, mode, dataset_id, params, config_yaml, vram_min_gb, status, queue_reason) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NULL)",
    )
    .bind(job_id)
    .bind(&req.kind)
    .bind(&req.engine)
    .bind(&resolved_model)
    .bind(&req.mode)
    .bind(dataset_id)
    .bind(&params)
    .bind(&req.config_yaml)
    .bind(req.vram_min_gb)
    .bind(initial_status)
    .execute(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("insert job: {e}")))?;

    // Posição na fila: só jobs `queued` contam; `preparing` → NULL.
    let queue_position = if initial_status == "queued" {
        let queue_pos: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM jobs WHERE status = 'queued' AND created_at < (SELECT created_at FROM jobs WHERE id = $1)",
        )
        .bind(job_id)
        .fetch_one(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("queue position: {e}")))?;
        Some((queue_pos.0 + 1) as i32)
    } else {
        None
    };

    Ok(CreateJobResponse {
        job_id: job_id.to_string(),
        status: initial_status.to_string(),
        queue_position,
    })
}
