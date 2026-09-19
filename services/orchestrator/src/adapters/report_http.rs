use std::time::Duration;

use async_trait::async_trait;

use crate::domain::models::ReportBody;
use crate::ports::reporter::ReportClient;

/// Cliente HTTP que reporta ao manager via POST /internal/jobs/:id/report.
pub struct HttpReportClient {
    manager_url: String,
    token: Option<String>,
    client: reqwest::Client,
}

impl HttpReportClient {
    pub fn new(manager_url: &str, token: Option<&str>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(3))
            .build()
            .expect("reqwest client do report");
        Self {
            manager_url: manager_url.trim_end_matches('/').to_string(),
            token: token.map(|s| s.to_string()),
            client,
        }
    }
}

#[async_trait]
impl ReportClient for HttpReportClient {
    async fn report(&self, job_id: &str, body: &ReportBody) -> Result<(), String> {
        let url = format!("{}/internal/jobs/{job_id}/report", self.manager_url);
        let is_terminal = body.status == "done" || body.status == "failed";
        let max_attempts = if is_terminal { 5 } else { 2 };
        let mut last_err = "unknown report error".to_string();

        for attempt in 0..max_attempts {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(300 * (1 << attempt))).await;
            }
            let mut req = self.client.post(&url).json(body);
            if let Some(ref token) = self.token {
                req = req.header("authorization", format!("Bearer {token}"));
            }
            match req.send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        return Ok(());
                    }
                    last_err = format!("report status: {}", resp.status());
                    if resp.status().is_client_error()
                        && resp.status() != reqwest::StatusCode::TOO_MANY_REQUESTS
                    {
                        return Err(last_err);
                    }
                }
                Err(e) => {
                    last_err = format!("report request: {e}");
                }
            }
        }
        Err(last_err)
    }
}
