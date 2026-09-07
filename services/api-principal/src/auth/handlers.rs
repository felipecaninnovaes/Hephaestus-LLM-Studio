//! Handlers auth (contrato `packages/contracts/openapi.yaml`).
//!
//! - `login`: 200 + `Set-Cookie` | 400 `invalid_request` | 401
//!   `invalid_credentials` | 503 `setup_required`.
//! - `me`: auto-valida o cookie → 200 `{userId, loggedAt=iat}` | 401.
//! - `logout`: sempre 204 + cookie expirado (idempotente, nem lê token).
//!
//! Cookie SEMPRE via header `SET_COOKIE` manual (axum 0.7, sem torre extra).
//! Nenhuma senha/token é logada em nenhum caminho.

use axum::{
    body::Bytes,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use super::{
    password,
    session::{self, Claims},
    AppState,
};
use crate::error::{err, MSG_UNAUTHORIZED};

pub const SESSION_COOKIE: &str = "heph_session";
pub const SESSION_MAX_AGE_SECS: u64 = 604800; // 7 dias = TTL do JWT

// --- mensagens estáticas por code (D6): nunca contêm hash/token/segredo ---

const MSG_INVALID_REQUEST: &str = "invalid request: password is required";
const MSG_INVALID_CREDENTIALS: &str = "invalid credentials";
const MSG_SETUP_REQUIRED: &str = "setup required: no user yet, set STUDIO_PASSWORD on first boot";
const MSG_INTERNAL: &str = "internal server error";

/// Construtor puro do `Set-Cookie` de sessão (formato exato do contrato).
pub fn build_set_cookie(token: &str, max_age_secs: u64, secure: bool) -> String {
    if secure {
        format!(
            "{SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age_secs}; Secure"
        )
    } else {
        format!("{SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age_secs}")
    }
}

/// Cookie de limpeza do logout (`Max-Age=0`).
pub fn build_cleared_cookie(secure: bool) -> String {
    build_set_cookie("", 0, secure)
}

fn set_cookie_headers(value: &str, secure: bool, max_age_secs: u64) -> Result<HeaderMap, Response> {
    let mut headers = HeaderMap::new();
    let hv: HeaderValue = build_set_cookie(value, max_age_secs, secure)
        .parse()
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "internal", MSG_INTERNAL))?;
    headers.insert(header::SET_COOKIE, hv);
    Ok(headers)
}

/// Extrai `heph_session` do header `Cookie`. Pura e testável.
pub fn extract_session_cookie(cookie_header: Option<&str>) -> Option<String> {
    let header_value = cookie_header?;
    for part in header_value.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("heph_session=") {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MeResponse {
    user_id: String,
    logged_at: DateTime<Utc>,
}

fn me_response(user_id: &Uuid, iat: DateTime<Utc>) -> MeResponse {
    MeResponse {
        user_id: user_id.to_string(),
        logged_at: iat,
    }
}

fn claims_to_me(claims: &Claims) -> Result<MeResponse, Response> {
    let iat = DateTime::from_timestamp(claims.iat as i64, 0)
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "unauthorized", MSG_UNAUTHORIZED))?;
    Ok(MeResponse {
        user_id: claims.sub.clone(),
        logged_at: iat,
    })
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginRequest {
    password: String,
}

/// POST /api/auth/login — ordem: 400 (body) → 503 (setup) → 401 (credencial).
pub async fn login(state: axum::extract::State<AppState>, body: Bytes) -> Response {
    let req: LoginRequest = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                MSG_INVALID_REQUEST,
            );
        }
    };
    let password_input = req.password.as_str();

    if state.setup_required {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "setup_required",
            MSG_SETUP_REQUIRED,
        );
    }

    // Query única sobre users: SELECT id, password_hash LIMIT 1.
    let row: Option<(Uuid, String)> =
        match sqlx::query_as("SELECT id, password_hash FROM users LIMIT 1")
            .fetch_optional(&state.pool)
            .await
        {
            Ok(r) => r,
            Err(_) => {
                return err(StatusCode::INTERNAL_SERVER_ERROR, "internal", MSG_INTERNAL);
            }
        };
    let (user_id, phc) = match row {
        Some(r) => r,
        // Genérico por design: não distingue "sem usuário" de "senha errada".
        None => {
            return err(
                StatusCode::UNAUTHORIZED,
                "invalid_credentials",
                MSG_INVALID_CREDENTIALS,
            );
        }
    };
    if !password::verify(password_input, &phc) {
        return err(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            MSG_INVALID_CREDENTIALS,
        );
    }

    let (token, iat) = session::issue_jwt(user_id, &state.jwt_secret);
    let headers = match set_cookie_headers(&token, state.secure_cookie, SESSION_MAX_AGE_SECS) {
        Ok(h) => h,
        Err(resp) => return resp,
    };
    (StatusCode::OK, headers, Json(me_response(&user_id, iat))).into_response()
}

