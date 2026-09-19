//! Configuração tipada do orchestrator (Fatia 1 — P1-1 da modularização).
//!
//! Centraliza todas as leituras de variáveis de ambiente do boot (`main.rs`)
//! em structs tipadas com validação fail-fast: segredo obrigatório ausente e
//! `PORT` inválido abortam o boot com mensagem clara, sem ecoar valores.
//!
//! O comportamento é 1:1 com o parsing inline que existia no `main.rs`
//! (defaults, filtros de string vazia e fallbacks silenciosos preservados).

/// Resolve a imagem do daemon de difusão a partir do env `DIFFUSION_TRAINER_IMAGE`
/// (mesmo nome usado pelo manager e pelo compose).
///
/// Env ausente ou vazio (só whitespace) → default `"hephaestus/trainer-difusao:local"`.
pub fn resolve_daemon_diffusion_image(env_value: Option<&str>) -> String {
    match env_value.map(str::trim) {
        Some(v) if !v.is_empty() => v.to_string(),
        _ => "hephaestus/trainer-difusao:local".to_string(),
    }
}

/// Configuração completa do orchestrator, lida uma vez no boot via
/// [`OrchestratorConfig::from_env`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrchestratorConfig {
    /// `S3_ORCH_ENDPOINT_URL` — obrigatório.
    pub s3_endpoint: String,
    /// `S3_ORCH_BUCKET` — default `"heph-data"`.
    pub s3_bucket: String,
    /// `S3_ORCH_ACCESS_KEY` — obrigatório.
    pub s3_access_key: String,
    /// `S3_ORCH_SECRET_KEY` — obrigatório.
    pub s3_secret_key: String,
    /// `ORCH_WORKDIR` — default `"/data"`.
    pub workdir: String,
    /// `EXEC_MODE` (`docker`|`subprocess`) — default `"docker"`.
    pub exec_mode: String,
    /// `MANAGER_URL` — default `"http://manager:8081"`.
    pub manager_url: String,
    /// `MANAGER_TOKEN` — ausente = auth liberada (modo dev v1).
    pub manager_token: Option<String>,
    /// `PORT` — default `8082`.
    pub port: u16,
    /// `MAX_CONCURRENT_JOBS` — default `1`; valor inválido cai no default.
    pub max_concurrent_jobs: usize,
    /// `ORCH_ADVERTISE_URL` — default `http://orchestrator-local:8082`.
    pub advertise_url: String,
    /// `ORCH_PAIRING_CODE` — `None` = gerar no boot (single-use, D5.1-2).
    pub pairing_code: Option<String>,
    /// `ORCH_GPU_DEVICES` — `None` = sem GPU dedicada.
    pub gpu_devices: Option<String>,
    /// `ORCH_GPU_ALLOW_MOCK` — só `"1"` habilita (default `false`).
    pub gpu_allow_mock: bool,
    /// Sub-config do daemon de difusão (D1 — ADR-0023).
    pub daemon: DaemonConfig,
}

/// Configuração do daemon de difusão (long-lived, GPU).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfig {
    /// `DIFFUSION_DAEMON_ENABLED` — só `"1"` habilita (default `false`).
    pub enabled: bool,
    /// `DIFFUSION_DAEMON_PORT` — default `8766`; valor inválido cai no default.
    pub port: u16,
    /// `DIFFUSION_DAEMON_IDLE_TTL_S` — default `600`; inválido cai no default.
    pub idle_ttl: u64,
    /// `DIFFUSION_DAEMON_URL` — `Some` = daemon externo (spawn desabilitado).
    pub url_override: Option<String>,
    /// `DIFFUSION_TRAINER_IMAGE` com fallback para o default `:local`.
    pub image: String,
    /// `ORCH_VOL_DATASETS` — default `"infra_datasets"`.
    pub vol_datasets: String,
    /// `ORCH_VOL_OUTPUTS` — default `"infra_outputs"`.
    pub vol_outputs: String,
    /// `HF_TOKEN` (ou `HUGGING_FACE_HUB_TOKEN` como fallback).
    pub hf_token: Option<String>,
    /// `FLUX_MODEL_ID`.
    pub flux_model_id: Option<String>,
    /// `DIFFUSION_DAEMON_NETWORK`.
    pub network: Option<String>,
}

/// Valor ou default (string vazia conta como valor — igual ao `unwrap_or` do boot).
fn get_or(get: &impl Fn(&str) -> Option<String>, key: &str, default: &str) -> String {
    get(key).unwrap_or_else(|| default.to_string())
}

/// Presente e não-vazia (sem trim — igual ao `.filter(|s| !s.is_empty())` do boot).
fn get_present(get: &impl Fn(&str) -> Option<String>, key: &str) -> Option<String> {
    get(key).filter(|v| !v.is_empty())
}

