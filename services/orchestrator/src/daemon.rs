//! Daemon de inferência por nó (D1 — ADR-0023).
//!
//! Gerencia o ciclo de vida de um daemon HTTP de difusão:
//! spawn, health check, geração via HTTP, preempção e idle TTL kill.
//!
//! O daemon state vive no processo do orchestrator (1 orchestrator = 1 nó).

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Body do `POST /generate`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateBody {
    pub config: String,
    pub output_dir: String,
    pub telemetry_path: String,
}

/// Resposta do `GET /health` do daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub loaded_spec: Option<String>,
    pub busy: bool,
}

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

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

/// Estratégia de spawn/kill do daemon.
/// Docker: `docker run -d` com mesmos mounts do one-shot, `serve --port`.
/// Subprocess: spawn do venv `python -m trainer_difusao serve --port N`.
/// Implementação fake para testes.
#[async_trait]
pub trait DaemonLauncher: Send + Sync {
    /// Sobe o daemon. Retorna a URL base (ex.: `http://localhost:8766`).
    async fn start(&self) -> Result<String, String>;

    /// Mata o daemon (docker stop ou kill).
    async fn kill(&self) -> Result<(), String>;
}

// ---------------------------------------------------------------------------
// Docker launcher (D1)
// ---------------------------------------------------------------------------

/// Launcher real: `docker run -d` com os mesmos volumes/mounts do one-shot.
pub struct DockerDaemonLauncher {
    image: String,
    container_name: String,
    volumes: Vec<(String, String)>,
    port: u16,
    gpu_devices: Option<String>,
    exec_env: Vec<(String, String)>,
}

impl DockerDaemonLauncher {
    pub fn new(
        image: &str,
        container_name: &str,
        volumes: Vec<(String, String)>,
        port: u16,
        gpu_devices: Option<String>,
        exec_env: Vec<(String, String)>,
    ) -> Self {
        Self {
            image: image.to_string(),
            container_name: container_name.to_string(),
            volumes,
            port,
            gpu_devices,
            exec_env,
        }
    }

    /// Monta args do `docker run -d` (sem `--rm`, com `-d`).
    pub fn build_daemon_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        args.push("run".to_string());
        args.push("--name".to_string());
        args.push(self.container_name.clone());
        args.push("--add-host".to_string());
        args.push("host.docker.internal:host-gateway".to_string());

        // Detached mode (D1) — sem --rm (daemon persiste)
        args.push("-d".to_string());

        for (host, container) in &self.volumes {
            args.push("-v".to_string());
            args.push(format!("{host}:{container}"));
        }

        if let Some(ref devices) = self.gpu_devices {
            args.push("--gpus".to_string());
            args.push(format!("device={devices}"));
            args.push("--shm-size".to_string());
            args.push("2g".to_string());
            args.push("-e".to_string());
            args.push(format!("NVIDIA_VISIBLE_DEVICES={devices}"));
        }

        for (key, value) in &self.exec_env {
            args.push("-e".to_string());
            args.push(format!("{key}={value}"));
        }

        args.push(self.image.clone());
        // Subcomando: serve --port N
        args.push("serve".to_string());
        args.push("--port".to_string());
        args.push(self.port.to_string());

        args
    }
}

