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
        storage: std::sync::Arc::new(api_principal::storage::MockStorage::new()),
        storage_config: api_principal::storage::StorageConfig {
            bucket: "heph-test".into(),
            public_endpoint: None,
            url_ttl_secs: 60,
        },
        embedder: std::sync::Arc::new(api_principal::search::MockEmbedder::new()),
        embedding_model: "ViT-B-32".to_string(),
        manager: std::sync::Arc::new(api_principal::jobs::manager_client::MockManager::default()),
        model_download_allowed_hosts: vec![],
    }
}

async fn call(app: axum::Router, req: Request<Body>) -> (StatusCode, http::HeaderMap, Vec<u8>) {
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

/// OpenAPI `{param}` → axum 0.7 / matchit `:param` (comparamos paths, não estilos).
fn to_axum_path(p: &str) -> String {
    let mut out = String::with_capacity(p.len());
    for c in p.chars() {
        match c {
            '{' => out.push(':'),
            '}' => {}
            _ => out.push(c),
        }
    }
    out
}

/// Path de `PROTECTED_ROUTES` com `:id` → URI sondável (UUID nil).
fn probe_uri(path: &str) -> String {
    path.replace(":id", "00000000-0000-0000-0000-000000000000")
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
    // T9: a spec declara SOMENTE `{...}` — `:` na spec faria a normalização
    // virar no-op silencioso e o codegen quebraria.
    assert!(
        !paths
            .keys()
            .any(|p| p.as_str().expect("path string").contains(':')),
        "spec com `:` em path: a OpenAPI declara somente `{{...}}`"
    );
    for (path, item) in paths {
        let path = to_axum_path(path.as_str().expect("path string"));
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

    assert_eq!(
        code, spec,
        "deriva contrato vs router (D8): routes_all ≠ openapi.yaml"
    );

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
        BTreeSet::from(["status", "service", "auth", "version"]),
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

    // HealthResponse inclui `version` (ADR-0009 D5).
    assert!(
        health["properties"].get("version").is_some(),
        "HealthResponse missing version property"
    );

    // Schemas novos existem na spec 0.9.0 (ADR-0009).
    let schemas = yaml["components"]["schemas"]
        .as_mapping()
        .expect("components.schemas");
    for name in ["Orchestrator", "ModelWeight", "StorageUsage"] {
        assert!(
            schemas.get(name).is_some(),
            "missing schema {name} in openapi.yaml"
        );
    }

    // Telemetry inclui `ramTotal` (ADR-0009 D4).
    let telemetry = &yaml["components"]["schemas"]["Telemetry"];
    assert!(
        telemetry["properties"].get("ramTotal").is_some(),
        "Telemetry missing ramTotal property"
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
        let path = to_axum_path(path.as_str().expect("path string"));
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
    // Cinto: toda rota protegida sem cookie → 401 unauthorized (o loop
    // cobre as rotas protegidas declaradas em PROTECTED_ROUTES).
    let app = routes::build(setup_state());
    for (method, path, _) in routes::PROTECTED_ROUTES {
        let (status, _, body) = call(
            app.clone(),
            Request::builder()
                .method(*method)
                .uri(probe_uri(path))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}");
        assert_eq!(json(&body)["code"], "unauthorized", "{method} {path}");
    }
}

#[tokio::test]
async fn datasets_probe_without_db() {
    // Gate + handlers + envelope sem banco: o pool é `connect_lazy` e todos os
    // caminhos abaixo falham antes de qualquer query.
    let app = routes::build(setup_state());
    let (token, _) = session::issue_jwt(uuid::Uuid::new_v4(), &SETUP_SECRET);
    let cookie = format!("heph_session={token}");

    // 1. GET id não-UUID com cookie → 404 COM corpo (rota roteada; o 404 do
    // gate_fallback é sem corpo).
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/datasets/nao-e-uuid")
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&body)["code"], "not_found");

    // 2. POST body {} → 400 invalid_request.
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/datasets")
            .header(http::header::CONTENT_TYPE, "application/json")
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // 3. POST com chave desconhecida → 400 (input-trust, deny_unknown_fields).
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/datasets")
            .header(http::header::CONTENT_TYPE, "application/json")
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::from(
                r#"{"title":"x","type":"yolo_bbox","status":"ready"}"#,
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // 4. POST com type fora do enum → 400.
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/datasets")
            .header(http::header::CONTENT_TYPE, "application/json")
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::from(r#"{"title":"x","type":"tipo_inexistente"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // 5. GET sem cookie → 401 (gate ativo na rota, não só no fallback).
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/datasets")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json(&body)["code"], "unauthorized");
}

#[tokio::test]
async fn images_and_upload_reject_non_uuid_before_anything() {
    // 404-AR ANTES de ler fields/query: o parse do id é o passo 1 dos dois
    // handlers (multipart válido com field dummy; sem isto o extractor do
    // axum devolveria 400 de content-type antes do handler).
    let app = routes::build(setup_state());
    let (token, _) = session::issue_jwt(uuid::Uuid::new_v4(), &SETUP_SECRET);
    let cookie = format!("heph_session={token}");

    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/datasets/nao-e-uuid/images")
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&body)["code"], "not_found");

    let boundary = "heph-test-boundary";
    let multipart_body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"dummy.png\"\r\nContent-Type: image/png\r\n\r\nxxx\r\n--{boundary}--\r\n"
    );
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/datasets/nao-e-uuid/upload")
            .header(
                http::header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::from(multipart_body))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&body)["code"], "not_found");
}

