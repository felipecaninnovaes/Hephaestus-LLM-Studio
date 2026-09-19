use async_trait::async_trait;

use crate::ports::executor::TrainerExecutor;

/// Executor via subprocess (EXEC_MODE=subprocess, não é caminho de aceite R5).
pub struct SubprocessExecutor;

#[async_trait]
impl TrainerExecutor for SubprocessExecutor {
    async fn run(
        &self,
        _image: &str,
        _container_name: &str,
        _volumes: &[(String, String)],
        _args: &[String],
        _env: &[(String, String)],
        _gpu_devices: Option<&str>,
    ) -> (i32, String) {
        // Subprocess mode: tenta rodar o trainer diretamente.
        // Não é o caminho de aceite — falha honestamente se o pacote não estiver instalado.
        (-1, "subprocess mode not supported in v1".to_string())
    }

    async fn stop(&self, _container_name: &str) -> Result<(), String> {
        Ok(())
    }
}
