use serde::{Deserialize, Serialize};

/// Referência de pacote ZIP no S3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageRef {
    pub key: String,
    pub md5_zip: String,
    pub bytes: i64,
}

/// Referência a peso de modelo (checkpoint ou text encoder).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeightRef {
    pub s3_key: String,
    pub md5: String,
}

/// Referência legada de pesos para fine-tune.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeightsRef {
    pub s3_key: String,
    pub md5: String,
}

/// Referência de imagem inicial para pipelines img2img.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitImageRef {
    pub s3_key: String,
    pub md5: Option<String>,
}

/// Referência de adaptador LoRA para staging multi-LoRA.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoraRefStage {
    pub s3_key: String,
    pub md5: String,
    pub scale: f64,
}

fn default_mode() -> String {
    "train".to_string()
}

/// Payload completo de dispatch enviado do manager ao orchestrator (POST /internal/dispatch).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchRequest {
    pub job_id: String,
    pub engine: String,
    pub image: String,
    pub exec_mode: String,
    #[serde(default)]
    pub package_ref: Option<PackageRef>,
    pub config_yaml: Option<String>,
    pub dataset_version_id: Option<String>,
    pub workdir: String,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub weights_ref: Option<WeightsRef>,
    #[serde(default)]
    pub loras: Vec<LoraRefStage>,
    #[serde(default)]
    pub custom_checkpoint: Option<WeightRef>,
    #[serde(default)]
    pub text_encoder: Option<WeightRef>,
    #[serde(default)]
    pub init_image_ref: Option<InitImageRef>,
    #[serde(default)]
    pub control_package_ref: Option<PackageRef>,
}
