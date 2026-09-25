//! Montagem do payload de dispatch e resolução de imagens Docker (MM-14).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Resolução da imagem do container Docker para o job.
///
/// Suporta:
/// - `DIFFUSION_TRAINER_IMAGE` (prioridade máxima via env).
/// - Fallback herdando tag `:gpu` do trainer base (`hephaestus/trainer-difusao:gpu`).
/// - Mapeamento automático de `trainer-yolo` para `trainer-difusao`.
/// - `TRAINER_IMAGE` caso a imagem base seja vazia.
pub fn resolve_diffusion_image(image: &str) -> String {
    let env_diff = std::env::var("DIFFUSION_TRAINER_IMAGE").unwrap_or_default();
    if !env_diff.is_empty() {
        return env_diff;
    }
    let base_image = if image.is_empty() {
        std::env::var("TRAINER_IMAGE").unwrap_or_default()
    } else {
        image.to_string()
    };
    if base_image.ends_with(":gpu") || base_image.contains(":gpu") {
        "hephaestus/trainer-difusao:gpu".to_string()
    } else if base_image.contains("trainer-yolo") {
        base_image.replace("trainer-yolo", "trainer-difusao")
    } else if !base_image.is_empty() {
        base_image
    } else {
        "hephaestus/trainer-difusao:local".to_string()
    }
}

/// DTO fortemente tipado em snake_case para envio ao orquestrador via POST /internal/dispatch.
///
/// Invariante #5:
/// - Todas as chaves em snake_case.
/// - Campos opcionais sem valor são omitidos da serialização (`skip_serializing_if`).
/// - Preserva `init_image_ref.md5` como `Option<String>` (podendo ser `null` explicitamente).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DispatchPayload {
    pub job_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub engine: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub mode: String,
    pub image: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_ref: Option<String>,
    pub exec_mode: String,
    pub workdir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_yaml: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset_version_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_ref: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weights_ref: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loras: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_checkpoint: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_encoder: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_encoder_ref: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub init_image_ref: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control_package_ref: Option<serde_json::Value>,
}

/// Parâmetros de entrada para montagem do payload.
#[derive(Debug)]
pub struct BuildPayloadInput<'a> {
    pub job_id: Uuid,
    pub kind: Option<&'a str>,
    pub engine: &'a str,
    pub model: &'a str,
    pub mode: &'a str,
    pub dataset_id: Option<Uuid>,
    pub params: Option<&'a serde_json::Value>,
    pub config_yaml: Option<&'a str>,
    pub exec_mode: &'a str,
    pub orch_workdir: &'a str,
    pub image: &'a str,
}

