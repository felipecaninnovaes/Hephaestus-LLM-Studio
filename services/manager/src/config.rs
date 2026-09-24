//! Configurações estruturadas para o manager (MM-05).

/// Parser puro da flag `AUTO_ADOPT_LOCAL`: `None` ou != "0" → true (habilita);
/// `"0"` → false (desabilita). Usado para evitar conflito com auto-registro
/// de orchestrator-local no boot (sessão GPU — ADR-0010 D2).
pub fn auto_adopt_enabled(raw: Option<&str>) -> bool {
    raw.map(|v| v != "0").unwrap_or(true)
}

#[derive(Debug, Clone)]
pub struct WatchdogConfig {
    pub stale_timeout_secs: u64,
    pub degraded_secs: u64,
    pub offline_secs: u64,
    pub prepare_timeout_minutes: i64,
    pub dispatch_interval_secs: u64,
    pub dataset_gc_older_than_days: i64,
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            stale_timeout_secs: 10,
            degraded_secs: 15,
            offline_secs: 60,
            prepare_timeout_minutes: 60,
            dispatch_interval_secs: 2,
            dataset_gc_older_than_days: 7,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ManagerConfig {
    pub database_url: String,
    pub environment: String,
    pub is_prod: bool,
    pub manager_token: String,
    pub exec_mode: String,
    pub orch_workdir: String,
    pub trainer_image: String,
    pub diffusion_trainer_image: String,
    pub port: u16,
    pub auto_adopt_local: bool,
    pub vram_table_path: Option<String>,
    pub watchdog: WatchdogConfig,
}

impl ManagerConfig {
    pub fn from_env() -> Result<Self, String> {
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| "DATABASE_URL obrigatório".to_string())?;
        let environment = std::env::var("ENVIRONMENT").unwrap_or_else(|_| "development".into());
        let is_prod = environment.eq_ignore_ascii_case("production");

        let manager_token = resolve_manager_token(std::env::var("MANAGER_TOKEN"), is_prod)?;
        let exec_mode = std::env::var("EXEC_MODE").unwrap_or_else(|_| "docker".into());
        let orch_workdir = std::env::var("ORCH_WORKDIR").unwrap_or_else(|_| "/data".into());
        let trainer_image = std::env::var("TRAINER_IMAGE")
            .unwrap_or_else(|_| "hephaestus/trainer-yolo:local".into());
        let diffusion_trainer_image = std::env::var("DIFFUSION_TRAINER_IMAGE")
            .unwrap_or_else(|_| "hephaestus/trainer-diffusion:local".into());
        let port: u16 = std::env::var("PORT")
            .unwrap_or_else(|_| "8081".into())
            .parse()
            .map_err(|e| format!("PORT deve ser um número: {e}"))?;
        let auto_adopt_local = auto_adopt_enabled(
            std::env::var("AUTO_ADOPT_LOCAL").ok().as_deref(),
        );
        let vram_table_path = std::env::var("VRAM_TABLE_PATH").ok();

        Ok(Self {
            database_url,
            environment,
            is_prod,
            manager_token,
            exec_mode,
            orch_workdir,
            trainer_image,
            diffusion_trainer_image,
            port,
            auto_adopt_local,
            vram_table_path,
            watchdog: WatchdogConfig::default(),
        })
    }
}

/// Valida e resolve o MANAGER_TOKEN.
/// Em produção (ENVIRONMENT=production), exige token explícito e rejeita valores triviais ("changeme", "manager-dev-token").
/// Em desenvolvimento, permite fallback para "manager-dev-token" emitindo warning.
pub fn resolve_manager_token(
    raw_token: Result<String, std::env::VarError>,
    is_prod: bool,
) -> Result<String, String> {
    match raw_token {
        Ok(t)
            if is_prod && (t.trim().is_empty() || t == "changeme" || t == "manager-dev-token") =>
        {
            Err(format!(
                "MANAGER_TOKEN inseguro ('{t}') não permitido em produção"
            ))
        }
        Ok(t) if t.trim().is_empty() => Err("MANAGER_TOKEN não pode ser vazio".to_string()),
        Ok(t) => Ok(t),
        Err(_) if is_prod => Err("MANAGER_TOKEN é obrigatório em produção".to_string()),
        Err(_) => {
            tracing::warn!(
                "MANAGER_TOKEN não definido: usando token dev inseguro ('manager-dev-token')"
            );
            Ok("manager-dev-token".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_manager_token_dev_default() {
        let res = resolve_manager_token(Err(std::env::VarError::NotPresent), false);
        assert_eq!(res.unwrap(), "manager-dev-token");
    }

    #[test]
    fn test_resolve_manager_token_empty_fails() {
        let res = resolve_manager_token(Ok("   ".to_string()), false);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), "MANAGER_TOKEN não pode ser vazio");
    }

    #[test]
    fn test_resolve_manager_token_prod_missing_fails() {
        let res = resolve_manager_token(Err(std::env::VarError::NotPresent), true);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), "MANAGER_TOKEN é obrigatório em produção");
    }

    #[test]
    fn test_resolve_manager_token_prod_trivial_fails() {
        let res = resolve_manager_token(Ok("changeme".to_string()), true);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("não permitido em produção"));

        let res2 = resolve_manager_token(Ok("manager-dev-token".to_string()), true);
        assert!(res2.is_err());
        assert!(res2.unwrap_err().contains("não permitido em produção"));
    }

    #[test]
    fn test_resolve_manager_token_prod_valid() {
        let res = resolve_manager_token(Ok("super-secret-token-123".to_string()), true);
        assert_eq!(res.unwrap(), "super-secret-token-123");
    }
}
