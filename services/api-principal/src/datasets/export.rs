//! Export de dataset — backup estruturado (ADR-0006 D1/D2, fatia 3e.1).
//!
//! `POST /api/datasets/:id/export` responde o zip em stream
//! (`application/zip`, `Content-Disposition: attachment`, `Content-Length`).
//! Pipeline em 3 fases: (A) coleta async do banco + `get_to_file` para
//! tempdir e materialização dos derivados; (B) `zip::ZipWriter` sync em
//! `spawn_blocking` (`Stored` p/ imagens, `Deflated` p/ texto); (C)
//! `ReaderStream` do arquivo spoolado → body. Só imagens ATIVAS
//! (`deleted_at IS NULL`, P5/D8). Órfã (linha sem objeto) ⇒ skip +
//! `eprintln`, export segue; `counts` refletem o exportado (R4).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use uuid::Uuid;

use super::models::parse_id;
use crate::{
    error::{err, MSG_NOT_FOUND, MSG_STORAGE_UNAVAILABLE},
    state::AppState,
    storage::StorageError,
};

const MSG_INTERNAL: &str = "internal server error";

fn internal() -> Response {
    err(StatusCode::INTERNAL_SERVER_ERROR, "internal", MSG_INTERNAL)
}

// ---------------------------------------------------------------------------
// Manifest (D1) — artefato de transporte snake_case, FONTE DA VERDADE.
// ---------------------------------------------------------------------------

/// `counts` informativo do manifest (reflete o que foi EXPORTADO, R4).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ManifestCounts {
    pub images: usize,
    pub labeled: usize,
    pub classes: usize,
    pub size_bytes: i64,
}

/// Cabeçalho `dataset` do manifest.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ManifestDataset {
    pub name: String,
    pub title: String,
    pub r#type: String,
    pub format: String,
    pub counts: ManifestCounts,
}

/// Classe do manifest (`idx` = posição no array — estável no roundtrip).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ManifestClass {
    pub idx: i32,
    pub name: String,
    pub color: String,
}

/// Box COMPLETA (fidelidade obrigatória — origin/conf/track_id sobrevivem).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ManifestBox {
    pub class_idx: i32,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub conf: Option<f64>,
    pub origin: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub track_id: Option<i32>,
}

/// Caption do manifest.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ManifestCaption {
    pub text: String,
    pub origin: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model: Option<String>,
}

/// Imagem do manifest.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ManifestImage {
    pub filename: String,
    pub split: String,
    pub width: i32,
    pub height: i32,
    pub bytes: i64,
    pub sha256: String,
    pub media_type: String,
    #[serde(default)]
    pub boxes: Vec<ManifestBox>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub caption: Option<ManifestCaption>,
}

/// `manifest.json` — `schema_version: 1`, `exported_at` RFC 3339.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExportManifest {
    pub schema_version: u32,
    pub exported_at: String,
    pub dataset: ManifestDataset,
    pub classes: Vec<ManifestClass>,
    pub images: Vec<ManifestImage>,
}

// ---------------------------------------------------------------------------
// Builder puro (unit-testável sem banco): domínios, class_idx, counts.
// ---------------------------------------------------------------------------

/// Linha de classe vinda do banco (ordem de `idx`).
pub struct ExportClassRow {
    pub id: Uuid,
    pub name: String,
    pub idx: i32,
    pub color: String,
}

/// Linha de imagem ATIVA vinda do banco.
pub struct ExportImageRow {
    pub id: Uuid,
    pub filename: String,
    pub object_key: String,
    pub bytes: i64,
    pub width: i32,
    pub height: i32,
    pub sha256: String,
    pub media_type: String,
    pub split: String,
}

/// Linha de box vinda do banco (JOIN com imagens ativas).
pub struct ExportBoxRow {
    pub image_id: Uuid,
    pub class_id: Uuid,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub conf: Option<f64>,
    pub origin: String,
    pub track_id: Option<i32>,
}

/// Linha de caption vinda do banco (JOIN com imagens ativas).
pub struct ExportCaptionRow {
    pub image_id: Uuid,
    pub text: String,
    pub origin: String,
    pub model: Option<String>,
}

