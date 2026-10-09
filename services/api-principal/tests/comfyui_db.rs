//! Integração comfyui × Postgres (--ignored): CRUD de destinos, cifra do token,
//! validação, `exports` ambíguo, envio ponta a ponta contra o servidor fake e
//! recovery de exports pendentes no boot.
//!
//! Execução: `DATABASE_URL=postgres://…/studio_test_comfy cargo test -p
//! api-principal --test comfyui_db -- --ignored` (banco efêmero; a guarda
//! recusa o banco de dev `studio` antes de conectar — o setup faz DELETEs).

mod common;

use api_principal::auth::{routes, session, AppState};
use api_principal::integrations::comfyui::{client::Remote, repository, runner, source};
use axum::body::Body;
use common::comfy_fake::{serve, Fake, TOKEN};
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

const TEST_SECRET: [u8; 32] = [0x42; 32];
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn assert_test_db_url(url: &str) {
    let db = url
        .rsplit('/')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("");
    assert!(
        db.starts_with("studio_test"),
        "HARNESS DE TESTE RECUSANDO BANCO PERIGOSO: use studio_test* — nunca o DB de dev 'studio' (banco na URL: '{db}')"
    );
}

#[test]
#[should_panic(expected = "HARNESS DE TESTE RECUSANDO BANCO PERIGOSO")]
fn guarda_recusa_banco_de_dev() {
    assert_test_db_url("postgres://studio:studio@localhost:5432/studio");
}

async fn state(storage: api_principal::storage::MockStorage) -> AppState {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL é obrigatório");
    assert_test_db_url(&url);
    let pool = sqlx::PgPool::connect(&url).await.expect("conectar");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations");
    sqlx::query("DELETE FROM comfy_exports")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM comfy_targets")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM models WHERE s3_key LIKE 'models/diffusion/comfy-test/%'")
        .execute(&pool)
        .await
        .unwrap();
    AppState {
        pool,
        jwt_secret: TEST_SECRET,
        secure_cookie: false,
        setup_required: false,
        storage: std::sync::Arc::new(storage),
        storage_config: api_principal::storage::MockStorage::test_config(),
        embedder: std::sync::Arc::new(api_principal::search::MockEmbedder::new()),
        embedding_model: "ViT-B-32".to_string(),
        manager: std::sync::Arc::new(api_principal::jobs::manager_client::MockManager::default()),
        model_download_allowed_hosts: vec![],
        job_events: api_principal::jobs::events_hub::JobEventsHub::new(),
        thumb_semaphore: api_principal::datasets::thumb::default_thumb_semaphore(),
    }
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let (token, _) = session::issue_jwt(Uuid::new_v4(), &TEST_SECRET);
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header(http::header::COOKIE, format!("heph_session={token}"));
    let body = match body {
        Some(b) => {
            req = req.header(http::header::CONTENT_TYPE, "application/json");
            Body::from(b.to_string())
        }
        None => Body::empty(),
    };
    let resp = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

const BASE: &str = "/api/integrations/comfyui";

