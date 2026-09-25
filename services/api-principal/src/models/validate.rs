//! Validação pura de upload/download de modelos (I.4a — ADR-0012 D3/D4).
//!
//! Funções sem I/O nem estado — testáveis sem banco nem S3.

/// Engines suportadas (yolo, world, diffusion, clip).
pub const ALLOWED_ENGINES: &[&str] = &["yolo", "world", "diffusion", "clip"];

/// Extensões aceitas para modelos.
pub const ALLOWED_EXTENSIONS: &[&str] = &[".pt", ".safetensors"];

/// Extensão obrigatória legada para yolo na v1 (D3).
pub const YOLO_EXTENSION: &str = ".pt";

/// Magic bytes do torch.save (zip): `PK\x03\x04`.
pub const MAGIC_PK: &[u8] = b"PK\x03\x04";

/// Teto por arquivo: 8 GiB (D4 — ADR-0023 D4, SDXL fp16 ≈ 6.5 GiB).
pub const MODEL_MAX_FILE_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// Limite do corpo total multipart: 8 GiB + 8 MiB de envelope (D4).
pub const MODEL_UPLOAD_BODY_LIMIT_BYTES: usize = MODEL_MAX_FILE_BYTES as usize + 8 * 1024 * 1024;

/// Máximo de redirects no download (D4).
pub const MAX_REDIRECTS: u32 = 5;

/// Validador de upload de modelo.
#[derive(Debug, PartialEq)]
pub struct UploadValidation {
    pub engine: String,
    pub name: String,
}

/// Resultado da validação de upload.
#[derive(Debug, PartialEq)]
pub enum UploadError {
    /// Engine não suportada.
    InvalidEngine,
    /// Extensão inválida para a engine (.pt ou .safetensors).
    InvalidExtension,
    /// Magic bytes divergem do formato esperado.
    InvalidMagic,
    /// Nome sanitizado resultado em vazio.
    InvalidName,
}

/// Valida engine + nome para upload (D3).
///
/// O `name` é o display name do usuário — NÃO precisa conter extensão.
/// A extensão é validada separadamente em [`validate_raw_filename`] sobre o
/// arquivo real.
pub fn validate_upload(engine: &str, name: Option<&str>) -> Result<UploadValidation, UploadError> {
    if !ALLOWED_ENGINES.contains(&engine) {
        return Err(UploadError::InvalidEngine);
    }

    let name = match name {
        Some(n) => {
            let sanitized = sanitize_model_name(n);
            if sanitized.is_empty() || sanitized.len() > 255 {
                return Err(UploadError::InvalidName);
            }
            sanitized
        }
        None => "model".to_string(),
    };

    Ok(UploadValidation {
        engine: engine.to_string(),
        name,
    })
}

/// Valida extensão do nome de arquivo bruto (multipart `filename` ou basename
/// de URL de download).
///
/// Retorna a extensão autorizada (lowercase, com ponto) ou `Err(InvalidExtension)`.
pub fn validate_raw_filename(filename: &str) -> Result<String, UploadError> {
    let lower = filename.to_lowercase();
    if let Some(ext) = ALLOWED_EXTENSIONS.iter().find(|ext| lower.ends_with(*ext)) {
        Ok(ext.to_string())
    } else {
        Err(UploadError::InvalidExtension)
    }
}

/// Sanitiza nome de modelo: mantém `[A-Za-z0-9_.-]`, colapsa runs, trunca 255.
pub fn sanitize_model_name(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    // Remove . e - das pontas.
    let trimmed = out.trim_matches(['.', '-']).to_string();
    let mut result: String = trimmed.chars().take(255).collect();
    result = result.trim_matches(['.', '-']).to_string();
    if result.is_empty() {
        return String::new();
    }
    result
}

/// Valida magic bytes de um arquivo de modelo (.pt ou .safetensors).
pub fn validate_magic(header: &[u8], filename: &str) -> bool {
    let lower = filename.to_lowercase();
    if lower.ends_with(".safetensors") {
        validate_safetensors_header(header)
    } else if lower.ends_with(".pt") {
        header.len() >= 4 && header[..4] == *MAGIC_PK
    } else {
        false
    }
}

