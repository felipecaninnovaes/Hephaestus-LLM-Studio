//! Orchestrator service — axum routes + boot (F4.4, ADR-0007).
//!
//! `main.rs` é a camada fina: tracing, env loading, rotas axum,
//! dispatch handler (aceita job e spawna task), abort handler,
//! heartbeat loop, e servidor HTTP.

use orchestrator::{
    self,
    config::OrchestratorConfig,
    server::{build_router, AppState},
    HeartbeatBody, HttpHeartbeatClient, HttpReportClient, S3Client,
};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------

/// Re-export de [`orchestrator::config::resolve_daemon_diffusion_image`] para
/// compatibilidade com os testes existentes deste módulo (Fatia 1).
#[cfg(test)]
pub(crate) use orchestrator::config::resolve_daemon_diffusion_image;
#[tokio::main]
async fn main() {
    // Tracing (JSON + env filter, padrão manager).
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "orchestrator=info,tower_http=info".into()),
        )
        .json()
        .init();

    // Config tipada — fail-fast sem ecoar valor (config::OrchestratorConfig).
    let cfg = OrchestratorConfig::from_env().unwrap_or_else(|e| panic!("{e}"));

    // D5.1-2 — pairing code: env define, ou gera no boot e loga uma vez.
    let pairing = Arc::new(match cfg.pairing_code.clone() {
        Some(code) => {
            tracing::info!("ORCH_PAIRING_CODE definido via env");
            orchestrator::PairingState::new(code)
        }
        None => {
            let code = orchestrator::generate_pairing_code();
            tracing::info!(
                pairing_code = %code,
                "pairing code gerado — copie para o manager (usado uma única vez)"
            );
            orchestrator::PairingState::new(code)
        }
    });

    tracing::info!(
        "orchestrator boot: exec_mode={}, workdir={}",
        cfg.exec_mode,
        cfg.workdir
    );

    // S3 client (D2 — cliente escopado).
    let s3 = Arc::new(S3Client::new(
        &cfg.s3_endpoint,
        &cfg.s3_access_key,
        &cfg.s3_secret_key,
        &cfg.s3_bucket,
    )) as Arc<dyn orchestrator::S3Port>;

    // Report client (D4 — POST /internal/jobs/:id/report).
    let report_client = Arc::new(HttpReportClient::new(
        &cfg.manager_url,
        cfg.manager_token.as_deref(),
    )) as Arc<dyn orchestrator::ReportClient>;

    // Heartbeat client (D4/D9 — POST /internal/heartbeat).
    let heartbeat_client = Arc::new(HttpHeartbeatClient::new(
        &cfg.manager_url,
        cfg.manager_token.as_deref(),
    )) as Arc<dyn orchestrator::HeartbeatClient>;

    // Executor (D5 — docker CLI via socket do host).
    let executor: Arc<dyn orchestrator::TrainerExecutor> = match cfg.exec_mode.as_str() {
        "subprocess" => Arc::new(orchestrator::SubprocessExecutor),
        _ => Arc::new(orchestrator::DockerExecutor),
    };

    // State.
    let active_jobs = orchestrator::new_active_jobs();

    // GPU config: lida uma vez no boot via cfg (D4/D7).
    if let Some(devices) = &cfg.gpu_devices {
        tracing::info!("ORCH_GPU_DEVICES={devices} — modo GPU habilitado");
    }
    if cfg.gpu_allow_mock {
        tracing::info!("ORCH_GPU_ALLOW_MOCK=1 — guarda anti-mock desabilitada");
    }

    // Daemon (D1 — ADR-0023).
    // Trata string vazia como ausente: compose emite `DIFFUSION_DAEMON_URL=`
    // (presença + valor vazio) e o unwrap_or vê Ok("") → daemon externo errado.
    // (Filtro aplicado em `config::DaemonConfig`.)
    let daemon_state = if cfg.daemon.enabled {
        // Mesmo env do manager/compose (`DIFFUSION_TRAINER_IMAGE`): o nome
        // antigo `TRAINER_IMAGE_DIFFUSION` caía sempre no default :local e a
        // guarda D2 recusava job real no nó GPU.
        let image = cfg.daemon.image.clone();
        let client: Arc<dyn orchestrator::daemon::DaemonClient> =
            if let Some(url) = &cfg.daemon.url_override {
                Arc::new(orchestrator::daemon::HttpDaemonClient::new(url))
            } else {
                Arc::new(orchestrator::daemon::HttpDaemonClient::new(&format!(
                    "http://localhost:{}",
                    cfg.daemon.port
                )))
            };

        // Mesmos volumes/mounts do one-shot (build_docker_run_args).
        let daemon_volumes: Vec<(String, String)> = vec![
            (
                cfg.daemon.vol_datasets.clone(),
                "/data/datasets".to_string(),
            ),
            (cfg.daemon.vol_outputs.clone(), "/data/outputs".to_string()),
        ];

        // Mesmas envs do one-shot: ENGINE_MOCK=0 quando GPU, HF cache paths, etc.
        let mut daemon_env: Vec<(String, String)> = Vec::new();
        if cfg.gpu_devices.is_some() {
            daemon_env.push(("ENGINE_MOCK".to_string(), "0".to_string()));
        }
        // HF cache paths para diffusion (igual one-shot L1473-1493)
        daemon_env.push((
            "HF_HOME".to_string(),
            "/data/outputs/.cache/huggingface".to_string(),
        ));
        daemon_env.push((
            "HF_HUB_CACHE".to_string(),
            "/data/outputs/.cache/huggingface/hub".to_string(),
        ));
        daemon_env.push((
            "TRANSFORMERS_CACHE".to_string(),
            "/data/outputs/.cache/huggingface/hub".to_string(),
        ));
        daemon_env.push((
            "DIFFUSERS_CACHE".to_string(),
            "/data/outputs/.cache/huggingface/hub".to_string(),
        ));
        daemon_env.push((
            "TORCH_HOME".to_string(),
            "/data/outputs/.cache/torch".to_string(),
        ));
        daemon_env.push(("HF_HUB_DISABLE_XET".to_string(), "1".to_string()));
        daemon_env.push(("HF_HUB_ENABLE_HF_TRANSFER".to_string(), "0".to_string()));
        if let Some(token) = cfg.daemon.hf_token.clone() {
            daemon_env.push(("HF_TOKEN".to_string(), token.clone()));
            daemon_env.push(("HUGGING_FACE_HUB_TOKEN".to_string(), token));
        }
        if let Some(model_id) = cfg.daemon.flux_model_id.clone() {
            daemon_env.push(("FLUX_MODEL_ID".to_string(), model_id));
        }
        if let Ok(v) = std::env::var("ENABLE_TEXT_ENCODER_UNLOAD") {
            if !v.trim().is_empty() {
                daemon_env.push((
                    "ENABLE_TEXT_ENCODER_UNLOAD".to_string(),
                    v.trim().to_string(),
                ));
            }
        }

        let launcher: Arc<dyn orchestrator::daemon::DaemonLauncher> =
            Arc::new(orchestrator::daemon::DockerDaemonLauncher::new(
                &image,
                "diffusion-daemon",
                daemon_volumes,
                cfg.daemon.port,
                cfg.gpu_devices.clone(),
                daemon_env,
                cfg.daemon.network.clone(),
            ));
        let ds = Arc::new(orchestrator::daemon::DaemonState::new(
            &image,
            cfg.daemon.port,
            cfg.daemon.idle_ttl,
            client,
            launcher,
        ));
        tracing::info!(
            "diffusion daemon habilitado: port={}, idle_ttl={}s",
            cfg.daemon.port,
            cfg.daemon.idle_ttl
        );
        if let Some(url) = &cfg.daemon.url_override {
            tracing::info!("DIFFUSION_DAEMON_URL={url} — daemon externo, spawn desabilitado");
        }

        // Spawn idle TTL housekeeping task (D1)
        let ds_clone = Arc::clone(&ds);
        tokio::spawn(async move {
            orchestrator::daemon::idle_ttl_housekeeping(ds_clone).await;
        });

        Some(ds)
    } else {
        tracing::info!("diffusion daemon desabilitado (DIFFUSION_DAEMON_ENABLED=0) — one-shot");
        None
    };

    let state = AppState {
        s3: Arc::clone(&s3),
        report_client: Arc::clone(&report_client),
        executor,
        active_jobs,
        manager_token: cfg.manager_token.clone(),
        gpu_devices: cfg.gpu_devices.clone(),
        gpu_allow_mock: cfg.gpu_allow_mock,
        pairing,
        daemon_state,
        max_concurrent_jobs: cfg.max_concurrent_jobs,
    };

    // Heartbeat loop (~2s, D4/D9).
    let heartbeat_active_jobs = Arc::clone(&state.active_jobs);
    let heartbeat_advertise_url = cfg.advertise_url.clone();

    // GPU telemetry: tenta nvidia-smi no boot; se falhar, warn único e fallback.
    let gpu_telemetry_boot = orchestrator::try_nvidia_smi().await;
    if gpu_telemetry_boot.is_some() {
        tracing::info!("nvidia-smi disponível — telemetria GPU habilitada");
    } else {
        tracing::warn!(
            "nvidia-smi indisponível — telemetria GPU desabilitada (gpus:[], vram:None)"
        );
    }

    // Sweep de containers órfãos no boot (anti-processos fantasmas pós crash).
    orchestrator::sweep_orphan_trainer_containers().await;
    orchestrator::sweep_orphan_workdirs(
        &std::path::PathBuf::from(&cfg.workdir),
        std::time::Duration::from_secs(86400),
    )
    .await;

    tokio::spawn(async move {
        let heartbeat_secs: u64 = std::env::var("HEARTBEAT_INTERVAL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2);
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(heartbeat_secs));
        loop {
            interval.tick().await;

            // Tenta nvidia-smi a cada tick; fallback silencioso.
            let (gpus, vram_total, vram_used, max_gpu_mib) =
                match orchestrator::try_nvidia_smi().await {
                    Some(telemetry) => (
                        telemetry.gpus,
                        Some(telemetry.vram_total),
                        Some(telemetry.vram_used),
                        Some(telemetry.max_gpu_mib),
                    ),
                    None => (vec![], None, None, None),
                };

            let body = HeartbeatBody {
                endpoint: heartbeat_advertise_url.clone(),
                gpus,
                vram_total,
                vram_used,
                cpu: Some(orchestrator::read_cpu()),
                ram: Some(orchestrator::read_ram()),
                ram_total: orchestrator::read_ram_total(),
                jobs_active: heartbeat_active_jobs.len() as i32,
                max_gpu_mib,
            };

            if let Err(e) = heartbeat_client.send(&body).await {
                tracing::warn!("heartbeat failed: {e}");
            }
        }
    });

    // Server.
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", cfg.port))
        .await
        .expect("bind");
    tracing::info!("orchestrator ouvindo em 0.0.0.0:{}", cfg.port);
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::resolve_daemon_diffusion_image;

    #[test]
    fn daemon_image_env_explicito_vence() {
        assert_eq!(
            resolve_daemon_diffusion_image(Some("meu-registry/trainer-difusao:gpu")),
            "meu-registry/trainer-difusao:gpu"
        );
    }

    #[test]
    fn daemon_image_default_quando_ausente_ou_vazio() {
        // Env ausente, vazio ou só whitespace → default :local (guarda D2 só
        // recusa :local em nó GPU; CPU-only continua mock).
        for v in [None, Some(""), Some("   ")] {
            assert_eq!(
                resolve_daemon_diffusion_image(v),
                "hephaestus/trainer-difusao:local",
                "env={v:?}"
            );
        }
    }

    #[test]
    fn daemon_image_preserva_whitespace_externo() {
        // Trim: compose nunca deve injetar espaços no nome da imagem.
        assert_eq!(
            resolve_daemon_diffusion_image(Some("  hephaestus/trainer-difusao:gpu  ")),
            "hephaestus/trainer-difusao:gpu"
        );
    }
}
