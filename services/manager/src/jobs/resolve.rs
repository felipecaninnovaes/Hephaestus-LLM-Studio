//! Resolução de pesos, LoRAs, checkpoints, text encoders e imagens de entrada (MM-12).

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::ManagerError;

/// Referência de pesos para fine-tune (ADR-0012 D5 — snake_case interno).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightsRef {
    pub s3_key: String,
    pub md5: String,
}

/// Referência resolvida de LoRA para o dispatch (D3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedLora {
    pub s3_key: String,
    pub md5: String,
    pub scale: f64,
}

/// Referência resolvida de checkpoint custom para o dispatch (D4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedCheckpoint {
    pub s3_key: String,
    pub md5: String,
}

/// Referência resolvida de text encoder custom para o dispatch (fatia feat/pesos-custom-flux2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedTextEncoder {
    pub s3_key: String,
    pub md5: String,
}

/// Referência resolvida de imagem inicial para img2img (S4 — feat/img2img).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedInitImage {
    pub s3_key: String,
    pub md5: Option<String>,
}

/// Resolve `weights_id` buscando na tabela `models`, com fallback para `job_artifacts`.
/// Retorna `(WeightsRef, String)` onde a segunda string é o modelo resolvido (ou variante).
pub async fn resolve_weights_ref(
    pool: &PgPool,
    weights_id: Uuid,
    req_engine: &str,
    req_mode: &str,
    req_model: &str,
) -> Result<(WeightsRef, String), ManagerError> {
    let mut resolved_model = req_model.to_string();

    let row: Option<(String, String, String, Option<String>)> =
        match sqlx::query_as("SELECT s3_key, hash, engine, model FROM models WHERE id = $1")
            .bind(weights_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("resolve weights from models: {e}")))?
        {
            Some(r) => Some(r),
            None => {
                // Fallback: busca em job_artifacts (ex.: checkpoints periódicos por época ou modelos intermediários)
                let art_row: Option<(Uuid, String, String, String, String)> = sqlx::query_as(
                    "SELECT a.job_id, a.path, a.md5, j.engine, j.model \
                     FROM job_artifacts a \
                     JOIN jobs j ON j.id = a.job_id \
                     WHERE a.id = $1 AND a.kind IN ('checkpoint', 'model')",
                )
                .bind(weights_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| {
                    ManagerError::Internal(format!("resolve weights from artifacts: {e}"))
                })?;

                art_row.map(|(job_id, path, md5, engine, model)| {
                    (
                        format!("artifacts/{job_id}/{path}"),
                        md5,
                        engine,
                        Some(model),
                    )
                })
            }
        };

    match row {
        None => Err(ManagerError::NotFound),
        Some((s3_key, hash, engine, variant)) => {
            if engine != "yolo" && engine != "world" && engine != "diffusion" {
                return Err(ManagerError::InvalidRequest(format!(
                    "weights engine must be 'yolo', 'world', or 'diffusion', got '{engine}'"
                )));
            }
            // Defesa: fine-tune yolo (mode=train) NÃO aceita pesos world nem diffusion.
            if req_engine == "yolo" && req_mode == "train" && engine != "yolo" {
                return Err(ManagerError::InvalidRequest(format!(
                    "fine-tune weights engine must be 'yolo', got '{engine}'"
                )));
            }
            // Defesa: treino de difusão exige pesos de difusão.
            if req_engine == "diffusion" && engine != "diffusion" {
                return Err(ManagerError::InvalidRequest(format!(
                    "diffusion weights engine must be 'diffusion', got '{engine}'"
                )));
            }

            let weights_ref = WeightsRef { s3_key, md5: hash };

            // ADR-0012 D5/I.2b: predict com variant → jobs.model = variante (ex.: yolo11m).
            if req_model == "predict" {
                if let Some(v) = variant.as_deref() {
                    resolved_model = v.to_string();
                }
            }
            // ADR-0014 D5: autotracker com engine=world → jobs.model = variante | "world".
            if engine == "world" && req_model != "predict" {
                if let Some(v) = variant.as_deref() {
                    resolved_model = v.to_string();
                } else {
                    resolved_model = "world".into();
                }
            }

            Ok((weights_ref, resolved_model))
        }
    }
}

