//! Regras puras de datasets (Fatia 3a): derivação e validação sem banco.
//!
//! Toda regra aqui é testável sem Postgres; sqlx aparece só no `FromRow`.

use chrono::{DateTime, Utc};
use uuid::Uuid;

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

/// Trim + valida + deduplica (1ª ocorrência, case-sensitive). Cap de 200 para
/// o body nunca encostar o `DefaultBodyLimit` do axum (413 sem envelope).
pub fn normalize_classes(raw: &[String]) -> Result<Vec<String>, InvalidClass> {
    let mut out: Vec<String> = Vec::with_capacity(raw.len().min(200));
    for name in raw {
        let trimmed = name.trim().to_string();
        if trimmed.is_empty()
            || trimmed.len() > 64
            || !trimmed
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(InvalidClass::InvalidName);
        }
        if !out.contains(&trimmed) {
            out.push(trimmed);
        }
    }
    if out.len() > 200 {
        return Err(InvalidClass::TooMany);
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
    pub source: Option<String>,
    pub size_bytes: i64,
    pub images_count: i32,
    pub labeled_count: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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
    pub source: Option<String>,
    pub size_bytes: i64,
    pub images_count: i32,
    pub labeled_count: i32,
    pub created_at: DateTime<Utc>,
    /// Coluna `updated_at` exposta como `lastModified` (contrato `Dataset`).
    pub last_modified: DateTime<Utc>,
    pub classes: Vec<String>,
    pub auto_tracked: bool,
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
            source: row.source,
            size_bytes: row.size_bytes,
            images_count: row.images_count,
            labeled_count: row.labeled_count,
            created_at: row.created_at,
            last_modified: row.updated_at,
            classes: Vec::new(),
            auto_tracked: false,
        }
    }
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
        assert_eq!(derive(DatasetType::YoloSeg), ("yolo", "segment", "yolo_txt"));
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
}
