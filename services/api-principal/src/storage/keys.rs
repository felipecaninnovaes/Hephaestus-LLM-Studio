//! Chaves legíveis de objetos (ADR-0003 D5).
//!
//! A chave é legível (`datasets/{dataset_id}/images/{image_id}/{stem}.{ext}`)
//! com IDs imutáveis; a extensão vem SEMPRE do sniff do conteúdo, nunca do
//! nome enviado no form (nome é decorativo e não confiável).

use uuid::Uuid;

/// Sanitiza nome de arquivo não confiável: remove a extensão aparente,
/// troca todo char fora de `[A-Za-z0-9._-]` (inclui `/ \ NUL, Unicode e
/// espaço) por `-`, colapsa runs de `-`/`.` em um `-`, tira `.`/`-` das
/// pontas, trunca em 64 chars; vazio ⇒ `"img"`.
pub fn sanitize_filename(raw: &str) -> String {
    // Remove a extensão aparente (o `.jpg` do form é decorativo).
    let base = match raw.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => raw,
    };
    let mut tmp = String::with_capacity(base.len());
    for c in base.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            tmp.push(c);
        } else {
            tmp.push('-');
        }
    }
    // Colapsa runs de `-`/`.` em um `-` e tira das pontas; trunca em 64.
    let mut out = String::with_capacity(tmp.len());
    let mut prev_dash = false;
    for c in tmp.chars() {
        if c == '-' || c == '.' {
            if !prev_dash { out.push('-'); prev_dash = true; }
        } else {
            out.push(c);
            prev_dash = false;
        }
    }
    let trimmed = out.trim_matches(['.', '-']).to_string();
    let mut stem: String = trimmed.chars().take(64).collect();
    stem = stem.trim_matches(['.', '-']).to_string();
    if stem.is_empty() {
        return "img".to_string();
    }
    stem
}

/// Chave legível do objeto (D5): IDs imutáveis; `media_ext` vem do sniff.
pub fn image_object_key(
    dataset_id: Uuid,
    image_id: Uuid,
    filename: &str,
    media_ext: &str,
) -> String {
    let stem = sanitize_filename(filename);
    format!("datasets/{dataset_id}/images/{image_id}/{stem}.{media_ext}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traversal_sem_ponto_ou_barra() {
        let s = sanitize_filename("../../etc/passwd.jpg");
        assert!(!s.contains('.'));
        assert!(!s.contains('/'));
        assert!(!s.contains('\\'));
        assert_eq!(s, "etc-passwd");
    }

    #[test]
    fn vazio_vira_img() {
        assert_eq!(sanitize_filename("  "), "img");
        assert_eq!(sanitize_filename(""), "img");
    }

    #[test]
    fn colapso_documentado() {
        // `my.pic.png`: extensão `.png` removida ⇒ `my.pic` ⇒ colapso ⇒ `my-pic`.
        assert_eq!(sanitize_filename("my.pic.png"), "my-pic");
    }

    #[test]
    fn sniff_vence_nome() {
        let (ds, img) = (Uuid::nil(), Uuid::nil());
        assert_eq!(image_object_key(ds, img, "foto.png", "jpg"), format!("datasets/{ds}/images/{img}/foto.jpg"));
    }

    #[test]
    fn chave_formato_exato() {
        let (ds, img) = (Uuid::nil(), Uuid::nil());
        assert_eq!(image_object_key(ds, img, "a.png", "png"), format!("datasets/{ds}/images/{img}/a.png"));
    }
}
