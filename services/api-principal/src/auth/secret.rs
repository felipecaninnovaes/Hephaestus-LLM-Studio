//! Segredo HS256 (ADR-0001 D2): `AUTH_SECRET` env > linha em `auth_state` >
//! gerar 32 B + INSERT. Sem pânico: DB fora vira `Err` e o boot propaga.

use rand_core::{OsRng, RngCore};
use sqlx::PgPool;

/// Decodifica `AUTH_SECRET`: exatamente 64 chars hex (32 B). Pura e testável
/// sem DB. Espaço em branco nas bordas é tolerado (env com newline final);
/// espaços internos são rejeitados.
pub fn parse_auth_secret_env(raw: &str) -> Result<[u8; 32], String> {
    let trimmed = raw.trim();
    let hex = trimmed;
    if hex.len() != 64 {
        return Err(format!(
            "AUTH_SECRET must be 64 hex chars (32 bytes), got {} chars",
            hex.len()
        ));
    }
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("AUTH_SECRET must be 64 hex chars (invalid hex digit)".to_string());
    }
    let mut out = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        out[i] = u8::from_str_radix(std::str::from_utf8(chunk).unwrap_or(""), 16)
            .map_err(|_| "AUTH_SECRET must be 64 hex chars (invalid hex digit)".to_string())?;
    }
    Ok(out)
}

/// Carrega ou gera o segredo HS256 (D2). Nunca faz panic; erro vira `Err`.
pub async fn load_or_generate_secret(pool: &PgPool) -> Result<[u8; 32], String> {
    if let Ok(raw) = std::env::var("AUTH_SECRET") {
        if !raw.trim().is_empty() {
            return parse_auth_secret_env(&raw);
        }
    }

    let existing: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT jwt_secret FROM auth_state WHERE id = 1")
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("auth_state select: {e}"))?;
    if let Some(bytes) = existing {
        if bytes.len() != 32 {
            return Err(format!(
                "auth_state.jwt_secret has {} bytes, expected 32",
                bytes.len()
            ));
        }
        let mut secret = [0u8; 32];
        secret.copy_from_slice(&bytes);
        return Ok(secret);
    }

    let mut secret = [0u8; 32];
    OsRng
        .try_fill_bytes(&mut secret)
        .map_err(|e| format!("rng failure: {e}"))?;
    // Atômico contra corrida de dois boots: o perdedor cai no DO NOTHING e
    // relê a linha do vencedor logo abaixo.
    sqlx::query(
        "INSERT INTO auth_state (id, jwt_secret) VALUES (1, $1)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(secret.to_vec())
    .execute(pool)
    .await
    .map_err(|e| format!("auth_state insert: {e}"))?;
    let stored: Vec<u8> = sqlx::query_scalar("SELECT jwt_secret FROM auth_state WHERE id = 1")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("auth_state re-select: {e}"))?;
    if stored.len() != 32 {
        return Err(format!(
            "auth_state.jwt_secret has {} bytes, expected 32",
            stored.len()
        ));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&stored);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_hex() {
        let raw = "ab".repeat(32);
        let secret = parse_auth_secret_env(&raw).expect("valid");
        assert_eq!(secret, [0xab; 32]);
        // maiúsculas também valem
        assert!(parse_auth_secret_env(&"AB".repeat(32)).is_ok());
    }

    #[test]
    fn parse_rejects_short() {
        let err = parse_auth_secret_env(&"ab".repeat(31)).unwrap_err();
        assert!(err.contains("64 hex chars"), "{err}");
        assert!(parse_auth_secret_env("").is_err());
    }

    #[test]
    fn parse_rejects_bad_hex() {
        let bad = "zz".repeat(32);
        let err = parse_auth_secret_env(&bad).unwrap_err();
        assert!(err.contains("hex"), "{err}");
        // 64 chars com 1 dígito inválido no meio
        let mut mid = "ab".repeat(32);
        mid.replace_range(10..11, "z");
        assert!(parse_auth_secret_env(&mid).is_err());
    }
}
