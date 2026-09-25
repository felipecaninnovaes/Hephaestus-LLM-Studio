//! Handlers de submissão de jobs de treino e inferência.

use super::helpers::{dataset_not_ready, invalid_request, not_found, queue_unavailable};
use super::types::SubmitJobResponse;
use crate::error::err;
use crate::jobs::manager_client::ManagerError;
use crate::jobs::models::{self, AutotrackerJobRequest, PredictJobRequest, YoloJobRequest};
use crate::state::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

// ---------------------------------------------------------------------------
// POST /api/jobs/yolo — criação de job de treino YOLO (F4.2b)
// ---------------------------------------------------------------------------

/// POST /api/jobs/yolo — submete job de treino YOLO (ADR-0007 D7, ADR-0025 D0).
///
/// Status: 202 | 400 `invalid_request` | 401 | 404 `not_found` |
/// 409 `dataset_not_ready` | 503 `queue_unavailable`.
///
/// Fluxo (aceite <1s): valida body → dataset existe? → dataset pronto? →
/// fingerprint (só SQL) → `create_job` em modo `preparing` → 202; o
/// empacotamento pesado roda em background (`jobs::prepare`).
pub async fn submit_yolo_job(
    State(state): State<AppState>,
    body: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    // 1. Parse body.
    let raw = match body {
        Ok(b) => b,
        Err(_) => return invalid_request(),
    };
    let req: YoloJobRequest = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 2. Validação pura (models.rs).
    let req = match models::validate_yolo_request(req) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 3. Parse dataset_id — não-UUID ⇒ 404 (D8).
    let ds_id: uuid::Uuid = match req.dataset_id.parse() {
        Ok(v) => v,
        Err(_) => return not_found(),
    };

    // 4. Dataset existe?
    let ds_exists: bool =
        match sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM datasets WHERE id = $1)")
            .bind(ds_id)
            .fetch_one(&state.pool)
            .await
        {
            Ok(b) => b,
            Err(_) => {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                )
            }
        };
    if !ds_exists {
        return not_found();
    }

    // 5. Dataset pronto? (D7 :364-365 — D8 :334-335)
    //    409 `dataset_not_ready`: category ≠ 'yolo' OU 0 classes OU 0 imagens ativas.
    let readiness: Option<(String, i64, i64)> = match sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT d.category, \
         (SELECT count(*) FROM classes WHERE dataset_id = d.id), \
         (SELECT count(*) FROM images WHERE dataset_id = d.id AND deleted_at IS NULL) \
         FROM datasets d WHERE d.id = $1",
    )
    .bind(ds_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };
    match readiness {
        None => return not_found(),
        Some((category, class_count, image_count)) => {
            if category != "yolo" || class_count == 0 || image_count == 0 {
                return dataset_not_ready();
            }
        }
    }

    // 6. Fingerprint do dataset (só SQL barato, sem S3 — ADR-0025 D0/D1).
    let fingerprint =
        match crate::jobs::prepare::fingerprint_for_dataset(&state.pool, ds_id, None, "yolo", "")
            .await
        {
            Ok(f) => f,
            Err(_) => {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                )
            }
        };

    // 7. Gera config.yaml (pura, barata; o job_id embutido é decorativo como
    //    antes — o manager aloca o id real no create_job).
    let config_yaml = models::generate_config_yaml(&uuid::Uuid::new_v4().to_string(), &req);

    // 8. Body ao manager em modo preparing (package_ref null; o accept injeta
    //    params.prepare — D7 :237-240, snake_case interno).
    //    vram_min_gb = null na v1 (decisão registrada: política VRAM real entra
    //    com GPU; R3: vram_min gravado mas não bloqueante no mock).
    //    D5: weights_id repassado quando presente (manager resolve → weights_ref).
    let mut manager_body = serde_json::json!({
        "kind": "yolo_train",
        "engine": "yolo",
        "model": req.model.clone(),
        "mode": "train",
        "dataset_id": ds_id.to_string(),
        "config_yaml": config_yaml,
        "params": {},
        "vram_min_gb": null,
    });
    // D5: insere weights_id no body quando presente.
    if let Some(ref weights_id) = req.weights {
        manager_body["weights_id"] = serde_json::json!(weights_id);
    }
    // ADR-0015 D2: insere orchestrator_hint no body quando presente.
    if let Some(ref orch_id) = req.orchestrator_id {
        manager_body["orchestrator_hint"] = serde_json::json!(orch_id);
    }
    // ADR-0022 D1: insere output_name nos params quando presente.
    if let Some(ref out_name) = req.output_name {
        manager_body["params"]["output_name"] = serde_json::json!(out_name);
    }

    // 9. Aceite assíncrono: dedupe → create → insert → spawn → 202.
    let spec = crate::jobs::prepare::PrepareSpec {
        kind: "yolo_train".to_string(),
        dataset_id: ds_id,
        resolved_image_ids: None,
        fingerprint,
        engine: "yolo".to_string(),
        trigger_word: None,
        params: serde_json::json!({
            "model": req.model,
            "epochs": req.epochs,
            "batch": req.batch,
            "imgsz": req.imgsz,
            "lr0": req.lr0,
            "optimizer": req.optimizer,
            "augment": { "mosaic": req.augment.mosaic, "mixupFlip": req.augment.mixup_flip },
            "seed": req.seed,
            "weights": req.weights,
            "orchestratorId": req.orchestrator_id,
            "outputName": req.output_name,
        }),
    };
    crate::jobs::prepare::accept_job_preparing(&state, spec, manager_body).await
}

