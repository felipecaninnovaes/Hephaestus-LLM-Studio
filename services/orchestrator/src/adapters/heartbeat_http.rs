use std::time::Duration;

use async_trait::async_trait;

use crate::domain::models::HeartbeatBody;
use crate::ports::heartbeat::HeartbeatClient;

pub struct HttpHeartbeatClient {
    manager_url: String,
    token: Option<String>,
    client: reqwest::Client,
}

impl HttpHeartbeatClient {
    pub fn new(manager_url: &str, token: Option<&str>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(3))
            .build()
            .expect("reqwest client do heartbeat");
        Self {
            manager_url: manager_url.trim_end_matches('/').to_string(),
            token: token.map(|s| s.to_string()),
            client,
        }
    }
}

#[async_trait]
impl HeartbeatClient for HttpHeartbeatClient {
    async fn send(&self, body: &HeartbeatBody) -> Result<(), String> {
        let url = format!("{}/internal/heartbeat", self.manager_url);
        let mut req = self.client.post(&url).json(body);
        if let Some(ref token) = self.token {
            req = req.header("authorization", format!("Bearer {token}"));
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("heartbeat request: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("heartbeat status: {}", resp.status()));
        }
        Ok(())
    }
}