/// Valida cabeçalho de arquivo safetensors.
/// Safetensors começa com 8 bytes indicando o tamanho do header JSON em little-endian,
/// seguido imediatamente por `{` (JSON).
pub fn validate_safetensors_header(header: &[u8]) -> bool {
    if header.len() < 9 {
        return false;
    }
    let size_bytes: [u8; 8] = match header[..8].try_into() {
        Ok(b) => b,
        Err(_) => return false,
    };
    let header_size = u64::from_le_bytes(size_bytes);
    (2..=100 * 1024 * 1024).contains(&header_size) && header[8] == b'{'
}

/// Valida URL de download (D4): scheme http/https.
pub fn validate_download_url(url: &str) -> Result<url::Url, &'static str> {
    let parsed = url::Url::parse(url).map_err(|_| "invalid url")?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed),
        _ => Err("invalid scheme"),
    }
}

/// Verifica se um hostname está na allow-list (E1).
/// Sufixo por domínio: `huggingface.co` aceita `huggingface.co` e `*.huggingface.co`.
pub fn host_allowed(hostname: &str, allowed_hosts: &[String]) -> bool {
    for allowed in allowed_hosts {
        let allowed = allowed.trim().to_lowercase();
        if allowed.is_empty() {
            continue;
        }
        // Sufixo por domínio: `*.example.com` aceita qualquer subdomínio.
        if let Some(suffix) = allowed.strip_prefix("*.") {
            if hostname == suffix || hostname.ends_with(&format!(".{suffix}")) {
                return true;
            }
        } else if hostname == allowed {
            return true;
        }
    }
    false
}

/// Verifica se um IP é privado/metadata (D4 — deny ranges).
pub fn is_private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let octets = v4.octets();
            let addr = u32::from_be_bytes(octets);
            // 127.0.0.0/8
            if (addr & 0xFF000000) == 0x7F000000 {
                return true;
            }
            // 10.0.0.0/8
            if (addr & 0xFF000000) == 0x0A000000 {
                return true;
            }
            // 172.16.0.0/12
            if (addr & 0xFFF00000) == 0xAC100000 {
                return true;
            }
            // 192.168.0.0/16
            if (addr & 0xFFFF0000) == 0xC0A80000 {
                return true;
            }
            // 169.254.0.0/16
            if (addr & 0xFFFF0000) == 0xA9FE0000 {
                return true;
            }
            false
        }
        std::net::IpAddr::V6(v6) => {
            // ::1
            if v6.is_loopback() {
                return true;
            }
            // fc00::/7 (ULA)
            let segments = v6.segments();
            if (segments[0] & 0xFE00) == 0xFC00 {
                return true;
            }
            // fe80::/10 (link-local)
            if (segments[0] & 0xFFC0) == 0xFE80 {
                return true;
            }
            false
        }
    }
}

/// Extrai basename de uma URL (sanitizado para ser filename seguro).
pub fn url_basename(url: &url::Url) -> String {
    let path = url.path();
    let raw = path.rsplit('/').next().unwrap_or("model");
    // Remove query params que possam ter ficado.
    let base = raw.split('?').next().unwrap_or(raw);
    sanitize_model_name(base)
}

// ---------------------------------------------------------------------------
// Safetensors header sniff (ADR-0023 D4)
// ---------------------------------------------------------------------------

/// Arquiteturas de difusão suportadas.
pub const ALLOWED_ARCHS: &[&str] = &["flux-2-klein-4b", "sdxl", "sd15", "qwen-image-2.1"];

/// Kinds de modelo suportados (text_encoder = text encoders custom flux-2,
/// fatia feat/pesos-custom-flux2 — só admite arch flux-2-klein-4b, regra
/// aplicada em `resolve_kind_arch`).
pub const ALLOWED_KINDS: &[&str] = &["lora", "checkpoint", "text_encoder"];

/// Teto do header JSON do safetensors: 2 MiB (headers reais de SDXL
/// são centenas de KB; 2 MiB é generoso).
pub const SAFETENSORS_HEADER_MAX: usize = 2 * 1024 * 1024;

/// Resultado do sniff de safetensors.
#[derive(Debug, Clone, PartialEq)]
pub struct SafetensorsSniff {
    pub kind: String,    // "lora" ou "checkpoint"
    pub arch: String,    // "flux-2-klein-4b", "sdxl" ou "sd15"
    pub confidence: f64, // 0.0 a 1.0 (qualidade do sniff)
}

