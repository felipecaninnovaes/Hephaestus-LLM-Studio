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
        storage: std::sync::Arc::new(api_principal::storage::MockStorage::new()),
        storage_config: api_principal::storage::StorageConfig {
            bucket: "heph-test".into(),
            public_endpoint: None,
            url_ttl_secs: 60,
        },
    }
}

fn authed_cookie() -> String {
    let (token, _) = session::issue_jwt(uuid::Uuid::new_v4(), &TEST_SECRET);
    format!("heph_session={token}")
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
    let class_names: Vec<&str> = created["classes"]
        .as_array()
        .expect("classes array")
        .iter()
        .map(|c| c["name"].as_str().expect("class name"))
        .collect();
    assert_eq!(class_names, vec!["solda_fria", "curto_circuito"]);
    assert_eq!(created["classes"][0]["idx"], 0);
    assert_eq!(created["classes"][0]["color"], "#10b981");
    assert!(created["classes"][0]["id"].is_string());
    assert_eq!(created["autoTracked"], false);
    assert!(created["createdAt"].is_string());
    assert!(created["lastModified"].is_string());
    for snake in [
        "size_bytes",
        "images_count",
        "labeled_count",
        "auto_tracked",
        "trash_count",
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

    let (status, _, _) = call(app.clone(), post_create("Mesmo Nome", &classes, &cookie)).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = call(app.clone(), post_create("Mesmo Nome", &classes, &cookie)).await;
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

    let rows: Vec<(String, i32, String)> =
        sqlx::query_as("SELECT name, idx, color FROM classes WHERE dataset_id = $1 ORDER BY idx")
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
    let list = json(&body);
    let names: Vec<&str> = list[0]["classes"]
        .as_array()
        .expect("classes array")
        .iter()
        .map(|c| c["name"].as_str().expect("class name"))
        .collect();
    assert_eq!(names, vec!["gato", "cao", "passaro"]);
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

    let (created_before, updated_before): (
        chrono::DateTime<chrono::Utc>,
        chrono::DateTime<chrono::Utc>,
    ) = sqlx::query_as("SELECT created_at, updated_at FROM datasets WHERE id = $1")
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
    let (created_after, updated_after): (
        chrono::DateTime<chrono::Utc>,
        chrono::DateTime<chrono::Utc>,
    ) = sqlx::query_as("SELECT created_at, updated_at FROM datasets WHERE id = $1")
        .bind(id)
        .fetch_one(&st.pool)
        .await
        .expect("timestamps");

    assert!(
        updated_after > updated_before,
        "trigger não bumpou updated_at"
    );
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
            .body(Body::from(
                r#"{"title":"x","type":"yolo_bbox","imagesCount":9}"#,
            ))
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

async fn counters_of(pool: &sqlx::PgPool, id: uuid::Uuid) -> (i32, i32, i64, String) {
    sqlx::query_as::<_, (i32, i32, i64, String)>(
        "SELECT images_count, labeled_count, size_bytes, status FROM datasets WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("contadores do dataset")
}

async fn insert_image(
    pool: &sqlx::PgPool,
    dataset_id: uuid::Uuid,
    filename: &str,
    bytes: i64,
) -> uuid::Uuid {
    let img = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO images (id, dataset_id, filename, object_key, bytes, width, height, md5, sha256, media_type) \
         VALUES ($1,$2,$3,$4,$5,640,480,'d41d8cd98f00b204e9800998ecf8427e','e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855','jpeg')",
    )
    .bind(img)
    .bind(dataset_id)
    .bind(filename)
    .bind(format!("datasets/{dataset_id}/images/{img}/{filename}"))
    .bind(bytes)
    .execute(pool)
    .await
    .expect("insert image");
    img
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_coluna_source_drops() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.columns WHERE table_name = 'datasets' AND column_name = 'source'",
    )
    .fetch_one(&st.pool)
    .await
    .expect("information_schema");
    assert_eq!(n, 0, "datasets.source ainda existe");
    let t: String = sqlx::query_scalar("SELECT to_regclass('public.videos')::text")
        .fetch_one(&st.pool)
        .await
        .expect("to_regclass videos");
    assert_eq!(t, "videos");
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_gatilho_contadores_status_yolo() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();
    let (status, _, body) = call(
        app.clone(),
        post_create("Yolo Trig", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ds: uuid::Uuid = json(&body)["id"]
        .as_str()
        .expect("id")
        .parse()
        .expect("uuid");
    let class_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM classes WHERE dataset_id = $1 LIMIT 1")
            .bind(ds)
            .fetch_one(&st.pool)
            .await
            .expect("class id");

    let img1 = insert_image(&st.pool, ds, "a.jpg", 100).await;
    assert_eq!(
        counters_of(&st.pool, ds).await,
        (1, 0, 100, "needs_labeling".to_string())
    );

    sqlx::query(
        "INSERT INTO boxes (image_id, class_id, x, y, w, h, origin) VALUES ($1,$2,0.5,0.5,0.2,0.2,'manual')",
    )
    .bind(img1)
    .bind(class_id)
    .execute(&st.pool)
    .await
    .expect("insert box");
    assert_eq!(
        counters_of(&st.pool, ds).await,
        (1, 1, 100, "ready".to_string())
    );

    let _img2 = insert_image(&st.pool, ds, "b.jpg", 50).await;
    assert_eq!(
        counters_of(&st.pool, ds).await,
        (2, 1, 150, "in_progress".to_string())
    );

    // Cenário exato da T2: delete da imagem rotulada não pode errar nem
    // deixar estado intermediário persistido.
    sqlx::query("DELETE FROM images WHERE id = $1")
        .bind(img1)
        .execute(&st.pool)
        .await
        .expect("delete imagem rotulada");
    assert_eq!(
        counters_of(&st.pool, ds).await,
        (1, 0, 50, "needs_labeling".to_string())
    );
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_gatilho_caption_format_captions() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    // format importa: captions só rotula dataset de format captions (R9).
    let ds = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO datasets (id, slug, title, category, type, task, format, status) \
         VALUES ($1,'cap-trig','Cap Trig','difusao','difusao_lora','caption','captions','needs_labeling')",
    )
    .bind(ds)
    .execute(&st.pool)
    .await
    .expect("insert dataset captions");
    let class_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO classes (id, dataset_id, name, idx, color) VALUES ($1,$2,'a',0,'#10b981')",
    )
    .bind(class_id)
    .bind(ds)
    .execute(&st.pool)
    .await
    .expect("insert class");

    let img = insert_image(&st.pool, ds, "c.jpg", 80).await;
    assert_eq!(
        counters_of(&st.pool, ds).await,
        (1, 0, 80, "needs_labeling".to_string())
    );

    sqlx::query("INSERT INTO captions (image_id, text, origin) VALUES ($1,'um gato','manual')")
        .bind(img)
        .execute(&st.pool)
        .await
        .expect("insert caption");
    assert_eq!(
        counters_of(&st.pool, ds).await,
        (1, 1, 80, "ready".to_string())
    );

    // Box NÃO conta para format captions (R9 espelhado).
    sqlx::query(
        "INSERT INTO boxes (image_id, class_id, x, y, w, h, origin) VALUES ($1,$2,0.5,0.5,0.2,0.2,'manual')",
    )
    .bind(img)
    .bind(class_id)
    .execute(&st.pool)
    .await
    .expect("insert box");
    assert_eq!(
        counters_of(&st.pool, ds).await,
        (1, 1, 80, "ready".to_string())
    );

    // CHECK: caption com text vazio é erro; imagem segue rotulada pela anterior.
    let img2 = insert_image(&st.pool, ds, "d.jpg", 10).await;
    let bad = sqlx::query("INSERT INTO captions (image_id, text, origin) VALUES ($1,'','manual')")
        .bind(img2)
        .execute(&st.pool)
        .await;
    assert!(bad.is_err(), "caption vazio deveria violar o CHECK");
    assert_eq!(
        counters_of(&st.pool, ds).await,
        (2, 1, 90, "in_progress".to_string())
    );
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_delete_dataset_cascade() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let ds = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO datasets (id, slug, title, category, type, task, format, status) \
         VALUES ($1,'cascata-3b','Cascata 3b','difusao','difusao_lora','caption','captions','needs_labeling')",
    )
    .bind(ds)
    .execute(&st.pool)
    .await
    .expect("insert dataset");
    let class_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO classes (id, dataset_id, name, idx, color) VALUES ($1,$2,'a',0,'#10b981')",
    )
    .bind(class_id)
    .bind(ds)
    .execute(&st.pool)
    .await
    .expect("insert class");
    let img = insert_image(&st.pool, ds, "e.jpg", 70).await;
    sqlx::query("INSERT INTO captions (image_id, text, origin) VALUES ($1,'legenda','manual')")
        .bind(img)
        .execute(&st.pool)
        .await
        .expect("insert caption");
    sqlx::query(
        "INSERT INTO boxes (image_id, class_id, x, y, w, h, origin) VALUES ($1,$2,0.5,0.5,0.2,0.2,'manual')",
    )
    .bind(img)
    .bind(class_id)
    .execute(&st.pool)
    .await
    .expect("insert box");
    let vid = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO videos (dataset_id, filename, object_key, md5, bytes) \
         VALUES ($1,'v.mp4',$2,'d41d8cd98f00b204e9800998ecf8427e',33)",
    )
    .bind(ds)
    .bind(format!("datasets/{ds}/videos/{vid}/v.mp4"))
    .execute(&st.pool)
    .await
    .expect("insert video");

    // updated_at NÃO muda quando um refresh não altera nada: duas chamadas
    // diretas seguidas, a segunda não bumba updated_at.
    sqlx::query("SELECT heph_refresh_dataset_counters($1)")
        .bind(ds)
        .execute(&st.pool)
        .await
        .expect("refresh 1");
    let updated1: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT updated_at FROM datasets WHERE id = $1")
            .bind(ds)
            .fetch_one(&st.pool)
            .await
            .expect("updated_at 1");
    sqlx::query("SELECT pg_sleep(0.05)")
        .execute(&st.pool)
        .await
        .expect("pg_sleep");
    sqlx::query("SELECT heph_refresh_dataset_counters($1)")
        .bind(ds)
        .execute(&st.pool)
        .await
        .expect("refresh 2");
    let updated2: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT updated_at FROM datasets WHERE id = $1")
            .bind(ds)
            .fetch_one(&st.pool)
            .await
            .expect("updated_at 2");
    assert_eq!(updated1, updated2, "refresh sem mudança bumpou updated_at");

    sqlx::query("DELETE FROM datasets WHERE id = $1")
        .bind(ds)
        .execute(&st.pool)
        .await
        .expect("delete dataset");
    // As linhas-filhas morrem com o pai: contar via subselect que sobrevive.
    let ni: i64 = sqlx::query_scalar("SELECT count(*) FROM images WHERE dataset_id = $1")
        .bind(ds)
        .fetch_one(&st.pool)
        .await
        .expect("count images");
    let nb: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM boxes b JOIN images i ON i.id = b.image_id WHERE i.dataset_id = $1",
    )
    .bind(ds)
    .fetch_one(&st.pool)
    .await
    .expect("count boxes");
    let nc: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM captions c JOIN images i ON i.id = c.image_id WHERE i.dataset_id = $1",
    )
    .bind(ds)
    .fetch_one(&st.pool)
    .await
    .expect("count captions");
    let nv: i64 = sqlx::query_scalar("SELECT count(*) FROM videos WHERE dataset_id = $1")
        .bind(ds)
        .fetch_one(&st.pool)
        .await
        .expect("count videos");
    assert_eq!((ni, nb, nc, nv), (0, 0, 0, 0));
}

