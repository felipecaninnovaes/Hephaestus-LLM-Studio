//! Validação pura de `POST /api/jobs/yolo` e geração de `config.yaml`.
//!
//! Toda regra aqui é testável sem Postgres nem manager — padrão `models.rs`
//! da casa. `Err(String)` ⇒ o handler responde 400 `invalid_request`.

// ---------------------------------------------------------------------------
// Constantes (ADR-0007 D6)
// ---------------------------------------------------------------------------

const ALLOWED_MODELS: &[&str] = &["yolo11n", "yolo11m", "yolo11x", "yolov9-c", "yolo11-seg"];
const ALLOWED_BATCH: &[u32] = &[8, 16, 32, 64];
const ALLOWED_IMGSZ: &[u32] = &[416, 640, 1024];
const ALLOWED_OPTIMIZER: &[&str] = &["AdamW", "SGD", "Muon"];

// ---------------------------------------------------------------------------
// Request (wire camelCase, deny_unknown_fields — D6 :333-334)
// ---------------------------------------------------------------------------

/// Augmentação do treino (D6 :326-327).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct YoloAugment {
    #[serde(default = "default_true")]
    pub mosaic: bool,
    #[serde(default)]
    pub mixup_flip: bool,
}

fn default_true() -> bool {
    true
}

/// Body de `POST /api/jobs/yolo` (wire camelCase — D6/D7).
///
/// `datasetId` é obrigatório; os demais campos têm defaults e são OPCIONAIS.
/// `deny_unknown_fields` garante 400 para chaves desconhecidas.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct YoloJobRequest {
    pub dataset_id: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_epochs")]
    pub epochs: u32,
    #[serde(default = "default_batch")]
    pub batch: u32,
    #[serde(default = "default_imgsz")]
    pub imgsz: u32,
    #[serde(default = "default_lr0")]
    pub lr0: f64,
    #[serde(default = "default_optimizer")]
    pub optimizer: String,
    #[serde(default = "default_augment")]
    pub augment: YoloAugment,
    pub seed: Option<u64>,
}

fn default_model() -> String {
    "yolo11m".to_string()
}
fn default_epochs() -> u32 {
    100
}
fn default_batch() -> u32 {
    16
}
fn default_imgsz() -> u32 {
    640
}
fn default_lr0() -> f64 {
    0.01
}
fn default_optimizer() -> String {
    "AdamW".to_string()
}
fn default_augment() -> YoloAugment {
    YoloAugment {
        mosaic: true,
        mixup_flip: false,
    }
}

/// Defaults públicos (usados pelo handler para montar `config.yaml`).
pub fn default_augment_values() -> (bool, bool) {
    (true, false)
}

// ---------------------------------------------------------------------------
// Validação pura (ADR-0007 D6 :329-334)
// ---------------------------------------------------------------------------

