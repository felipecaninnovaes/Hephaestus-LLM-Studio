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

/// Body de `POST /api/jobs/yolo` (wire camelCase — D6/D7/D5 ADR-0012).
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
    /// UUID de pesos existentes na tabela `models` (fine-tune — D5).
    /// Validação: string não-UUID ⇒ 400 `invalid_request`.
    pub weights: Option<String>,
    /// UUID de orquestrador preferencial (ADR-0015 D2).
    /// Validação: string não-UUID ⇒ 400 `invalid_request`.
    pub orchestrator_id: Option<String>,
    /// Nome customizado opcional do modelo gerado (ADR-0022 D1).
    pub output_name: Option<String>,
}

/// Valida outputName customizado (ADR-0022 D1).
/// 1 a 100 caracteres, slug-safe (alfanumérico, hífen, underscore, ponto, espaços).
pub fn validate_output_name(name: &str) -> Result<(), String> {
    let clean = name.trim();
    if clean.is_empty() || clean.chars().count() > 100 {
        return Err("outputName must be between 1 and 100 characters".to_string());
    }
    if !clean
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == ' ')
    {
        return Err("outputName contains invalid characters".to_string());
    }
    Ok(())
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
// AutoTracker (ADR-0008 D3) — body, validação e config.yaml
// ---------------------------------------------------------------------------

/// Models aceitos para AutoTracker v1 (Apenas mock).
const ALLOWED_AUTOTRACK_MODELS: &[&str] = &["mock"];

/// Body de `POST /api/jobs/autotracker` (wire camelCase — ADR-0008 D3, ADR-0014 D2/D6).
///
/// `datasetId` é obrigatório; `model`, `conf` e `modelId` são OPCIONAIS com defaults.
/// `deny_unknown_fields` garante 400 para chaves desconhecidas.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutotrackerJobRequest {
    pub dataset_id: String,
    #[serde(default = "default_autotrack_model")]
    pub model: String,
    #[serde(default = "default_autotrack_conf")]
    pub conf: f64,
    /// UUID de modelo existente na tabela `models` (engine='world' — ADR-0014 D2).
    /// Presente → job REAL (usa pesos); ausente → mock.
    pub model_id: Option<String>,
    /// UUID de orquestrador preferencial (ADR-0015 D2).
    /// Validação: string não-UUID ⇒ 400 `invalid_request`.
    pub orchestrator_id: Option<String>,
}

fn default_autotrack_model() -> String {
    "mock".to_string()
}

fn default_autotrack_conf() -> f64 {
    0.65
}

/// Valida o body do POST /api/jobs/autotracker.
///
/// `Err(String)` ⇒ 400 `invalid_request`; `Ok(AutotrackerJobRequest)` com defaults
/// já aplicados pelo serde.
pub fn validate_autotrack_request(
    req: AutotrackerJobRequest,
) -> Result<AutotrackerJobRequest, String> {
    if !ALLOWED_AUTOTRACK_MODELS.contains(&req.model.as_str()) {
        return Err(format!(
            "model must be one of: {}",
            ALLOWED_AUTOTRACK_MODELS.join(", ")
        ));
    }
    if !(0.0..=1.0).contains(&req.conf) {
        return Err("conf must be between 0.0 and 1.0".to_string());
    }
    // ADR-0014 D6: modelId presente e não-UUID ⇒ 400 `invalid_request`.
    if let Some(ref mid) = req.model_id {
        if uuid::Uuid::parse_str(mid).is_err() {
            return Err("modelId must be a valid UUID".to_string());
        }
    }
    // ADR-0015 D2: orchestratorId presente e não-UUID ⇒ 400 `invalid_request`.
    if let Some(ref oid) = req.orchestrator_id {
        if uuid::Uuid::parse_str(oid).is_err() {
            return Err("orchestratorId must be a valid UUID".to_string());
        }
    }
    Ok(req)
}

/// Gera `config.yaml` para AutoTracker (ADR-0008 D3, ADR-0014 D6).
///
/// Placeholders literais `{dataset_path}` e `{output_path}` — o orquestrador
/// substitui no spawn; o principal é agnóstico de paths.
/// Quando `model_id` está presente, emite `weights_path: "{weights_path}"` e
/// `autotrack.model: "world"` (o model do body permanece "mock" — a derivação
/// é interna, documentada).
pub fn generate_autotrack_config_yaml(job_id: &str, req: &AutotrackerJobRequest) -> String {
    let weights_line = if req.model_id.is_some() {
        "weights_path: \"{weights_path}\"\n".to_string()
    } else {
        String::new()
    };
    let at_model = if req.model_id.is_some() {
        "world"
    } else {
        &req.model
    };
    format!(
        r#"# Configuração de autotrack (gerada pelo api-principal)
job_id: "{job_id}"
engine: "autotracker"
model: "{model}"
mode: "autotrack"
dataset_path: "{{dataset_path}}"
output_path: "{{output_path}}"
{weights_line}seed: 42
autotrack:
  model: "{at_model}"
  conf: {conf}
"#,
        job_id = job_id,
        model = req.model,
        weights_line = weights_line,
        at_model = at_model,
        conf = req.conf,
    )
}

// ---------------------------------------------------------------------------
// AutoTracker apply (ADR-0008 D1/D1a) — body, parse do boxes.json
// ---------------------------------------------------------------------------

/// Body de `POST /api/jobs/:id/autotracker/apply` (wire camelCase — ADR-0008 D1).
///
/// `overwrite` (default false), `imageId` (UUID opcional) e `createMissingClasses` (array opcional) — `deny_unknown_fields`.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutotrackerApplyRequest {
    #[serde(default)]
    pub overwrite: bool,
    #[serde(default)]
    pub image_id: Option<String>,
    #[serde(default)]
    pub create_missing_classes: Option<Vec<String>>,
}

/// Contagem de boxes por classe no preview do autotracker.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutotrackerClassCount {
    pub name: String,
    pub boxes_count: i64,
}

/// Resposta de `GET /api/jobs/:id/autotracker/preview`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutotrackerPreviewResponse {
    pub total_images: i64,
    pub total_boxes: i64,
    pub existing_classes: Vec<AutotrackerClassCount>,
    pub missing_classes: Vec<AutotrackerClassCount>,
}

/// Box extraída do `boxes.json` do engine (snake_case transporte — ADR-0008 D1).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EngineBox {
    pub class: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub conf: f64,
}

/// Imagem dentro do `boxes.json` (snake_case transporte — ADR-0008 D1).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EngineImage {
    pub filename: String,
    pub boxes: Vec<EngineBox>,
}

/// Formato completo do artefato `boxes.json` (snake_case transporte — ADR-0008 D1).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct BoxesArtifact {
    pub engine: String,
    pub model: String,
    pub seed: u64,
    pub conf: f64,
    pub images: Vec<EngineImage>,
}

/// Parse do `boxes.json` com validação de shape e domínios.
///
/// `Err(String)` ⇒ 400 `invalid_request`. Validações:
/// - JSON parseável com shape `BoxesArtifact`
/// - Cada box: `x/y/w/h` em `0..=1`, `conf` em `0..=1`
/// - Nome de classe não vazio
/// - Filename não vazio
/// - Nenhuma imagem com filename duplicado
pub fn parse_boxes_json(data: &[u8]) -> Result<BoxesArtifact, String> {
    let art: BoxesArtifact =
        serde_json::from_slice(data).map_err(|e| format!("invalid boxes.json: {e}"))?;

    // Validação de campos não vazios
    if art.engine.is_empty() {
        return Err("engine is empty".into());
    }
    if art.images.is_empty() {
        // Imagens vazias é válido (job sem resultado — aplica nada).
        return Ok(art);
    }

    let mut filenames = std::collections::HashSet::new();
    for img in &art.images {
        if img.filename.is_empty() {
            return Err("filename is empty".into());
        }
        if !filenames.insert(&img.filename) {
            return Err(format!("duplicate filename: {}", img.filename));
        }
        for b in &img.boxes {
            if b.class.is_empty() {
                return Err("class name is empty".into());
            }
            for v in [b.x, b.y, b.w, b.h] {
                if !(0.0..=1.0).contains(&v) {
                    return Err(format!("box coordinate out of 0..=1: {v}"));
                }
            }
            if !(0.0..=1.0).contains(&b.conf) {
                return Err(format!("box conf out of 0..=1: {}", b.conf));
            }
        }
    }
    Ok(art)
}

/// Resolve nomes de classe para class_ids do dataset.
///
/// Retorna `(class_id_map, skipped_count)`: um mapa de `class_name → class_id`
/// para classes que existem; classes ausentes são omitidas (contadas como
/// skipped pelo caller).
pub fn resolve_class_ids(
    classes: &[(uuid::Uuid, String)],
) -> std::collections::HashMap<String, uuid::Uuid> {
    classes
        .iter()
        .map(|(id, name)| (name.clone(), *id))
        .collect()
}

/// Resolve o class_id correspondente a uma classe retornada pelo engine,
/// usando correspondência exata primeiro, com fallback para case-insensitive
/// e equivalência entre espaço e underscore (ex.: ARMPITS_EXPOSED vs armpits_exposed).
pub fn match_class_id<'a>(
    class_name: &str,
    exact_map: &'a std::collections::HashMap<String, uuid::Uuid>,
    lower_map: &'a std::collections::HashMap<String, uuid::Uuid>,
) -> Option<&'a uuid::Uuid> {
    if let Some(id) = exact_map.get(class_name) {
        return Some(id);
    }
    let lower = class_name.to_lowercase();
    if let Some(id) = lower_map.get(&lower) {
        return Some(id);
    }
    let with_space = lower.replace('_', " ");
    if let Some(id) = lower_map.get(&with_space) {
        return Some(id);
    }
    let with_underscore = lower.replace(' ', "_");
    if let Some(id) = lower_map.get(&with_underscore) {
        return Some(id);
    }
    None
}