/// GET /api/auth/me — valida o próprio cookie (gate desta rota, D9).
pub async fn me(state: axum::extract::State<AppState>, headers: HeaderMap) -> Response {
    let cookie = headers.get(header::COOKIE).and_then(|v| v.to_str().ok());
    let token = match extract_session_cookie(cookie) {
        Some(t) => t,
        None => return err(StatusCode::UNAUTHORIZED, "unauthorized", MSG_UNAUTHORIZED),
    };
    let claims = match session::verify_jwt(&token, &state.jwt_secret) {
        Ok(c) => c,
        Err(_) => return err(StatusCode::UNAUTHORIZED, "unauthorized", MSG_UNAUTHORIZED),
    };
    match claims_to_me(&claims) {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        Err(resp) => resp,
    }
}

/// POST /api/auth/logout — sempre 204 + cookie expirado.
pub async fn logout(state: axum::extract::State<AppState>) -> Response {
    let mut headers = HeaderMap::new();
    let hv: HeaderValue = if state.secure_cookie {
        HeaderValue::from_static("heph_session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0; Secure")
    } else {
        HeaderValue::from_static("heph_session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0")
    };
    headers.insert(header::SET_COOKIE, hv);
    (StatusCode::NO_CONTENT, headers).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_without_secure_matches_contract() {
        assert_eq!(
            build_set_cookie("TOKEN", SESSION_MAX_AGE_SECS, false),
            "heph_session=TOKEN; HttpOnly; SameSite=Lax; Path=/; Max-Age=604800"
        );
    }

    #[test]
    fn cookie_with_secure_appends_flag() {
        assert_eq!(
            build_set_cookie("TOKEN", SESSION_MAX_AGE_SECS, true),
            "heph_session=TOKEN; HttpOnly; SameSite=Lax; Path=/; Max-Age=604800; Secure"
        );
    }

    #[test]
    fn cleared_cookie_expires() {
        assert_eq!(
            build_cleared_cookie(false),
            "heph_session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0"
        );
        assert!(build_cleared_cookie(true).ends_with("; Secure"));
    }

    #[test]
    fn extract_cookie_values() {
        assert_eq!(
            extract_session_cookie(Some("heph_session=abc.def.ghi")),
            Some("abc.def.ghi".to_string())
        );
        assert_eq!(
            extract_session_cookie(Some("a=1; heph_session=tok123; b=2")),
            Some("tok123".to_string())
        );
        assert_eq!(extract_session_cookie(None), None);
        assert_eq!(extract_session_cookie(Some("a=1; b=2")), None);
        assert_eq!(extract_session_cookie(Some("heph_session=; a=1")), None);
    }

    #[tokio::test]
    async fn login_rejects_unknown_fields() {
        let pool = sqlx::PgPool::connect_lazy("postgres://n/n").expect("lazy pool");
        let state = crate::auth::AppState {
            pool,
            jwt_secret: [0x42; 32],
            secure_cookie: false,
            setup_required: true,
            storage: std::sync::Arc::new(crate::storage::MockStorage::new()),
            storage_config: crate::storage::StorageConfig {
                bucket: "heph-test".into(),
                public_endpoint: None,
                url_ttl_secs: 60,
            },
            embedder: std::sync::Arc::new(crate::search::MockEmbedder::new()),
            embedding_model: "ViT-B-32".to_string(),
        };
        let resp = login(
            axum::extract::State(state),
            Bytes::from(r#"{"password":"x","extra":1}"#),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
