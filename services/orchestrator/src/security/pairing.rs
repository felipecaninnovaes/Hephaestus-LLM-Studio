use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Pairing (D5.1-2 — single-use em memória)
// ---------------------------------------------------------------------------

/// Estado do pairing code no orquestrador.
/// O `used` flag é single-use: 1ª chamada com código correto consome; 2ª → false.
pub struct PairingState {
    pub code: String,
    pub used: std::sync::atomic::AtomicBool,
}

impl PairingState {
    pub fn new(code: String) -> Self {
        Self {
            code,
            used: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Verifica o código e consome se válido (single-use, atômico via compare_exchange).
    pub fn verify(&self, code: &str) -> bool {
        if self.code == code {
            self.used
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        } else {
            false
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct PairingVerifyRequest {
    pub code: String,
}

#[derive(Debug, Serialize)]
pub struct PairingVerifyResponse {
    pub valid: bool,
}

/// Resolve o URL de advertise do orquestrador.
///
/// Se o valor for `None` ou string vazia, retorna o default
/// `http://orchestrator-local:8082`. Função pura — sem side effects.
pub fn resolve_advertise_url(env_val: Option<&str>) -> String {
    match env_val {
        Some(v) if !v.is_empty() => v.to_string(),
        _ => "http://orchestrator-local:8082".into(),
    }
}

/// Gera um pairing code aleatório no formato `heph_p_<32hex>`.
pub fn generate_pairing_code() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    let hex = hex::encode(bytes);
    format!("heph_p_{hex}")
}
