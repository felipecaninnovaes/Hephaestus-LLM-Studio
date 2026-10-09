//! Fluxo de envio ao ComfyUI contra um servidor HTTP fake que implementa o
//! protocolo §1 do contrato (init → PUT em partes → commit; DELETE aborta).

mod common;

use std::time::Duration;

use api_principal::integrations::comfyui::client::{Remote, TransferError};
use common::comfy_fake::{serve, Fake, TOKEN};

fn remote(url: &str) -> Remote {
    Remote::new(url, TOKEN).with_retry_delay(Duration::from_millis(1))
}

fn data(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 7 % 251) as u8).collect()
}

async fn run(r: &Remote, bytes: &[u8], total: u64) -> (Result<String, TransferError>, Vec<u64>) {
    let mut seen = Vec::new();
    let mut reader = bytes;
    let res = r
        .transfer("x.safetensors", false, total, &mut reader, &mut |n| {
            seen.push(n)
        })
        .await;
    (res, seen)
}

#[tokio::test]
async fn parte_tem_o_tamanho_do_init_e_o_arquivo_chega_integro() {
    let (url, fake) = serve(Fake {
        chunk_size: 10,
        ..Default::default()
    })
    .await;
    let d = data(25);
    let (res, seen) = run(&remote(&url), &d, 25).await;
    assert_eq!(res.unwrap(), "/models/loras/hephaestus/x.safetensors");
    let f = fake.lock().unwrap();
    assert_eq!(f.puts, vec![(0, 10), (10, 10), (20, 5)]);
    assert_eq!(f.committed.as_deref(), Some(&d[..]));
    assert_eq!(seen, vec![10, 20, 25]);
    assert!(!f.deleted);
}

#[tokio::test]
async fn retoma_pelo_expected_offset_apos_falha_no_meio_da_parte() {
    let (url, fake) = serve(Fake {
        chunk_size: 10,
        partial_then_500_once: true,
        ..Default::default()
    })
    .await;
    let d = data(25);
    let (res, _) = run(&remote(&url), &d, 25).await;
    res.unwrap();
    let f = fake.lock().unwrap();
    // 1º PUT (0,10) cai após gravar 5; a retentativa começa em 0 → 409 expected=5;
    // o cliente reenvia só o resto (5, 5) sem duplicar bytes.
    assert_eq!(&f.puts[..3], &[(0, 10), (0, 10), (5, 5)]);
    assert_eq!(f.committed.as_deref(), Some(&d[..]));
}

#[tokio::test]
async fn sha256_divergente_falha_e_remove_o_upload_remoto() {
    let (url, fake) = serve(Fake {
        chunk_size: 10,
        corrupt: true,
        ..Default::default()
    })
    .await;
    let d = data(15);
    let (res, _) = run(&remote(&url), &d, 15).await;
    match res {
        Err(TransferError::Failed(f)) => {
            assert!(f.message.contains("sha256"), "{f:?}");
            assert_eq!(f.code, "checksum_mismatch");
        }
        other => panic!("esperava Failed, veio {other:?}"),
    }
    let f = fake.lock().unwrap();
    assert!(f.deleted, "DELETE remoto");
    assert!(f.committed.is_none());
}

#[tokio::test]
async fn erro_4xx_e_definitivo_sem_retry_e_aborta_o_upload() {
    let (url, fake) = serve(Fake {
        chunk_size: 10,
        put_always: Some((413, "chunk_too_large")),
        ..Default::default()
    })
    .await;
    let (res, _) = run(&remote(&url), &data(15), 15).await;
    assert!(matches!(res, Err(TransferError::Failed(_))));
    let f = fake.lock().unwrap();
    assert_eq!(f.puts.len(), 1, "4xx não é repetido");
    assert!(f.deleted);
}

#[tokio::test]
async fn erro_5xx_persistente_tenta_3_vezes_e_falha() {
    let (url, fake) = serve(Fake {
        chunk_size: 10,
        put_always: Some((502, "bad_gateway")),
        ..Default::default()
    })
    .await;
    let (res, _) = run(&remote(&url), &data(15), 15).await;
    assert!(matches!(res, Err(TransferError::Failed(_))));
    let f = fake.lock().unwrap();
    assert_eq!(f.puts.len(), 3);
    assert!(f.deleted);
}

#[tokio::test]
async fn arquivo_existente_falha_no_init_sem_enviar_nada() {
    let (url, fake) = serve(Fake {
        chunk_size: 10,
        file_exists: true,
        ..Default::default()
    })
    .await;
    let (res, _) = run(&remote(&url), &data(15), 15).await;
    match res {
        Err(TransferError::Failed(f)) => {
            assert!(f.message.contains("já existe"), "{f:?}");
            assert_eq!(f.code, "file_exists");
        }
        other => panic!("{other:?}"),
    }
    assert!(fake.lock().unwrap().puts.is_empty());
}

