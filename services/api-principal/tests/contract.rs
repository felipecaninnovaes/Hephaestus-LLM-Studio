//! Teste de contrato D8 (Fatia 2B): inventário OpenAPI ≡ `auth::routes::routes_all()`
//! (ignorando `x-reserved: true`) + sondas oneshot em estado "setup" (sem
//! Postgres: `PgPool::connect_lazy` nunca é consultado nesses caminhos).

use std::collections::BTreeSet;
use std::path::PathBuf;

use api_principal::auth::{routes, session, AppState};
use axum::body::Body;
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;

const SETUP_SECRET: [u8; 32] = [0x42; 32];

/// Localiza `packages/contracts/openapi.yaml` subindo do `CARGO_MANIFEST_DIR`
/// (vale para `services/api-principal`: 2 níveis até a raiz do monorepo).
fn openapi_path() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for _ in 0..4 {
        let cand = dir.join("packages/contracts/openapi.yaml");
        if cand.is_file() {
            return cand;
        }
        if !dir.pop() {
            break;
        }
    }
    panic!("packages/contracts/openapi.yaml não encontrado a partir de CARGO_MANIFEST_DIR");
}

fn setup_state() -> AppState {
    // Lazy: nunca conecta (sondas não tocam o banco).
    let pool = PgPool::connect_lazy("postgres://n/n").expect("lazy pool");
    AppState {
        pool,
        jwt_secret: SETUP_SECRET,
        secure_cookie: false,
        setup_required: true,
    }
}

async fn call(
    app: axum::Router,
    req: Request<Body>,
) -> (StatusCode, http::HeaderMap, Vec<u8>) {
    let resp = app.oneshot(req).await.expect("oneshot");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body = resp
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes()
        .to_vec();
    (status, headers, body)
}

fn json(body: &[u8]) -> serde_json::Value {
    serde_json::from_slice(body).expect("corpo JSON das sondas")
}

#[test]
fn inventory_matches_openapi() {
    let text = std::fs::read_to_string(openapi_path()).expect("ler openapi.yaml");
    let yaml: serde_yaml::Value = serde_yaml::from_str(&text).expect("parse openapi.yaml");

    // Conjunto da spec: (path, METHOD, status), sem `x-reserved: true`.
    let mut spec = BTreeSet::new();
    let methods = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];
    let paths = yaml
        .get("paths")
        .and_then(|v| v.as_mapping())
        .expect("openapi.paths");
    for (path, item) in paths {
        let path = path.as_str().expect("path string");
        let item = item.as_mapping().expect("path-item mapping");
        for (method, op) in item {
            let method = method.as_str().expect("method string").to_uppercase();
            if !methods.contains(&method.as_str()) {
                continue;
            }
            let responses = op
                .get("responses")
                .and_then(|v| v.as_mapping())
                .expect("operation.responses");
            for (status, resp) in responses {
                if resp.get("x-reserved").and_then(|v| v.as_bool()) == Some(true) {
                    continue;
                }
                let status: u16 = status
                    .as_str()
                    .expect("status string")
                    .parse()
                    .expect("status numérico");
                spec.insert((path.to_string(), method.clone(), status));
            }
        }
    }

    // Conjunto do código: expansão da união real (routes_all).
    let mut code = BTreeSet::new();
    for (method, path, statuses) in &routes::routes_all() {
        for s in *statuses {
            code.insert((path.to_string(), method.to_string(), *s));
        }
    }

    assert_eq!(code, spec, "deriva contrato vs router (D8): routes_all ≠ openapi.yaml");

    // HealthResponse do inventário bate com o `/health` atual (campo `auth` novo).
    let health = &yaml["components"]["schemas"]["HealthResponse"];
    let required: BTreeSet<&str> = health["required"]
        .as_sequence()
        .expect("HealthResponse.required")
        .iter()
        .map(|v| v.as_str().expect("required string"))
        .collect();
    assert_eq!(
        required,
        BTreeSet::from(["status", "service", "auth"]),
        "HealthResponse.required"
    );
    let auth_enum: BTreeSet<&str> = health["properties"]["auth"]["enum"]
        .as_sequence()
        .expect("HealthResponse.auth enum")
        .iter()
        .map(|v| v.as_str().expect("enum string"))
        .collect();
    assert_eq!(
        auth_enum,
        BTreeSet::from(["ready", "setup_required"]),
        "HealthResponse.auth enum"
    );
}