/// Valida o orchestrator_hint garantindo que o nó existe e está online.
pub async fn resolve_orchestrator_hint(
    pool: &PgPool,
    hint_str: &str,
) -> Result<Uuid, ManagerError> {
    let hint_uuid = Uuid::parse_str(hint_str).map_err(|_| {
        ManagerError::InvalidRequest("orchestrator_hint must be a valid UUID".into())
    })?;

    let orch_status: Option<(String,)> =
        sqlx::query_as("SELECT status FROM orchestrators WHERE id = $1")
            .bind(hint_uuid)
            .fetch_optional(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("check orchestrator_hint: {e}")))?;

    match orch_status {
        None => Err(ManagerError::NotFound),
        Some((status,)) if status != "online" => Err(ManagerError::InvalidRequest(
            "nó de execução indisponível (offline ou revogado) — escolha outro ou Automático"
                .into(),
        )),
        _ => Ok(hint_uuid),
    }
}

/// Valida até 4 LoRAs e extrai referências com s3_key, md5 e scale.
pub async fn resolve_loras(
    pool: &PgPool,
    loras_arr: &[serde_json::Value],
) -> Result<Vec<ResolvedLora>, ManagerError> {
    if loras_arr.len() > 4 {
        return Err(ManagerError::InvalidRequest(
            "loras must have at most 4 items".into(),
        ));
    }
    let mut resolved_loras: Vec<ResolvedLora> = Vec::with_capacity(loras_arr.len());
    for (i, lora_entry) in loras_arr.iter().enumerate() {
        let model_id_str = lora_entry
            .get("modelId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ManagerError::InvalidRequest(format!("loras[{i}].modelId is required"))
            })?;
        let model_uuid = Uuid::parse_str(model_id_str).map_err(|_| {
            ManagerError::InvalidRequest(format!("loras[{i}].modelId must be a valid UUID"))
        })?;
        let scale = lora_entry
            .get("scale")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0);

        let row: Option<(String, String, Option<String>, Option<String>)> =
            sqlx::query_as("SELECT s3_key, hash, kind, arch FROM models WHERE id = $1")
                .bind(model_uuid)
                .fetch_optional(pool)
                .await
                .map_err(|e| ManagerError::Internal(format!("resolve lora model {i}: {e}")))?;

        match row {
            None => {
                return Err(ManagerError::InvalidRequest(format!(
                    "lora modelId at index {i} not found"
                )));
            }
            Some((s3_key, hash, kind, _arch)) => {
                if kind.as_deref() != Some("lora") {
                    return Err(ManagerError::InvalidRequest(format!(
                        "lora modelId at index {i} must have kind='lora', got {:?}",
                        kind
                    )));
                }
                resolved_loras.push(ResolvedLora {
                    s3_key,
                    md5: hash,
                    scale,
                });
            }
        }
    }
    Ok(resolved_loras)
}

/// Valida checkpoint customizado para difusão (SDXL, SD15, Flux, Qwen).
pub async fn resolve_custom_checkpoint(
    pool: &PgPool,
    custom_id_str: &str,
) -> Result<ResolvedCheckpoint, ManagerError> {
    let custom_uuid = Uuid::parse_str(custom_id_str)
        .map_err(|_| ManagerError::InvalidRequest("customModelId must be a valid UUID".into()))?;

    let row: Option<(String, String, Option<String>, Option<String>)> =
        sqlx::query_as("SELECT s3_key, hash, kind, arch FROM models WHERE id = $1")
            .bind(custom_uuid)
            .fetch_optional(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("resolve custom model: {e}")))?;

    match row {
        None => Err(ManagerError::InvalidRequest(
            "customModelId not found".into(),
        )),
        Some((s3_key, hash, kind, arch)) => {
            if kind.as_deref() != Some("checkpoint") {
                return Err(ManagerError::InvalidRequest(format!(
                    "customModelId must have kind='checkpoint', got {:?}",
                    kind
                )));
            }
            let arch_val = arch.as_deref().unwrap_or("");
            let arch_norm = if arch_val == "flux" {
                "flux-2-klein-4b"
            } else {
                arch_val
            };
            if !matches!(
                arch_norm,
                "sdxl" | "sd15" | "flux-2-klein-4b" | "qwen-image-2.1"
            ) {
                return Err(ManagerError::InvalidRequest(format!(
                    "customModelId arch must be 'sdxl', 'sd15', 'flux-2-klein-4b' or 'qwen-image-2.1', got '{arch_val}'"
                )));
            }
            Ok(ResolvedCheckpoint { s3_key, md5: hash })
        }
    }
}