#[tokio::test]
async fn upload_esquecido_pelo_destino_pede_recomeco() {
    let (url, _) = serve(Fake {
        chunk_size: 10,
        forget_uploads: true,
        ..Default::default()
    })
    .await;
    let (res, _) = run(&remote(&url), &data(15), 15).await;
    assert_eq!(res, Err(TransferError::UploadLost));
}

#[tokio::test]
async fn origem_maior_que_o_tamanho_registrado_nao_vira_arquivo_truncado() {
    let (url, fake) = serve(Fake {
        chunk_size: 10,
        ..Default::default()
    })
    .await;
    let (res, _) = run(&remote(&url), &data(20), 15).await;
    assert!(matches!(res, Err(TransferError::Failed(_))));
    let f = fake.lock().unwrap();
    assert!(f.committed.is_none());
    assert!(f.deleted);
}

#[tokio::test]
async fn origem_menor_que_o_tamanho_registrado_falha() {
    let (url, fake) = serve(Fake {
        chunk_size: 10,
        ..Default::default()
    })
    .await;
    let (res, _) = run(&remote(&url), &data(12), 15).await;
    assert!(matches!(res, Err(TransferError::Failed(_))));
    assert!(fake.lock().unwrap().deleted);
}

#[tokio::test]
async fn health_ok_e_token_errado_em_ptbr() {
    let (url, _) = serve(Fake::default()).await;
    let h = remote(&url).health().await.unwrap();
    assert_eq!((h.version.as_str(), h.chunk_size), ("1", 33554432));
    let err = Remote::new(&url, "errado").health().await.unwrap_err();
    assert!(err.contains("token"), "{err}");
    let err = Remote::new("http://127.0.0.1:1", TOKEN)
        .health()
        .await
        .unwrap_err();
    assert!(err.contains("conexão"), "{err}");
}

/// Servidor que responde `/hephaestus/health` com o que `make` produzir e
/// conta quantas vezes `/alvo` foi alcançado.
async fn health_server<F>(make: F) -> (String, std::sync::Arc<std::sync::atomic::AtomicU32>)
where
    F: Fn() -> axum::response::Response + Clone + Send + Sync + 'static,
{
    use axum::routing::get;
    use std::sync::atomic::{AtomicU32, Ordering};
    let hits = std::sync::Arc::new(AtomicU32::new(0));
    let h2 = hits.clone();
    let app = axum::Router::new()
        .route("/hephaestus/health", get(move || async move { make() }))
        .route(
            "/alvo",
            get(move || async move {
                h2.fetch_add(1, Ordering::Relaxed);
                "x"
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, hits)
}

#[tokio::test]
async fn redirect_do_destino_nao_e_seguido_e_falha_com_mensagem_clara() {
    use axum::response::IntoResponse;
    use std::sync::atomic::Ordering;
    let (url, hits) = health_server(|| {
        (
            axum::http::StatusCode::FOUND,
            [(axum::http::header::LOCATION, "/alvo")],
        )
            .into_response()
    })
    .await;
    let err = remote(&url).health().await.unwrap_err();
    assert!(err.contains("redirecionamento"), "{err}");
    assert_eq!(hits.load(Ordering::Relaxed), 0, "o redirect foi seguido");
}

#[tokio::test]
async fn corpo_de_erro_gigante_do_destino_e_limitado_sem_vazar_no_erro() {
    use axum::response::IntoResponse;
    let (url, _) = health_server(|| {
        (
            axum::http::StatusCode::BAD_GATEWAY,
            "A".repeat(5 * 1024 * 1024),
        )
            .into_response()
    })
    .await;
    let err = remote(&url).health().await.unwrap_err();
    assert!(err.contains("502") && err.len() < 200, "{err}");
}

#[tokio::test]
async fn corpo_de_sucesso_gigante_e_resposta_invalida() {
    use axum::response::IntoResponse;
    let big = format!(
        r#"{{"version":"1","chunkSize":1,"pad":"{}"}}"#,
        "A".repeat(200 * 1024)
    );
    let (url, _) = health_server(move || big.clone().into_response()).await;
    let err = remote(&url).health().await.unwrap_err();
    assert!(err.contains("resposta inválida"), "{err}");
}

#[test]
fn debug_do_remote_nao_vaza_o_token() {
    let dbg = format!("{:?}", Remote::new("http://h", TOKEN));
    assert!(!dbg.contains(TOKEN), "{dbg}");
}
