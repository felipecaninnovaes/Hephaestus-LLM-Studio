//! Embedding 3f.2 — `MockEmbedder` determinístico + `HttpEmbedder` (sem banco).
//!
//! Execução: `cargo test -p api-principal --test search_embed` (unidades
//! normais, sem `#[ignore]`, sem Postgres).

use api_principal::search::{EmbeddingError, EmbeddingPort, HttpEmbedder, MockEmbedder, DIM};

fn cos_sim(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (na * nb)
}

#[tokio::test]
async fn mock_deterministico() {
    let m = MockEmbedder::new();
    let a1 = m.embed_texts(&["abc".to_string()]).await.expect("mock");
    let a2 = m.embed_texts(&["abc".to_string()]).await.expect("mock");
    assert_eq!(a1.len(), 1);
    assert_eq!(a1[0].len(), DIM);
    assert_eq!(a1[0], a2[0], "mesmo texto → vetores bitwise iguais");
    let b = m.embed_texts(&["abd".to_string()]).await.expect("mock");
    assert_ne!(a1[0], b[0], "textos diferentes → vetores diferentes");
}

#[tokio::test]
async fn mock_normalizado_l2() {
    let m = MockEmbedder::new();
    let v = m.embed_texts(&["abc".to_string()]).await.expect("mock");
    let s: f32 = v[0].iter().map(|x| x * x).sum();
    assert!((s - 1.0).abs() < 1e-6, "Σx² == 1.0, obteve {s}");
}

#[tokio::test]
async fn mock_ranking_by_image_top1() {
    let m = MockEmbedder::new();
    let id_a = uuid::Uuid::new_v4();
    let id_b = uuid::Uuid::new_v4();
    let img_a = vec![0x89u8, 0x50, 0x4e, 0x47, 0x01, 0x02, 0x03];
    let img_b = vec![0x89u8, 0x50, 0x4e, 0x47, 0x09, 0x08, 0x07];
    let va = m
        .embed_images(&[(id_a, img_a.clone())])
        .await
        .expect("mock")[0]
        .1
        .clone();
    let va2 = m.embed_images(&[(id_a, img_a)]).await.expect("mock")[0]
        .1
        .clone();
    let vb = m.embed_images(&[(id_b, img_b)]).await.expect("mock")[0]
        .1
        .clone();
    let self_sim = cos_sim(&va, &va2);
    assert!(
        (self_sim - 1.0).abs() < 1e-6,
        "auto-sim == 1.0, obteve {self_sim}"
    );
    assert!(self_sim > cos_sim(&va, &vb), "top-1 é a própria imagem");
}

#[tokio::test]
async fn mock_golden_values() {
    // Prova de paridade com o serve.py do spike: payload fixo, f32 bitwise.
    let payload = b"SPIKE-VECTOR-ALIGNED-PAYLOAD-000".repeat(64);
    let m = MockEmbedder::new();
    let id = uuid::Uuid::nil();
    let v = m.embed_images(&[(id, payload)]).await.expect("mock")[0]
        .1
        .clone();
    assert_eq!(v.len(), DIM);
    let expected: [(usize, f32); 6] = [
        (0, 0.035050030797719955_f32),
        (1, 0.023601748049259186_f32),
        (2, -0.012896332889795303_f32),
        (3, 0.014493524096906185_f32),
        (510, 0.06539401412010193_f32),
        (511, -0.04202665761113167_f32),
    ];
    for (i, e) in expected {
        assert_eq!(
            v[i].to_bits(),
            e.to_bits(),
            "golden diverge no índice {i}: obteve {:?} esperado {:?}",
            v[i],
            e
        );
    }
}

async fn spawn_mock_server() -> (String, tokio::task::JoinHandle<()>) {
    let app = axum::Router::new()
        .route(
            "/embed",
            axum::routing::post(|body: axum::Json<serde_json::Value>| async move {
                let first_id = body["items"][0]["id"].clone();
                let mut vector = vec![0.0f64; DIM];
                vector[0] = 0.5;
                vector[1] = 0.5;
                axum::Json(serde_json::json!({
                    "items": [{ "id": first_id, "vector": vector, "dim": DIM }]
                }))
            }),
        )
        .route(
            "/embed-text",
            axum::routing::post(|_: axum::Json<serde_json::Value>| async move {
                let mut vector = vec![0.0f64; DIM];
                vector[0] = 0.25;
                vector[1] = 0.75;
                axum::Json(serde_json::json!({
                    "items": [{ "id": 0, "vector": vector, "dim": DIM }]
                }))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind efêmero");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve mock");
    });
    (format!("http://{addr}"), handle)
}

#[tokio::test]
async fn http_embedder_roundtrip() {
    let (url, _srv) = spawn_mock_server().await;
    let e = HttpEmbedder::new(url, "ViT-B-32".to_string());
    let id = uuid::Uuid::new_v4();
    let imgs = e
        .embed_images(&[(id, vec![1u8, 2, 3])])
        .await
        .expect("roundtrip images");
    assert_eq!(imgs.len(), 1);
    assert_eq!(imgs[0].0, id);
    assert_eq!(imgs[0].1.len(), DIM);
    assert_eq!(imgs[0].1[0], 0.5_f32);
    assert_eq!(imgs[0].1[1], 0.5_f32);
    assert_eq!(imgs[0].1[2], 0.0_f32);
    let texts = e
        .embed_texts(&["olá".to_string()])
        .await
        .expect("roundtrip texts");
    assert_eq!(texts.len(), 1);
    assert_eq!(texts[0].len(), DIM);
    assert_eq!(texts[0][0], 0.25_f32);
    assert_eq!(texts[0][1], 0.75_f32);
}

#[tokio::test]
async fn http_embedder_unavailable() {
    let e = HttpEmbedder::new("http://127.0.0.1:1".to_string(), "ViT-B-32".to_string());
    let err = e
        .embed_texts(&["x".to_string()])
        .await
        .expect_err("porta inválida → Unavailable");
    assert!(
        matches!(err, EmbeddingError::Unavailable(_)),
        "esperado Unavailable, obteve {err:?}"
    );
}

#[tokio::test]
async fn embedding_error_mapeia_503_no_envelope() {
    // Cobertura do 503 da 3f.5 sem servidor falso: o mapeamento é função
    // pura (`embedding_error_response`) — ambas as variantes caem na mesma
    // família (o envelope `{code, message}` não tem campo de detalhe).
    use http_body_util::BodyExt;
    for e in [
        EmbeddingError::Unavailable("caiu".to_string()),
        EmbeddingError::InvalidResponse("shape".to_string()),
    ] {
        let resp = api_principal::search::handlers::embedding_error_response(&e);
        assert_eq!(resp.status(), http::StatusCode::SERVICE_UNAVAILABLE);
        let body = resp
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes()
            .to_vec();
        let v: serde_json::Value = serde_json::from_slice(&body).expect("corpo JSON");
        assert_eq!(v["code"], "embedding_unavailable");
        assert!(v["message"].is_string());
    }
}
