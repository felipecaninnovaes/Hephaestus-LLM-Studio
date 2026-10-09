//! Cliente do protocolo de upload em partes do custom node (contrato §1) e
//! laço de transferência: lê a origem em streaming, corta em partes do
//! `chunkSize` do init, retoma por `expectedOffset`, confere sha256 no commit.

use bytes::Bytes;
use reqwest::{Client, Method, Response, StatusCode};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};

const MAX_ATTEMPTS_PER_PART: u32 = 3;
const SHORT_TIMEOUT: Duration = Duration::from_secs(30);
const PART_TIMEOUT: Duration = Duration::from_secs(600);
const COMMIT_TIMEOUT: Duration = Duration::from_secs(900);
/// Teto de segurança para o `chunkSize` devolvido pelo destino (RAM do BFF).
const MAX_CHUNK_SIZE: u64 = 64 * 1024 * 1024;
/// Teto do corpo de resposta lido do destino (não confiável).
const MAX_RESPONSE_BODY: usize = 64 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum TransferError {
    /// Falha definitiva; mensagem PT-BR legível para `comfy_exports.error`.
    Failed(Failure),
    /// O destino não conhece mais o `uploadId` (ComfyUI reiniciou): o chamador
    /// pode recomeçar do zero com novo init.
    UploadLost,
}

/// Falha definitiva: `code` estável p/ máquina (UI), `message` PT-BR p/ humano.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Failure {
    pub code: String,
    pub message: String,
}

impl Failure {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }
}

fn failed(code: &str, msg: impl Into<String>) -> TransferError {
    TransferError::Failed(Failure::new(code, msg))
}

/// Código do custom node quando veio dele; `network`/`remote_error` do BFF.
fn call_failure(e: CallError) -> TransferError {
    let code = match &e {
        CallError::Network => "network".to_string(),
        CallError::Redirect => "remote_error".to_string(),
        CallError::Api { code, .. } if !code.is_empty() => code.clone(),
        CallError::Api { .. } => "remote_error".to_string(),
    };
    failed(&code, describe(e))
}

#[derive(Debug)]
enum CallError {
    Network,
    /// 3xx: redirects não são seguidos (o token iria a outro host).
    Redirect,
    Api {
        status: u16,
        code: String,
        expected_offset: Option<u64>,
    },
}

#[derive(Deserialize)]
struct ApiErrorBody {
    #[serde(default)]
    error: String,
    #[serde(default, rename = "expectedOffset")]
    expected_offset: Option<u64>,
}

/// Mensagem PT-BR por código de erro do custom node.
pub fn api_message(status: u16, code: &str) -> String {
    match code {
        "token_not_configured" => "o ComfyUI não tem token configurado para o Hephaestus".into(),
        "unauthorized" => "o ComfyUI recusou o token do destino".into(),
        "invalid_filename" => "o ComfyUI recusou o nome do arquivo".into(),
        "invalid_size" | "invalid_body" | "invalid_offset" => {
            "o ComfyUI recusou a requisição de envio".into()
        }
        "file_exists" => "o arquivo já existe no ComfyUI (envie com sobrescrever)".into(),
        "size_too_large" => "o arquivo passa do tamanho máximo aceito pelo ComfyUI (4 GiB)".into(),
        "insufficient_storage" => "sem espaço em disco no ComfyUI".into(),
        "too_many_uploads" => "o ComfyUI já tem 4 envios em andamento".into(),
        "upload_not_found" => "o ComfyUI perdeu o upload em andamento".into(),
        "chunk_too_large" => "o ComfyUI recusou a parte: maior que o permitido".into(),
        "size_exceeded" => "o envio passou do tamanho declarado".into(),
        "size_mismatch" => "o ComfyUI recebeu um tamanho diferente do declarado".into(),
        "checksum_mismatch" => "o sha256 do arquivo não confere no ComfyUI".into(),
        "offset_mismatch" => "o ComfyUI e o envio perderam a sincronia de offset".into(),
        _ => format!("o ComfyUI respondeu com erro HTTP {status}"),
    }
}

#[derive(Clone)]
pub struct Remote {
    client: Client,
    base_url: String,
    token: String,
    retry_delay: Duration,
}

impl std::fmt::Debug for Remote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Remote")
            .field("base_url", &self.base_url)
            .field("token", &"<redacted>")
            .finish_non_exhaustive()
    }
}

