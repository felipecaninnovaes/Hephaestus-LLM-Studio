//! Hash Argon2id + bootstrap do usuário único (ADR-0001 D1/D3).
//!
//! Nenhuma senha é logada em nenhum caminho deste módulo.

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rand_core::OsRng;
use sqlx::PgPool;

/// Erro do bootstrap: hash (CPU/rng) ou banco. `String` no display para o
/// boot propagar mensagem legível sem pânico.
#[derive(Debug)]
pub enum BootstrapError {
    Hash(String),
    Db(sqlx::Error),
}

impl std::fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hash(e) => write!(f, "hash bootstrap password: {e}"),
            Self::Db(e) => write!(f, "bootstrap user query: {e}"),
        }
    }
}

impl std::error::Error for BootstrapError {}

/// Gera o hash PHC (`$argon2id$...`) com `Params::default()` e salt de 16 B.
pub fn hash(password: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    // Default = Argon2id + Version 0x13 + Params::default() (m=19MiB,t=2,p=1).
    let argon2 = Argon2::default();
    Ok(argon2
        .hash_password(password.as_bytes(), &salt)?
        .to_string())
}

/// Compara a senha contra o PHC; qualquer erro de parse/hash → `false`
/// (genérico por design, sem distinguir causa).
pub fn verify(password: &str, phc: &str) -> bool {
    let parsed = match PasswordHash::new(phc) {
        Ok(h) => h,
        Err(_) => return false,
    };
    // Reforça Argon2id explícito (Default já é Argon2id, defesa extra).
    let argon2 = Argon2::default();
    argon2.verify_password(password.as_bytes(), &parsed).is_ok()
}

/// Bootstrap D3 (ajuste 2): INSERT único atômico, no-op se `users` já tem
/// linha ou se `password` é `None` (env ausente/vazio). Retorna `true` se
/// existe usuário após a chamada.
pub async fn ensure_bootstrap_user(
    pool: &PgPool,
    password: Option<&str>,
) -> Result<bool, BootstrapError> {
    if let Some(pw) = password.filter(|s| !s.is_empty()) {
        let phc = hash(pw).map_err(|e| BootstrapError::Hash(e.to_string()))?;
        sqlx::query(
            "INSERT INTO users (password_hash)
             SELECT $1 WHERE NOT EXISTS (SELECT 1 FROM users)",
        )
        .bind(phc)
        .execute(pool)
        .await
        .map_err(BootstrapError::Db)?;
    }
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users)")
        .fetch_one(pool)
        .await
        .map_err(BootstrapError::Db)?;
    Ok(exists)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_then_verify_ok() {
        let phc = hash("correct-horse").expect("hash");
        assert!(verify("correct-horse", &phc));
    }

    #[test]
    fn verify_wrong_password_false() {
        let phc = hash("correct-horse").expect("hash");
        assert!(!verify("wrong-password", &phc));
    }

    #[test]
    fn phc_format_is_argon2id() {
        let phc = hash("x").expect("hash");
        assert!(phc.starts_with("$argon2id$"), "phc: {phc}");
        assert!(!verify("x", "not-a-valid-phc"));
    }
}