async fn create_target(app: &axum::Router, name: &str, url: &str) -> Value {
    let (st, body) = call(
        app,
        "POST",
        &format!("{BASE}/targets"),
        Some(json!({"name": name, "baseUrl": url, "token": TOKEN})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{body}");
    body
}

#[tokio::test]
#[ignore = "requer Postgres efêmero (studio_test*)"]
async fn crud_nunca_devolve_o_token_e_cifra_no_banco() {
    let _g = SERIAL.lock().await;
    let st = state(Default::default()).await;
    let app = routes::build(st.clone());

    let t = create_target(&app, "RunPod", "https://pod.proxy.runpod.net/").await;
    assert_eq!(t["baseUrl"], "https://pod.proxy.runpod.net");
    assert_eq!(t["hasToken"], true);
    assert!(!t.to_string().contains(TOKEN), "token não pode voltar");
    let id = t["id"].as_str().unwrap().to_string();

    // No banco: só ciphertext; decifra de volta com o segredo do BFF.
    let row = repository::get_target(&st.pool, id.parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert!(!row
        .token_ciphertext
        .windows(TOKEN.len())
        .any(|w| w == TOKEN.as_bytes()));
    let plain = api_principal::integrations::comfyui::crypto::decrypt(
        &TEST_SECRET,
        &row.token_ciphertext,
        &row.token_nonce,
    )
    .unwrap();
    assert_eq!(plain, TOKEN);

    let (s, list) = call(&app, "GET", &format!("{BASE}/targets"), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert!(!list.to_string().contains(TOKEN));

    // Nome duplicado ⇒ 409.
    let (s, b) = call(
        &app,
        "POST",
        &format!("{BASE}/targets"),
        Some(json!({"name": "RunPod", "baseUrl": "http://x", "token": "t"})),
    )
    .await;
    assert_eq!(
        (s, b["code"].as_str()),
        (StatusCode::CONFLICT, Some("comfy_target_name_taken"))
    );

    // PATCH sem token mantém o token; com token troca.
    let (s, p) = call(
        &app,
        "PATCH",
        &format!("{BASE}/targets/{id}"),
        Some(json!({"name": "Pod 2"})),
    )
    .await;
    assert_eq!((s, p["name"].as_str()), (StatusCode::OK, Some("Pod 2")));
    let same = repository::get_target(&st.pool, id.parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(same.token_ciphertext, row.token_ciphertext);
    let (s, _) = call(
        &app,
        "PATCH",
        &format!("{BASE}/targets/{id}"),
        Some(json!({"token": "novo-token"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let changed = repository::get_target(&st.pool, id.parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    let plain = api_principal::integrations::comfyui::crypto::decrypt(
        &TEST_SECRET,
        &changed.token_ciphertext,
        &changed.token_nonce,
    )
    .unwrap();
    assert_eq!(plain, "novo-token");

    let (s, _) = call(&app, "DELETE", &format!("{BASE}/targets/{id}"), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = call(&app, "DELETE", &format!("{BASE}/targets/{id}"), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "requer Postgres efêmero (studio_test*)"]
async fn base_url_e_corpos_invalidos_sao_400() {
    let _g = SERIAL.lock().await;
    let app = routes::build(state(Default::default()).await);
    for bad in [
        "ftp://h",
        "http://u:p@h",
        "javascript:x",
        "nada",
        "http://h/?q=1",
    ] {
        let (s, b) = call(
            &app,
            "POST",
            &format!("{BASE}/targets"),
            Some(json!({"name": "n", "baseUrl": bad, "token": "t"})),
        )
        .await;
        assert_eq!(
            (s, b["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some("invalid_request")),
            "{bad}"
        );
    }
    let (s, _) = call(
        &app,
        "POST",
        &format!("{BASE}/targets"),
        Some(json!({"name": " ", "baseUrl": "http://h", "token": "t"})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = call(
        &app,
        "POST",
        &format!("{BASE}/targets"),
        Some(json!({"name": "n", "baseUrl": "http://h"})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let t = create_target(&app, "ok", "http://h").await;
    let id = t["id"].as_str().unwrap();
    let (s, _) = call(
        &app,
        "PATCH",
        &format!("{BASE}/targets/{id}"),
        Some(json!({"baseUrl": "ftp://h"})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[ignore = "requer Postgres efêmero (studio_test*)"]
async fn exports_com_corpo_ambiguo_e_400() {
    let _g = SERIAL.lock().await;
    let app = routes::build(state(Default::default()).await);
    let t = create_target(&app, "t", "http://127.0.0.1:1").await;
    let uri = format!("{BASE}/targets/{}/exports", t["id"].as_str().unwrap());
    let (m, j, a) = (
        Uuid::new_v4().to_string(),
        Uuid::new_v4().to_string(),
        Uuid::new_v4().to_string(),
    );
    for body in [
        json!({}),
        json!({"modelId": m, "jobId": j, "artifactId": a}),
        json!({"modelId": m, "jobId": j}),
        json!({"jobId": j}),
        json!({"artifactId": a}),
        json!({"modelId": "não-uuid"}),
    ] {
        let (s, b) = call(&app, "POST", &uri, Some(body.clone())).await;
        assert_eq!(
            (s, b["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some("invalid_request")),
            "{body}"
        );
    }
    // Forma válida mas modelo inexistente ⇒ 404; destino inexistente ⇒ 404.
    let (s, _) = call(&app, "POST", &uri, Some(json!({"modelId": m}))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = call(
        &app,
        "POST",
        &format!("{BASE}/targets/{}/exports", Uuid::new_v4()),
        Some(json!({"modelId": m})),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

/// LoRA safetensors com prefixo PEFT e alpha ≠ rank no metadata.
fn lora_bytes() -> (Vec<u8>, Vec<u8>) {
    let data: Vec<u8> = (0..128u32).map(|i| (i * 3 % 256) as u8).collect();
    let header = json!({
        "__metadata__": {"lora_alpha": "8"},
        "base_model.model.blk.lora_A.weight": {"dtype": "F32", "shape": [4, 4], "data_offsets": [0, 64]},
        "base_model.model.blk.lora_B.weight": {"dtype": "F32", "shape": [4, 4], "data_offsets": [64, 128]},
    });
    let h = serde_json::to_vec(&header).unwrap();
    let mut f = (h.len() as u64).to_le_bytes().to_vec();
    f.extend(h);
    f.extend_from_slice(&data);
    (f, data)
}

async fn insert_lora_model(st: &AppState, key: &str, bytes: i64, kind: &str, arch: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO models (id, engine, name, s3_key, source, hash, bytes, kind, arch) \
         VALUES ($1, 'diffusion', $2, $3, 'upload', $4, $5, $6, $7)",
    )
    .bind(id)
    .bind("meu lora v1.safetensors")
    .bind(key)
    .bind("0".repeat(32))
    .bind(bytes)
    .bind(kind)
    .bind(arch)
    .execute(&st.pool)
    .await
    .unwrap();
    id
}

async fn wait_final(app: &axum::Router, id: &str) -> Value {
    for _ in 0..200 {
        let (_, e) = call(app, "GET", &format!("{BASE}/exports/{id}"), None).await;
        if matches!(e["status"].as_str(), Some("done" | "failed")) {
            return e;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("export não terminou");
}

#[tokio::test]
#[ignore = "requer Postgres efêmero (studio_test*)"]
async fn envio_ponta_a_ponta_reescreve_header_e_confere_sha() {
    let _g = SERIAL.lock().await;
    let storage = api_principal::storage::MockStorage::new();
    let (file, data) = lora_bytes();
    let key = format!(
        "models/diffusion/comfy-test/{}/lora.safetensors",
        Uuid::new_v4()
    );
    storage.put_bytes(&key, file.clone()).await;
    let st = state(storage).await;
    let app = routes::build(st.clone());
    let model = insert_lora_model(&st, &key, file.len() as i64, "lora", "flux-2-klein-4b").await;
    let (url, fake) = serve(Fake {
        chunk_size: 100,
        ..Default::default()
    })
    .await;
    let t = create_target(&app, "fake", &url).await;
    let tid = t["id"].as_str().unwrap();

    // Teste de conexão grava o resultado.
    let (s, checked) = call(&app, "POST", &format!("{BASE}/targets/{tid}/test"), None).await;
    assert_eq!(
        (s, checked["lastCheckOk"].clone()),
        (StatusCode::OK, json!(true))
    );

    let (s, exp) = call(
        &app,
        "POST",
        &format!("{BASE}/targets/{tid}/exports"),
        Some(json!({"modelId": model})),
    )
    .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{exp}");
    assert_eq!(exp["filename"], "meu_lora_v1.safetensors");
    let done = wait_final(&app, exp["id"].as_str().unwrap()).await;
    assert_eq!(done["status"], "done", "{done}");
    assert_eq!(done["remotePath"], "/models/loras/hephaestus/x.safetensors");
    assert_eq!(done["bytesSent"], done["bytesTotal"]);

    let f = fake.lock().unwrap();
    let sent = f.committed.clone().expect("commit");
    assert_eq!(done["bytesTotal"].as_i64().unwrap(), sent.len() as i64);
    let n = u64::from_le_bytes(sent[..8].try_into().unwrap()) as usize;
    let header: Value = serde_json::from_slice(&sent[8..8 + n]).unwrap();
    assert!(
        header.get("blk.lora_A.weight").is_some(),
        "prefixo PEFT removido: {header}"
    );
    assert_eq!(header["blk.alpha"]["data_offsets"], json!([128, 132]));
    assert_eq!(&sent[8 + n..8 + n + 128], &data[..], "tensores byte a byte");
    assert_eq!(&sent[8 + n + 128..], &8f32.to_le_bytes());
    assert!(
        f.puts.iter().all(|(_, len)| *len <= 100),
        "partes ≤ chunkSize do init"
    );
    drop(f);

    let (_, list) = call(&app, "GET", &format!("{BASE}/exports?limit=5"), None).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
}

#[tokio::test]
#[ignore = "requer Postgres efêmero (studio_test*)"]
async fn falha_marca_failed_e_apaga_upload_remoto() {
    let _g = SERIAL.lock().await;
    let storage = api_principal::storage::MockStorage::new();
    let (file, _) = lora_bytes();
    let key = format!(
        "models/diffusion/comfy-test/{}/lora.safetensors",
        Uuid::new_v4()
    );
    storage.put_bytes(&key, file.clone()).await;
    let st = state(storage).await;
    let app = routes::build(st.clone());
    let model = insert_lora_model(&st, &key, file.len() as i64, "lora", "sdxl").await;
    let (url, fake) = serve(Fake {
        chunk_size: 100,
        corrupt: true,
        ..Default::default()
    })
    .await;
    let t = create_target(&app, "fake", &url).await;
    let (_, exp) = call(
        &app,
        "POST",
        &format!("{BASE}/targets/{}/exports", t["id"].as_str().unwrap()),
        Some(json!({"modelId": model})),
    )
    .await;
    let done = wait_final(&app, exp["id"].as_str().unwrap()).await;
    assert_eq!(done["status"], "failed");
    assert!(done["error"].as_str().unwrap().contains("sha256"), "{done}");
    assert_eq!(done["errorCode"], "checksum_mismatch");
    assert!(fake.lock().unwrap().deleted);
}

#[tokio::test]
#[ignore = "requer Postgres efêmero (studio_test*)"]
async fn origem_que_nao_e_lora_e_422() {
    let _g = SERIAL.lock().await;
    let st = state(Default::default()).await;
    let app = routes::build(st.clone());
    let key = format!(
        "models/diffusion/comfy-test/{}/ckpt.safetensors",
        Uuid::new_v4()
    );
    let model = insert_lora_model(&st, &key, 10, "checkpoint", "sdxl").await;
    let t = create_target(&app, "t", "http://127.0.0.1:1").await;
    let (s, b) = call(
        &app,
        "POST",
        &format!("{BASE}/targets/{}/exports", t["id"].as_str().unwrap()),
        Some(json!({"modelId": model})),
    )
    .await;
    assert_eq!(
        (s, b["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("export_unsupported"))
    );
}

#[tokio::test]
#[ignore = "requer Postgres efêmero (studio_test*)"]
async fn exports_pendentes_viram_failed_no_boot() {
    let _g = SERIAL.lock().await;
    let st = state(Default::default()).await;
    let app = routes::build(st.clone());
    let t = create_target(&app, "t", "http://127.0.0.1:1").await;
    let tid: Uuid = t["id"].as_str().unwrap().parse().unwrap();
    let mut ids = Vec::new();
    for _ in 0..3 {
        let id = Uuid::new_v4();
        repository::insert_export(
            &st.pool,
            &repository::NewExport {
                id,
                target_id: tid,
                model_id: Some(Uuid::new_v4()),
                job_id: None,
                artifact_id: None,
                filename: "a.safetensors",
                bytes_total: 10,
            },
        )
        .await
        .unwrap();
        ids.push(id);
    }
    repository::mark_uploading(&st.pool, ids[1], 10)
        .await
        .unwrap();
    repository::mark_uploading(&st.pool, ids[2], 10)
        .await
        .unwrap();
    repository::finish_done(&st.pool, ids[2], 10, "/x")
        .await
        .unwrap();

    assert_eq!(runner::fail_interrupted(&st.pool).await.unwrap(), 2);
    for (i, expected) in [(0, "failed"), (1, "failed"), (2, "done")] {
        let (_, e) = call(&app, "GET", &format!("{BASE}/exports/{}", ids[i]), None).await;
        assert_eq!(e["status"], expected);
        if expected == "failed" {
            assert_eq!(e["errorCode"], "interrupted");
            assert_eq!(e["error"], "interrompido por reinício do servidor");
        }
    }
}

#[tokio::test]
#[ignore = "requer Postgres efêmero (studio_test*)"]
async fn run_export_direto_com_origem_vazia_falha_legivel() {
    let _g = SERIAL.lock().await;
    let st = state(Default::default()).await;
    let app = routes::build(st.clone());
    let t = create_target(&app, "t", "http://127.0.0.1:1").await;
    let tid: Uuid = t["id"].as_str().unwrap().parse().unwrap();
    let id = Uuid::new_v4();
    repository::insert_export(
        &st.pool,
        &repository::NewExport {
            id,
            target_id: tid,
            model_id: Some(Uuid::new_v4()),
            job_id: None,
            artifact_id: None,
            filename: "a.safetensors",
            bytes_total: 0,
        },
    )
    .await
    .unwrap();
    let job = runner::ExportJob {
        export_id: id,
        remote: Remote::new("http://127.0.0.1:1", "t"),
        source: source::ResolvedSource {
            model_id: None,
            job_id: None,
            artifact_id: None,
            key: "k".into(),
            filename: "a.safetensors".into(),
            bytes: 0,
            arch: None,
            alpha: None,
        },
        overwrite: false,
    };
    runner::run_export(st.pool.clone(), st.storage.clone(), job).await;
    let (_, e) = call(&app, "GET", &format!("{BASE}/exports/{id}"), None).await;
    assert_eq!(e["status"], "failed");
    assert!(e["error"].as_str().unwrap().contains("vazio"));
    assert_eq!(e["errorCode"], "source_invalid");
}
