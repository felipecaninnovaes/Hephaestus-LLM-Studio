//! Cifra do token dos destinos: AES-256-GCM, chave = HKDF-SHA256 do
//! `jwt_secret` (info `heph:integration-secrets:v1`). O token em claro só
//! existe em memória no instante do uso — nunca volta em resposta nem em log.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm,
};
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore};
use sha2::Sha256;

const HKDF_INFO: &[u8] = b"heph:integration-secrets:v1";
pub const NONCE_LEN: usize = 12;

/// Falha de cifra/decifra (mensagem estática; nunca carrega o segredo).
#[derive(Debug, PartialEq, Eq)]
pub struct CryptoError;

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "integration secret crypto failure")
    }
}

impl std::error::Error for CryptoError {}

fn cipher(jwt_secret: &[u8; 32]) -> Result<Aes256Gcm, CryptoError> {
    let mut key = [0u8; 32];
    Hkdf::<Sha256>::new(None, jwt_secret)
        .expand(HKDF_INFO, &mut key)
        .map_err(|_| CryptoError)?;
    Aes256Gcm::new_from_slice(&key).map_err(|_| CryptoError)
}

/// Cifra `plaintext` com nonce aleatório novo; devolve `(ciphertext, nonce)`.
pub fn encrypt(
    jwt_secret: &[u8; 32],
    plaintext: &str,
) -> Result<(Vec<u8>, [u8; NONCE_LEN]), CryptoError> {
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let ct = cipher(jwt_secret)?
        .encrypt((&nonce).into(), plaintext.as_bytes())
        .map_err(|_| CryptoError)?;
    Ok((ct, nonce))
}

/// Decifra; chave errada, nonce errado ou ciphertext adulterado ⇒ `CryptoError`.
pub fn decrypt(
    jwt_secret: &[u8; 32],
    ciphertext: &[u8],
    nonce: &[u8],
) -> Result<String, CryptoError> {
    let nonce: &[u8; NONCE_LEN] = nonce.try_into().map_err(|_| CryptoError)?;
    let pt = cipher(jwt_secret)?
        .decrypt(nonce.into(), ciphertext)
        .map_err(|_| CryptoError)?;
    String::from_utf8(pt).map_err(|_| CryptoError)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ida_e_volta_preserva_o_token() {
        let secret = [7u8; 32];
        let (ct, nonce) = encrypt(&secret, "tok-ção-123").unwrap();
        assert!(
            !ct.windows(3).any(|w| w == b"tok"),
            "ciphertext não é claro"
        );
        assert_eq!(decrypt(&secret, &ct, &nonce).unwrap(), "tok-ção-123");
    }

    #[test]
    fn nonces_distintos_a_cada_cifra() {
        let secret = [7u8; 32];
        let (c1, n1) = encrypt(&secret, "x").unwrap();
        let (c2, n2) = encrypt(&secret, "x").unwrap();
        assert_ne!(n1, n2);
        assert_ne!(c1, c2);
    }

    #[test]
    fn segredo_errado_ou_adulteracao_falham() {
        let (mut ct, nonce) = encrypt(&[1u8; 32], "abc").unwrap();
        assert_eq!(decrypt(&[2u8; 32], &ct, &nonce), Err(CryptoError));
        ct[0] ^= 1;
        assert_eq!(decrypt(&[1u8; 32], &ct, &nonce), Err(CryptoError));
        assert_eq!(decrypt(&[1u8; 32], &ct, &nonce[..5]), Err(CryptoError));
    }
}