fn multipart_body(boundary: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, data) in files {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

fn post_upload(cookie: &str, dataset_id: &str, boundary: &str, body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/api/datasets/{dataset_id}/upload"))
        .header(
            http::header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header(http::header::COOKIE, cookie)
        .body(Body::from(body))
        .unwrap()
}

/// JPEG 1×1 gerado em tempo de teste (encode via `image`, sempre válido).
fn jpeg_1x1() -> Vec<u8> {
    let img = image::RgbImage::from_pixel(1, 1, image::Rgb([255, 0, 0]));
    let mut buf = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new(&mut buf);
    enc.encode(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgb8,
    )
    .expect("encode jpeg 1x1");
    assert!(buf.len() >= 3 && buf[0] == 0xFF && buf[1] == 0xD8 && buf[2] == 0xFF);
    buf
}

/// PNG 1×1 (70 bytes reais do `base64 -d` da constante da spec —
/// a spec diz 67, mas a string decodifica para 70; vale o real).
fn png_1x1() -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==")
        .expect("png 1x1")
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_upload_stored_duplicate_rejected() {
    let _guard = SERIAL.lock().await;
    let mut st = state().await;
    // Mock explícito (AppState é do teste).
    let mock = std::sync::Arc::new(api_principal::storage::MockStorage::new());
    st.storage = mock.clone();
    let app = routes::build(st.clone());
    let cookie = authed_cookie();
    let png = png_1x1();
    assert_eq!(png.len(), 70);

    // Dataset yolo_txt via API.
    let (status, _, body) = call(
        app.clone(),
        post_create("Upload Rt", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ds = json(&body)["id"].as_str().expect("id").to_string();

    // Lote com 2 fields de MESMO filename+conteúdo (reenvio no lote):
    // 1º stored, 2º duplicate com o MESMO imageId.
    let boundary = "heph-upload-boundary";
    let (status, _, body) = call(
        app.clone(),
        post_upload(
            &cookie,
            &ds,
            boundary,
            multipart_body(boundary, &[("a.png", &png), ("a.png", &png)]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = json(&body)["items"].clone();
    assert_eq!(items.as_array().expect("items").len(), 2);
    assert_eq!(items[0]["status"], "stored");
    assert_eq!(items[0]["filename"], "a.png");
    assert_eq!(items[1]["status"], "duplicate");
    assert_eq!(items[1]["reason"], "duplicate_filename");
    assert_eq!(items[1]["imageId"], items[0]["imageId"]);
    assert_eq!(items[1]["filename"], "a.png");
    assert_eq!(items[0]["bytes"], 70);
    assert_eq!(items[0]["width"], 1);
    assert_eq!(items[0]["height"], 1);
    let image_id: uuid::Uuid = items[0]["imageId"]
        .as_str()
        .expect("imageId")
        .parse()
        .expect("uuid");

    // Compensação D7: PUT do duplicate seguido de DELETE da MESMA key.
    let ops = mock.ops();
    let dup_pair = ops.windows(2).any(|w| {
        w[0].starts_with("PUT ")
            && w[1] == w[0].replacen("PUT ", "DELETE ", 1)
            && w[0].ends_with("/a.png")
    });
    assert!(dup_pair, "PUT+DELETE da mesma key ausente: {ops:?}");

    // Gatilho: 1 imagem, 0 rotuladas, 70 bytes, needs_labeling.
    let ds_id: uuid::Uuid = ds.parse().expect("uuid");
    assert_eq!(
        counters_of(&st.pool, ds_id).await,
        (1, 0, 70, "needs_labeling".to_string())
    );

    // Rejeitados NÃO tocam contadores: dataset separado para não poluir o
    // principal (lote misto ⇒ 200 com item rejected; lote só-rejeitado ⇒ 400).
    let (status, _, body) = call(
        app.clone(),
        post_create("Upload Rej", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ds2 = json(&body)["id"].as_str().expect("id").to_string();
    let (status, _, body) = call(
        app.clone(),
        post_upload(
            &cookie,
            &ds2,
            boundary,
            multipart_body(boundary, &[("d.png", &png), ("x.png", b"GIF89a-nao")]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = json(&body)["items"].clone();
    assert_eq!(items.as_array().expect("items").len(), 2);
    assert_eq!(items[0]["status"], "stored");
    assert_eq!(items[1]["status"], "rejected");
    assert_eq!(items[1]["reason"], "unsupported_media");
    assert!(items[1]["imageId"].is_null());

    let (status, _, body) = call(
        app.clone(),
        post_upload(
            &cookie,
            &ds2,
            boundary,
            multipart_body(boundary, &[("x.png", b"GIF89a-nao")]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // F4 end-to-end: conteúdo JPEG com nome `.png` ⇒ stored com canônico `.jpg`.
    let jpeg = jpeg_1x1();
    let (status, _, body) = call(
        app.clone(),
        post_upload(
            &cookie,
            &ds2,
            boundary,
            multipart_body(boundary, &[("foto.png", &jpeg)]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = json(&body)["items"].clone();
    assert_eq!(items.as_array().expect("items").len(), 1);
    assert_eq!(items[0]["status"], "stored");
    assert_eq!(items[0]["filename"], "foto.jpg");

    // Box na imagem ⇒ gatilho end-to-end pela rota de verdade.
    let class_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM classes WHERE dataset_id = $1 LIMIT 1")
            .bind(ds_id)
            .fetch_one(&st.pool)
            .await
            .expect("class id");
    sqlx::query(
        "INSERT INTO boxes (image_id, class_id, x, y, w, h, origin) VALUES ($1,$2,0.5,0.5,0.2,0.2,'manual')",
    )
    .bind(image_id)
    .bind(class_id)
    .execute(&st.pool)
    .await
    .expect("insert box");
    assert_eq!(
        counters_of(&st.pool, ds_id).await,
        (1, 1, 70, "ready".to_string())
    );
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_list_images_filtros() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();

    let (status, _, body) = call(
        app.clone(),
        post_create("Lista Img", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ds = json(&body)["id"].as_str().expect("id").to_string();
    let ds_id: uuid::Uuid = ds.parse().expect("uuid");

    let img1 = insert_image(&st.pool, ds_id, "l1.jpg", 10).await;
    let img2 = insert_image(&st.pool, ds_id, "l2.jpg", 20).await;
    let _img3 = insert_image(&st.pool, ds_id, "l3.jpg", 30).await;
    sqlx::query("UPDATE images SET split = 'val' WHERE id = $1")
        .bind(img2)
        .execute(&st.pool)
        .await
        .expect("split val");
    let class_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM classes WHERE dataset_id = $1 LIMIT 1")
            .bind(ds_id)
            .fetch_one(&st.pool)
            .await
            .expect("class id");
    sqlx::query(
        "INSERT INTO boxes (image_id, class_id, x, y, w, h, origin) VALUES ($1,$2,0.5,0.5,0.2,0.2,'manual')",
    )
    .bind(img1)
    .bind(class_id)
    .execute(&st.pool)
    .await
    .expect("insert box");

    // Sem filtros: total 3, limit 50, ordem created_at DESC, url fallback.
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/datasets/{ds}/images"))
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let page = json(&body);
    assert_eq!(page["total"], 3);
    assert_eq!(page["limit"], 50);
    assert_eq!(page["offset"], 0);
    let items = page["items"].as_array().expect("items");
    assert_eq!(items.len(), 3);
    let c0 = items[0]["createdAt"].as_str().expect("createdAt");
    let c1 = items[1]["createdAt"].as_str().expect("createdAt");
    let c2 = items[2]["createdAt"].as_str().expect("createdAt");
    assert!(c0 >= c1 && c1 >= c2, "ordem created_at DESC");
    let url = items[0]["url"].as_str().expect("url");
    assert!(url.starts_with("/api/datasets/"), "{url}");
    assert!(items[0].get("objectKey").is_some());
    assert!(items[0].get("object_key").is_none(), "snake_case no wire");
    assert!(items[0].get("mediaType").is_some());

    // ?split=val ⇒ total 1.
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/datasets/{ds}/images?split=val"))
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["total"], 1);

    // ?labeled=true ⇒ total 1 (format yolo_txt ⇒ box).
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/datasets/{ds}/images?labeled=true"))
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["total"], 1);

    // ?limit=0 ⇒ 400; ?split=test ⇒ 400; id não-UUID ⇒ 404.
    for uri in [
        format!("/api/datasets/{ds}/images?limit=0"),
        format!("/api/datasets/{ds}/images?split=test"),
    ] {
        let (status, _, body) = call(
            app.clone(),
            Request::builder()
                .method("GET")
                .uri(uri)
                .header(http::header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json(&body)["code"], "invalid_request");
    }
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/datasets/nao-e-uuid/images")
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&body)["code"], "not_found");
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_upload_storage_unavailable_503() {
    let _guard = SERIAL.lock().await;
    let mut st = state().await;
    st.storage = std::sync::Arc::new(api_principal::storage::MockStorage::failing());
    let app = routes::build(st.clone());
    let cookie = authed_cookie();
    let (status, _, body) = call(
        app.clone(),
        post_create("Bucket Morto", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ds = json(&body)["id"].as_str().expect("id").to_string();
    let ds_id: uuid::Uuid = ds.parse().expect("uuid");
    let boundary = "heph-failing-boundary";
    let (status, _, body) = call(
        app.clone(),
        post_upload(
            &cookie,
            &ds,
            boundary,
            multipart_body(boundary, &[("a.png", &png_1x1())]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json(&body)["code"], "storage_unavailable");
    // PUT falhou ANTES do INSERT: nenhuma linha órfã.
    assert_eq!(
        counters_of(&st.pool, ds_id).await,
        (0, 0, 0, "needs_labeling".to_string())
    );
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_list_presign_503() {
    let _guard = SERIAL.lock().await;
    let mut st = state().await;
    st.storage = std::sync::Arc::new(api_principal::storage::MockStorage::failing());
    st.storage_config.public_endpoint = Some("http://localhost:8333".to_string());
    let app = routes::build(st.clone());
    let cookie = authed_cookie();
    let (status, _, body) = call(
        app.clone(),
        post_create("Presign Morto", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ds = json(&body)["id"].as_str().expect("id").to_string();
    let ds_id: uuid::Uuid = ds.parse().expect("uuid");
    let _ = insert_image(&st.pool, ds_id, "p.jpg", 10).await;
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/datasets/{ds}/images"))
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json(&body)["code"], "storage_unavailable");
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_upload_malformed_400() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();
    let (status, _, body) = call(
        app.clone(),
        post_create("Malformado", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ds = json(&body)["id"].as_str().expect("id").to_string();
    // Content-type declara boundary X mas o corpo é lixo que nunca fecha o
    // boundary: o ramo next_field-Err não-413 dá `break` com items vazios ⇒
    // 400 invalid_request (e nunca hang).
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/datasets/{ds}/upload"))
            .header(
                http::header::CONTENT_TYPE,
                "multipart/form-data; boundary=X",
            )
            .header(http::header::COOKIE, &cookie)
            .body(Body::from("lixo-que-nunca-fecha-boundary"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_detail_completo() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();

    let (status, _, body) = call(
        app.clone(),
        post_create("Detail Rt", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ds = json(&body)["id"].as_str().expect("id").to_string();
    let ds_id: uuid::Uuid = ds.parse().expect("uuid");
    let class_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM classes WHERE dataset_id = $1 LIMIT 1")
            .bind(ds_id)
            .fetch_one(&st.pool)
            .await
            .expect("class id");

    let img = insert_image(&st.pool, ds_id, "d1.jpg", 42).await;
    sqlx::query(
        "INSERT INTO boxes (image_id, class_id, x, y, w, h, conf, origin, track_id) \
          VALUES ($1,$2,0.5,0.5,0.2,0.2,0.9,'manual',NULL)",
    )
    .bind(img)
    .bind(class_id)
    .execute(&st.pool)
    .await
    .expect("insert box 1");
    sqlx::query(
        "INSERT INTO boxes (image_id, class_id, x, y, w, h, origin, track_id) \
          VALUES ($1,$2,0.1,0.1,0.3,0.3,'autotracker',7)",
    )
    .bind(img)
    .bind(class_id)
    .execute(&st.pool)
    .await
    .expect("insert box 2");
    sqlx::query("INSERT INTO captions (image_id, text, origin) VALUES ($1,'um gato','manual')")
        .bind(img)
        .execute(&st.pool)
        .await
        .expect("insert caption");

    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/datasets/{ds}/images/{img}"))
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let detail = json(&body);
    assert_eq!(detail["id"], img.to_string());
    assert_eq!(detail["filename"], "d1.jpg");
    let boxes = detail["boxes"].as_array().expect("boxes");
    assert_eq!(boxes.len(), 2);
    assert!(boxes[0].get("classId").is_some(), "{}", boxes[0]);
    assert!(boxes[0].get("trackId").is_some(), "{}", boxes[0]);
    assert!(boxes[0].get("class_id").is_none(), "snake_case no wire");
    assert!(boxes[0].get("track_id").is_none(), "snake_case no wire");
    assert_eq!(detail["caption"]["text"], "um gato");
    let url = detail["url"].as_str().expect("url");
    assert_eq!(url, format!("/api/datasets/{ds}/images/{img}/data"));

    // Escopo: imageId de OUTRO dataset ⇒ 404.
    let (status, _, body) = call(
        app.clone(),
        post_create("Detail Outro", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ds2 = json(&body)["id"].as_str().expect("id").to_string();
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/datasets/{ds2}/images/{img}"))
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&body)["code"], "not_found");
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_data_proxy() {
    let _guard = SERIAL.lock().await;
    let mut st = state().await;
    let mock = std::sync::Arc::new(api_principal::storage::MockStorage::new());
    st.storage = mock.clone();
    let app = routes::build(st.clone());
    let cookie = authed_cookie();

    let (status, _, body) = call(
        app.clone(),
        post_create("Data Rt", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ds = json(&body)["id"].as_str().expect("id").to_string();
    let ds_id: uuid::Uuid = ds.parse().expect("uuid");

    // Imagem PNG (media_type dirige o content-type do proxy).
    let img = uuid::Uuid::new_v4();
    let filename = "p.png";
    let object_key = format!("datasets/{ds_id}/images/{img}/{filename}");
    sqlx::query(
        "INSERT INTO images (id, dataset_id, filename, object_key, bytes, width, height, md5, sha256, media_type) \
         VALUES ($1,$2,$3,$4,$5,1,1,'d41d8cd98f00b204e9800998ecf8427e','e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855','png')",
    )
    .bind(img)
    .bind(ds_id)
    .bind(filename)
    .bind(&object_key)
    .bind(70i64)
    .execute(&st.pool)
    .await
    .expect("insert image png");
    let key: String = sqlx::query_scalar("SELECT object_key FROM images WHERE id = $1")
        .bind(img)
        .fetch_one(&st.pool)
        .await
        .expect("select object_key");
    let png = png_1x1();
    mock.put_bytes(&key, png.clone()).await;

    let (status, headers, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/datasets/{ds}/images/{img}/data"))
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, png);
    assert_eq!(
        headers
            .get(http::header::CONTENT_TYPE)
            .expect("content-type")
            .to_str()
            .expect("content-type str"),
        "image/png"
    );
    let cc = headers
        .get(http::header::CACHE_CONTROL)
        .expect("cache-control")
        .to_str()
        .expect("cache-control str");
    assert!(cc.contains("immutable"), "{cc}");

    // Linha sem objeto (sem put_bytes) ⇒ 404 honesto.
    let img2 = insert_image(&st.pool, ds_id, "sem-objeto.jpg", 10).await;
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/datasets/{ds}/images/{img2}/data"))
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&body)["code"], "not_found");

    // Backend morto ⇒ 503.
    let mut st_fail = state().await;
    st_fail.storage = std::sync::Arc::new(api_principal::storage::MockStorage::failing());
    let app_fail = routes::build(st_fail.clone());
    let ds_id2: uuid::Uuid = {
        let (status, _, body) = call(
            app_fail.clone(),
            post_create("Data Morto", &serde_json::json!(["a"]), &cookie),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        json(&body)["id"]
            .as_str()
            .expect("id")
            .parse()
            .expect("uuid")
    };
    let img3 = insert_image(&st_fail.pool, ds_id2, "m.jpg", 10).await;
    let ds2 = ds_id2.to_string();
    let (status, _, body) = call(
        app_fail.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/datasets/{ds2}/images/{img3}/data"))
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json(&body)["code"], "storage_unavailable");
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_list_e_detail_url_presinada() {
    let _guard = SERIAL.lock().await;
    let mut st = state().await;
    st.storage = std::sync::Arc::new(api_principal::storage::MockStorage::new());
    st.storage_config.public_endpoint = Some("http://localhost:8333".to_string());
    let app = routes::build(st.clone());
    let cookie = authed_cookie();

    let (status, _, body) = call(
        app.clone(),
        post_create("Presign Rt", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ds = json(&body)["id"].as_str().expect("id").to_string();
    let ds_id: uuid::Uuid = ds.parse().expect("uuid");
    let img = insert_image(&st.pool, ds_id, "s.jpg", 10).await;

    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/datasets/{ds}/images"))
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let list_url = json(&body)["items"][0]["url"]
        .as_str()
        .expect("url")
        .to_string();
    assert!(list_url.starts_with("mock://heph-test/"), "{list_url}");

    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/datasets/{ds}/images/{img}"))
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let detail_url = json(&body)["url"].as_str().expect("url").to_string();
    assert!(detail_url.starts_with("mock://heph-test/"), "{detail_url}");
}

fn put_json(cookie: &str, method: &str, uri: String, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::COOKIE, cookie)
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn class_id_of(pool: &sqlx::PgPool, dataset_id: uuid::Uuid) -> uuid::Uuid {
    sqlx::query_scalar("SELECT id FROM classes WHERE dataset_id = $1 LIMIT 1")
        .bind(dataset_id)
        .fetch_one(pool)
        .await
        .expect("class id")
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_put_boxes_fluxo_status() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();

    let (status, _, body) = call(
        app.clone(),
        post_create("Boxes Rt", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let created = json(&body);
    let ds = created["id"].as_str().expect("id").to_string();
    let ds_id: uuid::Uuid = ds.parse().expect("uuid");
    // Gap 3b.7 fechado end-to-end: classId sai do wire e alimenta o PUT boxes.
    let class_id: uuid::Uuid = created["classes"][0]["id"]
        .as_str()
        .expect("classes[0].id")
        .parse()
        .expect("uuid");
    assert_eq!(class_id, class_id_of(&st.pool, ds_id).await);

    let img1 = insert_image(&st.pool, ds_id, "b1.jpg", 100).await;
    let img2 = insert_image(&st.pool, ds_id, "b2.jpg", 50).await;

    // PUT 1 box (conf 0.9, trackId 7; origin ausente ⇒ "manual").
    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/images/{img1}/boxes"),
            serde_json::json!({"boxes": [{"classId": class_id, "x": 0.5, "y": 0.5, "w": 0.2, "h": 0.2, "conf": 0.9, "trackId": 7}]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resp = json(&body);
    let boxes = resp["boxes"].as_array().expect("boxes");
    assert_eq!(boxes.len(), 1);
    assert!(boxes[0]["id"].is_string());
    assert_eq!(boxes[0]["classId"], class_id.to_string());
    assert_eq!(boxes[0]["origin"], "manual");
    assert_eq!(boxes[0]["conf"], 0.9);
    assert_eq!(boxes[0]["trackId"], 7);
    assert_eq!(
        counters_of(&st.pool, ds_id).await,
        (2, 1, 150, "in_progress".to_string())
    );

    // Substituição total: PUT com 2 boxes ⇒ GET detail tem len 2.
    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/images/{img1}/boxes"),
            serde_json::json!({"boxes": [
                {"classId": class_id, "x": 0.1, "y": 0.1, "w": 0.3, "h": 0.3},
                {"classId": class_id, "x": 0.6, "y": 0.6, "w": 0.2, "h": 0.2, "origin": "import"}
            ]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["boxes"].as_array().expect("boxes").len(), 2);
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/datasets/{ds}/images/{img1}"))
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["boxes"].as_array().expect("boxes").len(), 2);

    // Segunda imagem rotulada ⇒ ready (2,2).
    let (status, _, _) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/images/{img2}/boxes"),
            serde_json::json!({"boxes": [{"classId": class_id, "x": 0.5, "y": 0.5, "w": 0.2, "h": 0.2}]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        counters_of(&st.pool, ds_id).await,
        (2, 2, 150, "ready".to_string())
    );

    // Classe inexistente ⇒ 400 (sem vazar nada).
    let ghost = uuid::Uuid::new_v4();
    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/images/{img1}/boxes"),
            serde_json::json!({"boxes": [{"classId": ghost, "x": 0.5, "y": 0.5, "w": 0.2, "h": 0.2}]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // Classe de OUTRO dataset ⇒ 400 seco (não vaza existência).
    let (status, _, body) = call(
        app.clone(),
        post_create("Boxes Outro", &serde_json::json!(["b"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ds2 = json(&body)["id"].as_str().expect("id").to_string();
    let ds2_id: uuid::Uuid = ds2.parse().expect("uuid");
    let other_class = class_id_of(&st.pool, ds2_id).await;
    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/images/{img1}/boxes"),
            serde_json::json!({"boxes": [{"classId": other_class, "x": 0.5, "y": 0.5, "w": 0.2, "h": 0.2}]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // Imagem de outro dataset ⇒ 404 (escopo, não 400).
    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds2}/images/{img1}/boxes"),
            serde_json::json!({"boxes": [{"classId": other_class, "x": 0.5, "y": 0.5, "w": 0.2, "h": 0.2}]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&body)["code"], "not_found");
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_put_caption() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();

    // format captions (R9): caption rotula, box não.
    let ds_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO datasets (id, slug, title, category, type, task, format, status) \
         VALUES ($1,'cap-put','Cap Put','difusao','difusao_lora','caption','captions','needs_labeling')",
    )
    .bind(ds_id)
    .execute(&st.pool)
    .await
    .expect("insert dataset captions");
    let ds = ds_id.to_string();
    let img = insert_image(&st.pool, ds_id, "cap.jpg", 80).await;

    // Text vazio ⇒ 400 SEM linha.
    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/images/{img}/caption"),
            serde_json::json!({"text": ""}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM captions WHERE image_id = $1")
        .bind(img)
        .fetch_one(&st.pool)
        .await
        .expect("count captions");
    assert_eq!(n, 0);

    // PUT "um gato" ⇒ 200 com updatedAt.
    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/images/{img}/caption"),
            serde_json::json!({"text": "um gato"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let first = json(&body);
    assert_eq!(first["text"], "um gato");
    assert_eq!(first["origin"], "manual");
    assert!(first["updatedAt"].is_string());
    let updated1 = first["updatedAt"].as_str().expect("updatedAt").to_string();

    // Upsert: texto novo, updated_at >= anterior, continua 1 linha.
    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/images/{img}/caption"),
            serde_json::json!({"text": "um gato preto", "origin": "import"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let second = json(&body);
    assert_eq!(second["text"], "um gato preto");
    assert_eq!(second["origin"], "import");
    let updated2 = second["updatedAt"].as_str().expect("updatedAt").to_string();
    assert!(updated2 >= updated1, "{updated2} < {updated1}");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM captions WHERE image_id = $1")
        .bind(img)
        .fetch_one(&st.pool)
        .await
        .expect("count captions");
    assert_eq!(n, 1);

    // Contadores: caption rotula format captions ⇒ ready.
    assert_eq!(
        counters_of(&st.pool, ds_id).await,
        (1, 1, 80, "ready".to_string())
    );

    // GET detail mostra o caption novo.
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/datasets/{ds}/images/{img}"))
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["caption"]["text"], "um gato preto");
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_put_boxes_wires_camel() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();

    let (status, _, body) = call(
        app.clone(),
        post_create("Wires Camel", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ds = json(&body)["id"].as_str().expect("id").to_string();
    let ds_id: uuid::Uuid = ds.parse().expect("uuid");
    let class_id = class_id_of(&st.pool, ds_id).await;
    let img = insert_image(&st.pool, ds_id, "wire.jpg", 10).await;

    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/images/{img}/boxes"),
            serde_json::json!({"boxes": [{"classId": class_id, "x": 0.5, "y": 0.5, "w": 0.2, "h": 0.2, "trackId": 7}]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let raw = String::from_utf8(body).expect("utf8");
    assert!(raw.contains("classId"), "{raw}");
    assert!(raw.contains("trackId"), "{raw}");
    assert!(!raw.contains("class_id"), "snake_case no wire: {raw}");
    assert!(!raw.contains("track_id"), "snake_case no wire: {raw}");

    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/images/{img}/caption"),
            serde_json::json!({"text": "fios"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let raw = String::from_utf8(body).expect("utf8");
    assert!(raw.contains("updatedAt"), "{raw}");
    assert!(!raw.contains("updated_at"), "snake_case no wire: {raw}");
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_delete_sweep_registrado() {
    // D7 commit→sweep: dataset + 1 imagem no mock, DELETE ⇒ 204 e ops contém
    // `DELETE_PREFIX datasets/{id}/`; `source` derivado prova a Parte B.
    let _guard = SERIAL.lock().await;
    let mut st = state().await;
    let mock = std::sync::Arc::new(api_principal::storage::MockStorage::new());
    st.storage = mock.clone();
    let app = routes::build(st.clone());
    let cookie = authed_cookie();

    let (status, _, body) = call(
        app.clone(),
        post_create("Sweep Rt", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ds = json(&body)["id"].as_str().expect("id").to_string();
    let ds_id: uuid::Uuid = ds.parse().expect("uuid");

    let img = insert_image(&st.pool, ds_id, "s.png", 70).await;
    let key: String = sqlx::query_scalar("SELECT object_key FROM images WHERE id = $1")
        .bind(img)
        .fetch_one(&st.pool)
        .await
        .expect("object_key");
    mock.put_bytes(&key, vec![1, 2, 3]).await;

    // Parte B ao vivo no banco: com imagem, `source` deriva `s3://…`.
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/datasets/{ds}"))
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let source = json(&body)["source"].as_str().expect("source").to_string();
    assert!(source.starts_with("s3://"), "{source}");
    assert!(source.contains(&ds), "{source}");

    let (status, _, _) = call(
        app.clone(),
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/datasets/{ds}"))
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let ops = mock.ops();
    assert!(
        ops.contains(&format!("DELETE_PREFIX datasets/{ds_id}/")),
        "sweep ausente: {ops:?}"
    );
    assert!(
        mock.snapshot()
            .iter()
            .all(|(k, _)| !k.starts_with(&format!("datasets/{ds_id}/"))),
        "objeto órfão sob o prefixo: {:?}",
        mock.snapshot()
    );
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_delete_sweep_falho_ainda_204() {
    // D7: sweep falho NÃO vira erro — DELETE com backend morto ainda é 204.
    let _guard = SERIAL.lock().await;
    let mut st = state().await;
    st.storage = std::sync::Arc::new(api_principal::storage::MockStorage::failing());
    let app = routes::build(st.clone());
    let cookie = authed_cookie();

    let (status, _, body) = call(
        app.clone(),
        post_create("Sweep Morto", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ds = json(&body)["id"].as_str().expect("id").to_string();

    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/datasets/{ds}"))
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty());
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
        ("GET", "/api/datasets/00000000-0000-0000-0000-000000000000"),
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

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn auto_tracked_derives_from_box_origin() {
    // autoTracked derivado (dívida T7): true só com box origin='autotracker'.
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();

    async fn dataset_id_of(
        app: &axum::Router,
        cookie: &str,
        title: &str,
    ) -> (axum::Router, uuid::Uuid) {
        let (status, _, body) = call(
            app.clone(),
            post_create(title, &serde_json::json!(["a"]), cookie),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        // Recém-criado não tem imagens ⇒ autoTracked false.
        assert_eq!(json(&body)["autoTracked"], false);
        let id: uuid::Uuid = json(&body)["id"]
            .as_str()
            .expect("id")
            .parse()
            .expect("uuid");
        (app.clone(), id)
    }

    let (app2, ds_a) = dataset_id_of(&app, &cookie, "Auto A").await;
    let (app2, ds_b) = dataset_id_of(&app2, &cookie, "Auto B").await;
    let (app2, ds_c) = dataset_id_of(&app2, &cookie, "Auto C").await;
    let app = app2;

    let img_a = insert_image(&st.pool, ds_a, "a.jpg", 10).await;
    let class_a = class_id_of(&st.pool, ds_a).await;
    sqlx::query(
        "INSERT INTO boxes (image_id, class_id, x, y, w, h, origin) VALUES ($1,$2,0.5,0.5,0.2,0.2,'autotracker')",
    )
    .bind(img_a)
    .bind(class_a)
    .execute(&st.pool)
    .await
    .expect("insert box autotracker");

    let img_b = insert_image(&st.pool, ds_b, "b.jpg", 10).await;
    let class_b = class_id_of(&st.pool, ds_b).await;
    sqlx::query(
        "INSERT INTO boxes (image_id, class_id, x, y, w, h, origin) VALUES ($1,$2,0.5,0.5,0.2,0.2,'manual')",
    )
    .bind(img_b)
    .bind(class_b)
    .execute(&st.pool)
    .await
    .expect("insert box manual");

    // GET /api/datasets/:id por dataset.
    for (ds, expected) in [(ds_a, true), (ds_b, false), (ds_c, false)] {
        let (status, _, body) = call(
            app.clone(),
            Request::builder()
                .method("GET")
                .uri(format!("/api/datasets/{ds}"))
                .header(http::header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{ds}");
        assert_eq!(json(&body)["autoTracked"], expected, "{ds}");
    }

    // GET /api/datasets lista os três com os mesmos valores.
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
    let items = list.as_array().expect("array");
    assert_eq!(items.len(), 3);
    let by_id = |id: uuid::Uuid| {
        items
            .iter()
            .find(|d| d["id"] == id.to_string())
            .unwrap_or_else(|| panic!("dataset {id} ausente na lista"))
            .clone()
    };
    assert_eq!(by_id(ds_a)["autoTracked"], true);
    assert_eq!(by_id(ds_b)["autoTracked"], false);
    assert_eq!(by_id(ds_c)["autoTracked"], false);
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0005_image_soft_delete() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();

    let (status, _, body) = call(
        app.clone(),
        post_create("Lixeira", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ds_id: uuid::Uuid = json(&body)["id"]
        .as_str()
        .expect("id")
        .parse()
        .expect("uuid");
    let ds = ds_id.to_string();
    let class_id = class_id_of(&st.pool, ds_id).await;

    let img1 = insert_image(&st.pool, ds_id, "a.jpg", 100).await;
    sqlx::query(
        "INSERT INTO boxes (image_id, class_id, x, y, w, h, origin) VALUES ($1,$2,0.5,0.5,0.2,0.2,'manual')",
    )
    .bind(img1)
    .bind(class_id)
    .execute(&st.pool)
    .await
    .expect("insert box");
    assert_eq!(
        counters_of(&st.pool, ds_id).await,
        (1, 1, 100, "ready".to_string())
    );

    // (b) soft delete: contadores zeram, status volta, source derivado vira null.
    sqlx::query("UPDATE images SET deleted_at = now() WHERE id = $1")
        .bind(img1)
        .execute(&st.pool)
        .await
        .expect("soft delete");
    assert_eq!(
        counters_of(&st.pool, ds_id).await,
        (0, 0, 0, "needs_labeling".to_string())
    );
    let (status, _, body) = call(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/datasets/{ds}"))
            .header(http::header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(json(&body)["source"].is_null());

    // (c) restore: contadores voltam.
    sqlx::query("UPDATE images SET deleted_at = NULL WHERE id = $1")
        .bind(img1)
        .execute(&st.pool)
        .await
        .expect("restore");
    assert_eq!(
        counters_of(&st.pool, ds_id).await,
        (1, 1, 100, "ready".to_string())
    );

    // (d) unique parcial: filename na lixeira libera reinsert ativo.
    sqlx::query("UPDATE images SET deleted_at = now() WHERE id = $1")
        .bind(img1)
        .execute(&st.pool)
        .await
        .expect("soft delete 2");
    let img2 = insert_image(&st.pool, ds_id, "a.jpg", 50).await;
    assert_eq!(
        counters_of(&st.pool, ds_id).await,
        (1, 0, 50, "needs_labeling".to_string())
    );

    // Duplicada entre duas ATIVAS continua violando.
    let dup = sqlx::query(
        "INSERT INTO images (id, dataset_id, filename, object_key, bytes, width, height, md5, sha256, media_type) \
         VALUES ($1,$2,'a.jpg',$3,10,640,480,'d41d8cd98f00b204e9800998ecf8427e','e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855','jpeg')",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(ds_id)
    .bind(format!("datasets/{ds_id}/images/{}/a.jpg", uuid::Uuid::new_v4()))
    .execute(&st.pool)
    .await;
    assert!(
        dup.is_err(),
        "duplicada ativa deveria violar o índice parcial"
    );

    // Limpeza: a linha re-inserida sai para não poluir outros testes.
    sqlx::query("DELETE FROM images WHERE id = $1")
        .bind(img2)
        .execute(&st.pool)
        .await
        .expect("cleanup img2");
    sqlx::query("DELETE FROM images WHERE id = $1")
        .bind(img1)
        .execute(&st.pool)
        .await
        .expect("cleanup img1");
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_put_classes_rename_reordena_adiciona() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();
    let (status, _, body) = call(
        app.clone(),
        post_create(
            "Classes Rename",
            &serde_json::json!(["solda_fria", "curto"]),
            &cookie,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let created = json(&body);
    let ds = created["id"].as_str().expect("id").to_string();
    let ds_id: uuid::Uuid = ds.parse().expect("uuid");
    let class_a: uuid::Uuid = created["classes"][0]["id"]
        .as_str()
        .expect("classes[0].id")
        .parse()
        .expect("uuid");
    let class_b: uuid::Uuid = created["classes"][1]["id"]
        .as_str()
        .expect("classes[1].id")
        .parse()
        .expect("uuid");

    // Caixa apontando para class_a antes do rename.
    let img = insert_image(&st.pool, ds_id, "r.jpg", 100).await;
    sqlx::query(
        "INSERT INTO boxes (image_id, class_id, x, y, w, h, origin) VALUES ($1,$2,0.5,0.5,0.2,0.2,'manual')",
    )
    .bind(img)
    .bind(class_a)
    .execute(&st.pool)
    .await
    .expect("insert box");

    // (a) rename preserva id + (b) reordena (idx 0..n-1) + (c) adiciona nova.
    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/classes"),
            serde_json::json!({"classes": [
                {"id": class_b, "name": "curto_novo"},
                {"id": class_a, "name": "solda_renomeada"},
                {"name": "nova_classe"}
            ]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resp = json(&body);
    let classes = resp["classes"].as_array().expect("classes");
    assert_eq!(classes.len(), 3);
    assert_eq!(classes[0]["id"], class_b.to_string());
    assert_eq!(classes[0]["name"], "curto_novo");
    assert_eq!(classes[0]["idx"], 0);
    assert_eq!(classes[0]["color"], "#10b981");
    assert_eq!(classes[1]["id"], class_a.to_string());
    assert_eq!(classes[1]["name"], "solda_renomeada");
    assert_eq!(classes[1]["idx"], 1);
    assert_eq!(classes[1]["color"], "#f59e0b");
    assert!(classes[2]["id"].is_string());
    assert_eq!(classes[2]["name"], "nova_classe");
    assert_eq!(classes[2]["idx"], 2);
    assert_eq!(classes[2]["color"], "#f43f5e");

    // A caixa continua apontando para o mesmo classId e segue funcionando.
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM boxes WHERE class_id = $1")
        .bind(class_a)
        .fetch_one(&st.pool)
        .await
        .expect("count boxes");
    assert_eq!(n, 1);
    let (status, _, _) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/images/{img}/boxes"),
            serde_json::json!({"boxes": [{"classId": class_a, "x": 0.1, "y": 0.1, "w": 0.2, "h": 0.2}]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_put_classes_guard_409_e_remove_livre() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();
    let (status, _, body) = call(
        app.clone(),
        post_create(
            "Classes Guard",
            &serde_json::json!(["em_uso", "livre"]),
            &cookie,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let created = json(&body);
    let ds = created["id"].as_str().expect("id").to_string();
    let ds_id: uuid::Uuid = ds.parse().expect("uuid");
    let used: uuid::Uuid = created["classes"][0]["id"]
        .as_str()
        .expect("classes[0].id")
        .parse()
        .expect("uuid");
    let free: uuid::Uuid = created["classes"][1]["id"]
        .as_str()
        .expect("classes[1].id")
        .parse()
        .expect("uuid");

    let img = insert_image(&st.pool, ds_id, "g.jpg", 100).await;
    sqlx::query(
        "INSERT INTO boxes (image_id, class_id, x, y, w, h, origin) VALUES ($1,$2,0.5,0.5,0.2,0.2,'manual')",
    )
    .bind(img)
    .bind(used)
    .execute(&st.pool)
    .await
    .expect("insert box");
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM boxes")
        .fetch_one(&st.pool)
        .await
        .expect("count boxes");

    // (d) remover classe EM USO → 409 e caixas intactas.
    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/classes"),
            serde_json::json!({"classes": [{"id": free, "name": "livre"}]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(json(&body)["code"], "classes_in_use");
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM boxes")
        .fetch_one(&st.pool)
        .await
        .expect("count boxes");
    assert_eq!(before, after);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM classes WHERE dataset_id = $1")
        .bind(ds_id)
        .fetch_one(&st.pool)
        .await
        .expect("count classes");
    assert_eq!(n, 2, "409 não escreve nada");

    // (e) remover classe livre → 200 e classe some.
    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/classes"),
            serde_json::json!({"classes": [{"id": used, "name": "em_uso"}]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let classes = json(&body)["classes"].as_array().expect("classes").clone();
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0]["id"], used.to_string());
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_put_classes_erros_404_400() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();
    let (status, _, body) = call(
        app.clone(),
        post_create("Classes Erros", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let created = json(&body);
    let ds = created["id"].as_str().expect("id").to_string();

    // (f) dataset inexistente → 404 (e path não-UUID → 404).
    let ghost = uuid::Uuid::new_v4();
    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ghost}/classes"),
            serde_json::json!({"classes": [{"name": "x"}]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&body)["code"], "not_found");
    let (status, _, _) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            "/api/datasets/nao-e-uuid/classes".to_string(),
            serde_json::json!({"classes": [{"name": "x"}]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // (g) id de classe de OUTRO dataset → 400.
    let (status, _, body) = call(
        app.clone(),
        post_create("Classes Outro", &serde_json::json!(["b"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let other = json(&body)["classes"][0]["id"]
        .as_str()
        .expect("id")
        .to_string();
    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/classes"),
            serde_json::json!({"classes": [{"id": other, "name": "b"}]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // (h) nome duplicado → 400.
    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/classes"),
            serde_json::json!({"classes": [{"name": "dup"}, {"name": "dup"}]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_put_classes_remove_primeira_e_meio() {
    // Regressão do review: DELETE das removidas roda ANTES da fase 2, senão
    // renumerar um mantido para um idx ainda ocupado volta 500 (UNIQUE idx).
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();
    let (status, _, body) = call(
        app.clone(),
        post_create(
            "Classes Colisao",
            &serde_json::json!(["primeira", "segunda", "terceira"]),
            &cookie,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let created = json(&body);
    let ds = created["id"].as_str().expect("id").to_string();
    let ids: Vec<String> = created["classes"]
        .as_array()
        .expect("classes")
        .iter()
        .map(|c| c["id"].as_str().expect("id").to_string())
        .collect();
    assert_eq!(ids.len(), 3);

    // (a) probe do reviewer: remover a 1ª (idx 0) mantendo a 2ª → 200, idx 0.
    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/classes"),
            serde_json::json!({"classes": [{"id": ids[1], "name": "segunda"}]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let classes = json(&body)["classes"].as_array().expect("classes").clone();
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0]["id"], ids[1]);
    assert_eq!(classes[0]["idx"], 0);

    // (b) 3 classes de novo, remover a do MEIO → 200 com idx 0,1.
    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/classes"),
            serde_json::json!({"classes": [
                {"id": ids[1], "name": "segunda"},
                {"name": "nova_a"},
                {"name": "nova_b"},
            ]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let classes = json(&body)["classes"].as_array().expect("classes").clone();
    assert_eq!(classes.len(), 3);
    let cur: Vec<String> = classes
        .iter()
        .map(|c| c["id"].as_str().expect("id").to_string())
        .collect();
    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/classes"),
            serde_json::json!({"classes": [
                {"id": cur[0], "name": "segunda"},
                {"id": cur[2], "name": "nova_b"},
            ]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let classes = json(&body)["classes"].as_array().expect("classes").clone();
    assert_eq!(classes.len(), 2);
    assert_eq!(classes[0]["id"], cur[0]);
    assert_eq!(classes[0]["idx"], 0);
    assert_eq!(classes[1]["id"], cur[2]);
    assert_eq!(classes[1]["idx"], 1);

    // (c) remover a última continua 200 (caso já coberto, sem regressão).
    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/classes"),
            serde_json::json!({"classes": [{"id": cur[0], "name": "segunda"}]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let classes = json(&body)["classes"].as_array().expect("classes").clone();
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0]["idx"], 0);
}

fn bare_req(cookie: &str, method: &str, uri: String) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(http::header::COOKIE, cookie)
        .body(Body::empty())
        .unwrap()
}

async fn insert_labeled_image(
    pool: &sqlx::PgPool,
    ds: uuid::Uuid,
    class_id: uuid::Uuid,
    filename: &str,
    bytes: i64,
) -> uuid::Uuid {
    let img = insert_image(pool, ds, filename, bytes).await;
    sqlx::query(
        "INSERT INTO boxes (image_id, class_id, x, y, w, h, origin) VALUES ($1,$2,0.5,0.5,0.2,0.2,'manual')",
    )
    .bind(img)
    .bind(class_id)
    .execute(pool)
    .await
    .expect("insert box");
    img
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_trash_soft_delete_e_lista() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();
    let (status, _, body) = call(
        app.clone(),
        post_create("Lixeira A", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let created = json(&body);
    let ds = created["id"].as_str().expect("id").to_string();
    let ds_id: uuid::Uuid = ds.parse().expect("uuid");
    let class_id: uuid::Uuid = created["classes"][0]["id"]
        .as_str()
        .expect("classes[0].id")
        .parse()
        .expect("uuid");

    let img = insert_labeled_image(&st.pool, ds_id, class_id, "a.jpg", 100).await;

    // (a) soft delete → 204, sem corpo.
    let (status, _, body) = call(
        app.clone(),
        bare_req(
            &cookie,
            "DELETE",
            format!("/api/datasets/{ds}/images/{img}"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty());

    // Sai do list default, aparece em ?deleted=true; contadores zeram.
    let (status, _, body) = call(
        app.clone(),
        bare_req(&cookie, "GET", format!("/api/datasets/{ds}/images")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["total"], 0);
    let (status, _, body) = call(
        app.clone(),
        bare_req(
            &cookie,
            "GET",
            format!("/api/datasets/{ds}/images?deleted=true"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let trash = json(&body);
    assert_eq!(trash["total"], 1);
    assert_eq!(trash["items"].as_array().expect("items").len(), 1);
    assert_eq!(
        counters_of(&st.pool, ds_id).await,
        (0, 0, 0, "needs_labeling".to_string())
    );

    // DELETE de novo → 404 (já na lixeira).
    let (status, _, body) = call(
        app.clone(),
        bare_req(
            &cookie,
            "DELETE",
            format!("/api/datasets/{ds}/images/{img}"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&body)["code"], "not_found");

    // Restore de ATIVA → 404.
    let img2 = insert_image(&st.pool, ds_id, "b.jpg", 50).await;
    let (status, _, body) = call(
        app.clone(),
        bare_req(
            &cookie,
            "POST",
            format!("/api/datasets/{ds}/images/{img2}/restore"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&body)["code"], "not_found");
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_trash_restore_sem_conflito() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();
    let (status, _, body) = call(
        app.clone(),
        post_create("Lixeira B", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let created = json(&body);
    let ds = created["id"].as_str().expect("id").to_string();
    let ds_id: uuid::Uuid = ds.parse().expect("uuid");
    let class_id: uuid::Uuid = created["classes"][0]["id"]
        .as_str()
        .expect("classes[0].id")
        .parse()
        .expect("uuid");

    let img = insert_labeled_image(&st.pool, ds_id, class_id, "a.jpg", 100).await;
    let (status, _, _) = call(
        app.clone(),
        bare_req(
            &cookie,
            "DELETE",
            format!("/api/datasets/{ds}/images/{img}"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // (b) restore sem conflito → 204; volta ao list; contadores voltam.
    let (status, _, body) = call(
        app.clone(),
        bare_req(
            &cookie,
            "POST",
            format!("/api/datasets/{ds}/images/{img}/restore"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty());
    let (status, _, body) = call(
        app.clone(),
        bare_req(&cookie, "GET", format!("/api/datasets/{ds}/images")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["total"], 1);
    assert_eq!(
        counters_of(&st.pool, ds_id).await,
        (1, 1, 100, "ready".to_string())
    );
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_trash_restore_com_conflito() {
    let _guard = SERIAL.lock().await;
    let mut st = state().await;
    let mock = std::sync::Arc::new(api_principal::storage::MockStorage::new());
    st.storage = mock.clone();
    let app = routes::build(st.clone());
    let cookie = authed_cookie();
    let (status, _, body) = call(
        app.clone(),
        post_create("Lixeira C", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let created = json(&body);
    let ds = created["id"].as_str().expect("id").to_string();
    let ds_id: uuid::Uuid = ds.parse().expect("uuid");
    let class_id: uuid::Uuid = created["classes"][0]["id"]
        .as_str()
        .expect("classes[0].id")
        .parse()
        .expect("uuid");

    // Original rotulada + objeto real no mock; vai para a lixeira.
    let img = insert_labeled_image(&st.pool, ds_id, class_id, "foto.jpg", 100).await;
    let old_key = format!("datasets/{ds_id}/images/{img}/foto.jpg");
    mock.put_bytes(&old_key, vec![7; 10]).await;
    let (status, _, _) = call(
        app.clone(),
        bare_req(
            &cookie,
            "DELETE",
            format!("/api/datasets/{ds}/images/{img}"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Re-ocupa o filename (nova linha ATIVA — UNIQUE parcial permite).
    let _img2 = insert_image(&st.pool, ds_id, "foto.jpg", 50).await;

    // (c) restore com conflito → 200 com filename `_restaurado`.
    let (status, _, body) = call(
        app.clone(),
        bare_req(
            &cookie,
            "POST",
            format!("/api/datasets/{ds}/images/{img}/restore"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["filename"], "foto_restaurado.jpg");

    // Mock registra COPY com as keys corretas; linha aponta p/ key nova.
    let new_key = format!("datasets/{ds_id}/images/{img}/foto_restaurado.jpg");
    assert!(mock.ops().contains(&format!("COPY {old_key} {new_key}")));
    let key_db: String = sqlx::query_scalar("SELECT object_key FROM images WHERE id = $1")
        .bind(img)
        .fetch_one(&st.pool)
        .await
        .expect("object_key");
    assert_eq!(key_db, new_key);
    let name_db: String = sqlx::query_scalar("SELECT filename FROM images WHERE id = $1")
        .bind(img)
        .fetch_one(&st.pool)
        .await
        .expect("filename");
    assert_eq!(name_db, "foto_restaurado.jpg");
    // Key antiga deletada no mock (best-effort pós-commit).
    assert!(mock.ops().contains(&format!("DELETE {old_key}")));
    let keys: Vec<String> = mock.snapshot().into_iter().map(|(k, _)| k).collect();
    assert!(keys.contains(&new_key));
    assert!(!keys.contains(&old_key));
    // Restaurada volta a contar (boxes intactas): 2 imagens, 1 rotulada.
    assert_eq!(
        counters_of(&st.pool, ds_id).await,
        (2, 1, 150, "in_progress".to_string())
    );
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_trash_restore_503_sem_parcial() {
    let _guard = SERIAL.lock().await;
    let mut st = state().await;
    let (status, _, body) = call(
        routes::build(st.clone()),
        post_create("Lixeira D", &serde_json::json!(["a"]), &authed_cookie()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let created = json(&body);
    let ds = created["id"].as_str().expect("id").to_string();
    let ds_id: uuid::Uuid = ds.parse().expect("uuid");
    let class_id: uuid::Uuid = created["classes"][0]["id"]
        .as_str()
        .expect("classes[0].id")
        .parse()
        .expect("uuid");

    let img = insert_labeled_image(&st.pool, ds_id, class_id, "x.jpg", 100).await;
    let cookie = authed_cookie();
    let app_ok = routes::build(st.clone());
    let (status, _, _) = call(
        app_ok.clone(),
        bare_req(
            &cookie,
            "DELETE",
            format!("/api/datasets/{ds}/images/{img}"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // Conflito ativo para forçar o caminho do copy_object.
    let _img2 = insert_image(&st.pool, ds_id, "x.jpg", 50).await;

    // (d) storage morto ⇒ 503 e a linha CONTINUA na lixeira.
    st.storage = std::sync::Arc::new(api_principal::storage::MockStorage::failing());
    let app = routes::build(st.clone());
    let (status, _, body) = call(
        app.clone(),
        bare_req(
            &cookie,
            "POST",
            format!("/api/datasets/{ds}/images/{img}/restore"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json(&body)["code"], "storage_unavailable");
    let still_trash: bool =
        sqlx::query_scalar("SELECT deleted_at IS NOT NULL FROM images WHERE id = $1")
            .bind(img)
            .fetch_one(&st.pool)
            .await
            .expect("deleted_at");
    assert!(still_trash, "nada parcial: linha continua na lixeira");
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_trash_purge_idempotente() {
    let _guard = SERIAL.lock().await;
    let mut st = state().await;
    let mock = std::sync::Arc::new(api_principal::storage::MockStorage::new());
    st.storage = mock.clone();
    let app = routes::build(st.clone());
    let cookie = authed_cookie();
    let (status, _, body) = call(
        app.clone(),
        post_create("Lixeira E", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let created = json(&body);
    let ds = created["id"].as_str().expect("id").to_string();
    let ds_id: uuid::Uuid = ds.parse().expect("uuid");
    let class_id: uuid::Uuid = created["classes"][0]["id"]
        .as_str()
        .expect("classes[0].id")
        .parse()
        .expect("uuid");

    // 1 ativa + 2 na lixeira (uma rotulada — CASCADE sai nos contadores).
    let keep = insert_image(&st.pool, ds_id, "keep.jpg", 10).await;
    let t1 = insert_labeled_image(&st.pool, ds_id, class_id, "t1.jpg", 100).await;
    let t2 = insert_image(&st.pool, ds_id, "t2.jpg", 50).await;
    for img in [t1, t2] {
        let (status, _, _) = call(
            app.clone(),
            bare_req(
                &cookie,
                "DELETE",
                format!("/api/datasets/{ds}/images/{img}"),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    // (e) purge → 204; lixeira some, ativa fica; sweep por imagem.
    let (status, _, body) = call(
        app.clone(),
        bare_req(&cookie, "DELETE", format!("/api/datasets/{ds}/trash")),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty());
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM images WHERE dataset_id = $1")
        .bind(ds_id)
        .fetch_one(&st.pool)
        .await
        .expect("count images");
    assert_eq!(n, 1);
    let kept: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM images WHERE id = $1)")
        .bind(keep)
        .fetch_one(&st.pool)
        .await
        .expect("ativa permanece");
    assert!(kept);
    assert!(mock
        .ops()
        .contains(&format!("DELETE_PREFIX datasets/{ds_id}/images/{t1}/")));
    assert!(mock
        .ops()
        .contains(&format!("DELETE_PREFIX datasets/{ds_id}/images/{t2}/")));
    assert_eq!(
        counters_of(&st.pool, ds_id).await,
        (1, 0, 10, "needs_labeling".to_string())
    );

    // Purge de novo (vazio) → 204 idempotente.
    let (status, _, _) = call(
        app.clone(),
        bare_req(&cookie, "DELETE", format!("/api/datasets/{ds}/trash")),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_trash_query_invalida_e_404s() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();
    let (status, _, body) = call(
        app.clone(),
        post_create("Lixeira F", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let created = json(&body);
    let ds = created["id"].as_str().expect("id").to_string();
    let ds_id: uuid::Uuid = ds.parse().expect("uuid");
    let img = insert_image(&st.pool, ds_id, "q.jpg", 10).await;

    // (f) deleted=banana → 400.
    let (status, _, body) = call(
        app.clone(),
        bare_req(
            &cookie,
            "GET",
            format!("/api/datasets/{ds}/images?deleted=banana"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // (g) imageId não-UUID (soft delete e restore) → 404.
    for (method, uri) in [
        ("DELETE", format!("/api/datasets/{ds}/images/nao-e-uuid")),
        (
            "POST",
            format!("/api/datasets/{ds}/images/nao-e-uuid/restore"),
        ),
    ] {
        let (status, _, body) = call(app.clone(), bare_req(&cookie, method, uri.clone())).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(json(&body)["code"], "not_found", "{uri}");
    }

    // Imagem de OUTRO dataset (soft delete e restore) → 404.
    let (status, _, body) = call(
        app.clone(),
        post_create("Lixeira G", &serde_json::json!(["b"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ds2 = json(&body)["id"].as_str().expect("id").to_string();
    for (method, uri) in [
        ("DELETE", format!("/api/datasets/{ds2}/images/{img}")),
        ("POST", format!("/api/datasets/{ds2}/images/{img}/restore")),
    ] {
        let (status, _, body) = call(app.clone(), bare_req(&cookie, method, uri.clone())).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(json(&body)["code"], "not_found", "{uri}");
    }

    // Trash com dataset inexistente → 404.
    let ghost = uuid::Uuid::new_v4();
    let (status, _, body) = call(
        app.clone(),
        bare_req(&cookie, "DELETE", format!("/api/datasets/{ghost}/trash")),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&body)["code"], "not_found");
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t0003_trash_invisivel_e_trash_count() {
    let _guard = SERIAL.lock().await;
    let st = state().await;
    let app = routes::build(st.clone());
    let cookie = authed_cookie();
    let (status, _, body) = call(
        app.clone(),
        post_create("Lixeira H", &serde_json::json!(["a"]), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let created = json(&body);
    let ds = created["id"].as_str().expect("id").to_string();
    let ds_id: uuid::Uuid = ds.parse().expect("uuid");
    let class_id: uuid::Uuid = created["classes"][0]["id"]
        .as_str()
        .expect("classes[0].id")
        .parse()
        .expect("uuid");

    // img1 com a ÚNICA box autotracker; img2 com box manual.
    let img1 = insert_image(&st.pool, ds_id, "h1.jpg", 100).await;
    sqlx::query(
        "INSERT INTO boxes (image_id, class_id, x, y, w, h, origin) VALUES ($1,$2,0.5,0.5,0.2,0.2,'autotracker')",
    )
    .bind(img1)
    .bind(class_id)
    .execute(&st.pool)
    .await
    .expect("insert box autotracker");
    let img2 = insert_labeled_image(&st.pool, ds_id, class_id, "h2.jpg", 50).await;

    // Sanidade pré-delete: autoTracked true, trashCount 0.
    let (status, _, body) = call(
        app.clone(),
        bare_req(&cookie, "GET", format!("/api/datasets/{ds}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["autoTracked"], true);
    assert_eq!(json(&body)["trashCount"], 0);

    // Soft delete da img1.
    let (status, _, _) = call(
        app.clone(),
        bare_req(
            &cookie,
            "DELETE",
            format!("/api/datasets/{ds}/images/{img1}"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // (a) detail da deletada → 404.
    let (status, _, body) = call(
        app.clone(),
        bare_req(&cookie, "GET", format!("/api/datasets/{ds}/images/{img1}")),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&body)["code"], "not_found");

    // (b) /data da deletada → 404.
    let (status, _, body) = call(
        app.clone(),
        bare_req(
            &cookie,
            "GET",
            format!("/api/datasets/{ds}/images/{img1}/data"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&body)["code"], "not_found");

    // (c) PUT boxes na deletada → 404 (body válido passa da validação pura).
    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/images/{img1}/boxes"),
            serde_json::json!({"boxes": [{"classId": class_id, "x": 0.5, "y": 0.5, "w": 0.2, "h": 0.2}]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&body)["code"], "not_found");

    // (d) PUT caption na deletada → 404.
    let (status, _, body) = call(
        app.clone(),
        put_json(
            &cookie,
            "PUT",
            format!("/api/datasets/{ds}/images/{img1}/caption"),
            serde_json::json!({"text": "uma legenda"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&body)["code"], "not_found");

    // (e) list/get_one: trashCount=1, imagesCount=1.
    let (status, _, body) = call(
        app.clone(),
        bare_req(&cookie, "GET", format!("/api/datasets/{ds}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let one = json(&body);
    assert_eq!(one["trashCount"], 1);
    assert_eq!(one["imagesCount"], 1);
    // (f) autoTracked NÃO fica true por causa da imagem deletada.
    assert_eq!(one["autoTracked"], false);
    let (status, _, body) = call(
        app.clone(),
        bare_req(&cookie, "GET", "/api/datasets".to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let item = json(&body)
        .as_array()
        .expect("array")
        .iter()
        .find(|d| d["id"] == ds)
        .expect("dataset na lista")
        .clone();
    assert_eq!(item["trashCount"], 1);
    assert_eq!(item["autoTracked"], false);

    // (g) restore → trashCount=0 e detail volta a 200.
    let (status, _, _) = call(
        app.clone(),
        bare_req(
            &cookie,
            "POST",
            format!("/api/datasets/{ds}/images/{img1}/restore"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, body) = call(
        app.clone(),
        bare_req(&cookie, "GET", format!("/api/datasets/{ds}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["trashCount"], 0);
    assert_eq!(json(&body)["autoTracked"], true);
    let (status, _, _) = call(
        app.clone(),
        bare_req(&cookie, "GET", format!("/api/datasets/{ds}/images/{img1}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // img2 segue intacta o tempo todo.
    let (status, _, _) = call(
        app.clone(),
        bare_req(&cookie, "GET", format!("/api/datasets/{ds}/images/{img2}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}
