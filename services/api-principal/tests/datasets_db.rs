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

async fn counters_of(
    pool: &sqlx::PgPool,
    id: uuid::Uuid,
) -> (i32, i32, i64, String) {
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
    let t: String = sqlx::query_scalar(
        "SELECT to_regclass('public.videos')::text",
    )
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
    assert_eq!(counters_of(&st.pool, ds).await, (1, 0, 100, "needs_labeling".to_string()));

    sqlx::query(
        "INSERT INTO boxes (image_id, class_id, x, y, w, h, origin) VALUES ($1,$2,0.5,0.5,0.2,0.2,'manual')",
    )
    .bind(img1)
    .bind(class_id)
    .execute(&st.pool)
    .await
    .expect("insert box");
    assert_eq!(counters_of(&st.pool, ds).await, (1, 1, 100, "ready".to_string()));

    let _img2 = insert_image(&st.pool, ds, "b.jpg", 50).await;
    assert_eq!(counters_of(&st.pool, ds).await, (2, 1, 150, "in_progress".to_string()));

    // Cenário exato da T2: delete da imagem rotulada não pode errar nem
    // deixar estado intermediário persistido.
    sqlx::query("DELETE FROM images WHERE id = $1")
        .bind(img1)
        .execute(&st.pool)
        .await
        .expect("delete imagem rotulada");
    assert_eq!(counters_of(&st.pool, ds).await, (1, 0, 50, "needs_labeling".to_string()));
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
    assert_eq!(counters_of(&st.pool, ds).await, (1, 0, 80, "needs_labeling".to_string()));

    sqlx::query("INSERT INTO captions (image_id, text, origin) VALUES ($1,'um gato','manual')")
        .bind(img)
        .execute(&st.pool)
        .await
        .expect("insert caption");
    assert_eq!(counters_of(&st.pool, ds).await, (1, 1, 80, "ready".to_string()));

    // Box NÃO conta para format captions (R9 espelhado).
    sqlx::query(
        "INSERT INTO boxes (image_id, class_id, x, y, w, h, origin) VALUES ($1,$2,0.5,0.5,0.2,0.2,'manual')",
    )
    .bind(img)
    .bind(class_id)
    .execute(&st.pool)
    .await
    .expect("insert box");
    assert_eq!(counters_of(&st.pool, ds).await, (1, 1, 80, "ready".to_string()));

    // CHECK: caption com text vazio é erro; imagem segue rotulada pela anterior.
    let img2 = insert_image(&st.pool, ds, "d.jpg", 10).await;
    let bad = sqlx::query("INSERT INTO captions (image_id, text, origin) VALUES ($1,'','manual')")
        .bind(img2)
        .execute(&st.pool)
        .await;
    assert!(bad.is_err(), "caption vazio deveria violar o CHECK");
    assert_eq!(counters_of(&st.pool, ds).await, (2, 1, 90, "in_progress".to_string()));
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
    sqlx::query("SELECT pg_sleep(0.05)").execute(&st.pool).await.expect("pg_sleep");
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
        post_upload(&cookie, &ds2, boundary, multipart_body(boundary, &[("x.png", b"GIF89a-nao")])),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "invalid_request");

    // F4 end-to-end: conteúdo JPEG com nome `.png` ⇒ stored com canônico `.jpg`.
    let jpeg = jpeg_1x1();
    let (status, _, body) = call(
        app.clone(),
        post_upload(&cookie, &ds2, boundary, multipart_body(boundary, &[("foto.png", &jpeg)])),
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
        post_upload(&cookie, &ds, boundary, multipart_body(boundary, &[("a.png", &png_1x1())])),
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
            .header(http::header::CONTENT_TYPE, "multipart/form-data; boundary=X")
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
    st_fail.storage =
        std::sync::Arc::new(api_principal::storage::MockStorage::failing());
    let app_fail = routes::build(st_fail.clone());
    let ds_id2: uuid::Uuid = {
        let (status, _, body) = call(
            app_fail.clone(),
            post_create("Data Morto", &serde_json::json!(["a"]), &cookie),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        json(&body)["id"].as_str().expect("id").parse().expect("uuid")
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