// ---------------------------------------------------------------------------
// POST /api/jobs/autotracker — submit job de autotrack (ADR-0008 D0/D3)
// ---------------------------------------------------------------------------

/// POST /api/jobs/autotracker — cria job de autotrack (ADR-0008 D0/D3).
///
/// Status: 202 | 400 `invalid_request` | 401 | 404 `not_found` |
/// 409 `dataset_not_ready` | 503 `queue_unavailable`.
pub async fn submit_autotracker_job(
    State(state): State<AppState>,
    body: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    // 1. Parse body.
    let raw = match body {
        Ok(b) => b,
        Err(_) => return invalid_request(),
    };
    let req: AutotrackerJobRequest = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 2. Validação pura (models.rs) — inclui UUID check do modelId.
    let req = match models::validate_autotrack_request(req) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 3. Parse dataset_id — não-UUID ⇒ 404 (D8).
    let ds_id: uuid::Uuid = match req.dataset_id.parse() {
        Ok(v) => v,
        Err(_) => return not_found(),
    };

    // 4. Dataset existe?
    let ds_exists: bool =
        match sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM datasets WHERE id = $1)")
            .bind(ds_id)
            .fetch_one(&state.pool)
            .await
        {
            Ok(b) => b,
            Err(_) => {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                )
            }
        };
    if !ds_exists {
        return not_found();
    }

    // 5. Dataset pronto? (ADR-0008 D3)
    //    409 `dataset_not_ready`: category ≠ 'yolo' OU 0 classes OU 0 imagens ativas.
    let readiness: Option<(String, i64, i64)> = match sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT d.category, \
         (SELECT count(*) FROM classes WHERE dataset_id = d.id), \
         (SELECT count(*) FROM images WHERE dataset_id = d.id AND deleted_at IS NULL) \
         FROM datasets d WHERE d.id = $1",
    )
    .bind(ds_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };
    match readiness {
        None => return not_found(),
        Some((category, class_count, image_count)) => {
            if category != "yolo" || class_count == 0 || image_count == 0 {
                return dataset_not_ready();
            }
        }
    }

    // 6. Fingerprint do dataset (só SQL barato, sem S3 — ADR-0025 D0/D1).
    let fingerprint = match crate::jobs::prepare::fingerprint_for_dataset(
        &state.pool,
        ds_id,
        None,
        "autotracker",
        "",
    )
    .await
    {
        Ok(f) => f,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };

    // 7. Gera config.yaml (pura, barata).
    let config_yaml =
        models::generate_autotrack_config_yaml(&uuid::Uuid::new_v4().to_string(), &req);

    // 8. Body ao manager em modo preparing (ADR-0008 D3, snake_case interno;
    //    ADR-0014 D6 — weights_id; package_ref null, prepare injetado no accept).
    let mut manager_body = serde_json::json!({
        "kind": "autotracker",
        "engine": "autotracker",
        "model": req.model.clone(),
        "mode": "autotrack",
        "dataset_id": ds_id.to_string(),
        "config_yaml": config_yaml,
        "params": {
            "model": req.model.clone(),
            "conf": req.conf,
        },
        "vram_min_gb": null,
    });
    // ADR-0014 D2/D6: modelId presente → weights_id (manager resolve weights_ref).
    if let Some(ref model_id) = req.model_id {
        manager_body["weights_id"] = serde_json::json!(model_id);
    }
    // ADR-0015 D2: insere orchestrator_hint no body quando presente.
    if let Some(ref orch_id) = req.orchestrator_id {
        manager_body["orchestrator_hint"] = serde_json::json!(orch_id);
    }

    // 9. Aceite assíncrono: dedupe → create → insert → spawn → 202.
    //    Mapeamento R6 preservado (NotFound→404, InvalidRequest→400,
    //    Unavailable→503) dentro do accept.
    let spec = crate::jobs::prepare::PrepareSpec {
        kind: "autotracker".to_string(),
        dataset_id: ds_id,
        resolved_image_ids: None,
        fingerprint,
        engine: "autotracker".to_string(),
        trigger_word: None,
        params: serde_json::json!({
            "model": req.model,
            "conf": req.conf,
            "modelId": req.model_id,
            "orchestratorId": req.orchestrator_id,
        }),
    };
    crate::jobs::prepare::accept_job_preparing(&state, spec, manager_body).await
}

// ---------------------------------------------------------------------------
// POST /api/jobs/autolabel — submit job de autolabel (ADR-0016 D0)
// ---------------------------------------------------------------------------

