//! Porta de embedding + `MockEmbedder` + `HttpEmbedder` (3f.2).
//!
//! `EmbeddingPort` é o análogo de `StoragePort` para vetores (ADR-0004 D1):
//! imagens e textos entram, vetores L2-normalizados de dim fixa saem.

use base64::Engine as _;
use sha2::Digest as _;

/// Dimensão fixa dos vetores (ViT-B-32).
pub const DIM: usize = 512;

/// Erro da porta de embedding.
///
/// Doc: mapeia a 503 `embedding_unavailable` no handler (fatia 3f.5).
#[derive(Debug)]
pub enum EmbeddingError {
    /// Embedder indisponível ou falha de rede/timeout/HTTP não-200
    /// (detalhe estático; nunca URL/credencial).
    Unavailable(String),
    /// Resposta 200 com shape divergente do contrato (len/id/dim/vector).
    InvalidResponse(String),
}

impl std::fmt::Display for EmbeddingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(_) => write!(f, "embedding unavailable"),
            Self::InvalidResponse(_) => write!(f, "embedding invalid response"),
        }
    }
}

impl std::error::Error for EmbeddingError {}

/// Config do embedder lida no boot: model entra no JSON do `/embed`.
#[derive(Debug, Clone)]
pub struct EmbedderConfig {
    /// Nome do modelo (default `ViT-B-32`).
    pub model: String,
}

/// Porta de embedding: batch na mesma ordem do input.
#[async_trait::async_trait]
pub trait EmbeddingPort: Send + Sync {
    /// Imagens: retorna vetores NA MESMA ORDEM do input.
    /// id do wire = uuid string (ecoado pelo servidor).
    async fn embed_images(
        &self,
        items: &[(uuid::Uuid, Vec<u8>)],
    ) -> Result<Vec<(uuid::Uuid, Vec<f32>)>, EmbeddingError>;
    /// Textos: retorna vetores na mesma ordem
    /// (id do wire = índice do array — contrato do serve.py).
    async fn embed_texts(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError>;
    /// Nome do modelo servido.
    fn model(&self) -> &str;
}

/// Vetor determinístico do spike (paridade Rust ≡ Python provada na 3f.0):
/// SHA-256 encadeado, 4×u53 por digest mapeados a [-1, 1), L2 em f64,
//  conversão para f32 NO FIM.
pub(crate) fn mock_vector(payload: &[u8]) -> Vec<f32> {
    const MASK: u64 = (1 << 53) - 1;
    const SCALE: f64 = 9007199254740992.0; // 2^53
    let mut h: [u8; 32] = sha2::Sha256::digest(payload).into();
    let mut vals: Vec<f64> = Vec::with_capacity(DIM);
    while vals.len() < DIM {
        h = sha2::Sha256::digest(h).into();
        for i in 0..4 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&h[8 * i..8 * i + 8]);
            let q = u64::from_le_bytes(b) & MASK;
            vals.push((q as f64 / SCALE) * 2.0 - 1.0);
        }
    }
    vals.truncate(DIM);
    let norm = vals.iter().map(|v| v * v).sum::<f64>().sqrt();
    vals.into_iter().map(|v| (v / norm) as f32).collect()
}

/// Mock unit, sem rede, default dev: vetor do payload = bytes da imagem
/// (`embed_images`) ou bytes UTF-8 do texto (`embed_texts`).
pub struct MockEmbedder;

impl MockEmbedder {
    /// Cria o mock (sem estado).
    pub fn new() -> Self {
        Self
    }
}

impl Default for MockEmbedder {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl EmbeddingPort for MockEmbedder {
    async fn embed_images(
        &self,
        items: &[(uuid::Uuid, Vec<u8>)],
    ) -> Result<Vec<(uuid::Uuid, Vec<f32>)>, EmbeddingError> {
        Ok(items
            .iter()
            .map(|(id, bytes)| (*id, mock_vector(bytes)))
            .collect())
    }

    async fn embed_texts(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        Ok(texts.iter().map(|t| mock_vector(t.as_bytes())).collect())
    }

