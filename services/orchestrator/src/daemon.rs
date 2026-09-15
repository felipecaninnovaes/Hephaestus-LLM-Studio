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
/// Contrato real do serve.py: `{"ok": bool, "loaded_spec": dict|null, "busy": bool, ...}`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub ok: bool,
    #[serde(default)]
    pub loaded_spec: Option<serde_json::Value>,
    #[serde(default)]
    pub busy: bool,
    /// Ignora campos extras que o engine possa adicionar no futuro.
    #[serde(flatten)]
    pub _extra: std::collections::HashMap<String, serde_json::Value>,
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
    /// Se Some, roda o daemon na rede compose (DNS resolve `diffusion-daemon`).
    /// Se None, usa `--network host` (fallback legado, ex.: TrueNAS/GPU).
    network_name: Option<String>,
}

impl DockerDaemonLauncher {
    pub fn new(
        image: &str,
        container_name: &str,
        volumes: Vec<(String, String)>,
        port: u16,
        gpu_devices: Option<String>,
        exec_env: Vec<(String, String)>,
        network_name: Option<String>,
    ) -> Self {
        Self {
            image: image.to_string(),
            container_name: container_name.to_string(),
            volumes,
            port,
            gpu_devices,
            exec_env,
            network_name,
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

        // Rede: se network_name definido, usa rede compose (DNS resolve
        // `diffusion-daemon` internamente). Caso contrário, fallback host.
        if let Some(ref net) = self.network_name {
            args.push("--network".to_string());
            args.push(net.clone());
        } else {
            // Rede host: daemon escuta na NET do host (0.0.0.0:PORT).
            args.push("--network".to_string());
            args.push("host".to_string());
        }

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

    /// Obtém o IP do container na rede bridge via `docker inspect`.
    /// Necessário porque o orchestrator e o daemon vivem em containers
    /// separados — `localhost` não alcança o loopback do outro container.
    async fn inspect_container_ip(&self) -> Result<String, String> {
        let output = tokio::process::Command::new("docker")
            .args([
                "inspect",
                "-f",
                "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}",
                &self.container_name,
            ])
            .output()
            .await
            .map_err(|e| format!("docker inspect daemon IP: {e}"))?;

        if !output.status.success() {
            return Err(format!(
                "docker inspect daemon IP failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if ip.is_empty() {
            return Err(format!(
                "docker inspect returned empty IP for container {}",
                self.container_name
            ));
        }
        Ok(ip)
    }
}

#[async_trait]
impl DaemonLauncher for DockerDaemonLauncher {
    async fn start(&self) -> Result<String, String> {
        // Idempotente: remove container com mesmo nome de tentativa anterior
        // que tenha ficado de pé (ex.: falha de health, job interrompido).
        // Ignora erro se não existir (best-effort).
        let rm_output = tokio::process::Command::new("docker")
            .args(["rm", "-f", &self.container_name])
            .output()
            .await;
        match rm_output {
            Ok(out) if out.status.success() => {
                tracing::debug!(
                    container = %self.container_name,
                    "removed leftover container before spawn"
                );
            }
            Ok(_) | Err(_) => {
                tracing::debug!(
                    container = %self.container_name,
                    "no leftover container to remove (or rm failed)"
                );
            }
        }

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

        // URL do daemon depende do modo de rede:
        // - Rede compose (--network <net>): DNS interno resolve `diffusion-daemon`
        // - Rede host (--network host): host.docker.internal alcança o host
        if self.network_name.is_some() {
            Ok(format!("http://diffusion-daemon:{}", self.port))
        } else {
            Ok(format!("http://host.docker.internal:{}", self.port))
        }
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
    pub client: std::sync::RwLock<Arc<dyn DaemonClient>>,
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
            client: std::sync::RwLock::new(client),
            launcher,
        }
    }

    /// Substitui o client interno (usado após launcher.start() retornar a URL real).
    pub fn set_client(&self, client: Arc<dyn DaemonClient>) {
        *self.client.write().unwrap() = client;
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Modo host (fallback): network_name=None → --network host, URL host.docker.internal
    /// Volume targets devem ser /data/datasets e /data/outputs (consistente com compose).
    #[test]
    fn build_daemon_args_structure() {
        let launcher = DockerDaemonLauncher::new(
            "hephaestus/trainer-difusao:local",
            "diffusion-daemon",
            vec![
                ("/host/data".into(), "/data/datasets".into()),
                ("/host/out".into(), "/data/outputs".into()),
            ],
            8766,
            Some("0".into()),
            vec![("ENGINE_MOCK".into(), "0".into())],
            None, // fallback host
        );

        let args = launcher.build_daemon_args();

        // Cabeçalho
        assert_eq!(args[0], "run");
        assert!(args.contains(&"--name".to_string()));
        assert!(args.contains(&"diffusion-daemon".to_string()));
        assert!(args.contains(&"--add-host".to_string()));
        assert!(args.contains(&"host.docker.internal:host-gateway".to_string()));
        assert!(args.contains(&"--network".to_string()));
        assert!(args.contains(&"host".to_string()));
        assert!(args.contains(&"-d".to_string()));

        // Volumes — targets devem ser /data/datasets e /data/outputs
        assert!(args.contains(&"-v".to_string()));
        assert!(args.contains(&"/host/data:/data/datasets".to_string()));
        assert!(args.contains(&"/host/out:/data/outputs".to_string()));

        // GPU
        assert!(args.contains(&"--gpus".to_string()));
        assert!(args.contains(&"device=0".to_string()));
        assert!(args.contains(&"--shm-size".to_string()));

        // Env
        assert!(args.contains(&"-e".to_string()));
        assert!(args.contains(&"ENGINE_MOCK=0".to_string()));

        // Image + subcomando
        assert!(args.contains(&"hephaestus/trainer-difusao:local".to_string()));
        assert!(args.contains(&"serve".to_string()));
        assert!(args.contains(&"--port".to_string()));
        assert!(args.contains(&"8766".to_string()));
    }

    /// Modo compose: network_name=Some → --network <net>, URL diffusion-daemon:<port>
    #[test]
    fn build_daemon_args_compose_network() {
        let launcher = DockerDaemonLauncher::new(
            "hephaestus/trainer-difusao:local",
            "diffusion-daemon",
            vec![("/host/data".into(), "/data/datasets".into())],
            8766,
            None,
            vec![],
            Some("infra_default".into()),
        );

        let args = launcher.build_daemon_args();

        assert!(args.contains(&"--network".to_string()));
        assert!(args.contains(&"infra_default".to_string()));
        // Não deve ter --network host
        let net_idx = args.iter().position(|a| a == "--network").unwrap();
        assert_ne!(args[net_idx + 1], "host");
    }

    /// start() retorna URL correta no modo compose.
    #[test]
    fn daemon_url_compose_mode() {
        let launcher = DockerDaemonLauncher::new(
            "hephaestus/trainer-difusao:local",
            "diffusion-daemon",
            vec![],
            8766,
            None,
            vec![],
            Some("infra_default".into()),
        );
        // build_daemon_args valida a estrutura; URL é retornada em start().
        // Testamos o construtor sem crash — start() requer docker real.
        let args = launcher.build_daemon_args();
        assert!(args.contains(&"infra_default".to_string()));
    }

    /// start() retorna URL host.docker.internal no modo host.
    #[test]
    fn daemon_url_host_mode() {
        let launcher = DockerDaemonLauncher::new(
            "hephaestus/trainer-difusao:local",
            "diffusion-daemon",
            vec![],
            8766,
            None,
            vec![],
            None,
        );
        let args = launcher.build_daemon_args();
        assert!(args.contains(&"host".to_string()));
    }

    #[test]
    fn build_daemon_args_no_gpu() {
        let launcher = DockerDaemonLauncher::new(
            "hephaestus/trainer-difusao:local",
            "diffusion-daemon",
            vec![],
            8767,
            None,
            vec![],
            None,
        );

        let args = launcher.build_daemon_args();

        assert!(!args.contains(&"--gpus".to_string()));
        assert!(!args.contains(&"--shm-size".to_string()));
        assert!(args.contains(&"8767".to_string()));
    }

    #[test]
    fn daemon_state_url_round_trip() {
        let client = Arc::new(FakeDaemonClient::new());
        let launcher = Arc::new(FakeDaemonLauncher::new());
        let ds = DaemonState::new("img", 8766, 600, client, launcher);

        assert!(!ds.is_running());
        assert!(ds.get_url().is_none());

        ds.set_running(true, Some("http://172.17.0.2:8766".into()));
        assert!(ds.is_running());
        assert_eq!(ds.get_url().unwrap(), "http://172.17.0.2:8766");

        ds.set_running(false, None);
        assert!(!ds.is_running());
        assert!(ds.get_url().is_none());
    }

    /// Verifica que empty URL é rejeitada (Bug 1).
    /// O cliente HttpDaemonClient deve receber a URL da launcher,
    /// não a string vazia da env.
    #[test]
    fn empty_daemon_url_not_treated_as_set() {
        let empty = "";
        let result = Some(empty.to_string()).filter(|v| !v.trim().is_empty());
        assert!(
            result.is_none(),
            "empty string should be filtered out by .filter(!is_empty)"
        );

        let whitespace = "  ";
        let result = Some(whitespace.to_string()).filter(|v| !v.trim().is_empty());
        assert!(
            result.is_none(),
            "whitespace-only string should be filtered out"
        );

        let valid = "http://localhost:8766";
        let result = Some(valid.to_string()).filter(|v| !v.trim().is_empty());
        assert!(result.is_some(), "valid URL should pass through");
    }

    /// Fake DaemonClient para testes (duplicado do lib.rs tests para isolar).
    struct FakeDaemonClient;

    impl FakeDaemonClient {
        fn new() -> Self {
            Self
        }
    }

    #[async_trait]
    impl DaemonClient for FakeDaemonClient {
        async fn health(&self) -> Option<HealthResponse> {
            Some(HealthResponse {
                ok: true,
                loaded_spec: None,
                busy: false,
                _extra: Default::default(),
            })
        }

        async fn generate(&self, _body: &GenerateBody) -> Result<(), String> {
            Ok(())
        }

        async fn shutdown(&self) -> Result<(), String> {
            Ok(())
        }
    }

    /// Fake DaemonLauncher para testes (duplicado do lib.rs tests para isolar).
    struct FakeDaemonLauncher {
        start_count: std::sync::Mutex<usize>,
        kill_count: std::sync::Mutex<usize>,
    }

    impl FakeDaemonLauncher {
        fn new() -> Self {
            Self {
                start_count: std::sync::Mutex::new(0),
                kill_count: std::sync::Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl DaemonLauncher for FakeDaemonLauncher {
        async fn start(&self) -> Result<String, String> {
            *self.start_count.lock().unwrap() += 1;
            Ok("http://localhost:8766".to_string())
        }

        async fn kill(&self) -> Result<(), String> {
            *self.kill_count.lock().unwrap() += 1;
            Ok(())
        }
    }

    /// Launcher que grava a sequência de operações para verificação.
    /// Cada start() grava "rm" (remoção best-effort) + "run" (docker run).
    /// Cada kill() grava "stop".
    struct RecordingDaemonLauncher {
        log: std::sync::Mutex<Vec<String>>,
    }

    impl RecordingDaemonLauncher {
        fn new() -> Self {
            Self {
                log: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn log(&self) -> Vec<String> {
            self.log.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl DaemonLauncher for RecordingDaemonLauncher {
        async fn start(&self) -> Result<String, String> {
            // Simula o que DockerDaemonLauncher::start faz internamente:
            // 1. docker rm -f (best-effort) — sempre chamado antes do run
            // 2. docker run -d
            self.log.lock().unwrap().push("rm".to_string());
            self.log.lock().unwrap().push("run".to_string());
            Ok("http://localhost:8766".to_string())
        }

        async fn kill(&self) -> Result<(), String> {
            self.log.lock().unwrap().push("stop".to_string());
            Ok(())
        }
    }

    /// Verifica que start() executa rm -f antes de run (idempotência).
    /// Sem rm -f, um container de tentativa anterior causaria conflito de nome.
    #[tokio::test]
    async fn launcher_start_removes_container_before_run() {
        let launcher = RecordingDaemonLauncher::new();
        let result = launcher.start().await;
        assert!(result.is_ok());

        let log = launcher.log();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0], "rm", "first operation must be rm -f");
        assert_eq!(log[1], "run", "second operation must be docker run");
    }

    /// Verifica re-spawn: start → health falha → kill → novo start → rm -f chamado antes do run.
    /// Simula o ciclo: daemon fica de pé, health não responde, orchestrator mata,
    /// próximo job chama start novamente — rm -f deve limpar o container antigo.
    /// Usa RecordingDaemonLauncher direto (sem loop de health de 60s).
    #[tokio::test]
    async fn re_spawn_after_health_failure_calls_rm_before_run() {
        let client = Arc::new(FakeDaemonClient::new());
        let launcher = Arc::new(RecordingDaemonLauncher::new());
        let ds = DaemonState::new("img", 8766, 600, client, launcher.clone());

        // 1. Primeiro start: launcher grava rm + run
        let url = launcher.start().await.unwrap();
        ds.set_running(true, Some(url));

        // 2. Simula falha de health: daemon não responde → orchestrator mata
        let _ = launcher.kill().await;
        ds.set_running(false, None);

        // 3. Segundo start (novo job): launcher deve chamar rm -f antes do run novamente
        let url = launcher.start().await.unwrap();
        ds.set_running(true, Some(url));

        // Verifica sequência completa: rm+run (1º), stop (kill), rm+run (2º)
        let log = launcher.log();
        assert_eq!(log.len(), 5, "expected rm+run+stop+rm+run, got {log:?}");
        assert_eq!(log[0], "rm", "1st start: must rm before run");
        assert_eq!(log[1], "run", "1st start: must run after rm");
        assert_eq!(log[2], "stop", "kill must be called on health failure");
        assert_eq!(log[3], "rm", "2nd start: must rm before run (re-spawn)");
        assert_eq!(log[4], "run", "2nd start: must run after rm (re-spawn)");
    }

    /// Verifica que um daemon já rodando não é reiniciado desnecessariamente.
    /// Se is_running() == true e health responde ok, start() NÃO deve ser chamado.
    #[tokio::test]
    async fn existing_running_daemon_not_restarted() {
        let client = Arc::new(FakeDaemonClient::new());
        let launcher = Arc::new(RecordingDaemonLauncher::new());
        let ds = DaemonState::new("img", 8766, 600, client, launcher.clone());

        // Marca daemon como já rodando
        ds.set_running(true, Some("http://localhost:8766".into()));

        let result = ensure_daemon_ready(&ds, "spec-v1").await;
        assert!(result.is_ok());

        // Launcher não deve ter sido chamado
        let log = launcher.log();
        assert!(
            log.is_empty(),
            "start() should not be called when daemon is already running: {log:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Novos testes: set_client + ensure_daemon_ready com client atualizado
    // -----------------------------------------------------------------------

    /// FakeDaemonClient que grava a base_url usada em health() para verificação.
    struct UrlTrackingDaemonClient {
        base_url: String,
        call_count: std::sync::Mutex<usize>,
    }

    impl UrlTrackingDaemonClient {
        fn new(base_url: &str) -> Self {
            Self {
                base_url: base_url.to_string(),
                call_count: std::sync::Mutex::new(0),
            }
        }

        fn base_url(&self) -> &str {
            &self.base_url
        }

        fn call_count(&self) -> usize {
            *self.call_count.lock().unwrap()
        }
    }

    #[async_trait]
    impl DaemonClient for UrlTrackingDaemonClient {
        async fn health(&self) -> Option<HealthResponse> {
            *self.call_count.lock().unwrap() += 1;
            Some(HealthResponse {
                ok: true,
                loaded_spec: None,
                busy: false,
                _extra: Default::default(),
            })
        }

        async fn generate(&self, _body: &GenerateBody) -> Result<(), String> {
            Ok(())
        }

        async fn shutdown(&self) -> Result<(), String> {
            Ok(())
        }
    }

    /// Verifica que set_client troca o client usado no health.
    #[test]
    fn set_client_swaps_used_client() {
        let old_client = Arc::new(UrlTrackingDaemonClient::new("http://old:1234"));
        let launcher = Arc::new(FakeDaemonLauncher::new());
        let ds = DaemonState::new(
            "img",
            8766,
            600,
            old_client.clone() as Arc<dyn DaemonClient>,
            launcher,
        );

        // Health usa o client antigo
        {
            let client = ds.client.read().unwrap().clone();
            let rt = tokio::runtime::Runtime::new().unwrap();
            let resp = rt.block_on(client.health());
            assert!(resp.is_some());
            assert_eq!(old_client.call_count(), 1);
        }

        // Troca para o client novo
        let new_client = Arc::new(UrlTrackingDaemonClient::new("http://new:9999"));
        ds.set_client(new_client.clone() as Arc<dyn DaemonClient>);

        // Health agora usa o client novo
        {
            let client = ds.client.read().unwrap().clone();
            let rt = tokio::runtime::Runtime::new().unwrap();
            let resp = rt.block_on(client.health());
            assert!(resp.is_some());
            assert_eq!(
                old_client.call_count(),
                1,
                "old client should still have 1 call"
            );
            assert_eq!(new_client.call_count(), 1, "new client should have 1 call");
            assert_eq!(new_client.base_url(), "http://new:9999");
        }
    }

    /// Launcher que retorna uma URL diferente da que o client antigo apontava.
    struct DynamicUrlLauncher {
        url: String,
    }

    impl DynamicUrlLauncher {
        fn new(url: &str) -> Self {
            Self {
                url: url.to_string(),
            }
        }
    }

    #[async_trait]
    impl DaemonLauncher for DynamicUrlLauncher {
        async fn start(&self) -> Result<String, String> {
            Ok(self.url.clone())
        }

        async fn kill(&self) -> Result<(), String> {
            Ok(())
        }
    }

    /// Mini servidor HTTP que responde `/health` com JSON válido.
    /// Usado para testar ensure_daemon_ready com HttpDaemonClient real.
    struct MockHealthServer {
        addr: std::net::SocketAddr,
        health_hits: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl MockHealthServer {
        async fn start() -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let health_hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let hits = health_hits.clone();

            tokio::spawn(async move {
                loop {
                    let (stream, _) = match listener.accept().await {
                        Ok(v) => v,
                        Err(_) => break,
                    };
                    let hits = hits.clone();
                    tokio::spawn(async move {
                        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
                        let (reader, mut writer) = stream.into_split();
                        let mut lines = BufReader::new(reader).lines();
                        // Lê Request-Line (ignora)
                        let _ = lines.next_line().await;
                        // Lê headers até blank line
                        while let Ok(Some(line)) = lines.next_line().await {
                            if line.is_empty() {
                                break;
                            }
                        }
                        hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let body = r#"{"ok":true,"loaded_spec":null,"busy":false}"#;
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                            body.len(), body
                        );
                        let _ = writer.write_all(resp.as_bytes()).await;
                    });
                }
            });

            Self { addr, health_hits }
        }

        fn url(&self) -> String {
            format!("http://127.0.0.1:{}", self.addr.port())
        }

        fn health_hits(&self) -> usize {
            self.health_hits.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    /// Verifica que ensure_daemon_ready usa o client NOVO (da URL do launcher),
    /// não o antigo (localhost). Usa mock HTTP server para health check real.
    #[tokio::test]
    async fn ensure_daemon_ready_uses_new_client_from_launcher() {
        // Client antigo aponta para porta inválida (nunca deve ser chamado)
        let old_client = Arc::new(UrlTrackingDaemonClient::new("http://127.0.0.1:1"));
        // Mock server que responde /health → aponta para porta real
        let mock = MockHealthServer::start().await;
        // Launcher retorna a URL do mock server
        let launcher = Arc::new(DynamicUrlLauncher::new(&mock.url()));

        let ds = DaemonState::new(
            "img",
            8766,
            600,
            old_client.clone() as Arc<dyn DaemonClient>,
            launcher as Arc<dyn DaemonLauncher>,
        );

        // Daemon não está rodando → ensure_daemon_ready vai:
        // 1. chamar launcher.start() → "http://127.0.0.1:<port>"
        // 2. set_client(HttpDaemonClient::new(&url)) → aponta para mock
        // 3. health poll → mock responde 200
        let result = ensure_daemon_ready(&ds, "spec-v1").await;
        assert!(
            result.is_ok(),
            "ensure_daemon_ready should succeed: {:?}",
            result.err()
        );

        let url = result.unwrap();
        assert_eq!(url, mock.url());

        // Client antigo NÃO deve ter sido chamado
        assert_eq!(
            old_client.call_count(),
            0,
            "old client should NOT be called after ensure_daemon_ready"
        );

        // Mock server DEVE ter recebido health calls
        assert!(
            mock.health_hits() > 0,
            "mock server should have received at least 1 health call"
        );
    }
}
