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
const ALLOWED_DIFFUSION_RESOLUTIONS: &[u32] = &[256, 512, 768, 1024, 1280, 1328, 1536, 2048];
const ALLOWED_DIFFUSION_GRAD_ACCUM: &[u32] = &[1, 2, 4, 8];
const ALLOWED_DIFFUSION_OPTIMIZERS: &[&str] = &["adamw8bit", "adamw", "prodigy"];
const ALLOWED_DIFFUSION_LR_SCHEDULERS: &[&str] =
    &["cosine", "linear", "constant", "constant_with_warmup"];
const ALLOWED_DIFFUSION_PRECISION: &[&str] = &["fp16", "bf16", "no"];
const ALLOWED_DIFFUSION_QUANTIZATIONS: &[&str] = &[
    "none", "2bit", "4bit", "6bit", "8bit", "4bit-nf4", "8bit-bnb",
];
/// Samplers suportados no generate (wire camelCase `sampler`).
/// `default` = sampler padrão do engine para o arch (retrocompat).
const ALLOWED_DIFFUSION_SAMPLERS: &[&str] = &[
    "default",
    "euler",
    "euler_a",
    "heun",
    "dpmpp_2m",
    "dpmpp_2m_karras",
    "dpmpp_2m_sde",
    "dpmpp_2m_sde_karras",
    "dpmpp_sde",
    "ddim",
];
/// Samplers aceitos pela base `flux-2-klein-4b` (arch-aware: o motor Flux.2
/// só suporta estes; os demais → 400 com mensagem clara).
const ALLOWED_FLUX2_KLEIN_SAMPLERS: &[&str] = &["default", "euler", "heun"];
/// Modelos de upscale suportados no generate (`upscale.model`, default "4x").
const ALLOWED_DIFFUSION_UPSCALE_MODELS: &[&str] = &["4x", "ultrasharp", "siax"];

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiffusionJobRequest {
    pub dataset_id: String,
    /// `baseModel` deixou de ser required — XOR com `customModelId`.
    /// Se ambos ausentes, usa "sdxl" (default legado do treino).
    #[serde(default)]
    pub base_model: Option<String>,
    /// UUID de checkpoint custom (kind=checkpoint) como base do treino.
    /// XOR com `baseModel`. Placeholders literais no YAML, nunca id/path real.
    #[serde(default)]
    pub custom_model_id: Option<String>,
    /// UUID de text encoder custom (kind=text_encoder). Só tem efeito com
    /// arch flux-2-klein-4b; outro arch ⇒ 400.
    #[serde(default)]
    pub text_encoder_model_id: Option<String>,
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
    /// Bucketing por aspect ratio (ADR-0018): preserva a proporção das imagens.
    #[serde(default = "default_diffusion_enable_bucket")]
    pub enable_bucket: bool,
    /// Nome customizado opcional do modelo gerado (ADR-0022 D1).
    pub output_name: Option<String>,
    #[serde(default = "default_diffusion_checkpoint_interval")]
    pub checkpoint_interval: u32,
    #[serde(default)]
    pub epoch_offset: Option<u32>,
    /// Dataset de regularização/controle (treino Flux.2). `None` = sem controle.
    /// Wire camelCase `controlDatasetId`; quando `Some`, deve diferir do
    /// dataset principal e existir (ownership = existência, mesma guarda do
    /// principal — datasets não têm coluna de dono).
    #[serde(default)]
    pub control_dataset_id: Option<uuid::Uuid>,
    /// Cache de text embeddings no treino (default false).
    #[serde(default)]
    pub cache_text_embeddings: bool,
}

