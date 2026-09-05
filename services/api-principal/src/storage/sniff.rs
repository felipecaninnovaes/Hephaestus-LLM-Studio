//! Sniff de tipo de mídia pelo conteúdo (ADR-0003 D2/D5).
//!
//! O `media_type` canônico vem dos bytes do arquivo, nunca do nome ou do
//! `content-type` declarado no form (ambos são decorativos e não confiáveis).

/// Tipo de mídia suportado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    Jpeg,
    Png,
    WebP,
}

impl MediaType {
    /// Valor canônico no banco (`media_type`).
    pub fn as_db(&self) -> &'static str {
        match self {
            Self::Jpeg => "jpeg",
            Self::Png => "png",
            Self::WebP => "webp",
        }
    }

    /// Extensão canônica na chave do objeto (vem do sniff, nunca do nome).
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::WebP => "webp",
        }
    }

    /// Content-Type canônico.
    pub fn content_type(&self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::WebP => "image/webp",
        }
    }
}

/// Sniff pelos magic bytes: PNG = `89 50 4E 47 0D 0A 1A 0A`;
/// JPEG = `FF D8 FF`; WEBP = `RIFF....WEBP` (bytes 0..4 e 8..12).
/// Ordem irrelevante (mágicas disjuntas). Prefixo curto ⇒ `None`.
pub fn sniff(head: &[u8]) -> Option<MediaType> {
    const PNG: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    if head.len() >= PNG.len() && &head[..8] == PNG {
        return Some(MediaType::Png);
    }
    if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        return Some(MediaType::WebP);
    }
    if head.len() >= 3 && head[0] == 0xFF && head[1] == 0xD8 && head[2] == 0xFF {
        return Some(MediaType::Jpeg);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniff_png() {
        let head = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
        assert_eq!(sniff(&head), Some(MediaType::Png));
    }

    #[test]
    fn sniff_jpeg() {
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]), Some(MediaType::Jpeg));
    }

    #[test]
    fn sniff_webp() {
        let mut head = vec![0u8; 12];
        head[0..4].copy_from_slice(b"RIFF");
        head[8..12].copy_from_slice(b"WEBP");
        assert_eq!(sniff(&head), Some(MediaType::WebP));
    }

    #[test]
    fn gif_nao_suportado() {
        assert_eq!(sniff(b"GIF89a...."), None);
    }

    #[test]
    fn prefixo_curto_none() {
        assert_eq!(sniff(&[0xFF, 0xD8]), None);
        assert_eq!(sniff(&[]), None);
    }
}
