//! api-principal — boot D3 (ADR-0001):
//! pool via `DATABASE_URL` → migrations → segredo → bootstrap de usuário →
//! modo `ready`/`setup_required` → router de `auth::routes::build` (D9).

use api_principal::auth::{password, routes, secret, AppState};
use api_principal::jobs::manager_client::{HttpManager, ManagerPort};
use api_principal::search::{EmbeddingPort, HttpEmbedder, MockEmbedder};
use api_principal::storage::{MockStorage, S3Storage, StorageConfig, StoragePort};
use sqlx::PgPool;
use std::sync::Arc;

/// Boot da allow-list de hosts para download por URL (E1 — ADR-0012 D4).
///
/// `MODEL_DOWNLOAD_ALLOWED_HOSTS`: lista separada por vírgulas de hosts
/// autorizados (ex.: `huggingface.co,civitai.com,*.githubusercontent.com`).
/// Env ausente ou vazia ⇒ lista vazia ⇒ download por URL responde 403
/// `model_download_disabled` (fail-closed).
fn load_download_allowed_hosts() -> Vec<String> {
    std::env::var("MODEL_DOWNLOAD_ALLOWED_HOSTS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Boot do storage (ADR-0003 D8).
///
/// `STORAGE_BACKEND` (default `"mock"`): `"mock"` monta `MockStorage`;
/// `"s3"` monta o `S3Storage` da 3b.4 (env `S3_*`, fail-fast sem ecoar
/// credencial) e garante o bucket via `ensure_bucket` (idempotente, retry
/// ~60s e então fail-fast — sem bucket o serviço não sobe). Mensagens
/// estáticas, sem credencial.
///
/// Async porque o ramo s3 faz I/O de rede (CreateBucket); o ramo mock
/// continua sem I/O (prova: teste `mock_backend_monta_sem_s3`).
async fn load_storage() -> Result<(Arc<dyn StoragePort>, StorageConfig), String> {
    let bucket = std::env::var("S3_BUCKET").unwrap_or_else(|_| "heph-data".to_string());
    let public_endpoint = std::env::var("S3_PUBLIC_ENDPOINT_URL")
        .ok()
        .filter(|s| !s.is_empty());
    let url_ttl_secs = std::env::var("S3_URL_TTL_SECS")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<u64>()
                .map_err(|_| "storage: S3_URL_TTL_SECS inválido")
        })
        .transpose()?
        .unwrap_or(3600);
    if !(1..=604_800).contains(&url_ttl_secs) {
        return Err(
            "storage: S3_URL_TTL_SECS fora de 1..=604800 (máx SigV4 de 7 dias)".to_string(),
        );
    }
    let config = StorageConfig {
        bucket,
        public_endpoint,
        url_ttl_secs,
    };
    let backend = std::env::var("STORAGE_BACKEND").unwrap_or_else(|_| "mock".to_string());
    match backend.as_str() {
        "mock" => {
            eprintln!("aviso: STORAGE_BACKEND=mock — uploads NÃO sobrevivem a restart (objeto vive só na RAM; S3Storage disponível via STORAGE_BACKEND=s3 (3b.4))");
            Ok((Arc::new(MockStorage::new()), config))
        }
        "s3" => {
            let endpoint_url = std::env::var("S3_ENDPOINT_URL")
                .ok()
                .filter(|s| !s.is_empty())
                .ok_or("S3_ENDPOINT_URL is not set (required for STORAGE_BACKEND=s3)")?;
            let access_key = std::env::var("S3_ACCESS_KEY")
                .ok()
                .filter(|s| !s.is_empty())
                .ok_or("S3_ACCESS_KEY is not set (required for STORAGE_BACKEND=s3)")?;
            let secret_key = std::env::var("S3_SECRET_KEY")
                .ok()
                .filter(|s| !s.is_empty())
                .ok_or("S3_SECRET_KEY is not set (required for STORAGE_BACKEND=s3)")?;
            let storage = S3Storage::new(&config, &endpoint_url, &access_key, &secret_key)?;
            // Só o backend s3 tem bucket a garantir (trava do teste
            // `bucket_ensure_so_para_s3`); mock nunca passa por aqui.
            if bucket_ensure_required(&backend) {
                storage.ensure_bucket().await?;
            }
            Ok((Arc::new(storage), config))
        }
        _ => Err("STORAGE_BACKEND desconhecido".to_string()),
    }
}

/// Puro/testável: só o backend s3 exige garantia de bucket no boot.
/// O mock é RAM-local (sem bucket); qualquer outro valor é rejeitado
/// pelo `load_storage` antes de chegar aqui.
fn bucket_ensure_required(backend: &str) -> bool {
    backend == "s3"
}