/// Presente e não-vazia após trim (valor original preservado, sem trim).
fn get_trimmed(get: &impl Fn(&str) -> Option<String>, key: &str) -> Option<String> {
    get(key).filter(|v| !v.trim().is_empty())
}

impl OrchestratorConfig {
    /// Lê a configuração do ambiente do processo.
    ///
    /// Falha (fail-fast) quando faltar segredo obrigatório (`S3_ORCH_*`) ou
    /// quando `PORT` não for número — mesma severidade dos `.expect()` do boot.
    /// Nenhum valor é ecoado nas mensagens de erro.
    pub fn from_env() -> Result<Self, String> {
        Self::from_get(&|key| std::env::var(key).ok())
    }

    fn from_get(get: &impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let s3_endpoint = get("S3_ORCH_ENDPOINT_URL")
            .ok_or_else(|| "S3_ORCH_ENDPOINT_URL obrigatório".to_string())?;
        let s3_access_key = get("S3_ORCH_ACCESS_KEY")
            .ok_or_else(|| "S3_ORCH_ACCESS_KEY obrigatório".to_string())?;
        let s3_secret_key = get("S3_ORCH_SECRET_KEY")
            .ok_or_else(|| "S3_ORCH_SECRET_KEY obrigatório".to_string())?;
        let port: u16 = get_or(get, "PORT", "8082")
            .parse()
            .map_err(|_| "PORT deve ser um número".to_string())?;

        Ok(Self {
            s3_endpoint,
            s3_bucket: get_or(get, "S3_ORCH_BUCKET", "heph-data"),
            s3_access_key,
            s3_secret_key,
            workdir: get_or(get, "ORCH_WORKDIR", "/data"),
            exec_mode: get_or(get, "EXEC_MODE", "docker"),
            manager_url: get_or(get, "MANAGER_URL", "http://manager:8081"),
            manager_token: get("MANAGER_TOKEN"),
            port,
            max_concurrent_jobs: get("MAX_CONCURRENT_JOBS")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1),
            advertise_url: crate::resolve_advertise_url(get("ORCH_ADVERTISE_URL").as_deref()),
            pairing_code: get_present(get, "ORCH_PAIRING_CODE"),
            gpu_devices: get_present(get, "ORCH_GPU_DEVICES"),
            gpu_allow_mock: get("ORCH_GPU_ALLOW_MOCK").as_deref() == Some("1"),
            daemon: DaemonConfig::from_get(get),
        })
    }
}

