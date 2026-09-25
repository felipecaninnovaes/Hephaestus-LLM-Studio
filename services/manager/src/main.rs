//! Manager service — entrypoint minimalista (ADR-0007 F4.3).
//!
//! `main.rs` é o entrypoint enxuto: tracing, leitura estruturada de config,
//! pool com retry, boot (auto-adoção + recovery), worker de dispatch em background
//! e inicialização do servidor HTTP axum.

use sqlx::PgPool;
use std::sync::Arc;

use manager::config::ManagerConfig;
use manager::http::{build_router, AppState};
use manager::orchestrator::{HttpOrchestratorClient, OrchestratorClient};
use manager::policy::VramTable;

#[tokio::main]
async fn main() {
    // Tracing.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "manager=info,tower_http=info".into()),
        )
        .json()
        .init();

    // Configuração estruturada.
    let config = ManagerConfig::from_env().unwrap_or_else(|e| panic!("config error: {e}"));

    // DB pool com retry.
    let pool = loop {
        match PgPool::connect(&config.database_url).await {
            Ok(p) => {
                tracing::info!("conectado ao Postgres");
                break p;
            }
            Err(e) => {
                tracing::warn!("falha ao conectar no Postgres: {e}, tentando em 2s...");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    };

    // Boot: adopt (AUTO_ADOPT_LOCAL=0 → skip) + recovery.
    if config.auto_adopt_local {
        if let Err(e) = manager::adopt_orchestrator(&pool).await {
            tracing::error!("falha ao auto-adotar orchestrator: {e}");
        } else {
            tracing::info!("orchestrator-local auto-adotado");
        }
    } else {
        tracing::info!("AUTO_ADOPT_LOCAL=0: pulando auto-adoção de orchestrator-local");
    }

    match manager::recover_jobs(&pool).await {
        Ok(n) if n > 0 => tracing::info!("recovery: {n} jobs recuperados para queued"),
        Ok(_) => tracing::info!("recovery: nenhum job órfão"),
        Err(e) => tracing::error!("falha no recovery: {e}"),
    }

    // VRAM table (fail-fast no boot).
    let vram_table = VramTable::load_or_default(config.vram_table_path.as_deref())
        .unwrap_or_else(|e| panic!("vram-table inválido: {e}"));
    tracing::info!(
        "vram-table: {} entradas, headroom={}GB",
        vram_table.entries.len(),
        vram_table.defaults.headroom_gb
    );

    // State.
    let orch_client = Arc::new(HttpOrchestratorClient::new(Some(
        config.manager_token.clone(),
    )));
    let state = AppState {
        pool: pool.clone(),
        token: config.manager_token,
        telemetry_cache: manager::new_telemetry_cache(),
        orch_client,
        exec_mode: config.exec_mode,
        orch_workdir: config.orch_workdir,
        trainer_image: config.trainer_image,
        vram_table,
    };

    // Dispatch worker (tokio task).
    let dispatch_pool = pool.clone();
    let dispatch_client: Arc<dyn OrchestratorClient> = state.orch_client.clone();
    let dispatch_mode = state.exec_mode.clone();
    let dispatch_workdir = state.orch_workdir.clone();
    let dispatch_image = state.trainer_image.clone();
    let dispatch_vram = state.vram_table.clone();
    let dispatch_interval = std::time::Duration::from_secs(config.watchdog.dispatch_interval_secs);

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(dispatch_interval);
        loop {
            interval.tick().await;

            // Watchdog tick (ADR-0011 D4).
            if let Err(e) = manager::watchdog_tick(&dispatch_pool).await {
                tracing::error!("watchdog error: {e}");
            }

            match manager::dispatch_next(
                &dispatch_pool,
                dispatch_client.as_ref(),
                &dispatch_mode,
                &dispatch_workdir,
                &dispatch_image,
                &dispatch_vram,
            )
            .await
            {
                Ok(true) => tracing::info!("dispatch: job despachado"),
                Ok(false) => {} // Nenhum job na fila.
                Err(e) => tracing::error!("dispatch error: {e}"),
            }
        }
    });

    // Server.
    let port = config.port;
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("bind");
    tracing::info!("manager ouvindo em 0.0.0.0:{port}");
    axum::serve(listener, app).await.unwrap();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use manager::config::resolve_manager_token;
    use tower::ServiceExt;

    struct NoopOrch;

    #[async_trait::async_trait]
    impl OrchestratorClient for NoopOrch {
        async fn post(&self, _url: &str, _body: &serde_json::Value) -> Result<(), String> {
            Ok(())
        }
        async fn post_json(
            &self,
            _url: &str,
            _body: &serde_json::Value,
        ) -> Result<serde_json::Value, String> {
            Ok(serde_json::json!({}))
        }
    }

    fn test_state(pool: PgPool) -> AppState {
        let vram_table = VramTable::parse(
            "defaults:\n  headroom_gb: 2\nentries:\n  - { engine: yolo, model: yolo11n, mode: train, vram_min_gb: 6 }\n",
        )
        .unwrap();
        AppState {
            pool,
            token: "test-token".into(),
            telemetry_cache: manager::new_telemetry_cache(),
            orch_client: Arc::new(NoopOrch),
            exec_mode: "local".into(),
            orch_workdir: "/tmp".into(),
            trainer_image: "trainer:latest".into(),
            vram_table,
        }
    }

    /// Guarda anti-footgun: `#[sqlx::test]` cria bancos `_sqlx_test_*` no
    /// servidor de `DATABASE_URL` — a URL deve apontar para o banco efêmero
    /// `studio_test` (via `bash scripts/test-db.sh`), nunca para o dev
    /// `studio`. Checa a URL (não `current_database`, que é o nome gerado).
    fn assert_test_db_url(url: &str) {
        let path = url.rsplit('/').find(|s| !s.is_empty()).unwrap_or("");
        let db = path.split('?').next().unwrap_or("");
        assert!(
            db.starts_with("studio_test"),
            "HARNESS DE TESTE RECUSANDO BANCO PERIGOSO: use studio_test via scripts/test-db.sh — nunca o DB de dev 'studio' (banco na URL: '{db}')"
        );
    }

    fn test_db_url() -> String {
        let url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL é obrigatório (bash scripts/test-db.sh)");
        assert_test_db_url(&url);
        url
    }

    #[test]
    fn guarda_harness_aceita_studio_test() {
        assert_test_db_url("postgres://studio:studio@localhost:5432/studio_test");
    }

    #[test]
    #[should_panic(expected = "HARNESS DE TESTE RECUSANDO BANCO PERIGOSO")]
    fn guarda_harness_rejeita_studio_dev() {
        assert_test_db_url("postgres://studio:studio@localhost:5432/studio");
    }

    /// POST /internal/jobs com weights_id inexistente → 404 not_found.
    #[sqlx::test(migrations = "../api-principal/migrations")]
    #[ignore = "requer Postgres (bash scripts/test-db.sh)"]
    async fn create_job_handler_not_found(pool: PgPool) {
        let _ = test_db_url();
        let app = build_router(test_state(pool));

        let fake_id = uuid::Uuid::new_v4();
        let body = serde_json::json!({
            "kind": "yolo_train",
            "engine": "yolo",
            "model": "yolo11m",
            "mode": "train",
            "weights_id": fake_id,
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/internal/jobs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(json["code"], "not_found");
    }

    /// POST /internal/jobs com JSON inválido → 400 invalid_request.
    #[sqlx::test(migrations = "../api-principal/migrations")]
    #[ignore = "requer Postgres (bash scripts/test-db.sh)"]
    async fn create_job_handler_invalid_json(pool: PgPool) {
        let _ = test_db_url();
        let app = build_router(test_state(pool));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/internal/jobs")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from("not json"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(json["code"], "invalid_request");
    }

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
