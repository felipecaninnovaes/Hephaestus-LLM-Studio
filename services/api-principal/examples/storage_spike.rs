//! SPIKE 3b.0 — harness descartável (NÃO mergear). Matriz de critérios da ADR-0003.
//!
//! Roda contra o SeaweedFS do `infra/compose.spike.yaml` (`http://localhost:8333`).
//! Uso: `cargo run --example storage_spike --release` (dev-deps: aws-sdk-s3 sem aws-config).
//!
//! Cobre os critérios binários que precisam do SDK real:
//!   C2 PUT 1 KiB + 512 MiB sem trilha aws-chunked/STREAMING (via WhenRequired)
//!   C3 round-trip byte-idêntico (sha256) + head.content_length igual
//!   C4 emite presigned GET (imprime URL p/ browser + curl)
//!   C5 1500 objetos → list_objects_v2 paginado + delete_objects (≤3 chamadas)
//!   C7 servidor morto → put_object falha em ≤5 s (com TimeoutConfig)
//! C1 (cargo check) e C6 (boot axum com as 6 rotas) são verificados fora deste example.

use std::time::{Duration, Instant};

use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region, RequestChecksumCalculation};
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use aws_smithy_types::retry::RetryConfig;
use aws_smithy_types::timeout::TimeoutConfig;
use sha2::{Digest, Sha256};

const BUCKET: &str = "heph-data";
const EP: &str = "http://localhost:8333";
const AK: &str = "heph";
const SK: &str = "heph-local-dev";

/// `aws-sdk-s3` sem `aws-config`: config::Builder + Credentials estáticos.
/// D4: force_path_style(true) + request_checksum_calculation(WhenRequired).
fn build_client(hard_timeout: bool) -> Client {
    // Credentials implementa ProvideCredentials → vai direto no credentials_provider.
    // (new, não from_keys: from_keys pede feature "hardcoded-credentials".)
    let creds = Credentials::new(AK, SK, None, None, "heph-spike");
    let mut b = aws_sdk_s3::config::Builder::new()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .endpoint_url(EP)
        .force_path_style(true)
        .request_checksum_calculation(RequestChecksumCalculation::WhenRequired)
        .credentials_provider(creds);
    // retry desabilitado (spike mede o erro cru, não o backoff).
    b = b.retry_config(RetryConfig::disabled());
    if hard_timeout {
        // C7: budget total ≤5 s → porta mapeia a StorageError::Unavailable (503).
        b = b.timeout_config(
            TimeoutConfig::builder()
                .connect_timeout(Duration::from_secs(2))
                .read_timeout(Duration::from_secs(5))
                .operation_timeout(Duration::from_secs(5))
                .build(),
        );
    }
    Client::from_conf(b.build())
}

fn sha(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex::encode(h.finalize())
}

#[tokio::main]
async fn main() {
    // Duas passadas sem stdin (a shell coordenadora pode ser não-interativa):
    //   SPIKE_DEAD unset → roda C2/C3/C4/C5 contra o servidor VIVO.
    //   SPIKE_DEAD=1    → assume servidor MORTO e mede só o C7 (timeout ≤5 s).
    if std::env::var("SPIKE_DEAD").is_ok() {
        spike_c7_dead_server().await;
        return;
    }
    if std::env::var("SPIKE_PUT_ONLY").is_ok() {
        // modo p/ wire-check: um único PUT de 1 KiB (comprimento exato) e sai.
        let client = build_client(false);
        let small: Vec<u8> = (0..1024u32).map(|i| (i % 256) as u8).collect();
        let r = client
            .put_object()
            .bucket(BUCKET)
            .key("datasets/spike/wirecheck.bin")
            .content_length(small.len() as i64)
            .body(ByteStream::from(small))
            .send()
            .await;
        println!("[PUT_ONLY] resultado: {:?}", r.map(|_| "ok").map_err(|e| e.to_string()));
        return;
    }
    if std::env::var("SPIKE_PUT_BIG").is_ok() {
        // wire-check do caso >16 MiB (threshold do #21611): um PUT de 512 MiB de arquivo.
        let client = build_client(false);
        let p = "/tmp/spike_512m.bin";
        use std::io::Write;
        if std::fs::metadata(p).map(|m| m.len()).unwrap_or(0) != 512 * 1024 * 1024 {
            let mut f = std::fs::File::create(p).unwrap();
            let chunk = vec![0xCDu8; 1024 * 1024];
            for _ in 0..512 {
                f.write_all(&chunk).unwrap();
            }
            f.flush().unwrap();
        }
        let len = std::fs::metadata(p).unwrap().len();
        let r = client
            .put_object()
            .bucket(BUCKET)
            .key("datasets/spike/wirebig.bin")
            .content_length(len as i64)
            .body(ByteStream::from_path(p).await.unwrap())
            .send()
            .await;
        println!("[PUT_BIG] 512 MiB resultado: {:?}", r.map(|_| "ok").map_err(|e| e.to_string()));
        return;
    }
    if let Ok(path) = std::env::var("SPIKE_UPLOAD_FILE") {
        // C4: sobe um arquivo real (PNG) e imprime o presigned GET p/ testar no browser.
        let client = build_client(false);
        let key = "datasets/spike/images/pic.png";
        let len = std::fs::metadata(&path).unwrap().len();
        client
            .put_object()
            .bucket(BUCKET)
            .key(key)
            .content_length(len as i64)
            .content_type("image/png")
            .body(ByteStream::from_path(&path).await.unwrap())
            .send()
            .await
            .unwrap();
        let cfg = PresigningConfig::expires_in(Duration::from_secs(3600)).unwrap();
        let pre = client.get_object().bucket(BUCKET).key(key).presigned(cfg).await.unwrap();
        println!("PRESIGNED_URL {}", pre.uri());
        return;
    }
    spike_live_server().await;
}

