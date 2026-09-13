//! Regras puras de datasets (Fatia 3a): derivação e validação sem banco.
//!
//! Toda regra aqui é testável sem Postgres; sqlx aparece só no `FromRow`.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use std::collections::HashSet;

/// Paleta do design system (hex minúsculo, bate o CHECK `^#[0-9a-f]{6}$`).
pub const CLASS_PALETTE: [&str; 6] = [
    "#10b981", // emerald
    "#f59e0b", // amber
    "#f43f5e", // rose
    "#06b6d4", // cyan
    "#8b5cf6", // violet
    "#84cc16", // lime
];

/// Cor por índice de classe (cicla na paleta).
pub fn color_for(idx: usize) -> &'static str {
    CLASS_PALETTE[idx % CLASS_PALETTE.len()]
}

/// O enum fechado da spec (códigos wire `yolo_bbox | yolo_seg | …`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatasetType {
    YoloBbox,
    YoloSeg,
    DifusaoLora,
    ClipImageText,
}

/// Tabela fixa (category, task, format) por tipo — sem fallback.
pub fn derive(t: DatasetType) -> (&'static str, &'static str, &'static str) {
    match t {
        DatasetType::YoloBbox => ("yolo", "detect_track", "yolo_txt"),
        DatasetType::YoloSeg => ("yolo", "segment", "yolo_txt"),
        DatasetType::DifusaoLora => ("difusao", "caption", "captions"),
        DatasetType::ClipImageText => ("openclip", "embedding", "pairs"),
    }
}

/// Slug de `title`: minúsculas, deaccent Latin-1/PT via `match` explícito por
/// `char` (sem nova crate), runs de não-`[a-z0-9]` viram um único `-`, sem `-`
/// nas pontas, trunca em 96 sem `-` final. Total: vazio ⇒ `String::new()`
/// (o handler decide o 400).
pub fn slugify(title: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = true;
    for c in title.chars() {
        let mapped: Option<&str> = match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ä' | 'Å' => {
                Some("a")
            }
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => Some("e"),
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => Some("i"),
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'ø' | 'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' | 'Ø' => {
                Some("o")
            }
            'ú' | 'ù' | 'ü' | 'û' | 'Ú' | 'Ù' | 'Ü' | 'Û' => Some("u"),
            'ñ' | 'Ñ' => Some("n"),
            'ç' | 'Ç' => Some("c"),
            'ÿ' | 'Ÿ' => Some("y"),
            'æ' | 'Æ' => Some("ae"),
            'œ' | 'Œ' => Some("oe"),
            'ß' | 'ẞ' => Some("ss"),
            _ => None,
        };
        if let Some(s) = mapped {
            out.push_str(s);
            prev_dash = false;
        } else if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    let mut slug = trimmed.to_string();
    if slug.len() > 96 {
        slug.truncate(96);
        slug = slug.trim_end_matches('-').to_string();
    }
    slug
}

/// Falhas de `normalize_classes`.
#[derive(Debug, PartialEq, Eq)]
pub enum InvalidClass {
    InvalidName,
    TooMany,
}

/// Nome de classe já aparado é válido se 1..=64 chars ASCII `[A-Za-z0-9_]`
/// (espelha o CHECK `name ~ '^[A-Za-z0-9_][A-Za-z0-9_]{0,63}$'` da 0002).
/// Regra única da casa — `normalize_classes` e o PUT classes reutilizam.
pub fn is_valid_class_name(trimmed: &str) -> bool {
    !trimmed.is_empty()
        && trimmed.len() <= 64
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}
/// Trim + valida + deduplica (1ª ocorrência, case-sensitive). O cap de 200 é
/// o `maxItems` da spec sobre o array ENVIADO (antes do dedupe): com o
/// `DefaultBodyLimit` do axum em 2 MiB o body nunca encosta o limite mesmo
/// com 200 nomes no tamanho máximo (413 sem envelope continua impossível).
pub fn normalize_classes(raw: &[String]) -> Result<Vec<String>, InvalidClass> {
    if raw.len() > 200 {
        return Err(InvalidClass::TooMany);
    }
    let mut out: Vec<String> = Vec::with_capacity(raw.len());
    for name in raw {
        let trimmed = name.trim().to_string();
        if !is_valid_class_name(&trimmed) {
            return Err(InvalidClass::InvalidName);
        }
        if !out.contains(&trimmed) {
            out.push(trimmed);
        }
    }
    Ok(out)
}

/// Parse de id de path/query. `None` ⇒ o chamador responde 404 `not_found`
/// (nunca 400 — ADR-0002 D8).
pub fn parse_id(raw: &str) -> Option<Uuid> {
    raw.parse().ok()
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CreateDatasetRequest {
    pub title: String,
    pub r#type: DatasetType,
    #[serde(default)]
    pub classes: Vec<String>,
}

/// Linha de `datasets` 1:1 com as colunas (snake_case).
#[derive(sqlx::FromRow)]
pub struct DatasetRow {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub category: String,
    pub r#type: String,
    pub task: String,
    pub format: String,
    pub status: String,
    pub size_bytes: i64,
    pub images_count: i32,
    pub labeled_count: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub auto_tracked: bool,
    /// Derivado (D11): conta `images` com `deleted_at IS NOT NULL` do dataset.
    pub trash_count: i32,
}

/// Classe no wire (3b.7): objeto completo `{id, name, idx, color}` camelCase —
/// a resposta é canônica (nomes só no create request); o `id` alimenta o PUT
/// boxes (`classId`).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasetClassResponse {
    pub id: String,
    pub name: String,
    pub idx: i32,
    pub color: String,
}

