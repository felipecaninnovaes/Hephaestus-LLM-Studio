//! Import de dataset — backup estruturado (ADR-0006 D3–D7, fatia 3e.2).
//!
//! `POST /api/datasets/import` multipart com `file` (zip obrigatório) +
//! `title` (opcional, override) + `replace` (opcional, consentimento de
//! substituição). Ordem obrigatória: validação COMPLETA do pacote ANTES de
//! qualquer write/teardown (D3/D4) → teardown condicional (D5/D6, mesma
//! transação DELETE antigo + INSERT novo) → ingest objeto→linha→compensação
//! (espelho da D7 da 3b). Falha fatal no bucket ⇒ 503 + delete da linha nova
//! + sweep. Sucesso ⇒ 201 + indexação fire-and-forget (espelho do upload).

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::PathBuf;

use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use md5::Digest as Md5Digest;
use sha2::Digest as Sha256Digest;
use uuid::Uuid;

use super::export::ExportManifest;
use super::models::{
    category_task_of, color_for, derived_source, parse_import_replace, resolve_import_title,
    slugify, validate_import_manifest, DatasetClassResponse, DatasetResponse, DatasetRow,
};
use crate::{
    error::{
        err, MSG_IMPORT_INVALID, MSG_INVALID_REQUEST, MSG_SLUG_CONFLICT, MSG_STORAGE_UNAVAILABLE,
    },
    state::AppState,
    storage::{keys, sniff},
};

const MSG_INTERNAL: &str = "internal server error";

fn internal() -> Response {
    err(StatusCode::INTERNAL_SERVER_ERROR, "internal", MSG_INTERNAL)
}

fn import_invalid() -> Response {
    err(
        StatusCode::BAD_REQUEST,
        "import_invalid",
        MSG_IMPORT_INVALID,
    )
}

fn invalid_request() -> Response {
    err(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        MSG_INVALID_REQUEST,
    )
}

/// Compensação fatal do import (all-or-nothing D5/D6): DELETE da linha NOVA
/// (CASCADE apaga images/boxes/captions) + sweep best-effort do prefixo.
async fn cleanup_failed_import(state: &AppState, new_id: Uuid) {
    let _ = sqlx::query("DELETE FROM datasets WHERE id = $1")
        .bind(new_id)
        .execute(&state.pool)
        .await;
    let prefix = format!("datasets/{new_id}/");
    if let Err(e) = state.storage.delete_prefix(&prefix).await {
        eprintln!("aviso: sweep do prefixo {prefix} falhou ({e}) — objetos reapáveis");
    }
}

/// Teto do spool do zip (defesa extra — o teto de CORPO mora na rota,
/// `IMPORT_BODY_LIMIT_BYTES`; excedeu aqui ⇒ 413 no envelope).
pub const MAX_IMPORT_SPOOL_BYTES: i64 = 200 * 1024 * 1024;
/// Tetos do pré-scan (D4): entradas e soma declarada do central directory.
const MAX_ENTRIES: usize = 100_000;
const MAX_DECLARED_TOTAL: u64 = 8 * 1024 * 1024 * 1024;
/// Tetos do reader contado (a defesa real — declarações mentem).
const MAX_IMAGE_BYTES: u64 = 200 * 1024 * 1024;
const MAX_TEXT_BYTES: u64 = 10 * 1024 * 1024;
const MAX_GLOBAL_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// Erro opaco do axum: `LengthLimitError` no debug (estouro do
/// `DefaultBodyLimit` do CORPO TOTAL) ⇒ 413; o resto é stream morto.
fn is_too_large(err: &axum::extract::multipart::MultipartError) -> bool {
    format!("{err:?}").contains("LengthLimit")
}

// ---------------------------------------------------------------------------
// Layout do pacote (D4) — puro, unit-testável sem banco nem zip.
// ---------------------------------------------------------------------------

