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

/// Calcula o tempo de espera do próximo heartbeat com backoff adaptativo e jitter (§P2-1).
///
/// - Se `consecutive_failures == 0`: retorna `Duration::from_secs(base_interval_secs)`.
/// - Se `consecutive_failures > 0`: calcula backoff exponencial `(base * 2^failures).min(30s)` + jitter (0..1000ms).
pub fn compute_heartbeat_backoff(
    base_interval_secs: u64,
    consecutive_failures: u32,
    jitter_ms: u64,
) -> Duration {
    if consecutive_failures == 0 {
        Duration::from_secs(base_interval_secs)
    } else {
        let exp_secs = (base_interval_secs * 2u64.pow(consecutive_failures.min(4))).min(30);
        Duration::from_millis(exp_secs * 1000 + (jitter_ms % 1000))
    }
}
