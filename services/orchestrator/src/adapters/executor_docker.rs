use std::collections::VecDeque;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex as StdMutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::ports::executor::TrainerExecutor;

/// Nº de linhas finais mantidas em memória para o `logs_tail` do erro
/// (`app/mod.rs:1062`): o output completo já vai para `run.log` em
/// streaming, então não precisamos mais acumular o processo inteiro em RAM.
const TAIL_LINES: usize = 20;

/// Teto de bytes de `run.log` por job (Fatia 1c/§6: "truncagem com marcador
/// acima de 1 GiB por job" — default conservador de 256 MiB, `RUN_LOG_MAX_BYTES`
/// sobrescreve). Acima do teto, o escritor para de gravar linhas novas e grava
/// UMA linha marcador; os pipes continuam sendo drenados (nunca pausar leitura,
/// ou o processo trava em write() quando o pipe do SO enche).
fn run_log_max_bytes() -> u64 {
    std::env::var("RUN_LOG_MAX_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(256 * 1024 * 1024)
}

/// Escritor incremental de `run.log`: epoch-millis (sem dependência de
/// crate de data/hora — nenhuma já existe no Cargo.toml do orchestrator) +
/// nome da stream (`stdout`/`stderr`) + linha, uma por escrita. Falha de
/// escrita (disco cheio, permissão) vira WARN e desliga a persistência para
/// o resto do job — nunca aborta a execução do trainer (§2/regra do coordenador).
struct RunLogWriter {
    file: Option<tokio::fs::File>,
    bytes_written: u64,
    max_bytes: u64,
    truncated: bool,
    path: std::path::PathBuf,
}

impl RunLogWriter {
    async fn open(path: &Path) -> Self {
        let file = match tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
        {
            Ok(f) => Some(f),
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "falha ao abrir run.log para escrita — log do job não será persistido (WARN, job segue)"
                );
                None
            }
        };
        Self {
            file,
            bytes_written: 0,
            max_bytes: run_log_max_bytes(),
            truncated: false,
            path: path.to_path_buf(),
        }
    }

    async fn write_line(&mut self, stream_name: &str, line: &str) {
        let Some(file) = self.file.as_mut() else {
            return;
        };
        if self.truncated {
            return;
        }
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let rendered = format!("{ts} {stream_name} {line}\n");
        if self.bytes_written + rendered.len() as u64 > self.max_bytes {
            self.truncated = true;
            let marker = format!("[run.log truncado em {} bytes]\n", self.bytes_written);
            if let Err(e) = file.write_all(marker.as_bytes()).await {
                tracing::warn!(path = %self.path.display(), error = %e, "falha ao gravar marcador de truncamento em run.log");
            }
            let _ = file.flush().await;
            return;
        }
        if let Err(e) = file.write_all(rendered.as_bytes()).await {
            tracing::warn!(path = %self.path.display(), error = %e, "falha ao gravar linha em run.log — persistência desligada para o resto do job (WARN)");
            self.file = None;
            return;
        }
        self.bytes_written += rendered.len() as u64;
    }
}

/// Divide bytes crus do container em linhas completas, tratando `\r`
/// (carriage-return "nu", padrão de barra de progresso tqdm/Rich) como um
/// reset da linha corrente: só o trecho após o ÚLTIMO `\r` antes do próximo
/// `\n` é mantido — sem isso, `BufReader::lines` (que só quebra em `\n`)
/// acumularia uma única linha gigante com a barra de progresso reescrita
/// centenas de vezes por segundo. `\n` sempre fecha e emite a linha
/// corrente (separador real).
///
/// `MAX_PENDING_BYTES` evita crescimento sem limite em memória: uma linha
/// sem `\n`/`\r` (ex.: engine com bug emitindo um `print` gigante sem
/// quebra) excedendo o teto é emitida como um "pedaço" e o acúmulo
/// recomeça do zero — o escritor segue, só a linha fica partida em vários
/// registros no `run.log`.
const MAX_PENDING_BYTES: usize = 64 * 1024;

#[derive(Default)]
struct CrCollapsingSplitter {
    pending: Vec<u8>,
}

impl CrCollapsingSplitter {
    fn feed(&mut self, chunk: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        for &b in chunk {
            match b {
                b'\n' => {
                    out.push(String::from_utf8_lossy(&self.pending).into_owned());
                    self.pending.clear();
                }
                b'\r' => self.pending.clear(),
                _ => {
                    self.pending.push(b);
                    if self.pending.len() >= MAX_PENDING_BYTES {
                        out.push(String::from_utf8_lossy(&self.pending).into_owned());
                        self.pending.clear();
                    }
                }
            }
        }
        out
    }

    /// Flush final: EOF sem `\n` à direita — mesma convenção de
    /// `tokio::io::Lines`, que devolve o resto pendente como última linha.
    fn finish(mut self) -> Option<String> {
        if self.pending.is_empty() {
            None
        } else {
            Some(String::from_utf8_lossy(&std::mem::take(&mut self.pending)).into_owned())
        }
    }
}

async fn emit_line(
    line: String,
    stream_name: &'static str,
    writer: &Arc<tokio::sync::Mutex<RunLogWriter>>,
    tail: &Arc<StdMutex<VecDeque<String>>>,
) {
    {
        let mut t = tail.lock();
        if t.len() == TAIL_LINES {
            t.pop_front();
        }
        t.push_back(line.clone());
    }
    writer.lock().await.write_line(stream_name, &line).await;
}