    fn model(&self) -> &str {
        "ViT-B-32"
    }
}

/// Embedder HTTP: POST `/embed` e `/embed-text` no servidor do embedder.
pub struct HttpEmbedder {
    base_url: String,
    client: reqwest::Client,
    model: String,
}

impl HttpEmbedder {
    /// Constrói com timeout de request 30s e connect timeout 5s.
    pub fn new(base_url: String, model: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("reqwest client do HttpEmbedder");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
            model,
        }
    }

    async fn post_items(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<Vec<EmbedItem>, EmbeddingError> {
        let url = format!("{}{path}", self.base_url);
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| EmbeddingError::Unavailable(format!("embedder request: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(EmbeddingError::Unavailable(format!(
                "embedder status: {status}"
            )));
        }
        let parsed: EmbedResponse = resp
            .json()
            .await
            .map_err(|e| EmbeddingError::InvalidResponse(format!("embedder body: {e}")))?;
        Ok(parsed.items)
    }

    fn check_item(id: &str, item: &EmbedItem) -> Result<Vec<f32>, EmbeddingError> {
        if item.id != serde_json::Value::from(id) {
            return Err(EmbeddingError::InvalidResponse(format!(
                "embedder id mismatch: esperado {id}"
            )));
        }
        check_dim(item)
    }
}

fn check_dim(item: &EmbedItem) -> Result<Vec<f32>, EmbeddingError> {
    if item.dim != DIM || item.vector.len() != DIM {
        return Err(EmbeddingError::InvalidResponse(format!(
            "embedder dim divergente: dim={} vector.len={}",
            item.dim,
            item.vector.len()
        )));
    }
    Ok(item.vector.iter().map(|v| *v as f32).collect())
}

#[derive(Debug, serde::Deserialize)]
struct EmbedItem {
    id: serde_json::Value,
    vector: Vec<f64>,
    dim: usize,
}

#[derive(Debug, serde::Deserialize)]
struct EmbedResponse {
    items: Vec<EmbedItem>,
}

#[async_trait::async_trait]
impl EmbeddingPort for HttpEmbedder {
    async fn embed_images(
        &self,
        items: &[(uuid::Uuid, Vec<u8>)],
    ) -> Result<Vec<(uuid::Uuid, Vec<f32>)>, EmbeddingError> {
        let mut out = Vec::with_capacity(items.len());
        for chunk in items.chunks(32) {
            let wire: Vec<serde_json::Value> = chunk
                .iter()
                .map(|(id, bytes)| {
                    serde_json::json!({
                        "id": id.to_string(),
                        "b64": base64::engine::general_purpose::STANDARD.encode(bytes),
                    })
                })
                .collect();
            let body = serde_json::json!({ "model": self.model, "items": wire });
            let got = self.post_items("/embed", body).await?;
            if got.len() != chunk.len() {
                return Err(EmbeddingError::InvalidResponse(format!(
                    "embedder len divergente: esperado {} recebeu {}",
                    chunk.len(),
                    got.len()
                )));
            }
            for ((id, _), item) in chunk.iter().zip(got.iter()) {
                out.push((*id, Self::check_item(&id.to_string(), item)?));
            }
        }
        Ok(out)
    }

    async fn embed_texts(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let body = serde_json::json!({ "model": self.model, "texts": texts });
        let got = self.post_items("/embed-text", body).await?;
        if got.len() != texts.len() {
            return Err(EmbeddingError::InvalidResponse(format!(
                "embedder len divergente: esperado {} recebeu {}",
                texts.len(),
                got.len()
            )));
        }
        got.iter()
            .enumerate()
            .map(|(i, item)| {
                if item.id != serde_json::Value::from(i) {
                    return Err(EmbeddingError::InvalidResponse(format!(
                        "embedder id mismatch: esperado {i}"
                    )));
                }
                check_dim(item)
            })
            .collect()
    }

    fn model(&self) -> &str {
        &self.model
    }
}
