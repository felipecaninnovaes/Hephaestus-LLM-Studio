use std::time::Duration;

use async_trait::async_trait;

use crate::ports::executor::TrainerExecutor;

/// Executor real via CLI docker (EXEC_MODE=docker, default).
pub struct DockerExecutor;

/// Monta os argumentos do `docker run` para teste (D7).
/// Retorna os argumentos que seriam passados ao `docker run` (sem o binário).
pub fn build_docker_run_args(
    image: &str,
    container_name: &str,
    volumes: &[(String, String)],
    args: &[String],
    env: &[(String, String)],
    gpu_devices: Option<&str>,
) -> Vec<String> {
    let mut cmd_args = Vec::new();
    cmd_args.push("run".to_string());
    cmd_args.push("--rm".to_string());
    cmd_args.push("--name".to_string());
    cmd_args.push(container_name.to_string());
    cmd_args.push("--add-host".to_string());
    cmd_args.push("host.docker.internal:host-gateway".to_string());

    let network = std::env::var("ENGINE_NETWORK")
        .or_else(|_| std::env::var("DIFFUSION_DAEMON_NETWORK"))
        .unwrap_or_else(|_| "infra_default".to_string());
    cmd_args.push("--network".to_string());
    cmd_args.push(network);

    if let Ok(user) = std::env::var("ENGINE_USER") {
        if !user.is_empty() {
            cmd_args.push("--user".to_string());
            cmd_args.push(user);
        }
    }

    for (host, container) in volumes {
        cmd_args.push("-v".to_string());
        cmd_args.push(format!("{host}:{container}"));
    }

    // GPU flags (D4/D7): só quando gpu_devices está setado.
    if let Some(devices) = gpu_devices {
        cmd_args.push("--gpus".to_string());
        cmd_args.push(format!("device={devices}"));
        cmd_args.push("--shm-size".to_string());
        cmd_args.push("2g".to_string());
        cmd_args.push("-e".to_string());
        cmd_args.push(format!("NVIDIA_VISIBLE_DEVICES={devices}"));
    }

    // Env extras (ENGINE_MOCK=0, etc.)
    for (key, value) in env {
        cmd_args.push("-e".to_string());
        cmd_args.push(format!("{key}={value}"));
    }

    cmd_args.push(image.to_string());
    cmd_args.extend_from_slice(args);

    cmd_args
}

#[async_trait]
impl TrainerExecutor for DockerExecutor {
    async fn run(
        &self,
        image: &str,
        container_name: &str,
        volumes: &[(String, String)],
        args: &[String],
        env: &[(String, String)],
        gpu_devices: Option<&str>,
    ) -> (i32, String) {
        let cmd_args =
            build_docker_run_args(image, container_name, volumes, args, env, gpu_devices);

        let mut cmd = tokio::process::Command::new("docker");
        cmd.args(&cmd_args);
        cmd.kill_on_drop(true);

        let timeout_secs: u64 = std::env::var("TRAINER_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(7200);
        let timeout_duration = Duration::from_secs(timeout_secs);

        match tokio::time::timeout(timeout_duration, cmd.output()).await {
            Ok(Ok(o)) => {
                let exit_code = o.status.code().unwrap_or(-1);
                let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                let logs = format!("{stdout}\n{stderr}");
                (exit_code, logs)
            }
            Ok(Err(e)) => (-1, format!("docker exec error: {e}")),
            Err(_) => {
                let _ = self.stop(container_name).await;
                (
                    -1,
                    format!("timeout de execução atingido ({timeout_secs}s) - container encerrado"),
                )
            }
        }
    }

    async fn stop(&self, container_name: &str) -> Result<(), String> {
        let output = tokio::process::Command::new("docker")
            .args(["stop", "--time", "5", container_name])
            .output()
            .await
            .map_err(|e| format!("docker stop error: {e}"))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "docker stop failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }
}
