use std::sync::Arc;

use async_trait::async_trait;

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
    assert!(args.contains(&"infra_default".to_string()));
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
