//! Estratégias de spawn/kill do daemon (D1).

use std::time::Duration;

use async_trait::async_trait;

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

        // Rede: usa network_name definido ou fallback para ENGINE_NETWORK / infra_default (nunca host).
        let net = self.network_name.clone().unwrap_or_else(|| {
            std::env::var("ENGINE_NETWORK").unwrap_or_else(|_| "infra_default".to_string())
        });
        args.push("--network".to_string());
        args.push(net);

        // uid do engine — mesmo contrato do executor one-shot (DockerExecutor):
        // sem isto o daemon (USER studio, uid 1000) não consegue escrever nos
        // diretórios de job criados pelo orquestrador (root) no dataset compartilhado.
        if let Ok(user) = std::env::var("ENGINE_USER") {
            if !user.is_empty() {
                args.push("--user".to_string());
                args.push(user);
            }
        }

        // Detached mode (D1) — sem --rm (daemon persiste)
        args.push("-d".to_string());

        for (host, container) in &self.volumes {
            args.push("-v".to_string());
            args.push(format!("{host}:{container}"));
        }

        if let Some(devices) = &self.gpu_devices {
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
