use serde::{Deserialize, Serialize};

/// Peso/modelo persistido (tabela models — ADR-0012 D2, ADR-0023 D4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelItem {
    pub id: String,
    pub name: String,
    pub engine: String,
    #[serde(default)]
    pub model: Option<String>,
    pub source: String,
    #[serde(rename = "hash", alias = "md5")]
    pub md5: String,
    pub bytes: i64,
    pub path: String,
    #[serde(default)]
    pub job_id: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
}

impl ModelItem {
    pub fn hash(&self) -> &str {
        &self.md5
    }
}

/// Modelo público retornado pelo manager (camelCase wire — D6 ADR-0012, ADR-0023 D4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelResponse {
    pub id: String,
    pub name: String,
    pub engine: String,
    #[serde(default)]
    pub model: Option<String>,
    pub source: String,
    pub bytes: i64,
    #[serde(rename = "hash", alias = "md5")]
    pub md5: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub job_id: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
}

impl ModelResponse {
    pub fn hash(&self) -> &str {
        &self.md5
    }
}

/// Generation retornada pelo manager (snake_case interno).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerationItem {
    pub id: String,
    /// `None` quando o job de origem foi expurgado (AC-003: galeria sobrevive).
    #[serde(default)]
    pub job_id: Option<String>,
    pub s3_key: String,
    #[serde(default)]
    pub thumb_s3_key: Option<String>,
    pub filename: String,
    pub seed: i64,
    pub prompt: String,
    #[serde(default)]
    pub negative_prompt: Option<String>,
    pub width: i32,
    pub height: i32,
    #[serde(default)]
    pub params: serde_json::Value,
    pub created_at: String,
    #[serde(default)]
    pub deleted_at: Option<String>,
}

pub type GenerationRow = GenerationItem;

/// Uso de storage retornado pelo manager (snake_case interno).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StorageUsageResponse {
    pub artifacts_bytes: i64,
    #[serde(default)]
    pub models_bytes: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_item_hash_and_md5() {
        let json = serde_json::json!({
            "id": "model-1",
            "name": "best.pt",
            "engine": "yolo",
            "source": "train",
            "hash": "abc123md5",
            "bytes": 1024,
            "path": "runs/exp/weights/best.pt",
            "created_at": "2026-03-01T00:00:00Z"
        });

        let item: ModelItem = serde_json::from_value(json).unwrap();
        assert_eq!(item.md5, "abc123md5");
        assert_eq!(item.hash(), "abc123md5");

        let serialized = serde_json::to_string(&item).unwrap();
        assert!(serialized.contains("\"hash\":\"abc123md5\""));
    }

    #[test]
    fn test_storage_usage_response() {
        let json = serde_json::json!({
            "artifacts_bytes": 500,
            "models_bytes": 1000
        });

        let resp: StorageUsageResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.artifacts_bytes, 500);
        assert_eq!(resp.models_bytes, 1000);
    }
}