/// POST /api/jobs/autolabel — cria job de autolabel (ADR-0016 D0).
///
/// Status: 202 | 400 `invalid_request` | 401 | 404 `not_found` |
/// 409 `dataset_not_ready` | 503 `queue_unavailable`.
pub async fn submit_autolabel_job(
    State(state): State<AppState>,
    body: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    // 1. Parse body.
    let raw = match body {
        Ok(b) => b,
        Err(_) => return invalid_request(),
    };
    let req: models::AutolabelJobRequest = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 2. Validação pura.
    let mut req = match models::validate_autolabel_request(req) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 3. Parse dataset_id — não-UUID ⇒ 404.
    let ds_id: uuid::Uuid = match req.dataset_id.parse() {
        Ok(v) => v,
        Err(_) => return not_found(),
    };

    // 4. Dataset existe e está pronto?
    // ADR-0016 D0: Requer dataset com ao menos 1 imagem ativa (images_count > 0).
    let image_count: Option<i64> = match sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM images WHERE dataset_id = $1 AND deleted_at IS NULL",
    )
    .bind(ds_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(c) => c,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };

    let ds_exists: bool =
        match sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM datasets WHERE id = $1)")
            .bind(ds_id)
            .fetch_one(&state.pool)
            .await
        {
            Ok(b) => b,
            Err(_) => {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                )
            }
        };
    if !ds_exists {
        return not_found();
    }

    let count = image_count.unwrap_or(0);
    if count == 0 {
        return dataset_not_ready();
    }

    // 4.1. Resolução seletiva de imagens (filter_class_id e/ou image_ids)
    let mut resolved_image_ids: Option<Vec<uuid::Uuid>> = None;

    if let Some(ref fc_str) = req.filter_class_id {
        let class_id = match uuid::Uuid::parse_str(fc_str) {
            Ok(u) => u,
            Err(_) => return invalid_request(),
        };

        let class_info: Option<(uuid::Uuid, String)> =
            match sqlx::query_as("SELECT id, name FROM classes WHERE id = $1 AND dataset_id = $2")
                .bind(class_id)
                .bind(ds_id)
                .fetch_optional(&state.pool)
                .await
            {
                Ok(c) => c,
                Err(_) => {
                    return err(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal",
                        "internal server error",
                    )
                }
            };

        let (_cid, class_name) = match class_info {
            Some(c) => c,
            None => {
                return err(
                    StatusCode::NOT_FOUND,
                    "not_found",
                    "classe informada não encontrada no dataset",
                )
            }
        };

        if let Some(ref mut p) = req.prompt {
            *p = p.replace("{class_name}", &class_name);
        }

        let matched_imgs: Vec<uuid::Uuid> = match sqlx::query_scalar(
            "SELECT DISTINCT b.image_id \
             FROM boxes b \
             JOIN images i ON i.id = b.image_id \
             WHERE i.dataset_id = $1 AND i.deleted_at IS NULL AND b.class_id = $2",
        )
        .bind(ds_id)
        .bind(class_id)
        .fetch_all(&state.pool)
        .await
        {
            Ok(imgs) => imgs,
            Err(_) => {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                )
            }
        };

        if matched_imgs.is_empty() {
            return err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "dataset_not_ready",
                "nenhuma imagem ativa possui anotações para a classe selecionada",
            );
        }

        resolved_image_ids = Some(matched_imgs);
    }

    if let Some(ref ids) = req.image_ids {
        let parsed_ids: Vec<uuid::Uuid> = ids
            .iter()
            .filter_map(|s| uuid::Uuid::parse_str(s).ok())
            .collect();

        if let Some(existing) = resolved_image_ids {
            let set: std::collections::HashSet<uuid::Uuid> = parsed_ids.into_iter().collect();
            let intersected: Vec<uuid::Uuid> =
                existing.into_iter().filter(|id| set.contains(id)).collect();
            if intersected.is_empty() {
                return err(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "dataset_not_ready",
                    "nenhuma das imagens selecionadas possui anotações da classe",
                );
            }
            resolved_image_ids = Some(intersected);
        } else {
            let valid_imgs: Vec<uuid::Uuid> = match sqlx::query_scalar(
                "SELECT id FROM images WHERE dataset_id = $1 AND deleted_at IS NULL AND id = ANY($2)",
            )
            .bind(ds_id)
            .bind(&parsed_ids)
            .fetch_all(&state.pool)
            .await
            {
                Ok(imgs) => imgs,
                Err(_) => {
                    return err(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal",
                        "internal server error",
                    )
                }
            };

            if valid_imgs.is_empty() {
                return err(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "dataset_not_ready",
                    "nenhuma imagem ativa válida encontrada na seleção",
                );
            }
            resolved_image_ids = Some(valid_imgs);
        }
    }

    // 5. Fingerprint do escopo resolvido (só SQL barato, sem S3 — ADR-0025).
    let fingerprint = match crate::jobs::prepare::fingerprint_for_dataset(
        &state.pool,
        ds_id,
        resolved_image_ids.as_deref(),
        "autolabel",
        &req.model,
    )
    .await
    {
        Ok(f) => f,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };

    // 6. Config YAML (pura, barata — carrega apiKey como antes; o tratamento
    //    é idêntico ao legado: viaja no create_job, nunca em GET jobs).
    let config_yaml =
        models::generate_autolabel_config_yaml(&uuid::Uuid::new_v4().to_string(), &req);

    // 7. Body ao manager em modo preparing (package_ref null; prepare no accept).
    let mut manager_body = serde_json::json!({
        "kind": "autolabel",
        "engine": "autolabel",
        "model": req.model,
        "mode": "autolabel",
        "dataset_id": ds_id.to_string(),
        "config_yaml": config_yaml,
        "params": {
            "model": req.model,
            "prompt": req.prompt,
            "filter_class_id": req.filter_class_id,
            "image_ids_count": resolved_image_ids.as_ref().map(|v| v.len()),
        },
        "vram_min_gb": null,
    });

    if let Some(ref orch_id) = req.orchestrator_id {
        manager_body["orchestrator_hint"] = serde_json::json!(orch_id);
    }

    // 8. Aceite assíncrono: dedupe → create → insert → spawn → 202.
    //    SEM apiKey no spec: o worker nunca regenera config (não há endpoint
    //    no manager para config tardia) — persistir o segredo seria risco
    //    gratuito (ver `jobs::prepare`).
    let mut spec_params = crate::jobs::prepare::spec_params_autolabel(
        &req.model,
        req.prompt.as_deref(),
        req.api_base.as_deref(),
        req.openai_model.as_deref(),
        req.reasoning_effort.as_deref(),
        req.filter_class_id.as_deref(),
        manager_body["params"]["image_ids_count"]
            .as_u64()
            .map(|n| n as usize),
    );
    if let Some(ref orch_id) = req.orchestrator_id {
        spec_params["orchestratorId"] = serde_json::json!(orch_id);
    }
    let spec = crate::jobs::prepare::PrepareSpec {
        kind: "autolabel".to_string(),
        dataset_id: ds_id,
        resolved_image_ids,
        fingerprint,
        engine: "autolabel".to_string(),
        trigger_word: None,
        params: spec_params,
    };
    crate::jobs::prepare::accept_job_preparing(&state, spec, manager_body).await
}

