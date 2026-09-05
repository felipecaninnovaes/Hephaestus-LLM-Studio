//! Integração `S3Storage` × SeaweedFS do compose (Fatia 3b.4, ADR-0003).
//!
//! `#[ignore]` — roda CONTRA o seaweedfs do compose via env (não sobe nada):
//! `bash scripts/test-storage.sh`. Prefixo isolado por teste (uuid) p/ não colidir.
//! Teste 2 (512 MiB) é SKIP por default (spike provou C2 no host); roda com `STORAGE_BIG=1`.
//! Teste 5 ("servidor morto") é SKIP por default (C7 provado no spike); roda com
//! `STORAGE_DEAD=1` (endpoint morto, sem precisar parar o compose).

use std::io::Write;
use std::time::{Duration, Instant};

use api_principal::storage::{S3Storage, StorageConfig, StoragePort};
use sha2::{Digest, Sha256};

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn test_storage() -> S3Storage {
    let cfg = StorageConfig {
        bucket: env_or("S3_BUCKET", "heph-data"),
        public_endpoint: Some(env_or(
            "S3_PUBLIC_ENDPOINT_URL",
            "http://localhost:8333",
        )),
        url_ttl_secs: 3600,
    };
    S3Storage::new(
        &cfg,
        &env_or("S3_ENDPOINT_URL", "http://localhost:8333"),
        &env_or("S3_ACCESS_KEY", "heph"),
        &env_or("S3_SECRET_KEY", "heph-local-dev"),
    )
    .expect("S3Storage::new (seaweedfs do compose no ar?)")
}

fn sha(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex::encode(h.finalize())
}

fn tmp_file(bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().expect("tempfile");
    f.write_all(bytes).expect("write tmp");
    f.flush().expect("flush tmp");
    f
}

fn prefix(name: &str) -> String {
    format!("datasets/_test-storage-{}/{}", name, uuid::Uuid::new_v4())
}

/// 1. put(1KiB) + get byte-idêntico (sha256 iguais).
#[ignore]
#[tokio::test]
async fn s3_put_get_roundtrip_1kib() {
    let s = test_storage();
    let p = prefix("roundtrip");
    let data: Vec<u8> = (0..1024u32).map(|i| (i % 256) as u8).collect();
    let key = format!("{p}/a.bin");
    let f = tmp_file(&data);
    s.put(&key, f.path()).await.expect("put 1KiB");
    let got = s.get(&key).await.expect("get 1KiB");
    assert_eq!(sha(&got), sha(&data), "round-trip byte-idêntico");
    s.delete(&key).await.expect("delete");
}

/// 2. put(512MiB) — SKIP por default; roda com `STORAGE_BIG=1`, assert <60s.
#[ignore]
#[tokio::test]
async fn s3_put_512mib_big() {
    if std::env::var("STORAGE_BIG").unwrap_or_default() != "1" {
        eprintln!("SKIP s3_put_512mib_big (rode com STORAGE_BIG=1; C2 provado no spike)");
        return;
    }
    let s = test_storage();
    let p = prefix("big");
    let key = format!("{p}/big.bin");
    let path = std::env::temp_dir().join(format!("heph-test-512m-{}.bin", uuid::Uuid::new_v4()));
    {
        let mut f = std::fs::File::create(&path).expect("create big tmp");
        let chunk = vec![0xABu8; 1024 * 1024];
        for _ in 0..512 {
            f.write_all(&chunk).expect("write big chunk");
        }
        f.flush().expect("flush big");
    }
    let t0 = Instant::now();
    s.put(&key, &path).await.expect("put 512MiB");
    let dt = t0.elapsed();
    std::fs::remove_file(&path).ok();
    assert!(dt < Duration::from_secs(60), "put 512MiB <60s ({dt:?})");
    s.delete(&key).await.expect("delete big");
}

/// 3. presign_get: URL contém `X-Amz-Signature` e `curl -f` == 200 bytes-idênticos.
#[ignore]
#[tokio::test]
async fn s3_presign_get_curl() {
    let s = test_storage();
    let p = prefix("presign");
    let data: Vec<u8> = (0..1024u32).map(|i| (i % 256) as u8).collect();
    let key = format!("{p}/pic.bin");
    let f = tmp_file(&data);
    s.put(&key, f.path()).await.expect("put presign");
    let url = s.presign_get(&key).await.expect("presign_get");
    assert!(url.contains("X-Amz-Signature"), "URL assinada SigV4: {url}");
    // curl via std::process (sem reqwest, D4).
    let out = std::env::temp_dir().join(format!("heph-presign-{}.bin", uuid::Uuid::new_v4()));
    let st = std::process::Command::new("curl")
        .args(["-sf", &url, "-o", &out.to_string_lossy()])
        .status()
        .expect("spawn curl");
    assert!(st.success(), "curl -f no presigned == 200");
    let got = std::fs::read(&out).expect("read curl out");
    std::fs::remove_file(&out).ok();
    assert_eq!(sha(&got), sha(&data), "presigned byte-idêntico");
    s.delete(&key).await.expect("delete");
}

/// 4. delete_prefix: 1500 objetos de 16B → contagem ≥1500 e listagem vazia depois.
#[ignore]
#[tokio::test]
async fn s3_delete_prefix_1500() {
    let s = test_storage();
    let p = prefix("sweep");
    let t0 = Instant::now();
    for i in 0..1500usize {
        let key = format!("{p}/obj-{i:05}.bin");
        let f = tmp_file(&[i as u8; 16]);
        s.put(&key, f.path()).await.expect("seed put");
    }
    if t0.elapsed() > Duration::from_secs(60) {
        eprintln!("SKIP resto de s3_delete_prefix_1500 (seed >60s)");
        return;
    }
    let count = s.delete_prefix(&p).await.expect("delete_prefix");
    assert!(count >= 1500, "contagem ≥1500 sob o prefixo (foi {count})");
    let rest = s.delete_prefix(&p).await.expect("re-list vazio");
    assert_eq!(rest, 0, "prefixo vazio depois do sweep");
}

/// 5. "servidor morto" — SKIP por default; roda com `STORAGE_DEAD=1`
/// (endpoint fechado, sem precisar parar o compose).
#[ignore]
#[tokio::test]
async fn s3_dead_server_unavailable() {
    if std::env::var("STORAGE_DEAD").unwrap_or_default() != "1" {
        eprintln!("SKIP s3_dead_server_unavailable (rode com STORAGE_DEAD=1; C7 provado no spike)");
        return;
    }
    let cfg = StorageConfig {
        bucket: "heph-data".to_string(),
        public_endpoint: None,
        url_ttl_secs: 3600,
    };
    let s = S3Storage::new(&cfg, "http://127.0.0.1:9", "heph", "heph-local-dev")
        .expect("new com endpoint morto");
    let f = tmp_file(b"x");
    let t0 = Instant::now();
    let r = s.put("datasets/_test-dead/x.bin", f.path()).await;
    let dt = t0.elapsed();
    assert!(matches!(r, Err(api_principal::storage::StorageError::Unavailable(_))));
    assert!(dt < Duration::from_secs(5), "servidor morto ≤5s ({dt:?})");
}