#[tokio::test]
async fn detail_and_data_reject_non_uuid_before_anything() {
    // 404 ANTES de qualquer query/SQL: o parse de id+imageId é o passo 1
    // dos dois handlers (pool `connect_lazy` nunca é tocado — body vazio).
    let app = routes::build(setup_state());
    let (token, _) = session::issue_jwt(uuid::Uuid::new_v4(), &SETUP_SECRET);
    let cookie = format!("heph_session={token}");

    for uri in [
        "/api/datasets/nao-e-uuid/images/00000000-0000-0000-0000-000000000000".to_string(),
        format!("/api/datasets/00000000-0000-0000-0000-000000000000/images/nao-e-uuid"),
        "/api/datasets/nao-e-uuid/images/00000000-0000-0000-0000-000000000000/data".to_string(),
        format!("/api/datasets/00000000-0000-0000-0000-000000000000/images/nao-e-uuid/data"),
    ] {
        let (status, _, body) = call(
            app.clone(),
            Request::builder()
                .method("GET")
                .uri(uri.clone())
                .header(http::header::COOKIE, cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(json(&body)["code"], "not_found", "{uri}");
    }
}

#[tokio::test]
async fn put_boxes_and_caption_reject_without_db() {
    // Validades (c) testáveis sem DB: pool `connect_lazy` nunca é tocado
    // porque parse-uuid (a), parse de body (b) e validação pura (c) vêm
    // antes de qualquer query. Happy-path fica para datasets_db (precisa banco).
    let app = routes::build(setup_state());
    let (token, _) = session::issue_jwt(uuid::Uuid::new_v4(), &SETUP_SECRET);
    let cookie = format!("heph_session={token}");
    let ds = "00000000-0000-0000-0000-000000000000";
    let img = "11111111-1111-1111-1111-111111111111";
    let class = "22222222-2222-2222-2222-222222222222";

    // PUT boxes com x fora de 0..=1 ⇒ 400 (validação pura).
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("PUT")
            .uri(format!("/api/datasets/{ds}/images/{img}/boxes"))
            .header(http::header::CONTENT_TYPE, "application/json")
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::from(format!(
                r#"{{"boxes":[{{"classId":"{class}","x":1.5,"y":0,"w":0,"h":0}}]}}"#
            )))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // PUT boxes com chave desconhecida ⇒ 400 (deny_unknown_fields da casa).
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("PUT")
            .uri(format!("/api/datasets/{ds}/images/{img}/boxes"))
            .header(http::header::CONTENT_TYPE, "application/json")
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::from(r#"{"boxes":[],"extra":1}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // PUT caption com text vazio ⇒ 400 (a linha não nasce).
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("PUT")
            .uri(format!("/api/datasets/{ds}/images/{img}/caption"))
            .header(http::header::CONTENT_TYPE, "application/json")
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::from(r#"{"text":""}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // PUT caption sem text ⇒ 400 (required).
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("PUT")
            .uri(format!("/api/datasets/{ds}/images/{img}/caption"))
            .header(http::header::CONTENT_TYPE, "application/json")
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::from(r#"{"origin":"manual"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // imageId não-uuid + body válido ⇒ 404 ((a) antes de (b)).
    for uri in [
        format!("/api/datasets/{ds}/images/nao-e-uuid/boxes"),
        format!("/api/datasets/{ds}/images/nao-e-uuid/caption"),
    ] {
        let (status, _, body) = call(
            app.clone(),
            Request::builder()
                .method("PUT")
                .uri(uri.clone())
                .header(http::header::CONTENT_TYPE, "application/json")
                .header(http::header::COOKIE, cookie.clone())
                .body(Body::from(format!(
                    r#"{{"boxes":[{{"classId":"{class}","x":0.5,"y":0.5,"w":0.2,"h":0.2}}]}}"#
                )))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(json(&body)["code"], "not_found", "{uri}");
    }

    // Corpo > 2 MiB (DefaultBodyLimit global herdado das rotas JSON) ⇒ 413 no
    // envelope, nunca text-plain do axum (revisão 3b.6 F5; padrão create).
    let huge = format!(r#"{{"text":"{}"}}"#, "a".repeat(2 * 1024 * 1024 + 64));
    for uri in [
        format!("/api/datasets/{ds}/images/{img}/caption"),
        format!("/api/datasets/{ds}/images/{img}/boxes"),
    ] {
        let (status, _, body) = call(
            app.clone(),
            Request::builder()
                .method("PUT")
                .uri(uri.clone())
                .header(http::header::CONTENT_TYPE, "application/json")
                .header(http::header::COOKIE, cookie.clone())
                .body(Body::from(huge.clone()))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{uri}");
        assert_eq!(json(&body)["code"], "invalid_request", "{uri}");
    }
}

#[tokio::test]
async fn search_reject_without_db() {
    // Validação pura da 3f.5 ANTES de qualquer query (pool `connect_lazy`
    // nunca é tocado — mesmo padrão de `put_boxes_and_caption_reject_without_db`).
    let app = routes::build(setup_state());
    let (token, _) = session::issue_jwt(uuid::Uuid::new_v4(), &SETUP_SECRET);
    let cookie = format!("heph_session={token}");
    let ds = "00000000-0000-0000-0000-000000000000";

    // GET sem `q` ⇒ 400.
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/datasets/{ds}/search"))
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // `q` com 501 chars ⇒ 400; `q` vazio ⇒ 400.
    for q in ["a".repeat(501), String::new()] {
        let (status, _, body) = call(
            app.clone(),
            Request::builder()
                .method("GET")
                .uri(format!("/api/datasets/{ds}/search?q={q}"))
                .header(http::header::COOKIE, cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "q len");
        assert_eq!(json(&body)["code"], "invalid_request");
    }

    // `k=0` / `k=101` ⇒ 400.
    for k in ["0", "101"] {
        let (status, _, body) = call(
            app.clone(),
            Request::builder()
                .method("GET")
                .uri(format!("/api/datasets/{ds}/search?q=x&k={k}"))
                .header(http::header::COOKIE, cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "k={k}");
        assert_eq!(json(&body)["code"], "invalid_request");
    }

    // `classId=abc` ⇒ 400 (filtro opcional, não id de recurso).
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/datasets/{ds}/search?q=x&classId=abc"))
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // `split=test` ⇒ 400 (CHECK da 0003 só admite train|val).
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/datasets/{ds}/search?q=x&split=test"))
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // by-image com `imageId="abc"` ⇒ 404 (D8 replicado).
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/datasets/{ds}/search/by-image"))
            .header(http::header::CONTENT_TYPE, "application/json")
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::from(r#"{"imageId":"abc"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&body)["code"], "not_found");

    // by-image com `threshold=1.5` ⇒ 400.
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/datasets/{ds}/search/by-image"))
            .header(http::header::CONTENT_TYPE, "application/json")
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::from(
                r#"{"imageId":"00000000-0000-0000-0000-000000000000","threshold":1.5}"#,
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // dataset id não-UUID nas duas rotas ⇒ 404 (D8).
    for req in [
        Request::builder()
            .method("GET")
            .uri("/api/datasets/nao-e-uuid/search?q=x")
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::empty())
            .unwrap(),
        Request::builder()
            .method("POST")
            .uri("/api/datasets/nao-e-uuid/search/by-image")
            .header(http::header::CONTENT_TYPE, "application/json")
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::from(r#"{"imageId":"abc"}"#))
            .unwrap(),
    ] {
        let (status, _, body) = call(app.clone(), req).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json(&body)["code"], "not_found");
    }
}

#[test]
fn json_property_names_are_camel_case() {
    // Enforcement D1: todo nome de propriedade e de parâmetro na spec é
    // camelCase (`^[a-z][A-Za-z0-9]*$`). Walk recursivo no YAML inteiro:
    // cobre `properties` aninhados (`items.properties`, etc.) e `parameters`
    // (ex.: `limit`/`offset` da 3b), não só `components.schemas[*].properties`.
    fn is_camel(name: &str) -> bool {
        let mut chars = name.chars();
        chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_alphanumeric())
    }
    fn walk_camel_case(v: &serde_yaml::Value, ruim: &mut Vec<String>) {
        match v {
            serde_yaml::Value::Mapping(map) => {
                if let Some(props) = map.get("properties") {
                    if let Some(props) = props.as_mapping() {
                        for prop in props.keys() {
                            if let Some(name) = prop.as_str() {
                                if !is_camel(name) {
                                    ruim.push(name.to_string());
                                }
                            }
                        }
                    }
                }
                if let Some(params) = map.get("parameters") {
                    if let Some(params) = params.as_sequence() {
                        for p in params {
                            let Some(item) = p.as_mapping() else {
                                continue;
                            };
                            let Some(name) = item.get("name").and_then(|v| v.as_str()) else {
                                continue;
                            };
                            if !is_camel(name) {
                                ruim.push(name.to_string());
                            }
                        }
                    }
                }
                for (_, child) in map {
                    walk_camel_case(child, ruim);
                }
            }
            serde_yaml::Value::Sequence(seq) => {
                for child in seq {
                    walk_camel_case(child, ruim);
                }
            }
            _ => {}
        }
    }
    let text = std::fs::read_to_string(openapi_path()).expect("ler openapi.yaml");
    let yaml: serde_yaml::Value = serde_yaml::from_str(&text).expect("parse openapi.yaml");
    let mut ruim = Vec::new();
    walk_camel_case(&yaml, &mut ruim);
    assert!(ruim.is_empty(), "D1: nomes fora de camelCase: {ruim:?}");
}

#[tokio::test]
async fn job_response_keys_are_camel_case() {
    // Contract test: GET /api/jobs/:id and GET /api/jobs must NOT leak
    // snake_case keys (ADR-0007 D7 :370-371 — Job camelCase with
    // queuePosition, queueReason, createdAt, finishedAt).
    use api_principal::jobs::manager_client::{InternalJob, MockManager};

    let mock = MockManager::default();
    let mut state = setup_state();
    state.manager = std::sync::Arc::new({
        let mut m = mock;
        let job = InternalJob {
            id: "550e8400-e29b-41d4-a716-446655440000".into(),
            kind: "yolo_train".into(),
            engine: "yolo".into(),
            model: "yolo11m".into(),
            mode: "train".into(),
            dataset_id: Some("550e8400-e29b-41d4-a716-446655440001".into()),
            status: "running".into(),
            queue_reason: Some("waiting_vram".into()),
            queue_position: Some(2),
            progress: Some(0.5),
            epoch: Some(5),
            step: Some(100),
            metrics: None,
            vram_min_gb: Some(4),
            orchestrator_id: Some("550e8400-e29b-41d4-a716-446655440002".into()),
            orchestrator_name: Some("local-node".into()),
            orchestrator_kind: Some("local".into()),
            orchestrator_fallback: false,
            created_at: "2026-01-01T00:00:00Z".into(),
            finished_at: None,
            error: None,
        };
        m.get_job_result = Some(job.clone());
        m.list_jobs_result = Some((vec![job], 1));
        m
    });
    let app = routes::build(state);
    let (token, _) = session::issue_jwt(uuid::Uuid::new_v4(), &SETUP_SECRET);
    let cookie = format!("heph_session={token}");

    // GET /api/jobs/:id → 200 + camelCase keys.
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/jobs/550e8400-e29b-41d4-a716-446655440000")
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let job = json(&body);
    // Must NOT contain snake_case keys.
    for key in job.as_object().expect("job is object").keys() {
        assert!(
            !key.contains('_'),
            "GET /api/jobs/:id leaked snake_case key: {key}"
        );
    }
    // Must contain expected camelCase keys.
    assert!(job.get("queuePosition").is_some(), "missing queuePosition");
    assert!(job.get("queueReason").is_some(), "missing queueReason");
    assert!(job.get("createdAt").is_some(), "missing createdAt");
    assert!(job.get("datasetId").is_some(), "missing datasetId");
    assert!(job.get("vramMinGb").is_some(), "missing vramMinGb");
    assert!(
        job.get("orchestratorId").is_some(),
        "missing orchestratorId"
    );
    assert_eq!(job["orchestratorName"], "local-node");
    assert_eq!(job["orchestratorKind"], "local");
    assert_eq!(job["orchestratorFallback"], false);
    // Must NOT contain the snake_case equivalents.
    assert!(job.get("queue_position").is_none(), "leaked queue_position");
    assert!(job.get("queue_reason").is_none(), "leaked queue_reason");
    assert!(job.get("created_at").is_none(), "leaked created_at");
    assert!(job.get("dataset_id").is_none(), "leaked dataset_id");
    assert!(job.get("vram_min_gb").is_none(), "leaked vram_min_gb");
    assert!(
        job.get("orchestrator_id").is_none(),
        "leaked orchestrator_id"
    );
    assert!(
        job.get("orchestrator_name").is_none(),
        "leaked orchestrator_name"
    );
    assert!(
        job.get("orchestrator_kind").is_none(),
        "leaked orchestrator_kind"
    );
    assert!(
        job.get("orchestrator_fallback").is_none(),
        "leaked orchestrator_fallback"
    );

    // GET /api/jobs → 200 + items with camelCase keys.
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/jobs")
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let list = json(&body);
    let items = list["items"].as_array().expect("items array");
    assert!(!items.is_empty(), "expected at least one job in list");
    for (i, item) in items.iter().enumerate() {
        for key in item.as_object().expect("item is object").keys() {
            assert!(
                !key.contains('_'),
                "GET /api/jobs items[{i}] leaked snake_case key: {key}"
            );
        }
        assert!(
            item.get("queuePosition").is_some(),
            "items[{i}] missing queuePosition"
        );
        assert!(
            item.get("createdAt").is_some(),
            "items[{i}] missing createdAt"
        );
        assert_eq!(item["orchestratorName"], "local-node");
        assert_eq!(item["orchestratorKind"], "local");
        assert_eq!(item["orchestratorFallback"], false);
    }
}

#[test]
fn dataset_response_keys_match_openapi() {
    // O inventário só cruza statuses; este teste trava o SHAPE serializado:
    // o set de chaves que `DatasetResponse` produz tem que ser o declarado em
    // `components.schemas.Dataset` (pega drift D1, ex.: renomear
    // `last_modified` sem atualizar a spec).
    use api_principal::datasets::models::{DatasetClassResponse, DatasetResponse, DatasetRow};
    use chrono::{DateTime, Utc};
    let row = DatasetRow {
        id: uuid::Uuid::nil(),
        slug: "s".to_string(),
        title: "t".to_string(),
        category: "yolo".to_string(),
        r#type: "yolo_bbox".to_string(),
        task: "detect_track".to_string(),
        format: "yolo_txt".to_string(),
        status: "needs_labeling".to_string(),
        size_bytes: 0,
        images_count: 0,
        labeled_count: 0,
        created_at: DateTime::<Utc>::UNIX_EPOCH,
        updated_at: DateTime::<Utc>::UNIX_EPOCH,
        auto_tracked: false,
        trash_count: 0,
    };
    let mut resp = DatasetResponse::from(row);
    resp.classes = vec![DatasetClassResponse {
        id: uuid::Uuid::nil().to_string(),
        name: "a".to_string(),
        idx: 0,
        color: "#10b981".to_string(),
    }];
    let value = serde_json::to_value(&resp).expect("serializar DatasetResponse");
    let got: BTreeSet<String> = value
        .as_object()
        .expect("DatasetResponse serializa como objeto")
        .keys()
        .cloned()
        .collect();
    let text = std::fs::read_to_string(openapi_path()).expect("ler openapi.yaml");
    let yaml: serde_yaml::Value = serde_yaml::from_str(&text).expect("parse openapi.yaml");
    let props = yaml["components"]["schemas"]["Dataset"]["properties"]
        .as_mapping()
        .expect("Dataset.properties");
    let spec: BTreeSet<String> = props
        .keys()
        .map(|k| k.as_str().expect("property string").to_string())
        .collect();
    let required: BTreeSet<String> = yaml["components"]["schemas"]["Dataset"]["required"]
        .as_sequence()
        .expect("Dataset.required")
        .iter()
        .map(|v| v.as_str().expect("required string").to_string())
        .collect();
    assert_eq!(
        got, spec,
        "chaves serializadas ≠ Dataset.properties: {got:?} vs {spec:?}"
    );
    assert!(
        required.is_subset(&got),
        "required fora do objeto serializado: {required:?} vs {got:?}"
    );
}

#[tokio::test]
async fn orchestrator_response_keys_are_camel_case() {
    // Contract test: GET /api/orchestrators must return camelCase keys.
    use api_principal::jobs::manager_client::{InternalOrchestrator, MockManager};

    let mock = MockManager::default();
    let mut state = setup_state();
    state.manager = std::sync::Arc::new({
        let mut m = mock;
        m.list_orchestrators_result = Some(vec![InternalOrchestrator {
            id: "550e8400-e29b-41d4-a716-446655440000".into(),
            name: "orchestrator-local".into(),
            kind: "local".into(),
            endpoint: "http://orchestrator-local:8082".into(),
            status: "online".into(),
            last_heartbeat: Some("2026-09-09T12:00:00Z".into()),
            measured: true,
            cpu: Some(42.5),
            ram: Some(4096),
            ram_total: Some(8192),
            vram_used: Some(3072),
            vram_total: Some(6144),
            vram_total_gb: Some(6),
            gpus: vec!["NVIDIA GeForce GTX 1660 SUPER".into()],
            jobs_active: 1,
        }]);
        m
    });
    let app = routes::build(state);
    let (token, _) = session::issue_jwt(uuid::Uuid::new_v4(), &SETUP_SECRET);
    let cookie = format!("heph_session={token}");

    let (status, _, body) = call(
        app,
        Request::builder()
            .method("GET")
            .uri("/api/orchestrators")
            .header(http::header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let json = json(&body);
    let items = json["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    let item = &items[0];
    // Must have camelCase lastHeartbeat, not snake_case.
    assert!(item.get("lastHeartbeat").is_some(), "missing lastHeartbeat");
    assert!(
        item.get("last_heartbeat").is_none(),
        "leaked snake_case last_heartbeat"
    );
}

#[tokio::test]
async fn model_response_keys_are_camel_case() {
    // Contract test: GET /api/models must return camelCase keys (D6 ADR-0012).
    use api_principal::jobs::manager_client::{InternalModel, MockManager};

    let mock = MockManager::default();
    let mut state = setup_state();
    state.manager = std::sync::Arc::new({
        let mut m = mock;
        m.list_models_result = Some(vec![InternalModel {
            id: "550e8400-e29b-41d4-a716-446655440003".into(),
            name: "best.pt".into(),
            engine: "yolo".into(),
            model: Some("yolo11m".into()),
            source: "train".into(),
            md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
            bytes: 110,
            path: "artifacts/550e8400-e29b-41d4-a716-446655440004/best.pt".into(),
            job_id: Some("550e8400-e29b-41d4-a716-446655440004".into()),
            created_at: "2026-09-09T12:00:00Z".into(),
        }]);
        m
    });
    let app = routes::build(state);
    let (token, _) = session::issue_jwt(uuid::Uuid::new_v4(), &SETUP_SECRET);
    let cookie = format!("heph_session={token}");

    let (status, _, body) = call(
        app,
        Request::builder()
            .method("GET")
            .uri("/api/models")
            .header(http::header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let json = json(&body);
    let items = json["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    let item = &items[0];
    // Must have camelCase keys.
    assert!(item.get("jobId").is_some(), "missing jobId");
    assert!(item.get("createdAt").is_some(), "missing createdAt");
    assert!(item.get("job_id").is_none(), "leaked snake_case job_id");
    assert!(
        item.get("created_at").is_none(),
        "leaked snake_case created_at"
    );
    // D6 novos campos.
    assert_eq!(item["source"], "train");
    assert!(item.get("md5").is_some(), "missing md5");
    assert!(item.get("model").is_some(), "missing model");
    assert!(item.get("url").is_some(), "missing url key");
    // name = basename of path.
    assert_eq!(item["name"], "best.pt");
}

#[tokio::test]
async fn submit_jobs_reject_invalid_orchestrator_id() {
    let app = routes::build(setup_state());
    let (token, _) = session::issue_jwt(uuid::Uuid::new_v4(), &SETUP_SECRET);
    let cookie = format!("heph_session={token}");
    let valid_uuid = "550e8400-e29b-41d4-a716-446655440000";

    // 1. POST /api/jobs/yolo com orchestratorId inválido ⇒ 400 invalid_request.
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/jobs/yolo")
            .header(http::header::CONTENT_TYPE, "application/json")
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::from(format!(
                r#"{{"datasetId":"{valid_uuid}","model":"yolo11n","epochs":10,"batch":16,"orchestratorId":"nao-eh-uuid"}}"#
            )))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // 2. POST /api/jobs/autotracker com orchestratorId inválido ⇒ 400 invalid_request.
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/jobs/autotracker")
            .header(http::header::CONTENT_TYPE, "application/json")
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::from(format!(
                r#"{{"datasetId":"{valid_uuid}","confidence":0.5,"orchestratorId":"nao-eh-uuid"}}"#
            )))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // 3. POST /api/jobs/predict com orchestratorId inválido ⇒ 400 invalid_request.
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/jobs/predict")
            .header(http::header::CONTENT_TYPE, "application/json")
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::from(format!(
                r#"{{"datasetId":"{valid_uuid}","orchestratorId":"nao-eh-uuid"}}"#
            )))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // 4. POST /api/jobs/autolabel com orchestratorId inválido ⇒ 400 invalid_request.
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/jobs/autolabel")
            .header(http::header::CONTENT_TYPE, "application/json")
            .header(http::header::COOKIE, cookie.clone())
            .body(Body::from(format!(
                r#"{{"datasetId":"{valid_uuid}","orchestratorId":"nao-eh-uuid"}}"#
            )))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");
}
