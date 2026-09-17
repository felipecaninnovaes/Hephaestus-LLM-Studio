//! `S3Storage` — impl real da `StoragePort` sobre S3-compatível (ADR-0003 D4/D8, 3b.4).
//!
//! Nomes de API copiados do harness do spike (`examples/storage_spike.rs`, 7/7 PASS):
//! `config::Builder` + `Credentials::new` (sem `aws-config`), `force_path_style(true)`,
//! `request_checksum_calculation(WhenRequired)`, `TimeoutConfig`/`RetryConfig` de
//! `aws_smithy_types`, presign via `.presigned(PresigningConfig::expires_in(..))` + `.uri()`.

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region, RequestChecksumCalculation};
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use aws_smithy_types::retry::RetryConfig;
use aws_smithy_types::timeout::TimeoutConfig;

use super::port::{StorageConfig, StorageError, StoragePort};

/// Orçamento do `ensure_bucket` no boot (backend s3): tenta CreateBucket,
/// retry a cada `ENSURE_BUCKET_RETRY_SECS` até `ENSURE_BUCKET_TIMEOUT_SECS`
/// e então fail-fast (serviço sem bucket serve jobs "done" zerados — o bug
/// que isto fecha). Constantes + `bucket_retry_due` puras e testáveis sem
/// rede (o boot nunca dorme 60s em teste).
pub const ENSURE_BUCKET_TIMEOUT_SECS: u64 = 60;
pub const ENSURE_BUCKET_RETRY_SECS: u64 = 2;

/// Puro/testável: ainda cabe outra tentativa após `elapsed_secs`?
pub fn bucket_retry_due(elapsed_secs: u64) -> bool {
    elapsed_secs < ENSURE_BUCKET_TIMEOUT_SECS
}

/// Puro/testável: intervalo entre tentativas do `ensure_bucket`.
pub fn bucket_retry_interval() -> Duration {
    Duration::from_secs(ENSURE_BUCKET_RETRY_SECS)
}

/// Classifica erro de CreateBucket: `true` = bucket já existe ou já é nosso
/// (idempotente, boot segue). Qualquer outro erro (rede, 403, nome inválido)
/// → retry até o orçamento, depois fail-fast. Sem pânico: usa match
/// exaustivo, nunca `unwrap`/`expect` no erro do SDK.
pub fn create_bucket_conflict(
    err: &aws_sdk_s3::operation::create_bucket::CreateBucketError,
) -> bool {
    use aws_sdk_s3::operation::create_bucket::CreateBucketError as E;
    matches!(
        err,
        E::BucketAlreadyExists(_) | E::BucketAlreadyOwnedByYou(_)
    )
}

/// Percent-encode de key p/ `copy_source` (sem crate nova): preserva
/// `A-Za-z0-9-_.~/`, encoda o resto byte a byte (`%XX` maiúsculo).
fn encode_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    for b in key.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn build_client(endpoint_url: &str, access_key: &str, secret_key: &str) -> Client {
    let creds = Credentials::new(access_key, secret_key, None, None, "heph-s3");
    let conf = aws_sdk_s3::config::Builder::new()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .endpoint_url(endpoint_url)
        .force_path_style(true)
        .request_checksum_calculation(RequestChecksumCalculation::WhenRequired)
        .credentials_provider(creds)
        .retry_config(RetryConfig::disabled())
        // C7 do spike (revisado p/ objetos multi-GB): fail-fast de servidor
        // morto vem de connect_timeout (connect_refused em ~ms) + read_timeout
        // (stream engasgado); operation_timeout é só o teto de transferência —
        // 5s abortava qualquer PUT de GBs, 60min cobre datasets de vários GB.
        .timeout_config(
            TimeoutConfig::builder()
                .connect_timeout(Duration::from_secs(2))
                .read_timeout(Duration::from_secs(30))
                .operation_timeout(Duration::from_secs(3600))
                .build(),
        )
        .build();
    Client::from_conf(conf)
}