async fn spike_live_server() {
    let client = build_client(false);
    let mut pass = 0usize;
    let mut total = 0usize;

    // ---- C2a + C3: PUT 1 KiB → round-trip byte-idêntico + head.bytes ------
    total += 1;
    let small: Vec<u8> = (0..1024u32).map(|i| (i % 256) as u8).collect();
    let key_small = "datasets/spike/images/1KiB.bin";
    let t0 = Instant::now();
    let put_small = client
        .put_object()
        .bucket(BUCKET)
        .key(key_small)
        .content_length(small.len() as i64)
        .body(ByteStream::from(small.clone()))
        .send()
        .await;
    match put_small {
        Ok(_) => {
            println!(
                "[C2] PUT 1 KiB ok em {:?} — o bucket heph-data foi auto-criado (no init-container)",
                t0.elapsed()
            );
            let head = client.head_object().bucket(BUCKET).key(key_small).send().await.unwrap();
            let got = client
                .get_object()
                .bucket(BUCKET)
                .key(key_small)
                .send()
                .await
                .unwrap()
                .body
                .collect()
                .await
                .unwrap()
                .into_bytes();
            let identico = got.as_ref() == small.as_slice();
            let cl = head.content_length().unwrap_or(-1);
            let len_ok = cl as usize == small.len();
            println!(
                "[C3] round-trip 1 KiB: sha_local={} sha_got={} idêntico={} head.bytes={} len_ok={}",
                &sha(&small)[..8],
                &sha(&got)[..8],
                identico,
                cl,
                len_ok
            );
            if identico && len_ok {
                pass += 1;
                println!("  → C3 PASS");
            } else {
                println!("  → C3 FAIL");
            }
        }
        Err(e) => println!("  → C2 FAIL (PUT 1 KiB): {e}"),
    }

    // ---- C2b: PUT 512 MiB (streaming do disco, content-length exato) ------
    // ByteStream::from_path + WhenRequired → sem aws-chunked/trailer.
    total += 1;
    let big_path = "/tmp/spike_512m.bin";
    {
        use std::io::Write;
        let mut f = std::fs::File::create(big_path).unwrap();
        let chunk = vec![0xABu8; 1024 * 1024];
        for _ in 0..512 {
            f.write_all(&chunk).unwrap();
        }
        f.flush().unwrap();
    }
    let big_len = std::fs::metadata(big_path).unwrap().len();
    let t1 = Instant::now();
    let put_big = client
        .put_object()
        .bucket(BUCKET)
        .key("datasets/spike/images/512MiB.bin")
        .content_length(big_len as i64)
        .body(ByteStream::from_path(big_path).await.unwrap())
        .send()
        .await;
    match put_big {
        Ok(_) => {
            println!("[C2] PUT 512 MiB (from_path, len exato) ok em {:?} → CONFIRMAR LOG: zero req aws-chunked/STREAMING", t1.elapsed());
            pass += 1;
        }
        Err(e) => println!("[C2] PUT 512 MiB FAIL: {e}  (fallback: UNSIGNED-PAYLOAD explícito, ou inverter D4 p/ outro crate)"),
    }

    // ---- C4: presigned GET (imprime URL p/ browser :3000 + curl) ----------
    match PresigningConfig::expires_in(Duration::from_secs(3600)) {
        Ok(cfg) => match client
            .get_object()
            .bucket(BUCKET)
            .key(key_small)
            .presigned(cfg)
            .await
        {
            Ok(pre) => println!(
                "[C4] presigned GET ({}):\n  {}\n  → colar em <img src> de http://localhost:3000 e rodar curl -f do host",
                pre.method(),
                pre.uri()
            ),
            Err(e) => println!("[C4] presigned FAIL: {e}"),
        },
        Err(e) => println!("[C4] PresigningConfig FAIL: {e}"),
    }

    // ---- C5: 1500 objetos → list paginado + delete_objects ≤3 chamadas ----
    total += 1;
    let prefix = "datasets/spike/sweep";
    let n = 1500usize;
    println!("[C5] criando {n} objetos sob {prefix}/ ...");
    for i in 0..n {
        client
            .put_object()
            .bucket(BUCKET)
            .key(format!("{prefix}/obj-{i:05}.bin"))
            .content_length(1)
            .body(ByteStream::from_static(b"x"))
            .send()
            .await
            .unwrap();
    }
    let mut all_keys: Vec<String> = Vec::new();
    let mut token: Option<String> = None;
    let mut pages = 0usize;
    loop {
        let mut req = client
            .list_objects_v2()
            .bucket(BUCKET)
            .prefix(prefix)
            .max_keys(1000);
        if let Some(t) = &token {
            req = req.continuation_token(t.clone());
        }
        let resp = req.send().await.unwrap();
        all_keys.extend(resp.contents().iter().filter_map(|o| o.key().map(|k| k.to_string())));
        pages += 1;
        token = resp.next_continuation_token().map(|s| s.to_string());
        if token.is_none() {
            break;
        }
    }
    let listed = all_keys.len();
    let mut calls = 0usize;
    for chunk in all_keys.chunks(1000) {
        let objs: Vec<_> = chunk
            .iter()
            .map(|k| {
                aws_sdk_s3::types::ObjectIdentifier::builder()
                    .set_key(Some(k.clone()))
                    .build()
                    .unwrap()
            })
            .collect();
        client
            .delete_objects()
            .bucket(BUCKET)
            .delete(
                aws_sdk_s3::types::Delete::builder()
                    .set_objects(Some(objs))
                    .quiet(true)
                    .build()
                    .unwrap(),
            )
            .send()
            .await
            .unwrap();
        calls += 1;
    }
    let leftover = client
        .list_objects_v2()
        .bucket(BUCKET)
        .prefix(prefix)
        .max_keys(1000)
        .send()
        .await
        .unwrap();
    println!(
        "[C5] listou {listed} em {pages} página(s); delete_objects em {calls} chamada(s); restante={} → CHECK: ≤3 chamadas",
        leftover.contents().len()
    );
    if listed == n && calls <= 3 && leftover.contents().is_empty() {
        pass += 1;
        println!("  → C5 PASS");
    } else {
        println!("  → C5 FAIL (listed={listed}, calls={calls})");
    }

    println!("\n===== SPIKE live (C2/C3/C4/C5): {pass}/{total} auto-verificados =====");
    println!("Próxima passada (C7):  docker compose -f infra/compose.spike.yaml stop seaweedfs  &&  SPIKE_DEAD=1 cargo run --example storage_spike");
}

/// C7: servidor já morto — put_object deve falhar em ≤5 s (TimeoutConfig) → 503.
async fn spike_c7_dead_server() {
    let c7 = build_client(true);
    let t7 = Instant::now();
    let r7 = c7
        .put_object()
        .bucket(BUCKET)
        .key("datasets/spike/dead.bin")
        .content_length(1)
        .body(ByteStream::from_static(b"x"))
        .send()
        .await;
    let dt = t7.elapsed();
    match r7 {
        Ok(_) => println!("[C7] put_object SUCEDEU pós-stop ({dt:?}) — servidor ainda estava vivo? refaça"),
        Err(e) => {
            println!("[C7] put_object ERROU em {dt:?} → mapeável a StorageError::Unavailable (503): {e}");
            if dt <= Duration::from_secs(5) {
                println!("  → C7 PASS (≤5s)");
            } else {
                println!("  → C7 FAIL (>5s; apertar timeout)");
            }
        }
    }
}