/// Monta o manifest a partir das linhas JÁ filtradas (ativas + exportadas).
/// `class_idx` = posição no array `classes`; origem preservada literal.
/// `labeled` segue a taxonomia do trigger (`yolo_txt` ⇒ com box, demais ⇒
/// com caption). `exported_at` = agora (RFC 3339).
pub fn build_manifest(
    slug: &str,
    title: &str,
    dataset_type: &str,
    format: &str,
    classes: &[ExportClassRow],
    images: &[ExportImageRow],
    boxes: &[ExportBoxRow],
    captions: &[ExportCaptionRow],
) -> ExportManifest {
    let mut class_idx_by_id: HashMap<Uuid, i32> = HashMap::new();
    let mut manifest_classes = Vec::with_capacity(classes.len());
    for (pos, c) in classes.iter().enumerate() {
        class_idx_by_id.insert(c.id, pos as i32);
        manifest_classes.push(ManifestClass {
            idx: pos as i32,
            name: c.name.clone(),
            color: c.color.clone(),
        });
    }
    let mut boxes_by_image: HashMap<Uuid, Vec<&ExportBoxRow>> = HashMap::new();
    for b in boxes {
        boxes_by_image.entry(b.image_id).or_default().push(b);
    }
    let mut caption_by_image: HashMap<Uuid, &ExportCaptionRow> = HashMap::new();
    for c in captions {
        caption_by_image.insert(c.image_id, c);
    }
    let mut manifest_images = Vec::with_capacity(images.len());
    let mut labeled = 0usize;
    let mut size_bytes = 0i64;
    for img in images {
        let img_boxes: Vec<ManifestBox> = boxes_by_image
            .get(&img.id)
            .map(|rows| {
                rows.iter()
                    .map(|b| ManifestBox {
                        class_idx: class_idx_by_id.get(&b.class_id).copied().unwrap_or(0),
                        x: b.x,
                        y: b.y,
                        w: b.w,
                        h: b.h,
                        conf: b.conf,
                        origin: b.origin.clone(),
                        track_id: b.track_id,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let caption = caption_by_image.get(&img.id).map(|c| ManifestCaption {
            text: c.text.clone(),
            origin: c.origin.clone(),
            model: c.model.clone(),
        });
        let is_labeled = if format == "yolo_txt" {
            !img_boxes.is_empty()
        } else {
            caption.is_some()
        };
        if is_labeled {
            labeled += 1;
        }
        size_bytes += img.bytes;
        manifest_images.push(ManifestImage {
            filename: img.filename.clone(),
            split: img.split.clone(),
            width: img.width,
            height: img.height,
            bytes: img.bytes,
            sha256: img.sha256.clone(),
            media_type: img.media_type.clone(),
            boxes: img_boxes,
            caption,
        });
    }
    ExportManifest {
        schema_version: 1,
        exported_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        dataset: ManifestDataset {
            name: slug.to_string(),
            title: title.to_string(),
            r#type: dataset_type.to_string(),
            format: format.to_string(),
            counts: ManifestCounts {
                images: images.len(),
                labeled,
                classes: classes.len(),
                size_bytes,
            },
        },
        classes: manifest_classes,
        images: manifest_images,
    }
}

/// Stem = filename sem extensão (para `labels/<stem>.txt`).
pub fn stem_of(filename: &str) -> &str {
    match filename.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => filename,
    }
}

/// Extensão canônica do filename sem o ponto (para desambiguar labels).
fn ext_of(filename: &str) -> &str {
    match filename.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => ext,
        _ => "bin",
    }
}

/// Arcname do label YOLO de uma imagem. A primeira imagem com um dado stem
/// fica com `labels/{stem}.txt`; em colisão (ex.: `a.jpg` + `a.png`) a
/// imagem corrente é desambiguada para `labels/{stem}_{ext}.txt` (ordem do
/// manifest/created_at ⇒ determinístico). `used` guarda os arcnames já
/// emitidos. Só o arcname muda — o conteúdo continua o da imagem.
fn label_arcname_for(filename: &str, used: &mut std::collections::HashSet<String>) -> String {
    let stem = stem_of(filename);
    let proposed = format!("labels/{stem}.txt");
    if used.insert(proposed.clone()) {
        return proposed;
    }
    let disambiguated = format!("labels/{stem}_{}.txt", ext_of(filename));
    eprintln!("[export] labels de mesmo stem colidem; desambiguado {proposed} para {disambiguated} ({filename})");
    used.insert(disambiguated.clone());
    disambiguated
}

/// Entradas `labels/*.txt` para imagens com ≥1 box (puro, unit-testável):
/// `(arcname, conteúdo YOLO da imagem correspondente)`.
pub fn build_label_entries(images: &[ManifestImage]) -> Vec<(String, String)> {
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out = Vec::new();
    for img in images {
        if img.boxes.is_empty() {
            continue;
        }
        let arcname = label_arcname_for(&img.filename, &mut used);
        out.push((arcname, yolo_label_lines(&img.boxes)));
    }
    out
}

/// Uma linha `cls cx cy w h` por box, 6 decimais (materialização YOLO —
/// sem conf/origin/track_id por construção).
pub fn yolo_label_lines(boxes: &[ManifestBox]) -> String {
    let mut out = String::new();
    for b in boxes {
        out.push_str(&format!(
            "{} {:.6} {:.6} {:.6} {:.6}\n",
            b.class_idx, b.x, b.y, b.w, b.h
        ));
    }
    out
}

/// `dataset.yaml` derivado (só `yolo_txt`): `path: .`, `train:`/`val:` como
/// LISTAS de `images/<filename>`, `names: {idx: name}`.
pub fn render_dataset_yaml(manifest: &ExportManifest) -> String {
    let mut train: Vec<String> = Vec::new();
    let mut val: Vec<String> = Vec::new();
    for img in &manifest.images {
        let p = format!("images/{}", img.filename);
        if img.split == "val" {
            val.push(p);
        } else {
            train.push(p);
        }
    }
    let mut out = String::from("path: .\n");
    out.push_str("train: [");
    out.push_str(
        &train
            .iter()
            .map(|p| format!("{p}"))
            .collect::<Vec<_>>()
            .join(", "),
    );
    out.push_str("]\nval: [");
    out.push_str(&val.join(", "));
    out.push_str("]\nnames:\n");
    for c in &manifest.classes {
        out.push_str(&format!("  {}: {}\n", c.idx, c.name));
    }
    out
}

/// `captions.jsonl` derivado: `{"image": "<filename>", "caption": "..."}`.
pub fn render_captions_jsonl(manifest: &ExportManifest) -> String {
    let mut out = String::new();
    for img in &manifest.images {
        if let Some(cap) = &img.caption {
            let line = serde_json::json!({"image": img.filename, "caption": cap.text});
            out.push_str(&line.to_string());
            out.push('\n');
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Fase B — zip sync em `spawn_blocking` (D2).
// ---------------------------------------------------------------------------

/// Entrada a zipar: nome no arquivo + caminho no tempdir + método.
pub struct ZipEntry {
    pub arcname: String,
    pub fs_path: PathBuf,
    pub is_text: bool,
}

/// Escreve o zip (sync — roda em `spawn_blocking`): `Stored` para
/// `images/*` (mídia já comprimida), `Deflated` para artefatos de texto.
pub fn write_zip(entries: &[ZipEntry], zip_path: &Path) -> std::io::Result<()> {
    let file = std::fs::File::create(zip_path)?;
    let mut zip = zip::ZipWriter::new(file);
    for e in entries {
        let method = if e.is_text {
            zip::CompressionMethod::Deflated
        } else {
            zip::CompressionMethod::Stored
        };
        let options = zip::write::SimpleFileOptions::default().compression_method(method);
        zip.start_file(&e.arcname, options)?;
        let mut src = std::fs::File::open(&e.fs_path)?;
        std::io::copy(&mut src, &mut zip)?;
    }
    zip.finish()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Handler — Fase A (async) → B (blocking) → C (stream).
// ---------------------------------------------------------------------------

/// Valor do `Content-Disposition` do export (`{slug}.zip`).
pub fn content_disposition(slug: &str) -> String {
    format!("attachment; filename=\"{slug}.zip\"")
}

/// POST /api/datasets/:id/export — 200 zip | 404 `not_found` | 503
/// `storage_unavailable`. 401 vem do gate, nunca daqui.
pub async fn export_dataset(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let id = match parse_id(&id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    // Dataset (slug/title/type/format p/ o manifest).
    let ds: Option<(String, String, String, String)> =
        match sqlx::query_as("SELECT slug, title, type, format FROM datasets WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.pool)
            .await
        {
            Ok(r) => r,
            Err(_) => return internal(),
        };
    let (slug, title, dataset_type, format) = match ds {
        Some(r) => r,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };
    // Classes ordenadas por idx (posição = class_idx do manifest).
    let class_rows: Vec<(Uuid, String, i32, String)> = match sqlx::query_as(
        "SELECT id, name, idx, color FROM classes WHERE dataset_id = $1 ORDER BY idx",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    let classes: Vec<ExportClassRow> = class_rows
        .into_iter()
        .map(|(cid, name, idx, color)| ExportClassRow {
            id: cid,
            name,
            idx,
            color,
        })
        .collect();
    // Imagens ATIVAS (P5/D8).
    let image_rows: Vec<(
        Uuid,
        String,
        String,
        i64,
        i32,
        i32,
        String,
        String,
        String,
    )> = match sqlx::query_as(
        "SELECT id, filename, object_key, bytes, width, height, sha256, media_type, split FROM images WHERE dataset_id = $1 AND deleted_at IS NULL ORDER BY created_at, id",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    // Boxes das imagens ativas (via JOIN — lixeira fora, D8).
    let box_rows: Vec<(
        Uuid,
        Uuid,
        f64,
        f64,
        f64,
        f64,
        Option<f64>,
        String,
        Option<i32>,
    )> = match sqlx::query_as(
        "SELECT b.image_id, b.class_id, b.x, b.y, b.w, b.h, b.conf, b.origin, b.track_id FROM boxes b JOIN images i ON i.id = b.image_id WHERE i.dataset_id = $1 AND i.deleted_at IS NULL",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    // Captions das imagens ativas.
    let caption_rows: Vec<(Uuid, String, String, Option<String>)> = match sqlx::query_as(
        "SELECT c.image_id, c.text, c.origin, c.model FROM captions c JOIN images i ON i.id = c.image_id WHERE i.dataset_id = $1 AND i.deleted_at IS NULL",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };

    // Fase A: tempdir + `get_to_file` por imagem (órfã ⇒ skip + eprintln).
    let tmp = match tempfile::TempDir::new() {
        Ok(d) => d,
        Err(_) => return internal(),
    };
    let images_dir = tmp.path().join("images");
    let labels_dir = tmp.path().join("labels");
    if tokio::fs::create_dir_all(&images_dir).await.is_err() {
        return internal();
    }
    if tokio::fs::create_dir_all(&labels_dir).await.is_err() {
        return internal();
    }
    let mut exported: Vec<ExportImageRow> = Vec::new();
    for (img_id, filename, object_key, bytes, width, height, sha256, media_type, split) in
        image_rows
    {
        let dest = images_dir.join(&filename);
        match state.storage.get_to_file(&object_key, &dest).await {
            Ok(()) => exported.push(ExportImageRow {
                id: img_id,
                filename,
                object_key,
                bytes,
                width,
                height,
                sha256,
                media_type,
                split,
            }),
            Err(StorageError::NotFound) => {
                // R4: linha sem objeto — skip honesto, export segue.
                eprintln!("aviso: export pulou imagem órfã {img_id} ({filename}) — objeto ausente");
            }
            Err(StorageError::Unavailable(_)) => {
                return err(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "storage_unavailable",
                    MSG_STORAGE_UNAVAILABLE,
                );
            }
        }
    }
    let exported_ids: std::collections::HashSet<Uuid> = exported.iter().map(|i| i.id).collect();
    let boxes: Vec<ExportBoxRow> = box_rows
        .into_iter()
        .filter(|(img_id, _, _, _, _, _, _, _, _)| exported_ids.contains(img_id))
        .map(
            |(image_id, class_id, x, y, w, h, conf, origin, track_id)| ExportBoxRow {
                image_id,
                class_id,
                x,
                y,
                w,
                h,
                conf,
                origin,
                track_id,
            },
        )
        .collect();
    let captions: Vec<ExportCaptionRow> = caption_rows
        .into_iter()
        .filter(|(img_id, _, _, _)| exported_ids.contains(img_id))
        .map(|(image_id, text, origin, model)| ExportCaptionRow {
            image_id,
            text,
            origin,
            model,
        })
        .collect();
    let manifest = build_manifest(
        &slug,
        &title,
        &dataset_type,
        &format,
        &classes,
        &exported,
        &boxes,
        &captions,
    );
    // Materialização no tempdir.
    let manifest_path = tmp.path().join("manifest.json");
    let manifest_json = serde_json::to_string_pretty(&manifest).unwrap_or_else(|_| "{}".into());
    if tokio::fs::write(&manifest_path, manifest_json)
        .await
        .is_err()
    {
        return internal();
    }
    let mut entries: Vec<ZipEntry> = vec![ZipEntry {
        arcname: "manifest.json".to_string(),
        fs_path: manifest_path,
        is_text: true,
    }];
    if format == "yolo_txt" {
        let yaml_path = tmp.path().join("dataset.yaml");
        if tokio::fs::write(&yaml_path, render_dataset_yaml(&manifest))
            .await
            .is_err()
        {
            return internal();
        }
        entries.push(ZipEntry {
            arcname: "dataset.yaml".to_string(),
            fs_path: yaml_path,
            is_text: true,
        });
    }
    let mut used_labels: std::collections::HashSet<String> = std::collections::HashSet::new();
    for img in &manifest.images {
        if img.boxes.is_empty() {
            continue;
        }
        let arcname = label_arcname_for(&img.filename, &mut used_labels);
        let label_path = labels_dir.join(arcname.strip_prefix("labels/").unwrap_or(&arcname));
        if tokio::fs::write(&label_path, yolo_label_lines(&img.boxes))
            .await
            .is_err()
        {
            return internal();
        }
        entries.push(ZipEntry {
            arcname,
            fs_path: label_path,
            is_text: true,
        });
    }
    let captions_text = render_captions_jsonl(&manifest);
    if !captions_text.is_empty() {
        let captions_path = tmp.path().join("captions.jsonl");
        if tokio::fs::write(&captions_path, &captions_text)
            .await
            .is_err()
        {
            return internal();
        }
        entries.push(ZipEntry {
            arcname: "captions.jsonl".to_string(),
            fs_path: captions_path,
            is_text: true,
        });
    }
    for img in &manifest.images {
        entries.push(ZipEntry {
            arcname: format!("images/{}", img.filename),
            fs_path: images_dir.join(&img.filename),
            is_text: false,
        });
    }

    // Fase B: zip sync em spawn_blocking.
    let zip_path = tmp.path().join(format!("{slug}.zip"));
    let zip_path_blocking = zip_path.clone();
    let blocking: Result<std::io::Result<()>, tokio::task::JoinError> =
        tokio::task::spawn_blocking(move || write_zip(&entries, &zip_path_blocking)).await;
    let zip_ok = match blocking {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    if zip_ok.is_err() {
        return internal();
    }
    // Fase C: ReaderStream do arquivo spoolado → body. O fd abre ANTES do
    // drop do TempDir (no Unix o unlink pós-open não quebra a leitura).
    let len = match tokio::fs::metadata(&zip_path).await {
        Ok(m) => m.len(),
        Err(_) => return internal(),
    };
    let file = match tokio::fs::File::open(&zip_path).await {
        Ok(f) => f,
        Err(_) => return internal(),
    };
    let stream = tokio_util::io::ReaderStream::new(file);
    let body = Body::from_stream(stream);
    (
        StatusCode::OK,
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/zip".to_string(),
            ),
            (
                axum::http::header::CONTENT_DISPOSITION,
                content_disposition(&slug),
            ),
            (axum::http::header::CONTENT_LENGTH, len.to_string()),
        ],
        body,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(id: Uuid, name: &str, idx: i32) -> ExportClassRow {
        ExportClassRow {
            id,
            name: name.to_string(),
            idx,
            color: crate::datasets::models::color_for(idx as usize).to_string(),
        }
    }

    fn img(id: Uuid, filename: &str, split: &str) -> ExportImageRow {
        ExportImageRow {
            id,
            filename: filename.to_string(),
            object_key: format!("k/{filename}"),
            bytes: 100,
            width: 64,
            height: 48,
            sha256: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".to_string(),
            media_type: "jpeg".to_string(),
            split: split.to_string(),
        }
    }

    #[test]
    fn manifest_class_idx_origem_e_counts() {
        let c0 = Uuid::new_v4();
        let c1 = Uuid::new_v4();
        let i1 = Uuid::new_v4();
        let i2 = Uuid::new_v4();
        let classes = vec![class(c0, "solda_fria", 0), class(c1, "ponte", 1)];
        let images = vec![
            img(i1, "img_0001.jpg", "train"),
            img(i2, "img_0002.jpg", "val"),
        ];
        let boxes = vec![ExportBoxRow {
            image_id: i1,
            class_id: c1,
            x: 0.5,
            y: 0.5,
            w: 0.2,
            h: 0.3,
            conf: Some(0.96),
            origin: "autotracker".to_string(),
            track_id: Some(7),
        }];
        let captions = vec![ExportCaptionRow {
            image_id: i2,
            text: "placa ok".to_string(),
            origin: "manual".to_string(),
            model: None,
        }];
        let m = build_manifest(
            "pcb",
            "PCB",
            "yolo_bbox",
            "yolo_txt",
            &classes,
            &images,
            &boxes,
            &captions,
        );
        assert_eq!(m.schema_version, 1);
        assert_eq!(m.classes.len(), 2);
        assert_eq!(m.classes[1].name, "ponte");
        // class_idx = posição no array classes (c1 ⇒ 1).
        assert_eq!(m.images[0].boxes.len(), 1);
        let b = &m.images[0].boxes[0];
        assert_eq!(b.class_idx, 1);
        assert_eq!(b.origin, "autotracker");
        assert_eq!(b.conf, Some(0.96));
        assert_eq!(b.track_id, Some(7));
        assert!(m.images[1].boxes.is_empty());
        // yolo_txt: labeled = com box.
        assert_eq!(m.dataset.counts.images, 2);
        assert_eq!(m.dataset.counts.labeled, 1);
        assert_eq!(m.dataset.counts.classes, 1 + 1);
        assert_eq!(m.dataset.counts.size_bytes, 200);
        // Caption preservada com origin/model.
        assert_eq!(m.images[1].caption.as_ref().unwrap().text, "placa ok");
    }

    #[test]
    fn manifest_labeled_taxonomia_nao_yolo() {
        let c0 = Uuid::new_v4();
        let i1 = Uuid::new_v4();
        let classes = vec![class(c0, "a", 0)];
        let images = vec![img(i1, "a.jpg", "train")];
        let boxes = vec![ExportBoxRow {
            image_id: i1,
            class_id: c0,
            x: 0.1,
            y: 0.1,
            w: 0.1,
            h: 0.1,
            conf: None,
            origin: "manual".to_string(),
            track_id: None,
        }];
        let m = build_manifest(
            "d",
            "D",
            "clip_image_text",
            "pairs",
            &classes,
            &images,
            &boxes,
            &[],
        );
        // pairs: labeled = com caption (box sozinha não conta).
        assert_eq!(m.dataset.counts.labeled, 0);
    }

    #[test]
    fn dataset_yaml_listas_e_names() {
        let c0 = Uuid::new_v4();
        let i1 = Uuid::new_v4();
        let i2 = Uuid::new_v4();
        let classes = vec![class(c0, "solda_fria", 0)];
        let images = vec![
            img(i1, "img_0001.jpg", "train"),
            img(i2, "img_0002.jpg", "val"),
        ];
        let m = build_manifest(
            "d",
            "D",
            "yolo_bbox",
            "yolo_txt",
            &classes,
            &images,
            &[],
            &[],
        );
        let yaml = render_dataset_yaml(&m);
        assert!(yaml.contains("path: ."), "{yaml}");
        assert!(yaml.contains("train: [images/img_0001.jpg]"), "{yaml}");
        assert!(yaml.contains("val: [images/img_0002.jpg]"), "{yaml}");
        assert!(yaml.contains("0: solda_fria"), "{yaml}");
        // Parseável como YAML válido.
        let v: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("yaml válido");
        assert_eq!(v["path"].as_str(), Some("."));
    }

    #[test]
    fn labels_seis_decimais_e_captions_jsonl() {
        let boxes = vec![ManifestBox {
            class_idx: 0,
            x: 0.5,
            y: 0.5,
            w: 0.2,
            h: 0.3,
            conf: Some(0.96),
            origin: "autotracker".to_string(),
            track_id: Some(7),
        }];
        assert_eq!(
            yolo_label_lines(&boxes),
            "0 0.500000 0.500000 0.200000 0.300000\n"
        );
        assert_eq!(yolo_label_lines(&[]), "");
        let m = ExportManifest {
            schema_version: 1,
            exported_at: "2026-09-07T12:00:00Z".to_string(),
            dataset: ManifestDataset {
                name: "d".to_string(),
                title: "D".to_string(),
                r#type: "yolo_bbox".to_string(),
                format: "yolo_txt".to_string(),
                counts: ManifestCounts {
                    images: 1,
                    labeled: 0,
                    classes: 0,
                    size_bytes: 0,
                },
            },
            classes: vec![],
            images: vec![ManifestImage {
                filename: "img_0042.jpg".to_string(),
                split: "train".to_string(),
                width: 1,
                height: 1,
                bytes: 1,
                sha256: "x".to_string(),
                media_type: "jpeg".to_string(),
                boxes: vec![],
                caption: Some(ManifestCaption {
                    text: "uma placa".to_string(),
                    origin: "manual".to_string(),
                    model: None,
                }),
            }],
        };
        assert_eq!(
            render_captions_jsonl(&m),
            "{\"caption\":\"uma placa\",\"image\":\"img_0042.jpg\"}\n"
        );
    }

    #[test]
    fn zip_roundtrip_write_read() {
        // Critério de inversão embutido (ADR-0006, spike não obrigatório): o
        // próprio crate prova write→read com as entradas esperadas.
        let dir = tempfile::TempDir::new().expect("tempdir");
        let a = dir.path().join("manifest.json");
        let b = dir.path().join("img.jpg");
        std::fs::write(&a, r#"{"schema_version":1}"#).expect("seed");
        std::fs::write(&b, [0xFFu8, 0xD8, 0xFF]).expect("seed");
        let entries = vec![
            ZipEntry {
                arcname: "manifest.json".to_string(),
                fs_path: a,
                is_text: true,
            },
            ZipEntry {
                arcname: "images/img.jpg".to_string(),
                fs_path: b,
                is_text: false,
            },
        ];
        let zip_path = dir.path().join("d.zip");
        write_zip(&entries, &zip_path).expect("write");
        let file = std::fs::File::open(&zip_path).expect("open");
        let mut archive = zip::ZipArchive::new(file).expect("read");
        assert_eq!(archive.len(), 2);
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).expect("entry").name().to_string())
            .collect();
        assert_eq!(names, vec!["manifest.json", "images/img.jpg"]);
        let mut manifest = archive.by_name("manifest.json").expect("manifest");
        let mut text = String::new();
        std::io::Read::read_to_string(&mut manifest, &mut text).expect("read text");
        assert_eq!(text, r#"{"schema_version":1}"#);
    }

    #[test]
    fn manifest_snake_case_na_serializacao() {
        let m = ExportManifest {
            schema_version: 1,
            exported_at: "2026-09-07T12:00:00Z".to_string(),
            dataset: ManifestDataset {
                name: "d".to_string(),
                title: "D".to_string(),
                r#type: "yolo_bbox".to_string(),
                format: "yolo_txt".to_string(),
                counts: ManifestCounts {
                    images: 0,
                    labeled: 0,
                    classes: 0,
                    size_bytes: 0,
                },
            },
            classes: vec![],
            images: vec![],
        };
        let v = serde_json::to_value(&m).expect("json");
        assert!(v.get("schema_version").is_some());
        assert!(v.get("exported_at").is_some());
        assert!(v["dataset"].get("size_bytes").is_none());
        assert!(v["dataset"]["counts"].get("size_bytes").is_some());
        assert!(v.get("schemaVersion").is_none(), "manifest é snake_case");
    }

    #[test]
    fn stem_of_regras() {
        assert_eq!(stem_of("img_0001.jpg"), "img_0001");
        assert_eq!(stem_of("sem_ext"), "sem_ext");
    }

    #[test]
    fn labels_mesmo_stem_desambiguado_por_ext() {
        // `a.jpg` + `a.png` (mesmo stem, extensões distintas) ⇒ arcnames
        // `labels/a.txt` + `labels/a_png.txt`, sem duplicata, cada conteúdo
        // com as boxes da imagem certa.
        let c0 = Uuid::new_v4();
        let i1 = Uuid::new_v4();
        let i2 = Uuid::new_v4();
        let classes = vec![class(c0, "a", 0)];
        let images = vec![img(i1, "a.jpg", "train"), img(i2, "a.png", "train")];
        let boxes = vec![
            ExportBoxRow {
                image_id: i1,
                class_id: c0,
                x: 0.1,
                y: 0.1,
                w: 0.1,
                h: 0.1,
                conf: None,
                origin: "manual".to_string(),
                track_id: None,
            },
            ExportBoxRow {
                image_id: i2,
                class_id: c0,
                x: 0.9,
                y: 0.9,
                w: 0.05,
                h: 0.05,
                conf: None,
                origin: "manual".to_string(),
                track_id: None,
            },
        ];
        let m = build_manifest(
            "d",
            "D",
            "yolo_bbox",
            "yolo_txt",
            &classes,
            &images,
            &boxes,
            &[],
        );
        let entries = build_label_entries(&m.images);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "labels/a.txt");
        assert_eq!(entries[1].0, "labels/a_png.txt");
        let uniq: std::collections::HashSet<&str> =
            entries.iter().map(|(a, _)| a.as_str()).collect();
        assert_eq!(uniq.len(), 2, "sem duplicata de arcname");
        assert_eq!(entries[0].1, yolo_label_lines(&m.images[0].boxes));
        assert_eq!(entries[1].1, yolo_label_lines(&m.images[1].boxes));
        assert_ne!(entries[0].1, entries[1].1);
    }

    #[test]
    fn content_disposition_formato() {
        assert_eq!(
            content_disposition("inspecao-pcb-defeitos-v2"),
            "attachment; filename=\"inspecao-pcb-defeitos-v2.zip\""
        );
    }

    #[test]
    fn orfa_skip_counts_coerentes() {
        // R4: a órfã (linha sem objeto) nem chega ao builder — `counts`
        // refletem só o exportado; boxes/captions da órfã caem junto.
        let c0 = Uuid::new_v4();
        let boa = Uuid::new_v4();
        let orfa = Uuid::new_v4();
        let classes = vec![class(c0, "a", 0)];
        let images = vec![img(boa, "boa.jpg", "train")];
        let boxes = vec![
            ExportBoxRow {
                image_id: boa,
                class_id: c0,
                x: 0.5,
                y: 0.5,
                w: 0.2,
                h: 0.2,
                conf: None,
                origin: "manual".to_string(),
                track_id: None,
            },
            ExportBoxRow {
                image_id: orfa,
                class_id: c0,
                x: 0.1,
                y: 0.1,
                w: 0.1,
                h: 0.1,
                conf: None,
                origin: "manual".to_string(),
                track_id: None,
            },
        ];
        // O handler filtra boxes/captions pelos ids exportados; aqui a órfã
        // já saiu de `images` mas sua box ainda está no lote (prova do filtro).
        let exported_ids: std::collections::HashSet<Uuid> = images.iter().map(|i| i.id).collect();
        let kept: Vec<&ExportBoxRow> = boxes
            .iter()
            .filter(|b| exported_ids.contains(&b.image_id))
            .collect();
        assert_eq!(kept.len(), 1);
        let m = build_manifest(
            "d",
            "D",
            "yolo_bbox",
            "yolo_txt",
            &classes,
            &images,
            &boxes[..1],
            &[],
        );
        assert_eq!(m.dataset.counts.images, 1);
        assert_eq!(m.dataset.counts.labeled, 1);
        assert_eq!(m.images.len(), 1);
    }

    #[test]
    fn dataset_vazio_so_manifest() {
        // Dataset vazio ⇒ 200 com zip só-manifest (`counts.images=0`).
        let m = build_manifest(
            "vazio",
            "Vazio",
            "yolo_bbox",
            "yolo_txt",
            &[],
            &[],
            &[],
            &[],
        );
        assert_eq!(m.dataset.counts.images, 0);
        assert_eq!(m.dataset.counts.labeled, 0);
        assert_eq!(
            render_dataset_yaml(&m),
            "path: .\ntrain: []\nval: []\nnames:\n"
        );
        assert_eq!(render_captions_jsonl(&m), "");
        let dir = tempfile::TempDir::new().expect("tempdir");
        let manifest_path = dir.path().join("manifest.json");
        std::fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&m).expect("json"),
        )
        .expect("seed");
        let entries = vec![ZipEntry {
            arcname: "manifest.json".to_string(),
            fs_path: manifest_path,
            is_text: true,
        }];
        let zip_path = dir.path().join("vazio.zip");
        write_zip(&entries, &zip_path).expect("write");
        let file = std::fs::File::open(&zip_path).expect("open");
        let archive = zip::ZipArchive::new(file).expect("read");
        assert_eq!(archive.len(), 1);
    }

    #[tokio::test]
    async fn handler_id_nao_uuid_404_sem_banco() {
        // Fiação da rota: id inválido ⇒ 404 `not_found` sem tocar o pool.
        let pool = sqlx::PgPool::connect_lazy("postgres://n/n").expect("lazy");
        let state = crate::state::AppState {
            pool,
            jwt_secret: [0x42; 32],
            secure_cookie: false,
            setup_required: false,
            storage: std::sync::Arc::new(crate::storage::MockStorage::new()),
            storage_config: crate::storage::MockStorage::test_config(),
            embedder: std::sync::Arc::new(crate::search::MockEmbedder::new()),
            embedding_model: "ViT-B-32".to_string(),
            manager: std::sync::Arc::new(crate::jobs::manager_client::MockManager::default()),
            model_download_allowed_hosts: vec![],
        };
        let resp = export_dataset(
            axum::extract::State(state),
            axum::extract::Path("nao-e-uuid".to_string()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
