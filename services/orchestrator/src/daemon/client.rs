//! Cliente HTTP do daemon de inferência (D1).

use std::time::Duration;

use async_trait::async_trait;

use super::types::{GenerateBody, HealthResponse};

/// Cliente HTTP do daemon (health, generate, shutdown).
/// Implementação real usa reqwest; fake para testes.
#[async_trait]
pub trait DaemonClient: Send + Sync {
    /// Checa `/health`. Retorna `None` se o daemon não responde.
    async fn health(&self) -> Option<HealthResponse>;

    /// POST `/generate` com body JSON. Retorna Ok(()) no 200,
    /// Err("busy") no 409, Err("error:...") em outros erros.
    async fn generate(&self, body: &GenerateBody) -> Result<(), String>;

    /// POST `/shutdown`. Idempotente.
    async fn shutdown(&self) -> Result<(), String>;
}

/// Cliente HTTP real para o daemon de inferência.
pub struct HttpDaemonClient {
    base_url: String,
    client: reqwest::Client,
}

impl HttpDaemonClient {
    pub fn new(base_url: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .expect("reqwest client do daemon");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
        }
    }
}

#[async_trait]
impl DaemonClient for HttpDaemonClient {
    async fn health(&self) -> Option<HealthResponse> {
        let url = format!("{}/health", self.base_url);
        let resp = self.client.get(&url).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        resp.json::<HealthResponse>().await.ok()
    }

    async fn generate(&self, body: &GenerateBody) -> Result<(), String> {
        let url = format!("{}/generate", self.base_url);
        let resp = self
            .client
            .post(&url)
            .json(body)
            .send()
            .await
            .map_err(|e| format!("daemon generate request: {e}"))?;

        match resp.status().as_u16() {
            200 => Ok(()),
            409 => Err("busy".to_string()),
            status => {
                let text = resp.text().await.unwrap_or_default();
                Err(format!("daemon generate error {status}: {text}"))
            }
        }
    }

    async fn shutdown(&self) -> Result<(), String> {
        let url = format!("{}/shutdown", self.base_url);
        let _ = self.client.post(&url).send().await;
        Ok(())
    }
}