/// Drena uma stream (`stdout`/`stderr`) do container linha a linha (com
/// colapso de `\r`, ver `CrCollapsingSplitter`), alimentando o `run.log`
/// compartilhado e o ring buffer das últimas `TAIL_LINES` para o
/// `logs_tail` do erro.
async fn pump_stream<R>(
    mut reader: R,
    stream_name: &'static str,
    writer: Arc<tokio::sync::Mutex<RunLogWriter>>,
    tail: Arc<StdMutex<VecDeque<String>>>,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let mut splitter = CrCollapsingSplitter::default();
    let mut buf = [0u8; 8192];
    loop {
        let n = match reader.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        for line in splitter.feed(&buf[..n]) {
            emit_line(line, stream_name, &writer, &tail).await;
        }
    }
    if let Some(line) = splitter.finish() {
        emit_line(line, stream_name, &writer, &tail).await;
    }
}

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
        run_log_path: &Path,
    ) -> (i32, String) {
        let cmd_args =
            build_docker_run_args(image, container_name, volumes, args, env, gpu_devices);

        let mut cmd = tokio::process::Command::new("docker");
        cmd.args(&cmd_args);
        cmd.kill_on_drop(true);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let timeout_secs: u64 = std::env::var("TRAINER_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(7200);
        let timeout_duration = Duration::from_secs(timeout_secs);

        let run_log_path = run_log_path.to_path_buf();
        let run_fut = async move {
            let mut child = cmd
                .spawn()
                .map_err(|e| format!("docker spawn error: {e}"))?;
            let stdout = child.stdout.take().expect("stdout piped at spawn");
            let stderr = child.stderr.take().expect("stderr piped at spawn");

            // RunLogWriter abre (cria) o arquivo ANTES de qualquer linha
            // chegar: `run.log` existe desde o início do container, mesmo
            // que ele morra no import/segfault antes de emitir uma linha.
            let writer = Arc::new(tokio::sync::Mutex::new(
                RunLogWriter::open(&run_log_path).await,
            ));
            let tail = Arc::new(StdMutex::new(VecDeque::<String>::with_capacity(
                TAIL_LINES + 1,
            )));

            let out_task = tokio::spawn(pump_stream(
                stdout,
                "stdout",
                Arc::clone(&writer),
                Arc::clone(&tail),
            ));
            let err_task = tokio::spawn(pump_stream(
                stderr,
                "stderr",
                Arc::clone(&writer),
                Arc::clone(&tail),
            ));
            let _ = tokio::join!(out_task, err_task);

            let status = child
                .wait()
                .await
                .map_err(|e| format!("docker wait error: {e}"))?;
            let exit_code = status.code().unwrap_or(-1);
            let tail_lines = tail.lock().iter().cloned().collect::<Vec<_>>().join("\n");
            Ok::<(i32, String), String>((exit_code, tail_lines))
        };

        match tokio::time::timeout(timeout_duration, run_fut).await {
            Ok(Ok((exit_code, logs))) => (exit_code, logs),
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

#[cfg(test)]
mod cr_collapsing_splitter_tests {
    use super::CrCollapsingSplitter;

    #[test]
    fn collapses_tqdm_style_carriage_returns_keeping_last_segment() {
        let mut s = CrCollapsingSplitter::default();
        let lines = s.feed(b"a\rb\rc\n");
        assert_eq!(lines, vec!["c".to_string()]);
        assert!(s.finish().is_none());
    }

    #[test]
    fn plain_newlines_still_split_normally() {
        let mut s = CrCollapsingSplitter::default();
        let lines = s.feed(b"line1\nline2\n");
        assert_eq!(lines, vec!["line1".to_string(), "line2".to_string()]);
    }

    #[test]
    fn trailing_partial_line_without_newline_flushed_at_eof() {
        let mut s = CrCollapsingSplitter::default();
        let lines = s.feed(b"partial");
        assert!(lines.is_empty());
        assert_eq!(s.finish(), Some("partial".to_string()));
    }

    #[test]
    fn cr_progress_bar_across_multiple_feed_calls() {
        let mut s = CrCollapsingSplitter::default();
        assert!(s.feed(b"10%\r").is_empty());
        assert!(s.feed(b"55%\r").is_empty());
        assert_eq!(s.feed(b"100%\n"), vec!["100%".to_string()]);
    }

    #[test]
    fn overlong_line_without_newline_is_chunked_at_64kib_instead_of_growing_unbounded() {
        // Engine sem bug emite `\n` normalmente — este caso simula um print
        // gigante sem quebra de linha (ou um stream binário mal
        // interpretado) para provar que `pending` nunca cresce acima do teto.
        let mut s = CrCollapsingSplitter::default();
        let huge = vec![b'x'; super::MAX_PENDING_BYTES + 100];
        let lines = s.feed(&huge);
        assert_eq!(
            lines.len(),
            1,
            "deve emitir exatamente 1 pedaço ao atingir o teto"
        );
        assert_eq!(lines[0].len(), super::MAX_PENDING_BYTES);
        // O restante (100 bytes) continua pendente, não foi descartado.
        assert_eq!(s.finish(), Some("x".repeat(100)));
    }

    #[test]
    fn multiple_overlong_chunks_split_into_multiple_lines() {
        let mut s = CrCollapsingSplitter::default();
        let huge = vec![b'y'; super::MAX_PENDING_BYTES * 2 + 5];
        let lines = s.feed(&huge);
        assert_eq!(lines.len(), 2, "duas linhas de MAX_PENDING_BYTES cada");
        assert_eq!(lines[0].len(), super::MAX_PENDING_BYTES);
        assert_eq!(lines[1].len(), super::MAX_PENDING_BYTES);
        assert_eq!(s.finish(), Some("y".repeat(5)));
    }
}