/// Storage S3-compatível (SeaweedFS em dev, endpoint+credenciais em env).
///
/// Mantém DOIS clients: o principal (rede interna, ex. `http://seaweedfs:8333`) e o
/// `presign_client` (endpoint público, ex. `http://localhost:8333`). Gotcha SigV4 (D3):
/// o Host entra na assinatura, então o presigned tem de nascer assinado no endpoint
/// público que o browser vai alcançar — assinar no host interno invalida a URL fora.
pub struct S3Storage {
    client: Client,
    presign_client: Client,
    bucket: String,
    url_ttl_secs: u64,
}

impl S3Storage {
    /// Constrói os clients S3. Não faz I/O (bucket auto-cria no 1º PUT de
    /// identidade `Admin` — spike ACHADO 2, sem init-container).
    pub fn new(
        cfg: &StorageConfig,
        endpoint_url: &str,
        access_key: &str,
        secret_key: &str,
    ) -> Result<Self, String> {
        if endpoint_url.is_empty() {
            return Err("storage: endpoint S3 vazio".to_string());
        }
        if access_key.is_empty() || secret_key.is_empty() {
            return Err("storage: credencial S3 vazia".to_string());
        }
        let client = build_client(endpoint_url, access_key, secret_key);
        let public_url = cfg.public_endpoint.as_deref().unwrap_or(endpoint_url);
        let presign_client = if public_url == endpoint_url {
            client.clone()
        } else {
            build_client(public_url, access_key, secret_key)
        };
        Ok(Self {
            client,
            presign_client,
            bucket: cfg.bucket.clone(),
            url_ttl_secs: cfg.url_ttl_secs,
        })
    }

    /// Garante o bucket no boot com `STORAGE_BACKEND=s3` (idempotente).
    ///
    /// CreateBucket no bucket configurado: criado ou já-existente (nosso ou
    /// não — namespace S3 é global, colisão alheia também é "ok, segue",
    /// o PUT posterior falha alto se não tivermos acesso) = sucesso com
    /// log `bucket '<nome>' garantido`. Falha de rede/transiente → retry
    /// 2s até ~60s; estourado o orçamento, `Err` claro e o boot aborta
    /// (fail-fast: sem bucket o serviço mente "done" com zero artefatos).
    /// Nome do bucket vai ao log; credencial/endpoint, nunca.
    pub async fn ensure_bucket(&self) -> Result<(), String> {
        let t0 = std::time::Instant::now();
        loop {
            match self
                .client
                .create_bucket()
                .bucket(&self.bucket)
                .send()
                .await
            {
                Ok(_) => {
                    tracing::info!("bucket '{}' garantido", self.bucket);
                    return Ok(());
                }
                Err(sdk_err) => {
                    // `as_service_error` (não `into_service_error`): falha de
                    // transporte/timeout não é erro de serviço e não pode
                    // dar pânico — cai no retry como transiente.
                    if let Some(svc) = sdk_err.as_service_error() {
                        if create_bucket_conflict(svc) {
                            tracing::info!("bucket '{}' garantido", self.bucket);
                            return Ok(());
                        }
                    }
                    if !bucket_retry_due(t0.elapsed().as_secs()) {
                        return Err(format!(
                            "ensure bucket '{}': indisponivel apos ~{}s (verifique S3_ENDPOINT_URL e credenciais)",
                            self.bucket, ENSURE_BUCKET_TIMEOUT_SECS
                        ));
                    }
                    tracing::warn!(
                        "bucket '{}' indisponivel, tentando de novo em {}s",
                        self.bucket,
                        ENSURE_BUCKET_RETRY_SECS
                    );
                    tokio::time::sleep(bucket_retry_interval()).await;
                }
            }
        }
    }
}

