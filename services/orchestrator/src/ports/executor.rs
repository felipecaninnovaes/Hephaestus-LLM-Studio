use async_trait::async_trait;

#[async_trait]
pub trait TrainerExecutor: Send + Sync {
    /// Executa o trainer. Retorna (exit_code, logs_completos).
    ///
    /// `env` — variáveis de ambiente extras (ex.: `ENGINE_MOCK=0`).
    /// `gpu_devices` — lista de índices nvidia-smi (ex.: `"0"` ou `"0,1"`).
    ///   `Some(v)` → `--gpus "device={v}"` + `-e NVIDIA_VISIBLE_DEVICES={v}` +
    ///   `--shm-size=2g` + envs repassados. `None` → comportamento padrão.
    async fn run(
        &self,
        image: &str,
        container_name: &str,
        volumes: &[(String, String)], // (host_path, container_path)
        args: &[String],              // argumentos após a imagem (ex.: train --config …)
        env: &[(String, String)],     // variáveis de ambiente extras
        gpu_devices: Option<&str>,    // índices nvidia-smi (ex.: "0")
    ) -> (i32, String);

    /// Para um container (abort via docker stop --time 5 → exit 137).
    async fn stop(&self, container_name: &str) -> Result<(), String>;
}
