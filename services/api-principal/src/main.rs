//! api-principal — boot D3 (ADR-0001):
//! pool via `DATABASE_URL` → migrations → segredo → bootstrap de usuário →
//! modo `ready`/`setup_required` → router de `auth::routes::build` (D9).

use api_principal::auth::{password, routes, secret, AppState};
use sqlx::PgPool;

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

    let state = AppState {
        pool,
        jwt_secret,
        secure_cookie,
        setup_required,
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