#[async_trait]
impl StoragePort for S3Storage {
    async fn put(&self, key: &str, path: &Path) -> Result<(), StorageError> {
        // `from_path` faz streaming do disco com length exato (C2/C3 do spike:
        // com WhenRequired o wire sai sem trilha aws-chunked/trailer).
        let body = ByteStream::from_path(path)
            .await
            .map_err(|_| StorageError::Unavailable("storage unavailable".into()))?;
        // D2/C2: o length é INVARIANTE, não otimização — stat que falha aborta
        // o PUT (nunca enviar length 0 com corpo de N bytes; revisão 3b.6 F2).
        let len = std::fs::metadata(path)
            .map(|m| m.len() as i64)
            .map_err(|_| StorageError::Unavailable("storage unavailable".into()))?;
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_length(len)
            .body(body)
            .send()
            .await
            .map_err(|_| StorageError::Unavailable("storage unavailable".into()))?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>, StorageError> {
        let out = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                let svc = e.into_service_error();
                if matches!(
                    svc,
                    aws_sdk_s3::operation::get_object::GetObjectError::NoSuchKey(_)
                ) {
                    StorageError::NotFound
                } else {
                    StorageError::Unavailable("storage unavailable".into())
                }
            })?;
        let bytes = out
            .body
            .collect()
            .await
            .map_err(|_| StorageError::Unavailable("storage unavailable".into()))?;
        Ok(bytes.into_bytes().to_vec())
    }

    async fn get_to_file(&self, key: &str, path: &Path) -> Result<(), StorageError> {
        // Espelho do `put` (ADR-0006 D9): `get_object().body.into_async_read()`
        // → copy para arquivo — streama bucket→disco sem carregar RAM.
        let out = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                let svc = e.into_service_error();
                if matches!(
                    svc,
                    aws_sdk_s3::operation::get_object::GetObjectError::NoSuchKey(_)
                ) {
                    StorageError::NotFound
                } else {
                    StorageError::Unavailable("storage unavailable".into())
                }
            })?;
        let mut reader = out.body.into_async_read();
        let mut file = tokio::fs::File::create(path)
            .await
            .map_err(|_| StorageError::Unavailable("storage unavailable".into()))?;
        tokio::io::copy(&mut reader, &mut file)
            .await
            .map_err(|_| StorageError::Unavailable("storage unavailable".into()))?;
        Ok(())
    }

    async fn presign_get(&self, key: &str) -> Result<String, StorageError> {
        // Assinado no endpoint PÚBLICO (presign_client) — gotcha SigV4 D3.
        let cfg = PresigningConfig::expires_in(Duration::from_secs(self.url_ttl_secs))
            .map_err(|_| StorageError::Unavailable("storage unavailable".into()))?;
        let pre = self
            .presign_client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(cfg)
            .await
            .map_err(|_| StorageError::Unavailable("storage unavailable".into()))?;
        Ok(pre.uri().to_string())
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        // S3 delete é idempotente: chave inexistente retorna Ok (204), então
        // qualquer resposta do servidor é sucesso; só falha de transporte vira 503.
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|_| StorageError::Unavailable("storage unavailable".into()))?;
        Ok(())
    }

    async fn delete_prefix(&self, prefix: &str) -> Result<u32, StorageError> {
        // Sweep D7: ListObjectsV2 paginado + DeleteObjects em lotes de ≤1000
        // (C5 do spike: 1500 objetos em 2 páginas + 2 chamadas).
        let mut keys: Vec<String> = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut req = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(prefix)
                .max_keys(1000);
            if let Some(t) = &token {
                req = req.continuation_token(t.clone());
            }
            let resp = req
                .send()
                .await
                .map_err(|_| StorageError::Unavailable("storage unavailable".into()))?;
            keys.extend(
                resp.contents()
                    .iter()
                    .filter_map(|o| o.key().map(|k| k.to_string())),
            );
            token = resp.next_continuation_token().map(|s| s.to_string());
            if token.is_none() {
                break;
            }
        }
        if keys.is_empty() {
            return Ok(0);
        }
        let mut removed: u32 = 0;
        for chunk in keys.chunks(1000) {
            let objs: Vec<_> = chunk
                .iter()
                .map(|k| {
                    aws_sdk_s3::types::ObjectIdentifier::builder()
                        .set_key(Some(k.clone()))
                        .build()
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| StorageError::Unavailable("storage unavailable".into()))?;
            // Revisão 3b.6: SEM quiet(true) — quiet suprime `deleted`, e a
            // contagem precisa ser de chaves CONFIRMADAS deletadas; erros por-
            // chave de um 200 parcial ficam de fora (log completo: varredura
            // da 3b.7 junto da dívida de logging).
            let resp = self
                .client
                .delete_objects()
                .bucket(&self.bucket)
                .delete(
                    aws_sdk_s3::types::Delete::builder()
                        .set_objects(Some(objs))
                        .build()
                        .map_err(|_| StorageError::Unavailable("storage unavailable".into()))?,
                )
                .send()
                .await
                .map_err(|_| StorageError::Unavailable("storage unavailable".into()))?;
            removed += resp.deleted().len() as u32;
        }
        Ok(removed)
    }

    async fn copy_object(&self, from_key: &str, to_key: &str) -> Result<(), StorageError> {
        // Gotcha do SDK: `copy_source` é `{bucket}/{key}` com a key
        // URL-encoded (barras preservadas); sem encode, keys com caracteres
        // especiais falham com erro opaco do servidor.
        let source = format!("{}/{}", self.bucket, encode_key(from_key));
        self.client
            .copy_object()
            .bucket(&self.bucket)
            .key(to_key)
            .copy_source(source)
            .send()
            .await
            .map_err(|_| StorageError::Unavailable("storage unavailable".into()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(public: Option<&str>) -> StorageConfig {
        StorageConfig {
            bucket: "heph-data".to_string(),
            public_endpoint: public.map(|s| s.to_string()),
            url_ttl_secs: 3600,
        }
    }

    #[test]
    fn new_com_endpoint_dummy_ok_sem_io() {
        // Só constrói clients — nenhum I/O no `new` (prova real é storage_s3.rs ignore).
        let cfg = test_config(Some("http://localhost:8333"));
        let s = S3Storage::new(&cfg, "http://seaweedfs:8333", "heph", "heph-local-dev");
        assert!(s.is_ok());
    }

    #[test]
    fn new_sem_public_reusa_endpoint() {
        let cfg = test_config(None);
        let s = S3Storage::new(&cfg, "http://seaweedfs:8333", "heph", "heph-local-dev");
        assert!(s.is_ok());
    }

    #[test]
    fn presigning_config_acima_do_max_sigv4_erro_sem_crash() {
        // SigV4 limita presigned a 7 dias — 8 dias deve Err, não crash.
        assert!(PresigningConfig::expires_in(Duration::from_secs(8 * 24 * 3600)).is_err());
    }

    #[test]
    fn encode_key_preserva_barra_e_reservados() {
        assert_eq!(
            encode_key("datasets/a/images/b/foto.jpg"),
            "datasets/a/images/b/foto.jpg"
        );
        assert_eq!(encode_key("a b/c+d.png"), "a%20b/c%2Bd.png");
    }

    #[test]
    fn bucket_retry_interval_e_orcamento() {
        // Backoff/duração como fns puras: o teste nunca dorme.
        assert_eq!(bucket_retry_interval(), Duration::from_secs(2));
        assert_eq!(ENSURE_BUCKET_TIMEOUT_SECS, 60);
        assert!(bucket_retry_due(0), "início: ainda cabe retry");
        assert!(bucket_retry_due(59), "último segundo: ainda cabe retry");
        assert!(!bucket_retry_due(60), "orçamento estourado: fail-fast");
        assert!(!bucket_retry_due(3600), "muito além: fail-fast");
    }

    #[test]
    fn create_bucket_conflict_aceita_buckets_existentes() {
        use aws_sdk_s3::operation::create_bucket::CreateBucketError as E;
        use aws_sdk_s3::types::error as TE;
        // Constrói os erros via builders do SDK (sem rede): os dois
        // "já existe" são idempotentes (ok), o resto é retry/fail.
        let owned = E::BucketAlreadyOwnedByYou(TE::BucketAlreadyOwnedByYou::builder().build());
        assert!(
            create_bucket_conflict(&owned),
            "BucketAlreadyOwnedByYou = ok"
        );
        let exists = E::BucketAlreadyExists(TE::BucketAlreadyExists::builder().build());
        assert!(create_bucket_conflict(&exists), "BucketAlreadyExists = ok");
        let unhandled = E::unhandled("boom");
        assert!(
            !create_bucket_conflict(&unhandled),
            "Unhandled = retry/fail"
        );
    }
}