/// Erro do sniff.
#[derive(Debug, Clone, PartialEq)]
pub enum SniffError {
    /// Header muito pequeno ou inválido.
    InvalidHeader,
    /// JSON do header não parseável.
    InvalidJson,
    /// Chaves não encaixam em nenhuma regra conhecida.
    UnknownClassification,
}

/// Lê e parseia o header JSON de um safetensors a partir dos primeiros bytes.
///
/// Formato safetensors: 8 bytes LE (comprimento N do header) + N bytes JSON.
/// Retorna o mapa de chaves do JSON (tensor_name → metadata).
pub fn parse_safetensors_header(
    header_bytes: &[u8],
) -> Result<serde_json::Map<String, serde_json::Value>, SniffError> {
    if header_bytes.len() < 9 {
        return Err(SniffError::InvalidHeader);
    }
    let size_bytes: [u8; 8] = header_bytes[..8]
        .try_into()
        .map_err(|_| SniffError::InvalidHeader)?;
    let header_size = u64::from_le_bytes(size_bytes) as usize;

    if header_size < 2 || header_size > SAFETENSORS_HEADER_MAX {
        return Err(SniffError::InvalidHeader);
    }
    if header_bytes.len() < 8 + header_size {
        return Err(SniffError::InvalidHeader);
    }
    if header_bytes[8] != b'{' {
        return Err(SniffError::InvalidHeader);
    }

    let json_slice = &header_bytes[8..8 + header_size];
    let json: serde_json::Value =
        serde_json::from_slice(json_slice).map_err(|_| SniffError::InvalidJson)?;

    match json {
        serde_json::Value::Object(map) => Ok(map),
        _ => Err(SniffError::InvalidJson),
    }
}

/// Classifica o tipo de modelo a partir das chaves do header safetensors.
///
/// Regras (ADR-0023 D4):
/// - Chaves contêm `.lora_` / `lora_A` / `lora_B` / peft patterns → kind=lora
///   - Prefixo `transformer.*` ou `transformer_blocks.*` → arch flux-2-klein-4b
///   - Prefixo `conditioner`/`unet` → arch sdxl/sd15 (sdxl se conditioner presente, senão sd15)
/// - Checkpoint:
///   - `model.diffusion_model.*` + `conditioner.embedders.*` → sdxl
///   - `model.diffusion_model.*` sem conditioner → sd15
///   - Chaves de `transformer`/`guidance_embedder` → flux (checkpoint)
pub fn sniff_safetensors(
    keys: &serde_json::Map<String, serde_json::Value>,
) -> Result<SafetensorsSniff, SniffError> {
    if keys.is_empty() {
        return Err(SniffError::UnknownClassification);
    }

    let key_names: Vec<&str> = keys.keys().map(|s| s.as_str()).collect();

    // Checagem de LoRA: peft patterns.
    let is_lora = key_names.iter().any(|k| {
        k.contains(".lora_A")
            || k.contains(".lora_B")
            || k.contains("lora_A.")
            || k.contains("lora_B.")
            || k.contains(".lora_")
            || k.contains("lora_down")
            || k.contains("lora_up")
            || k.contains("lora_enable")
            || k.contains("lora_alpha")
    });

    if is_lora {
        if let Some(serde_json::Value::Object(meta)) = keys.get("__metadata__") {
            if let Some(serde_json::Value::String(bm)) = meta.get("base_model") {
                if bm.contains("qwen") {
                    return Ok(SafetensorsSniff {
                        kind: "lora".to_string(),
                        arch: "qwen-image-2.1".to_string(),
                        confidence: 0.95,
                    });
                }
            }
        }
        let has_qwen = key_names
            .iter()
            .any(|k| k.contains("qwen") || k.contains("qwen_image"));
        // Deriva arch a partir dos prefixos das chaves.
        let has_transformer = key_names
            .iter()
            .any(|k| k.starts_with("transformer.") || k.starts_with("transformer_blocks."));
        let has_guidance = key_names.iter().any(|k| k.contains("guidance_embedder"));
        let has_conditioner = key_names.iter().any(|k| k.starts_with("conditioner."));
        let has_unet = key_names.iter().any(|k| k.starts_with("unet."));

        let arch = if has_qwen {
            "qwen-image-2.1".to_string()
        } else if has_transformer || has_guidance {
            // Flux LoRA: transformer.* ou transformer_blocks.* ou guidance_embedder
            "flux-2-klein-4b".to_string()
        } else if has_conditioner {
            // SDXL tem conditioner.embedders
            "sdxl".to_string()
        } else if has_unet {
            // unet.* sem conditioner → padrão para sdxl (mais comum para LoRA)
            "sdxl".to_string()
        } else {
            // Não dá para derivar → retorna vazio (caller deve usar hint).
            String::new()
        };

        return Ok(SafetensorsSniff {
            kind: "lora".to_string(),
            arch,
            confidence: 0.9,
        });
    }

    // Checkpoint: classificação por padrões de chaves.
    let has_diffusion_model = key_names
        .iter()
        .any(|k| k.starts_with("model.diffusion_model."));
    let has_transformer = key_names
        .iter()
        .any(|k| k.starts_with("transformer.") || k.starts_with("transformer_blocks."));
    let has_guidance = key_names.iter().any(|k| k.contains("guidance_embedder"));
    let has_conditioner = key_names
        .iter()
        .any(|k| k.starts_with("conditioner.embedders."));

    if has_transformer && has_guidance {
        // Flux checkpoint.
        return Ok(SafetensorsSniff {
            kind: "checkpoint".to_string(),
            arch: "flux-2-klein-4b".to_string(),
            confidence: 0.85,
        });
    }

    // Flux checkpoint sem guidance explícito mas com transformer.
    if has_transformer {
        return Ok(SafetensorsSniff {
            kind: "checkpoint".to_string(),
            arch: "flux-2-klein-4b".to_string(),
            confidence: 0.7,
        });
    }

    if has_diffusion_model && has_conditioner {
        // SDXL: model.diffusion_model.* + conditioner.embedders.*
        return Ok(SafetensorsSniff {
            kind: "checkpoint".to_string(),
            arch: "sdxl".to_string(),
            confidence: 0.9,
        });
    }

    if has_diffusion_model && !has_conditioner {
        // SD15: model.diffusion_model.* sem conditioner.
        return Ok(SafetensorsSniff {
            kind: "checkpoint".to_string(),
            arch: "sd15".to_string(),
            confidence: 0.85,
        });
    }

    Err(SniffError::UnknownClassification)
}