// ---------------------------------------------------------------------------
// POST /api/jobs/diffusion — submit job de treino de difusão LoRA (ADR-0018 D1)
// ---------------------------------------------------------------------------

/// POST /api/jobs/diffusion — cria job de treino de difusão LoRA (ADR-0018 D1).
///
/// Status: 202 `SubmitJobResponse` | 400 `invalid_request` | 401 gate | 404 `not_found` |
/// 409 `dataset_not_ready` | 503 `queue_unavailable`.
pub async fn submit_diffusion_job(
    State(state): State<AppState>,
    body: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    // 1. Parse body.
    let raw = match body {
        Ok(b) => b,
        Err(_) => return invalid_request(),
    };
    let req: models::DiffusionJobRequest = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 2. Validação pura.
    let req = match models::validate_diffusion_request(req) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 3. Parse dataset_id — não-UUID ⇒ 404.
    let ds_id: uuid::Uuid = match req.dataset_id.parse() {
        Ok(v) => v,
        Err(_) => return not_found(),
    };

    // 4. Dataset existe e possui imagens ativas?
    let ds_exists: bool =
        match sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM datasets WHERE id = $1)")
            .bind(ds_id)
            .fetch_one(&state.pool)
            .await
        {
            Ok(b) => b,
            Err(_) => {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                )
            }
        };
    if !ds_exists {
        return not_found();
    }

    let image_count: Option<i64> = match sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM images WHERE dataset_id = $1 AND deleted_at IS NULL",
    )
    .bind(ds_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(c) => c,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };

    let count = image_count.unwrap_or(0);
    if count == 0 {
        return dataset_not_ready();
    }

    // 4.1. Control dataset (motor Flux.2): mesma guarda do principal —
    // existência + imagens ativas. (Datasets não têm coluna de dono; a
    // "ownership" aqui é a existência, idêntica à do dataset principal.)
    // A igualdade com o principal já foi rejeitada na validação pura.
    let control_ds_id: Option<uuid::Uuid> = req.control_dataset_id;
    if let Some(control_id) = control_ds_id {
        let control_exists: bool = match sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM datasets WHERE id = $1)",
        )
        .bind(control_id)
        .fetch_one(&state.pool)
        .await
        {
            Ok(b) => b,
            Err(_) => {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                )
            }
        };
        if !control_exists {
            return not_found();
        }
        let control_count: Option<i64> = match sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM images WHERE dataset_id = $1 AND deleted_at IS NULL",
        )
        .bind(control_id)
        .fetch_optional(&state.pool)
        .await
        {
            Ok(c) => c,
            Err(_) => {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                )
            }
        };
        if control_count.unwrap_or(0) == 0 {
            return dataset_not_ready();
        }
    }

    // 5. Fingerprint (só SQL barato, sem S3 — ADR-0025). `trigger_word`
    //    entra no fingerprint porque altera as captions do pacote.
    //    Com control dataset: o fingerprint do control entra no `extra` como
    //    `{principal_fp}:control:{control_fp}` — o dedupe por fingerprint
    //    considera o PAR (principal, control), nunca o principal sozinho.
    let fingerprint = match crate::jobs::prepare::fingerprint_for_dataset(
        &state.pool,
        ds_id,
        None,
        "diffusion",
        req.trigger_word.as_deref().unwrap_or(""),
    )
    .await
    {
        Ok(f) => f,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };
    let fingerprint = match control_ds_id {
        Some(control_id) => {
            let control_fp = match crate::jobs::prepare::fingerprint_for_dataset(
                &state.pool,
                control_id,
                None,
                "diffusion",
                req.trigger_word.as_deref().unwrap_or(""),
            )
            .await
            {
                Ok(f) => f,
                Err(_) => {
                    return err(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal",
                        "internal server error",
                    )
                }
            };
            format!("{fingerprint}:control:{control_fp}")
        }
        None => fingerprint,
    };

    // 5.1. Pesos custom (fatia feat/pesos-custom-flux2): resolve custom_model_id
    //    e text_encoder_model_id no manager (mesmo padrão do generate —
    //    `list_models` + kind/arch). Inexistente ⇒ 404; kind errado ⇒ 400;
    //    encoder em arch ≠ flux-2-klein-4b ⇒ 400. O YAML leva só placeholders
    //    literais (NUNCA id/path real); o manager resolve os refs p/ staging.
    let custom_arch: Option<String> = if let Some(ref custom_id) = req.custom_model_id {
        let models = match state.manager.list_models().await {
            Ok(m) => m,
            Err(_) => return queue_unavailable(),
        };
        let model = match models.iter().find(|m| m.id == *custom_id) {
            Some(m) => m,
            None => return not_found(),
        };
        match model.kind.as_deref() {
            Some("checkpoint") => {}
            _ => {
                return err(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "customModelId must reference a checkpoint model",
                );
            }
        }
        match model.arch.as_deref() {
            Some(arch @ ("sdxl" | "sd15" | "flux" | "flux-2-klein-4b" | "qwen-image-2.1")) => {
                // Normaliza alias legado "flux" → arch canônico do YAML.
                if arch == "flux" {
                    Some("flux-2-klein-4b".to_string())
                } else {
                    Some(arch.to_string())
                }
            }
            Some(_) => {
                return err(
                    StatusCode::BAD_REQUEST,
                    "unsupported_architecture",
                    "custom checkpoint architecture not supported",
                );
            }
            None => {
                return err(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "custom model has no arch metadata",
                );
            }
        }
    } else {
        None
    };
    if let Some(ref encoder_id) = req.text_encoder_model_id {
        let models = match state.manager.list_models().await {
            Ok(m) => m,
            Err(_) => return queue_unavailable(),
        };
        let model = match models.iter().find(|m| m.id == *encoder_id) {
            Some(m) => m,
            None => return not_found(),
        };
        match model.kind.as_deref() {
            Some("text_encoder") => {}
            _ => {
                return err(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "textEncoderModelId must reference a text_encoder model",
                );
            }
        }
        // Arch efetivo do treino: arch do custom ou base_model (default sdxl).
        // Alias legado da UI "flux" ⇒ flux-2-klein-4b (mesma normalização
        // aplicada ao custom_arch acima) — preset FLUX.2 chega como "flux".
        let effective_arch = match custom_arch
            .as_deref()
            .unwrap_or_else(|| req.base_model.as_deref().unwrap_or("sdxl"))
        {
            "flux" => "flux-2-klein-4b",
            other => other,
        };
        if effective_arch != "flux-2-klein-4b" {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "textEncoderModelId requires arch 'flux-2-klein-4b'",
            );
        }
    }
    // Arch efetivo do treino (p/ YAML + VRAM): custom resolvido ou base_model.
    let effective_base: String = custom_arch
        .clone()
        .unwrap_or_else(|| req.base_model.clone().unwrap_or_else(|| "sdxl".to_string()));
    // 6. Config YAML (pura, barata).
    let config_yaml = models::generate_diffusion_config_yaml(
        &uuid::Uuid::new_v4().to_string(),
        &req,
        custom_arch.as_deref(),
    );

    // 6.1. Pacote do control dataset (MVP síncrono): quando controlDatasetId
    //    informado, empacota o control via `build_package_diffusion` AGORA no
    //    request e publica `params.control_package_ref` (snake_case, mesmo
    //    shape do `package_ref` do dispatch: {version_id, key, md5_zip,
    //    bytes} — `md5_zip`/`bytes` vêm direto do build, sem recomputo).
    //    LIMITAÇÃO documentada: packaging síncrono no request (fora do worker
    //    ADR-0025) e SEM reuso por fingerprint — NUNCA reusar fingerprint que
    //    ignore o control dataset. O worker continua empacotando o principal;
    //    o control viaja pronto em `params.control_package_ref`.
    //    `gc_dataset_versions` precisa cobrir `control_package_ref.version_id`
    //    (anti-GC) — fora deste ownership (slice do manager).
    let control_package_ref: Option<serde_json::Value> = match control_ds_id {
        Some(control_id) => {
            match crate::datasets::package::build_package_diffusion(
                &state,
                control_id,
                req.trigger_word.as_deref(),
            )
            .await
            {
                Ok(pkg) => Some(serde_json::json!({
                    "version_id": pkg.version_id,
                    "key": pkg.key,
                    "md5_zip": pkg.md5_zip,
                    "bytes": pkg.bytes,
                })),
                Err(resp) => return resp,
            }
        }
        None => None,
    };

    // 7. VRAM mínima por arch efetivo (ADR-0018 D2 — FLUX.2 Klein 4B requer
    //    ~10 GB; fatia: custom flux-2 custa como flux-2-klein-4b).
    let vram_min = match effective_base.as_str() {
        "sd15" => 8,
        "qwen-image-2.1" => 8,
        "flux" | "flux-2-klein-4b" => 10,
        _ => 12, // sdxl e default
    };

    // 8. Body ao manager em modo preparing (package_ref null; prepare no accept).
    let mut manager_body = serde_json::json!({
        "kind": "diffusion_train",
        "engine": "diffusion",
        "model": effective_base,
        "mode": "train",
        "dataset_id": ds_id.to_string(),
        "config_yaml": config_yaml,
        "params": {
            "datasetId": ds_id.to_string(),
            "baseModel": effective_base,
            "customModelId": req.custom_model_id,
            "textEncoderModelId": req.text_encoder_model_id,
            "triggerWord": req.trigger_word,
            "epochs": req.epochs,
            "batchSize": req.batch_size,
            "learningRate": req.learning_rate,
            "rank": req.rank,
            "alpha": req.alpha,
            "resolution": req.resolution,
            "gradientAccumulationSteps": req.gradient_accumulation_steps,
            "optimizer": req.optimizer,
            "lrScheduler": req.lr_scheduler,
            "lrWarmupSteps": req.lr_warmup_steps,
            "mixedPrecision": req.mixed_precision,
            "quantization": req.quantization,
            "enableBucket": req.enable_bucket,
            "checkpointInterval": req.checkpoint_interval,
            "epochOffset": req.epoch_offset,
            "samplePrompt": req.sample_prompt,
            "sampleInterval": req.sample_interval,
            "sampleSeed": req.sample_seed,
            "weights": req.weights,
            "outputName": req.output_name,
            "controlDatasetId": req.control_dataset_id.map(|u| u.to_string()),
            "cacheTextEmbeddings": req.cache_text_embeddings,
        },
        "vram_min_gb": vram_min,
    });
    if let Some(control_ref) = control_package_ref {
        manager_body["params"]["control_package_ref"] = control_ref;
    }
    // ADR-0015 D2: insere orchestrator_hint no body quando presente.
    if let Some(orch_id) = &req.orchestrator_id {
        manager_body["orchestrator_hint"] = serde_json::json!(orch_id);
    }

    // 9. Aceite assíncrono: dedupe → create → insert → spawn → 202.
    //    NOTA P4a+P4b: `build_package_diffusion` ainda não grava fingerprint
    //    no manifest — o reuso por versão fica para a fusão; o dedupe por
    //    `job_prepares` (30min) já vale.
    let spec = crate::jobs::prepare::PrepareSpec {
        kind: "diffusion_train".to_string(),
        dataset_id: ds_id,
        resolved_image_ids: None,
        fingerprint,
        engine: "diffusion".to_string(),
        trigger_word: req.trigger_word.clone(),
        params: serde_json::json!({
            "baseModel": effective_base,
            "customModelId": req.custom_model_id,
            "textEncoderModelId": req.text_encoder_model_id,
            "triggerWord": req.trigger_word,
            "epochs": req.epochs,
            "batchSize": req.batch_size,
            "learningRate": req.learning_rate,
            "rank": req.rank,
            "alpha": req.alpha,
            "resolution": req.resolution,
            "gradientAccumulationSteps": req.gradient_accumulation_steps,
            "optimizer": req.optimizer,
            "lrScheduler": req.lr_scheduler,
            "lrWarmupSteps": req.lr_warmup_steps,
            "mixedPrecision": req.mixed_precision,
            "quantization": req.quantization,
            "enableBucket": req.enable_bucket,
            "checkpointInterval": req.checkpoint_interval,
            "epochOffset": req.epoch_offset,
            "samplePrompt": req.sample_prompt,
            "sampleInterval": req.sample_interval,
            "sampleSeed": req.sample_seed,
            "weights": req.weights,
            "orchestratorId": req.orchestrator_id,
            "outputName": req.output_name,
            "controlDatasetId": req.control_dataset_id.map(|u| u.to_string()),
            "cacheTextEmbeddings": req.cache_text_embeddings,
        }),
    };
    crate::jobs::prepare::accept_job_preparing(&state, spec, manager_body).await
}