impl From<(Uuid, String, i32, String)> for DatasetClassResponse {
    fn from(row: (Uuid, String, i32, String)) -> Self {
        Self {
            id: row.0.to_string(),
            name: row.1,
            idx: row.2,
            color: row.3,
        }
    }
}

/// `source` derivado no wire (ADR-0003 D5): `Some(s3://{bucket}/datasets/{id}/)`
/// quando há imagens, `None` em dataset vazio. O bucket no valor é CONFIG
/// (`StorageConfig`), não dado — a coluna morreu na migration 0003.
pub fn derived_source(bucket: &str, dataset_id: Uuid, images_count: i32) -> Option<String> {
    if images_count > 0 {
        Some(format!("s3://{bucket}/datasets/{dataset_id}/"))
    } else {
        None
    }
}

/// Ponto único de conversão snake_case (coluna) → camelCase (wire), política
/// ADR-0002 D1 — proibido `AS "camelCase"` em SQL ou `rename` individual.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasetResponse {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub category: String,
    pub r#type: String,
    pub task: String,
    pub format: String,
    pub status: String,
    /// Coluna removida pela migration 0003 (ADR-0003 D5); no wire permanece, `null` até a derivação `s3://…` da 3b.7.
    pub source: Option<String>,
    pub size_bytes: i64,
    pub images_count: i32,
    pub labeled_count: i32,
    pub created_at: DateTime<Utc>,
    /// Coluna `updated_at` exposta como `lastModified` (contrato `Dataset`).
    pub last_modified: DateTime<Utc>,
    pub classes: Vec<DatasetClassResponse>,
    pub auto_tracked: bool,
    pub trash_count: i32,
}

impl From<DatasetRow> for DatasetResponse {
    fn from(row: DatasetRow) -> Self {
        Self {
            id: row.id.to_string(),
            slug: row.slug,
            title: row.title,
            category: row.category,
            r#type: row.r#type,
            task: row.task,
            format: row.format,
            status: row.status,
            source: None,
            size_bytes: row.size_bytes,
            images_count: row.images_count,
            labeled_count: row.labeled_count,
            created_at: row.created_at,
            last_modified: row.updated_at,
            classes: Vec::new(),
            auto_tracked: row.auto_tracked,
            trash_count: row.trash_count,
        }
    }
}

/// Item por arquivo do upload (ADR-0003 D2: status por item).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadItem {
    /// None em rejected.
    pub image_id: Option<String>,
    /// Nome canônico server-side (stem sanitizado + extensão do sniff),
    /// não o nome enviado no form.
    pub filename: String,
    /// "stored"|"duplicate"|"rejected"|"failed".
    pub status: String,
    /// "duplicate_filename"|"unsupported_media"|"too_large"|"storage_error".
    pub reason: Option<String>,
    pub bytes: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadResult {
    pub items: Vec<UploadItem>,
}

/// Linha de `images` (construída à mão no handler; md5/sha256 ficam no
/// banco, fora do wire e fora desta struct).
pub struct ImageRow {
    pub id: Uuid,
    pub filename: String,
    pub object_key: String,
    pub bytes: i64,
    pub width: i32,
    pub height: i32,
    pub media_type: String,
    pub split: String,
    pub created_at: DateTime<Utc>,
}

/// Ponto único de conversão para o wire (ADR-0002 D1);
/// `url` é computada no handler. md5/sha256 ficam no banco,
/// fora do wire 3b.3.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageResponse {
    pub id: String,
    pub filename: String,
    pub object_key: String,
    pub bytes: i64,
    pub width: i32,
    pub height: i32,
    pub media_type: String,
    pub split: String,
    pub url: String,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boxes_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
}