// ---------------------------------------------------------------------------
// Predict (Fatia J — ADR-0013 D0/D1/D2/D8) — body, validação e config.yaml
// ---------------------------------------------------------------------------

/// Body de `POST /api/jobs/predict` (wire camelCase — ADR-0013 D8).
///
/// `modelId` e `datasetId` são obrigatórios; `conf` é OPCIONAL com default 0.65.
/// `deny_unknown_fields` garante 400 para chaves desconhecidas.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PredictJobRequest {
    /// UUID do modelo (tabela `models`); validação: não-UUID ⇒ 400.
    pub model_id: String,
    /// UUID do dataset; validação: não-UUID ⇒ 404 (D8).
    pub dataset_id: String,
    #[serde(default = "default_predict_conf")]
    pub conf: f64,
    /// UUID de orquestrador preferencial (ADR-0015 D2).
    /// Validação: string não-UUID ⇒ 400 `invalid_request`.
    pub orchestrator_id: Option<String>,
}

fn default_predict_conf() -> f64 {
    0.65
}

/// Valida o body do POST /api/jobs/predict.
///
/// `Err(String)` ⇒ 400 `invalid_request`; `Ok(PredictJobRequest)` com defaults
/// já aplicados pelo serde.
pub fn validate_predict_request(req: PredictJobRequest) -> Result<PredictJobRequest, String> {
    if uuid::Uuid::parse_str(&req.model_id).is_err() {
        return Err("modelId must be a valid UUID".to_string());
    }
    if !(0.0..=1.0).contains(&req.conf) {
        return Err("conf must be between 0.0 and 1.0".to_string());
    }
    // ADR-0015 D2: orchestratorId presente e não-UUID ⇒ 400 `invalid_request`.
    if let Some(ref oid) = req.orchestrator_id {
        if uuid::Uuid::parse_str(oid).is_err() {
            return Err("orchestratorId must be a valid UUID".to_string());
        }
    }
    Ok(req)
}

