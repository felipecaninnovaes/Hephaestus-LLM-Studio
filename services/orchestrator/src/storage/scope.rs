//! Chaves S3 escopadas (D2 — invariante de prefixo, barreira principal).

use crate::domain::errors::ScopedKeyError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum S3Scope {
    Packages,
    Artifacts,
    Models,
    GenerationInputs,
}

impl S3Scope {
    pub fn prefix(&self) -> &str {
        match self {
            S3Scope::Packages => "packages/",
            S3Scope::Artifacts => "artifacts/",
            S3Scope::Models => "models/",
            S3Scope::GenerationInputs => "generation_inputs/",
        }
    }
}

/// Valida e retorna uma key S3 dentro do escopo permitido.
///
/// Regras:
/// - Key não pode ser vazia
/// - Key não pode começar com `/`
/// - Key não pode conter `..`
/// - Key deve começar com o prefixo do scope (`packages/` ou `artifacts/`)
///
/// Esta é a BARREIRA PRINCIPAL contra path-traversal (D2, spike F4.0).
pub fn scoped_key(scope: S3Scope, key: &str) -> Result<String, ScopedKeyError> {
    if key.is_empty() {
        return Err(ScopedKeyError::EmptyKey);
    }
    if key.starts_with('/') {
        return Err(ScopedKeyError::AbsolutePath);
    }
    if key.contains("..") {
        return Err(ScopedKeyError::PathTraversal);
    }
    let prefix = scope.prefix();
    if !key.starts_with(prefix) {
        return Err(ScopedKeyError::OutsideScope);
    }
    Ok(key.to_string())
}

/// Valida key de imagem inicial img2img (S4 — feat/img2img).
///
/// Aceita `generation_inputs/<...>` (upload avulso) OU `artifacts/<...>`
/// (galeria) — as duas origens possíveis do init. Escopo próprio do staging
/// do init: NÃO afrouxa `scoped_key` para outros usos.
pub fn scoped_init_image_key(key: &str) -> Result<String, ScopedKeyError> {
    if key.is_empty() {
        return Err(ScopedKeyError::EmptyKey);
    }
    if key.starts_with('/') {
        return Err(ScopedKeyError::AbsolutePath);
    }
    if key.contains("..") {
        return Err(ScopedKeyError::PathTraversal);
    }
    if key.starts_with(S3Scope::GenerationInputs.prefix())
        || key.starts_with(S3Scope::Artifacts.prefix())
    {
        return Ok(key.to_string());
    }
    Err(ScopedKeyError::OutsideScope)
}

/// Extensão sanitizada da imagem inicial a partir do s3_key (S4 — feat/img2img).
///
/// Usa o sufixo após o último `.` do filename (após a última `/`): só
/// `[a-zA-Z0-9]` com 2..5 chars (lowercased) — qualquer outra coisa cai em
/// `"png"`. Nunca devolve `..` ou barras.
pub fn init_image_ext(s3_key: &str) -> String {
    let filename = s3_key.rsplit('/').next().unwrap_or("");
    let ext = filename.rsplit('.').next().unwrap_or("");
    let ok = (2..=5).contains(&ext.len())
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
        && filename.contains('.');
    if ok {
        ext.to_ascii_lowercase()
    } else {
        "png".to_string()
    }
}