impl From<ImageRow> for ImageResponse {
    fn from(row: ImageRow) -> Self {
        Self {
            id: row.id.to_string(),
            filename: row.filename,
            object_key: row.object_key,
            bytes: row.bytes,
            width: row.width,
            height: row.height,
            media_type: row.media_type,
            split: row.split,
            url: String::new(),
            created_at: row.created_at,
            boxes_count: None,
            caption: None,
        }
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagePage {
    pub items: Vec<ImageResponse>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

/// Item de `boxes` no wire (3b.5, ADR-0003 D6). Linhas chegam como tupla
/// `query_as` no handler (estilo da casa: `ImageRow` também não usa
/// `FromRow`); o ponto único de conversão é o `From` abaixo.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoxResponse {
    pub id: String,
    pub class_id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub conf: Option<f64>,
    pub origin: String,
    pub track_id: Option<i32>,
}

impl
    From<(
        Uuid,
        Uuid,
        f64,
        f64,
        f64,
        f64,
        Option<f64>,
        String,
        Option<i32>,
    )> for BoxResponse
{
    fn from(
        row: (
            Uuid,
            Uuid,
            f64,
            f64,
            f64,
            f64,
            Option<f64>,
            String,
            Option<i32>,
        ),
    ) -> Self {
        Self {
            id: row.0.to_string(),
            class_id: row.1.to_string(),
            x: row.2,
            y: row.3,
            w: row.4,
            h: row.5,
            conf: row.6,
            origin: row.7,
            track_id: row.8,
        }
    }
}

/// Linha de `captions` no wire (PK = image_id, no máximo uma por imagem).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptionResponse {
    pub text: String,
    pub origin: String,
    pub model: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl From<(String, String, Option<String>, DateTime<Utc>)> for CaptionResponse {
    fn from(row: (String, String, Option<String>, DateTime<Utc>)) -> Self {
        Self {
            text: row.0,
            origin: row.1,
            model: row.2,
            updated_at: row.3,
        }
    }
}

/// Detalhe da imagem (3b.5, ADR-0003 delta): struct FLAT — repete todos os
/// campos de `Image` mais `boxes` + `caption`, sem envelope aninhado.
/// Floats ecoam o banco (6 decimais é assunto do PUT da 3b.6).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageDetailResponse {
    pub id: String,
    pub filename: String,
    pub object_key: String,
    pub bytes: i64,
    pub width: i32,
    pub height: i32,
    pub media_type: String,
    pub split: String,
    pub url: String,
    pub created_at: DateTime<Utc>,
    pub boxes: Vec<BoxResponse>,
    pub caption: Option<CaptionResponse>,
}

impl From<(ImageRow, Vec<BoxResponse>, Option<CaptionResponse>, String)> for ImageDetailResponse {
    fn from(parts: (ImageRow, Vec<BoxResponse>, Option<CaptionResponse>, String)) -> Self {
        let (row, boxes, caption, url) = parts;
        Self {
            id: row.id.to_string(),
            filename: row.filename,
            object_key: row.object_key,
            bytes: row.bytes,
            width: row.width,
            height: row.height,
            media_type: row.media_type,
            split: row.split,
            url,
            created_at: row.created_at,
            boxes,
            caption,
        }
    }
}

/// Box de entrada do PUT (3b.6, ADR-0003 D6). `deny_unknown_fields` segue a
/// casa (`CreateDatasetRequest`): chave estranha ⇒ 400 `invalid_request`.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputBox {
    pub class_id: Uuid,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    #[serde(default)]
    pub conf: Option<f64>,
    #[serde(default)]
    pub origin: Option<String>,
    #[serde(default)]
    pub track_id: Option<i32>,
}

/// Corpo do `PUT .../boxes`: substituição total (max 1000, anti-DoS; YOLO
/// raramente passa de centenas).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PutBoxesRequest {
    pub boxes: Vec<InputBox>,
}

/// Resposta canônica do PUT boxes (ids novos, eco do banco).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PutBoxesResponse {
    pub boxes: Vec<BoxResponse>,
}

/// Corpo do `PUT .../caption` (upsert; `deny_unknown_fields` como a casa).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PutCaptionRequest {
    pub text: String,
    #[serde(default)]
    pub origin: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

/// Box validada e normalizada: `(class_id, x, y, w, h, conf, origin, track_id)`
/// com `origin` já resolvida para o default `"manual"`.
pub type ValidatedBox = (Uuid, f64, f64, f64, f64, Option<f64>, String, Option<i32>);

/// Origens aceitas para boxes (CHECK do banco à letra).
pub fn is_valid_box_origin(s: &str) -> bool {
    matches!(s, "manual" | "autotracker" | "import")
}

/// Origens aceitas para captions (ADR-0016 D2: inclui 'autolabel').
pub fn is_valid_caption_origin(s: &str) -> bool {
    matches!(s, "manual" | "autolabel" | "autotracker" | "import")
}

/// Origens aceitas (compatibilidade / padrão de boxes).
pub fn is_valid_origin(s: &str) -> bool {
    is_valid_box_origin(s)
}

/// Validação pura do PUT boxes (sem banco, unit-testável): len ≤ 1000;
/// cada `x/y/w/h` em `0..=1`; `conf` presente ⇒ `0..=1`; `origin`
/// ausente ⇒ `"manual"`, presente ⇒ uma das 3. `Err(())` ⇒ o handler
/// responde 400 `invalid_request` (sem detalhe).
pub fn validate_boxes(req: &PutBoxesRequest) -> Result<Vec<ValidatedBox>, ()> {
    if req.boxes.len() > 1000 {
        return Err(());
    }
    let mut out = Vec::with_capacity(req.boxes.len());
    for b in &req.boxes {
        for v in [b.x, b.y, b.w, b.h] {
            if !(0.0..=1.0).contains(&v) {
                return Err(());
            }
        }
        if let Some(c) = b.conf {
            if !(0.0..=1.0).contains(&c) {
                return Err(());
            }
        }
        let origin = match &b.origin {
            None => "manual".to_string(),
            Some(s) if is_valid_box_origin(s) => s.clone(),
            Some(_) => return Err(()),
        };
        out.push((b.class_id, b.x, b.y, b.w, b.h, b.conf, origin, b.track_id));
    }
    Ok(out)
}