impl DaemonConfig {
    fn from_get(get: &impl Fn(&str) -> Option<String>) -> Self {
        Self {
            enabled: get_trimmed(get, "DIFFUSION_DAEMON_ENABLED").as_deref() == Some("1"),
            port: get_trimmed(get, "DIFFUSION_DAEMON_PORT")
                .unwrap_or_else(|| "8766".to_string())
                .parse()
                .unwrap_or(8766),
            idle_ttl: get_trimmed(get, "DIFFUSION_DAEMON_IDLE_TTL_S")
                .unwrap_or_else(|| "600".to_string())
                .parse()
                .unwrap_or(600),
            url_override: get_trimmed(get, "DIFFUSION_DAEMON_URL"),
            image: resolve_daemon_diffusion_image(get("DIFFUSION_TRAINER_IMAGE").as_deref()),
            vol_datasets: get_or(get, "ORCH_VOL_DATASETS", "infra_datasets"),
            vol_outputs: get_or(get, "ORCH_VOL_OUTPUTS", "infra_outputs"),
            hf_token: get("HF_TOKEN")
                .or_else(|| get("HUGGING_FACE_HUB_TOKEN"))
                .filter(|v| !v.is_empty()),
            flux_model_id: get_present(get, "FLUX_MODEL_ID"),
            network: get_trimmed(get, "DIFFUSION_DAEMON_NETWORK"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn getter(map: &HashMap<String, String>) -> impl Fn(&str) -> Option<String> + '_ {
        |key| map.get(key).cloned()
    }

    fn env_map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    fn minimal() -> HashMap<String, String> {
        env_map(&[
            ("S3_ORCH_ENDPOINT_URL", "http://minio:9000"),
            ("S3_ORCH_ACCESS_KEY", "ak"),
            ("S3_ORCH_SECRET_KEY", "sk"),
        ])
    }

    fn load(map: &HashMap<String, String>) -> OrchestratorConfig {
        OrchestratorConfig::from_get(&getter(map)).expect("config mínima válida")
    }

    #[test]
    fn fail_fast_sem_s3_obrigatorio() {
        let err = OrchestratorConfig::from_get(&getter(&HashMap::new())).unwrap_err();
        assert!(err.contains("S3_ORCH_ENDPOINT_URL"), "err={err}");

        let map = env_map(&[("S3_ORCH_ENDPOINT_URL", "http://minio:9000")]);
        let err = OrchestratorConfig::from_get(&getter(&map)).unwrap_err();
        assert!(err.contains("S3_ORCH_ACCESS_KEY"), "err={err}");

        let map = env_map(&[
            ("S3_ORCH_ENDPOINT_URL", "http://minio:9000"),
            ("S3_ORCH_ACCESS_KEY", "ak"),
        ]);
        let err = OrchestratorConfig::from_get(&getter(&map)).unwrap_err();
        assert!(err.contains("S3_ORCH_SECRET_KEY"), "err={err}");
    }

    #[test]
    fn erro_nao_ecoa_valor() {
        // Mensagem de obrigatório não contém o segredo (fail-fast sem eco).
        let map = env_map(&[
            ("S3_ORCH_ENDPOINT_URL", "http://minio:9000"),
            ("S3_ORCH_ACCESS_KEY", "ak"),
        ]);
        let err = OrchestratorConfig::from_get(&getter(&map)).unwrap_err();
        assert!(!err.contains("ak"), "err={err}");
    }

    #[test]
    fn defaults_batidos() {
        let cfg = load(&minimal());
        assert_eq!(cfg.s3_endpoint, "http://minio:9000");
        assert_eq!(cfg.s3_bucket, "heph-data");
        assert_eq!(cfg.workdir, "/data");
        assert_eq!(cfg.exec_mode, "docker");
        assert_eq!(cfg.manager_url, "http://manager:8081");
        assert_eq!(cfg.manager_token, None);
        assert_eq!(cfg.port, 8082);
        assert_eq!(cfg.max_concurrent_jobs, 1);
        assert_eq!(cfg.advertise_url, "http://orchestrator-local:8082");
        assert_eq!(cfg.pairing_code, None);
        assert_eq!(cfg.gpu_devices, None);
        assert!(!cfg.gpu_allow_mock);
        assert!(!cfg.daemon.enabled);
        assert_eq!(cfg.daemon.port, 8766);
        assert_eq!(cfg.daemon.idle_ttl, 600);
        assert_eq!(cfg.daemon.url_override, None);
        assert_eq!(cfg.daemon.image, "hephaestus/trainer-difusao:local");
        assert_eq!(cfg.daemon.vol_datasets, "infra_datasets");
        assert_eq!(cfg.daemon.vol_outputs, "infra_outputs");
        assert_eq!(cfg.daemon.hf_token, None);
        assert_eq!(cfg.daemon.flux_model_id, None);
        assert_eq!(cfg.daemon.network, None);
    }

    #[test]
    fn overrides_respeitados() {
        let map = env_map(&[
            ("S3_ORCH_ENDPOINT_URL", "http://s3:9000"),
            ("S3_ORCH_BUCKET", "bkt"),
            ("S3_ORCH_ACCESS_KEY", "ak"),
            ("S3_ORCH_SECRET_KEY", "sk"),
            ("ORCH_WORKDIR", "/w"),
            ("EXEC_MODE", "subprocess"),
            ("MANAGER_URL", "http://m:1"),
            ("MANAGER_TOKEN", "tok"),
            ("PORT", "9999"),
            ("MAX_CONCURRENT_JOBS", "4"),
            ("ORCH_ADVERTISE_URL", "http://n:9999"),
            ("ORCH_PAIRING_CODE", "code"),
            ("ORCH_GPU_DEVICES", "0,1"),
            ("ORCH_GPU_ALLOW_MOCK", "1"),
            ("DIFFUSION_DAEMON_ENABLED", "1"),
            ("DIFFUSION_DAEMON_PORT", "9001"),
            ("DIFFUSION_DAEMON_IDLE_TTL_S", "60"),
            ("DIFFUSION_DAEMON_URL", "http://ext:9001"),
            ("DIFFUSION_TRAINER_IMAGE", "reg/img:gpu"),
            ("ORCH_VOL_DATASETS", "ds"),
            ("ORCH_VOL_OUTPUTS", "out"),
            ("HF_TOKEN", "hf"),
            ("FLUX_MODEL_ID", "flux"),
            ("DIFFUSION_DAEMON_NETWORK", "net"),
        ]);
        let cfg = load(&map);
        assert_eq!(cfg.s3_bucket, "bkt");
        assert_eq!(cfg.workdir, "/w");
        assert_eq!(cfg.exec_mode, "subprocess");
        assert_eq!(cfg.manager_url, "http://m:1");
        assert_eq!(cfg.manager_token.as_deref(), Some("tok"));
        assert_eq!(cfg.port, 9999);
        assert_eq!(cfg.max_concurrent_jobs, 4);
        assert_eq!(cfg.advertise_url, "http://n:9999");
        assert_eq!(cfg.pairing_code.as_deref(), Some("code"));
        assert_eq!(cfg.gpu_devices.as_deref(), Some("0,1"));
        assert!(cfg.gpu_allow_mock);
        assert!(cfg.daemon.enabled);
        assert_eq!(cfg.daemon.port, 9001);
        assert_eq!(cfg.daemon.idle_ttl, 60);
        assert_eq!(cfg.daemon.url_override.as_deref(), Some("http://ext:9001"));
        assert_eq!(cfg.daemon.image, "reg/img:gpu");
        assert_eq!(cfg.daemon.vol_datasets, "ds");
        assert_eq!(cfg.daemon.vol_outputs, "out");
        assert_eq!(cfg.daemon.hf_token.as_deref(), Some("hf"));
        assert_eq!(cfg.daemon.flux_model_id.as_deref(), Some("flux"));
        assert_eq!(cfg.daemon.network.as_deref(), Some("net"));
    }

    #[test]
    fn port_invalido_falha() {
        let mut map = minimal();
        map.insert("PORT".to_string(), "abc".to_string());
        let err = OrchestratorConfig::from_get(&getter(&map)).unwrap_err();
        assert!(err.contains("PORT"), "err={err}");
    }

    #[test]
    fn max_concurrent_invalido_cai_no_default() {
        let mut map = minimal();
        map.insert("MAX_CONCURRENT_JOBS".to_string(), "x".to_string());
        assert_eq!(load(&map).max_concurrent_jobs, 1);
    }

    #[test]
    fn daemon_port_e_ttl_invalidos_caem_no_default() {
        // Fallback silencioso herdado do boot (parse().unwrap_or(default)).
        let mut map = minimal();
        map.insert("DIFFUSION_DAEMON_PORT".to_string(), "abc".to_string());
        map.insert("DIFFUSION_DAEMON_IDLE_TTL_S".to_string(), "".to_string());
        let cfg = load(&map);
        assert_eq!(cfg.daemon.port, 8766);
        assert_eq!(cfg.daemon.idle_ttl, 600);
    }

    #[test]
    fn advertise_vazio_cai_no_default() {
        let mut map = minimal();
        map.insert("ORCH_ADVERTISE_URL".to_string(), String::new());
        assert_eq!(load(&map).advertise_url, "http://orchestrator-local:8082");
    }

    #[test]
    fn hf_token_tem_fallback_para_hub_token() {
        let mut map = minimal();
        map.insert("HUGGING_FACE_HUB_TOKEN".to_string(), "hub".to_string());
        assert_eq!(load(&map).daemon.hf_token.as_deref(), Some("hub"));

        // HF_TOKEN tem precedência e vazio conta como ausente.
        map.insert("HF_TOKEN".to_string(), "hf".to_string());
        assert_eq!(load(&map).daemon.hf_token.as_deref(), Some("hf"));
        map.insert("HF_TOKEN".to_string(), String::new());
        assert_eq!(load(&map).daemon.hf_token, None);
    }

    #[test]
    fn gpu_allow_mock_so_1() {
        let mut map = minimal();
        assert!(!load(&map).gpu_allow_mock);
        for v in ["0", "true", "", " 1 "] {
            map.insert("ORCH_GPU_ALLOW_MOCK".to_string(), v.to_string());
            assert!(!load(&map).gpu_allow_mock, "v={v:?}");
        }
        map.insert("ORCH_GPU_ALLOW_MOCK".to_string(), "1".to_string());
        assert!(load(&map).gpu_allow_mock);
    }

    #[test]
    fn daemon_enabled_so_1() {
        let mut map = minimal();
        assert!(!load(&map).daemon.enabled);
        map.insert("DIFFUSION_DAEMON_ENABLED".to_string(), "1".to_string());
        assert!(load(&map).daemon.enabled);
    }

    #[test]
    fn pairing_vazio_conta_como_ausente() {
        let mut map = minimal();
        map.insert("ORCH_PAIRING_CODE".to_string(), String::new());
        assert_eq!(load(&map).pairing_code, None);
    }

    #[test]
    fn daemon_image_env_explicito_vence() {
        assert_eq!(
            resolve_daemon_diffusion_image(Some("meu-registry/trainer-difusao:gpu")),
            "meu-registry/trainer-difusao:gpu"
        );
    }

    #[test]
    fn daemon_image_default_quando_ausente_ou_vazio() {
        for v in [None, Some(""), Some("   ")] {
            assert_eq!(
                resolve_daemon_diffusion_image(v),
                "hephaestus/trainer-difusao:local",
                "env={v:?}"
            );
        }
    }

    #[test]
    fn daemon_image_preserva_trim_externo() {
        assert_eq!(
            resolve_daemon_diffusion_image(Some("  hephaestus/trainer-difusao:gpu  ")),
            "hephaestus/trainer-difusao:gpu"
        );
    }
}
