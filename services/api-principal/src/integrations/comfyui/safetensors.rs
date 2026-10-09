//! Reescrita do header safetensors no caminho do envio ao ComfyUI.
//!
//! Formato: `u64 LE = N` + `N` bytes de JSON + dados dos tensores. A etapa
//! renomeia chaves do JSON (regras do spike, ComfyUI v0.39.2) e, quando o
//! alpha treinado difere do rank, ACRESCENTA tensores escalares `<mod>.alpha`
//! no fim da região de dados. Offsets existentes e bytes originais dos tensores
//! ficam intactos — o stream de dados é copiado sem alteração.

use serde_json::{Map, Value};
use tokio::io::{AsyncRead, AsyncReadExt};

pub type Header = Map<String, Value>;
/// Mapeia uma chave de tensor para a chave esperada pelo ComfyUI (`None` = mantém).
pub type KeyMapper = fn(&str) -> Option<String>;

/// Teto do header (safetensors do HF usa 100 MB; LoRA real tem poucos MB).
const MAX_HEADER_BYTES: u64 = 100 * 1024 * 1024;
const METADATA_KEY: &str = "__metadata__";

#[derive(Debug, PartialEq, Eq)]
pub enum HeaderError {
    /// Arquivo menor que o prefixo/ header declarado.
    Truncated,
    /// Tamanho do header absurdo ou inconsistente com o arquivo.
    BadLength,
    /// JSON do header inválido (ou não é objeto).
    BadJson,
    /// Duas chaves distintas viram a mesma após a renomeação.
    KeyCollision,
    Io,
}

impl HeaderError {
    pub fn message_pt(&self) -> &'static str {
        match self {
            Self::Truncated => "arquivo safetensors truncado",
            Self::BadLength => "header safetensors com tamanho inválido",
            Self::BadJson => "header safetensors não é JSON válido",
            Self::KeyCollision => "renomeação de chaves geraria nomes duplicados",
            Self::Io => "falha ao ler o arquivo safetensors do storage",
        }
    }
}

/// Prefixos PEFT que o ComfyUI não reconhece (regra 1, todas as arquiteturas).
const PEFT_PREFIXES: [&str; 3] = [
    "base_model.model.",
    "unet.base_model.model.",
    "transformer.base_model.model.",
];

fn strip_peft(key: &str) -> &str {
    PEFT_PREFIXES
        .iter()
        .find_map(|p| key.strip_prefix(p))
        .unwrap_or(key)
}

fn changed(old: &str, new: String) -> Option<String> {
    (old != new).then_some(new)
}

/// Regra 1 apenas (Klein 4B/9B, Qwen-Image 2.1 e arquitetura desconhecida).
fn map_base(key: &str) -> Option<String> {
    changed(key, strip_peft(key).to_string())
}

/// Regras 1 + 2 (UNet diffusers de SD1.5/SDXL: o ComfyUI só indexa `.processor.*`).
fn map_sd(key: &str) -> Option<String> {
    let key = strip_peft(key);
    for proj in ["to_q", "to_k", "to_v"] {
        for ab in ["lora_A", "lora_B"] {
            let tail = format!(".{proj}.{ab}.weight");
            if let Some(stem) = key.strip_suffix(&tail) {
                return Some(format!("{stem}.processor.{proj}.{ab}.weight"));
            }
        }
    }
    for ab in ["lora_A", "lora_B"] {
        let tail = format!(".to_out.0.{ab}.weight");
        if let Some(stem) = key.strip_suffix(&tail) {
            return Some(format!("{stem}.processor.to_out.{ab}.weight"));
        }
    }
    Some(key.to_string())
}

/// Mapeador de chaves por arquitetura (slug de `models.arch`). Sempre existe:
/// a regra 1 vale para todas, inclusive arquitetura desconhecida.
pub fn key_mapper(arch: &str) -> KeyMapper {
    match arch {
        "sd15" | "sdxl" => map_sd,
        _ => map_base,
    }
}

/// Função pura `(arch, header) -> header` (só renomeia chaves);
/// `__metadata__` nunca é renomeado.
pub fn rewrite_header(arch: &str, header: &Header) -> Result<Header, HeaderError> {
    rewrite_header_with(key_mapper(arch), header)
}

pub fn rewrite_header_with(mapper: KeyMapper, header: &Header) -> Result<Header, HeaderError> {
    let mut out = Header::with_capacity(header.len());
    for (key, value) in header {
        let new_key = if key == METADATA_KEY {
            key.clone()
        } else {
            mapper(key).unwrap_or_else(|| key.clone())
        };
        if out.insert(new_key, value.clone()).is_some() {
            return Err(HeaderError::KeyCollision);
        }
    }
    Ok(out)
}