/// Puro/testável: intervalo entre tentativas de conexão ao Postgres.
/// O boot nunca dorme em teste — o valor é travado aqui (2s, padrão do
/// manager) e o loop em `main` só o consome.
fn pg_retry_interval() -> std::time::Duration {
    std::time::Duration::from_secs(2)
}

/// Puro/testável: gerar senha de bootstrap SÓ quando o env está ausente
/// E `users` está vazia (day-one real). Env presente → usa/ignora como
/// antes; usuário existente → silencioso, nunca loga senha inútil.
fn needs_bootstrap_password(env_present: bool, user_exists: bool) -> bool {
    !env_present && !user_exists
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 0. Tracing subscriber (D11 — formatter JSON).
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .json()
        .init();
    tracing::info!("api-principal booting");

    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL is not set (required for PgPool)")?;

    // 1. pool com retry (espelha o manager): em runtime real (não-compose)
    //    o DNS do banco pode atrasar; healthcheck do compose é muleta, não
    //    garantia. WARN curto SEM a URL (credencial nunca em log); dorme 2s
    //    até conectar. `DATABASE_URL` ausente continua fail-fast acima.
    let pool = loop {
        match PgPool::connect(&database_url).await {
            Ok(p) => {
                tracing::info!("conectado ao Postgres");
                break p;
            }
            Err(_) => {
                tracing::warn!("Postgres indisponivel, tentando de novo em 2s");
                tokio::time::sleep(pg_retry_interval()).await;
            }
        }
    };

    // 2. migrations embutidas (sem DATABASE_URL no build).
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|e| format!("run migrations: {e}"))?;

    // 3. segredo HS256 (env > auth_state > gerar).
    let jwt_secret = secret::load_or_generate_secret(&pool)
        .await
        .map_err(|e| format!("load jwt secret: {e}"))?;

    // 4. bootstrap do usuário único (só no 1º boot; depois env é ignorado).
    //    Day-one sem `STUDIO_PASSWORD` e com `users` vazia: gera senha forte,
    //    faz o bootstrap com ela e loga UMA ÚNICA VEZ (precedente: pairing
    //    code do orchestrator). Usuário já existente → env ignorado
    //    silenciosamente (comportamento atual, preservado).
    let studio_password = std::env::var("STUDIO_PASSWORD")
        .ok()
        .filter(|s| !s.is_empty());
    let user_exists_pre: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users)")
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("check users: {e}"))?;
    let generated_password: Option<String> = if needs_bootstrap_password(
        studio_password.is_some(),
        user_exists_pre,
    ) {
        let generated = password::generate_bootstrap_password()
            .map_err(|e| format!("bootstrap password: {e}"))?;
        tracing::warn!(
            bootstrap_password = %generated,
            "STUDIO_PASSWORD nao definida — senha de bootstrap gerada (copie agora; usuario unico, definida so no 1o boot)"
        );
        Some(generated)
    } else {
        None
    };
    let effective_password = studio_password.as_deref().or(generated_password.as_deref());
    password::ensure_bootstrap_user(&pool, effective_password)
        .await
        .map_err(|e| format!("bootstrap user: {e}"))?;

    // 5. modo: ready se existe linha em users, senão setup_required.
    let user_exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users)")
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("check users: {e}"))?;
    let setup_required = !user_exists;

    let secure_cookie = std::env::var("SECURE_COOKIE")
        .map(|v| v == "true")
        .unwrap_or(false);

    let (storage, storage_config) = load_storage().await.map_err(|e| format!("storage: {e}"))?;

    // 6. embedding (ADR-0004 D1, 3f.2): `EMBEDDING_BACKEND` (default `"mock"`).
    let embedding_backend =
        std::env::var("EMBEDDING_BACKEND").unwrap_or_else(|_| "mock".to_string());
    let embedding_model =
        std::env::var("EMBEDDING_MODEL").unwrap_or_else(|_| "ViT-B-32".to_string());
    let embedder: Arc<dyn EmbeddingPort> = match embedding_backend.as_str() {
        "mock" => Arc::new(MockEmbedder::new()),
        "http" => Arc::new(HttpEmbedder::new(
            std::env::var("EMBEDDER_URL").unwrap_or_else(|_| "http://embedder:8090".to_string()),
            embedding_model.clone(),
        )),
        other => return Err(format!("EMBEDDING_BACKEND inválido: {other} (use mock|http)").into()),
    };
    tracing::info!(backend = %embedding_backend, model = %embedding_model, "embedding configured");

    // 7. manager client (ADR-0007 D3): `MANAGER_URL` + `MANAGER_TOKEN` (fail-fast).
    let manager_url =
        std::env::var("MANAGER_URL").unwrap_or_else(|_| "http://manager:8081".to_string());
    let manager_token = std::env::var("MANAGER_TOKEN")
        .map_err(|_| "MANAGER_TOKEN is not set (required for manager client)")?;
    let manager: Arc<dyn ManagerPort> = Arc::new(HttpManager::new(manager_url, manager_token));
    tracing::info!(url = %std::env::var("MANAGER_URL").unwrap_or_else(|_| "http://manager:8081".to_string()), "manager client configured");

    let state = AppState {
        pool,
        jwt_secret,
        secure_cookie,
        setup_required,
        storage,
        storage_config,
        embedder,
        embedding_model,
        manager,
        model_download_allowed_hosts: load_download_allowed_hosts(),
    };

    // 8. Recovery de preparações órfãs (ADR-0025 D3 — espelha `recover_jobs`
    //    do manager): `job_prepares` em `preparing` com `updated_at` > 10min
    //    ⇒ re-spawn (attempts < 3) ou `prepare_fail{timeout}`. Best-effort:
    //    nunca derruba o boot.
    match api_principal::jobs::prepare::recover_stale_prepares(&state).await {
        Ok((respawned, failed)) if respawned + failed > 0 => {
            tracing::info!(respawned, failed, "recovery de preparações concluído")
        }
        Ok(_) => tracing::info!("recovery de preparações: nada órfão"),
        Err(e) => tracing::error!("recovery de preparações falhou: {e}"),
    }

    // 8b. Worker periódico de recovery de preparações órfãs em background (a cada 60s)
    let bg_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if let Err(e) = api_principal::jobs::prepare::recover_stale_prepares(&bg_state).await {
                tracing::warn!("background recover_stale_prepares erro: {e}");
            }
        }
    });

    let app = routes::build(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .map_err(|e| format!("bind 0.0.0.0:8080: {e}"))?;
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("serve: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pg_retry_interval_e_2s() {
        // Retry sem dormir em teste: o loop consome este valor (2s, padrão
        // do manager). Sem URL/credencial envolvida.
        assert_eq!(pg_retry_interval(), std::time::Duration::from_secs(2));
    }

    #[test]
    fn bootstrap_password_so_no_day_one_real() {
        // Gera SÓ com env ausente E users vazia; nos outros 3 casos o env
        // segue o fluxo atual (usa ou ignora silenciosamente).
        assert!(needs_bootstrap_password(false, false), "day-one: gera");
        assert!(
            !needs_bootstrap_password(true, false),
            "env presente, sem usuário: usa o env, não gera"
        );
        assert!(
            !needs_bootstrap_password(false, true),
            "usuário existe, sem env: silencioso, não gera nem loga"
        );
        assert!(
            !needs_bootstrap_password(true, true),
            "env presente, usuário existe: ignora silenciosamente"
        );
    }

    #[test]
    fn bucket_ensure_so_para_s3() {
        // Trava a invariante: mock (RAM-local) nunca exige bucket.
        assert!(bucket_ensure_required("s3"), "s3 garante bucket");
        assert!(!bucket_ensure_required("mock"), "mock não invoca ensure");
        assert!(
            !bucket_ensure_required("gcs"),
            "backend desconhecido não invoca ensure (é rejeitado antes)"
        );
    }

    #[tokio::test]
    async fn mock_backend_monta_sem_s3() {
        // `STORAGE_BACKEND=mock` não invoca `ensure_bucket`: prova por
        // construção — com env S3 ausente o mock monta Ok (o ramo s3
        // falharia fail-fast no `S3_ENDPOINT_URL`). Salva/restaura env
        // (processo de teste é compartilhado entre threads).
        let prev_backend = std::env::var("STORAGE_BACKEND").ok();
        let prev_endpoint = std::env::var("S3_ENDPOINT_URL").ok();
        let prev_access = std::env::var("S3_ACCESS_KEY").ok();
        let prev_secret = std::env::var("S3_SECRET_KEY").ok();
        std::env::set_var("STORAGE_BACKEND", "mock");
        std::env::remove_var("S3_ENDPOINT_URL");
        std::env::remove_var("S3_ACCESS_KEY");
        std::env::remove_var("S3_SECRET_KEY");
        let r = load_storage().await;
        match prev_backend {
            Some(v) => std::env::set_var("STORAGE_BACKEND", v),
            None => std::env::remove_var("STORAGE_BACKEND"),
        }
        match prev_endpoint {
            Some(v) => std::env::set_var("S3_ENDPOINT_URL", v),
            None => {}
        }
        match prev_access {
            Some(v) => std::env::set_var("S3_ACCESS_KEY", v),
            None => {}
        }
        match prev_secret {
            Some(v) => std::env::set_var("S3_SECRET_KEY", v),
            None => {}
        }
        assert!(r.is_ok(), "mock monta sem S3 nem rede");
    }
}