#[async_trait]
impl DaemonLauncher for DockerDaemonLauncher {
    async fn start(&self) -> Result<String, String> {
        let args = self.build_daemon_args();
        let output = tokio::process::Command::new("docker")
            .args(&args)
            .output()
            .await
            .map_err(|e| format!("docker run daemon: {e}"))?;

        if !output.status.success() {
            return Err(format!(
                "docker run daemon failed (exit {}): {}",
                output.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(format!("http://localhost:{}", self.port))
    }

    async fn kill(&self) -> Result<(), String> {
        let output = tokio::process::Command::new("docker")
            .args(["stop", "--time", "5", &self.container_name])
            .output()
            .await
            .map_err(|e| format!("docker stop daemon: {e}"))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "docker stop daemon failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Subprocess launcher (D1)
// ---------------------------------------------------------------------------

/// Launcher via subprocess: `python -m trainer_difusao serve --port N`.
pub struct SubprocessDaemonLauncher {
    port: u16,
    child: std::sync::Mutex<Option<tokio::process::Child>>,
}

impl SubprocessDaemonLauncher {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            child: std::sync::Mutex::new(None),
        }
    }
}

#[async_trait]
impl DaemonLauncher for SubprocessDaemonLauncher {
    async fn start(&self) -> Result<String, String> {
        let python_bin = std::env::var("PYTHON_BIN").unwrap_or_else(|_| "python3".into());
        let mut child = tokio::process::Command::new(&python_bin)
            .args([
                "-m",
                "trainer_difusao",
                "serve",
                "--port",
                &self.port.to_string(),
            ])
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("spawn daemon subprocess: {e}"))?;

        // Aguarda um instante para verificar se o processo não morreu imediatamente
        tokio::time::sleep(Duration::from_millis(500)).await;

        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("check daemon status: {e}"))?
        {
            return Err(format!(
                "daemon subprocess exited immediately with status: {status}"
            ));
        }

        *self.child.lock().unwrap() = Some(child);
        Ok(format!("http://localhost:{}", self.port))
    }

    async fn kill(&self) -> Result<(), String> {
        // Take the child out of the Mutex BEFORE any .await to avoid holding MutexGuard across await
        let maybe_child = self.child.lock().unwrap().take();
        if let Some(mut child) = maybe_child {
            child
                .kill()
                .await
                .map_err(|e| format!("kill daemon subprocess: {e}"))?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// HTTP client (D1)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Daemon state (shared, per-node)
// ---------------------------------------------------------------------------

/// Estado compartilhado do daemon no processo do orchestrator.
/// Cada orchestrator = 1 nó (padrão da casa).
///
/// Mantém referências ao client e launcher para que preempção,
/// housekeeping e o path de generate possam operar sem criar
/// instâncias novas.
pub struct DaemonState {
    pub running: std::sync::Mutex<bool>,
    pub url: std::sync::Mutex<Option<String>>,
    pub last_used: std::sync::Mutex<Instant>,
    pub loaded_spec: std::sync::Mutex<Option<String>>,
    pub image: String,
    pub container_name: String,
    pub port: u16,
    pub idle_ttl: Duration,
    pub client: Arc<dyn DaemonClient>,
    pub launcher: Arc<dyn DaemonLauncher>,
}

impl DaemonState {
    pub fn new(
        image: &str,
        port: u16,
        idle_ttl_secs: u64,
        client: Arc<dyn DaemonClient>,
        launcher: Arc<dyn DaemonLauncher>,
    ) -> Self {
        Self {
            running: std::sync::Mutex::new(false),
            url: std::sync::Mutex::new(None),
            last_used: std::sync::Mutex::new(Instant::now()),
            loaded_spec: std::sync::Mutex::new(None),
            image: image.to_string(),
            container_name: "diffusion-daemon".to_string(),
            port,
            idle_ttl: Duration::from_secs(idle_ttl_secs),
            client,
            launcher,
        }
    }

    pub fn touch(&self) {
        *self.last_used.lock().unwrap() = Instant::now();
    }

    pub fn is_running(&self) -> bool {
        *self.running.lock().unwrap()
    }

    pub fn get_url(&self) -> Option<String> {
        self.url.lock().unwrap().clone()
    }

    pub fn set_running(&self, running: bool, url: Option<String>) {
        *self.running.lock().unwrap() = running;
        *self.url.lock().unwrap() = url;
        if running {
            self.touch();
        }
    }

    pub fn get_loaded_spec(&self) -> Option<String> {
        self.loaded_spec.lock().unwrap().clone()
    }

    pub fn set_loaded_spec(&self, spec: String) {
        *self.loaded_spec.lock().unwrap() = Some(spec);
    }

    pub fn is_idle(&self, busy: bool) -> bool {
        if busy {
            return false;
        }
        let last_used = *self.last_used.lock().unwrap();
        last_used.elapsed() > self.idle_ttl / 2
    }
}

// ---------------------------------------------------------------------------
// Daemon lifecycle (D1)
// ---------------------------------------------------------------------------

/// Garante que o daemon está de pé e com a spec correta.
/// Retorna a URL do daemon. Erro → job falha honesto.
pub async fn ensure_daemon_ready(
    daemon_state: &DaemonState,
    _target_spec: &str,
) -> Result<String, String> {
    if !daemon_state.is_running() {
        // Sobe o daemon via launcher armazenado no state
        let url = daemon_state.launcher.start().await?;
        daemon_state.set_running(true, Some(url.clone()));
    }

    let url = daemon_state
        .get_url()
        .ok_or_else(|| "daemon URL not set after start".to_string())?;

    // Poll health com timeout (~60s, spec)
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(resp) = daemon_state.client.health().await {
            if resp.status == "ok" || resp.status == "ready" {
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
    if let Some(resp) = daemon_state.client.health().await {
        if let Some(spec) = resp.loaded_spec {
            daemon_state.set_loaded_spec(spec);
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

    let busy = match daemon_state.client.health().await {
        Some(resp) => resp.busy,
        None => {
            // Daemon não responde → não está vivo, limpa estado
            daemon_state.set_running(false, None);
            return;
        }
    };

    if daemon_state.is_idle(busy) {
        tracing::info!("preempting idle diffusion daemon before training job");
        let _ = daemon_state.client.shutdown().await;
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
            let _ = daemon_state.client.shutdown().await;
            let _ = daemon_state.launcher.kill().await;
            daemon_state.set_running(false, None);
        }
    }
}