/// Resultado da etapa. Arquivo reescrito = `prefix` (u64 LE + header JSON
/// alinhado a 8) + `data_len` bytes originais dos tensores + `suffix` (tensores
/// `.alpha` acrescentados; vazio quando não há).
#[derive(Debug)]
pub struct Rewritten {
    pub prefix: Vec<u8>,
    pub data_len: u64,
    pub suffix: Vec<u8>,
    pub total_len: u64,
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn metadata_number(header: &Header, key: &str) -> Option<f64> {
    header.get(METADATA_KEY)?.get(key).and_then(number)
}

/// Fim (exclusivo) da maior região de dados declarada no header.
fn max_data_end(header: &Header) -> u64 {
    header
        .iter()
        .filter(|(k, _)| *k != METADATA_KEY)
        .filter_map(|(_, v)| v.get("data_offsets")?.get(1)?.as_u64())
        .max()
        .unwrap_or(0)
}

/// Acrescenta `<mod>.alpha` (F32 escalar) para cada `<mod>.lora_A.weight` cujo
/// rank difere de `alpha`; devolve os bytes a anexar ao fim dos dados.
/// Rank = shape[0] de `lora_A` (senão metadata `lora_rank`). Alpha == rank,
/// alpha desconhecido ou `.alpha` já existente ⇒ nada é acrescentado.
pub fn append_alpha_tensors(
    header: &mut Header,
    data_len: u64,
    alpha: Option<f64>,
) -> Result<Vec<u8>, HeaderError> {
    let Some(alpha) = alpha.filter(|a| a.is_finite()) else {
        return Ok(Vec::new());
    };
    let meta_rank = metadata_number(header, "lora_rank");
    let mut pending = Vec::new();
    for (key, value) in header.iter() {
        let Some(module) = key.strip_suffix(".lora_A.weight") else {
            continue;
        };
        let rank = value
            .get("shape")
            .and_then(|s| s.get(0))
            .and_then(Value::as_f64)
            .or(meta_rank);
        let alpha_key = format!("{module}.alpha");
        if rank.is_some_and(|r| r != alpha) && !header.contains_key(&alpha_key) {
            pending.push(alpha_key);
        }
    }
    if pending.is_empty() {
        return Ok(Vec::new());
    }
    if max_data_end(header) != data_len {
        return Err(HeaderError::BadLength);
    }
    let bytes = (alpha as f32).to_le_bytes();
    let mut suffix = Vec::with_capacity(pending.len() * 4);
    for (i, alpha_key) in pending.into_iter().enumerate() {
        let start = data_len + 4 * i as u64;
        header.insert(
            alpha_key,
            serde_json::json!({"dtype": "F32", "shape": [], "data_offsets": [start, start + 4]}),
        );
        suffix.extend_from_slice(&bytes);
    }
    Ok(suffix)
}

/// Lê o header de `reader` (posição 0 do arquivo de `total_len` bytes), aplica
/// as regras de `arch` e o alpha (`alpha_fallback` quando o header não traz
/// `lora_alpha`) e deixa o reader no início dos dados. O chamador envia
/// `prefix`, depois `data_len` bytes de `reader` sem tocá-los, depois `suffix`.
pub async fn rewrite_stream_head<R: AsyncRead + Unpin>(
    reader: &mut R,
    total_len: u64,
    arch: &str,
    alpha_fallback: Option<f64>,
) -> Result<Rewritten, HeaderError> {
    rewrite_stream_head_with(reader, total_len, key_mapper(arch), alpha_fallback).await
}

pub async fn rewrite_stream_head_with<R: AsyncRead + Unpin>(
    reader: &mut R,
    total_len: u64,
    mapper: KeyMapper,
    alpha_fallback: Option<f64>,
) -> Result<Rewritten, HeaderError> {
    let mut len_buf = [0u8; 8];
    read_exact(reader, &mut len_buf).await?;
    let n = u64::from_le_bytes(len_buf);
    if n > MAX_HEADER_BYTES || n.checked_add(8).is_none_or(|end| end > total_len) {
        return Err(HeaderError::BadLength);
    }
    let mut raw = vec![0u8; n as usize];
    read_exact(reader, &mut raw).await?;
    let header: Header = match serde_json::from_slice(&raw) {
        Ok(Value::Object(m)) => m,
        _ => return Err(HeaderError::BadJson),
    };
    let mut rewritten = rewrite_header_with(mapper, &header)?;
    let data_len = total_len - 8 - n;
    let alpha = metadata_number(&rewritten, "lora_alpha").or(alpha_fallback);
    let suffix = append_alpha_tensors(&mut rewritten, data_len, alpha)?;
    let mut json = serde_json::to_vec(&rewritten).map_err(|_| HeaderError::BadJson)?;
    while json.len() % 8 != 0 {
        json.push(b' ');
    }
    let mut prefix = Vec::with_capacity(8 + json.len());
    prefix.extend_from_slice(&(json.len() as u64).to_le_bytes());
    prefix.extend_from_slice(&json);
    Ok(Rewritten {
        total_len: prefix.len() as u64 + data_len + suffix.len() as u64,
        prefix,
        data_len,
        suffix,
    })
}

async fn read_exact<R: AsyncRead + Unpin>(r: &mut R, buf: &mut [u8]) -> Result<(), HeaderError> {
    r.read_exact(buf).await.map(|_| ()).map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            HeaderError::Truncated
        } else {
            HeaderError::Io
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn file(header: Value, data: &[u8]) -> Vec<u8> {
        let h = serde_json::to_vec(&header).unwrap();
        let mut f = (h.len() as u64).to_le_bytes().to_vec();
        f.extend(h);
        f.extend_from_slice(data);
        f
    }

    /// Reconstrói o arquivo reescrito como o runner o envia.
    async fn rewrite(src: &[u8], arch: &str, alpha: Option<f64>) -> (Vec<u8>, Rewritten) {
        let mut reader = src;
        let out = rewrite_stream_head(&mut reader, src.len() as u64, arch, alpha)
            .await
            .unwrap();
        let mut full = out.prefix.clone();
        full.extend_from_slice(&reader[..out.data_len as usize]);
        full.extend_from_slice(&out.suffix);
        assert_eq!(full.len() as u64, out.total_len);
        (full, out)
    }

    fn parse(full: &[u8]) -> (Value, &[u8]) {
        let n = u64::from_le_bytes(full[..8].try_into().unwrap()) as usize;
        (
            serde_json::from_slice(&full[8..8 + n]).unwrap(),
            &full[8 + n..],
        )
    }

    fn lora_file(prefix: &str, meta: Value) -> (Vec<u8>, Vec<u8>) {
        let data: Vec<u8> = (0..=255u8).cycle().take(64).collect();
        let f = file(
            json!({
                "__metadata__": meta,
                format!("{prefix}blk.to_q.lora_A.weight"): {"dtype": "F32", "shape": [4, 4], "data_offsets": [0, 32]},
                format!("{prefix}blk.to_q.lora_B.weight"): {"dtype": "F32", "shape": [4, 4], "data_offsets": [32, 64]},
            }),
            &data,
        );
        (f, data)
    }

    #[tokio::test]
    async fn remove_prefixo_peft_e_preserva_tensores_byte_a_byte() {
        for prefix in [
            "base_model.model.",
            "unet.base_model.model.",
            "transformer.base_model.model.",
            "",
        ] {
            let (src, data) = lora_file(prefix, json!({"format": "pt"}));
            let (full, out) = rewrite(&src, "flux-2-klein-4b", None).await;
            assert_eq!(out.prefix.len() % 8, 0);
            let (header, body) = parse(&full);
            assert_eq!(header["__metadata__"]["format"], "pt");
            assert_eq!(
                header["blk.to_q.lora_A.weight"]["data_offsets"],
                json!([0, 32])
            );
            assert_eq!(header.as_object().unwrap().len(), 3);
            assert_eq!(body, &data[..], "tensores intactos ({prefix:?})");
            assert!(out.suffix.is_empty());
        }
    }

    #[tokio::test]
    async fn sd_family_reescreve_attn_para_processor() {
        let src = file(
            json!({
                "base_model.model.m.attn1.to_q.lora_A.weight": {"dtype": "F32", "shape": [2, 2], "data_offsets": [0, 16]},
                "base_model.model.m.attn1.to_k.lora_B.weight": {"dtype": "F32", "shape": [2, 2], "data_offsets": [16, 32]},
                "base_model.model.m.attn1.to_out.0.lora_A.weight": {"dtype": "F32", "shape": [2, 2], "data_offsets": [32, 48]},
                "base_model.model.m.ff.net.0.lora_A.weight": {"dtype": "F32", "shape": [2, 2], "data_offsets": [48, 64]},
            }),
            &[0u8; 64],
        );
        for arch in ["sd15", "sdxl"] {
            let (full, _) = rewrite(&src, arch, None).await;
            let (h, _) = parse(&full);
            let keys: Vec<&str> = h.as_object().unwrap().keys().map(String::as_str).collect();
            assert!(
                keys.contains(&"m.attn1.processor.to_q.lora_A.weight"),
                "{keys:?}"
            );
            assert!(keys.contains(&"m.attn1.processor.to_k.lora_B.weight"));
            assert!(keys.contains(&"m.attn1.processor.to_out.lora_A.weight"));
            assert!(keys.contains(&"m.ff.net.0.lora_A.weight"));
        }
        // Outras arquiteturas só perdem o prefixo PEFT.
        let (full, _) = rewrite(&src, "qwen-image-2.1", None).await;
        assert!(parse(&full).0.get("m.attn1.to_q.lora_A.weight").is_some());
    }

    #[tokio::test]
    async fn alpha_diferente_do_rank_acrescenta_tensores_alpha() {
        let (src, data) = lora_file("base_model.model.", json!({"lora_alpha": "16"}));
        let (full, out) = rewrite(&src, "flux-2-klein-4b", None).await;
        let (h, body) = parse(&full);
        assert_eq!(out.suffix, 16f32.to_le_bytes());
        assert_eq!(
            h["blk.to_q.alpha"],
            json!({"dtype": "F32", "shape": [], "data_offsets": [64, 68]})
        );
        assert_eq!(&body[..64], &data[..], "originais idênticos");
        assert_eq!(&body[64..], &16f32.to_le_bytes());
        assert_eq!(h["blk.to_q.lora_A.weight"]["data_offsets"], json!([0, 32]));
    }

    #[tokio::test]
    async fn alpha_vem_do_fallback_do_job_e_um_por_modulo() {
        let data = [9u8; 96];
        let src = file(
            json!({
                "a.lora_A.weight": {"dtype": "F32", "shape": [8, 1], "data_offsets": [0, 32]},
                "a.lora_B.weight": {"dtype": "F32", "shape": [1, 8], "data_offsets": [32, 64]},
                "b.lora_A.weight": {"dtype": "F32", "shape": [8, 1], "data_offsets": [64, 96]},
            }),
            &data,
        );
        let (full, out) = rewrite(&src, "sdxl", Some(4.0)).await;
        let (h, _) = parse(&full);
        assert_eq!(out.suffix.len(), 8);
        assert_eq!(h["a.alpha"]["data_offsets"], json!([96, 100]));
        assert_eq!(h["b.alpha"]["data_offsets"], json!([100, 104]));
        assert!(h.get("a.lora_B.alpha").is_none());
    }

    #[tokio::test]
    async fn alpha_igual_ao_rank_ou_desconhecido_nao_acrescenta() {
        let (src, _) = lora_file("", json!({}));
        assert!(rewrite(&src, "sd15", Some(4.0)).await.1.suffix.is_empty());
        assert!(rewrite(&src, "sd15", None).await.1.suffix.is_empty());
        let (src, _) = lora_file("", json!({"lora_alpha": 4}));
        assert!(rewrite(&src, "sd15", Some(99.0)).await.1.suffix.is_empty());
    }

    #[tokio::test]
    async fn alpha_com_dados_inconsistentes_com_o_header_e_erro() {
        let (mut src, _) = lora_file("", json!({}));
        src.extend_from_slice(&[0; 8]); // dados além do declarado
        let r = rewrite_stream_head(&mut &src[..], src.len() as u64, "sd15", Some(16.0)).await;
        assert_eq!(r.unwrap_err(), HeaderError::BadLength);
    }

    #[test]
    fn colisao_de_chaves_e_erro() {
        fn all_same(_: &str) -> Option<String> {
            Some("k".into())
        }
        let h: Header = serde_json::from_value(json!({"a": {}, "b": {}})).unwrap();
        assert_eq!(
            rewrite_header_with(all_same, &h),
            Err(HeaderError::KeyCollision)
        );
    }

    #[tokio::test]
    async fn header_invalido_ou_truncado_e_erro() {
        let mut huge = u64::MAX.to_le_bytes().to_vec();
        huge.extend_from_slice(b"{}");
        let r = rewrite_stream_head(&mut &huge[..], huge.len() as u64, "sd15", None).await;
        assert_eq!(r.unwrap_err(), HeaderError::BadLength);

        let bad = file(json!([1, 2]), &[]);
        let r = rewrite_stream_head(&mut &bad[..], bad.len() as u64, "sd15", None).await;
        assert_eq!(r.unwrap_err(), HeaderError::BadJson);

        let r = rewrite_stream_head(&mut &[1u8, 2][..], 2, "sd15", None).await;
        assert_eq!(r.unwrap_err(), HeaderError::Truncated);
    }
}
