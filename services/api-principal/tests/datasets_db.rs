//! Integração datasets × Postgres (Fatia 3a) — router+gate+SQL juntos.
//!
//! Execução: `DATABASE_URL=postgres://studio:studio@localhost:5432/studio cargo test -p api-principal --test datasets_db -- --ignored`
//!
//! AVISO EM MAIÚSCULAS: ESTE TESTE É PARA BANCO DE DESENVOLVIMENTO. ELE APAGA
//! `classes` E `datasets` NO SETUP — `DATABASE_URL` NUNCA DEVE APONTAR PARA
//! BANCO COM DADOS REAIS.

use api_principal::auth::{routes, session, AppState};
use axum::body::Body;
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

const TEST_SECRET: [u8; 32] = [0x42; 32];

// Os 7 testes partilham UM banco (setup com DELETE) e o harness roda em
// paralelo — sem isto eles se apagam uns aos outros. O lock serializa os
// corpos sem mudar o comando de execução (`--test-threads=1` quebraria a
// forma canônica `... -- --ignored` do doc acima).
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn state() -> AppState {
    // Runtime (nunca `env!` de compilação) e conexão real (nunca `connect_lazy`).
    let url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL é obrigatório para este teste --ignored");
    let pool = sqlx::PgPool::connect(&url)
        .await
        .expect("conectar no Postgres de desenvolvimento");
    // Idempotente: aplica 0001 + 0002.
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("rodar migrations");
    // Limpeza explícita (sem TRUNCATE: não toca `users`/`auth_state`;
    // a lista cresce na 3b com `images` e filhas).
    sqlx::query("DELETE FROM classes")
        .execute(&pool)
        .await
        .expect("limpar classes");
    sqlx::query("DELETE FROM datasets")
        .execute(&pool)
        .await
        .expect("limpar datasets");
    AppState {
        pool,
        jwt_secret: TEST_SECRET,
        secure_cookie: false,
        setup_required: false,
    }
}

fn authed_cookie() -> String {
    let (token, _) = session::issue_jwt(uuid::Uuid::new_v4(), &TEST_SECRET);
    format!("heph_session={token}")
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
    serde_json::from_slice(body).expect("corpo JSON")
}

