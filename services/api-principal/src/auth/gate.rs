//! Gate de auth D9 (Fatia 2B): cookie `heph_session` → `verify_jwt` →
//! `AuthUser` na extension do request. Qualquer falha → 401 `unauthorized`.
//! O parse do cookie é puro (sem HTTP) e delega a `handlers` (fonte única).

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};

use super::{handlers, session, AppState};

/// Identidade injetada no request pelo gate (lida via `Extension<AuthUser>`).
#[derive(Clone, Debug)]
pub struct AuthUser {
    pub user_id: String,
    pub jti: String,
}

/// Envelope 401 do gate (D6: `{code, message}` estáticos, sem vazar causa).
pub fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "code": "unauthorized", "message": "unauthorized" })),
    )
        .into_response()
}

/// Parse puro do cookie de sessão (sem HTTP; testável sem tower/axum serve).
pub fn parse_session_cookie(cookie_header: Option<&str>) -> Option<String> {
    handlers::extract_session_cookie(cookie_header)
}

/// Middleware D9: valida `heph_session` e injeta `AuthUser`; falha → 401.
pub async fn require_auth(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    let token = parse_session_cookie(
        req.headers()
            .get(axum::http::header::COOKIE)
            .and_then(|v| v.to_str().ok()),
    );
    let token = match token {
        Some(t) => t,
        None => return unauthorized(),
    };
    let claims = match session::verify_jwt(&token, &state.jwt_secret) {
        Ok(c) => c,
        Err(_) => return unauthorized(),
    };
    req.extensions_mut().insert(AuthUser {
        user_id: claims.sub,
        jti: claims.jti,
    });
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_cookie() {
        assert_eq!(
            parse_session_cookie(Some("heph_session=abc.def.ghi")),
            Some("abc.def.ghi".to_string())
        );
    }

    #[test]
    fn parse_among_multiple_headers() {
        assert_eq!(
            parse_session_cookie(Some("a=1; heph_session=tok123; b=2")),
            Some("tok123".to_string())
        );
        assert_eq!(
            parse_session_cookie(Some("heph_session=primeiro; heph_session=segundo")),
            Some("primeiro".to_string())
        );
    }

    #[test]
    fn parse_missing_cookie() {
        assert_eq!(parse_session_cookie(None), None);
        assert_eq!(parse_session_cookie(Some("a=1; b=2")), None);
        assert_eq!(parse_session_cookie(Some("")), None);
    }

    #[test]
    fn parse_empty_value_rejected() {
        assert_eq!(parse_session_cookie(Some("heph_session=; a=1")), None);
        assert_eq!(parse_session_cookie(Some("heph_session=")), None);
        assert_eq!(parse_session_cookie(Some("heph_session=   ")), None);
    }
}
