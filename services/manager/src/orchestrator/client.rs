//! Cliente HTTP e trait mockável do orquestrador (MM-07).

use async_trait::async_trait;

#[async_trait]
pub trait OrchestratorClient: Send + Sync {
    async fn post(&self, url: &str, body: &serde_json::Value) -> Result<(), String>;
    async fn post_json(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, String>;
}

/// Cliente HTTP real do orquestrador.
pub struct HttpOrchestratorClient {
    pub client: reqwest::Client,
    pub token: Option<String>,
}

impl HttpOrchestratorClient {
    pub fn new(token: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("reqwest client do orchestrator");
        Self { client, token }
    }
}

#[async_trait]
impl OrchestratorClient for HttpOrchestratorClient {
    async fn post(&self, url: &str, body: &serde_json::Value) -> Result<(), String> {
        let mut req = self.client.post(url).json(body);
        if let Some(t) = &self.token {
            req = req.header("Authorization", format!("Bearer {t}"));
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("orchestrator request: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("orchestrator status: {status}"));
        }
        Ok(())
    }

    async fn post_json(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let mut req = self.client.post(url).json(body);
        if let Some(t) = &self.token {
            req = req.header("Authorization", format!("Bearer {t}"));
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("orchestrator request: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("orchestrator status: {status}"));
        }
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("orchestrator response body: {e}"))?;
        Ok(json)
    }
}