/// Lê o corpo em stream; `None` se passar de `MAX_RESPONSE_BODY` ou falhar.
async fn read_limited(mut resp: Response) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await.ok()? {
        if out.len() + chunk.len() > MAX_RESPONSE_BODY {
            return None;
        }
        out.extend_from_slice(&chunk);
    }
    Some(out)
}

async fn read_json<T: serde::de::DeserializeOwned>(resp: Response) -> Option<T> {
    serde_json::from_slice(&read_limited(resp).await?).ok()
}

#[derive(Debug, Deserialize)]
pub struct Health {
    pub version: String,
    #[serde(rename = "chunkSize")]
    pub chunk_size: u64,
}

#[derive(Debug, Deserialize)]
struct InitResponse {
    #[serde(rename = "uploadId")]
    upload_id: String,
    #[serde(rename = "chunkSize")]
    chunk_size: u64,
}

#[derive(Debug, Deserialize)]
struct CommitResponse {
    path: String,
}

impl Remote {
    pub fn new(base_url: &str, token: &str) -> Self {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default();
        Self {
            client,
            base_url: base_url.to_string(),
            token: token.to_string(),
            retry_delay: Duration::from_millis(500),
        }
    }

    pub fn with_retry_delay(mut self, d: Duration) -> Self {
        self.retry_delay = d;
        self
    }

    async fn call(
        &self,
        method: Method,
        path: &str,
        timeout: Duration,
        json: Option<serde_json::Value>,
        body: Option<Bytes>,
    ) -> Result<Response, CallError> {
        let mut req = self
            .client
            .request(method, format!("{}{path}", self.base_url))
            .bearer_auth(&self.token)
            .timeout(timeout);
        if let Some(j) = json {
            req = req.json(&j);
        }
        if let Some(b) = body {
            req = req
                .header("content-type", "application/octet-stream")
                .body(b);
        }
        let resp = req.send().await.map_err(|_| CallError::Network)?;
        if resp.status().is_success() {
            return Ok(resp);
        }
        if resp.status().is_redirection() {
            return Err(CallError::Redirect);
        }
        let status = resp.status().as_u16();
        let parsed: Option<ApiErrorBody> = read_json(resp).await;
        Err(CallError::Api {
            status,
            code: parsed.as_ref().map(|p| p.error.clone()).unwrap_or_default(),
            expected_offset: parsed.and_then(|p| p.expected_offset),
        })
    }

    /// `GET /hephaestus/health` (timeout 10 s). Erro já em PT-BR.
    pub async fn health(&self) -> Result<Health, String> {
        let r = self
            .call(
                Method::GET,
                "/hephaestus/health",
                Duration::from_secs(10),
                None,
                None,
            )
            .await
            .map_err(describe)?;
        read_json(r).await.ok_or_else(|| {
            "resposta inválida do ComfyUI (o custom node está instalado?)".to_string()
        })
    }

    async fn init(
        &self,
        filename: &str,
        size: u64,
        overwrite: bool,
    ) -> Result<InitResponse, TransferError> {
        let body = serde_json::json!({"filename": filename, "size": size, "overwrite": overwrite});
        let r = self
            .call(
                Method::POST,
                "/hephaestus/lora/uploads",
                SHORT_TIMEOUT,
                Some(body),
                None,
            )
            .await
            .map_err(call_failure)?;
        read_json(r).await.ok_or_else(|| {
            failed(
                "remote_error",
                "resposta inválida do ComfyUI ao iniciar o envio",
            )
        })
    }

    async fn abort(&self, upload_id: &str) {
        let _ = self
            .call(
                Method::DELETE,
                &format!("/hephaestus/lora/uploads/{upload_id}"),
                SHORT_TIMEOUT,
                None,
                None,
            )
            .await;
    }

