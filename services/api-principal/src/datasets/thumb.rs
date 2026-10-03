//! Geração sob demanda e helpers de miniaturas (thumbnails).

use std::sync::Arc;
use tokio::sync::Semaphore;

/// Teto de dimensões para decode de thumbnails: 16k x 16k pixels.
pub const THUMB_MAX_DIMENSION: u32 = 16384;
/// Teto de alocação de memória para decodificação de imagem: 256 MiB.
pub const THUMB_MAX_ALLOC_BYTES: u64 = 256 * 1024 * 1024;
/// Dimensão máxima padrão do lado maior da miniatura.
pub const DEFAULT_THUMB_MAX_SIDE: u32 = 512;

/// Erro durante a geração de thumbnail.
#[derive(Debug, PartialEq, Eq)]
pub enum ThumbError {
    DecodeFailed,
    EncodeFailed,
}

impl std::fmt::Display for ThumbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DecodeFailed => write!(f, "failed to decode image"),
            Self::EncodeFailed => write!(f, "failed to encode thumbnail as jpeg"),
        }
    }
}

impl std::error::Error for ThumbError {}

/// Gera miniatura JPEG otimizada (max_side preservando aspect ratio).
///
/// Proteções de recursos e robustez:
/// 1. Limites estritos de dimensões (≤16384) e alocação (≤256 MiB) no `image::ImageReader`.
/// 2. Conversão explícita para RGB8 antes do encode JPEG (evita quebra com PNG RGBA / 16-bit).
/// 3. Redimensionamento rápido preservando aspect ratio via `thumbnail(max_side, max_side)`.
pub fn generate_thumb_from_bytes(bytes: &[u8], max_side: u32) -> Result<Vec<u8>, ThumbError> {
    if bytes.is_empty() {
        return Err(ThumbError::DecodeFailed);
    }

    let cursor = std::io::Cursor::new(bytes);
    let mut reader = image::ImageReader::new(cursor)
        .with_guessed_format()
        .map_err(|_| ThumbError::DecodeFailed)?;

    let mut limits = image::Limits::default();
    limits.max_image_width = Some(THUMB_MAX_DIMENSION);
    limits.max_image_height = Some(THUMB_MAX_DIMENSION);
    limits.max_alloc = Some(THUMB_MAX_ALLOC_BYTES);
    reader.limits(limits);

    let dyn_img = reader.decode().map_err(|_| ThumbError::DecodeFailed)?;
    let thumb = dyn_img.thumbnail(max_side, max_side);
    let rgb = thumb.into_rgb8();

    let mut out = std::io::Cursor::new(Vec::new());
    rgb.write_to(&mut out, image::ImageFormat::Jpeg)
        .map_err(|_| ThumbError::EncodeFailed)?;

    Ok(out.into_inner())
}

/// Cria o semáforo padrão de geração de miniaturas com base nas CPUs disponíveis.
pub fn default_thumb_semaphore() -> Arc<Semaphore> {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    Arc::new(Semaphore::new(cpus.max(2)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_thumb_png_rgba() {
        // PNG RGBA de 1000x500
        let mut png_bytes = Vec::new();
        let img = image::RgbaImage::from_pixel(1000, 500, image::Rgba([255, 0, 0, 128]));
        img.write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .unwrap();

        let thumb_bytes = generate_thumb_from_bytes(&png_bytes, DEFAULT_THUMB_MAX_SIDE).unwrap();
        assert!(!thumb_bytes.is_empty());

        let decoded = image::load_from_memory(&thumb_bytes).unwrap();
        assert_eq!(decoded.width(), 512);
        assert_eq!(decoded.height(), 256);
    }

    #[test]
    fn test_generate_thumb_invalid_bytes() {
        let res = generate_thumb_from_bytes(b"not an image", DEFAULT_THUMB_MAX_SIDE);
        assert_eq!(res, Err(ThumbError::DecodeFailed));

        let res_empty = generate_thumb_from_bytes(b"", DEFAULT_THUMB_MAX_SIDE);
        assert_eq!(res_empty, Err(ThumbError::DecodeFailed));
    }
}