fn post_create(title: &str, classes: &serde_json::Value, cookie: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/datasets")
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::COOKIE, cookie)
        .body(Body::from(
            serde_json::json!({"title": title, "type": "yolo_bbox", "classes": classes})
                .to_string(),
        ))
        .unwrap()
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_read_delete_flow() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    // Sanidade: o teste só vale conectado ao banco certo.
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&st.pool)
        .await
        .expect("current_database");
    assert_eq!(db, "studio");
    let app = routes::build(st.clone());
    let cookie = authed_cookie();

    let (status, _, body) = call(
        app.clone(),
        post_create(
            "Inspeção PCB (Defeitos) v2",
            &serde_json::json!(["solda_fria", "curto_circuito"]),
            &cookie,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let created = json(&body);
    assert_eq!(created["slug"], "inspecao-pcb-defeitos-v2");
    assert_eq!(created["category"], "yolo");
    assert_eq!(created["type"], "yolo_bbox");
    assert_eq!(created["task"], "detect_track");
    assert_eq!(created["format"], "yolo_txt");
    assert_eq!(created["status"], "needs_labeling");
    assert!(created["source"].is_null());
    assert_eq!(created["sizeBytes"], 0);
    assert_eq!(created["imagesCount"], 0);
    assert_eq!(created["labeledCount"], 0);
    assert_eq!(created["classes"], serde_json::json!(["solda_fria", "curto_circuito"]));
    assert_eq!(created["autoTracked"], false);
    assert!(created["createdAt"].is_string());
    assert!(created["lastModified"].is_string());
    for snake in [
        "size_bytes",
        "images_count",
        "labeled_count",
        "auto_tracked",
        "last_modified",
        "created_at",
    ] {
        assert!(created.get(snake).is_none(), "chave snake_case: {snake}");
    }
    let id = created["id"].as_str().expect("id string").to_string();

    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/datasets")
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let list = json(&body);
    assert_eq!(list.as_array().expect("array").len(), 1);
    assert_eq!(list[0]["classes"], created["classes"]);

    let (status, _, _) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/datasets/{id}"))
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/datasets/{id}"))
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty());

    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/datasets/{id}"))
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&body)["code"], "not_found");

    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/datasets")
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body), serde_json::json!([]));
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn duplicate_slug_conflicts() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();
    let classes = serde_json::json!(["a"]);

    let (status, _, _) = call(
        app.clone(),
        post_create("Mesmo Nome", &classes, &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = call(
        app.clone(),
        post_create("Mesmo Nome", &classes, &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(json(&body)["code"], "slug_conflict");

    // O segundo POST não deixou linha pela metade (ON CONFLICT + transação).
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/datasets")
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body).as_array().expect("array").len(), 1);
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn classes_idx_and_colors() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();

    let (status, _, body) = call(
        app.clone(),
        post_create(
            "Cores",
            &serde_json::json!(["gato", "cao", "passaro"]),
            &cookie,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id: uuid::Uuid = json(&body)["id"]
        .as_str()
        .expect("id")
        .parse()
        .expect("uuid");

    let rows: Vec<(String, i32, String)> = sqlx::query_as(
        "SELECT name, idx, color FROM classes WHERE dataset_id = $1 ORDER BY idx",
    )
    .bind(id)
    .fetch_all(&st.pool)
    .await
    .expect("classes do banco");
    assert_eq!(
        rows,
        vec![
            ("gato".to_string(), 0, "#10b981".to_string()),
            ("cao".to_string(), 1, "#f59e0b".to_string()),
            ("passaro".to_string(), 2, "#f43f5e".to_string()),
        ]
    );

    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/datasets")
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json(&body)[0]["classes"],
        serde_json::json!(["gato", "cao", "passaro"])
    );
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn delete_cascades_classes() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();

    let (status, _, body) = call(
        app.clone(),
        post_create("Cascata", &serde_json::json!(["x", "y"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id: uuid::Uuid = json(&body)["id"]
        .as_str()
        .expect("id")
        .parse()
        .expect("uuid");

    let (status, _, _) = call(
        app.clone(),
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/datasets/{id}"))
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM classes WHERE dataset_id = $1")
        .bind(id)
        .fetch_one(&st.pool)
        .await
        .expect("count classes");
    assert_eq!(n, 0);
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn updated_at_trigger_bumps() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();

    let (status, _, body) = call(
        app.clone(),
        post_create("Trigger", &serde_json::json!([]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id: uuid::Uuid = json(&body)["id"]
        .as_str()
        .expect("id")
        .parse()
        .expect("uuid");

    let (created_before, updated_before): (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) =
        sqlx::query_as("SELECT created_at, updated_at FROM datasets WHERE id = $1")
            .bind(id)
            .fetch_one(&st.pool)
            .await
            .expect("timestamps");
    sqlx::query("SELECT pg_sleep(0.05)")
        .execute(&st.pool)
        .await
        .expect("pg_sleep");
    sqlx::query("UPDATE datasets SET title = title WHERE id = $1")
        .bind(id)
        .execute(&st.pool)
        .await
        .expect("update");
    let (created_after, updated_after): (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) =
        sqlx::query_as("SELECT created_at, updated_at FROM datasets WHERE id = $1")
            .bind(id)
            .fetch_one(&st.pool)
            .await
            .expect("timestamps");

    assert!(updated_after > updated_before, "trigger não bumpou updated_at");
    assert_eq!(created_after, created_before, "trigger mexeu em created_at");
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn rejects_unknown_and_bad_body() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();

    // Chave desconhecida (input-trust).
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/datasets")
            .header(http::header::CONTENT_TYPE, "application/json")
            .header(http::header::COOKIE, &cookie)
            .body(Body::from(r#"{"title":"x","type":"yolo_bbox","imagesCount":9}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // Type fora do enum.
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/datasets")
            .header(http::header::CONTENT_TYPE, "application/json")
            .header(http::header::COOKIE, &cookie)
            .body(Body::from(r#"{"title":"x","type":"tipo_inexistente"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // 201 classes (cap de input).
    let many: Vec<String> = (0..201).map(|i| format!("c{i}")).collect();
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/datasets")
            .header(http::header::CONTENT_TYPE, "application/json")
            .header(http::header::COOKIE, &cookie)
            .body(Body::from(
                serde_json::json!({"title": "muitas", "type": "yolo_bbox", "classes": many})
                    .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // Id não-UUID é 404, não 400.
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/datasets/nao-e-uuid")
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&body)["code"], "not_found");

    // Nada disso escreveu.
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM datasets")
        .fetch_one(&st.pool)
        .await
        .expect("count datasets");
    assert_eq!(n, 0);
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn unauthenticated_is_401_even_with_db() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st);
    for (method, uri) in [
        ("GET", "/api/datasets"),
        ("POST", "/api/datasets"),
        (
            "GET",
            "/api/datasets/00000000-0000-0000-0000-000000000000",
        ),
        (
            "DELETE",
            "/api/datasets/00000000-0000-0000-0000-000000000000",
        ),
    ] {
        let (status, _, body) = call(
            app.clone(),
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
        assert_eq!(json(&body)["code"], "unauthorized", "{method} {uri}");
    }
}