/// Classificação de uma entrada do zip: artefatos conhecidos, lixo de mac
/// (ignorado) ou pacote malformado. Zip-slip ( `..`, barra inicial, `\` ou
/// caminho não-relativo) ⇒ `Err(())` antes de qualquer whitelist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Manifest,
    TextArtifact,
    Label,
    Image,
    Ignored,
}

pub fn classify_entry(name: &str) -> Result<EntryKind, ()> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains("..")
        || std::path::Path::new(name).is_absolute()
    {
        return Err(());
    }
    // Diretórios do próprio zip (manifestação do `ZipWriter`): ignorados.
    if name.ends_with('/') {
        return Ok(EntryKind::Ignored);
    }
    // Lixo comum de mac (D4): ignorado, nunca erro.
    if name == "__MACOSX" || name.starts_with("__MACOSX/") {
        return Ok(EntryKind::Ignored);
    }
    if name == ".DS_Store" || name.rsplit('/').next() == Some(".DS_Store") {
        return Ok(EntryKind::Ignored);
    }
    if name == "manifest.json" {
        return Ok(EntryKind::Manifest);
    }
    if name == "dataset.yaml" || name == "captions.jsonl" {
        return Ok(EntryKind::TextArtifact);
    }
    if let Some(rest) = name.strip_prefix("labels/") {
        if !rest.is_empty() && !rest.contains('/') {
            return Ok(EntryKind::Label);
        }
        return Err(());
    }
    if let Some(rest) = name.strip_prefix("images/") {
        if !rest.is_empty() && !rest.contains('/') {
            return Ok(EntryKind::Image);
        }
        return Err(());
    }
    Err(())
}

/// Resultado do scan de layout: presença do manifest + entradas `images/*`.
#[derive(Debug, PartialEq, Eq)]
pub struct LayoutInfo {
    pub manifest_present: bool,
    pub image_entries: Vec<String>,
}

/// Scan puro sobre os nomes do central directory: slip/whitelist (D4),
/// teto de entradas e soma declarada. `Err(())` ⇒ 400 `import_invalid`.
pub fn scan_layout(names_sizes: &[(String, u64)]) -> Result<LayoutInfo, ()> {
    if names_sizes.len() > MAX_ENTRIES {
        return Err(());
    }
    let mut declared: u64 = 0;
    let mut manifest_present = false;
    let mut image_entries = Vec::new();
    for (name, size) in names_sizes {
        declared = declared.saturating_add(*size);
        if declared > MAX_DECLARED_TOTAL {
            return Err(());
        }
        match classify_entry(name)? {
            EntryKind::Manifest => manifest_present = true,
            EntryKind::Image => image_entries.push(name.clone()),
            _ => {}
        }
    }
    Ok(LayoutInfo {
        manifest_present,
        image_entries,
    })
}

// ---------------------------------------------------------------------------
// Imagem validada (spoolada em tempdir para o `put_object` posterior).
// ---------------------------------------------------------------------------

struct ValidatedImage {
    canonical: String,
    media_db: String,
    md5hex: String,
    sha256hex: String,
    width: i32,
    height: i32,
    bytes_len: i64,
    split: String,
    spool: PathBuf,
    boxes: Vec<super::export::ManifestBox>,
    caption: Option<super::export::ManifestCaption>,
}