fn default_diffusion_checkpoint_interval() -> u32 {
    1
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

fn default_diffusion_enable_bucket() -> bool {
    true
}

pub fn validate_diffusion_request(
    mut req: DiffusionJobRequest,
) -> Result<DiffusionJobRequest, String> {
    // --- XOR baseModel/customModelId (contrato da fatia feat/pesos-custom-flux2) ---
    // Ambos presentes ⇒ 400; ambos ausentes ⇒ default legado "sdxl" (retrocompat).
    // UUIDs são validados aqui (formato); existência/kind/arch resolve no handler.
    let has_custom = req.custom_model_id.is_some();
    let has_base = req.base_model.is_some();
    if has_custom && has_base {
        return Err("use either baseModel or customModelId, not both".to_string());
    }
    if !has_custom && !has_base {
        req.base_model = Some("sdxl".to_string());
    }
    if let Some(bm) = &req.base_model {
        if !ALLOWED_DIFFUSION_BASE_MODELS.contains(&bm.as_str()) {
            return Err(format!(
                "baseModel must be one of {:?}, got '{}'",
                ALLOWED_DIFFUSION_BASE_MODELS, bm
            ));
        }
    }
    if let Some(cm) = &req.custom_model_id {
        if uuid::Uuid::parse_str(cm).is_err() {
            return Err("customModelId must be a valid UUID".to_string());
        }
    }
    if let Some(tm) = &req.text_encoder_model_id {
        if uuid::Uuid::parse_str(tm).is_err() {
            return Err("textEncoderModelId must be a valid UUID".to_string());
        }
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
    // Motor Flux.2: control dataset deve diferir do principal (a igualdade
    // seria regularizar o treino contra ele mesmo). Existência do control é
    // verificada no handler (mesma guarda do dataset principal).
    if let Some(control_id) = req.control_dataset_id {
        if let Ok(main_id) = uuid::Uuid::parse_str(&req.dataset_id) {
            if control_id == main_id {
                return Err("controlDatasetId must differ from datasetId".to_string());
            }
        }
    }
    Ok(req)
}

/// Gera `config.yaml` de treino Difusão LoRA (fatia feat/pesos-custom-flux2).
///
/// `custom_arch`: arch resolvido pelo handler a partir de `custom_model_id`
/// (None = treino sobre repo oficial). Quando Some: `model:` recebe o arch
/// resolvido + root-level `custom_checkpoint_path: "{custom_checkpoint_path}"`
/// (placeholder literal — o orchestrator substitui via weights_ref; NUNCA
/// emitir id/path real aqui). Quando `text_encoder_model_id` Some: root-level
/// `text_encoder_path: "{text_encoder_path}"` (mesmo mecanismo).
pub fn generate_diffusion_config_yaml(
    job_id: &str,
    req: &DiffusionJobRequest,
    custom_arch: Option<&str>,
) -> String {
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
    // Pesos custom: root-level, placeholder literal para staging do orchestrator.
    let custom_checkpoint_line = if custom_arch.is_some() {
        "custom_checkpoint_path: \"{custom_checkpoint_path}\"\n".to_string()
    } else {
        String::new()
    };
    let text_encoder_line = if req.text_encoder_model_id.is_some() {
        "text_encoder_path: \"{text_encoder_path}\"\n".to_string()
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
    let enable_bucket_line = if req.enable_bucket {
        "  enable_bucket: true\n".to_string()
    } else {
        "  enable_bucket: false\n".to_string()
    };
    let control_dataset_line = if req.control_dataset_id.is_some() {
        "control_dataset_path: \"{control_dataset_path}\"\n".to_string()
    } else {
        String::new()
    };
    let cache_text_embeddings_line = if req.cache_text_embeddings {
        "cache_text_embeddings: true\n".to_string()
    } else {
        "cache_text_embeddings: false\n".to_string()
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
    // `model:` = arch resolvido quando custom, senão base_model (default sdxl).
    let effective_model =
        custom_arch.unwrap_or_else(|| req.base_model.as_deref().unwrap_or("sdxl"));
    format!(
        r#"# Configuração de treino Difusão LoRA (gerada pelo api-principal)
job_id: "{job_id}"
engine: "diffusion"
model: "{base_model}"
{output_name_line}{weights_line}{custom_checkpoint_line}{text_encoder_line}{epoch_offset_line}mode: "train"
dataset_path: "{{dataset_path}}"
{control_dataset_line}output_path: "{{output_path}}"
seed: 42
checkpoint_interval: {checkpoint_interval}
{cache_text_embeddings_line}lora:
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
{enable_bucket_line}  checkpoint_interval: {checkpoint_interval}
{samples_section}"#,
        job_id = job_id,
        base_model = effective_model,
        output_name_line = output_name_line,
        weights_line = weights_line,
        custom_checkpoint_line = custom_checkpoint_line,
        text_encoder_line = text_encoder_line,
        epoch_offset_line = epoch_offset_line,
        control_dataset_line = control_dataset_line,
        cache_text_embeddings_line = cache_text_embeddings_line,
        checkpoint_interval = req.checkpoint_interval,
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
        enable_bucket_line = enable_bucket_line,
        samples_section = samples_section,
    )
}

/// Referência a um LoRA para geração multi-LoRA (ADR-0023 D3).
///
/// `model_id` é o UUID do modelo na tabela `models` (kind='lora', engine='diffusion').
/// `scale` é a escala do adaptador (0.0..=2.0).
/// Serializa como `camelCase` no wire (`modelId`, `scale`).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoraRef {
    pub model_id: String,
    pub scale: f64,
}

/// Request v2 de geração Text-to-Image (ADR-0023 D2/D3/D4).
///
/// Retrocompat: campos novos (`batch_size`, `loras`, `custom_model_id`) são
/// opcionais com defaults que reproduzem o comportamento legado.
/// `base_model` deixou de ser required — XOR com `custom_model_id`.
/// `weights`/`lora_scale` continuam aceitos (deprecated); enviar ambos com
/// `loras` não vazio → 400 `invalid_request`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiffusionGenerateJobRequest {
    /// `baseModel` deixou de ser required — XOR com `customModelId`.
    /// Se ambos ausentes, usa "flux-2-klein-4b" (default legado).
    pub base_model: Option<String>,
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
    /// UUID de pesos existentes (deprecated — use `loras`).
    pub weights: Option<String>,
    /// Escala do LoRA legado (deprecated — use `loras`).
    #[serde(default = "default_diffusion_lora_scale")]
    pub lora_scale: f64,
    pub orchestrator_id: Option<String>,
    /// Batch size (1..8, default 1 — D2).
    #[serde(default = "default_diffusion_generate_batch_size")]
    pub batch_size: i64,
    /// LoRAs a aplicar (max 4 — D3). Aplicação em ordem do array.
    #[serde(default)]
    pub loras: Vec<LoraRef>,
    /// UUID de modelo custom (checkpoint — D4). XOR com `baseModel`.
    pub custom_model_id: Option<String>,
    /// UUID de text encoder custom (kind=text_encoder — fatia
    /// feat/pesos-custom-flux2). Só tem efeito com arch flux-2-klein-4b.
    #[serde(default)]
    pub text_encoder_model_id: Option<String>,
    /// ID de input efêmero (`POST /api/generations/inputs`) p/ img2img.
    /// Mutuamente exclusivo com `init_generation_id`.
    #[serde(default)]
    pub init_image_id: Option<uuid::Uuid>,
    /// ID de geração existente da galeria p/ img2img (sem re-upload).
    /// Mutuamente exclusivo com `init_image_id`.
    #[serde(default)]
    pub init_generation_id: Option<uuid::Uuid>,
    /// Força da imagem inicial no img2img (0.05..=0.95).
    /// Só válida com um dos ids; ausente com id ⇒ null; default 0.6 aplicado no config_yaml/engine.
    #[serde(default)]
    pub init_strength: Option<f32>,
    /// Amostrador do scheduler de difusão (wire `sampler`, default "default").
    #[serde(default = "default_diffusion_sampler")]
    pub sampler: String,
    /// Upscale pós-geração via Real-ESRGAN (`upscale: {model, scale}` ou null).
    #[serde(default)]
    pub upscale: Option<DiffusionUpscale>,
}

/// Configuração de upscale pós-geração (Real-ESRGAN / variantes RRDBNet x4).
///
/// Wire camelCase (`model`, `scale`); `deny_unknown_fields` consistente com os
/// demais requests de difusão. `model` ausente ⇒ "4x" (retrocompat);
/// suportados: "4x" (RealESRGAN x4plus, generalista), "ultrasharp"
/// (4x-UltraSharp, preserva textura) e "siax" (4x_NMKD-Siax_200k).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiffusionUpscale {
    #[serde(default = "default_diffusion_upscale_model")]
    pub model: String,
    pub scale: u8,
}

/// Sampler default: o padrão do engine para o arch (retrocompat).
fn default_diffusion_sampler() -> String {
    "default".to_string()
}
/// Upscale model default: "4x" (retrocompat — RealESRGAN x4plus).
fn default_diffusion_upscale_model() -> String {
    "4x".to_string()
}
/// Batch size default: 1 (idêntico ao comportamento legado).
fn default_diffusion_generate_batch_size() -> i64 {
    1
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

/// Valida `POST /api/jobs/diffusion/generate` v2 (ADR-0023 D2/D3/D4).
///
/// Validações:
/// - XOR: exatamente um de `base_model` / `custom_model_id`; se ambos None →
///   default "flux-2-klein-4b" (retrocompat).
/// - `batch_size` 1..8.
/// - `loras.len` <= 4; cada `scale` 0.0..=2.0; cada `model_id` UUID válido.
/// - `custom_model_id` UUID válido quando Some.
/// - Se `loras` não vazio E (`weights` Some OU `lora_scale` fornecido) → 400
///   ("use loras ou weights, não ambos").
/// - Prompt <= 4000 chars; dimensões 256..2048; steps 1..100; guidance 1.0..30.0.
/// - Quantização: none/2bit/4bit/6bit/8bit + aliases legados 4bit-nf4/8bit-bnb
///   (qualquer um; runtime @gpu valida compatibilidade com arch na sessão GPU).
/// - Sampler: enum global [default, euler, euler_a, heun, dpmpp_2m,
///   dpmpp_2m_karras, dpmpp_2m_sde, dpmpp_2m_sde_karras, dpmpp_sde, ddim]
///   (default "default"); arch-aware: base flux-2-klein-4b aceita apenas
///   [default, euler, heun].
/// - Upscale opcional: `{model: "4x"|"ultrasharp"|"siax", scale: 2|4}` ou null
///   (`model` ausente ⇒ "4x"; RRDBNet x4 em todos).
/// - Se `custom_model_id` Some → quantization "none" é PERMITIDO (S1 passou;
///   runtime @gpu valida na sessão GPU — documentado no comentário).
/// - img2img: `init_image_id` XOR `init_generation_id` (ambos ⇒ 400);
///   `init_strength` Some exige um dos ids (órfã ⇒ 400) e faixa 0.05..=0.95.
pub fn validate_diffusion_generate_request(
    mut req: DiffusionGenerateJobRequest,
) -> Result<DiffusionGenerateJobRequest, String> {
    // --- XOR: base_model / custom_model_id ---
    let has_custom = req.custom_model_id.is_some();
    let has_base = req.base_model.is_some();
    if has_custom && has_base {
        return Err("use either baseModel or customModelId, not both".to_string());
    }
    if !has_custom && !has_base {
        // Retrocompat: client legado não envia baseModel → default.
        req.base_model = Some("flux-2-klein-4b".to_string());
    }
    // Validação de UUID para base_model quando presente (mantém str-list check).
    if let Some(ref bm) = req.base_model {
        if !ALLOWED_DIFFUSION_BASE_MODELS.contains(&bm.as_str()) {
            return Err(format!(
                "baseModel must be one of {:?}, got '{}'",
                ALLOWED_DIFFUSION_BASE_MODELS, bm
            ));
        }
    }
    // Validação de UUID para custom_model_id quando presente.
    if let Some(cm) = &req.custom_model_id {
        if uuid::Uuid::parse_str(cm).is_err() {
            return Err("customModelId must be a valid UUID".to_string());
        }
    }
    // UUID do text encoder custom (formato); existência/kind/arch resolve no handler.
    if let Some(tm) = &req.text_encoder_model_id {
        if uuid::Uuid::parse_str(tm).is_err() {
            return Err("textEncoderModelId must be a valid UUID".to_string());
        }
    }

    // --- Prompt ---
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

    // --- Dimensões ---
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

    // --- Steps / Guidance ---
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

    // --- Quantização ---
    if !ALLOWED_DIFFUSION_QUANTIZATIONS.contains(&req.quantization.as_str()) {
        return Err(format!(
            "quantization must be one of {:?}, got '{}'",
            ALLOWED_DIFFUSION_QUANTIZATIONS, req.quantization
        ));
    }

    // --- Batch size (D2) ---
    if !(1..=8).contains(&req.batch_size) {
        return Err(format!(
            "batchSize must be between 1 and 8, got {}",
            req.batch_size
        ));
    }

    // --- LoRAs (D3) ---
    if req.loras.len() > 4 {
        return Err(format!(
            "loras must have at most 4 entries, got {}",
            req.loras.len()
        ));
    }
    for (i, lora) in req.loras.iter().enumerate() {
        if uuid::Uuid::parse_str(&lora.model_id).is_err() {
            return Err(format!("loras[{i}].modelId must be a valid UUID",));
        }
        if !(0.0..=2.0).contains(&lora.scale) || lora.scale.is_nan() {
            return Err(format!(
                "loras[{i}].scale must be between 0.0 and 2.0, got {}",
                lora.scale
            ));
        }
    }

    // --- Retrocompat lora_scale (mantém validação legada) ---
    if !(0.0..=2.0).contains(&req.lora_scale) || req.lora_scale.is_nan() {
        return Err(format!(
            "loraScale must be between 0.0 and 2.0, got {}",
            req.lora_scale
        ));
    }

    // --- Conflito: loras não vazio + (weights OU lora_scale legado) → 400 ---
    if !req.loras.is_empty() && (req.weights.is_some() || req.lora_scale != 1.0) {
        return Err("use loras or weights/loraScale, not both".to_string());
    }

    // --- UUID checks para weights e orchestrator_id ---
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

    // --- img2img: init_image_id XOR init_generation_id; init_strength órfã/faixa ---
    // (serde já garante UUID válido nos ids — `Option<Uuid>` falha no parse ⇒
    // 400 no handler; aqui só restam as regras relacionais.)
    let has_init_image = req.init_image_id.is_some();
    let has_init_generation = req.init_generation_id.is_some();
    if has_init_image && has_init_generation {
        return Err("use either initImageId or initGenerationId, not both".to_string());
    }
    if let Some(s) = req.init_strength {
        if !has_init_image && !has_init_generation {
            return Err("initStrength exige initImageId ou initGenerationId".to_string());
        }
        if s.is_nan() || !(0.05..=0.95).contains(&s) {
            return Err(format!(
                "initStrength must be between 0.05 and 0.95, got {s}"
            ));
        }
    }

    // --- Sampler (motor Flux.2): enum global + restrição arch-aware ---
    if !ALLOWED_DIFFUSION_SAMPLERS.contains(&req.sampler.as_str()) {
        return Err(format!(
            "sampler must be one of {:?}, got '{}'",
            ALLOWED_DIFFUSION_SAMPLERS, req.sampler
        ));
    }
    // Custom usa arch sdxl/sd15 (sem restrição Flux.2); sem custom, a base
    // efetiva é baseModel ou o default legado "flux-2-klein-4b".
    let effective_base = if req.custom_model_id.is_some() {
        String::new()
    } else {
        req.base_model
            .as_deref()
            .unwrap_or("flux-2-klein-4b")
            .to_string()
    };
    if effective_base == "flux-2-klein-4b"
        && !ALLOWED_FLUX2_KLEIN_SAMPLERS.contains(&req.sampler.as_str())
    {
        return Err(format!(
            "sampler '{}' not supported for baseModel 'flux-2-klein-4b' (use one of {:?})",
            req.sampler, ALLOWED_FLUX2_KLEIN_SAMPLERS
        ));
    }

    // --- Upscale (RRDBNet x4): model ∈ ["4x", "ultrasharp", "siax"], scale ∈ [2, 4] ---
    if let Some(up) = &req.upscale {
        if !ALLOWED_DIFFUSION_UPSCALE_MODELS.contains(&up.model.as_str()) {
            return Err(format!(
                "upscale.model must be one of {:?}, got '{}'",
                ALLOWED_DIFFUSION_UPSCALE_MODELS, up.model
            ));
        }
        if up.scale != 2 && up.scale != 4 {
            return Err(format!(
                "upscale.scale must be one of [2, 4], got {}",
                up.scale
            ));
        }
    }

    Ok(req)
}
/// Gera `config.yaml` de geração Text-to-Image v2 (ADR-0023 D2/D3/D4).
///
/// Placeholders `{output_path}` e `{weights_path}` são substituídos pelo
/// orquestrador no staging; `{lora_path_i}` são substituídos pelo orquestrador
/// para cada LoRA; `{custom_checkpoint_path}` é substituído pelo orquestrador
/// para custom models.
///
/// Quando `loras` não vazio: bloco `loras:` com placeholders `{lora_path_i}`.
/// Quando `loras` vazio E `weights` Some: formato legado `weights_path`/`lora_scale`.
/// Quando `custom_model_id` Some: `custom_checkpoint_path` + `arch` (resolvidos
/// pelo handler — `custom_arch` deve ser Some).
/// Quando `text_encoder_model_id` Some: dentro de `generate:`,
/// `text_encoder_path: "{text_encoder_path}"` (placeholder literal — o
/// orchestrator substitui via `text_encoder_ref`; NUNCA id/path real).
/// Quando `init_image_id` ou `init_generation_id` presente (img2img): bloco
/// `generate:` ganha `init_image_path: "{init_image_path}"` (placeholder
/// literal — o orchestrator substitui) + `init_strength` (pedido ou 0.6).
/// Retrocompat: request legado (sem campos novos) gera yaml quase idêntico
/// ao anterior à ADR-0023 (batch_size: 1, sem loras, sem custom, sem upscale)
/// — exceção aditiva da fatia Flux.2: linha `sampler: "default"` sempre presente.
pub fn generate_diffusion_generate_config_yaml(
    job_id: &str,
    req: &DiffusionGenerateJobRequest,
    custom_arch: Option<&str>,
) -> String {
    let neg_line = match &req.negative_prompt {
        Some(neg) => format!(
            "  negative_prompt: {}\n",
            serde_json::to_string(neg).unwrap_or_else(|_| "\"\"".into())
        ),
        None => "  negative_prompt: \"\"\n".to_string(),
    };

    // Seed: quando None, NÃO emitir seed: no yaml (engine sorteia e reporta no meta — D2).
    let seed_root_line = match req.seed {
        Some(s) => format!("seed: {s}\n"),
        None => String::new(),
    };
    let seed_gen_line = match req.seed {
        Some(s) => format!("  seed: {s}\n"),
        None => String::new(),
    };

    // --- Effetive model para a linha `model:` ---
    // Custom: usa arch resolvido; base: usa base_model.
    let effective_model =
        custom_arch.unwrap_or_else(|| req.base_model.as_deref().unwrap_or("flux-2-klein-4b"));

    // --- Regra de pesos (retrocompat + v2) ---
    // LEGADO (loras vazio, sem custom): `weights_path:` root-level ANTES de
    // `generate:` (byte-compatível com o formato anterior à ADR-0023) e
    // `lora_scale:` dentro de generate.
    // V2 (loras OU custom): SEM weights_path e SEM lora_scale.
    let is_legacy = req.loras.is_empty() && custom_arch.is_none();

    let weights_path_line = if is_legacy {
        "weights_path: \"{weights_path}\"\n".to_string()
    } else {
        String::new()
    };

    let loras_block = if req.loras.is_empty() {
        String::new()
    } else {
        let mut block = String::new();
        for (i, lora) in req.loras.iter().enumerate() {
            let scale_str = format_lora_scale(lora.scale);
            block.push_str(&format!(
                "    - path: \"{{lora_path_{i}}}\"\n      scale: {scale_str}\n      model_id: \"{}\"\n",
                lora.model_id
            ));
        }
        format!("  loras:\n{block}")
    };

    let custom_block = if let Some(arch) = custom_arch {
        let cid_line = if let Some(ref cid) = req.custom_model_id {
            format!("  custom_model_id: \"{cid}\"\n")
        } else {
            String::new()
        };
        format!("{cid_line}  custom_checkpoint_path: \"{{custom_checkpoint_path}}\"\n  arch: \"{arch}\"\n")
    } else {
        String::new()
    };

    // Encoder custom: placeholder literal dentro de `generate:` — o
    // orchestrator substitui via text_encoder_ref. Persiste também o model_id para reprodutibilidade.
    let text_encoder_block = if let Some(ref enc_id) = req.text_encoder_model_id {
        format!("  text_encoder_path: \"{{text_encoder_path}}\"\n  text_encoder_model_id: \"{enc_id}\"\n")
    } else {
        String::new()
    };

    let lora_scale_line = if is_legacy {
        format!("  lora_scale: {}\n", req.lora_scale)
    } else {
        String::new()
    };

    // Quando custom_arch está presente, NÃO emitir generate.base_model:
    // o engine (generate.py L127) faz fallback para root `model:` que já leva o arch.
    let base_model_line = if custom_arch.is_some() {
        String::new()
    } else {
        format!(
            "  base_model: \"{}\"\n",
            req.base_model.as_deref().unwrap_or("flux-2-klein-4b")
        )
    };

    // --- img2img: init dentro do bloco `generate:` ---
    // `init_image_path` é placeholder literal — o orchestrator substitui pelo
    // path stageado (NUNCA emitir o id/path real aqui); `init_strength` usa o
    // valor pedido ou 0.6 (default aplicado no config_yaml/engine). Sem id ⇒ bloco
    // vazio (yaml legado byte-idêntico).
    let init_block = if req.init_image_id.is_some() || req.init_generation_id.is_some() {
        let strength = req.init_strength.unwrap_or(0.6);
        format!(
            "  init_image_path: \"{{init_image_path}}\"\n  init_strength: {}\n",
            format_init_strength(strength)
        )
    } else {
        String::new()
    };

    // --- Upscale Real-ESRGAN: bloco `upscale:` com model/scale APENAS quando
    // Some; ausente (None) ⇒ nada (engine trata ausente como sem upscale).
    let upscale_block = match &req.upscale {
        Some(up) => format!(
            "  upscale:\n    model: \"{}\"\n    scale: {}\n",
            up.model, up.scale
        ),
        None => String::new(),
    };

    format!(
        r#"# Configuração de geração Difusão (Playground)
job_id: "{job_id}"
engine: "diffusion"
model: "{effective_model}"
mode: "generate"
output_path: "{{output_path}}"
{seed_root_line}{weights_path_line}generate:
{base_model_line}  prompt: {prompt_json}
{neg_line}  width: {width}
  height: {height}
  steps: {steps}
  guidance_scale: {guidance_scale}
{seed_gen_line}  quantization: "{quantization}"
  sampler: "{sampler}"
  distilled: {distilled}
  batch_size: {batch_size}
{loras_block}{custom_block}{text_encoder_block}{init_block}{upscale_block}{lora_scale_line}"#,
        job_id = job_id,
        effective_model = effective_model,
        prompt_json = serde_json::to_string(&req.prompt).unwrap_or_else(|_| "\"\"".into()),
        neg_line = neg_line,
        width = req.width,
        height = req.height,
        steps = req.steps,
        guidance_scale = req.guidance_scale,
        quantization = req.quantization,
        sampler = req.sampler,
        distilled = req.distilled,
        batch_size = req.batch_size,
        seed_root_line = seed_root_line,
        seed_gen_line = seed_gen_line,
        weights_path_line = weights_path_line,
        base_model_line = base_model_line,
        loras_block = loras_block,
        custom_block = custom_block,
        text_encoder_block = text_encoder_block,
        init_block = init_block,
        upscale_block = upscale_block,
        lora_scale_line = lora_scale_line,
    )
}

/// VRAM mínima de generate por (arch efetiva, quantization) — D8 estendido.
///
/// sd15 fixo 6; sdxl e flux: none→16, 6bit→10, 4bit→8, 2bit→7, 8bit→12
/// (aliases legados 4bit-nf4/8bit-bnb seguem o nível da família).
pub fn diffusion_generate_vram_min_gb(arch: &str, quantization: &str) -> i32 {
    match arch {
        "sd15" => 6,
        "sdxl" | "flux" | "flux-2-klein-4b" => match quantization {
            "2bit" => 7,
            "4bit" | "4bit-nf4" => 8,
            "6bit" => 10,
            "8bit" | "8bit-bnb" => 12,
            _ => 16,
        },
        _ => match quantization {
            "2bit" => 7,
            "4bit" | "4bit-nf4" => 8,
            "6bit" => 10,
            "8bit" | "8bit-bnb" => 12,
            _ => 16,
        },
    }
}

/// Formata init_strength com no mínimo 1 casa decimal e ponto (nunca vírgula).
/// 0.6 → "0.6", 0.5 → "0.5", 1.0 → "1.0", 0.05 → "0.05".
fn format_init_strength(v: f32) -> String {
    let s = format!("{v}").replace(',', ".");
    if s.contains('.') {
        s
    } else {
        format!("{s}.0")
    }
}

/// Formata scale de LoRA preservando até 2 casas decimais, trim de zeros à direita.
/// 0.85 → "0.85", 1.0 → "1", 0.50 → "0.5".
fn format_lora_scale(scale: f64) -> String {
    let s = format!("{scale:.2}");
    // Trim de zeros à direita e ponto decimal se inteiro
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    trimmed.to_string()
}

#[cfg(test)]
mod format_lora_scale_tests {
    use super::*;

    #[test]
    fn scale_085_preserves_two_decimals() {
        assert_eq!(format_lora_scale(0.85), "0.85");
    }

    #[test]
    fn scale_10_becomes_integer() {
        assert_eq!(format_lora_scale(1.0), "1");
    }

    #[test]
    fn scale_050_trims_one_zero() {
        assert_eq!(format_lora_scale(0.50), "0.5");
    }

    #[test]
    fn scale_035_two_decimals() {
        assert_eq!(format_lora_scale(0.35), "0.35");
    }
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
        assert_eq!(validated.base_model.as_deref(), Some("sdxl"));
        assert_eq!(validated.epochs, 10);
        assert_eq!(validated.batch_size, 1);
        assert_eq!(validated.rank, 16);
        assert_eq!(validated.alpha, 16);
        assert_eq!(validated.learning_rate, 0.0001);
        assert_eq!(validated.quantization, "4bit");
        assert!(validated.enable_bucket);
        assert!(validated.trigger_word.is_none());

        let yaml = generate_diffusion_config_yaml("job-123", &validated, None);
        assert!(yaml.contains(r#"engine: "diffusion""#));
        assert!(yaml.contains(r#"model: "sdxl""#));
        assert!(yaml.contains(r#"rank: 16"#));
        assert!(yaml.contains(r#"quantization: "4bit""#));
        assert!(yaml.contains("  enable_bucket: true"));
        assert!(!yaml.contains("output_name:"));

        // Desabilitar bucketing injeta `enable_bucket: false` no yaml
        let mut no_bucket = validated.clone();
        no_bucket.enable_bucket = false;
        let yaml3 = generate_diffusion_config_yaml("job-125", &no_bucket, None);
        assert!(yaml3.contains("  enable_bucket: false"));
        assert!(!yaml3.contains("  enable_bucket: true"));

        // Com output_name explícito
        let mut with_name = validated.clone();
        with_name.output_name = Some("custom-lora-v1".to_string());
        let yaml2 = generate_diffusion_config_yaml("job-124", &with_name, None);
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
    fn diffusion_train_xor_base_custom() {
        // XOR: ambos presentes ⇒ erro; ambos ausentes ⇒ default sdxl;
        // só custom ⇒ ok (arch resolve no handler).
        let both = r#"{"datasetId":"550e8400-e29b-41d4-a716-446655440001","baseModel":"sdxl","customModelId":"550e8400-e29b-41d4-a716-446655440000"}"#;
        let req: DiffusionJobRequest = serde_json::from_str(both).unwrap();
        assert!(validate_diffusion_request(req).is_err());

        let neither = r#"{"datasetId":"550e8400-e29b-41d4-a716-446655440001"}"#;
        let req: DiffusionJobRequest = serde_json::from_str(neither).unwrap();
        let v = validate_diffusion_request(req).expect("default sdxl");
        assert_eq!(v.base_model.as_deref(), Some("sdxl"));
        assert!(v.custom_model_id.is_none());

        let custom_only = r#"{"datasetId":"550e8400-e29b-41d4-a716-446655440001","customModelId":"550e8400-e29b-41d4-a716-446655440000"}"#;
        let req: DiffusionJobRequest = serde_json::from_str(custom_only).unwrap();
        let v = validate_diffusion_request(req).expect("custom only ok");
        assert!(v.base_model.is_none());
        assert_eq!(
            v.custom_model_id.as_deref(),
            Some("550e8400-e29b-41d4-a716-446655440000")
        );

        // UUID inválido ⇒ erro (custom e encoder).
        let bad_custom =
            r#"{"datasetId":"550e8400-e29b-41d4-a716-446655440001","customModelId":"not-a-uuid"}"#;
        let req: DiffusionJobRequest = serde_json::from_str(bad_custom).unwrap();
        assert!(validate_diffusion_request(req).is_err());
        let bad_enc = r#"{"datasetId":"550e8400-e29b-41d4-a716-446655440001","textEncoderModelId":"not-a-uuid"}"#;
        let req: DiffusionJobRequest = serde_json::from_str(bad_enc).unwrap();
        assert!(validate_diffusion_request(req).is_err());
    }

    #[test]
    fn diffusion_train_yaml_custom_and_encoder_placeholders() {
        // Custom: model: = arch resolvido + placeholder literal root-level.
        let json = r#"{"datasetId":"550e8400-e29b-41d4-a716-446655440001","customModelId":"550e8400-e29b-41d4-a716-446655440000"}"#;
        let req: DiffusionJobRequest = serde_json::from_str(json).unwrap();
        let v = validate_diffusion_request(req).unwrap();
        let yaml = generate_diffusion_config_yaml("job-train-custom", &v, Some("flux-2-klein-4b"));
        assert!(yaml.contains("model: \"flux-2-klein-4b\""));
        assert!(yaml.contains("custom_checkpoint_path: \"{custom_checkpoint_path}\""));
        // NUNCA o id real no yaml.
        assert!(!yaml.contains("550e8400-e29b-41d4-a716-446655440000"));
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("yaml válido");
        assert_eq!(parsed["model"].as_str(), Some("flux-2-klein-4b"));
        assert_eq!(
            parsed["custom_checkpoint_path"].as_str(),
            Some("{custom_checkpoint_path}")
        );

        // Sem custom ⇒ sem placeholder.
        let legacy = r#"{"datasetId":"550e8400-e29b-41d4-a716-446655440001"}"#;
        let req: DiffusionJobRequest = serde_json::from_str(legacy).unwrap();
        let v = validate_diffusion_request(req).unwrap();
        let yaml = generate_diffusion_config_yaml("job-train-legado", &v, None);
        assert!(!yaml.contains("custom_checkpoint_path"));
        assert!(!yaml.contains("text_encoder_path"));

        // Encoder: root-level placeholder literal.
        let enc = r#"{"datasetId":"550e8400-e29b-41d4-a716-446655440001","baseModel":"flux-2-klein-4b","textEncoderModelId":"550e8400-e29b-41d4-a716-446655440002"}"#;
        let req: DiffusionJobRequest = serde_json::from_str(enc).unwrap();
        let v = validate_diffusion_request(req).unwrap();
        let yaml = generate_diffusion_config_yaml("job-train-enc", &v, None);
        assert!(yaml.contains("text_encoder_path: \"{text_encoder_path}\""));
        assert!(!yaml.contains("550e8400-e29b-41d4-a716-446655440002"));
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

        let yaml = generate_diffusion_config_yaml("job-adv-1", &validated, None);
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

        let yaml = generate_diffusion_config_yaml("job-resume-1", &validated, None);
        assert!(yaml.contains(r#"weights_path: "{weights_path}""#));
        assert!(yaml.contains("checkpoint_interval: 5"));
        assert!(yaml.contains("epoch_offset: 10"));

        // Intervalo inválido (0 ou > 100) deve falhar
        let bad_json =
            r#"{"datasetId":"550e8400-e29b-41d4-a716-446655440001","checkpointInterval":0}"#;
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
        assert_eq!(validated.base_model.as_deref(), Some("flux-2-klein-4b"));
        assert_eq!(validated.width, 1024);
        assert_eq!(validated.height, 768);
        assert_eq!(validated.steps, 25);
        assert_eq!(validated.guidance_scale, 4.0);
        assert_eq!(validated.seed, Some(99999));
        assert_eq!(validated.quantization, "4bit");
        assert_eq!(validated.lora_scale, 0.9);

        let yaml = generate_diffusion_generate_config_yaml("job-gen-001", &validated, None);
        assert!(yaml.contains(r#"mode: "generate""#));
        assert!(yaml.contains(r#"base_model: "flux-2-klein-4b""#));
        assert!(yaml.contains("a stunning portrait in neon cyberpunk style"));
        assert!(yaml.contains("negative_prompt: \"blurry, distorted\""));
        assert!(yaml.contains("width: 1024"));
        assert!(yaml.contains("height: 768"));
        assert!(yaml.contains("seed: 99999"));
        assert!(yaml.contains(r#"quantization: "4bit""#));
        // Retrocompat: batch_size 1 (legado) + lora_scale legado + weights_path
        assert!(yaml.contains("batch_size: 1"));
        assert!(yaml.contains("lora_scale: 0.9"));
        assert!(yaml.contains("weights_path: \"{weights_path}\""));

        // Prompt vazio deve falhar
        let bad_json = r#"{"prompt": "   "}"#;
        let bad_req: DiffusionGenerateJobRequest = serde_json::from_str(bad_json).unwrap();
        assert!(validate_diffusion_generate_request(bad_req).is_err());
    }

    // =========================================================================
    // Diffusion Generate v2 tests (ADR-0023 D2/D3/D4)
    // =========================================================================

    #[test]
    fn diffusion_generate_defaults_without_base_model() {
        // Request legado sem baseModel → default "flux-2-klein-4b"
        let json = r#"{"prompt": "test"}"#;
        let req: DiffusionGenerateJobRequest = serde_json::from_str(json).unwrap();
        assert!(req.base_model.is_none());
        let validated = validate_diffusion_generate_request(req).expect("should validate");
        assert_eq!(validated.base_model.as_deref(), Some("flux-2-klein-4b"));
        assert_eq!(validated.batch_size, 1);
        assert!(validated.loras.is_empty());
        assert!(validated.custom_model_id.is_none());
    }

    #[test]
    fn diffusion_generate_xor_both_rejected() {
        // baseModel + customModelId → 400
        let json = r#"{
            "prompt": "test",
            "baseModel": "sdxl",
            "customModelId": "550e8400-e29b-41d4-a716-446655440000"
        }"#;
        let req: DiffusionGenerateJobRequest = serde_json::from_str(json).unwrap();
        assert!(validate_diffusion_generate_request(req).is_err());
    }

    #[test]
    fn diffusion_generate_xor_neither_defaults() {
        // Sem baseModel nem customModelId → default "flux-2-klein-4b"
        let json = r#"{"prompt": "test"}"#;
        let req: DiffusionGenerateJobRequest = serde_json::from_str(json).unwrap();
        let validated = validate_diffusion_generate_request(req).unwrap();
        assert_eq!(validated.base_model.as_deref(), Some("flux-2-klein-4b"));
        assert!(validated.custom_model_id.is_none());
    }

    #[test]
    fn diffusion_generate_custom_only() {
        // Só customModelId → válido
        let json = r#"{
            "prompt": "test",
            "customModelId": "550e8400-e29b-41d4-a716-446655440000"
        }"#;
        let req: DiffusionGenerateJobRequest = serde_json::from_str(json).unwrap();
        let validated = validate_diffusion_generate_request(req).unwrap();
        assert!(validated.base_model.is_none());
        assert_eq!(
            validated.custom_model_id.as_deref(),
            Some("550e8400-e29b-41d4-a716-446655440000")
        );
    }

    #[test]
    fn diffusion_generate_batch_size_boundary() {
        // batch_size 1 e 8 → ok
        for bs in [1, 8] {
            let json = format!(r#"{{"prompt":"test","batchSize":{bs}}}"#);
            let req: DiffusionGenerateJobRequest = serde_json::from_str(&json).unwrap();
            assert!(
                validate_diffusion_generate_request(req).is_ok(),
                "batch_size={bs}"
            );
        }
        // batch_size 0 e 9 → 400
        for bs in [0, 9] {
            let json = format!(r#"{{"prompt":"test","batchSize":{bs}}}"#);
            let req: DiffusionGenerateJobRequest = serde_json::from_str(&json).unwrap();
            assert!(
                validate_diffusion_generate_request(req).is_err(),
                "batch_size={bs}"
            );
        }
    }

    #[test]
    fn diffusion_generate_loras_max_4() {
        let uuid = "550e8400-e29b-41d4-a716-446655440000";
        // 4 LoRAs → ok
        let json = format!(
            r#"{{"prompt":"test","loras":[{{"modelId":"{uuid}","scale":1.0}},{{"modelId":"{uuid}","scale":0.8}},{{"modelId":"{uuid}","scale":0.5}},{{"modelId":"{uuid}","scale":0.3}}]}}"#
        );
        let req: DiffusionGenerateJobRequest = serde_json::from_str(&json).unwrap();
        assert!(validate_diffusion_generate_request(req).is_ok());

        // 5 LoRAs → 400
        let json = format!(
            r#"{{"prompt":"test","loras":[{{"modelId":"{uuid}","scale":1.0}},{{"modelId":"{uuid}","scale":0.8}},{{"modelId":"{uuid}","scale":0.5}},{{"modelId":"{uuid}","scale":0.3}},{{"modelId":"{uuid}","scale":0.1}}]}}"#
        );
        let req: DiffusionGenerateJobRequest = serde_json::from_str(&json).unwrap();
        assert!(validate_diffusion_generate_request(req).is_err());
    }

    #[test]
    fn diffusion_generate_loras_scale_boundary() {
        let uuid = "550e8400-e29b-41d4-a716-446655440000";
        // scale 0.0 e 2.0 → ok
        for s in [0.0, 2.0] {
            let json =
                format!(r#"{{"prompt":"test","loras":[{{"modelId":"{uuid}","scale":{s}}}]}}"#);
            let req: DiffusionGenerateJobRequest = serde_json::from_str(&json).unwrap();
            assert!(
                validate_diffusion_generate_request(req).is_ok(),
                "scale={s}"
            );
        }
        // scale 2.5 → 400
        let json = format!(r#"{{"prompt":"test","loras":[{{"modelId":"{uuid}","scale":2.5}}]}}"#);
        let req: DiffusionGenerateJobRequest = serde_json::from_str(&json).unwrap();
        assert!(validate_diffusion_generate_request(req).is_err());
    }

    #[test]
    fn diffusion_generate_loras_invalid_uuid() {
        let json = r#"{"prompt":"test","loras":[{"modelId":"not-a-uuid","scale":1.0}]}"#;
        let req: DiffusionGenerateJobRequest = serde_json::from_str(json).unwrap();
        assert!(validate_diffusion_generate_request(req).is_err());
    }

    #[test]
    fn diffusion_generate_custom_invalid_uuid() {
        let json = r#"{"prompt":"test","customModelId":"not-a-uuid"}"#;
        let req: DiffusionGenerateJobRequest = serde_json::from_str(json).unwrap();
        assert!(validate_diffusion_generate_request(req).is_err());
    }

    #[test]
    fn diffusion_generate_loras_and_weights_conflict() {
        let uuid = "550e8400-e29b-41d4-a716-446655440000";
        // loras não vazio + weights → 400
        let json = format!(
            r#"{{"prompt":"test","loras":[{{"modelId":"{uuid}","scale":1.0}}],"weights":"{uuid}"}}"#
        );
        let req: DiffusionGenerateJobRequest = serde_json::from_str(&json).unwrap();
        assert!(validate_diffusion_generate_request(req).is_err());

        // loras não vazio + lora_scale != 1.0 (default) → 400
        let json = format!(
            r#"{{"prompt":"test","loras":[{{"modelId":"{uuid}","scale":1.0}}],"loraScale":0.8}}"#
        );
        let req: DiffusionGenerateJobRequest = serde_json::from_str(&json).unwrap();
        assert!(validate_diffusion_generate_request(req).is_err());

        // loras não vazio + lora_scale == 1.0 (default) → ok (default não conta)
        let json = format!(
            r#"{{"prompt":"test","loras":[{{"modelId":"{uuid}","scale":1.0}}],"loraScale":1.0}}"#
        );
        let req: DiffusionGenerateJobRequest = serde_json::from_str(&json).unwrap();
        assert!(validate_diffusion_generate_request(req).is_ok());
    }

    #[test]
    fn diffusion_generate_yaml_loras_block() {
        let uuid1 = "550e8400-e29b-41d4-a716-446655440000";
        let uuid2 = "550e8400-e29b-41d4-a716-446655440001";
        let json = format!(
            r#"{{"prompt":"test","loras":[{{"modelId":"{uuid1}","scale":1.0}},{{"modelId":"{uuid2}","scale":0.8}}]}}"#
        );
        let req: DiffusionGenerateJobRequest = serde_json::from_str(&json).unwrap();
        let validated = validate_diffusion_generate_request(req).unwrap();
        let yaml = generate_diffusion_generate_config_yaml("job-lora-1", &validated, None);

        assert!(yaml.contains("batch_size: 1"));
        assert!(yaml.contains("loras:"));
        assert!(yaml.contains("{lora_path_0}"));
        assert!(yaml.contains("{lora_path_1}"));
        assert!(yaml.contains("scale: 1"));
        assert!(yaml.contains("scale: 0.8"));
        // Sem weights_path quando loras não vazio
        assert!(!yaml.contains("weights_path"));
        assert!(!yaml.contains("lora_scale"));
    }

    #[test]
    fn diffusion_generate_yaml_custom_block() {
        let json = r#"{
            "prompt": "test",
            "customModelId": "550e8400-e29b-41d4-a716-446655440000"
        }"#;
        let req: DiffusionGenerateJobRequest = serde_json::from_str(json).unwrap();
        let validated = validate_diffusion_generate_request(req).unwrap();
        let yaml =
            generate_diffusion_generate_config_yaml("job-custom-1", &validated, Some("sdxl"));

        assert!(yaml.contains("custom_checkpoint_path: \"{custom_checkpoint_path}\""));
        assert!(yaml.contains("arch: \"sdxl\""));
        assert!(yaml.contains(r#"model: "sdxl""#));
        assert!(yaml.contains("batch_size: 1"));
        // XOR: quando custom_arch está presente, NÃO deve haver base_model no generate
        assert!(
            !yaml.contains("base_model:"),
            "custom block must NOT contain base_model: {yaml}"
        );
        // Sem weights_path quando custom presente
        assert!(!yaml.contains("weights_path"));
        assert!(!yaml.contains("lora_scale"));

        // Validação cruzada: YAML parseável e sem generate.base_model
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("yaml deve ser válido");
        let gen = parsed["generate"]
            .as_mapping()
            .expect("generate deve ser mapping");
        assert!(
            !gen.contains_key(&serde_yaml::Value::String("base_model".into())),
            "generate must NOT contain base_model when custom_arch is present"
        );
        assert!(
            gen.contains_key(&serde_yaml::Value::String("custom_checkpoint_path".into())),
            "generate must contain custom_checkpoint_path"
        );
        assert!(
            gen.contains_key(&serde_yaml::Value::String("arch".into())),
            "generate must contain arch"
        );
    }
    #[test]
    fn diffusion_generate_yaml_text_encoder_block() {
        // Encoder Some ⇒ placeholder literal dentro de generate:.
        let json = r#"{
            "prompt": "test",
            "baseModel": "flux-2-klein-4b",
            "textEncoderModelId": "550e8400-e29b-41d4-a716-446655440002"
        }"#;
        let req: DiffusionGenerateJobRequest = serde_json::from_str(json).unwrap();
        let validated = validate_diffusion_generate_request(req).unwrap();
        let yaml = generate_diffusion_generate_config_yaml("job-enc-1", &validated, None);
        assert!(yaml.contains("text_encoder_path: \"{text_encoder_path}\""));
        assert!(!yaml.contains("550e8400-e29b-41d4-a716-446655440002"));
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("yaml válido");
        let gen = parsed["generate"].as_mapping().expect("generate mapping");
        assert!(
            gen.contains_key(&serde_yaml::Value::String("text_encoder_path".into())),
            "generate must contain text_encoder_path"
        );

        // Encoder ausente ⇒ sem placeholder (yaml legado byte-idêntico).
        let legacy = r#"{"prompt": "test"}"#;
        let req: DiffusionGenerateJobRequest = serde_json::from_str(legacy).unwrap();
        let validated = validate_diffusion_generate_request(req).unwrap();
        let yaml = generate_diffusion_generate_config_yaml("job-enc-legado", &validated, None);
        assert!(!yaml.contains("text_encoder_path"));

        // UUID inválido do encoder ⇒ erro de validação.
        let bad = r#"{"prompt": "test", "textEncoderModelId": "not-a-uuid"}"#;
        let req: DiffusionGenerateJobRequest = serde_json::from_str(bad).unwrap();
        assert!(validate_diffusion_generate_request(req).is_err());
    }

    #[test]
    fn diffusion_generate_yaml_legacy_byte_compat() {
        // Request LEGADO (sem campos novos) → weights_path root-level ANTES de
        // generate: e lora_scale dentro de generate.
        let json = r#"{
            "prompt": "a stunning portrait in neon cyberpunk style",
            "negativePrompt": "blurry, distorted",
            "width": 1024,
            "height": 768,
            "steps": 25,
            "guidanceScale": 4.0,
            "seed": 99999,
            "quantization": "4bit",
            "loraScale": 0.9,
            "weights": "550e8400-e29b-41d4-a716-446655440000"
        }"#;
        let req: DiffusionGenerateJobRequest = serde_json::from_str(json).unwrap();
        let validated = validate_diffusion_generate_request(req).unwrap();
        let yaml = generate_diffusion_generate_config_yaml("job-legado-1", &validated, None);

        // Retrocompat: batch_size 1, weights_path root-level, lora_scale inside generate
        assert!(yaml.contains("batch_size: 1"));
        assert!(yaml.contains("weights_path: \"{weights_path}\""));
        assert!(yaml.contains("lora_scale: 0.9"));
        assert!(yaml.contains(r#"mode: "generate""#));
        assert!(yaml.contains(r#"base_model: "flux-2-klein-4b""#));
        assert!(yaml.contains("a stunning portrait in neon cyberpunk style"));
        assert!(yaml.contains("negative_prompt: \"blurry, distorted\""));

        // weights_path DEVE estar ANTES de generate: (root-level)
        let wp_pos = yaml.find("weights_path:").expect("weights_path must exist");
        let gen_pos = yaml.find("generate:\n").expect("generate: must exist");
        assert!(
            wp_pos < gen_pos,
            "weights_path (pos {wp_pos}) must be BEFORE generate: (pos {gen_pos})"
        );
        // lora_scale DEVE estar dentro de generate (depois de batch_size)
        let bs_pos = yaml.find("batch_size:").expect("batch_size must exist");
        let ls_pos = yaml.find("lora_scale:").expect("lora_scale must exist");
        assert!(
            bs_pos < ls_pos,
            "batch_size (pos {bs_pos}) must be BEFORE lora_scale (pos {ls_pos})"
        );
        // Sem loras block no legado
        assert!(!yaml.contains("  loras:"));

        // Parse YAML válido
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("yaml deve ser válido");
        assert_eq!(
            parsed["weights_path"].as_str(),
            Some("{weights_path}"),
            "weights_path deve ser root-level string"
        );
        let gen = parsed["generate"]
            .as_mapping()
            .expect("generate deve ser mapping");
        assert!(
            gen.get(&serde_yaml::Value::String("lora_scale".into()))
                .is_some(),
            "lora_scale deve estar dentro de generate"
        );
    }

    // =====================================================================
    // Novos testes: separação de linhas no YAML (batch_size ↔ seções)
    // =====================================================================

    #[test]
    fn yaml_batch_size_newline_before_legacy_weights() {
        // Request legado: weights_path root-level ANTES de generate, lora_scale dentro
        let json = r#"{
            "prompt": "test legacy",
            "width": 512,
            "height": 512,
            "steps": 10,
            "guidanceScale": 7.0,
            "seed": 42,
            "quantization": "8bit",
            "loraScale": 0.7
        }"#;
        let req: DiffusionGenerateJobRequest = serde_json::from_str(json).unwrap();
        let validated = validate_diffusion_generate_request(req).unwrap();
        let yaml = generate_diffusion_generate_config_yaml("job-legacy-nl", &validated, None);

        // batch_size e weights_path em linhas separadas
        assert!(
            yaml.contains("batch_size: 1\n"),
            "batch_size deve terminar com newline: {yaml}"
        );
        assert!(
            yaml.contains("\nweights_path:"),
            "weights_path deve estar em linha própria: {yaml}"
        );
        // Nunca colado
        assert!(
            !yaml.contains("1weights_path"),
            "batch_size colou com weights_path: {yaml}"
        );
        // weights_path ANTES de generate:
        let wp_pos = yaml.find("weights_path:").expect("weights_path must exist");
        let gen_pos = yaml.find("generate:\n").expect("generate: must exist");
        assert!(
            wp_pos < gen_pos,
            "weights_path (pos {wp_pos}) must be BEFORE generate: (pos {gen_pos}): {yaml}"
        );
        // Estrutura correta do generate
        assert!(
            yaml.contains("generate:\n  base_model:"),
            "generate e base_model em linhas corretas: {yaml}"
        );
        // Sem loras block no legado
        assert!(
            !yaml.contains("  loras:"),
            "legado não deve ter loras: {yaml}"
        );

        // Parse YAML válido — weights_path root-level, lora_scale dentro de generate
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("yaml deve ser válido");
        assert_eq!(
            parsed["weights_path"].as_str(),
            Some("{weights_path}"),
            "weights_path deve ser root-level"
        );
        let gen = parsed["generate"]
            .as_mapping()
            .expect("generate deve ser mapping");
        assert!(
            gen.get(&serde_yaml::Value::String("lora_scale".into()))
                .is_some(),
            "lora_scale deve estar dentro de generate: {yaml}"
        );
    }

    #[test]
    fn yaml_batch_size_newline_before_loras_block() {
        // Request com 2 loras: batch_size NÃO deve colar com loras:
        let uuid1 = "550e8400-e29b-41d4-a716-446655440000";
        let uuid2 = "550e8400-e29b-41d4-a716-446655440001";
        let json = format!(
            r#"{{"prompt":"test loras","loras":[{{"modelId":"{uuid1}","scale":1.0}},{{"modelId":"{uuid2}","scale":0.8}}]}}"#
        );
        let req: DiffusionGenerateJobRequest = serde_json::from_str(&json).unwrap();
        let validated = validate_diffusion_generate_request(req).unwrap();
        let yaml = generate_diffusion_generate_config_yaml("job-loras-nl", &validated, None);

        // batch_size em linha separada de loras
        assert!(
            yaml.contains("batch_size: 1\n"),
            "batch_size deve terminar com newline: {yaml}"
        );
        assert!(
            yaml.contains("\n  loras:\n    - path:"),
            "loras deve estar em linhas separadas com indent: {yaml}"
        );
        assert!(
            !yaml.contains("1loras"),
            "batch_size colou com loras: {yaml}"
        );

        // Parse YAML válido
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("yaml deve ser válido");
        let loras = parsed["generate"]["loras"]
            .as_sequence()
            .expect("loras deve ser sequência");
        assert_eq!(loras.len(), 2);
    }

    #[test]
    fn yaml_batch_size_newline_before_custom_block() {
        // Request custom: batch_size NÃO deve colar com custom_checkpoint_path
        let json = r#"{
            "prompt": "test custom",
            "customModelId": "550e8400-e29b-41d4-a716-446655440000"
        }"#;
        let req: DiffusionGenerateJobRequest = serde_json::from_str(json).unwrap();
        let validated = validate_diffusion_generate_request(req).unwrap();
        let yaml =
            generate_diffusion_generate_config_yaml("job-custom-nl", &validated, Some("sdxl"));

        // batch_size em linha separada de custom_checkpoint_path
        assert!(
            yaml.contains("batch_size: 1\n"),
            "batch_size deve terminar com newline: {yaml}"
        );
        assert!(
            yaml.contains("\n  custom_checkpoint_path:"),
            "custom_checkpoint_path deve estar em linha própria: {yaml}"
        );
        assert!(
            !yaml.contains("1custom_checkpoint"),
            "batch_size colou com custom: {yaml}"
        );

        // Parse YAML válido
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("yaml deve ser válido");
        let gen = parsed["generate"]
            .as_mapping()
            .expect("generate deve ser mapping");
        assert!(gen
            .get(&serde_yaml::Value::String("custom_checkpoint_path".into()))
            .is_some());
        assert!(gen.get(&serde_yaml::Value::String("arch".into())).is_some());
    }

    // =====================================================================
    // Motor Flux.2 + treino (fatia feat/flux2-motor-treino)
    // =====================================================================

    #[test]
    fn flux2_sampler_defaults_euler_heun_ok() {
        // Default "default" + euler/heun passam na base flux-2-klein-4b.
        for sampler in ["default", "euler", "heun"] {
            let json = format!(
                r#"{{"prompt":"test","baseModel":"flux-2-klein-4b","sampler":"{sampler}"}}"#
            );
            let req: DiffusionGenerateJobRequest = serde_json::from_str(&json).unwrap();
            let v = validate_diffusion_generate_request(req).expect("sampler deveria passar");
            assert_eq!(v.sampler, sampler);
        }
        // Request legado sem sampler → default "default".
        let req: DiffusionGenerateJobRequest =
            serde_json::from_str(r#"{"prompt":"test"}"#).unwrap();
        let v = validate_diffusion_generate_request(req).unwrap();
        assert_eq!(v.sampler, "default");
    }

    #[test]
    fn flux2_rejeita_dpmpp_2m() {
        let json = r#"{"prompt":"test","baseModel":"flux-2-klein-4b","sampler":"dpmpp_2m"}"#;
        let req: DiffusionGenerateJobRequest = serde_json::from_str(json).unwrap();
        let err = validate_diffusion_generate_request(req).expect_err("dpmpp_2m deve falhar");
        assert!(
            err.contains("flux-2-klein-4b"),
            "erro deve citar a base: {err}"
        );
    }

    #[test]
    fn sampler_invalido_rejeitado() {
        let json = r#"{"prompt":"test","sampler":"nao-existe"}"#;
        let req: DiffusionGenerateJobRequest = serde_json::from_str(json).unwrap();
        assert!(validate_diffusion_generate_request(req).is_err());
    }

    #[test]
    fn sampler_dpmpp_ok_em_sdxl() {
        // Fora da base flux-2: dpmpp_2m passa (restrição é arch-aware).
        let json = r#"{"prompt":"test","baseModel":"sdxl","sampler":"dpmpp_2m"}"#;
        let req: DiffusionGenerateJobRequest = serde_json::from_str(json).unwrap();
        let v = validate_diffusion_generate_request(req).expect("sdxl aceita dpmpp_2m");
        assert_eq!(v.sampler, "dpmpp_2m");
    }

    #[test]
    fn upscale_none_e_4x_scale2_ok_scale3_400() {
        // None (ausente) → válido.
        let req: DiffusionGenerateJobRequest =
            serde_json::from_str(r#"{"prompt":"test"}"#).unwrap();
        let v = validate_diffusion_generate_request(req).unwrap();
        assert!(v.upscale.is_none());
        // {model: 4x, scale: 2} → válido.
        let req2: DiffusionGenerateJobRequest =
            serde_json::from_str(r#"{"prompt":"test","upscale":{"model":"4x","scale":2}}"#)
                .unwrap();
        let v2 = validate_diffusion_generate_request(req2).expect("upscale 4x/2 feliz");
        assert_eq!(v2.upscale.as_ref().unwrap().model, "4x");
        assert_eq!(v2.upscale.as_ref().unwrap().scale, 2);
        // {model: ultrasharp, scale: 4} → válido (preserva textura).
        let req_us: DiffusionGenerateJobRequest =
            serde_json::from_str(r#"{"prompt":"test","upscale":{"model":"ultrasharp","scale":4}}"#)
                .unwrap();
        let v_us = validate_diffusion_generate_request(req_us).expect("upscale ultrasharp/4 feliz");
        assert_eq!(v_us.upscale.as_ref().unwrap().model, "ultrasharp");
        assert_eq!(v_us.upscale.as_ref().unwrap().scale, 4);
        // {model: siax, scale: 2} → válido.
        let req_sx: DiffusionGenerateJobRequest =
            serde_json::from_str(r#"{"prompt":"test","upscale":{"model":"siax","scale":2}}"#)
                .unwrap();
        let v_sx = validate_diffusion_generate_request(req_sx).expect("upscale siax/2 feliz");
        assert_eq!(v_sx.upscale.as_ref().unwrap().model, "siax");
        assert_eq!(v_sx.upscale.as_ref().unwrap().scale, 2);
        // model ausente ⇒ default "4x" (retrocompat).
        let req_def: DiffusionGenerateJobRequest =
            serde_json::from_str(r#"{"prompt":"test","upscale":{"scale":4}}"#).unwrap();
        let v_def =
            validate_diffusion_generate_request(req_def).expect("upscale sem model usa default");
        assert_eq!(v_def.upscale.as_ref().unwrap().model, "4x");
        assert_eq!(v_def.upscale.as_ref().unwrap().scale, 4);
        // scale: 3 → 400.
        let req3: DiffusionGenerateJobRequest =
            serde_json::from_str(r#"{"prompt":"test","upscale":{"model":"4x","scale":3}}"#)
                .unwrap();
        assert!(validate_diffusion_generate_request(req3).is_err());
        // model inválido → 400 (mensagem lista os válidos).
        let req4: DiffusionGenerateJobRequest =
            serde_json::from_str(r#"{"prompt":"test","upscale":{"model":"9x","scale":2}}"#)
                .unwrap();
        let err = validate_diffusion_generate_request(req4).expect_err("upscale 9x deve falhar");
        assert!(
            err.contains("4x") && err.contains("ultrasharp") && err.contains("siax"),
            "erro lista válidos: {err}"
        );
    }

    #[test]
    fn quantization_2bit_6bit_aceitas_nos_dois_requests() {
        for q in ["2bit", "6bit"] {
            let json = format!(
                r#"{{"datasetId":"550e8400-e29b-41d4-a716-446655440001","quantization":"{q}"}}"#
            );
            let req: DiffusionJobRequest = serde_json::from_str(&json).unwrap();
            let v = validate_diffusion_request(req).expect("treino deveria aceitar");
            assert_eq!(v.quantization, q);
            let gjson = format!(r#"{{"prompt":"test","quantization":"{q}"}}"#);
            let greq: DiffusionGenerateJobRequest = serde_json::from_str(&gjson).unwrap();
            let gv = validate_diffusion_generate_request(greq).expect("generate deveria aceitar");
            assert_eq!(gv.quantization, q);
        }
        // Aliases legados continuam aceitos.
        for q in ["4bit-nf4", "8bit-bnb"] {
            let json = format!(
                r#"{{"datasetId":"550e8400-e29b-41d4-a716-446655440001","quantization":"{q}"}}"#
            );
            let req: DiffusionJobRequest = serde_json::from_str(&json).unwrap();
            assert!(validate_diffusion_request(req).is_ok());
        }
    }

    #[test]
    fn control_dataset_id_igual_ao_principal_400() {
        let id = "550e8400-e29b-41d4-a716-446655440001";
        let json = format!(r#"{{"datasetId":"{id}","controlDatasetId":"{id}"}}"#);
        let req: DiffusionJobRequest = serde_json::from_str(&json).unwrap();
        let err = validate_diffusion_request(req).expect_err("igual deve falhar");
        assert!(
            err.contains("controlDatasetId"),
            "erro deve citar o campo: {err}"
        );
        // Diferente → ok.
        let json2 = format!(
            r#"{{"datasetId":"{id}","controlDatasetId":"550e8400-e29b-41d4-a716-446655440002"}}"#
        );
        let req2: DiffusionJobRequest = serde_json::from_str(&json2).unwrap();
        assert!(validate_diffusion_request(req2).is_ok());
    }

    #[test]
    fn train_yaml_control_cache_text_embeddings() {
        // Sem control: sem linha control_dataset_path, cache false sempre presente.
        let req: DiffusionJobRequest =
            serde_json::from_str(r#"{"datasetId":"550e8400-e29b-41d4-a716-446655440001"}"#)
                .unwrap();
        let v = validate_diffusion_request(req).unwrap();
        assert!(!v.cache_text_embeddings);
        let yaml = generate_diffusion_config_yaml("job-train-sem-control", &v, None);
        assert!(
            !yaml.contains("control_dataset_path"),
            "sem control: {yaml}"
        );
        assert!(
            yaml.contains("cache_text_embeddings: false"),
            "cache sempre: {yaml}"
        );
        // Com control + cache true: linha presente + true.
        let req2: DiffusionJobRequest = serde_json::from_str(
            r#"{"datasetId":"550e8400-e29b-41d4-a716-446655440001","controlDatasetId":"550e8400-e29b-41d4-a716-446655440002","cacheTextEmbeddings":true}"#,
        )
        .unwrap();
        let v2 = validate_diffusion_request(req2).unwrap();
        assert!(v2.cache_text_embeddings);
        let yaml2 = generate_diffusion_config_yaml("job-train-com-control", &v2, None);
        assert!(
            yaml2.contains("control_dataset_path: \"{control_dataset_path}\""),
            "placeholder ausente: {yaml2}"
        );
        assert!(
            yaml2.contains("cache_text_embeddings: true"),
            "cache true: {yaml2}"
        );
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml2).expect("yaml válido");
        assert_eq!(
            parsed["control_dataset_path"].as_str(),
            Some("{control_dataset_path}")
        );
    }

    #[test]
    fn generate_yaml_contem_sampler_e_upscale() {
        let req: DiffusionGenerateJobRequest = serde_json::from_str(
            r#"{"prompt":"test","baseModel":"sdxl","sampler":"dpmpp_2m","upscale":{"model":"4x","scale":4}}"#,
        )
        .unwrap();
        let v = validate_diffusion_generate_request(req).unwrap();
        let yaml = generate_diffusion_generate_config_yaml("job-gen-sampler", &v, None);
        assert!(yaml.contains("sampler: \"dpmpp_2m\""), "sampler: {yaml}");
        assert!(yaml.contains("upscale:"), "bloco upscale: {yaml}");
        assert!(yaml.contains("scale: 4"), "scale: {yaml}");
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("yaml válido");
        assert_eq!(parsed["generate"]["sampler"].as_str(), Some("dpmpp_2m"));
        assert_eq!(parsed["generate"]["upscale"]["scale"].as_i64(), Some(4));
        // Sem upscale: bloco ausente, sampler default presente.
        let req2: DiffusionGenerateJobRequest =
            serde_json::from_str(r#"{"prompt":"test"}"#).unwrap();
        let v2 = validate_diffusion_generate_request(req2).unwrap();
        let yaml2 = generate_diffusion_generate_config_yaml("job-gen-sem-up", &v2, None);
        assert!(!yaml2.contains("upscale:"), "sem upscale: {yaml2}");
        assert!(
            yaml2.contains("sampler: \"default\""),
            "sampler default: {yaml2}"
        );
    }

    #[test]
    fn vram_min_2bit_7_e_6bit_10() {
        assert_eq!(diffusion_generate_vram_min_gb("sdxl", "2bit"), 7);
        assert_eq!(diffusion_generate_vram_min_gb("sdxl", "6bit"), 10);
        assert_eq!(diffusion_generate_vram_min_gb("flux-2-klein-4b", "2bit"), 7);
        assert_eq!(
            diffusion_generate_vram_min_gb("flux-2-klein-4b", "6bit"),
            10
        );
        assert_eq!(diffusion_generate_vram_min_gb("sd15", "2bit"), 6);
        assert_eq!(diffusion_generate_vram_min_gb("sdxl", "4bit"), 8);
        assert_eq!(diffusion_generate_vram_min_gb("sdxl", "8bit"), 12);
        assert_eq!(diffusion_generate_vram_min_gb("sdxl", "none"), 16);
    }
}

// =========================================================================
// img2img (fatia feat/img2img)

// =========================================================================
// img2img (fatia feat/img2img) — validate XOR/faixa + yaml com/sem init
// =========================================================================

#[cfg(test)]
mod img2img_tests {
    use super::*;

    const INIT_IMAGE: &str = "550e8400-e29b-41d4-a716-446655440010";
    const INIT_GEN: &str = "550e8400-e29b-41d4-a716-446655440011";

    fn parse(json: &str) -> DiffusionGenerateJobRequest {
        serde_json::from_str(json).expect("parse")
    }

    #[test]
    fn xor_ambos_ids_rejeitado() {
        let json = format!(
            r#"{{"prompt":"x","initImageId":"{INIT_IMAGE}","initGenerationId":"{INIT_GEN}"}}"#
        );
        let req = parse(&json);
        assert!(validate_diffusion_generate_request(req).is_err());
    }

    #[test]
    fn strength_orfa_rejeitada_com_mensagem_exata() {
        let json = r#"{"prompt":"x","initStrength":0.7}"#;
        let req = parse(json);
        let err = validate_diffusion_generate_request(req).expect_err("órfã deve falhar");
        assert_eq!(err, "initStrength exige initImageId ou initGenerationId");
    }

    #[test]
    fn strength_fora_da_faixa_rejeitada() {
        for s in [0.04, 0.0, 1.0, 1.5, -0.5] {
            let json =
                format!(r#"{{"prompt":"x","initImageId":"{INIT_IMAGE}","initStrength":{s}}}"#);
            let req = parse(&json);
            assert!(
                validate_diffusion_generate_request(req).is_err(),
                "initStrength={s} deveria falhar"
            );
        }
    }

    #[test]
    fn strength_nas_bordas_ok() {
        for s in [0.05, 0.6, 0.95] {
            let json =
                format!(r#"{{"prompt":"x","initImageId":"{INIT_IMAGE}","initStrength":{s}}}"#);
            let req = parse(&json);
            assert!(
                validate_diffusion_generate_request(req).is_ok(),
                "initStrength={s} deveria passar"
            );
        }
    }

    #[test]
    fn feliz_init_image_e_init_generation() {
        let json = format!(r#"{{"prompt":"x","initImageId":"{INIT_IMAGE}","initStrength":0.8}}"#);
        let req = parse(&json);
        let v = validate_diffusion_generate_request(req).expect("initImageId feliz");
        assert_eq!(
            v.init_image_id.map(|u| u.to_string()).as_deref(),
            Some(INIT_IMAGE)
        );
        assert!(v.init_generation_id.is_none());

        // initGenerationId sozinho (sem strength) também é feliz — manager
        // aplica 0.6.
        let json2 = format!(r#"{{"prompt":"x","initGenerationId":"{INIT_GEN}"}}"#);
        let req2 = parse(&json2);
        let v2 = validate_diffusion_generate_request(req2).expect("initGenerationId feliz");
        assert!(v2.init_image_id.is_none());
        assert_eq!(
            v2.init_generation_id.map(|u| u.to_string()).as_deref(),
            Some(INIT_GEN)
        );
        assert!(v2.init_strength.is_none());
    }

    #[test]
    fn sem_init_sem_strength_ok() {
        let req = parse(r#"{"prompt":"txt2img puro"}"#);
        let v = validate_diffusion_generate_request(req).expect("legado feliz");
        assert!(v.init_image_id.is_none());
        assert!(v.init_generation_id.is_none());
        assert!(v.init_strength.is_none());
    }

    #[test]
    fn init_id_nao_uuid_rejeitado_no_parse() {
        // `Option<Uuid>` falha no parse ⇒ 400 no handler (via serde).
        let json = r#"{"prompt":"x","initImageId":"nao-eh-uuid"}"#;
        assert!(serde_json::from_str::<DiffusionGenerateJobRequest>(json).is_err());
    }

    #[test]
    fn yaml_sem_init_nao_emite_bloco() {
        let req = parse(r#"{"prompt":"txt2img puro"}"#);
        let v = validate_diffusion_generate_request(req).unwrap();
        let yaml = generate_diffusion_generate_config_yaml("job-txt2img", &v, None);
        assert!(!yaml.contains("init_image_path"), "sem init: {yaml}");
        assert!(!yaml.contains("init_strength"), "sem init: {yaml}");
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("yaml válido");
        assert!(parsed["generate"].as_mapping().is_some());
    }

    #[test]
    fn yaml_com_init_emite_placeholder_e_strength() {
        let json =
            format!(r#"{{"prompt":"img2img","initImageId":"{INIT_IMAGE}","initStrength":0.8}}"#);
        let req = parse(&json);
        let v = validate_diffusion_generate_request(req).unwrap();
        let yaml = generate_diffusion_generate_config_yaml("job-img2img", &v, None);
        // Placeholder literal presente (o id real NUNCA vaza no yaml).
        assert!(
            yaml.contains("init_image_path: \"{init_image_path}\""),
            "placeholder ausente: {yaml}"
        );
        assert!(!yaml.contains(INIT_IMAGE), "id real vazou no yaml: {yaml}");
        assert!(yaml.contains("init_strength: 0.8"), "strength: {yaml}");
        // Dentro do bloco `generate:`.
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("yaml válido");
        let gen = parsed["generate"].as_mapping().expect("bloco generate");
        assert!(gen
            .get(&serde_yaml::Value::String("init_image_path".into()))
            .is_some());
        assert_eq!(
            gen.get(&serde_yaml::Value::String("init_strength".into()))
                .and_then(|n| n.as_f64()),
            Some(0.8)
        );
    }

    #[test]
    fn yaml_com_init_sem_strength_usa_default_formatado() {
        let json = format!(r#"{{"prompt":"img2img","initGenerationId":"{INIT_GEN}"}}"#);
        let req = parse(&json);
        let v = validate_diffusion_generate_request(req).unwrap();
        let yaml = generate_diffusion_generate_config_yaml("job-img2img-def", &v, None);
        assert!(
            yaml.contains("init_image_path: \"{init_image_path}\""),
            "placeholder ausente: {yaml}"
        );
        // Default 0.6 com 1 casa decimal no mínimo, sem vírgula.
        assert!(yaml.contains("init_strength: 0.6"), "default: {yaml}");
        assert!(!yaml.contains(','), "vírgula no yaml: {yaml}");
    }

    #[test]
    fn formato_init_strength_uma_casa_no_minimo() {
        assert_eq!(format_init_strength(0.6), "0.6");
        assert_eq!(format_init_strength(0.5), "0.5");
        assert_eq!(format_init_strength(0.05), "0.05");
        assert_eq!(format_init_strength(1.0), "1.0");
    }
}