// ---------------------------------------------------------------------------
// POST /api/jobs/diffusion/generate — submit job de geração Text-to-Image (ADR-0020)
// ---------------------------------------------------------------------------

pub async fn submit_diffusion_generate_job(
    State(state): State<AppState>,
    body: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    // 1. Parse body.
    let raw = match body {
        Ok(b) => b,
        Err(_) => return invalid_request(),
    };
    let req: models::DiffusionGenerateJobRequest = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 2. Validação pura (ADR-0023 D2/D3/D4).
    let req = match models::validate_diffusion_generate_request(req) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 3. Se custom_model_id presente → busca modelo no manager para obter arch.
    //    Valida kind=checkpoint e arch ∈ {sdxl, sd15, flux-2-klein-4b}
    //    (fatia feat/pesos-custom-flux2: flux-2 custom via transformer swap).
    let custom_arch: Option<String> = if let Some(custom_id) = &req.custom_model_id {
        let models = match state.manager.list_models().await {
            Ok(m) => m,
            Err(_) => return queue_unavailable(),
        };
        let model = match models.iter().find(|m| m.id == *custom_id) {
            Some(m) => m,
            None => {
                return err(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "customModelId not found",
                );
            }
        };
        // Valida kind=checkpoint (ADR-0023 D4).
        match model.kind.as_deref() {
            Some("checkpoint") => {}
            _ => {
                return err(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "customModelId must reference a checkpoint model",
                );
            }
        }
        // Valida arch ∈ {sdxl, sd15, flux-2-klein-4b} (fatia: +flux-2).
        match model.arch.as_deref() {
            Some(arch @ ("sdxl" | "sd15" | "flux" | "flux-2-klein-4b" | "qwen-image-2.1")) => {
                if arch == "flux" {
                    Some("flux-2-klein-4b".to_string())
                } else {
                    Some(arch.to_string())
                }
            }
            Some(_other) => {
                return err(
                    StatusCode::BAD_REQUEST,
                    "unsupported_architecture",
                    "custom checkpoint architecture not supported (use sdxl, sd15, flux-2-klein-4b or qwen-image-2.1)",
                );
            }
            None => {
                return err(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "custom model has no arch metadata",
                );
            }
        }
    } else {
        None
    };

    // 3.1. Encoder custom (fatia feat/pesos-custom-flux2): resolve
    //    text_encoder_model_id no manager (mesmo fluxo do custom_model_id):
    //    inexistente ⇒ 404; kind≠text_encoder ⇒ 400; arch efetivo
    //    (base_model ou arch do custom) ≠ flux-2-klein-4b ⇒ 400.
    if let Some(encoder_id) = &req.text_encoder_model_id {
        let models = match state.manager.list_models().await {
            Ok(m) => m,
            Err(_) => return queue_unavailable(),
        };
        let model = match models.iter().find(|m| m.id == *encoder_id) {
            Some(m) => m,
            None => return not_found(),
        };
        match model.kind.as_deref() {
            Some("text_encoder") => {}
            _ => {
                return err(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "textEncoderModelId must reference a text_encoder model",
                );
            }
        }
        let effective_arch = custom_arch
            .as_deref()
            .unwrap_or_else(|| req.base_model.as_deref().unwrap_or("flux-2-klein-4b"));
        if effective_arch != "flux-2-klein-4b" {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "textEncoderModelId requires arch 'flux-2-klein-4b'",
            );
        }
    }

    // 4. ID do job e config.yaml (v2 com custom_arch).
    let job_id = uuid::Uuid::new_v4().to_string();
    let config_yaml =
        models::generate_diffusion_generate_config_yaml(&job_id, &req, custom_arch.as_deref());

    // 5. VRAM mínima por (arch_efetiva, quantization) — D8 estendido:
    //    sd15 fixo 6; sdxl e flux: none→16, 6bit→10, 4bit→8, 2bit→7, 8bit→12.
    //    Custom: espelha first-class do arch (sdxl/sd15).
    let effective_arch = custom_arch
        .as_deref()
        .unwrap_or_else(|| req.base_model.as_deref().unwrap_or("flux-2-klein-4b"));
    let vram_min: i32 =
        models::diffusion_generate_vram_min_gb(effective_arch, req.quantization.as_str());

    // 6. Body para o Manager — params camelCase (ADR-0023).
    let loras_json: Vec<serde_json::Value> = req
        .loras
        .iter()
        .map(|l| serde_json::json!({ "modelId": l.model_id, "scale": l.scale }))
        .collect();
    let upscale_json = match &req.upscale {
        Some(up) => serde_json::json!({ "model": up.model, "scale": up.scale }),
        None => serde_json::Value::Null,
    };

    let mut manager_body = serde_json::json!({
        "kind": "diffusion_generate",
        "engine": "diffusion",
        "model": effective_arch,
        "mode": "generate",
        "config_yaml": config_yaml,
        "params": {
            "base_model": effective_arch,
            "prompt": req.prompt,
            "negative_prompt": req.negative_prompt,
            "width": req.width,
            "height": req.height,
            "steps": req.steps,
            "guidance_scale": req.guidance_scale,
            "seed": req.seed,
            "quantization": req.quantization,
            "sampler": req.sampler,
            "upscale": upscale_json,
            "distilled": req.distilled,
            "lora_scale": req.lora_scale,
            "batchSize": req.batch_size,
            "loras": loras_json,
            "customModelId": req.custom_model_id,
            "textEncoderModelId": req.text_encoder_model_id,
        },
        "vram_min_gb": vram_min,
    });

    if let Some(ref w_id) = req.weights {
        manager_body["weights_id"] = serde_json::json!(w_id);
    }
    if let Some(ref orch_id) = req.orchestrator_id {
        manager_body["orchestrator_hint"] = serde_json::json!(orch_id);
    }

    // img2img: encaminha o id que veio (`initImageId` OU `initGenerationId`) e
    // `initStrength` só quando há id — sem default local (ausente ⇒ null;
    // default 0.6 aplicado no config_yaml/engine). Existência/resolução dos
    // ids é do manager (S4) — aqui não há lookup.
    // VRAM: estimativa atual mantida p/ img2img (follow-up: medir overhead do
    // decode/resize da init no nó GPU).
    if let Some(id) = req.init_image_id {
        manager_body["params"]["initImageId"] = serde_json::json!(id.to_string());
    } else if let Some(id) = req.init_generation_id {
        manager_body["params"]["initGenerationId"] = serde_json::json!(id.to_string());
    }
    if req.init_image_id.is_some() || req.init_generation_id.is_some() {
        manager_body["params"]["initStrength"] = match req.init_strength {
            Some(s) => serde_json::json!(s),
            None => serde_json::Value::Null,
        };
    }

    match state.manager.create_job(&manager_body).await {
        Ok(resp) => {
            let body = SubmitJobResponse {
                job_id: resp.job_id,
                status: resp.status,
                queue_position: resp.queue_position,
            };
            (StatusCode::ACCEPTED, Json(body)).into_response()
        }
        Err(ManagerError::NotFound) => not_found(),
        Err(ManagerError::InvalidRequest(_)) => invalid_request(),
        Err(ManagerError::Unavailable(_)) => queue_unavailable(),
        Err(_) => queue_unavailable(),
    }
}

