//! Chaves legíveis de objetos (ADR-0003 D5).
//!
//! A chave é legível (`datasets/{dataset_id}/images/{image_id}/{stem}.{ext}`)
//! com IDs imutáveis; a extensão vem SEMPRE do sniff do conteúdo, nunca do
//! nome enviado no form (nome é decorativo e não confiável).

use uuid::Uuid;

/// Sanitiza nome de arquivo não confiável (stem, sem extensão): remove a
/// extensão aparente, preserva só `[A-Za-z0-9_]` (todo o resto — inclui
/// `/ \ NUL, Unicode, espaço, `.` e `-` — vira `-`), colapsa runs de `-`
/// em um `-`, tira `-` das pontas, trunca em 64 chars; vazio ⇒ `"img"`.
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
    // Colapsa runs de `-` em um só e tira das pontas; trunca em 64
    // (`tmp` nunca contém `.`: tudo fora de `[A-Za-z0-9_]` já virou `-`).
    let mut out = String::with_capacity(tmp.len());
    let mut prev_dash = false;
    for c in tmp.chars() {
        if c == '-' {
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

/// Nome canônico server-side: stem sanitizado + extensão do sniff
/// (nunca a do form). `a.png` + `a.jpg` de conteúdos distintos ⇒
/// canônicos distintos (sem duplicate falso).
pub fn canonical_filename(raw: &str, media: crate::storage::sniff::MediaType) -> String {
    format!("{}.{}", sanitize_filename(raw), media.extension())
}

/// Chave legível do objeto (D5): IDs imutáveis; `canonical` já é o
/// `canonical_filename` (formato final `datasets/{ds}/images/{img}/{stem}.{ext}`).
pub fn image_object_key(
    dataset_id: Uuid,
    image_id: Uuid,
    canonical: &str,
) -> String {
    format!("datasets/{dataset_id}/images/{image_id}/{canonical}")
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
        let canonical = canonical_filename("foto.png", crate::storage::sniff::MediaType::Jpeg);
        assert_eq!(canonical, "foto.jpg");
        assert_eq!(image_object_key(ds, img, &canonical), format!("datasets/{ds}/images/{img}/foto.jpg"));
    }

    #[test]
    fn chave_formato_exato() {
        let (ds, img) = (Uuid::nil(), Uuid::nil());
        let canonical = canonical_filename("a.png", crate::storage::sniff::MediaType::Png);
        assert_eq!(image_object_key(ds, img, &canonical), format!("datasets/{ds}/images/{img}/a.png"));
    }

    #[test]
    fn extensoes_distintas_nao_colidem() {
        let a_png = canonical_filename("a.png", crate::storage::sniff::MediaType::Png);
        let a_jpg = canonical_filename("a.jpg", crate::storage::sniff::MediaType::Jpeg);
        assert_eq!(a_png, "a.png");
        assert_eq!(a_jpg, "a.jpg");
        assert_ne!(a_png, a_jpg);
    }

    #[test]
    fn traversal_nao_vaza_para_sufixo() {
        let c = canonical_filename("../../etc/passwd.jpg", crate::storage::sniff::MediaType::Png);
        assert_eq!(c, "etc-passwd.png");
        assert!(!c.contains('/'));
        assert!(!c.contains('\\'));
    }
}