/// Validação pura do PUT caption: `text` com 1..=8000 chars (contagem em
/// chars, igual ao CHECK) e `origin` com o domínio estendido (inclui 'autolabel').
/// `Err(())` ⇒ 400. `caption ''` nunca nasce como linha (vira `unlabeled`).
pub fn validate_caption(
    text: &str,
    origin: Option<&str>,
    model: Option<&str>,
) -> Result<(String, String, Option<String>), ()> {
    let n = text.chars().count();
    if !(1..=8000).contains(&n) {
        return Err(());
    }
    let origin = match origin {
        None => "manual".to_string(),
        Some(s) if is_valid_caption_origin(s) => s.to_string(),
        Some(_) => return Err(()),
    };
    // Revisão 3b.6: `model` entra no banco para sempre — teto de 255 chars
    // (a rota só tem o backstop de 2 MiB do body-limit; single-user, mas o
    // custo de errar é uma linha).
    if model.is_some_and(|m| m.chars().count() > 255) {
        return Err(());
    }
    Ok((text.to_string(), origin, model.map(str::to_string)))
}

/// Item do `PUT /api/datasets/:id/classes` (3g.1): `id` ausente ⇒ criar;
/// presente ⇒ renomear preservando o id. `Uuid` no deserialize já rejeita
/// não-UUID com erro de parse (vira 400 no handler via `parse_json_body`).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PutClassesItem {
    #[serde(default)]
    pub id: Option<Uuid>,
    pub name: String,
}

/// Corpo do `PUT /api/datasets/:id/classes`: substituição total (max 200).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PutClassesRequest {
    pub classes: Vec<PutClassesItem>,
}

/// Resposta canônica do PUT classes (mesmo shape de `Dataset.classes`).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PutClassesResponse {
    pub classes: Vec<DatasetClassResponse>,
}

/// Plano de reconciliação por id (ADR-0005 D2/D3): `keep` carrega o idx
/// FINAL (posição no array do request) para a renumeração 0..n-1.
#[derive(Debug, PartialEq, Eq)]
pub struct ClassPlan {
    pub keep: Vec<(Uuid, String, usize)>,
    pub create: Vec<(String, usize)>,
    pub remove: Vec<Uuid>,
}

/// Validação pura + reconciliação do PUT classes (sem banco, unit-testável):
/// (1) array ≤ 200; (2) names válidos pela MESMA regra do create
/// (`is_valid_class_name`, sobre o nome aparado); (3) names únicos —
/// duplicado é erro, sem dedupe silencioso; (4) ids únicos; (5) id presente
/// ⇒ deve pertencer ao dataset (`existing`). `Err(())` ⇒ 400
/// `invalid_request` (inclui id de outro dataset — não vaza existência).
pub fn plan_classes(existing: &[Uuid], req: &PutClassesRequest) -> Result<ClassPlan, ()> {
    if req.classes.len() > 200 {
        return Err(());
    }
    let mut names: Vec<String> = Vec::with_capacity(req.classes.len());
    for item in &req.classes {
        let trimmed = item.name.trim().to_string();
        if !is_valid_class_name(&trimmed) {
            return Err(());
        }
        if names.contains(&trimmed) {
            return Err(());
        }
        names.push(trimmed);
    }
    let mut seen_ids: Vec<Uuid> = Vec::new();
    for item in &req.classes {
        if let Some(id) = item.id {
            if seen_ids.contains(&id) {
                return Err(());
            }
            seen_ids.push(id);
            if !existing.contains(&id) {
                return Err(());
            }
        }
    }
    let mut keep = Vec::new();
    let mut create = Vec::new();
    for (idx, (item, name)) in req.classes.iter().zip(names.iter()).enumerate() {
        match item.id {
            Some(id) => keep.push((id, name.clone(), idx)),
            None => create.push((name.clone(), idx)),
        }
    }
    let remove: Vec<Uuid> = existing
        .iter()
        .filter(|id| !seen_ids.contains(id))
        .copied()
        .collect();
    Ok(ClassPlan {
        keep,
        create,
        remove,
    })
}

// ---------------------------------------------------------------------------
// Import (fatia 3e.2, ADR-0006 D3/D4): validações puras do pacote, sem banco.
// O manifest é o de `export.rs` (fonte da verdade do roundtrip, D1); campos
// desconhecidos são permitidos no parse (forward-compat — o gate é o
// `schema_version`). `Err(())` ⇒ o handler responde 400 `import_invalid`.
// ---------------------------------------------------------------------------