/// Valida text encoder customizado para difusão (kind='text_encoder', arch='flux-2-klein-4b').
pub async fn resolve_text_encoder(
    pool: &PgPool,
    encoder_id_str: &str,
    params: &serde_json::Value,
) -> Result<ResolvedTextEncoder, ManagerError> {
    let encoder_uuid = Uuid::parse_str(encoder_id_str).map_err(|_| {
        ManagerError::InvalidRequest("textEncoderModelId must be a valid UUID".into())
    })?;

    let row: Option<(String, String, Option<String>, Option<String>)> =
        sqlx::query_as("SELECT s3_key, hash, kind, arch FROM models WHERE id = $1")
            .bind(encoder_uuid)
            .fetch_optional(pool)
            .await
            .map_err(|e| ManagerError::Internal(format!("resolve text encoder: {e}")))?;

    match row {
        None => Err(ManagerError::InvalidRequest(
            "textEncoderModelId not found".into(),
        )),
        Some((s3_key, hash, kind, _arch)) => {
            if kind.as_deref() != Some("text_encoder") {
                return Err(ManagerError::InvalidRequest(format!(
                    "textEncoderModelId must have kind='text_encoder', got {:?}",
                    kind
                )));
            }

            let effective = params
                .get("baseModel")
                .or_else(|| params.get("base_model"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let effective_norm = if effective == "flux" {
                "flux-2-klein-4b"
            } else {
                effective
            };
            if effective_norm != "flux-2-klein-4b" {
                return Err(ManagerError::InvalidRequest(
                    "textEncoderModelId requires arch 'flux-2-klein-4b'".into(),
                ));
            }

            Ok(ResolvedTextEncoder { s3_key, md5: hash })
        }
    }
}

/// Valida initImageId ou initGenerationId (img2img).
pub async fn resolve_init_image(
    pool: &PgPool,
    params: &serde_json::Value,
) -> Result<Option<ResolvedInitImage>, ManagerError> {
    if params.get("initImageId").and_then(|v| v.as_str()).is_some()
        && params
            .get("initGenerationId")
            .and_then(|v| v.as_str())
            .is_some()
    {
        return Err(ManagerError::InvalidRequest(
            "use either initImageId or initGenerationId, not both".into(),
        ));
    }

    if let Some(init_id_str) = params.get("initImageId").and_then(|v| v.as_str()) {
        let init_uuid = Uuid::parse_str(init_id_str)
            .map_err(|_| ManagerError::InvalidRequest("initImageId must be a valid UUID".into()))?;

        let row: Option<(String, String)> =
            sqlx::query_as("SELECT s3_key, md5 FROM generation_inputs WHERE id = $1")
                .bind(init_uuid)
                .fetch_optional(pool)
                .await
                .map_err(|e| ManagerError::Internal(format!("resolve init image: {e}")))?;

        match row {
            None => Err(ManagerError::InvalidRequest("initImageId not found".into())),
            Some((s3_key, md5)) => {
                let _ = sqlx::query("UPDATE generation_inputs SET used_at = now() WHERE id = $1")
                    .bind(init_uuid)
                    .execute(pool)
                    .await;
                Ok(Some(ResolvedInitImage {
                    s3_key,
                    md5: Some(md5),
                }))
            }
        }
    } else if let Some(gen_id_str) = params.get("initGenerationId").and_then(|v| v.as_str()) {
        let gen_uuid = Uuid::parse_str(gen_id_str).map_err(|_| {
            ManagerError::InvalidRequest("initGenerationId must be a valid UUID".into())
        })?;

        let row: Option<(String,)> =
            sqlx::query_as("SELECT s3_key FROM generations WHERE id = $1 AND deleted_at IS NULL")
                .bind(gen_uuid)
                .fetch_optional(pool)
                .await
                .map_err(|e| ManagerError::Internal(format!("resolve init generation: {e}")))?;

        match row {
            None => Err(ManagerError::InvalidRequest(
                "initGenerationId not found".into(),
            )),
            Some((s3_key,)) => Ok(Some(ResolvedInitImage { s3_key, md5: None })),
        }
    } else {
        Ok(None)
    }
}