/// Gera `config.yaml` para predict YOLO (ADR-0013 D3).
///
/// Placeholders literais `{dataset_path}`, `{output_path}` e `{weights_path}`
/// — o orquestrador substitui no spawn; o principal é agnóstico de paths.
pub fn generate_predict_config_yaml(job_id: &str, req: &PredictJobRequest) -> String {
    format!(
        r#"# Configuração de predict YOLO (gerada pelo api-principal)
job_id: "{job_id}"
engine: "yolo"
model: "predict"
mode: "predict"
dataset_path: "{{dataset_path}}"
output_path: "{{output_path}}"
weights_path: "{{weights_path}}"
seed: 42

predict:
  conf: {conf}
"#,
        job_id = job_id,
        conf = req.conf,
    )
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
    // D5: valida weights UUID (se presente).
    if let Some(ref w) = req.weights {
        if uuid::Uuid::parse_str(w).is_err() {
            return Err("weights must be a valid UUID".to_string());
        }
    }
    // ADR-0015 D2: orchestratorId presente e não-UUID ⇒ 400 `invalid_request`.
    if let Some(ref oid) = req.orchestrator_id {
        if uuid::Uuid::parse_str(oid).is_err() {
            return Err("orchestratorId must be a valid UUID".to_string());
        }
    }
    // ADR-0022 D1: valida outputName se fornecido.
    if let Some(ref out_name) = req.output_name {
        validate_output_name(out_name)?;
    }
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

/// Gera `config.yaml` como string YAML (D6/D5 ADR-0012).
///
/// Placeholders literais `{dataset_path}` e `{output_path}` — o orquestrador
/// substitui no spawn; o principal é agnóstico de paths.
/// Quando `weights` está presente, emite `weights_path: "{weights_path}"`.
pub fn generate_config_yaml(job_id: &str, req: &YoloJobRequest) -> String {
    let augment = &req.augment;
    let weights_line = if req.weights.is_some() {
        format!("weights_path: \"{{weights_path}}\"\n")
    } else {
        String::new()
    };
    format!(
        r#"# Configuração de treino YOLO (gerada pelo api-principal)
job_id: "{job_id}"
engine: "yolo"
model: "{model}"
mode: "train"
dataset_path: "{{dataset_path}}"
output_path: "{{output_path}}"
{weights_line}seed: {seed}

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
        weights_line = weights_line,
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
// AutoLabel (ADR-0016 / ADR-0019 AutoLabel v2)
// ---------------------------------------------------------------------------

pub const ALLOWED_AUTOLABEL_MODELS: &[&str] = &["mock", "florence-2", "qwen2-vl", "openai"];

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutolabelJobRequest {
    pub dataset_id: String,
    #[serde(default = "default_autolabel_model")]
    pub model: String,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub api_base: Option<String>,
    #[serde(default)]
    pub openai_model: Option<String>,
    #[serde(default)]
    pub orchestrator_id: Option<String>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub filter_class_id: Option<String>,
    #[serde(default)]
    pub image_ids: Option<Vec<String>>,
}

fn default_autolabel_model() -> String {
    "mock".to_string()
}

pub fn validate_autolabel_request(
    mut req: AutolabelJobRequest,
) -> Result<AutolabelJobRequest, String> {
    if !ALLOWED_AUTOLABEL_MODELS.contains(&req.model.as_str()) {
        return Err(format!(
            "model must be one of {:?}, got '{}'",
            ALLOWED_AUTOLABEL_MODELS, req.model
        ));
    }
    if let Some(ref mut p) = req.prompt {
        let trimmed = p.trim().to_string();
        if trimmed.chars().count() > 8000 {
            return Err("prompt must not exceed 8000 characters".to_string());
        }
        *p = trimmed;
    }
    if let Some(ref mut ak) = req.api_key {
        let clean = ak.trim().trim_matches('"').trim_matches('\'').to_string();
        if clean.chars().count() > 512 {
            return Err("apiKey must not exceed 512 characters".to_string());
        }
        *ak = clean;
    }
    if let Some(ref mut ab) = req.api_base {
        let clean = ab
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .trim_end_matches('/')
            .to_string();
        if clean.chars().count() > 512 {
            return Err("apiBase must not exceed 512 characters".to_string());
        }
        if !clean.starts_with("http://") && !clean.starts_with("https://") {
            return Err("apiBase must start with http:// or https://".to_string());
        }
        *ab = clean;
    }
    if let Some(ref mut om) = req.openai_model {
        let clean = om.trim().trim_matches('"').trim_matches('\'').to_string();
        if clean.chars().count() > 128 {
            return Err("openaiModel must not exceed 128 characters".to_string());
        }
        *om = clean;
    }
    if let Some(ref o) = req.orchestrator_id {
        if uuid::Uuid::parse_str(o).is_err() {
            return Err("orchestratorId must be a valid UUID".to_string());
        }
    }
    if let Some(ref re) = req.reasoning_effort {
        let valid = ["none", "low", "medium", "high"];
        if !valid.contains(&re.as_str()) {
            return Err(format!(
                "reasoningEffort must be one of {valid:?}, got '{re}'"
            ));
        }
    }
    if let Some(ref fc) = req.filter_class_id {
        if uuid::Uuid::parse_str(fc).is_err() {
            return Err("filterClassId must be a valid UUID".to_string());
        }
    }
    if let Some(ref ids) = req.image_ids {
        if ids.is_empty() {
            return Err("imageIds must not be empty if provided".to_string());
        }
        for id in ids {
            if uuid::Uuid::parse_str(id).is_err() {
                return Err(format!("imageId '{id}' must be a valid UUID"));
            }
        }
    }
    Ok(req)
}

pub fn generate_autolabel_config_yaml(job_id: &str, req: &AutolabelJobRequest) -> String {
    let mut autolabel_lines = String::new();
    if let Some(p) = &req.prompt {
        autolabel_lines.push_str(&format!(
            "  prompt: {}\n",
            serde_json::to_string(p).unwrap_or_else(|_| "\"\"".into())
        ));
    }
    if let Some(ak) = &req.api_key {
        let clean_ak = ak.trim().trim_matches('"').trim_matches('\'');
        autolabel_lines.push_str(&format!(
            "  api_key: {}\n",
            serde_json::to_string(clean_ak).unwrap_or_else(|_| "\"\"".into())
        ));
    }
    if let Some(ab) = &req.api_base {
        let clean_ab = ab
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .trim_end_matches('/');
        autolabel_lines.push_str(&format!(
            "  api_base: {}\n",
            serde_json::to_string(clean_ab).unwrap_or_else(|_| "\"\"".into())
        ));
    }
    if let Some(om) = &req.openai_model {
        let clean_om = om.trim().trim_matches('"').trim_matches('\'');
        autolabel_lines.push_str(&format!(
            "  openai_model: {}\n",
            serde_json::to_string(clean_om).unwrap_or_else(|_| "\"\"".into())
        ));
    }
    if let Some(re) = &req.reasoning_effort {
        autolabel_lines.push_str(&format!(
            "  reasoning_effort: {}\n",
            serde_json::to_string(re).unwrap_or_else(|_| "\"none\"".into())
        ));
    }
    format!(
        r#"# Configuração de autolabel (gerada pelo api-principal)
job_id: "{job_id}"
engine: "autolabel"
model: "{model}"
mode: "autolabel"
dataset_path: "{{dataset_path}}"
output_path: "{{output_path}}"
seed: 42
autolabel:
{autolabel_lines}"#,
        job_id = job_id,
        model = req.model,
        autolabel_lines = autolabel_lines,
    )
}

// ---------------------------------------------------------------------------
// Diffusion Job (ADR-0018 D1)
// ---------------------------------------------------------------------------

const ALLOWED_DIFFUSION_BASE_MODELS: &[&str] = &["sdxl", "flux", "sd15", "flux-2-klein-4b"];
const ALLOWED_DIFFUSION_BATCH: &[u32] = &[1, 2, 4, 8];
const ALLOWED_DIFFUSION_RESOLUTIONS: &[u32] = &[512, 768, 1024];
const ALLOWED_DIFFUSION_GRAD_ACCUM: &[u32] = &[1, 2, 4, 8];
const ALLOWED_DIFFUSION_OPTIMIZERS: &[&str] = &["adamw8bit", "adamw", "prodigy"];
const ALLOWED_DIFFUSION_LR_SCHEDULERS: &[&str] =
    &["cosine", "linear", "constant", "constant_with_warmup"];
const ALLOWED_DIFFUSION_PRECISION: &[&str] = &["fp16", "bf16", "no"];
const ALLOWED_DIFFUSION_QUANTIZATIONS: &[&str] = &["none", "4bit", "8bit", "4bit-nf4", "8bit-bnb"];

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiffusionJobRequest {
    pub dataset_id: String,
    #[serde(default = "default_diffusion_base_model")]
    pub base_model: String,
    #[serde(default)]
    pub trigger_word: Option<String>,
    #[serde(default = "default_diffusion_epochs")]
    pub epochs: u32,
    #[serde(default = "default_diffusion_batch")]
    pub batch_size: u32,
    #[serde(default = "default_diffusion_lr")]
    pub learning_rate: f64,
    #[serde(default = "default_diffusion_rank")]
    pub rank: u32,
    #[serde(default = "default_diffusion_alpha")]
    pub alpha: u32,
    #[serde(default)]
    pub weights: Option<String>,
    #[serde(default)]
    pub orchestrator_id: Option<String>,
    #[serde(default)]
    pub sample_prompt: Option<String>,
    #[serde(default = "default_diffusion_sample_interval")]
    pub sample_interval: u32,
    #[serde(default)]
    pub sample_seed: Option<u64>,
    #[serde(default)]
    pub resolution: Option<u32>,
    #[serde(default = "default_diffusion_grad_accum")]
    pub gradient_accumulation_steps: u32,
    #[serde(default = "default_diffusion_optimizer")]
    pub optimizer: String,
    #[serde(default = "default_diffusion_lr_scheduler")]
    pub lr_scheduler: String,
    #[serde(default = "default_diffusion_lr_warmup")]
    pub lr_warmup_steps: u32,
    #[serde(default = "default_diffusion_precision")]
    pub mixed_precision: String,
    #[serde(default = "default_diffusion_quantization")]
    pub quantization: String,
    /// Nome customizado opcional do modelo gerado (ADR-0022 D1).
    pub output_name: Option<String>,
    #[serde(default = "default_diffusion_checkpoint_interval")]
    pub checkpoint_interval: u32,
    #[serde(default)]
    pub epoch_offset: Option<u32>,
}

fn default_diffusion_checkpoint_interval() -> u32 {
    1
}

fn default_diffusion_base_model() -> String {
    "sdxl".to_string()
}
fn default_diffusion_quantization() -> String {
    "4bit".to_string()
}
fn default_diffusion_epochs() -> u32 {
    10
}
fn default_diffusion_batch() -> u32 {
    1
}
fn default_diffusion_lr() -> f64 {
    0.0001
}
fn default_diffusion_rank() -> u32 {
    16
}
fn default_diffusion_alpha() -> u32 {
    16
}
fn default_diffusion_sample_interval() -> u32 {
    1
}
fn default_diffusion_grad_accum() -> u32 {
    1
}
fn default_diffusion_optimizer() -> String {
    "adamw8bit".to_string()
}
fn default_diffusion_lr_scheduler() -> String {
    "cosine".to_string()
}
fn default_diffusion_lr_warmup() -> u32 {
    0
}
fn default_diffusion_precision() -> String {
    "fp16".to_string()
}

pub fn validate_diffusion_request(req: DiffusionJobRequest) -> Result<DiffusionJobRequest, String> {
    if !ALLOWED_DIFFUSION_BASE_MODELS.contains(&req.base_model.as_str()) {
        return Err(format!(
            "baseModel must be one of {:?}, got '{}'",
            ALLOWED_DIFFUSION_BASE_MODELS, req.base_model
        ));
    }
    if !(1..=100).contains(&req.epochs) {
        return Err(format!(
            "epochs must be between 1 and 100, got {}",
            req.epochs
        ));
    }
    if !ALLOWED_DIFFUSION_BATCH.contains(&req.batch_size) {
        return Err(format!(
            "batchSize must be one of {:?}, got {}",
            ALLOWED_DIFFUSION_BATCH, req.batch_size
        ));
    }
    if !(1e-6..=0.01).contains(&req.learning_rate) || req.learning_rate.is_nan() {
        return Err(format!(
            "learningRate must be between 0.000001 and 0.01, got {}",
            req.learning_rate
        ));
    }
    if !(4..=128).contains(&req.rank) {
        return Err(format!("rank must be between 4 and 128, got {}", req.rank));
    }
    if !(4..=128).contains(&req.alpha) {
        return Err(format!(
            "alpha must be between 4 and 128, got {}",
            req.alpha
        ));
    }
    if let Some(res) = req.resolution {
        if !ALLOWED_DIFFUSION_RESOLUTIONS.contains(&res) {
            return Err(format!(
                "resolution must be one of {:?}, got {}",
                ALLOWED_DIFFUSION_RESOLUTIONS, res
            ));
        }
    }
    if !ALLOWED_DIFFUSION_GRAD_ACCUM.contains(&req.gradient_accumulation_steps) {
        return Err(format!(
            "gradientAccumulationSteps must be one of {:?}, got {}",
            ALLOWED_DIFFUSION_GRAD_ACCUM, req.gradient_accumulation_steps
        ));
    }
    if !ALLOWED_DIFFUSION_OPTIMIZERS.contains(&req.optimizer.as_str()) {
        return Err(format!(
            "optimizer must be one of {:?}, got '{}'",
            ALLOWED_DIFFUSION_OPTIMIZERS, req.optimizer
        ));
    }
    if !ALLOWED_DIFFUSION_LR_SCHEDULERS.contains(&req.lr_scheduler.as_str()) {
        return Err(format!(
            "lrScheduler must be one of {:?}, got '{}'",
            ALLOWED_DIFFUSION_LR_SCHEDULERS, req.lr_scheduler
        ));
    }
    if !(0..=1000).contains(&req.lr_warmup_steps) {
        return Err(format!(
            "lrWarmupSteps must be between 0 and 1000, got {}",
            req.lr_warmup_steps
        ));
    }
    if !ALLOWED_DIFFUSION_PRECISION.contains(&req.mixed_precision.as_str()) {
        return Err(format!(
            "mixedPrecision must be one of {:?}, got '{}'",
            ALLOWED_DIFFUSION_PRECISION, req.mixed_precision
        ));
    }
    if !ALLOWED_DIFFUSION_QUANTIZATIONS.contains(&req.quantization.as_str()) {
        return Err(format!(
            "quantization must be one of {:?}, got '{}'",
            ALLOWED_DIFFUSION_QUANTIZATIONS, req.quantization
        ));
    }
    if let Some(ref tw) = req.trigger_word {
        if tw.chars().count() > 100 {
            return Err("triggerWord must not exceed 100 characters".to_string());
        }
    }
    if let Some(ref w) = req.weights {
        if uuid::Uuid::parse_str(w).is_err() {
            return Err("weights must be a valid UUID".to_string());
        }
    }
    if let Some(ref o) = req.orchestrator_id {
        if uuid::Uuid::parse_str(o).is_err() {
            return Err("orchestratorId must be a valid UUID".to_string());
        }
    }
    if let Some(ref sp) = req.sample_prompt {
        if sp.chars().count() > 500 {
            return Err("samplePrompt must not exceed 500 characters".to_string());
        }
    }
    if !(0..=100).contains(&req.sample_interval) {
        return Err(format!(
            "sampleInterval must be between 0 and 100, got {}",
            req.sample_interval
        ));
    }
    if !(1..=100).contains(&req.checkpoint_interval) {
        return Err(format!(
            "checkpointInterval must be between 1 and 100, got {}",
            req.checkpoint_interval
        ));
    }
    if let Some(offset) = req.epoch_offset {
        if offset > 1000 {
            return Err(format!(
                "epochOffset must be between 0 and 1000, got {}",
                offset
            ));
        }
    }
    // ADR-0022 D1: valida outputName se fornecido.
    if let Some(ref out_name) = req.output_name {
        validate_output_name(out_name)?;
    }
    Ok(req)
}

pub fn generate_diffusion_config_yaml(job_id: &str, req: &DiffusionJobRequest) -> String {
    let output_name_line = match &req.output_name {
        Some(name) if !name.trim().is_empty() => {
            format!(
                "output_name: {}\n",
                serde_json::to_string(name.trim()).unwrap_or_else(|_| "\"\"".into())
            )
        }
        _ => String::new(),
    };
    let weights_line = if req.weights.is_some() {
        "weights_path: \"{weights_path}\"\n".to_string()
    } else {
        String::new()
    };
    let epoch_offset_line = match req.epoch_offset {
        Some(offset) => format!("epoch_offset: {}\n", offset),
        None => String::new(),
    };
    let trigger_line = match &req.trigger_word {
        Some(tw) => format!(
            "  trigger_word: {}\n",
            serde_json::to_string(tw).unwrap_or_else(|_| "\"\"".into())
        ),
        None => "  trigger_word: \"\"\n".to_string(),
    };
    let resolution_line = match req.resolution {
        Some(r) => format!("  resolution: {}\n", r),
        None => String::new(),
    };
    let samples_section = match &req.sample_prompt {
        Some(sp) if !sp.trim().is_empty() => {
            let seed_line = match req.sample_seed {
                Some(s) => format!("  seed: {}\n", s),
                None => "  seed: 42\n".to_string(),
            };
            format!(
                "samples:\n  prompt: {}\n  interval: {}\n{seed_line}",
                serde_json::to_string(sp.trim()).unwrap_or_else(|_| "\"\"".into()),
                req.sample_interval
            )
        }
        _ => "".to_string(),
    };
    format!(
        r#"# Configuração de treino Difusão LoRA (gerada pelo api-principal)
job_id: "{job_id}"
engine: "diffusion"
model: "{base_model}"
{output_name_line}{weights_line}{epoch_offset_line}mode: "train"
dataset_path: "{{dataset_path}}"
output_path: "{{output_path}}"
seed: 42
checkpoint_interval: {checkpoint_interval}
lora:
{trigger_line}  epochs: {epochs}
  batch_size: {batch_size}
  learning_rate: {learning_rate}
  rank: {rank}
  alpha: {alpha}
{resolution_line}  gradient_accumulation_steps: {grad_accum}
  optimizer: "{optimizer}"
  lr_scheduler: "{lr_scheduler}"
  lr_warmup_steps: {lr_warmup_steps}
  mixed_precision: "{mixed_precision}"
  quantization: "{quantization}"
  checkpoint_interval: {checkpoint_interval}
{samples_section}"#,
        job_id = job_id,
        base_model = req.base_model,
        output_name_line = output_name_line,
        weights_line = weights_line,
        epoch_offset_line = epoch_offset_line,
        checkpoint_interval = req.checkpoint_interval,
        trigger_line = trigger_line,
        epochs = req.epochs,
        batch_size = req.batch_size,
        learning_rate = req.learning_rate,
        rank = req.rank,
        alpha = req.alpha,
        resolution_line = resolution_line,
        grad_accum = req.gradient_accumulation_steps,
        optimizer = req.optimizer,
        lr_scheduler = req.lr_scheduler,
        lr_warmup_steps = req.lr_warmup_steps,
        mixed_precision = req.mixed_precision,
        quantization = req.quantization,
        samples_section = samples_section,
    )
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiffusionGenerateJobRequest {
    #[serde(default = "default_diffusion_generate_base_model")]
    pub base_model: String,
    pub prompt: String,
    pub negative_prompt: Option<String>,
    #[serde(default = "default_diffusion_generate_dimension")]
    pub width: u32,
    #[serde(default = "default_diffusion_generate_dimension")]
    pub height: u32,
    #[serde(default = "default_diffusion_generate_steps")]
    pub steps: u32,
    #[serde(default = "default_diffusion_generate_guidance")]
    pub guidance_scale: f64,
    pub seed: Option<u64>,
    #[serde(default = "default_diffusion_quantization")]
    pub quantization: String,
    #[serde(default)]
    pub distilled: bool,
    pub weights: Option<String>,
    #[serde(default = "default_diffusion_lora_scale")]
    pub lora_scale: f64,
    pub orchestrator_id: Option<String>,
}

fn default_diffusion_generate_base_model() -> String {
    "flux-2-klein-4b".to_string()
}
fn default_diffusion_generate_dimension() -> u32 {
    1024
}
fn default_diffusion_generate_steps() -> u32 {
    20
}
fn default_diffusion_generate_guidance() -> f64 {
    3.5
}
fn default_diffusion_lora_scale() -> f64 {
    1.0
}

pub fn validate_diffusion_generate_request(
    req: DiffusionGenerateJobRequest,
) -> Result<DiffusionGenerateJobRequest, String> {
    if !ALLOWED_DIFFUSION_BASE_MODELS.contains(&req.base_model.as_str()) {
        return Err(format!(
            "baseModel must be one of {:?}, got '{}'",
            ALLOWED_DIFFUSION_BASE_MODELS, req.base_model
        ));
    }
    if req.prompt.trim().is_empty() {
        return Err("prompt cannot be empty".to_string());
    }
    if req.prompt.chars().count() > 4000 {
        return Err("prompt cannot exceed 4000 characters".to_string());
    }
    if let Some(ref neg) = req.negative_prompt {
        if neg.chars().count() > 4000 {
            return Err("negativePrompt cannot exceed 4000 characters".to_string());
        }
    }
    if !(256..=2048).contains(&req.width) {
        return Err(format!(
            "width must be between 256 and 2048, got {}",
            req.width
        ));
    }
    if !(256..=2048).contains(&req.height) {
        return Err(format!(
            "height must be between 256 and 2048, got {}",
            req.height
        ));
    }
    if !(1..=100).contains(&req.steps) {
        return Err(format!(
            "steps must be between 1 and 100, got {}",
            req.steps
        ));
    }
    if !(1.0..=30.0).contains(&req.guidance_scale) || req.guidance_scale.is_nan() {
        return Err(format!(
            "guidanceScale must be between 1.0 and 30.0, got {}",
            req.guidance_scale
        ));
    }
    if !ALLOWED_DIFFUSION_QUANTIZATIONS.contains(&req.quantization.as_str()) {
        return Err(format!(
            "quantization must be one of {:?}, got '{}'",
            ALLOWED_DIFFUSION_QUANTIZATIONS, req.quantization
        ));
    }
    if !(0.0..=2.0).contains(&req.lora_scale) || req.lora_scale.is_nan() {
        return Err(format!(
            "loraScale must be between 0.0 and 2.0, got {}",
            req.lora_scale
        ));
    }
    if let Some(ref w) = req.weights {
        if uuid::Uuid::parse_str(w).is_err() {
            return Err("weights must be a valid UUID".to_string());
        }
    }
    if let Some(ref o) = req.orchestrator_id {
        if uuid::Uuid::parse_str(o).is_err() {
            return Err("orchestratorId must be a valid UUID".to_string());
        }
    }
    Ok(req)
}

pub fn generate_diffusion_generate_config_yaml(
    job_id: &str,
    req: &DiffusionGenerateJobRequest,
) -> String {
    let neg_line = match &req.negative_prompt {
        Some(neg) => format!(
            "  negative_prompt: {}\n",
            serde_json::to_string(neg).unwrap_or_else(|_| "\"\"".into())
        ),
        None => "  negative_prompt: \"\"\n".to_string(),
    };
    let seed = req.seed.unwrap_or(42);
    format!(
        r#"# Configuração de geração Difusão (Playground)
job_id: "{job_id}"
engine: "diffusion"
model: "{base_model}"
mode: "generate"
output_path: "{{output_path}}"
seed: {seed}
weights_path: "{{weights_path}}"
generate:
  base_model: "{base_model}"
  prompt: {prompt_json}
{neg_line}  width: {width}
  height: {height}
  steps: {steps}
  guidance_scale: {guidance_scale}
  seed: {seed}
  quantization: "{quantization}"
  distilled: {distilled}
  lora_scale: {lora_scale}
"#,
        job_id = job_id,
        base_model = req.base_model,
        seed = seed,
        prompt_json = serde_json::to_string(&req.prompt).unwrap_or_else(|_| "\"\"".into()),
        neg_line = neg_line,
        width = req.width,
        height = req.height,
        steps = req.steps,
        guidance_scale = req.guidance_scale,
        quantization = req.quantization,
        distilled = req.distilled,
        lora_scale = req.lora_scale,
    )
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutolabelApplyItem {
    pub filename: String,
    pub caption: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutolabelApplyRequest {
    #[serde(default)]
    pub dataset_id: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
    #[serde(default)]
    pub items: Option<Vec<AutolabelApplyItem>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutolabelPreviewItem {
    pub image_id: uuid::Uuid,
    pub filename: String,
    pub image_url: String,
    pub generated_caption: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_caption: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_origin: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutolabelPreviewResponse {
    pub job_id: uuid::Uuid,
    pub dataset_id: uuid::Uuid,
    pub model: Option<String>,
    pub total_generated: i64,
    pub items: Vec<AutolabelPreviewItem>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutolabelApplyResponse {
    pub applied: i64,
    pub skipped: i64,
    pub images: i64,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CaptionsJsonlItem {
    pub filename: String,
    pub caption: String,
}

pub fn parse_captions_jsonl(bytes: &[u8]) -> Result<Vec<CaptionsJsonlItem>, ()> {
    let text = std::str::from_utf8(bytes).map_err(|_| ())?;
    let mut items = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let item: CaptionsJsonlItem = serde_json::from_str(trimmed).map_err(|_| ())?;
        items.push(item);
    }
    Ok(items)
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
        // Sem weights ⇒ sem weights_path.
        assert!(!yaml.contains("weights_path"));

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
        assert!(!yaml.contains("weights_path"));
    }

    // --- weights (D5 ADR-0012) ---

    #[test]
    fn weights_none_by_default() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000"}"#;
        let req: YoloJobRequest = serde_json::from_str(raw).expect("parse");
        assert!(req.weights.is_none());
    }

    #[test]
    fn weights_valid_uuid() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000","weights":"550e8400-e29b-41d4-a716-446655440000"}"#;
        let req: YoloJobRequest = serde_json::from_str(raw).expect("parse");
        assert_eq!(
            req.weights.as_deref(),
            Some("550e8400-e29b-41d4-a716-446655440000")
        );
        assert!(validate_yolo_request(req).is_ok());
    }

    #[test]
    fn weights_non_uuid_rejected() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000","weights":"not-a-uuid"}"#;
        let req: YoloJobRequest = serde_json::from_str(raw).expect("parse");
        assert!(validate_yolo_request(req).is_err());
    }

    #[test]
    fn config_yaml_with_weights() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000","weights":"550e8400-e29b-41d4-a716-446655440000"}"#;
        let req: YoloJobRequest = serde_json::from_str(raw).expect("parse");
        let yaml = generate_config_yaml("job-weights-001", &req);

        // weights_path presente com placeholder.
        assert!(yaml.contains("weights_path: \"{weights_path}\""));
        // Outros placeholders e defaults intactos.
        assert!(yaml.contains("{dataset_path}"));
        assert!(yaml.contains("{output_path}"));
        assert!(yaml.contains("model: \"yolo11m\""));
        assert!(yaml.contains("engine: \"yolo\""));
    }

    // =========================================================================
    // AutoTracker (ADR-0008 D3) tests
    // =========================================================================

    #[test]
    fn autotrack_defaults_apply() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000"}"#;
        let req: AutotrackerJobRequest = serde_json::from_str(raw).expect("parse");
        assert_eq!(req.model, "mock");
        assert!((req.conf - 0.65).abs() < 1e-10);
    }

    #[test]
    fn autotrack_custom_values() {
        let raw =
            r#"{"datasetId":"00000000-0000-0000-0000-000000000000","model":"mock","conf":0.8}"#;
        let req: AutotrackerJobRequest = serde_json::from_str(raw).expect("parse");
        assert_eq!(req.model, "mock");
        assert!((req.conf - 0.8).abs() < 1e-10);
    }

    #[test]
    fn autotrack_deny_unknown_fields() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000","extra":1}"#;
        let err = serde_json::from_str::<AutotrackerJobRequest>(raw);
        assert!(err.is_err(), "deny_unknown_fields");
    }

    #[test]
    fn autotrack_invalid_model() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000","model":"resnet50"}"#;
        let req: AutotrackerJobRequest = serde_json::from_str(raw).expect("parse");
        assert!(validate_autotrack_request(req).is_err());
    }

    #[test]
    fn autotrack_valid_model_mock() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000","model":"mock"}"#;
        let req: AutotrackerJobRequest = serde_json::from_str(raw).expect("parse");
        assert!(validate_autotrack_request(req).is_ok());
    }

    #[test]
    fn autotrack_conf_boundary() {
        // Dentro do domínio 0..=1
        for c in [0.0, 0.5, 1.0] {
            let raw =
                format!(r#"{{"datasetId":"00000000-0000-0000-0000-000000000000","conf":{c}}}"#);
            let req: AutotrackerJobRequest = serde_json::from_str(&raw).expect("parse");
            assert!(validate_autotrack_request(req).is_ok(), "conf={c}");
        }
        // Fora do domínio
        for c in [-0.1, 1.1] {
            let raw =
                format!(r#"{{"datasetId":"00000000-0000-0000-0000-000000000000","conf":{c}}}"#);
            let req: AutotrackerJobRequest = serde_json::from_str(&raw).expect("parse");
            assert!(validate_autotrack_request(req).is_err(), "conf={c}");
        }
    }

    #[test]
    fn autotrack_config_yaml_placeholders_and_defaults() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000"}"#;
        let req: AutotrackerJobRequest = serde_json::from_str(raw).expect("parse");
        let yaml = generate_autotrack_config_yaml("test-job-001", &req);

        // Placeholders presentes.
        assert!(yaml.contains("{dataset_path}"));
        assert!(yaml.contains("{output_path}"));

        // Defaults corretos.
        assert!(yaml.contains("model: \"mock\""));
        assert!(yaml.contains("conf: 0.65"));
        assert!(yaml.contains("seed: 42"));
        assert!(yaml.contains("job_id: \"test-job-001\""));
        assert!(yaml.contains("engine: \"autotracker\""));
        assert!(yaml.contains("mode: \"autotrack\""));
        assert!(yaml.contains("autotrack:"));

        // Parseável como YAML.
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("yaml parse");
        assert_eq!(parsed["autotrack"]["conf"].as_f64().unwrap(), 0.65);
        assert_eq!(parsed["engine"].as_str().unwrap(), "autotracker");
    }

    #[test]
    fn autotrack_config_yaml_custom_conf() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000","conf":0.9}"#;
        let req: AutotrackerJobRequest = serde_json::from_str(raw).expect("parse");
        let yaml = generate_autotrack_config_yaml("job-xyz", &req);

        assert!(yaml.contains("conf: 0.9"));
        assert!(yaml.contains("engine: \"autotracker\""));
        assert!(yaml.contains("mode: \"autotrack\""));
    }

    // =========================================================================
    // AutoTracker modelId (ADR-0014 D2/D6) tests
    // =========================================================================

    #[test]
    fn autotrack_model_id_none_by_default() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000"}"#;
        let req: AutotrackerJobRequest = serde_json::from_str(raw).expect("parse");
        assert!(req.model_id.is_none());
    }

    #[test]
    fn autotrack_model_id_valid_uuid() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000","modelId":"550e8400-e29b-41d4-a716-446655440000"}"#;
        let req: AutotrackerJobRequest = serde_json::from_str(raw).expect("parse");
        assert_eq!(
            req.model_id.as_deref(),
            Some("550e8400-e29b-41d4-a716-446655440000")
        );
        assert!(validate_autotrack_request(req).is_ok());
    }

    #[test]
    fn autotrack_model_id_not_uuid_rejected() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000","modelId":"not-a-uuid"}"#;
        let req: AutotrackerJobRequest = serde_json::from_str(raw).expect("parse");
        assert!(validate_autotrack_request(req).is_err());
    }

    #[test]
    fn autotrack_config_yaml_with_model_id() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000","modelId":"550e8400-e29b-41d4-a716-446655440000"}"#;
        let req: AutotrackerJobRequest = serde_json::from_str(raw).expect("parse");
        let yaml = generate_autotrack_config_yaml("test-job-real-001", &req);

        // weights_path presente com placeholder.
        assert!(yaml.contains("weights_path: \"{weights_path}\""));
        // autotrack.model = "world" (não "mock").
        assert!(yaml.contains("autotrack:"));
        assert!(yaml.contains("model: \"world\""));
        // Top-level model permanece "mock" (derivação interna).
        assert!(yaml.contains("engine: \"autotracker\""));
        assert!(yaml.contains("mode: \"autotrack\""));
        assert!(yaml.contains("conf: 0.65"));
        assert!(yaml.contains("seed: 42"));

        // Parseável como YAML.
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("yaml parse");
        assert_eq!(parsed["autotrack"]["model"].as_str().unwrap(), "world");
        assert_eq!(parsed["engine"].as_str().unwrap(), "autotracker");
    }

    #[test]
    fn autotrack_config_yaml_without_model_id_unchanged() {
        // Sem modelId → config byte a byte igual ao comportamento atual (mock).
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000000"}"#;
        let req: AutotrackerJobRequest = serde_json::from_str(raw).expect("parse");
        let yaml = generate_autotrack_config_yaml("test-job-mock-001", &req);

        // Sem weights_path.
        assert!(!yaml.contains("weights_path"));
        // autotrack.model = "mock" (default).
        assert!(yaml.contains("model: \"mock\""));
        assert!(yaml.contains("engine: \"autotracker\""));
        assert!(yaml.contains("mode: \"autotrack\""));
    }

    // =========================================================================
    // AutoTracker apply (ADR-0008 D1) — AutotrackerApplyRequest tests
    // =========================================================================

    #[test]
    fn apply_defaults_apply() {
        let raw = r#"{}"#;
        let req: AutotrackerApplyRequest = serde_json::from_str(raw).expect("parse");
        assert!(!req.overwrite);
        assert!(req.image_id.is_none());
    }

    #[test]
    fn apply_custom_values() {
        let raw = r#"{"overwrite":true,"imageId":"00000000-0000-0000-0000-000000000000"}"#;
        let req: AutotrackerApplyRequest = serde_json::from_str(raw).expect("parse");
        assert!(req.overwrite);
        assert!(req.image_id.is_some());
    }

    #[test]
    fn apply_deny_unknown_fields() {
        let raw = r#"{"extra":1}"#;
        let err = serde_json::from_str::<AutotrackerApplyRequest>(raw);
        assert!(err.is_err(), "deny_unknown_fields");
    }

    // =========================================================================
    // parse_boxes_json tests (ADR-0008 D1)
    // =========================================================================

    fn sample_boxes_json() -> &'static [u8] {
        br#"{"engine":"autotracker","model":"mock","seed":42,"conf":0.65,"images":[{"filename":"img_0001.jpg","boxes":[{"class":"solda_fria","x":0.1,"y":0.2,"w":0.3,"h":0.4,"conf":0.96}]}]}"#
    }

    #[test]
    fn parse_boxes_json_ok() {
        let art = parse_boxes_json(sample_boxes_json()).expect("parse ok");
        assert_eq!(art.engine, "autotracker");
        assert_eq!(art.model, "mock");
        assert_eq!(art.seed, 42);
        assert!((art.conf - 0.65).abs() < 1e-10);
        assert_eq!(art.images.len(), 1);
        assert_eq!(art.images[0].filename, "img_0001.jpg");
        assert_eq!(art.images[0].boxes.len(), 1);
        assert_eq!(art.images[0].boxes[0].class, "solda_fria");
    }

    #[test]
    fn parse_boxes_json_invalid_json() {
        let err = parse_boxes_json(b"not json");
        assert!(err.is_err());
    }

    #[test]
    fn parse_boxes_json_empty_images_ok() {
        let data = br#"{"engine":"autotracker","model":"mock","seed":1,"conf":0.5,"images":[]}"#;
        let art = parse_boxes_json(data).expect("empty images ok");
        assert!(art.images.is_empty());
    }

    #[test]
    fn parse_boxes_json_duplicate_filename() {
        let data = br#"{"engine":"autotracker","model":"mock","seed":1,"conf":0.5,"images":[{"filename":"a.jpg","boxes":[]},{"filename":"a.jpg","boxes":[]}]}"#;
        assert!(parse_boxes_json(data).is_err());
    }

    #[test]
    fn parse_boxes_json_empty_filename() {
        let data = br#"{"engine":"autotracker","model":"mock","seed":1,"conf":0.5,"images":[{"filename":"","boxes":[]}]}"#;
        assert!(parse_boxes_json(data).is_err());
    }

    #[test]
    fn parse_boxes_json_empty_class() {
        let data = br#"{"engine":"autotracker","model":"mock","seed":1,"conf":0.5,"images":[{"filename":"a.jpg","boxes":[{"class":"","x":0.1,"y":0.2,"w":0.3,"h":0.4,"conf":0.9}]}]}"#;
        assert!(parse_boxes_json(data).is_err());
    }

    #[test]
    fn parse_boxes_json_coord_out_of_range() {
        let data = br#"{"engine":"autotracker","model":"mock","seed":1,"conf":0.5,"images":[{"filename":"a.jpg","boxes":[{"class":"c","x":1.5,"y":0.2,"w":0.3,"h":0.4,"conf":0.9}]}]}"#;
        assert!(parse_boxes_json(data).is_err());
    }

    #[test]
    fn parse_boxes_json_conf_out_of_range() {
        let data = br#"{"engine":"autotracker","model":"mock","seed":1,"conf":0.5,"images":[{"filename":"a.jpg","boxes":[{"class":"c","x":0.1,"y":0.2,"w":0.3,"h":0.4,"conf":1.5}]}]}"#;
        assert!(parse_boxes_json(data).is_err());
    }

    #[test]
    fn parse_boxes_json_boundary_coords() {
        // 0.0 e 1.0 devem ser aceitos.
        let data = br#"{"engine":"autotracker","model":"mock","seed":1,"conf":0.5,"images":[{"filename":"a.jpg","boxes":[{"class":"c","x":0.0,"y":1.0,"w":0.5,"h":0.5,"conf":0.0},{"class":"d","x":1.0,"y":0.0,"w":1.0,"h":1.0,"conf":1.0}]}]}"#;
        let art = parse_boxes_json(data).expect("boundary ok");
        assert_eq!(art.images[0].boxes.len(), 2);
    }

    #[test]
    fn resolve_class_ids_ok() {
        let classes = vec![
            (uuid::Uuid::nil(), "solda_fria".into()),
            (uuid::Uuid::new_v4(), "good_weld".into()),
        ];
        let map = resolve_class_ids(&classes);
        assert_eq!(map.len(), 2);
        assert_eq!(map["solda_fria"], uuid::Uuid::nil());
    }

    #[test]
    fn match_class_id_cases() {
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        let classes = vec![
            (id1, "armpits_exposed".into()),
            (id2, "female breast exposed".into()),
        ];
        let exact_map = resolve_class_ids(&classes);
        let lower_map: std::collections::HashMap<String, uuid::Uuid> = classes
            .iter()
            .map(|(id, name)| (name.to_lowercase(), *id))
            .collect();

        // Exact match
        assert_eq!(
            match_class_id("armpits_exposed", &exact_map, &lower_map),
            Some(&id1)
        );
        // Case-insensitive (UPPERCASE de modelos como NudeNet)
        assert_eq!(
            match_class_id("ARMPITS_EXPOSED", &exact_map, &lower_map),
            Some(&id1)
        );
        // Space to underscore
        assert_eq!(
            match_class_id("FEMALE_BREAST_EXPOSED", &exact_map, &lower_map),
            Some(&id2)
        );
        // Inexistente
        assert_eq!(match_class_id("FACE_FEMALE", &exact_map, &lower_map), None);
    }

    // =========================================================================
    // Predict (Fatia J — ADR-0013 D0/D1/D8) tests
    // =========================================================================

    #[test]
    fn predict_defaults_apply() {
        let raw = r#"{"modelId":"00000000-0000-0000-0000-000000000000","datasetId":"00000000-0000-0000-0000-000000000001"}"#;
        let req: PredictJobRequest = serde_json::from_str(raw).expect("parse");
        assert!((req.conf - 0.65).abs() < 1e-10);
        assert_eq!(req.model_id, "00000000-0000-0000-0000-000000000000");
        assert_eq!(req.dataset_id, "00000000-0000-0000-0000-000000000001");
    }

    #[test]
    fn predict_custom_values() {
        let raw = r#"{"modelId":"550e8400-e29b-41d4-a716-446655440000","datasetId":"550e8400-e29b-41d4-a716-446655440001","conf":0.9}"#;
        let req: PredictJobRequest = serde_json::from_str(raw).expect("parse");
        assert!((req.conf - 0.9).abs() < 1e-10);
    }

    #[test]
    fn predict_deny_unknown_fields() {
        let raw = r#"{"modelId":"00000000-0000-0000-0000-000000000000","datasetId":"00000000-0000-0000-0000-000000000001","extra":1}"#;
        let err = serde_json::from_str::<PredictJobRequest>(raw);
        assert!(err.is_err(), "deny_unknown_fields");
    }

    #[test]
    fn predict_missing_model_id() {
        let raw = r#"{"datasetId":"00000000-0000-0000-0000-000000000001"}"#;
        let err = serde_json::from_str::<PredictJobRequest>(raw);
        assert!(err.is_err(), "missing model_id");
    }

    #[test]
    fn predict_missing_dataset_id() {
        let raw = r#"{"modelId":"00000000-0000-0000-0000-000000000000"}"#;
        let err = serde_json::from_str::<PredictJobRequest>(raw);
        assert!(err.is_err(), "missing dataset_id");
    }

    #[test]
    fn predict_validate_ok() {
        let raw = r#"{"modelId":"550e8400-e29b-41d4-a716-446655440000","datasetId":"550e8400-e29b-41d4-a716-446655440001"}"#;
        let req: PredictJobRequest = serde_json::from_str(raw).expect("parse");
        assert!(validate_predict_request(req).is_ok());
    }

    #[test]
    fn predict_validate_model_id_not_uuid() {
        let raw = r#"{"modelId":"not-a-uuid","datasetId":"550e8400-e29b-41d4-a716-446655440001"}"#;
        let req: PredictJobRequest = serde_json::from_str(raw).expect("parse");
        assert!(validate_predict_request(req).is_err());
    }

    #[test]
    fn predict_conf_boundary() {
        // Dentro do domínio 0..=1
        for c in [0.0, 0.5, 1.0] {
            let raw = format!(
                r#"{{"modelId":"550e8400-e29b-41d4-a716-446655440000","datasetId":"550e8400-e29b-41d4-a716-446655440001","conf":{c}}}"#
            );
            let req: PredictJobRequest = serde_json::from_str(&raw).expect("parse");
            assert!(validate_predict_request(req).is_ok(), "conf={c}");
        }
        // Fora do domínio
        for c in [-0.1, 1.1] {
            let raw = format!(
                r#"{{"modelId":"550e8400-e29b-41d4-a716-446655440000","datasetId":"550e8400-e29b-41d4-a716-446655440001","conf":{c}}}"#
            );
            let req: PredictJobRequest = serde_json::from_str(&raw).expect("parse");
            assert!(validate_predict_request(req).is_err(), "conf={c}");
        }
    }

    #[test]
    fn predict_config_yaml_placeholders_and_defaults() {
        let raw = r#"{"modelId":"550e8400-e29b-41d4-a716-446655440000","datasetId":"550e8400-e29b-41d4-a716-446655440001"}"#;
        let req: PredictJobRequest = serde_json::from_str(raw).expect("parse");
        let yaml = generate_predict_config_yaml("test-predict-001", &req);

        // Placeholders presentes.
        assert!(yaml.contains("{dataset_path}"));
        assert!(yaml.contains("{output_path}"));
        assert!(yaml.contains("{weights_path}"));

        // Defaults corretos.
        assert!(yaml.contains("model: \"predict\""));
        assert!(yaml.contains("engine: \"yolo\""));
        assert!(yaml.contains("mode: \"predict\""));
        assert!(yaml.contains("conf: 0.65"));
        assert!(yaml.contains("seed: 42"));
        assert!(yaml.contains("job_id: \"test-predict-001\""));
        assert!(yaml.contains("predict:"));

        // Parseável como YAML.
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("yaml parse");
        assert_eq!(parsed["predict"]["conf"].as_f64().unwrap(), 0.65);
        assert_eq!(parsed["engine"].as_str().unwrap(), "yolo");
    }

    #[test]
    fn predict_config_yaml_custom_conf() {
        let raw = r#"{"modelId":"550e8400-e29b-41d4-a716-446655440000","datasetId":"550e8400-e29b-41d4-a716-446655440001","conf":0.9}"#;
        let req: PredictJobRequest = serde_json::from_str(raw).expect("parse");
        let yaml = generate_predict_config_yaml("job-predict-xyz", &req);

        assert!(yaml.contains("conf: 0.9"));
        assert!(yaml.contains("engine: \"yolo\""));
        assert!(yaml.contains("mode: \"predict\""));
        assert!(yaml.contains("weights_path: \"{weights_path}\""));
    }

    #[test]
    fn orchestrator_id_validation_in_all_job_requests() {
        let valid_oid = "550e8400-e29b-41d4-a716-446655440002";
        let invalid_oid = "not-a-uuid";

        // YoloJobRequest
        let raw_yolo_ok = format!(
            r#"{{"datasetId":"550e8400-e29b-41d4-a716-446655440001","epochs":1,"batch":16,"model":"yolo11n","orchestratorId":"{valid_oid}"}}"#
        );
        let req_yolo_ok: YoloJobRequest = serde_json::from_str(&raw_yolo_ok).unwrap();
        assert!(validate_yolo_request(req_yolo_ok).is_ok());

        let raw_yolo_bad = format!(
            r#"{{"datasetId":"550e8400-e29b-41d4-a716-446655440001","epochs":1,"batch":16,"model":"yolo11n","orchestratorId":"{invalid_oid}"}}"#
        );
        let req_yolo_bad: YoloJobRequest = serde_json::from_str(&raw_yolo_bad).unwrap();
        assert!(validate_yolo_request(req_yolo_bad).is_err());

        // AutotrackerJobRequest
        let raw_auto_ok = format!(
            r#"{{"datasetId":"550e8400-e29b-41d4-a716-446655440001","orchestratorId":"{valid_oid}"}}"#
        );
        let req_auto_ok: AutotrackerJobRequest = serde_json::from_str(&raw_auto_ok).unwrap();
        assert!(validate_autotrack_request(req_auto_ok).is_ok());

        let raw_auto_bad = format!(
            r#"{{"datasetId":"550e8400-e29b-41d4-a716-446655440001","orchestratorId":"{invalid_oid}"}}"#
        );
        let req_auto_bad: AutotrackerJobRequest = serde_json::from_str(&raw_auto_bad).unwrap();
        assert!(validate_autotrack_request(req_auto_bad).is_err());

        // AutolabelJobRequest
        let raw_al_ok = format!(
            r#"{{"datasetId":"550e8400-e29b-41d4-a716-446655440001","orchestratorId":"{valid_oid}"}}"#
        );
        let req_al_ok: AutolabelJobRequest = serde_json::from_str(&raw_al_ok).unwrap();
        assert!(validate_autolabel_request(req_al_ok).is_ok());

        let raw_al_bad = format!(
            r#"{{"datasetId":"550e8400-e29b-41d4-a716-446655440001","orchestratorId":"{invalid_oid}"}}"#
        );
        let req_al_bad: AutolabelJobRequest = serde_json::from_str(&raw_al_bad).unwrap();
        assert!(validate_autolabel_request(req_al_bad).is_err());

        // PredictJobRequest
        let raw_pred_ok = format!(
            r#"{{"modelId":"550e8400-e29b-41d4-a716-446655440000","datasetId":"550e8400-e29b-41d4-a716-446655440001","orchestratorId":"{valid_oid}"}}"#
        );
        let req_pred_ok: PredictJobRequest = serde_json::from_str(&raw_pred_ok).unwrap();
        assert!(validate_predict_request(req_pred_ok).is_ok());

        let raw_pred_bad = format!(
            r#"{{"modelId":"550e8400-e29b-41d4-a716-446655440000","datasetId":"550e8400-e29b-41d4-a716-446655440001","orchestratorId":"{invalid_oid}"}}"#
        );
        let req_pred_bad: PredictJobRequest = serde_json::from_str(&raw_pred_bad).unwrap();
        assert!(validate_predict_request(req_pred_bad).is_err());
    }

    #[test]
    fn parse_captions_jsonl_roundtrip() {
        let data = br#"{"filename":"img1.jpg","caption":"legenda um"}
{"filename":"img2.jpg","caption":"legenda dois"}
"#;
        let items = parse_captions_jsonl(data).expect("parse ok");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].filename, "img1.jpg");
        assert_eq!(items[0].caption, "legenda um");
    }

    #[test]
    fn autolabel_v2_validation_models_and_openai() {
        // Modelos válidos
        for model in &["mock", "florence-2", "qwen2-vl", "openai"] {
            let json = format!(
                r#"{{"datasetId":"550e8400-e29b-41d4-a716-446655440001","model":"{model}"}}"#
            );
            let req: AutolabelJobRequest = serde_json::from_str(&json).unwrap();
            assert!(validate_autolabel_request(req).is_ok());
        }

        // Modelo inválido
        let bad_model =
            r#"{"datasetId":"550e8400-e29b-41d4-a716-446655440001","model":"unknown_vlm"}"#;
        let req: AutolabelJobRequest = serde_json::from_str(bad_model).unwrap();
        assert!(validate_autolabel_request(req).is_err());

        // OpenAI com apiBase válida
        let openai_ok = r#"{
            "datasetId":"550e8400-e29b-41d4-a716-446655440001",
            "model":"openai",
            "apiKey":"sk-test-12345",
            "apiBase":"https://api.openai.com/v1",
            "openaiModel":"gpt-4o-mini"
        }"#;
        let req_openai: AutolabelJobRequest = serde_json::from_str(openai_ok).unwrap();
        let validated = validate_autolabel_request(req_openai).expect("valid openai request");
        assert_eq!(validated.model, "openai");
        assert_eq!(validated.openai_model.as_deref(), Some("gpt-4o-mini"));

        // OpenAI com apiBase inválida (sem scheme)
        let openai_bad_scheme = r#"{
            "datasetId":"550e8400-e29b-41d4-a716-446655440001",
            "model":"openai",
            "apiBase":"ftp://invalid.host"
        }"#;
        let req_bad: AutolabelJobRequest = serde_json::from_str(openai_bad_scheme).unwrap();
        assert!(validate_autolabel_request(req_bad).is_err());

        // OpenAI com apiBase contendo aspas e trailing slash
        let openai_quoted = r#"{
            "datasetId":"550e8400-e29b-41d4-a716-446655440001",
            "model":"openai",
            "apiKey":"\"sk-test-quoted\"",
            "apiBase":"https://llama.felipecncloud.com/v1/\"",
            "openaiModel":"\"Qwen3.8-9B\""
        }"#;
        let req_quoted: AutolabelJobRequest = serde_json::from_str(openai_quoted).unwrap();
        let validated_quoted =
            validate_autolabel_request(req_quoted).expect("strips quotes successfully");
        assert_eq!(
            validated_quoted.api_base.as_deref(),
            Some("https://llama.felipecncloud.com/v1")
        );
        assert_eq!(validated_quoted.api_key.as_deref(), Some("sk-test-quoted"));
        assert_eq!(validated_quoted.openai_model.as_deref(), Some("Qwen3.8-9B"));

        // Geração do YAML
        let yaml = generate_autolabel_config_yaml("job-al-v2", &validated);
        assert!(yaml.contains("model: \"openai\""));
        assert!(yaml.contains("api_key: \"sk-test-12345\""));
        assert!(yaml.contains("api_base: \"https://api.openai.com/v1\""));
        assert!(yaml.contains("openai_model: \"gpt-4o-mini\""));
    }

    #[test]
    fn diffusion_validate_ok_and_defaults() {
        let json = r#"{"datasetId":"550e8400-e29b-41d4-a716-446655440001"}"#;
        let req: DiffusionJobRequest = serde_json::from_str(json).unwrap();
        let validated = validate_diffusion_request(req).expect("should validate");
        assert_eq!(validated.base_model, "sdxl");
        assert_eq!(validated.epochs, 10);
        assert_eq!(validated.batch_size, 1);
        assert_eq!(validated.rank, 16);
        assert_eq!(validated.alpha, 16);
        assert_eq!(validated.learning_rate, 0.0001);
        assert_eq!(validated.quantization, "4bit");
        assert!(validated.trigger_word.is_none());

        let yaml = generate_diffusion_config_yaml("job-123", &validated);
        assert!(yaml.contains(r#"engine: "diffusion""#));
        assert!(yaml.contains(r#"model: "sdxl""#));
        assert!(yaml.contains(r#"rank: 16"#));
        assert!(yaml.contains(r#"quantization: "4bit""#));
        assert!(!yaml.contains("output_name:"));

        // Com output_name explícito
        let mut with_name = validated.clone();
        with_name.output_name = Some("custom-lora-v1".to_string());
        let yaml2 = generate_diffusion_config_yaml("job-124", &with_name);
        assert!(yaml2.contains(r#"output_name: "custom-lora-v1""#));
    }

    #[test]
    fn diffusion_validate_invalid_base_model_400() {
        let json =
            r#"{"datasetId":"550e8400-e29b-41d4-a716-446655440001","baseModel":"unsupported"}"#;
        let req: DiffusionJobRequest = serde_json::from_str(json).unwrap();
        assert!(validate_diffusion_request(req).is_err());
    }

    #[test]
    fn diffusion_validate_invalid_epochs_and_rank() {
        let json = r#"{"datasetId":"550e8400-e29b-41d4-a716-446655440001","epochs":0}"#;
        let req: DiffusionJobRequest = serde_json::from_str(json).unwrap();
        assert!(validate_diffusion_request(req).is_err());

        let json2 = r#"{"datasetId":"550e8400-e29b-41d4-a716-446655440001","rank":200}"#;
        let req2: DiffusionJobRequest = serde_json::from_str(json2).unwrap();
        assert!(validate_diffusion_request(req2).is_err());

        let json3 =
            r#"{"datasetId":"550e8400-e29b-41d4-a716-446655440001","quantization":"invalid"}"#;
        let req3: DiffusionJobRequest = serde_json::from_str(json3).unwrap();
        assert!(validate_diffusion_request(req3).is_err());
    }

    #[test]
    fn diffusion_validate_advanced_params_and_yaml() {
        let json = r#"{
            "datasetId": "550e8400-e29b-41d4-a716-446655440001",
            "baseModel": "sdxl",
            "resolution": 1024,
            "gradientAccumulationSteps": 4,
            "optimizer": "adamw8bit",
            "lrScheduler": "cosine",
            "lrWarmupSteps": 50,
            "mixedPrecision": "bf16",
            "quantization": "8bit"
        }"#;
        let req: DiffusionJobRequest = serde_json::from_str(json).expect("should parse json");
        let validated = validate_diffusion_request(req).expect("should validate advanced params");
        assert_eq!(validated.resolution, Some(1024));
        assert_eq!(validated.gradient_accumulation_steps, 4);
        assert_eq!(validated.optimizer, "adamw8bit");
        assert_eq!(validated.lr_scheduler, "cosine");
        assert_eq!(validated.lr_warmup_steps, 50);
        assert_eq!(validated.mixed_precision, "bf16");
        assert_eq!(validated.quantization, "8bit");

        let yaml = generate_diffusion_config_yaml("job-adv-1", &validated);
        assert!(yaml.contains("resolution: 1024"));
        assert!(yaml.contains("gradient_accumulation_steps: 4"));
        assert!(yaml.contains(r#"optimizer: "adamw8bit""#));
        assert!(yaml.contains(r#"lr_scheduler: "cosine""#));
        assert!(yaml.contains("lr_warmup_steps: 50"));
        assert!(yaml.contains(r#"mixed_precision: "bf16""#));
        assert!(yaml.contains(r#"quantization: "8bit""#));
    }

    #[test]
    fn diffusion_validate_checkpoint_interval_and_epoch_offset_yaml() {
        let json = r#"{
            "datasetId": "550e8400-e29b-41d4-a716-446655440001",
            "baseModel": "flux",
            "checkpointInterval": 5,
            "epochOffset": 10,
            "weights": "550e8400-e29b-41d4-a716-446655440002"
        }"#;
        let req: DiffusionJobRequest = serde_json::from_str(json).expect("should parse json");
        let validated = validate_diffusion_request(req).expect("should validate");
        assert_eq!(validated.checkpoint_interval, 5);
        assert_eq!(validated.epoch_offset, Some(10));
        assert_eq!(
            validated.weights.as_deref(),
            Some("550e8400-e29b-41d4-a716-446655440002")
        );

        let yaml = generate_diffusion_config_yaml("job-resume-1", &validated);
        assert!(yaml.contains(r#"weights_path: "{weights_path}""#));
        assert!(yaml.contains("checkpoint_interval: 5"));
        assert!(yaml.contains("epoch_offset: 10"));

        // Intervalo inválido (0 ou > 100) deve falhar
        let bad_json = r#"{"datasetId":"550e8400-e29b-41d4-a716-446655440001","checkpointInterval":0}"#;
        let bad_req: DiffusionJobRequest = serde_json::from_str(bad_json).unwrap();
        assert!(validate_diffusion_request(bad_req).is_err());
    }

    #[test]
    fn autolabel_apply_request_with_curated_items() {
        let json = r#"{"datasetId":"550e8400-e29b-41d4-a716-446655440001","overwrite":true,"items":[{"filename":"photo.webp","caption":"a photo of a cat"}]}"#;
        let req: AutolabelApplyRequest = serde_json::from_str(json).expect("should parse");
        assert_eq!(
            req.dataset_id.as_deref(),
            Some("550e8400-e29b-41d4-a716-446655440001")
        );
        assert!(req.overwrite);
        let items = req.items.expect("items present");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].filename, "photo.webp");
        assert_eq!(items[0].caption, "a photo of a cat");

        // unknown fields deve falhar devido a deny_unknown_fields
        let bad_json = r#"{"datasetId":"550e8400-e29b-41d4-a716-446655440001","extra":123}"#;
        assert!(serde_json::from_str::<AutolabelApplyRequest>(bad_json).is_err());
    }

    #[test]
    fn autolabel_preview_response_serialization() {
        let item = AutolabelPreviewItem {
            image_id: uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440002").unwrap(),
            filename: "img1.png".into(),
            image_url: "https://minio.local/img1.png".into(),
            generated_caption: "a red sports car".into(),
            current_caption: Some("old caption".into()),
            current_origin: Some("manual".into()),
        };
        let resp = AutolabelPreviewResponse {
            job_id: uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440003").unwrap(),
            dataset_id: uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440004").unwrap(),
            model: Some("florence-2".into()),
            total_generated: 1,
            items: vec![item],
        };
        let serialized = serde_json::to_string(&resp).expect("serialize");
        assert!(serialized.contains("generatedCaption"));
        assert!(serialized.contains("currentCaption"));
        assert!(serialized.contains("totalGenerated"));
    }

    #[test]
    fn autolabel_request_selective_filters_validation() {
        let json_ok = r#"{
            "datasetId": "550e8400-e29b-41d4-a716-446655440000",
            "model": "openai",
            "filterClassId": "550e8400-e29b-41d4-a716-446655440001",
            "imageIds": [
                "550e8400-e29b-41d4-a716-446655440002",
                "550e8400-e29b-41d4-a716-446655440003"
            ]
        }"#;
        let req: AutolabelJobRequest = serde_json::from_str(json_ok).expect("parse selective ok");
        assert!(validate_autolabel_request(req).is_ok());

        // filterClassId não-uuid
        let json_bad_fc = r#"{
            "datasetId": "550e8400-e29b-41d4-a716-446655440000",
            "filterClassId": "not-a-uuid"
        }"#;
        let req_bad_fc: AutolabelJobRequest = serde_json::from_str(json_bad_fc).unwrap();
        assert!(validate_autolabel_request(req_bad_fc).is_err());

        // imageIds vazio
        let json_empty_ids = r#"{
            "datasetId": "550e8400-e29b-41d4-a716-446655440000",
            "imageIds": []
        }"#;
        let req_empty_ids: AutolabelJobRequest = serde_json::from_str(json_empty_ids).unwrap();
        assert!(validate_autolabel_request(req_empty_ids).is_err());

        // imageIds com item inválido
        let json_bad_item = r#"{
            "datasetId": "550e8400-e29b-41d4-a716-446655440000",
            "imageIds": ["not-a-uuid"]
        }"#;
        let req_bad_item: AutolabelJobRequest = serde_json::from_str(json_bad_item).unwrap();
        assert!(validate_autolabel_request(req_bad_item).is_err());
    }

    #[test]
    fn diffusion_generate_validate_ok_and_yaml() {
        let json = r#"{
            "prompt": "a stunning portrait in neon cyberpunk style",
            "negativePrompt": "blurry, distorted",
            "width": 1024,
            "height": 768,
            "steps": 25,
            "guidanceScale": 4.0,
            "seed": 99999,
            "quantization": "4bit",
            "loraScale": 0.9
        }"#;
        let req: DiffusionGenerateJobRequest = serde_json::from_str(json).unwrap();
        let validated = validate_diffusion_generate_request(req).expect("should validate");
        assert_eq!(validated.base_model, "flux-2-klein-4b");
        assert_eq!(validated.width, 1024);
        assert_eq!(validated.height, 768);
        assert_eq!(validated.steps, 25);
        assert_eq!(validated.guidance_scale, 4.0);
        assert_eq!(validated.seed, Some(99999));
        assert_eq!(validated.quantization, "4bit");
        assert_eq!(validated.lora_scale, 0.9);

        let yaml = generate_diffusion_generate_config_yaml("job-gen-001", &validated);
        assert!(yaml.contains(r#"mode: "generate""#));
        assert!(yaml.contains(r#"base_model: "flux-2-klein-4b""#));
        assert!(yaml.contains("a stunning portrait in neon cyberpunk style"));
        assert!(yaml.contains("negative_prompt: \"blurry, distorted\""));
        assert!(yaml.contains("width: 1024"));
        assert!(yaml.contains("height: 768"));
        assert!(yaml.contains("seed: 99999"));
        assert!(yaml.contains(r#"quantization: "4bit""#));

        // Prompt vazio deve falhar
        let bad_json = r#"{"prompt": "   "}"#;
        let bad_req: DiffusionGenerateJobRequest = serde_json::from_str(bad_json).unwrap();
        assert!(validate_diffusion_generate_request(bad_req).is_err());
    }
}