#[tokio::test]
async fn probes_setup_state() {
    let app = routes::build(setup_state());

    // GET /health → 200 + auth:"setup_required".
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri("/health")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["auth"], "setup_required");

    // GET /api/auth/me sem cookie → 401 unauthorized.
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/auth/me")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json(&body)["code"], "unauthorized");

    // POST /api/auth/logout → 204 (+ Set-Cookie Max-Age=0).
    let (status, headers, _) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/auth/logout")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let set_cookie = headers
        .get(http::header::SET_COOKIE)
        .expect("logout Set-Cookie")
        .to_str()
        .unwrap()
        .to_string();
    assert!(set_cookie.contains("Max-Age=0"), "{set_cookie}");

    // POST /api/auth/login body {} → 400 invalid_request (body-first, 2A).
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/auth/login")
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // POST /api/auth/login {"password":"x"} em setup → 503 setup_required
    // (handler checa setup ANTES do pool — não toca banco).
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/auth/login")
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"password":"x"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json(&body)["code"], "setup_required");

    // GET /api/auth/me COM cookie válido → 200 camelCase (guarda de casing).
    let fixed_id = uuid::Uuid::from_u128(0x1234_5678_9abc_def0_1234_56789abcdef0);
    let (token, _) = session::issue_jwt(fixed_id, &SETUP_SECRET);
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/auth/me")
            .header(http::header::COOKIE, format!("heph_session={token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let me = json(&body);
    assert!(me.get("userId").is_some(), "{me}");
    assert!(me.get("loggedAt").is_some(), "{me}");
    assert!(me.get("user_id").is_none(), "{me}");
    assert!(me.get("logged_at").is_none(), "{me}");

    // GET /api/nada sem cookie → 401 (D9, não 404).
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/nada")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json(&body)["code"], "unauthorized");

    // GET /api/nada COM cookie válido → 404 sem body.
    let (token, _) = session::issue_jwt(uuid::Uuid::new_v4(), &SETUP_SECRET);
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/nada")
            .header(http::header::COOKIE, format!("heph_session={token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.is_empty(), "404 do fallback não tem body");
}

#[test]
fn security_class_matches_openapi() {
    // Cinto estrutural p/ fadiga 3+: a CLASSE da rota vem da spec, não da boa
    // vontade do autor. Na OpenAPI, operação pública declara `security: []`
    // explícito; sem a chave, vale o default global (sessionCookie) → tem que
    // estar em PROTECTED_ROUTES. Mover rota de negócio p/ PUBLIC = vermelho.
    let text = std::fs::read_to_string(openapi_path()).expect("ler openapi.yaml");
    let yaml: serde_yaml::Value = serde_yaml::from_str(&text).expect("parse openapi.yaml");
    let paths = yaml
        .get("paths")
        .and_then(|v| v.as_mapping())
        .expect("openapi.paths");
    let methods = ["get", "post", "put", "delete", "patch"];
    let mut spec_public = BTreeSet::new();
    let mut spec_protected = BTreeSet::new();
    for (path, item) in paths {
        let path = path.as_str().expect("path string").to_string();
        for m in methods {
            let Some(op) = item.get(m) else { continue };
            let key = (path.clone(), m.to_uppercase());
            if op.get("security").is_some() {
                spec_public.insert(key);
            } else {
                spec_protected.insert(key);
            }
        }
    }
    let code_public: BTreeSet<(String, String)> = routes::PUBLIC_ROUTES
        .iter()
        .map(|(m, p, _)| (p.to_string(), m.to_string()))
        .collect();
    let code_protected: BTreeSet<(String, String)> = routes::PROTECTED_ROUTES
        .iter()
        .map(|(m, p, _)| (p.to_string(), m.to_string()))
        .collect();
    assert_eq!(code_public, spec_public, "públicas da spec ≠ PUBLIC_ROUTES");
    assert_eq!(
        code_protected, spec_protected,
        "rota com sessionCookie na spec ausente de PROTECTED_ROUTES (gate obrigatório)"
    );
}

#[tokio::test]
async fn protected_routes_fail_closed() {
    // Cinto: toda rota protegida sem cookie → 401 unauthorized (vazio hoje).
    let app = routes::build(setup_state());
    for (method, path, _) in routes::PROTECTED_ROUTES {
        let (status, _, body) = call(
            app.clone(),
            Request::builder()
                .method(*method)
                .uri(*path)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}");
        assert_eq!(json(&body)["code"], "unauthorized", "{method} {path}");
    }
}