/// Extração + validação de CADA imagem (D4): 1:1 manifest ↔ `images/*`,
/// reader contado (200 MiB/imagem, global 8 GiB), sniff exigido igual ao
/// `manifest.media_type`, extensão canônica do sniff, dedupe pós-sniff,
/// sha256 conferido, dimensões decodeadas iguais ao manifest.
fn extract_images(
    zip_path: &std::path::Path,
    manifest: &ExportManifest,
    workdir: &std::path::Path,
) -> Result<Vec<ValidatedImage>, Response> {
    use super::export::{ManifestBox, ManifestCaption};
    let file = std::fs::File::open(zip_path).map_err(|_| import_invalid())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| import_invalid())?;
    // 1:1 — entradas sem correspondência no manifest, ou manifest apontando
    // entrada ausente ⇒ 400 (D4).
    let mut manifest_names: HashSet<&str> = HashSet::new();
    for img in &manifest.images {
        manifest_names.insert(img.filename.as_str());
    }
    let mut entry_names: HashSet<String> = HashSet::new();
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|_| import_invalid())?;
        let name = entry.name().to_string();
        if matches!(classify_entry(&name), Ok(EntryKind::Image)) {
            entry_names.insert(name);
        }
    }
    let manifest_set: HashSet<String> = manifest_names
        .iter()
        .map(|n| format!("images/{n}"))
        .collect();
    if entry_names != manifest_set {
        return Err(import_invalid());
    }
    // Por imagem, na ordem do manifest.
    let mut seen_canonical: HashSet<String> = HashSet::new();
    let mut global: u64 = 0;
    let mut out = Vec::with_capacity(manifest.images.len());
    for img in &manifest.images {
        let arcname = format!("images/{}", img.filename);
        let entry = archive.by_name(&arcname).map_err(|_| import_invalid())?;
        let mut limited = entry.take(MAX_IMAGE_BYTES + 1);
        let mut buf = Vec::new();
        limited
            .read_to_end(&mut buf)
            .map_err(|_| import_invalid())?;
        if buf.len() as u64 > MAX_IMAGE_BYTES {
            return Err(import_invalid());
        }
        global = global.saturating_add(buf.len() as u64);
        if global > MAX_GLOBAL_BYTES {
            return Err(import_invalid());
        }
        let head_len = buf.len().min(12);
        let media = sniff::sniff(&buf[..head_len]).ok_or_else(import_invalid)?;
        if media.as_db() != img.media_type {
            return Err(import_invalid());
        }
        let canonical = keys::canonical_filename(&img.filename, media);
        if !seen_canonical.insert(canonical.clone()) {
            return Err(import_invalid());
        }
        let mut sha = sha2::Sha256::new();
        Sha256Digest::update(&mut sha, &buf);
        let sha_hex = hex::encode(Sha256Digest::finalize(sha));
        if sha_hex != img.sha256.to_lowercase() {
            return Err(import_invalid());
        }
        let mut md5 = md5::Md5::new();
        Md5Digest::update(&mut md5, &buf);
        let md5_hex = hex::encode(Md5Digest::finalize(md5));
        // Spool para o `put_object` posterior + decode das dimensões.
        let spool = workdir.join(&canonical);
        std::fs::write(&spool, &buf).map_err(|_| internal())?;
        let (width, height) = match std::fs::File::open(&spool) {
            Ok(f) => {
                match image::ImageReader::new(std::io::BufReader::new(f)).with_guessed_format() {
                    Ok(r) => match r.into_dimensions() {
                        Ok((w, h)) => (w as i32, h as i32),
                        Err(_) => return Err(import_invalid()),
                    },
                    Err(_) => return Err(import_invalid()),
                }
            }
            Err(_) => return Err(internal()),
        };
        if width != img.width || height != img.height {
            return Err(import_invalid());
        }
        let boxes: Vec<ManifestBox> = img.boxes.clone();
        let caption: Option<ManifestCaption> = img.caption.clone();
        out.push(ValidatedImage {
            canonical,
            media_db: media.as_db().to_string(),
            md5hex: md5_hex,
            sha256hex: sha_hex,
            width,
            height,
            bytes_len: buf.len() as i64,
            split: img.split.clone(),
            spool,
            boxes,
            caption,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Handler.
// ---------------------------------------------------------------------------

/// POST /api/datasets/import — 201 | 400 `invalid_request` (form) |
/// 400 `import_invalid` (pacote) | 409 `slug_conflict` | 503
/// `storage_unavailable`. 401 vem do gate, nunca daqui.
pub async fn import_dataset(State(state): State<AppState>, mut multipart: Multipart) -> Response {
    // 1. Multipart: `file` obrigatório + `title`/`replace` opcionais.
    let mut zip_spool: Option<tempfile::NamedTempFile> = None;
    let mut zip_too_large = false;
    let mut title_raw: Option<String> = None;
    let mut replace_raw: Option<String> = None;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            // `Ok(None)` = fim; `Err` não-limite = stream morto (padrão do
            // upload 3b: nunca há `continue` pós-`Err` — o multer nunca fuseja).
            Ok(None) => break,
            Err(e) => {
                if is_too_large(&e) {
                    return err(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "invalid_request",
                        MSG_INVALID_REQUEST,
                    );
                }
                break;
            }
        };
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                if zip_spool.is_some() {
                    continue;
                }
                let tmp = match tempfile::NamedTempFile::new() {
                    Ok(t) => t,
                    Err(_) => return internal(),
                };
                let tmp_path = tmp.path().to_path_buf();
                let mut out = match tokio::fs::File::create(&tmp_path).await {
                    Ok(f) => f,
                    Err(_) => return internal(),
                };
                {
                    use tokio::io::AsyncWriteExt;
                    let mut field = field;
                    let mut total: i64 = 0;
                    let mut over = false;
                    loop {
                        match field.chunk().await {
                            Ok(Some(bytes)) => {
                                total += bytes.len() as i64;
                                if total > MAX_IMPORT_SPOOL_BYTES {
                                    over = true;
                                    continue;
                                }
                                if out.write_all(&bytes).await.is_err() {
                                    break;
                                }
                            }
                            Ok(None) => break,
                            Err(e) => {
                                if is_too_large(&e) {
                                    return err(
                                        StatusCode::PAYLOAD_TOO_LARGE,
                                        "invalid_request",
                                        MSG_INVALID_REQUEST,
                                    );
                                }
                                break;
                            }
                        }
                    }
                    let _ = out.flush().await;
                    if over {
                        zip_too_large = true;
                    }
                }
                zip_spool = Some(tmp);
            }
            "title" => {
                if title_raw.is_none() {
                    match field.text().await {
                        Ok(t) => title_raw = Some(t),
                        Err(e) => {
                            if is_too_large(&e) {
                                return err(
                                    StatusCode::PAYLOAD_TOO_LARGE,
                                    "invalid_request",
                                    MSG_INVALID_REQUEST,
                                );
                            }
                            break;
                        }
                    }
                }
            }
            "replace" => {
                if replace_raw.is_none() {
                    match field.text().await {
                        Ok(t) => replace_raw = Some(t),
                        Err(e) => {
                            if is_too_large(&e) {
                                return err(
                                    StatusCode::PAYLOAD_TOO_LARGE,
                                    "invalid_request",
                                    MSG_INVALID_REQUEST,
                                );
                            }
                            break;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if zip_too_large {
        return err(
            StatusCode::PAYLOAD_TOO_LARGE,
            "invalid_request",
            MSG_INVALID_REQUEST,
        );
    }
    let zip_tmp = match zip_spool {
        Some(t) => t,
        None => return invalid_request(),
    };
    let replace = match parse_import_replace(replace_raw.as_deref()) {
        Ok(v) => v,
        Err(()) => return invalid_request(),
    };
    if let Some(t) = &title_raw {
        if t.chars().count() > 96 {
            return invalid_request();
        }
    }

    // 2. Pré-scan do central directory (D4) — antes de qualquer write.
    let zip_path = zip_tmp.path().to_path_buf();
    let names_sizes: Vec<(String, u64)> = {
        let file = match std::fs::File::open(&zip_path) {
            Ok(f) => f,
            Err(_) => return internal(),
        };
        let mut archive = match zip::ZipArchive::new(file) {
            Ok(a) => a,
            Err(_) => return import_invalid(),
        };
        let mut out = Vec::with_capacity(archive.len());
        for i in 0..archive.len() {
            match archive.by_index(i) {
                Ok(e) => out.push((e.name().to_string(), e.size())),
                Err(_) => return import_invalid(),
            }
        }
        out
    };
    let _layout = match scan_layout(&names_sizes) {
        Ok(l) => l,
        Err(()) => return import_invalid(),
    };

    // 3. Manifest: extração contada (10 MiB) + parse + domínios.
    let manifest: ExportManifest = {
        let file = match std::fs::File::open(&zip_path) {
            Ok(f) => f,
            Err(_) => return internal(),
        };
        let mut archive = match zip::ZipArchive::new(file) {
            Ok(a) => a,
            Err(_) => return import_invalid(),
        };
        let entry = match archive.by_name("manifest.json") {
            Ok(e) => e,
            Err(_) => return import_invalid(),
        };
        let mut limited = entry.take(MAX_TEXT_BYTES + 1);
        let mut buf = Vec::new();
        if limited.read_to_end(&mut buf).is_err() {
            return import_invalid();
        }
        if buf.len() as u64 > MAX_TEXT_BYTES {
            return import_invalid();
        }
        match serde_json::from_slice(&buf) {
            Ok(m) => m,
            Err(_) => return import_invalid(),
        }
    };
    if validate_import_manifest(&manifest).is_err() {
        return import_invalid();
    }

    // 4. Imagens: extração + validação completa (D4) — ainda sem write.
    let workdir = match tempfile::TempDir::new() {
        Ok(d) => d,
        Err(_) => return internal(),
    };
    let validated = match extract_images(&zip_path, &manifest, workdir.path()) {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    // 5. Slug + protocolo de substituição (D5): o 409 é detecção — o pacote
    // JÁ foi validado acima; o servidor nunca substitui sozinho.
    let title = match resolve_import_title(title_raw.as_deref(), &manifest.dataset.title) {
        Ok(t) => t,
        Err(()) => return invalid_request(),
    };
    let slug = slugify(&title);
    if slug.is_empty() {
        return import_invalid();
    }
    let slug_exists: bool =
        match sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM datasets WHERE slug = $1)")
            .bind(&slug)
            .fetch_one(&state.pool)
            .await
        {
            Ok(v) => v,
            Err(_) => return internal(),
        };
    if slug_exists && !replace {
        return err(StatusCode::CONFLICT, "slug_conflict", MSG_SLUG_CONFLICT);
    }

    // 6. Teardown condicional + criação (D6): no caminho `replace=true` a
    // MESMA transação funde DELETE do antigo + INSERT do novo + classes
    // (rollback devolve o antigo intacto; o UNIQUE libera intra-tx).
    let (category, task) = match category_task_of(&manifest.dataset.r#type) {
        Ok(v) => v,
        Err(()) => return import_invalid(),
    };
    let class_names: Vec<String> = {
        let mut indexed: Vec<(i32, String)> = manifest
            .classes
            .iter()
            .map(|c| (c.idx, c.name.clone()))
            .collect();
        indexed.sort_by_key(|(idx, _)| *idx);
        indexed
            .into_iter()
            .map(|(_, n)| n.trim().to_string())
            .collect()
    };
    let class_idxs: Vec<i32> = (0..class_names.len()).map(|i| i as i32).collect();
    let class_colors: Vec<String> = (0..class_names.len())
        .map(|i| color_for(i).to_string())
        .collect();
    let new_id = Uuid::new_v4();
    let mut tx = match state.pool.begin().await {
        Ok(t) => t,
        Err(_) => return internal(),
    };
    let old_id: Option<Uuid> = if replace {
        match sqlx::query_scalar::<_, Uuid>("DELETE FROM datasets WHERE slug = $1 RETURNING id")
            .bind(&slug)
            .fetch_optional(&mut *tx)
            .await
        {
            Ok(v) => v,
            Err(_) => {
                let _ = tx.rollback().await;
                return internal();
            }
        }
    } else {
        None
    };
    let inserted: Option<Uuid> = match sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO datasets (id, slug, title, category, type, task, format, status) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,'needs_labeling') \
         ON CONFLICT (slug) DO NOTHING RETURNING id",
    )
    .bind(new_id)
    .bind(&slug)
    .bind(&title)
    .bind(category)
    .bind(&manifest.dataset.r#type)
    .bind(task)
    .bind(&manifest.dataset.format)
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(v) => v,
        Err(_) => {
            let _ = tx.rollback().await;
            return internal();
        }
    };
    let new_id = match inserted {
        Some(id) => id,
        None => {
            let _ = tx.rollback().await;
            if replace {
                return internal();
            }
            return err(StatusCode::CONFLICT, "slug_conflict", MSG_SLUG_CONFLICT);
        }
    };
    if !class_names.is_empty() {
        if sqlx::query(
            "INSERT INTO classes (dataset_id, name, idx, color) \
             SELECT $1, t.name, t.idx, t.color \
             FROM unnest($2::text[], $3::int[], $4::text[]) AS t(name, idx, color)",
        )
        .bind(new_id)
        .bind(&class_names)
        .bind(&class_idxs)
        .bind(&class_colors)
        .execute(&mut *tx)
        .await
        .is_err()
        {
            let _ = tx.rollback().await;
            return internal();
        }
    }
    if tx.commit().await.is_err() {
        return internal();
    }
    // Pós-commit da substituição: sweep do prefixo ANTIGO best-effort.
    if let Some(old) = old_id {
        let prefix = format!("datasets/{old}/");
        if let Err(e) = state.storage.delete_prefix(&prefix).await {
            eprintln!("aviso: sweep do prefixo antigo {prefix} falhou ({e}) — objetos reapáveis");
        }
    }
    // Mapa class_idx → id da classe nova (boxes do passo 7).
    let new_classes: Vec<(Uuid, i32)> =
        match sqlx::query_as("SELECT id, idx FROM classes WHERE dataset_id = $1 ORDER BY idx")
            .bind(new_id)
            .fetch_all(&state.pool)
            .await
        {
            Ok(r) => r,
            Err(_) => return internal(),
        };
    let mut class_id_by_idx: HashMap<i32, Uuid> = HashMap::new();
    for (id, idx) in new_classes {
        class_id_by_idx.insert(idx, id);
    }

    // 7. Ingest por imagem, ordem do manifest: objeto → linha → boxes →
    // caption, com compensação por imagem (espelho D7 da 3b). Estado ruim
    // aceitável = objeto sem linha; nunca linha sem objeto.
    let mut stored_ids: Vec<Uuid> = Vec::with_capacity(validated.len());
    for v in &validated {
        let image_id = Uuid::new_v4();
        let key = keys::image_object_key(new_id, image_id, &v.canonical);
        if state.storage.put(&key, &v.spool).await.is_err() {
            // Falha fatal: 503 + delete da linha NOVA (CASCADE) + sweep.
            cleanup_failed_import(&state, new_id).await;
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "storage_unavailable",
                MSG_STORAGE_UNAVAILABLE,
            );
        }
        let image_ok = sqlx::query(
            "INSERT INTO images (id, dataset_id, filename, object_key, bytes, width, height, md5, sha256, media_type, split) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
        )
        .bind(image_id)
        .bind(new_id)
        .bind(&v.canonical)
        .bind(&key)
        .bind(v.bytes_len)
        .bind(v.width)
        .bind(v.height)
        .bind(&v.md5hex)
        .bind(&v.sha256hex)
        .bind(&v.media_db)
        .bind(&v.split)
        .execute(&state.pool)
        .await
        .is_ok();
        if !image_ok {
            cleanup_failed_import(&state, new_id).await;
            return internal();
        }
        if !v.boxes.is_empty() {
            let mut class_ids: Vec<Uuid> = Vec::with_capacity(v.boxes.len());
            let mut xs: Vec<f64> = Vec::with_capacity(v.boxes.len());
            let mut ys: Vec<f64> = Vec::with_capacity(v.boxes.len());
            let mut ws: Vec<f64> = Vec::with_capacity(v.boxes.len());
            let mut hs: Vec<f64> = Vec::with_capacity(v.boxes.len());
            let mut confs: Vec<Option<f64>> = Vec::with_capacity(v.boxes.len());
            let mut origins: Vec<String> = Vec::with_capacity(v.boxes.len());
            let mut tracks: Vec<Option<i32>> = Vec::with_capacity(v.boxes.len());
            let mut map_ok = true;
            for b in &v.boxes {
                match class_id_by_idx.get(&b.class_idx) {
                    Some(id) => class_ids.push(*id),
                    None => {
                        map_ok = false;
                        break;
                    }
                }
                xs.push(b.x);
                ys.push(b.y);
                ws.push(b.w);
                hs.push(b.h);
                confs.push(b.conf);
                origins.push(b.origin.clone());
                tracks.push(b.track_id);
            }
            let boxes_ok = map_ok
                && sqlx::query(
                    "INSERT INTO boxes (image_id, class_id, x, y, w, h, conf, origin, track_id) \
                     SELECT $1, t.class_id, t.x, t.y, t.w, t.h, t.conf, t.origin, t.track_id \
                     FROM unnest($2::uuid[], $3::float8[], $4::float8[], $5::float8[], $6::float8[], $7::float8[], $8::text[], $9::int[]) \
                     AS t(class_id, x, y, w, h, conf, origin, track_id)",
                )
                .bind(image_id)
                .bind(&class_ids)
                .bind(&xs)
                .bind(&ys)
                .bind(&ws)
                .bind(&hs)
                .bind(&confs)
                .bind(&origins)
                .bind(&tracks)
                .execute(&state.pool)
                .await
                .is_ok();
            if !boxes_ok {
                cleanup_failed_import(&state, new_id).await;
                return internal();
            }
        }
        if let Some(cap) = &v.caption {
            let caption_ok = sqlx::query(
                "INSERT INTO captions (image_id, text, origin, model) VALUES ($1,$2,$3,$4) \
                 ON CONFLICT (image_id) DO UPDATE SET text = EXCLUDED.text, origin = EXCLUDED.origin, model = EXCLUDED.model",
            )
            .bind(image_id)
            .bind(&cap.text)
            .bind(&cap.origin)
            .bind(cap.model.clone())
            .execute(&state.pool)
            .await
            .is_ok();
            if !caption_ok {
                cleanup_failed_import(&state, new_id).await;
                return internal();
            }
        }
        stored_ids.push(image_id);
    }

    // 8. 201 com o Dataset criado + indexação fire-and-forget (espelho do
    // upload: spawn best-effort com `eprintln`).
    let row: Option<DatasetRow> = match sqlx::query_as::<_, DatasetRow>(
        "SELECT id, slug, title, category, type, task, format, status, size_bytes, images_count, labeled_count, created_at, updated_at, EXISTS(SELECT 1 FROM images i JOIN boxes b ON b.image_id = i.id WHERE i.dataset_id = datasets.id AND i.deleted_at IS NULL AND b.origin = 'autotracker') AS auto_tracked, (SELECT count(*)::int FROM images i WHERE i.dataset_id = datasets.id AND i.deleted_at IS NOT NULL) AS trash_count FROM datasets WHERE id = $1",
    )
    .bind(new_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    let row = match row {
        Some(r) => r,
        None => return internal(),
    };
    let class_rows: Vec<(Uuid, String, i32, String)> = match sqlx::query_as(
        "SELECT id, name, idx, color FROM classes WHERE dataset_id = $1 ORDER BY idx",
    )
    .bind(new_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    let images_count = row.images_count;
    let mut resp = DatasetResponse::from(row);
    resp.classes = class_rows
        .into_iter()
        .map(DatasetClassResponse::from)
        .collect();
    resp.source = derived_source(&state.storage_config.bucket, new_id, images_count);
    if !stored_ids.is_empty() {
        let st = state.clone();
        tokio::spawn(async move {
            let wrote =
                crate::search::indexer::index_dataset_images(st, new_id, Some(stored_ids)).await;
            eprintln!("[indexer] dataset {new_id} import: {wrote} embeddings escritos");
        });
    }
    (StatusCode::CREATED, Json(resp)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn classify_raizes_permitidas() {
        assert_eq!(classify_entry("manifest.json"), Ok(EntryKind::Manifest));
        assert_eq!(classify_entry("dataset.yaml"), Ok(EntryKind::TextArtifact));
        assert_eq!(
            classify_entry("captions.jsonl"),
            Ok(EntryKind::TextArtifact)
        );
        assert_eq!(classify_entry("labels/a.txt"), Ok(EntryKind::Label));
        assert_eq!(classify_entry("images/a.jpg"), Ok(EntryKind::Image));
    }

    #[test]
    fn classify_lixo_de_mac_ignorado() {
        assert_eq!(
            classify_entry("__MACOSX/._manifest.json"),
            Ok(EntryKind::Ignored)
        );
        assert_eq!(classify_entry(".DS_Store"), Ok(EntryKind::Ignored));
        assert_eq!(classify_entry("images/.DS_Store"), Ok(EntryKind::Ignored));
        assert_eq!(classify_entry("labels/"), Ok(EntryKind::Ignored));
    }

    #[test]
    fn classify_slip_e_desconhecido_erro() {
        assert_eq!(classify_entry("../evil"), Err(()));
        assert_eq!(classify_entry("/abs"), Err(()));
        assert_eq!(classify_entry("images/../../evil"), Err(()));
        assert_eq!(classify_entry("a\\b"), Err(()));
        assert_eq!(classify_entry("evil.exe"), Err(()));
        assert_eq!(classify_entry("images/a/b.jpg"), Err(()));
        assert_eq!(classify_entry("labels"), Err(()));
    }

    #[test]
    fn scan_layout_tetos() {
        // Teto de entradas.
        let many: Vec<(String, u64)> = (0..MAX_ENTRIES + 1)
            .map(|i| (format!("images/{i}.jpg"), 1))
            .collect();
        assert_eq!(scan_layout(&many), Err(()));
        // Soma declarada > 8 GiB (declarações mentem — o reader é a defesa).
        let bomb = vec![
            ("manifest.json".to_string(), MAX_DECLARED_TOTAL),
            ("images/a.jpg".to_string(), 1),
        ];
        assert_eq!(scan_layout(&bomb), Err(()));
        // Layout honesto passa.
        let ok = vec![
            ("manifest.json".to_string(), 100),
            ("images/a.jpg".to_string(), 10),
            ("__MACOSX/._x".to_string(), 5),
        ];
        let layout = scan_layout(&ok).expect("layout honesto");
        assert!(layout.manifest_present);
        assert_eq!(layout.image_entries, vec!["images/a.jpg".to_string()]);
    }

    #[test]
    fn scan_layout_zip_real_com_malicioso() {
        // Fixture de zip malicioso: slip + entrada desconhecida ⇒ 400;
        // sem elas (só __MACOSX) ⇒ passa.
        let dir = tempfile::TempDir::new().expect("tempdir");
        let zip_path = dir.path().join("evil.zip");
        {
            let file = std::fs::File::create(&zip_path).expect("create");
            let mut zip = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default();
            for name in [
                "manifest.json",
                "images/a.jpg",
                "__MACOSX/._manifest.json",
                "evil.exe",
                "../slip",
            ] {
                zip.start_file(name, opts).expect("entry");
                zip.write_all(b"x").expect("write fixture");
            }
            zip.finish().expect("finish");
        }
        let file = std::fs::File::open(&zip_path).expect("open");
        let mut archive = zip::ZipArchive::new(file).expect("read");
        let mut names = Vec::new();
        for i in 0..archive.len() {
            let e = archive.by_index(i).expect("entry");
            names.push((e.name().to_string(), e.size()));
        }
        assert_eq!(scan_layout(&names), Err(()));
    }
}