// ---------------------------------------------------------------------------
// POST /api/jobs/predict — submit job de inferência YOLO (ADR-0013 D0/D1/D8)
// ---------------------------------------------------------------------------

/// POST /api/jobs/predict — cria job de predict YOLO (ADR-0013 D0/D1/D8).
///
/// Status: 202 | 400 `invalid_request` | 401 | 404 `not_found` |
/// 409 `dataset_not_ready` | 503 `queue_unavailable`.
///
/// Diferença R6: mapeia `NotFound→404`, `InvalidRequest→400` (padrão abort
/// L825-831), NÃO repete o `Err(_)→503` do submit_yolo_job existente.
pub async fn submit_predict_job(
    State(state): State<AppState>,
    body: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    // 1. Parse body.
    let raw = match body {
        Ok(b) => b,
        Err(_) => return invalid_request(),
    };
    let req: PredictJobRequest = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 2. Validação pura (models.rs).
    let req = match models::validate_predict_request(req) {
        Ok(v) => v,
        Err(_) => return invalid_request(),
    };

    // 3. Parse dataset_id — não-UUID ⇒ 404 (D8).
    let ds_id: uuid::Uuid = match req.dataset_id.parse() {
        Ok(v) => v,
        Err(_) => return not_found(),
    };

    // 4. Dataset existe?
    let ds_exists: bool =
        match sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM datasets WHERE id = $1)")
            .bind(ds_id)
            .fetch_one(&state.pool)
            .await
        {
            Ok(b) => b,
            Err(_) => {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                )
            }
        };
    if !ds_exists {
        return not_found();
    }

    // 5. Dataset pronto? (D8: predict — category=yolo, ≥1 imagem; classes NÃO obrigatórias).
    //    409 `dataset_not_ready`: category ≠ 'yolo' OU 0 imagens ativas.
    let readiness: Option<(String, i64)> = match sqlx::query_as::<_, (String, i64)>(
        "SELECT d.category, \
         (SELECT count(*) FROM images WHERE dataset_id = d.id AND deleted_at IS NULL) \
         FROM datasets d WHERE d.id = $1",
    )
    .bind(ds_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            )
        }
    };
    match readiness {
        None => return not_found(),
        Some((category, image_count)) => {
            if category != "yolo" || image_count == 0 {
                return dataset_not_ready();
            }
        }
    }

    // 6. Fingerprint do dataset (só SQL barato, sem S3 — ADR-0025).
    let fingerprint =
        match crate::jobs::prepare::fingerprint_for_dataset(&state.pool, ds_id, None, "yolo", "")
            .await
        {
            Ok(f) => f,
            Err(_) => {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error",
                )
            }
        };

    // 7. Gera config.yaml (pura, barata).
    let config_yaml = models::generate_predict_config_yaml(&uuid::Uuid::new_v4().to_string(), &req);

    // 8. Body ao manager em modo preparing (D5: kind='yolo_predict',
    //    engine='yolo', mode='predict', model='predict' placeholder,
    //    weights_id=modelId; package_ref null, prepare no accept).
    let mut manager_body = serde_json::json!({
        "kind": "yolo_predict",
        "engine": "yolo",
        "model": "predict",
        "mode": "predict",
        "dataset_id": ds_id.to_string(),
        "config_yaml": config_yaml,
        "params": {
            "conf": req.conf,
        },
        "vram_min_gb": null,
        "weights_id": req.model_id,
    });
    // ADR-0015 D2: insere orchestrator_hint no body quando presente.
    if let Some(ref orch_id) = req.orchestrator_id {
        manager_body["orchestrator_hint"] = serde_json::json!(orch_id);
    }

    // 9. Aceite assíncrono: dedupe → create → insert → spawn → 202.
    //    Mapeamento R6 preservado (NotFound→404, InvalidRequest→400) no accept.
    let spec = crate::jobs::prepare::PrepareSpec {
        kind: "yolo_predict".to_string(),
        dataset_id: ds_id,
        resolved_image_ids: None,
        fingerprint,
        engine: "yolo".to_string(),
        trigger_word: None,
        params: serde_json::json!({
            "modelId": req.model_id,
            "conf": req.conf,
            "orchestratorId": req.orchestrator_id,
        }),
    };
    crate::jobs::prepare::accept_job_preparing(&state, spec, manager_body).await
}
