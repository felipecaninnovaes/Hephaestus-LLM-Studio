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

/// Timeout das chamadas curtas (`/health`, `/shutdown`).
pub const SHORT_CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Teto do `POST /generate`. A resposta HTTP só chega no FIM da geração, então o
/// timeout precisa cobrir carga a frio do modelo (ex.: Klein 9B 4-bit) + todos os
/// steps. Um teto de 30s derrubava o job enquanto o daemon seguia gerando
/// (incidente 2026-10-06). 30 min é limitado (daemon travado não prende o job
/// para sempre) e folgado vs. ~5 min observados no 9B.
pub const DEFAULT_GENERATE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Cliente HTTP real para o daemon de inferência.
pub struct HttpDaemonClient {
    base_url: String,
    client: reqwest::Client,
    generate_timeout: Duration,
}

impl HttpDaemonClient {
    pub fn new(base_url: &str) -> Self {
        Self::with_generate_timeout(base_url, DEFAULT_GENERATE_TIMEOUT)
    }

    /// Como `new`, com teto próprio para `/generate` (as demais chamadas usam
    /// `SHORT_CALL_TIMEOUT`; não há timeout global no client).
    pub fn with_generate_timeout(base_url: &str, generate_timeout: Duration) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .build()
            .expect("reqwest client do daemon");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
            generate_timeout,
        }
    }
}

#[async_trait]
impl DaemonClient for HttpDaemonClient {
    async fn health(&self) -> Option<HealthResponse> {
        let url = format!("{}/health", self.base_url);
        let resp = self
            .client
            .get(&url)
            .timeout(SHORT_CALL_TIMEOUT)
            .send()
            .await
            .ok()?;
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
            .timeout(self.generate_timeout)
            .json(body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    format!(
                        "daemon generate timed out after {}s (daemon may still be running)",
                        self.generate_timeout.as_secs()
                    )
                } else {
                    format!("daemon generate request: {e}")
                }
            })?;

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
        let _ = self
            .client
            .post(&url)
            .timeout(SHORT_CALL_TIMEOUT)
            .send()
            .await;
        Ok(())
    }
}
