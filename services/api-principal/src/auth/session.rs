//! JWT HS256 (ADR-0001 D2): `jsonwebtoken 9`, TTL 7 dias, sem refresh.
//! Timestamps como `usize` via chrono — sem crate `time`.

use chrono::{DateTime, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const ISSUER: &str = "hephaestus-studio";
/// TTL de 7 dias em segundos.
pub const TTL_SECS: i64 = 7 * 24 * 60 * 60;
/// Leeway de 30 s contra drift de relógio (laptop suspenso).
pub const LEEWAY_SECS: u64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub iss: String,
    pub sub: String,
    pub iat: usize,
    pub exp: usize,
    pub jti: String,
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

fn mint(user_id: Uuid, secret: &[u8; 32], iat: DateTime<Utc>, exp: DateTime<Utc>) -> String {
    let mut jti_bytes = [0u8; 16];
    OsRng.fill_bytes(&mut jti_bytes);
    let claims = Claims {
        iss: ISSUER.to_string(),
        sub: user_id.to_string(),
        iat: iat.timestamp() as usize,
        exp: exp.timestamp() as usize,
        jti: hex_encode(&jti_bytes),
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret),
    )
    .expect("jwt encode uses in-memory secret and valid claims")
}

/// Emite o JWT da sessão. Retorna `(token, iat)` — `iat` vira `loggedAt`.
pub fn issue_jwt(user_id: Uuid, secret: &[u8; 32]) -> (String, DateTime<Utc>) {
    let now = Utc::now();
    let exp = now + chrono::Duration::seconds(TTL_SECS);
    (mint(user_id, secret, now, exp), now)
}

/// Valida assinatura + `exp` (leeway 30 s) + `iss`.
pub fn verify_jwt(token: &str, secret: &[u8; 32]) -> Result<Claims, jsonwebtoken::errors::Error> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = LEEWAY_SECS;
    validation.set_issuer(&[ISSUER]);
    let data = decode::<Claims>(token, &DecodingKey::from_secret(secret), &validation)?;
    Ok(data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret_fixture() -> [u8; 32] {
        [0x42; 32]
    }

    #[test]
    fn valid_token_claims() {
        let id = Uuid::new_v4();
        let secret = secret_fixture();
        let (token, iat) = issue_jwt(id, &secret);
        let claims = verify_jwt(&token, &secret).expect("valid");
        assert_eq!(claims.sub, id.to_string());
        assert_eq!(claims.iss, ISSUER);
        assert_eq!(claims.iat, iat.timestamp() as usize);
        assert_eq!(claims.exp - claims.iat, TTL_SECS as usize);
        assert_eq!(claims.jti.len(), 32);
    }

    #[test]
    fn expired_token_rejected() {
        let secret = secret_fixture();
        let past = Utc::now() - chrono::Duration::hours(2);
        let token = mint(
            Uuid::new_v4(),
            &secret,
            past,
            past + chrono::Duration::seconds(60),
        );
        assert!(verify_jwt(&token, &secret).is_err());
    }

    #[test]
    fn tampered_token_rejected() {
        let secret = secret_fixture();
        let (token, _) = issue_jwt(Uuid::new_v4(), &secret);
        let mut chars: Vec<char> = token.chars().collect();
        let mid = chars.len() / 2;
        chars[mid] = if chars[mid] == 'a' { 'b' } else { 'a' };
        let tampered: String = chars.into_iter().collect();
        assert!(verify_jwt(&tampered, &secret).is_err());
    }

    #[test]
    fn leeway_accepts_slightly_past_exp() {
        let secret = secret_fixture();
        let now = Utc::now();
        // exp 20 s no passado (< leeway 30 s) ainda valida
        let token = mint(
            Uuid::new_v4(),
            &secret,
            now - chrono::Duration::seconds(100),
            now - chrono::Duration::seconds(20),
        );
        assert!(verify_jwt(&token, &secret).is_ok());
    }

    #[test]
    fn wrong_secret_rejected() {
        let (token, _) = issue_jwt(Uuid::new_v4(), &secret_fixture());
        assert!(verify_jwt(&token, &[0x99; 32]).is_err());
    }
}
