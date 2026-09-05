//! SPIKE 3b.0 C6 — prova que as 6 rotas novas coexistem com as 4 existentes no
//! axum 0.7.9 / matchit 0.7.3 sem panic de montagem, e que `/…/data` cai no
//! handler certo (não no fallback bodyless). Handlers-stub + oneshot: sem banco/auth.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use tower::ServiceExt; // oneshot

/// Fallback de rota com envelope (o /…/data com :imageId inexistente deve cair AQUI
/// via handler, não no fallback bodyless do router — mas no spike não há banco,
/// então o que se prova é o ROTEAMENTO do path ao handler, não o conteúdo 404).
async fn not_found_envelope() -> Response {
    (StatusCode::NOT_FOUND, r#"{"error":{"code":"not_found"}}"#).into_response()
}

/// Router que espelha a montagem D9: rota exata vence, wildcard não colide.
fn build_spike_router() -> axum::Router {
    axum::Router::new()
        // --- 4 rotas core já existentes (3a) ---
        .route("/api/datasets", get(|| async { "LIST" }).post(|| async { "CREATE" }))
        .route(
            "/api/datasets/:id",
            get(|| async { "GET_DS" }).delete(|| async { "DELETE_DS" }),
        )
        // --- 6 rotas novas da 3b ---
        .route("/api/datasets/:id/upload", post(|| async { "UPLOAD" }))
        .route("/api/datasets/:id/images", get(|| async { "IMAGES_LIST" }))
        .route("/api/datasets/:id/images/:imageId", get(|| async { not_found_envelope().await }))
        .route(
            "/api/datasets/:id/images/:imageId/data",
            get(|| async { "IMAGE_DATA" }),
        )
        .route(
            "/api/datasets/:id/images/:imageId/boxes",
            put(|| async { "PUT_BOXES" }),
        )
        .route(
            "/api/datasets/:id/images/:imageId/caption",
            put(|| async { "PUT_CAPTION" }),
        )
        .fallback(|| async { (StatusCode::NOT_FOUND, "FALLBACK_BODYLESS") })
}

async fn probe(app: &axum::Router, method: &str, uri: &str) -> (u16, String) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status().as_u16();
    let body = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes()
        .iter()
        .map(|b| *b as char)
        .collect::<String>();
    (status, body)
}

#[tokio::main]
async fn main() {
    let app = build_spike_router();
    println!("axum/matchit montou as 10 rotas sem panic (boot OK).");

    let cases: &[(&str, &str, &str)] = &[
        ("POST", "/api/datasets/d/upload", "UPLOAD"),
        ("GET", "/api/datasets/d/images", "IMAGES_LIST"),
        ("GET", "/api/datasets/d/images/img/data", "IMAGE_DATA"),
        ("PUT", "/api/datasets/d/images/img/boxes", "PUT_BOXES"),
        ("PUT", "/api/datasets/d/images/img/caption", "PUT_CAPTION"),
    ];
    let mut pass = 0;
    for (m, uri, want) in cases {
        let (status, body) = probe(&app, m, uri).await;
        let ok = status == 200 && body == *want;
        println!("[C6] {m} {uri} → {status} \"{body}\" (queria {want}) {}", if ok { "OK" } else { "FAIL" });
        if ok {
            pass += 1;
        }
    }
    // rota aninhada com dois wildcards (/:id e /:imageId) — o oneshot resolve
    // /api/datasets/d/images/img (sem /data) no handler de DETAIL, não no data nem no fallback.
    let (s2, b2) = probe(&app, "GET", "/api/datasets/d/images/img").await;
    let nested_ok = b2 != "IMAGE_DATA" && b2 != "FALLBACK_BODYLESS" && b2.contains("not_found");
    println!("[C6] GET …/images/:imageId (detail) → {s2} \"{b2}\" roteado ao handler de detail (não data/fallback) {}", if nested_ok { "OK" } else { "FAIL" });

    // caminho NÃO roteado → fallback (o critério quer /data ROTAR ao handler, não ao fallback).
    let (s3, b3) = probe(&app, "GET", "/api/datasets/d/nao-existe").await;
    println!("[C6] GET caminho inexistente → {s3} \"{b3}\" (fallback bodyless esperado)");

    let total = cases.len() + 1;
    println!("\n===== C6: {}/{} rotas novas roteadas corretamente (sem panic, /data cai no handler) =====", pass + nested_ok as usize, total);
}
