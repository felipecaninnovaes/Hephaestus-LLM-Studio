//! api-principal — boot D3 (ADR-0001):
//! pool via `DATABASE_URL` → migrations → segredo → bootstrap de usuário →
//! modo `ready`/`setup_required` → router de `auth::routes::build` (D9).

use api_principal::auth::{password, routes, secret, AppState};
use api_principal::storage::{MockStorage, S3Storage, StorageConfig, StoragePort};
use sqlx::PgPool;
use std::sync::Arc;

/// Boot do storage (ADR-0003 D8).
///
/// `STORAGE_BACKEND` (default `"mock"`): `"mock"` monta `MockStorage`;
/// `"s3"` monta o `S3Storage` da 3b.4 (env `S3_*`, fail-fast sem ecoar
/// credencial). Mensagens estáticas, sem credencial.
fn load_storage() -> Result<(Arc<dyn StoragePort>, StorageConfig), String> {
    let bucket = std::env::var("S3_BUCKET").unwrap_or_else(|_| "heph-data".to_string());
    let public_endpoint = std::env::var("S3_PUBLIC_ENDPOINT_URL")
        .ok()
        .filter(|s| !s.is_empty());
    let url_ttl_secs = std::env::var("S3_URL_TTL_SECS")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<u64>().map_err(|_| "storage: S3_URL_TTL_SECS inválido"))
        .transpose()?
        .unwrap_or(3600);
    // Revisão 3b.6: fail-fast no range do SigV4 (presigned max 7 dias). Com
    // TTL válido, `presign_get` (assinatura local) não tem caminho de falha —
    // o 503 indocumentado de list/detail (achado F1) torna-se inalcançável.
    if !(1..=604_800).contains(&url_ttl_secs) {
        return Err("storage: S3_URL_TTL_SECS fora de 1..=604800 (máx SigV4 de 7 dias)".to_string());
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
            // Fail-fast sem ecoar VALOR: só o nome da var na mensagem.
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
            Ok((Arc::new(storage), config))
        }
        _ => Err("STORAGE_BACKEND desconhecido".to_string()),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL is not set (required for PgPool)")?;

    // 1. pool — sem pânico: erro vira mensagem no boot.
    //    (URL nunca logada: contém credencial do Postgres.)
    let pool = PgPool::connect(&database_url)
        .await
        .map_err(|e| format!("connect Postgres: {e}"))?;

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
    let studio_password = std::env::var("STUDIO_PASSWORD")
        .ok()
        .filter(|s| !s.is_empty());
    password::ensure_bootstrap_user(&pool, studio_password.as_deref())
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

    let (storage, storage_config) =
        load_storage().map_err(|e| format!("storage: {e}"))?;

    let state = AppState {
        pool,
        jwt_secret,
        secure_cookie,
        setup_required,
        storage,
        storage_config,
    };

    let app = routes::build(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .map_err(|e| format!("bind 0.0.0.0:8080: {e}"))?;
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("serve: {e}"))?;
    Ok(())
}
