//! Normalização, higienização de metadados e conversão canônica para WebP (ADR-0017).
//!
//! Todas as imagens aceitas (JPEG, PNG, WebP, BMP, TIFF, GIF) são decodificadas
//! para pixels brutos (expurgando metadados sensíveis como EXIF, GPS e XMP) e
//! recodificadas em WebP. O nome canônico é derivado do hash MD5 dos bytes WebP.

use md5::Digest as Md5Digest;
use sha2::Digest as Sha256Digest;

#[derive(Debug)]
pub struct NormalizedImage {
    pub webp_bytes: Vec<u8>,
    pub width: i32,
    pub height: i32,
    pub md5: String,
    pub sha256: String,
    pub filename: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum NormalizeError {
    UnsupportedMedia,
    DecodeFailed,
    EncodeFailed,
}

impl std::fmt::Display for NormalizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedMedia => write!(f, "unsupported media"),
            Self::DecodeFailed => write!(f, "failed to decode image"),
            Self::EncodeFailed => write!(f, "failed to encode image as webp"),
        }
    }
}

impl std::error::Error for NormalizeError {}

/// Normaliza uma imagem a partir de bytes em memória:
/// decodifica, higieniza metadados, recodifica para WebP e calcula hashes.
pub fn normalize_image(bytes: &[u8]) -> Result<NormalizedImage, NormalizeError> {
    if bytes.is_empty() {
        return Err(NormalizeError::DecodeFailed);
    }

    let cursor = std::io::Cursor::new(bytes);
    let reader = image::ImageReader::new(cursor)
        .with_guessed_format()
        .map_err(|_| NormalizeError::UnsupportedMedia)?;

    let dyn_img = reader.decode().map_err(|_| NormalizeError::DecodeFailed)?;
    let (width, height) = (dyn_img.width() as i32, dyn_img.height() as i32);

    let mut out = std::io::Cursor::new(Vec::new());
    dyn_img
        .write_to(&mut out, image::ImageFormat::WebP)
        .map_err(|_| NormalizeError::EncodeFailed)?;

    let webp_bytes = out.into_inner();

    let mut md5_hasher = md5::Md5::new();
    Md5Digest::update(&mut md5_hasher, &webp_bytes);
    let md5 = hex::encode(Md5Digest::finalize(md5_hasher));

    let mut sha_hasher = sha2::Sha256::new();
    Sha256Digest::update(&mut sha_hasher, &webp_bytes);
    let sha256 = hex::encode(Sha256Digest::finalize(sha_hasher));

    let filename = format!("{}.webp", md5);

    Ok(NormalizedImage {
        webp_bytes,
        width,
        height,
        md5,
        sha256,
        filename,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normaliza_png_para_webp() {
        let mut png_bytes = Vec::new();
        let img = image::RgbImage::new(32, 24);
        img.write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .unwrap();

        let norm = normalize_image(&png_bytes).expect("deve normalizar png");
        assert_eq!(norm.width, 32);
        assert_eq!(norm.height, 24);
        assert_eq!(norm.md5.len(), 32);
        assert_eq!(norm.sha256.len(), 64);
        assert_eq!(norm.filename, format!("{}.webp", norm.md5));
        assert!(norm.webp_bytes.starts_with(b"RIFF"));
    }

    #[test]
    fn normaliza_bmp_para_webp() {
        let mut bmp_bytes = Vec::new();
        let img = image::RgbImage::new(16, 16);
        img.write_to(
            &mut std::io::Cursor::new(&mut bmp_bytes),
            image::ImageFormat::Bmp,
        )
        .unwrap();

        let norm = normalize_image(&bmp_bytes).expect("deve normalizar bmp");
        assert_eq!(norm.width, 16);
        assert_eq!(norm.height, 16);
        assert!(norm.filename.ends_with(".webp"));
    }

    #[test]
    fn normaliza_rgba_preserva_dimensoes() {
        let mut png_bytes = Vec::new();
        let img = image::RgbaImage::new(20, 10);
        img.write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .unwrap();

        let norm = normalize_image(&png_bytes).expect("deve normalizar rgba");
        assert_eq!(norm.width, 20);
        assert_eq!(norm.height, 10);
        assert!(norm.webp_bytes.starts_with(b"RIFF"));
    }

    #[test]
    fn normaliza_jpeg_para_webp() {
        let mut jpeg_bytes = Vec::new();
        let img = image::RgbImage::new(40, 30);
        img.write_to(
            &mut std::io::Cursor::new(&mut jpeg_bytes),
            image::ImageFormat::Jpeg,
        )
        .unwrap();

        let norm = normalize_image(&jpeg_bytes).expect("deve normalizar jpeg");
        assert_eq!(norm.width, 40);
        assert_eq!(norm.height, 30);
        assert!(norm.filename.ends_with(".webp"));
    }

    #[test]
    fn normaliza_gif_para_webp() {
        let mut gif_bytes = Vec::new();
        let img = image::RgbImage::new(12, 12);
        img.write_to(
            &mut std::io::Cursor::new(&mut gif_bytes),
            image::ImageFormat::Gif,
        )
        .unwrap();

        let norm = normalize_image(&gif_bytes).expect("deve normalizar gif");
        assert_eq!(norm.width, 12);
        assert_eq!(norm.height, 12);
        assert!(norm.filename.ends_with(".webp"));
    }

    #[test]
    fn dados_invalidos_retornam_erro() {
        let res = normalize_image(b"isso nao eh uma imagem");
        assert!(res.is_err());
    }
}