    /// Envia uma parte inteira; retoma por `expectedOffset` e repete até 3x
    /// em erro de rede/5xx. Qualquer outro 4xx é definitivo.
    async fn put_part(
        &self,
        upload_id: &str,
        start: u64,
        part: &Bytes,
    ) -> Result<(), TransferError> {
        let end = start + part.len() as u64;
        let mut from = start;
        for attempt in 1..=MAX_ATTEMPTS_PER_PART {
            let body = part.slice((from - start) as usize..);
            let path = format!("/hephaestus/lora/uploads/{upload_id}?offset={from}");
            match self
                .call(Method::PUT, &path, PART_TIMEOUT, None, Some(body))
                .await
            {
                Ok(_) => return Ok(()),
                Err(CallError::Api {
                    status: 409,
                    code,
                    expected_offset: Some(e),
                }) if code == "offset_mismatch" && (start..=end).contains(&e) => {
                    if e == end {
                        return Ok(());
                    }
                    from = e;
                }
                Err(CallError::Api {
                    status: 404, code, ..
                }) if code == "upload_not_found" => {
                    return Err(TransferError::UploadLost);
                }
                Err(CallError::Api { status, code, .. }) if status < 500 => {
                    return Err(failed(&code, api_message(status, &code)));
                }
                Err(e) if attempt == MAX_ATTEMPTS_PER_PART => {
                    let TransferError::Failed(f) = call_failure(e) else {
                        unreachable!("call_failure sempre devolve Failed")
                    };
                    return Err(failed(
                        &f.code,
                        format!("falha ao enviar parte após 3 tentativas: {}", f.message),
                    ));
                }
                Err(_) => tokio::time::sleep(self.retry_delay * attempt).await,
            }
        }
        Err(failed(
            "remote_error",
            "falha ao enviar parte após 3 tentativas",
        ))
    }

    async fn commit(&self, upload_id: &str, sha256: &str) -> Result<String, TransferError> {
        let r = self
            .call(
                Method::POST,
                &format!("/hephaestus/lora/uploads/{upload_id}/commit"),
                COMMIT_TIMEOUT,
                Some(serde_json::json!({"sha256": sha256})),
                None,
            )
            .await
            .map_err(|e| match e {
                CallError::Api {
                    status: 404, code, ..
                } if code == "upload_not_found" => TransferError::UploadLost,
                e => call_failure(e),
            })?;
        let c: CommitResponse = read_json(r).await.ok_or_else(|| {
            failed(
                "remote_error",
                "resposta inválida do ComfyUI ao finalizar o envio",
            )
        })?;
        Ok(c.path)
    }

    /// Uma tentativa completa de envio: init → partes → commit. Em falha
    /// definitiva aborta o upload remoto (DELETE). Devolve o caminho remoto.
    /// `progress` recebe o total de bytes enviados após cada parte.
    pub async fn transfer<R: AsyncRead + Unpin>(
        &self,
        filename: &str,
        overwrite: bool,
        total: u64,
        reader: &mut R,
        progress: &mut (dyn FnMut(u64) + Send),
    ) -> Result<String, TransferError> {
        let init = self.init(filename, total, overwrite).await?;
        let chunk = init.chunk_size.clamp(1, MAX_CHUNK_SIZE);
        let result = self
            .send_all(&init.upload_id, total, chunk, reader, progress)
            .await;
        if let Err(TransferError::Failed(_)) = &result {
            self.abort(&init.upload_id).await;
        }
        result
    }

    async fn send_all<R: AsyncRead + Unpin>(
        &self,
        upload_id: &str,
        total: u64,
        chunk: u64,
        reader: &mut R,
        progress: &mut (dyn FnMut(u64) + Send),
    ) -> Result<String, TransferError> {
        let mut hasher = Sha256::new();
        let mut offset = 0u64;
        while offset < total {
            let want = chunk.min(total - offset) as usize;
            let mut buf = vec![0u8; want];
            reader.read_exact(&mut buf).await.map_err(|_| {
                failed(
                    "source_not_found",
                    "falha ao ler o arquivo de origem no storage",
                )
            })?;
            hasher.update(&buf);
            self.put_part(upload_id, offset, &Bytes::from(buf)).await?;
            offset += want as u64;
            progress(offset);
        }
        // O tamanho registrado é a verdade do envio: sobra de dados no storage
        // significaria enviar um arquivo truncado como se fosse completo.
        let mut probe = [0u8; 1];
        if !matches!(reader.read(&mut probe).await, Ok(0)) {
            return Err(failed(
                "source_invalid",
                "o tamanho do arquivo no storage difere do registrado",
            ));
        }
        self.commit(upload_id, &hex::encode(hasher.finalize()))
            .await
    }
}

fn describe(e: CallError) -> String {
    match e {
        CallError::Network => "sem conexão com o destino ComfyUI".to_string(),
        CallError::Redirect => {
            "o destino respondeu com redirecionamento (não seguido); use a URL final do ComfyUI"
                .to_string()
        }
        CallError::Api { status, code, .. } => {
            if status == StatusCode::NOT_FOUND.as_u16() && code.is_empty() {
                "o custom node do Hephaestus não foi encontrado no destino".to_string()
            } else {
                api_message(status, &code)
            }
        }
    }
}