/// Parse do campo multipart `replace` (D3/P3): ausente ⇒ `false`;
/// `"true"`/`"false"` literais; qualquer outro valor ⇒ `Err(())`
/// (o handler responde 400 `invalid_request`, não `import_invalid`).
pub fn parse_import_replace(raw: Option<&str>) -> Result<bool, ()> {
    match raw {
        None => Ok(false),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(_) => Err(()),
    }
}

/// Título resolvido do import (D3): form `title` quando presente e não-vazio
/// (após trim) tem precedência sobre o `title` do manifest; o resultado
/// precisa ter 1..=96 chars. `Err(())` ⇒ 400 `invalid_request`.
pub fn resolve_import_title(form: Option<&str>, manifest_title: &str) -> Result<String, ()> {
    let chosen = match form {
        Some(t) if !t.trim().is_empty() => t.trim().to_string(),
        _ => manifest_title.trim().to_string(),
    };
    let n = chosen.chars().count();
    if (1..=96).contains(&n) {
        Ok(chosen)
    } else {
        Err(())
    }
}

fn valid_dataset_type(s: &str) -> bool {
    matches!(
        s,
        "yolo_bbox" | "yolo_seg" | "difusao_lora" | "clip_image_text"
    )
}

fn valid_dataset_format(s: &str) -> bool {
    matches!(s, "yolo_txt" | "captions" | "pairs")
}

fn valid_media_type(s: &str) -> bool {
    matches!(s, "jpeg" | "png" | "webp")
}

fn valid_split(s: &str) -> bool {
    matches!(s, "train" | "val")
}

