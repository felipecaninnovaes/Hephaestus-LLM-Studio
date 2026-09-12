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

/// Teto por arquivo: 2 GiB (D3).
pub const MODEL_MAX_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Limite do corpo total multipart: 2 GiB + 8 MiB de envelope (D3).
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
        None => "model.pt".to_string(),
    };

    let lower = name.to_lowercase();
    if !ALLOWED_EXTENSIONS.iter().any(|ext| lower.ends_with(ext)) {
        return Err(UploadError::InvalidExtension);
    }

    Ok(UploadValidation {
        engine: engine.to_string(),
        name,
    })
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
        assert_eq!(v.name, "model.pt");
    }

    #[test]
    fn validate_upload_yolo_default_name() {
        let v = validate_upload("yolo", None).unwrap();
        assert_eq!(v.name, "model.pt");
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
    fn validate_upload_invalid_extension() {
        assert_eq!(
            validate_upload("yolo", Some("best.pth")),
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
}