/// Valida o body do POST /api/jobs/yolo.
///
/// `Err(String)` ⇒ 400 `invalid_request`; `Ok(YoloJobRequest)` com defaults
/// já aplicados pelo serde.
pub fn validate_yolo_request(req: YoloJobRequest) -> Result<YoloJobRequest, String> {
    if !ALLOWED_MODELS.contains(&req.model.as_str()) {
        return Err(format!(
            "model must be one of: {}",
            ALLOWED_MODELS.join(", ")
        ));
    }
    if !(1..=1000).contains(&req.epochs) {
        return Err("epochs must be between 1 and 1000".to_string());
    }
    if !ALLOWED_BATCH.contains(&req.batch) {
        return Err(format!(
            "batch must be one of: {}",
            ALLOWED_BATCH
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !ALLOWED_IMGSZ.contains(&req.imgsz) {
        return Err(format!(
            "imgsz must be one of: {}",
            ALLOWED_IMGSZ
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !(1e-5..=1e-1).contains(&req.lr0) {
        return Err("lr0 must be between 1e-5 and 1e-1".to_string());
    }
    if !ALLOWED_OPTIMIZER.contains(&req.optimizer.as_str()) {
        return Err(format!(
            "optimizer must be one of: {}",
            ALLOWED_OPTIMIZER.join(", ")
        ));
    }
    validate_augment(&req.augment)?;
    Ok(req)
}

fn validate_augment(aug: &YoloAugment) -> Result<(), String> {
    // mosaic e mixup_flip são bools — serde já valida tipo.
    // Validação de range: bools não têm range, mas garantimos consistência.
    let _ = aug;
    Ok(())
}

// ---------------------------------------------------------------------------
// Geração de config.yaml (ADR-0007 D6 :322-327)
// ---------------------------------------------------------------------------

/// Gera `config.yaml` como string YAML (D6 :322-327).
///
/// Placeholders literais `{dataset_path}` e `{output_path}` — o orquestrador
/// substitui no spawn; o principal é agnóstico de paths.
pub fn generate_config_yaml(job_id: &str, req: &YoloJobRequest) -> String {
    let augment = &req.augment;
    format!(
        r#"# Configuração de treino YOLO (gerada pelo api-principal)
job_id: "{job_id}"
engine: "yolo"
model: "{model}"
mode: "train"
dataset_path: "{{dataset_path}}"
output_path: "{{output_path}}"
seed: {seed}

yolo:
  model: "{model}"
  epochs: {epochs}
  batch: {batch}
  imgsz: {imgsz}
  lr0: {lr0}
  optimizer: "{optimizer}"
  augment:
    mosaic: {mosaic}
    mixup_flip: {mixup_flip}
"#,
        job_id = job_id,
        model = req.model,
        seed = req.seed.unwrap_or(42),
        epochs = req.epochs,
        batch = req.batch,
        imgsz = req.imgsz,
        lr0 = req.lr0,
        optimizer = req.optimizer,
        mosaic = augment.mosaic,
        mixup_flip = augment.mixup_flip,
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_apply() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000"}"#;
        let req: YoloJobRequest = serde_json::from_str(raw).expect("parse");
        assert_eq!(req.model, "yolo11m");
        assert_eq!(req.epochs, 100);
        assert_eq!(req.batch, 16);
        assert_eq!(req.imgsz, 640);
        assert!((req.lr0 - 0.01).abs() < 1e-10);
        assert_eq!(req.optimizer, "AdamW");
        assert!(req.augment.mosaic);
        assert!(!req.augment.mixup_flip);
        assert!(req.seed.is_none());
    }

    #[test]
    fn custom_values() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000","model":"yolo11x","epochs":50,"batch":32,"imgsz":1024,"lr0":0.001,"optimizer":"SGD","augment":{"mosaic":false,"mixupFlip":true},"seed":123}"#;
        let req: YoloJobRequest = serde_json::from_str(raw).expect("parse");
        assert_eq!(req.model, "yolo11x");
        assert_eq!(req.epochs, 50);
        assert_eq!(req.batch, 32);
        assert_eq!(req.imgsz, 1024);
        assert!((req.lr0 - 0.001).abs() < 1e-10);
        assert_eq!(req.optimizer, "SGD");
        assert!(!req.augment.mosaic);
        assert!(req.augment.mixup_flip);
        assert_eq!(req.seed, Some(123));
    }

    #[test]
    fn deny_unknown_fields() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000","extra":1}"#;
        let err = serde_json::from_str::<YoloJobRequest>(raw);
        assert!(err.is_err(), "deny_unknown_fields");
    }

    // --- validação por modelo ---

    #[test]
    fn valid_models() {
        for model in ALLOWED_MODELS {
            let raw = format!(
                r#"{{"datasetId":"00000000-0000-0000-0000-000000000000","model":"{model}"}}"#
            );
            let req: YoloJobRequest = serde_json::from_str(&raw).expect("parse");
            assert!(validate_yolo_request(req).is_ok(), "model={model}");
        }
    }

    #[test]
    fn invalid_model() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000","model":"resnet50"}"#;
        let req: YoloJobRequest = serde_json::from_str(raw).expect("parse");
        assert!(validate_yolo_request(req).is_err());
    }

    // --- validação por epochs ---

    #[test]
    fn epochs_boundary() {
        for e in [1, 1000] {
            let raw =
                format!(r#"{{"datasetId":"00000000-0000-0000-0000-000000000000","epochs":{e}}}"#);
            let req: YoloJobRequest = serde_json::from_str(&raw).expect("parse");
            assert!(validate_yolo_request(req).is_ok(), "epochs={e}");
        }
        for e in [0, 1001] {
            let raw =
                format!(r#"{{"datasetId":"00000000-0000-0000-0000-000000000000","epochs":{e}}}"#);
            let req: YoloJobRequest = serde_json::from_str(&raw).expect("parse");
            assert!(validate_yolo_request(req).is_err(), "epochs={e}");
        }
    }

    // --- validação por batch ---

    #[test]
    fn batch_allowed() {
        for b in ALLOWED_BATCH {
            let raw =
                format!(r#"{{"datasetId":"00000000-0000-0000-0000-000000000000","batch":{b}}}"#);
            let req: YoloJobRequest = serde_json::from_str(&raw).expect("parse");
            assert!(validate_yolo_request(req).is_ok(), "batch={b}");
        }
    }

    #[test]
    fn batch_invalid() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000","batch":12}"#;
        let req: YoloJobRequest = serde_json::from_str(raw).expect("parse");
        assert!(validate_yolo_request(req).is_err());
    }

    // --- validação por imgsz ---

    #[test]
    fn imgsz_allowed() {
        for s in ALLOWED_IMGSZ {
            let raw =
                format!(r#"{{"datasetId":"00000000-0000-0000-0000-000000000000","imgsz":{s}}}"#);
            let req: YoloJobRequest = serde_json::from_str(&raw).expect("parse");
            assert!(validate_yolo_request(req).is_ok(), "imgsz={s}");
        }
    }

    #[test]
    fn imgsz_invalid() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000","imgsz":800}"#;
        let req: YoloJobRequest = serde_json::from_str(raw).expect("parse");
        assert!(validate_yolo_request(req).is_err());
    }

    // --- validação por lr0 ---

    #[test]
    fn lr0_boundary() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000","lr0":0.00001}"#;
        let req: YoloJobRequest = serde_json::from_str(raw).expect("parse");
        assert!(validate_yolo_request(req).is_ok(), "lr0=1e-5");

        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000","lr0":0.1}"#;
        let req: YoloJobRequest = serde_json::from_str(raw).expect("parse");
        assert!(validate_yolo_request(req).is_ok(), "lr0=1e-1");

        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000","lr0":0.000001}"#;
        let req: YoloJobRequest = serde_json::from_str(raw).expect("parse");
        assert!(validate_yolo_request(req).is_err(), "lr0=1e-6");

        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000","lr0":0.2}"#;
        let req: YoloJobRequest = serde_json::from_str(raw).expect("parse");
        assert!(validate_yolo_request(req).is_err(), "lr0=0.2");
    }

    // --- validação por optimizer ---

    #[test]
    fn optimizer_allowed() {
        for opt in ALLOWED_OPTIMIZER {
            let raw = format!(
                r#"{{"datasetId":"00000000-0000-0000-0000-000000000000","optimizer":"{opt}"}}"#
            );
            let req: YoloJobRequest = serde_json::from_str(&raw).expect("parse");
            assert!(validate_yolo_request(req).is_ok(), "optimizer={opt}");
        }
    }

    #[test]
    fn optimizer_invalid() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000","optimizer":"Adam"}"#;
        let req: YoloJobRequest = serde_json::from_str(raw).expect("parse");
        assert!(validate_yolo_request(req).is_err());
    }

    // --- config.yaml ---

    #[test]
    fn config_yaml_placeholders_and_defaults() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000"}"#;
        let req: YoloJobRequest = serde_json::from_str(raw).expect("parse");
        let yaml = generate_config_yaml("test-job-001", &req);

        // Placeholders presentes.
        assert!(yaml.contains("{dataset_path}"));
        assert!(yaml.contains("{output_path}"));

        // Defaults corretos.
        assert!(yaml.contains("model: \"yolo11m\""));
        assert!(yaml.contains("epochs: 100"));
        assert!(yaml.contains("batch: 16"));
        assert!(yaml.contains("imgsz: 640"));
        assert!(yaml.contains("lr0: 0.01"));
        assert!(yaml.contains("optimizer: \"AdamW\""));
        assert!(yaml.contains("mosaic: true"));
        assert!(yaml.contains("mixup_flip: false"));
        assert!(yaml.contains("seed: 42"));
        assert!(yaml.contains("job_id: \"test-job-001\""));
        assert!(yaml.contains("engine: \"yolo\""));
        assert!(yaml.contains("mode: \"train\""));

        // Parseável como YAML.
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("yaml parse");
        assert_eq!(parsed["yolo"]["epochs"].as_u64().unwrap(), 100);
    }

    #[test]
    fn config_yaml_custom_values() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000","model":"yolo11x","epochs":50,"seed":99,"augment":{"mosaic":false,"mixupFlip":true}}"#;
        let req: YoloJobRequest = serde_json::from_str(raw).expect("parse");
        let yaml = generate_config_yaml("job-xyz", &req);

        assert!(yaml.contains("model: \"yolo11x\""));
        assert!(yaml.contains("epochs: 50"));
        assert!(yaml.contains("mosaic: false"));
        assert!(yaml.contains("mixup_flip: true"));
        assert!(yaml.contains("seed: 99"));
    }
}