fn valid_sha256(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// `(category, task)` derivados do `type` do manifest (espelho da tabela
/// `derive` — o import não recebe category/task do cliente).
pub fn category_task_of(dataset_type: &str) -> Result<(&'static str, &'static str), ()> {
    match dataset_type {
        "yolo_bbox" => Ok(("yolo", "detect_track")),
        "yolo_seg" => Ok(("yolo", "segment")),
        "difusao_lora" => Ok(("difusao", "caption")),
        "clip_image_text" => Ok(("openclip", "embedding")),
        _ => Err(()),
    }
}

/// Validação de domínios e referências do manifest (D3/D4), sem banco nem
/// zip: gate `schema_version == 1`; `dataset.type`/`format` nos domínios do
/// CHECK; classes com `idx` denso 0..n-1 (ordem do array) e nomes válidos
/// pela regra da casa; imagens com split/media_type/dimensões/bytes/splits
/// válidos, `sha256` 64-hex, `class_idx` dentro das classes, boxes em
/// 0..=1 com `conf` em 0..=1 e `origin` no CHECK, captions com texto
/// 1..=8000 e `origin`/`model` válidos.
pub fn validate_import_manifest(m: &super::export::ExportManifest) -> Result<(), ()> {
    if m.schema_version != 1 {
        return Err(());
    }
    if !valid_dataset_type(&m.dataset.r#type) || !valid_dataset_format(&m.dataset.format) {
        return Err(());
    }
    if m.dataset.title.trim().is_empty() || m.dataset.title.chars().count() > 96 {
        return Err(());
    }
    let n_classes = m.classes.len();
    if n_classes > 200 {
        return Err(());
    }
    let mut seen: HashSet<String> = HashSet::new();
    for (pos, c) in m.classes.iter().enumerate() {
        if c.idx != pos as i32 {
            return Err(());
        }
        let trimmed = c.name.trim();
        if !is_valid_class_name(trimmed) {
            return Err(());
        }
        if !seen.insert(trimmed.to_string()) {
            return Err(());
        }
    }
    for img in &m.images {
        if img.filename.is_empty() || img.filename.chars().count() > 255 {
            return Err(());
        }
        if !valid_split(&img.split) || !valid_media_type(&img.media_type) {
            return Err(());
        }
        if img.width <= 0 || img.height <= 0 || img.bytes < 0 {
            return Err(());
        }
        if !valid_sha256(&img.sha256) {
            return Err(());
        }
        for b in &img.boxes {
            if !(0..n_classes as i32).contains(&b.class_idx) {
                return Err(());
            }
            for v in [b.x, b.y, b.w, b.h] {
                if !(0.0..=1.0).contains(&v) {
                    return Err(());
                }
            }
            if let Some(c) = b.conf {
                if !(0.0..=1.0).contains(&c) {
                    return Err(());
                }
            }
            if !is_valid_origin(&b.origin) {
                return Err(());
            }
        }
        if let Some(cap) = &img.caption {
            let n = cap.text.chars().count();
            if !(1..=8000).contains(&n) {
                return Err(());
            }
            if !is_valid_caption_origin(&cap.origin) {
                return Err(());
            }
            if cap
                .model
                .as_deref()
                .is_some_and(|m| m.chars().count() > 255)
            {
                return Err(());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basico() {
        assert_eq!(
            slugify("Inspeção PCB (Defeitos) v2"),
            "inspecao-pcb-defeitos-v2"
        );
    }

    #[test]
    fn slugify_remove_emoji() {
        let s = slugify("Ação Estratégica 🔒");
        assert_eq!(s, "acao-estrategica");
        assert!(!s.chars().any(|c| !c.is_ascii()));
    }

    #[test]
    fn slugify_casos_limite() {
        assert_eq!(slugify("🎯🔥"), "");
        assert_eq!(slugify("  Espaços  "), "espacos");
        assert_eq!(slugify(""), "");
        let long = "a".repeat(200);
        let s = slugify(&long);
        assert_eq!(s.len(), 96);
        assert!(!s.ends_with('-'));
        let with_dash = format!("{} {}", "b".repeat(95), "c");
        let t = slugify(&with_dash);
        assert!(t.len() <= 96);
        assert!(!t.ends_with('-'));
    }

    #[test]
    fn derive_exaustivo() {
        assert_eq!(
            derive(DatasetType::YoloBbox),
            ("yolo", "detect_track", "yolo_txt")
        );
        assert_eq!(
            derive(DatasetType::YoloSeg),
            ("yolo", "segment", "yolo_txt")
        );
        assert_eq!(
            derive(DatasetType::DifusaoLora),
            ("difusao", "caption", "captions")
        );
        assert_eq!(
            derive(DatasetType::ClipImageText),
            ("openclip", "embedding", "pairs")
        );
    }

    #[test]
    fn normalize_regras() {
        let v = vec!["  gato ".to_string(), "gato".to_string(), "cao".to_string()];
        assert_eq!(normalize_classes(&v).unwrap(), vec!["gato", "cao"]);
        assert_eq!(
            normalize_classes(&["solda-fria".to_string()]),
            Err(InvalidClass::InvalidName)
        );
        assert_eq!(
            normalize_classes(&["".to_string()]),
            Err(InvalidClass::InvalidName)
        );
        let many: Vec<String> = (0..201).map(|i| format!("c{i}")).collect();
        assert_eq!(normalize_classes(&many), Err(InvalidClass::TooMany));
        // 250 nomes que deduplicam para <= 200: o cap é sobre o INPUT
        // (maxItems da spec), então continua TooMany.
        let mut dup250: Vec<String> = (0..125).map(|i| format!("c{i}")).collect();
        dup250.extend((0..125).map(|i| format!("c{i}")));
        assert_eq!(dup250.len(), 250);
        assert_eq!(normalize_classes(&dup250), Err(InvalidClass::TooMany));
        let ok200: Vec<String> = (0..200).map(|i| format!("c{i}")).collect();
        assert_eq!(normalize_classes(&ok200).unwrap().len(), 200);
    }

    #[test]
    fn color_cicla() {
        assert_eq!(color_for(0), CLASS_PALETTE[0]);
        assert_eq!(color_for(6), CLASS_PALETTE[0]);
        assert_eq!(color_for(7), CLASS_PALETTE[1]);
    }

    #[test]
    fn parse_id_regras() {
        let id = Uuid::new_v4();
        assert_eq!(parse_id(&id.to_string()), Some(id));
        assert_eq!(parse_id("nao-e-uuid"), None);
        assert!(parse_id(&id.to_string().to_uppercase()).is_some());
    }

    fn req_boxes(json: &str) -> PutBoxesRequest {
        serde_json::from_str(json).expect("body de teste válido")
    }

    #[test]
    fn validate_boxes_ok_com_defaults() {
        let id = Uuid::new_v4();
        let req = req_boxes(&format!(
            r#"{{"boxes":[{{"classId":"{id}","x":0.5,"y":0.5,"w":0.2,"h":0.2,"conf":0.9,"trackId":7}}]}}"#
        ));
        let out = validate_boxes(&req).expect("ok");
        assert_eq!(
            out,
            vec![(
                id,
                0.5,
                0.5,
                0.2,
                0.2,
                Some(0.9),
                "manual".to_string(),
                Some(7)
            )]
        );
    }

    #[test]
    fn validate_boxes_x_fora() {
        let id = Uuid::new_v4();
        let req = req_boxes(&format!(
            r#"{{"boxes":[{{"classId":"{id}","x":1.0001,"y":0,"w":0,"h":0}}]}}"#
        ));
        assert_eq!(validate_boxes(&req), Err(()));
    }

    #[test]
    fn validate_boxes_conf_negativa() {
        let id = Uuid::new_v4();
        let req = req_boxes(&format!(
            r#"{{"boxes":[{{"classId":"{id}","x":0,"y":0,"w":0,"h":0,"conf":-0.1}}]}}"#
        ));
        assert_eq!(validate_boxes(&req), Err(()));
    }

    #[test]
    fn validate_boxes_origin_invalida() {
        let id = Uuid::new_v4();
        let req = req_boxes(&format!(
            r#"{{"boxes":[{{"classId":"{id}","x":0,"y":0,"w":0,"h":0,"origin":"hack"}}]}}"#
        ));
        assert_eq!(validate_boxes(&req), Err(()));
    }

    #[test]
    fn validate_boxes_limite_1000() {
        let id = Uuid::new_v4();
        let one = format!(r#"{{"classId":"{id}","x":0,"y":0,"w":0,"h":0}}"#);
        let req: PutBoxesRequest = serde_json::from_str(&format!(
            r#"{{"boxes":[{}]}}"#,
            vec![one.as_str(); 1001].join(",")
        ))
        .expect("1001 boxes parseia");
        assert_eq!(validate_boxes(&req), Err(()));
        let ok: PutBoxesRequest = serde_json::from_str(&format!(
            r#"{{"boxes":[{}]}}"#,
            vec![one.as_str(); 1000].join(",")
        ))
        .expect("1000 boxes parseia");
        assert_eq!(validate_boxes(&ok).expect("1000 passa").len(), 1000);
    }

    #[test]
    fn validate_boxes_vazia_ok() {
        let req = req_boxes(r#"{"boxes":[]}"#);
        assert_eq!(validate_boxes(&req).expect("vazia passa"), vec![]);
    }

    #[test]
    fn validate_boxes_deny_unknown() {
        let bad: Result<PutBoxesRequest, _> = serde_json::from_str(r#"{"boxes":[],"extra":1}"#);
        assert!(bad.is_err(), "deny_unknown_fields da casa");
    }

    #[test]
    fn validate_caption_regras() {
        assert!(validate_caption("", None, None).is_err());
        assert!(validate_caption("um gato", None, None).expect("ok").1 == "manual");
        assert!(validate_caption("x", Some("hack"), None).is_err());
        assert!(validate_caption("x", Some("autolabel"), None).is_ok());
        assert!(validate_caption(&"a".repeat(8000), Some("import"), None).is_ok());
        assert!(validate_caption(&"a".repeat(8001), None, None).is_err());
        // modelo com teto (revisão 3b.6): 255 chars ok, 256 -> 400.
        assert!(validate_caption("x", None, Some(&"m".repeat(255))).is_ok());
        assert!(validate_caption("x", None, Some(&"m".repeat(256))).is_err());
    }

    fn req_classes(json: &str) -> PutClassesRequest {
        serde_json::from_str(json).expect("body de teste válido")
    }

    #[test]
    fn put_classes_nome_invalido() {
        let req = req_classes(r#"{"classes":[{"name":"solda-fria"}]}"#);
        assert!(plan_classes(&[], &req).is_err());
        let req = req_classes(r#"{"classes":[{"name":""}]}"#);
        assert!(plan_classes(&[], &req).is_err());
    }

    #[test]
    fn put_classes_limite_200() {
        let one = r#"{"name":"c"}"#.to_string();
        let req: PutClassesRequest = serde_json::from_str(&format!(
            r#"{{"classes":[{}]}}"#,
            vec![one.as_str(); 201].join(",")
        ))
        .expect("201 parseia");
        assert!(plan_classes(&[], &req).is_err());
    }

    #[test]
    fn put_classes_nome_duplicado_erro() {
        let req = req_classes(r#"{"classes":[{"name":"a"},{"name":"a"}]}"#);
        assert!(plan_classes(&[], &req).is_err());
    }

    #[test]
    fn put_classes_id_duplicado_erro() {
        let id = Uuid::new_v4();
        let req = req_classes(&format!(
            r#"{{"classes":[{{"id":"{id}","name":"a"}},{{"id":"{id}","name":"b"}}]}}"#
        ));
        assert!(plan_classes(&[id], &req).is_err());
    }

    #[test]
    fn put_classes_id_nao_uuid_erro() {
        let bad: Result<PutClassesRequest, _> =
            serde_json::from_str(r#"{"classes":[{"id":"nao-e-uuid","name":"a"}]}"#);
        assert!(bad.is_err(), "id não-UUID rejeita no parse");
    }

    #[test]
    fn put_classes_id_outro_dataset_erro() {
        let ghost = Uuid::new_v4();
        let req = req_classes(&format!(r#"{{"classes":[{{"id":"{ghost}","name":"a"}}]}}"#));
        assert!(plan_classes(&[], &req).is_err());
    }

    #[test]
    fn put_classes_plano_reconciliacao() {
        let keep_id = Uuid::new_v4();
        let gone_id = Uuid::new_v4();
        let req = req_classes(&format!(
            r#"{{"classes":[{{"id":"{keep_id}","name":"novo_nome"}},{{"name":"nova_classe"}}]}}"#
        ));
        let plan = plan_classes(&[keep_id, gone_id], &req).expect("plano válido");
        assert_eq!(plan.keep, vec![(keep_id, "novo_nome".to_string(), 0)]);
        assert_eq!(plan.create, vec![("nova_classe".to_string(), 1)]);
        assert_eq!(plan.remove, vec![gone_id]);
    }

    #[test]
    fn put_classes_deny_unknown() {
        let bad: Result<PutClassesRequest, _> = serde_json::from_str(r#"{"classes":[],"extra":1}"#);
        assert!(bad.is_err(), "deny_unknown_fields da casa");
    }

    #[test]
    fn import_replace_regras() {
        assert_eq!(parse_import_replace(None), Ok(false));
        assert_eq!(parse_import_replace(Some("true")), Ok(true));
        assert_eq!(parse_import_replace(Some("false")), Ok(false));
        assert_eq!(parse_import_replace(Some("1")), Err(()));
        assert_eq!(parse_import_replace(Some("True")), Err(()));
        assert_eq!(parse_import_replace(Some("")), Err(()));
    }

    #[test]
    fn import_title_form_vence_manifest() {
        assert_eq!(
            resolve_import_title(Some(" Novo Nome "), "Antigo").unwrap(),
            "Novo Nome"
        );
        assert_eq!(
            resolve_import_title(Some("   "), "Do Manifest").unwrap(),
            "Do Manifest"
        );
        assert_eq!(
            resolve_import_title(None, "Do Manifest").unwrap(),
            "Do Manifest"
        );
        assert!(resolve_import_title(None, "").is_err());
        assert!(resolve_import_title(Some(&"a".repeat(97)), "x").is_err());
        assert!(resolve_import_title(Some(&"a".repeat(96)), "x").is_ok());
    }

    #[test]
    fn import_category_task_espelho_do_derive() {
        assert_eq!(category_task_of("yolo_bbox"), Ok(("yolo", "detect_track")));
        assert_eq!(category_task_of("yolo_seg"), Ok(("yolo", "segment")));
        assert_eq!(category_task_of("difusao_lora"), Ok(("difusao", "caption")));
        assert_eq!(
            category_task_of("clip_image_text"),
            Ok(("openclip", "embedding"))
        );
        assert_eq!(category_task_of("outro"), Err(()));
    }

    fn import_manifest_minimo() -> super::super::export::ExportManifest {
        use super::super::export::*;
        ExportManifest {
            schema_version: 1,
            exported_at: "2026-09-07T12:00:00Z".to_string(),
            dataset: ManifestDataset {
                name: "d".to_string(),
                title: "D".to_string(),
                r#type: "yolo_bbox".to_string(),
                format: "yolo_txt".to_string(),
                counts: ManifestCounts {
                    images: 1,
                    labeled: 1,
                    classes: 1,
                    size_bytes: 10,
                },
            },
            classes: vec![ManifestClass {
                idx: 0,
                name: "gato".to_string(),
                color: "#10b981".to_string(),
            }],
            images: vec![ManifestImage {
                filename: "a.jpg".to_string(),
                split: "train".to_string(),
                width: 1,
                height: 1,
                bytes: 10,
                sha256: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
                    .to_string(),
                media_type: "jpeg".to_string(),
                boxes: vec![ManifestBox {
                    class_idx: 0,
                    x: 0.5,
                    y: 0.5,
                    w: 0.2,
                    h: 0.2,
                    conf: Some(0.9),
                    origin: "autotracker".to_string(),
                    track_id: Some(7),
                }],
                caption: None,
            }],
        }
    }

    #[test]
    fn import_manifest_ok_e_gates() {
        assert!(validate_import_manifest(&import_manifest_minimo()).is_ok());
        let mut m = import_manifest_minimo();
        m.schema_version = 2;
        assert!(validate_import_manifest(&m).is_err());
        let mut m = import_manifest_minimo();
        m.images[0].boxes[0].class_idx = 5;
        assert!(validate_import_manifest(&m).is_err());
        let mut m = import_manifest_minimo();
        m.images[0].boxes[0].origin = "hack".to_string();
        assert!(validate_import_manifest(&m).is_err());
        let mut m = import_manifest_minimo();
        m.images[0].media_type = "gif".to_string();
        assert!(validate_import_manifest(&m).is_err());
        let mut m = import_manifest_minimo();
        m.classes[0].idx = 3;
        assert!(validate_import_manifest(&m).is_err());
        let mut m = import_manifest_minimo();
        m.dataset.r#type = "outro".to_string();
        assert!(validate_import_manifest(&m).is_err());
    }

    #[test]
    fn import_manifest_classe_duplicada_pos_trim_rejeita() {
        use super::super::export::ManifestClass;
        let mut m = import_manifest_minimo();
        m.classes = vec![
            ManifestClass {
                idx: 0,
                name: "a".to_string(),
                color: "#10b981".to_string(),
            },
            ManifestClass {
                idx: 1,
                name: " a ".to_string(),
                color: "#f59e0b".to_string(),
            },
        ];
        assert!(validate_import_manifest(&m).is_err());
        let mut ok = import_manifest_minimo();
        // Nomes distintos com espaços nas bordas passam (trim sem
        // falso-positivo). Nota: a regra da casa só aceita [A-Za-z0-9_],
        // então o positivo usa padding externo — "a b" com espaço interno
        // é inválido pela regra e nunca chegaria ao dedupe.
        ok.classes = vec![
            ManifestClass {
                idx: 0,
                name: " a ".to_string(),
                color: "#10b981".to_string(),
            },
            ManifestClass {
                idx: 1,
                name: "ab ".to_string(),
                color: "#f59e0b".to_string(),
            },
        ];
        assert!(validate_import_manifest(&ok).is_ok());
    }
}