/// Resolve kind+arch a partir de hints do cliente e sniff do header.
///
/// Regras (ADR-0023 D4 + fatia feat/pesos-custom-flux2):
/// - Sniff confiante vence hint
/// - Sniff + hint conflitantes → erro
/// - Sniff desconhecido + sem hint → erro
/// - Sniff desconhecido + com hint → aceita hint
/// - kind=text_encoder só admite arch flux-2-klein-4b (outro arch ⇒ erro)
pub fn resolve_kind_arch(
    sniff: Result<SafetensorsSniff, SniffError>,
    hint_kind: Option<&str>,
    hint_arch: Option<&str>,
) -> Result<(String, String), &'static str> {
    match sniff {
        Ok(s) => {
            // Sniff OK: validar contra hints.
            if let Some(hk) = hint_kind {
                if hk != s.kind {
                    return Err("sniff and hint conflict on kind");
                }
            }
            if let Some(ha) = hint_arch {
                if !s.arch.is_empty() && ha != s.arch {
                    return Err("sniff and hint conflict on arch");
                }
            }
            // Arch vazio do sniff → usar hint se disponível.
            let arch = if s.arch.is_empty() {
                hint_arch.map(|a| a.to_string()).unwrap_or_default()
            } else {
                s.arch
            };
            // LoRA aceita arch vazio (o manager só exige arch para checkpoint).
            if arch.is_empty() && s.kind != "lora" {
                return Err("arch could not be determined; provide kind+arch hints");
            }
            // text_encoder só existe para flux-2 (encoder swap do Qwen3).
            if s.kind == "text_encoder" && !arch.is_empty() && arch != "flux-2-klein-4b" {
                return Err("text_encoder requires arch 'flux-2-klein-4b'");
            }
            Ok((s.kind, arch))
        }
        Err(SniffError::UnknownClassification) => {
            // Sniff falhou: precisa de hint.
            match (hint_kind, hint_arch) {
                (Some(k), Some(a)) => {
                    if !ALLOWED_KINDS.contains(&k) || !ALLOWED_ARCHS.contains(&a) {
                        return Err("invalid kind or arch");
                    }
                    if k == "text_encoder" && a != "flux-2-klein-4b" {
                        return Err("text_encoder requires arch 'flux-2-klein-4b'");
                    }
                    Ok((k.to_string(), a.to_string()))
                }
                _ => Err("kind/arch could not be determined; provide kind+arch hints"),
            }
        }
        Err(_) => Err("invalid safetensors header"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_upload_yolo_ok() {
        let v = validate_upload("yolo", Some("best.pt")).unwrap();
        assert_eq!(v.engine, "yolo");
        assert_eq!(v.name, "best.pt");
    }

    #[test]
    fn validate_upload_world_ok() {
        let v = validate_upload("world", Some("yolov8x-worldv2.pt")).unwrap();
        assert_eq!(v.engine, "world");
        assert_eq!(v.name, "yolov8x-worldv2.pt");
    }

    #[test]
    fn validate_upload_world_default_name() {
        let v = validate_upload("world", None).unwrap();
        assert_eq!(v.name, "model");
    }

    #[test]
    fn validate_upload_yolo_default_name() {
        let v = validate_upload("yolo", None).unwrap();
        assert_eq!(v.name, "model");
    }

    #[test]
    fn validate_upload_invalid_engine() {
        assert_eq!(
            validate_upload("unsupported", Some("best.pt")),
            Err(UploadError::InvalidEngine)
        );
    }

    #[test]
    fn validate_upload_safetensors_ok() {
        let v = validate_upload("diffusion", Some("flux1-dev.safetensors")).unwrap();
        assert_eq!(v.engine, "diffusion");
        assert_eq!(v.name, "flux1-dev.safetensors");
    }

    #[test]
    fn validate_upload_name_without_ext_ok() {
        // Nome de exibição sem extensão é aceito (extensão vem do arquivo).
        let v = validate_upload("yolo", Some("meu lora v2")).unwrap();
        assert_eq!(v.engine, "yolo");
        assert_eq!(v.name, "meu_lora_v2");
    }

    #[test]
    fn validate_upload_empty_name_after_sanitize() {
        assert_eq!(
            validate_upload("yolo", Some("...")),
            Err(UploadError::InvalidName)
        );
    }

    #[test]
    fn validate_raw_filename_pt_ok() {
        assert_eq!(validate_raw_filename("model.pt").unwrap(), ".pt");
    }

    #[test]
    fn validate_raw_filename_safetensors_ok() {
        assert_eq!(
            validate_raw_filename("model.safetensors").unwrap(),
            ".safetensors"
        );
    }

    #[test]
    fn validate_raw_filename_pth_rejected() {
        assert_eq!(
            validate_raw_filename("model.pth"),
            Err(UploadError::InvalidExtension)
        );
    }

    #[test]
    fn validate_raw_filename_no_ext_rejected() {
        assert_eq!(
            validate_raw_filename("model"),
            Err(UploadError::InvalidExtension)
        );
    }

    #[test]
    fn validate_magic_pt_ok() {
        assert!(validate_magic(b"PK\x03\x04rest", "best.pt"));
        assert!(validate_magic(b"PK\x03\x04", "model.pt"));
    }

    #[test]
    fn validate_magic_safetensors_ok() {
        let mut header = vec![0u8; 16];
        // 10 little-endian u64:
        header[0] = 10;
        header[8] = b'{';
        assert!(validate_magic(&header, "model.safetensors"));
    }

    #[test]
    fn validate_magic_safetensors_wrong() {
        let mut header = vec![0u8; 16];
        header[0] = 10;
        header[8] = b'X'; // não é '{'
        assert!(!validate_magic(&header, "model.safetensors"));
    }

    #[test]
    fn validate_magic_too_short() {
        assert!(!validate_magic(b"PK", "model.pt"));
    }

    #[test]
    fn validate_magic_wrong() {
        assert!(!validate_magic(b"\x89PNG", "model.pt"));
    }

    #[test]
    fn host_allowed_exact() {
        let allowed = vec!["huggingface.co".to_string(), "civitai.com".to_string()];
        assert!(host_allowed("huggingface.co", &allowed));
        assert!(host_allowed("civitai.com", &allowed));
        assert!(!host_allowed("evil.com", &allowed));
    }

    #[test]
    fn host_allowed_wildcard() {
        let allowed = vec!["*.githubusercontent.com".to_string()];
        assert!(host_allowed("raw.githubusercontent.com", &allowed));
        assert!(host_allowed("avatars.githubusercontent.com", &allowed));
        assert!(!host_allowed("github.com", &allowed));
    }

    #[test]
    fn host_allowed_exact_only_no_subdomain() {
        // A2: entrada sem * casa SOMENTE host exato; subdomínio exige *.
        let allowed = vec!["huggingface.co".to_string()];
        assert!(host_allowed("huggingface.co", &allowed));
        assert!(!host_allowed("cdn-lfs.huggingface.co", &allowed));
    }

    #[test]
    fn host_allowed_wildcard_subdomain() {
        // A2: *.huggingface.co aceita subdomínios.
        let allowed = vec!["*.huggingface.co".to_string()];
        assert!(host_allowed("cdn-lfs.huggingface.co", &allowed));
        assert!(host_allowed("huggingface.co", &allowed));
    }

    #[test]
    fn is_private_ip_127() {
        assert!(is_private_ip("127.0.0.1".parse().unwrap()));
        assert!(is_private_ip("127.255.255.255".parse().unwrap()));
    }

    #[test]
    fn is_private_ip_10() {
        assert!(is_private_ip("10.0.0.1".parse().unwrap()));
        assert!(is_private_ip("10.255.255.255".parse().unwrap()));
    }

    #[test]
    fn is_private_ip_172() {
        assert!(is_private_ip("172.16.0.1".parse().unwrap()));
        assert!(is_private_ip("172.31.255.255".parse().unwrap()));
        assert!(!is_private_ip("172.32.0.1".parse().unwrap()));
    }

    #[test]
    fn is_private_ip_192() {
        assert!(is_private_ip("192.168.0.1".parse().unwrap()));
        assert!(!is_private_ip("192.169.0.1".parse().unwrap()));
    }

    #[test]
    fn is_private_ip_link_local() {
        assert!(is_private_ip("169.254.1.1".parse().unwrap()));
    }

    #[test]
    fn is_private_ip_public() {
        assert!(!is_private_ip("8.8.8.8".parse().unwrap()));
        assert!(!is_private_ip("1.1.1.1".parse().unwrap()));
    }

    #[test]
    fn is_private_ip_ipv6_loopback() {
        assert!(is_private_ip("::1".parse().unwrap()));
    }

    #[test]
    fn is_private_ip_ipv6_ula() {
        assert!(is_private_ip("fc00::1".parse().unwrap()));
    }

    #[test]
    fn is_private_ip_ipv6_link_local() {
        assert!(is_private_ip("fe80::1".parse().unwrap()));
    }

    #[test]
    fn validate_download_url_ok() {
        assert!(validate_download_url("https://example.com/model.pt").is_ok());
        assert!(validate_download_url("http://example.com/model.pt").is_ok());
    }

    #[test]
    fn validate_download_url_bad_scheme() {
        assert!(validate_download_url("ftp://example.com/model.pt").is_err());
    }

    #[test]
    fn url_basename_normal() {
        let u = url::Url::parse("https://example.com/models/best.pt").unwrap();
        assert_eq!(url_basename(&u), "best.pt");
    }

    #[test]
    fn sanitize_model_name_clean() {
        assert_eq!(sanitize_model_name("best.pt"), "best.pt");
    }

    #[test]
    fn sanitize_model_name_dirty() {
        assert_eq!(
            sanitize_model_name("best model (1).pt"),
            "best_model__1_.pt"
        );
    }

    #[test]
    fn sanitize_model_name_empty_after() {
        assert_eq!(sanitize_model_name("..."), "");
    }

    // --- Safetensors sniff tests ---

    /// Constrói bytes de header safetensors sintético com um JSON de chaves.
    fn build_fake_safetensors_header(keys: &[&str]) -> Vec<u8> {
        let mut map = serde_json::Map::new();
        for k in keys {
            // Cada tensor precisa de um valor mínimo (offsets/tensordata).
            map.insert(
                k.to_string(),
                serde_json::json!({"dtype": "F16", "shape": [1, 1], "data_offsets": [0, 2]}),
            );
        }
        let json = serde_json::Value::Object(map);
        let json_bytes = serde_json::to_vec(&json).unwrap();
        let len = json_bytes.len() as u64;
        let mut header = len.to_le_bytes().to_vec();
        header.extend_from_slice(&json_bytes);
        header
    }

    #[test]
    fn sniff_sdxl_checkpoint() {
        let header = build_fake_safetensors_header(&[
            "model.diffusion_model.unet blocks.0.weight",
            "conditioner.embedders.0.proj.weight",
        ]);
        let map = parse_safetensors_header(&header).unwrap();
        let sniff = sniff_safetensors(&map).unwrap();
        assert_eq!(sniff.kind, "checkpoint");
        assert_eq!(sniff.arch, "sdxl");
    }

    #[test]
    fn sniff_sd15_checkpoint() {
        let header = build_fake_safetensors_header(&["model.diffusion_model.unet blocks.0.weight"]);
        let map = parse_safetensors_header(&header).unwrap();
        let sniff = sniff_safetensors(&map).unwrap();
        assert_eq!(sniff.kind, "checkpoint");
        assert_eq!(sniff.arch, "sd15");
    }

    #[test]
    fn sniff_flux_checkpoint() {
        let header = build_fake_safetensors_header(&[
            "transformer_blocks.0.attn.to_q.weight",
            "guidance_embedder.linear.weight",
        ]);
        let map = parse_safetensors_header(&header).unwrap();
        let sniff = sniff_safetensors(&map).unwrap();
        assert_eq!(sniff.kind, "checkpoint");
        assert_eq!(sniff.arch, "flux-2-klein-4b");
    }

    #[test]
    fn sniff_flux_lora() {
        let header = build_fake_safetensors_header(&[
            "transformer_blocks.0.attn.to_q.lora_A.weight",
            "transformer_blocks.0.attn.to_q.lora_B.weight",
        ]);
        let map = parse_safetensors_header(&header).unwrap();
        let sniff = sniff_safetensors(&map).unwrap();
        assert_eq!(sniff.kind, "lora");
        assert_eq!(sniff.arch, "flux-2-klein-4b");
    }
    #[test]
    fn sniff_qwen_lora() {
        let header = build_fake_safetensors_header(&[
            "transformer.layers.0.attention.to_q.lora_A.weight",
            "qwen_image_blocks.0.lora_B.weight",
        ]);
        let map = parse_safetensors_header(&header).unwrap();
        let sniff = sniff_safetensors(&map).unwrap();
        assert_eq!(sniff.kind, "lora");
        assert_eq!(sniff.arch, "qwen-image-2.1");
    }

    #[test]
    fn sniff_sdxl_lora() {
        let header = build_fake_safetensors_header(&[
            "unet.blocks.0.attention.lora_A.weight",
            "unet.blocks.0.attention.lora_B.weight",
        ]);
        let map = parse_safetensors_header(&header).unwrap();
        let sniff = sniff_safetensors(&map).unwrap();
        assert_eq!(sniff.kind, "lora");
        assert_eq!(sniff.arch, "sdxl");
    }

    #[test]
    fn sniff_unknown_keys() {
        let header = build_fake_safetensors_header(&["something_totally_unknown"]);
        let map = parse_safetensors_header(&header).unwrap();
        let result = sniff_safetensors(&map);
        assert_eq!(result, Err(SniffError::UnknownClassification));
    }

    #[test]
    fn resolve_sniff_wins_over_hint() {
        let sniff = Ok(SafetensorsSniff {
            kind: "checkpoint".to_string(),
            arch: "sdxl".to_string(),
            confidence: 0.9,
        });
        let result = resolve_kind_arch(sniff, Some("lora"), Some("sd15"));
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("conflict"));
    }

    #[test]
    fn resolve_sniff_ok_hint_agrees() {
        let sniff = Ok(SafetensorsSniff {
            kind: "checkpoint".to_string(),
            arch: "sdxl".to_string(),
            confidence: 0.9,
        });
        let (kind, arch) = resolve_kind_arch(sniff, Some("checkpoint"), Some("sdxl")).unwrap();
        assert_eq!(kind, "checkpoint");
        assert_eq!(arch, "sdxl");
    }

    #[test]
    fn resolve_unknown_with_hint() {
        let sniff: Result<SafetensorsSniff, SniffError> = Err(SniffError::UnknownClassification);
        let (kind, arch) = resolve_kind_arch(sniff, Some("lora"), Some("flux-2-klein-4b")).unwrap();
        assert_eq!(kind, "lora");
        assert_eq!(arch, "flux-2-klein-4b");
    }

    #[test]
    fn resolve_unknown_without_hint() {
        let sniff: Result<SafetensorsSniff, SniffError> = Err(SniffError::UnknownClassification);
        let result = resolve_kind_arch(sniff, None, None);
        assert!(result.is_err());
    }

    #[test]
    fn resolve_sniff_unknown_arch_uses_hint() {
        let sniff = Ok(SafetensorsSniff {
            kind: "lora".to_string(),
            arch: String::new(), // arch não derivável
            confidence: 0.9,
        });
        let (kind, arch) = resolve_kind_arch(sniff, None, Some("sdxl")).unwrap();
        assert_eq!(kind, "lora");
        assert_eq!(arch, "sdxl");
    }

    #[test]
    fn resolve_lora_without_arch_ok() {
        // LoRA clássica sem prefixo determinístico → arch vazio é aceito.
        let sniff = Ok(SafetensorsSniff {
            kind: "lora".to_string(),
            arch: String::new(),
            confidence: 0.9,
        });
        let (kind, arch) = resolve_kind_arch(sniff, None, None).unwrap();
        assert_eq!(kind, "lora");
        assert_eq!(arch, "");
    }

    #[test]
    fn resolve_checkpoint_without_arch_err() {
        // Checkpoint sem arch ainda dá erro.
        let sniff = Ok(SafetensorsSniff {
            kind: "checkpoint".to_string(),
            arch: String::new(),
            confidence: 0.9,
        });
        let result = resolve_kind_arch(sniff, None, None);
        assert!(result.is_err());
    }
    #[test]
    fn resolve_text_encoder_flux2_ok() {
        // hint text_encoder + arch flux-2 ⇒ aceita (único arch permitido).
        let sniff: Result<SafetensorsSniff, SniffError> = Err(SniffError::UnknownClassification);
        let (kind, arch) =
            resolve_kind_arch(sniff, Some("text_encoder"), Some("flux-2-klein-4b")).unwrap();
        assert_eq!(kind, "text_encoder");
        assert_eq!(arch, "flux-2-klein-4b");
    }

    #[test]
    fn resolve_text_encoder_wrong_arch_err() {
        // hint text_encoder + arch sdxl/sd15 ⇒ 400.
        let sniff: Result<SafetensorsSniff, SniffError> = Err(SniffError::UnknownClassification);
        assert!(resolve_kind_arch(sniff, Some("text_encoder"), Some("sdxl")).is_err());
        let sniff2: Result<SafetensorsSniff, SniffError> = Err(SniffError::UnknownClassification);
        assert!(resolve_kind_arch(sniff2, Some("text_encoder"), Some("sd15")).is_err());
    }

    #[test]
    fn resolve_text_encoder_sniff_wrong_arch_err() {
        // sniff text_encoder com arch não-flux-2 ⇒ erro (nunca persiste inválido).
        let sniff = Ok(SafetensorsSniff {
            kind: "text_encoder".to_string(),
            arch: "sdxl".to_string(),
            confidence: 0.9,
        });
        assert!(resolve_kind_arch(sniff, None, None).is_err());
    }

    #[test]
    fn parse_safetensors_header_too_short() {
        assert_eq!(
            parse_safetensors_header(&[0u8; 4]),
            Err(SniffError::InvalidHeader)
        );
    }

    #[test]
    fn parse_safetensors_header_not_json() {
        let mut header = vec![0u8; 16];
        let len_bytes = 8u64.to_le_bytes();
        header[..8].copy_from_slice(&len_bytes);
        header[8] = b'X'; // não é '{'
        assert_eq!(
            parse_safetensors_header(&header),
            Err(SniffError::InvalidHeader)
        );
    }
}
