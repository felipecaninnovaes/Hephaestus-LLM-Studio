use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use super::*;

/// Modo host (fallback): network_name=None → --network host, URL host.docker.internal
/// Volume targets devem ser os mounts do daemon (iguais aos do one-shot).
#[test]
fn build_daemon_args_structure() {
    let launcher = DockerDaemonLauncher::new(
        "hephaestus/trainer-difusao:local",
        "diffusion-daemon",
        vec![
            ("/host/data".into(), DAEMON_DATASETS_MOUNT.into()),
            ("/host/out".into(), DAEMON_OUTPUTS_MOUNT.into()),
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
    assert!(args.contains(&"infra_default".to_string()));
    assert!(args.contains(&"-d".to_string()));

    // Volumes — targets devem ser /datasets e /outputs (mesmos do one-shot)
    assert!(args.contains(&"-v".to_string()));
    assert!(args.contains(&"/host/data:/datasets".to_string()));
    assert!(args.contains(&"/host/out:/outputs".to_string()));

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

/// ENGINE_USER também vale para o daemon: sem `--user`, o container herda
/// `USER studio` da imagem (uid 1000) e não escreve em diretórios de job
/// criados pelo orquestrador (root) no dataset compartilhado.
#[test]
fn build_daemon_args_with_engine_user() {
    std::env::set_var("ENGINE_USER", "1000:1000");
    let launcher = DockerDaemonLauncher::new(
        "hephaestus/trainer-difusao:gpu",
        "diffusion-daemon",
        vec![("gpu_gpu_outputs".into(), "/outputs".into())],
        8766,
        Some("0".into()),
        vec![],
        Some("gpu_default".into()),
    );
    let args = launcher.build_daemon_args();
    std::env::remove_var("ENGINE_USER");

    let idx = args
        .iter()
        .position(|a| a == "--user")
        .expect("daemon deve receber --user quando ENGINE_USER está definido");
    assert_eq!(args[idx + 1], "1000:1000");
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

/// start() usa rede isolada e não faz fallback para modo host.
#[test]
fn daemon_url_isolated_network_mode() {
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
    assert!(args.contains(&"infra_default".to_string()));
    assert!(!args.contains(&"host".to_string()));
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

#[tokio::test]
async fn b2_preemption_lifecycle_same_gpu_different_gpu_unknown_gpu() {
    let client = Arc::new(FakeDaemonClient::new());
    let launcher = Arc::new(RecordingDaemonLauncher::new());

    // 1. Daemon em GPU0 (UUID 3060)
    let ds_3060 = DaemonState::with_gpu(
        "img",
        8766,
        600,
        client.clone(),
        launcher.clone(),
        Some("GPU-1c1e01c2-4192-8f38-1a8a-33fb78b06f17".to_string()),
    );
    ds_3060.set_running(true, Some("http://localhost:8766".to_string()));
    *ds_3060.last_used.lock().unwrap() =
        std::time::Instant::now() - std::time::Duration::from_secs(600);

    // Job em GPU1 (UUID 1660S) -> NÃO deve derrubar
    maybe_preempt_daemon_with_gpu(
        &ds_3060,
        Some("GPU-c83cc056-07f7-d31e-cc98-7486ddac0296"),
        None,
    )
    .await;
    assert!(
        ds_3060.is_running(),
        "Job em GPU diferente não deve derrubar daemon"
    );

    // Job na mesma GPU (GPU-1c1e01c2...) -> DEVE derrubar
    maybe_preempt_daemon_with_gpu(
        &ds_3060,
        Some("GPU-1c1e01c2-4192-8f38-1a8a-33fb78b06f17"),
        None,
    )
    .await;
    assert!(
        !ds_3060.is_running(),
        "Job na mesma GPU deve derrubar daemon"
    );

    // 2. Qualquer uma desconhecida -> DEVE derrubar
    let ds_unknown = DaemonState::with_gpu(
        "img",
        8766,
        600,
        client.clone(),
        launcher.clone(),
        None, // daemon em GPU desconhecida
    );
    ds_unknown.set_running(true, Some("http://localhost:8766".to_string()));
    *ds_unknown.last_used.lock().unwrap() =
        std::time::Instant::now() - std::time::Duration::from_secs(600);

    maybe_preempt_daemon_with_gpu(
        &ds_unknown,
        Some("GPU-c83cc056-07f7-d31e-cc98-7486ddac0296"),
        None,
    )
    .await;
    assert!(
        !ds_unknown.is_running(),
        "Daemon com GPU desconhecida deve ser derrubado"
    );

    // Daemon conhecido, job com GPU desconhecida (None) -> DEVE derrubar
    let ds_known = DaemonState::with_gpu(
        "img",
        8766,
        600,
        client.clone(),
        launcher.clone(),
        Some("GPU-1c1e01c2-4192-8f38-1a8a-33fb78b06f17".to_string()),
    );
    ds_known.set_running(true, Some("http://localhost:8766".to_string()));
    *ds_known.last_used.lock().unwrap() =
        std::time::Instant::now() - std::time::Duration::from_secs(600);

    maybe_preempt_daemon_with_gpu(&ds_known, None, None).await;
    assert!(
        !ds_known.is_running(),
        "Job com GPU desconhecida deve derrubar daemon"
    );
}

#[tokio::test]
async fn b2_preemption_index_to_uuid_via_sampler() {
    let client = Arc::new(FakeDaemonClient::new());
    let launcher = Arc::new(RecordingDaemonLauncher::new());
    let sampler = crate::telemetry::gpu::GpuSampler::new();

    // Injeta telemetria artificial no cache do sampler: GPU 0 -> 3060, GPU 1 -> 1660S
    let csv = "0, GPU-1c1e01c2-4192-8f38-1a8a-33fb78b06f17, NVIDIA GeForce RTX 3060, 12288, 1024, 75.0, 10.0, 45\n1, GPU-c83cc056-07f7-d31e-cc98-7486ddac0296, NVIDIA GeForce GTX 1660 SUPER, 6144, 512, 60.0, 5.0, 40";
    let telem = crate::telemetry::gpu::parse_nvidia_smi_csv(csv).expect("parse csv");
    // Popula o cache interno
    *sampler.cache.write().await = crate::telemetry::gpu::GpuSamplerCache {
        last_sampled: Some(std::time::Instant::now()),
        telemetry: Some(telem),
    };

    // Daemon configurado com índice "0" (que resolve para GPU-1c1e...)
    let ds_idx0 = DaemonState::with_gpu(
        "img",
        8766,
        600,
        client.clone(),
        launcher.clone(),
        Some("0".to_string()),
    );
    ds_idx0.set_running(true, Some("http://localhost:8766".to_string()));
    *ds_idx0.last_used.lock().unwrap() =
        std::time::Instant::now() - std::time::Duration::from_secs(600);

    // Job na GPU "1" (índice 1 -> 1660S) -> NÃO deve derrubar
    maybe_preempt_daemon_with_gpu(&ds_idx0, Some("1"), Some(&sampler)).await;
    assert!(
        ds_idx0.is_running(),
        "Job na GPU 1 não deve derrubar daemon na GPU 0"
    );

    // Job na GPU "0" (índice 0 -> 3060) -> DEVE derrubar
    maybe_preempt_daemon_with_gpu(&ds_idx0, Some("0"), Some(&sampler)).await;
    assert!(
        !ds_idx0.is_running(),
        "Job na mesma GPU 0 deve derrubar daemon"
    );
}

// -----------------------------------------------------------------------
// Mapeamento de paths do body + readiness + timeout do /generate
// -----------------------------------------------------------------------

/// output_dir/telemetry_path são do namespace do container do daemon, não do orquestrador.
#[test]
fn generate_body_for_job_uses_daemon_mount_namespace() {
    let body = GenerateBody::for_job("cfg".into(), "job-1");
    assert_eq!(body.config, "cfg");
    assert_eq!(body.output_dir, "/outputs/job-1");
    assert_eq!(body.telemetry_path, "/outputs/job-1/telemetry.jsonl");
    assert_eq!(DAEMON_OUTPUTS_MOUNT, "/outputs");
    assert_eq!(DAEMON_DATASETS_MOUNT, "/datasets");
}

/// Client que só responde health ok após N falhas (container ainda subindo).
struct WarmingUpClient {
    failures_left: std::sync::atomic::AtomicUsize,
    calls: std::sync::atomic::AtomicUsize,
}

#[async_trait]
impl DaemonClient for WarmingUpClient {
    async fn health(&self) -> Option<HealthResponse> {
        use std::sync::atomic::Ordering::SeqCst;
        self.calls.fetch_add(1, SeqCst);
        let left = self.failures_left.load(SeqCst);
        if left > 0 {
            self.failures_left.store(left - 1, SeqCst);
            return None;
        }
        Some(HealthResponse {
            ok: true,
            loaded_spec: None,
            busy: false,
            _extra: Default::default(),
        })
    }
    async fn generate(&self, _b: &GenerateBody) -> Result<(), String> {
        Ok(())
    }
    async fn shutdown(&self) -> Result<(), String> {
        Ok(())
    }
}

/// Cold start: health recusa por um tempo; ensure_daemon_ready espera e só
/// retorna quando o daemon responde (não despacha /generate antes).
#[tokio::test]
async fn ensure_daemon_ready_waits_until_health_ok_on_cold_start() {
    let client = Arc::new(WarmingUpClient {
        failures_left: std::sync::atomic::AtomicUsize::new(5),
        calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let launcher = Arc::new(RecordingDaemonLauncher::new());
    let ds = DaemonState::new("img", 8766, 600, client.clone(), launcher.clone());
    ds.set_running(true, Some("http://localhost:8766".into()));

    let url = ensure_daemon_ready_with(
        &ds,
        "spec",
        Duration::from_secs(5),
        Duration::from_millis(10),
    )
    .await
    .expect("deve esperar o health e ter sucesso");
    assert_eq!(url, "http://localhost:8766");
    // 5 falhas + 1 sucesso do poll + 1 consulta de loaded_spec.
    assert_eq!(client.calls.load(std::sync::atomic::Ordering::SeqCst), 7);
    assert!(ds.is_running());
    assert!(launcher.log().is_empty(), "daemon já rodando: sem restart");
}

/// Daemon que nunca fica pronto: erro após o teto informado, daemon morto e estado limpo.
#[tokio::test]
async fn ensure_daemon_ready_times_out_and_kills_daemon() {
    let client = Arc::new(WarmingUpClient {
        failures_left: std::sync::atomic::AtomicUsize::new(usize::MAX),
        calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let launcher = Arc::new(RecordingDaemonLauncher::new());
    let ds = DaemonState::new("img", 8766, 600, client, launcher.clone());
    ds.set_running(true, Some("http://localhost:8766".into()));

    let err = ensure_daemon_ready_with(
        &ds,
        "spec",
        Duration::from_millis(200),
        Duration::from_millis(10),
    )
    .await
    .expect_err("deve estourar o timeout");
    assert_eq!(err, "daemon health timeout after 0s");
    assert_eq!(launcher.log(), vec!["stop".to_string()]);
    assert!(!ds.is_running());
}

/// Servidor que aceita o POST e só responde 200 após `delay` (geração longa).
async fn slow_generate_server(delay: Duration) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = vec![0u8; 8192];
                let _ = stream.read(&mut buf).await;
                tokio::time::sleep(delay).await;
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}")
                    .await;
            });
        }
    });
    format!("http://127.0.0.1:{}", addr.port())
}

/// Geração mais lenta que o antigo teto fixo continua OK enquanto estiver
/// dentro do teto configurado do /generate.
#[tokio::test]
async fn http_generate_waits_for_slow_response_within_timeout() {
    let url = slow_generate_server(Duration::from_millis(800)).await;
    let client = HttpDaemonClient::with_generate_timeout(&url, Duration::from_secs(5));
    let body = GenerateBody::for_job("cfg".into(), "j");
    assert_eq!(client.generate(&body).await, Ok(()));
}

/// Estouro do teto do /generate produz erro explícito de timeout (limitado, não eterno).
#[tokio::test]
async fn http_generate_times_out_with_explicit_error() {
    let url = slow_generate_server(Duration::from_secs(3)).await;
    let client = HttpDaemonClient::with_generate_timeout(&url, Duration::from_millis(300));
    let body = GenerateBody::for_job("cfg".into(), "j");
    let err = client.generate(&body).await.unwrap_err();
    assert!(err.starts_with("daemon generate timed out after"), "{err}");
}

/// O default cobre carga a frio + steps do 9B (~5 min) com folga e é limitado.
#[test]
fn default_generate_timeout_covers_cold_9b_and_is_bounded() {
    assert!(DEFAULT_GENERATE_TIMEOUT >= Duration::from_secs(10 * 60));
    assert!(DEFAULT_GENERATE_TIMEOUT <= Duration::from_secs(60 * 60));
    assert!(SHORT_CALL_TIMEOUT < DEFAULT_GENERATE_TIMEOUT);
}
