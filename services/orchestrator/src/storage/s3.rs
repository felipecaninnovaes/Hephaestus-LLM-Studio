//! Cliente S3 real (aws-sdk-s3) e PUT com retry.

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;

use crate::ports::storage::S3Port;

/// Cliente S3 real usando aws-sdk-s3 (padrão services/api-principal/src/storage/s3.rs).
pub struct S3Client {
    client: aws_sdk_s3::Client,
    bucket: String,
}

impl S3Client {
    pub fn new(endpoint: &str, access_key: &str, secret_key: &str, bucket: &str) -> Self {
        let creds =
            aws_sdk_s3::config::Credentials::new(access_key, secret_key, None, None, "heph-orch");
        let conf = aws_sdk_s3::config::Builder::new()
            .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
            .region(aws_sdk_s3::config::Region::new("us-east-1"))
            .endpoint_url(endpoint)
            .force_path_style(true)
            .request_checksum_calculation(
                aws_sdk_s3::config::RequestChecksumCalculation::WhenRequired,
            )
            .credentials_provider(creds)
            .retry_config(aws_smithy_types::retry::RetryConfig::disabled())
            .timeout_config(
                aws_smithy_types::timeout::TimeoutConfig::builder()
                    .connect_timeout(Duration::from_secs(2))
                    .read_timeout(Duration::from_secs(30))
                    // Teto de transferência p/ objetos multi-GB (fail-fast vem de
                    // connect+read, não daqui): 120s flakeava em LAN lenta.
                    .operation_timeout(Duration::from_secs(3600))
                    .build(),
            )
            .build();
        Self {
            client: aws_sdk_s3::Client::from_conf(conf),
            bucket: bucket.to_string(),
        }
    }
}

#[async_trait]
impl S3Port for S3Client {
    async fn get_to_file(&self, key: &str, path: &Path) -> Result<(), String> {
        self.get_to_file_with_progress(key, path, None).await
    }

    async fn get_to_file_with_progress(
        &self,
        key: &str,
        path: &Path,
        on_progress: Option<&(dyn Fn(u64, Option<u64>) + Send + Sync)>,
    ) -> Result<(), String> {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

        let out = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| format!("S3 GET {key}: {e}"))?;

        let total_bytes = out.content_length().map(|l| l as u64);
        let mut reader = out.body.into_async_read();
        let mut file = tokio::fs::File::create(path)
            .await
            .map_err(|e| format!("create file {}: {e}", path.display()))?;

        let mut buf = [0u8; 64 * 1024];
        let mut downloaded_bytes: u64 = 0;
        let mut last_reported = tokio::time::Instant::now();

        if let Some(cb) = on_progress {
            cb(0, total_bytes);
        }

        loop {
            let n = reader
                .read(&mut buf)
                .await
                .map_err(|e| format!("read S3 GET {key}: {e}"))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])
                .await
                .map_err(|e| format!("write file {}: {e}", path.display()))?;
            downloaded_bytes += n as u64;

            if let Some(cb) = on_progress {
                if last_reported.elapsed() >= std::time::Duration::from_millis(200) {
                    cb(downloaded_bytes, total_bytes);
                    last_reported = tokio::time::Instant::now();
                }
            }
        }
        file.flush()
            .await
            .map_err(|e| format!("flush file {}: {e}", path.display()))?;

        if let Some(cb) = on_progress {
            cb(downloaded_bytes, total_bytes);
        }
        Ok(())
    }

    async fn put(&self, key: &str, path: &Path) -> Result<(), String> {
        let body = aws_sdk_s3::primitives::ByteStream::from_path(path)
            .await
            .map_err(|e| format!("read file {}: {e}", path.display()))?;
        let len = std::fs::metadata(path)
            .map(|m| m.len() as i64)
            .map_err(|e| format!("stat {}: {e}", path.display()))?;

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_length(len)
            .body(body)
            .send()
            .await
            .map_err(|e| format!("S3 PUT {key}: {e}"))?;
        Ok(())
    }

    async fn ping(&self) -> bool {
        // Prova de acessibilidade S3 no escopo do orquestrador (artifacts/):
        // heph-orchestrator tem menor privilégio e não possui permissão HeadBucket na raiz.
        self.client
            .list_objects_v2()
            .bucket(&self.bucket)
            .prefix("artifacts/")
            .max_keys(1)
            .send()
            .await
            .is_ok()
    }
}

/// PUT S3 com retry (N=3, backoff curto 100ms/200ms).
///
/// Upload de output não pode falhar silenciosamente (incidente galeria vazia):
/// quem chama registra o erro persistente e o report final vira failed.
/// Não aborta o resto do loop — o chamador decide após coletar tudo.
pub async fn put_with_retry(s3: &dyn S3Port, key: &str, path: &Path) -> Result<(), String> {
    let mut last_err = String::from("upload falhou");
    for attempt in 0..3 {
        match s3.put(key, path).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = e;
                if attempt < 2 {
                    tokio::time::sleep(Duration::from_millis(100 * (attempt as u64 + 1))).await;
                }
            }
        }
    }
    Err(last_err)
}
