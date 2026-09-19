//! Ciclo de vida do daemon de inferência (D1).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::client::HttpDaemonClient;
use super::state::DaemonState;

/// Garante que o daemon está de pé e com a spec correta.
/// Retorna a URL do daemon. Erro → job falha honesto.
pub async fn ensure_daemon_ready(
    daemon_state: &DaemonState,
    _target_spec: &str,
) -> Result<String, String> {
    if !daemon_state.is_running() {
        // Sobe o daemon via launcher armazenado no state
        let url = daemon_state.launcher.start().await?;
        // Atualiza o client com a URL real retornada pelo launcher
        daemon_state.set_client(Arc::new(HttpDaemonClient::new(&url)));
        daemon_state.set_running(true, Some(url.clone()));
    }

    let url = daemon_state
        .get_url()
        .ok_or_else(|| "daemon URL not set after start".to_string())?;

    // Poll health com timeout (~60s, spec)
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        // Clone do Arc antes de await — lock liberado imediatamente
        let client = daemon_state.client.read().unwrap().clone();
        if let Some(resp) = client.health().await {
            if resp.ok {
                // Daemon pronto — health consultada (D1)
                break;
            }
        }

        if Instant::now() > deadline {
            // Timeout — mata o daemon e retorna erro
            let _ = daemon_state.launcher.kill().await;
            daemon_state.set_running(false, None);
            return Err("daemon health timeout after 60s".to_string());
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // Atualiza spec carregada (D1: health consulted for loaded_spec)
    {
        let client = daemon_state.client.read().unwrap().clone();
        if let Some(resp) = client.health().await {
            if let Some(spec_val) = resp.loaded_spec {
                // loaded_spec é dict|str; serializa para string para armazenamento
                let spec_str = match &spec_val {
                    serde_json::Value::String(s) => s.clone(),
                    other => serde_json::to_string(other).unwrap_or_default(),
                };
                daemon_state.set_loaded_spec(spec_str);
            }
        }
    }

    daemon_state.touch();
    Ok(url)
}

/// Preempção: ANTES de despachar um job de TREINO, se daemon idle → kill.
/// busy → NÃO mata; o roteamento de VRAM do manager já protege (D1).
pub async fn maybe_preempt_daemon(daemon_state: &DaemonState) {
    if !daemon_state.is_running() {
        return;
    }

    let client = daemon_state.client.read().unwrap().clone();
    let busy = match client.health().await {
        Some(resp) => resp.busy,
        None => {
            // Daemon não responde → não está vivo, limpa estado
            daemon_state.set_running(false, None);
            return;
        }
    };

    if daemon_state.is_idle(busy) {
        tracing::info!("preempting idle diffusion daemon before training job");
        let _ = client.shutdown().await;
        let _ = daemon_state.launcher.kill().await;
        daemon_state.set_running(false, None);
    }
}

/// Housekeeping: mata daemon após idle TTL sem uso.
/// Executa como tokio task de fundo.
pub async fn idle_ttl_housekeeping(daemon_state: Arc<DaemonState>) {
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        interval.tick().await;

        if !daemon_state.is_running() {
            continue;
        }

        let last_used = *daemon_state.last_used.lock().unwrap();
        let elapsed = last_used.elapsed();

        if elapsed > daemon_state.idle_ttl {
            tracing::info!(
                idle_secs = elapsed.as_secs(),
                ttl_secs = daemon_state.idle_ttl.as_secs(),
                "diffusion daemon idle TTL exceeded — killing"
            );
            let client = daemon_state.client.read().unwrap().clone();
            let _ = client.shutdown().await;
            let _ = daemon_state.launcher.kill().await;
            daemon_state.set_running(false, None);
        }
    }
}