/// Constrói o `DispatchPayload` tipado a partir dos metadados e parâmetros do job.
pub fn build_dispatch_payload(input: BuildPayloadInput<'_>) -> DispatchPayload {
    let package_ref = input
        .params
        .and_then(|p| p.get("package_ref"))
        .filter(|p| !p.is_null() && p.get("key").is_some())
        .cloned();

    let dataset_version_id = input
        .params
        .and_then(|p| p.get("package_ref"))
        .and_then(|pr| pr.get("version_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Extrai weights_ref do params se presente (ADR-0012 D5/I.2b).
    let weights_ref = input.params.and_then(|p| p.get("weights_ref")).cloned();

    // Extrai loras resolvidos do params (D3 — ADR-0023).
    let resolved_loras = input
        .params
        .and_then(|p| p.get("loras"))
        .cloned()
        .filter(|v| v.as_array().is_some_and(|a| !a.is_empty()));

    // Extrai custom_checkpoint resolvido do params (D4 — ADR-0023).
    let custom_checkpoint = input
        .params
        .and_then(|p| p.get("custom_checkpoint"))
        .cloned();

    // Extrai init_image_ref resolvido do params (img2img — S4 feat/img2img).
    let init_image_ref = input.params.and_then(|p| p.get("init_image_ref")).cloned();

    // Extrai text_encoder_ref resolvido do params (fatia feat/pesos-custom-flux2).
    let text_encoder_ref = input
        .params
        .and_then(|p| p.get("text_encoder_ref"))
        .cloned();

    // Extrai control_package_ref resolvido do params (Wave 2 — RD-020).
    let control_package_ref = input
        .params
        .and_then(|p| p.get("control_package_ref"))
        .cloned();

    // Resolução de imagem de container.
    let job_image = match input.engine {
        "diffusion" => resolve_diffusion_image(input.image),
        _ => input.image.to_string(),
    };

    DispatchPayload {
        job_id: input.job_id.to_string(),
        kind: input.kind.map(|k| k.to_string()),
        engine: input.engine.to_string(),
        model: Some(input.model.to_string()),
        mode: input.mode.to_string(),
        image: job_image.clone(),
        image_ref: Some(job_image),
        exec_mode: input.exec_mode.to_string(),
        workdir: input.orch_workdir.to_string(),
        config_yaml: input.config_yaml.map(|c| c.to_string()),
        dataset_id: input.dataset_id.map(|d| d.to_string()),
        dataset_version_id,
        package_ref,
        params: input.params.cloned(),
        weights_ref,
        loras: resolved_loras,
        custom_checkpoint,
        text_encoder: text_encoder_ref.clone(),
        text_encoder_ref,
        init_image_ref,
        control_package_ref,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        match ENV_LOCK.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    #[test]
    fn resolve_diffusion_image_env_local_vence_sobre_gpu() {
        let _guard = lock_env();
        std::env::set_var(
            "DIFFUSION_TRAINER_IMAGE",
            "hephaestus/trainer-difusao:local",
        );
        let result = resolve_diffusion_image("hephaestus/trainer-yolo:gpu");
        assert_eq!(result, "hephaestus/trainer-difusao:local");
        std::env::remove_var("DIFFUSION_TRAINER_IMAGE");
    }

    #[test]
    fn resolve_diffusion_image_sem_env_herda_gpu() {
        let _guard = lock_env();
        std::env::remove_var("DIFFUSION_TRAINER_IMAGE");
        let result = resolve_diffusion_image("hephaestus/trainer-yolo:gpu");
        assert_eq!(result, "hephaestus/trainer-difusao:gpu");
    }

    #[test]
    fn resolve_diffusion_image_sem_env_herda_local() {
        let _guard = lock_env();
        std::env::remove_var("DIFFUSION_TRAINER_IMAGE");
        let result = resolve_diffusion_image("hephaestus/trainer-yolo:local");
        assert_eq!(result, "hephaestus/trainer-difusao:local");
    }

    #[test]
    fn resolve_diffusion_image_env_custom() {
        let _guard = lock_env();
        std::env::set_var("DIFFUSION_TRAINER_IMAGE", "meu-registry/exemplo:tag");
        let result = resolve_diffusion_image("hephaestus/trainer-yolo:gpu");
        assert_eq!(result, "meu-registry/exemplo:tag");
        std::env::remove_var("DIFFUSION_TRAINER_IMAGE");
    }

    #[test]
    fn resolve_diffusion_image_trainer_image_env_fallback() {
        let _guard = lock_env();
        std::env::remove_var("DIFFUSION_TRAINER_IMAGE");
        std::env::set_var("TRAINER_IMAGE", "hephaestus/trainer-yolo:gpu");
        let result = resolve_diffusion_image("");
        assert_eq!(result, "hephaestus/trainer-difusao:gpu");
        std::env::remove_var("TRAINER_IMAGE");
    }

    #[test]
    fn payload_serialization_omits_none_fields() {
        let input = BuildPayloadInput {
            job_id: Uuid::new_v4(),
            kind: Some("train"),
            engine: "yolo",
            model: "yolo11n",
            mode: "train",
            dataset_id: None,
            params: None,
            config_yaml: Some("epochs: 10"),
            exec_mode: "docker",
            orch_workdir: "/data",
            image: "hephaestus/trainer-yolo:local",
        };

        let payload = build_dispatch_payload(input);
        let val = serde_json::to_value(&payload).unwrap();

        assert_eq!(val["engine"], "yolo");
        assert_eq!(val["mode"], "train");
        assert!(val.get("weights_ref").is_none());
        assert!(val.get("loras").is_none());
        assert!(val.get("custom_checkpoint").is_none());
        assert!(val.get("init_image_ref").is_none());
        assert!(val.get("text_encoder").is_none());
    }

    #[test]
    fn payload_serialization_preserves_null_md5_in_init_image_ref() {
        let params = serde_json::json!({
            "init_image_ref": {
                "s3_key": "artifacts/abc/gen.png",
                "md5": null
            }
        });

        let input = BuildPayloadInput {
            job_id: Uuid::new_v4(),
            kind: Some("diffusion_generate"),
            engine: "diffusion",
            model: "flux2",
            mode: "generate",
            dataset_id: None,
            params: Some(&params),
            config_yaml: None,
            exec_mode: "docker",
            orch_workdir: "/data",
            image: "hephaestus/trainer-difusao:local",
        };

        let payload = build_dispatch_payload(input);
        let val = serde_json::to_value(&payload).unwrap();

        assert_eq!(val["init_image_ref"]["s3_key"], "artifacts/abc/gen.png");
        assert!(val["init_image_ref"]["md5"].is_null());
    }
}
