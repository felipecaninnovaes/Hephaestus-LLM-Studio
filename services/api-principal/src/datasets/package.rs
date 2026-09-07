//! Package de dataset — versão congelada + materialização YOLO + zip (ADR-0007 D1).
//!
//! `POST /api/datasets/:id/package` congela `dataset_versions{manifest}`,
//! materializa a árvore YOLO em tempdir, gera zip e faz PUT em
//! `packages/<version_id>/dataset.zip` + `manifest.json`.

use std::collections::HashMap;
use std::io::Read as _;
use std::path::Path;

use axum::{
    body::Bytes,
    extract::{
        rejection::{BytesRejection, FailedToBufferBody},
        Path as AxumPath, State,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use uuid::Uuid;

use super::export::{
    build_label_entries, render_dataset_yaml, write_zip, ExportManifest, ZipEntry,
};
use super::models::parse_id;
use crate::{
    error::{
        err, MSG_ENGINE_UNSUPPORTED, MSG_INVALID_REQUEST, MSG_NOT_FOUND, MSG_STORAGE_UNAVAILABLE,
    },
    state::AppState,
};

const MSG_INTERNAL: &str = "internal server error";

fn internal() -> Response {
    err(StatusCode::INTERNAL_SERVER_ERROR, "internal", MSG_INTERNAL)
}

// ---------------------------------------------------------------------------
// Snapshot manifest — congelado em dataset_versions.manifest (JSONB snake_case).
// Diferente do ExportManifest: sem sha256/bytes/media_type, sem schema_version,
// com snapshot do dataset em vez de dados derivados.
// ---------------------------------------------------------------------------

/// `counts` do snapshot (reflete o dataset no momento do congelamento).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotCounts {
    pub images: usize,
    pub labeled: usize,
    pub classes: usize,
}

/// Dataset no snapshot (metadados congelados).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotDataset {
    pub id: Uuid,
    pub slug: String,
    pub category: String,
    pub engine: String,
}

/// Classe no snapshot (idx = posição no array).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotClass {
    pub idx: i32,
    pub name: String,
}

/// Box no snapshot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotBox {
    pub class_idx: i32,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Imagem no snapshot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotImage {
    pub filename: String,
    pub split: String,
    pub width: i32,
    pub height: i32,
    pub boxes: Vec<SnapshotBox>,
}

/// Manifesto do package (congelado como JSONB em `dataset_versions.manifest`).
/// Formato snake_case (transporte interno).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PackageManifest {
    pub dataset: SnapshotDataset,
    pub classes: Vec<SnapshotClass>,
    pub images: Vec<SnapshotImage>,
    pub counts: SnapshotCounts,
}

// ---------------------------------------------------------------------------
// Request / Response (wire camelCase).
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageRequest {
    pub engine: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageResponse {
    pub version_id: String,
    pub key: String,
    pub bytes: i64,
    pub md5_zip: String,
    pub files: Vec<TransportFile>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportFile {
    pub filename: String,
    pub md5: String,
    pub bytes: i64,
}

// ---------------------------------------------------------------------------
// Conversão: PackageManifest → ExportManifest (reuso do builder compartilhado).
// ---------------------------------------------------------------------------

fn snapshot_to_export_manifest(snap: &PackageManifest) -> ExportManifest {
    let mut class_idx_by_id: HashMap<i32, usize> = HashMap::new();
    for (pos, c) in snap.classes.iter().enumerate() {
        class_idx_by_id.insert(c.idx, pos);
    }

    let mut labeled = 0usize;
    let mut images_out = Vec::with_capacity(snap.images.len());
    for img in &snap.images {
        let boxes: Vec<super::export::ManifestBox> = img
            .boxes
            .iter()
            .map(|b| super::export::ManifestBox {
                class_idx: b.class_idx,
                x: b.x,
                y: b.y,
                w: b.w,
                h: b.h,
                conf: None,
                origin: "package".to_string(),
                track_id: None,
            })
            .collect();
        if !boxes.is_empty() {
            labeled += 1;
        }
        images_out.push(super::export::ManifestImage {
            filename: img.filename.clone(),
            split: img.split.clone(),
            width: img.width,
            height: img.height,
            bytes: 0,
            sha256: String::new(),
            media_type: "jpeg".to_string(),
            boxes,
            caption: None,
        });
    }
    ExportManifest {
        schema_version: 1,
        exported_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        dataset: super::export::ManifestDataset {
            name: snap.dataset.slug.clone(),
            title: snap.dataset.slug.clone(),
            r#type: "yolo_bbox".to_string(),
            format: "yolo_txt".to_string(),
            counts: super::export::ManifestCounts {
                images: snap.counts.images,
                labeled,
                classes: snap.counts.classes,
                size_bytes: 0,
            },
        },
        classes: snap
            .classes
            .iter()
            .map(|c| super::export::ManifestClass {
                idx: c.idx,
                name: c.name.clone(),
                color: crate::datasets::models::color_for(c.idx as usize).to_string(),
            })
            .collect(),
        images: images_out,
    }
}

// ---------------------------------------------------------------------------
// Materialização YOLO (compartilhada): manifest → tempdir com árvore YOLO.
// ---------------------------------------------------------------------------

/// Materializa a árvore YOLO em tempdir: `images/`, `labels/`, `dataset.yaml`.
/// Retorna o caminho do tempdir (caller é responsável pelo drop).
async fn materialize_yolo_tree(tmp: &Path, manifest: &ExportManifest) -> Result<(), Response> {
    let images_dir = tmp.join("images");
    let labels_dir = tmp.join("labels");
    tokio::fs::create_dir_all(&images_dir)
        .await
        .map_err(|_| internal())?;
    tokio::fs::create_dir_all(&labels_dir)
        .await
        .map_err(|_| internal())?;

    // Labels (derivados do manifest).
    let label_entries = build_label_entries(&manifest.images);
    for (arcname, content) in &label_entries {
        let filename = arcname.strip_prefix("labels/").unwrap_or(arcname);
        let path = labels_dir.join(filename);
        tokio::fs::write(&path, content)
            .await
            .map_err(|_| internal())?;
    }

    // dataset.yaml (derivado do manifest).
    let yaml = render_dataset_yaml(manifest);
    let yaml_path = tmp.join("dataset.yaml");
    tokio::fs::write(&yaml_path, yaml)
        .await
        .map_err(|_| internal())?;

    Ok(())
}

/// Metadados de cada entrada do zip (para `files[]` do manifest de transporte).
struct ZipFileEntry {
    filename: String,
    md5: String,
    bytes: i64,
}

/// Gera zip do package (Stored para imagens, Deflated para texto).
/// Retorna o caminho do zip + metadados de cada entrada (md5/bytes por arquivo).
async fn generate_package_zip(
    tmp: &Path,
    manifest: &ExportManifest,
) -> Result<(std::path::PathBuf, Vec<ZipFileEntry>), Response> {
    let images_dir = tmp.join("images");
    let labels_dir = tmp.join("labels");
    let mut entries: Vec<ZipEntry> = Vec::new();

    // dataset.yaml
    let yaml_path = tmp.join("dataset.yaml");
    entries.push(ZipEntry {
        arcname: "dataset.yaml".to_string(),
        fs_path: yaml_path.clone(),
        is_text: true,
    });

    // Labels — reutiliza o builder compartilhado com o export (ADR-0007 D1).
    let label_entries = super::export::build_label_entries(&manifest.images);
    for (arcname, _content) in &label_entries {
        let filename = arcname.strip_prefix("labels/").unwrap_or(arcname);
        let path = labels_dir.join(filename);
        entries.push(ZipEntry {
            arcname: arcname.clone(),
            fs_path: path,
            is_text: true,
        });
    }

    // Imagens (já baixadas para o tempdir por materialize_images_from_storage).
    for img in &manifest.images {
        let path = images_dir.join(&img.filename);
        entries.push(ZipEntry {
            arcname: format!("images/{}", img.filename),
            fs_path: path,
            is_text: false,
        });
    }

    // Escreve zip em disco e calcula md5/bytes de cada entrada (streaming p/ imagens).
    let zip_path = tmp.join("dataset.zip");
    let zip_path_clone = zip_path.clone();
    let entries_clone: Vec<(String, std::path::PathBuf, bool)> = entries
        .iter()
        .map(|e| (e.arcname.clone(), e.fs_path.clone(), e.is_text))
        .collect();
    let file_entries = tokio::task::spawn_blocking(move || {
        use md5::Digest;
        write_zip(&entries, &zip_path_clone)?;
        let mut file_entries: Vec<ZipFileEntry> = Vec::new();
        // Calcula md5/bytes de cada entrada (textos pequenos em RAM, imagens streaming 64 KiB).
        for (arcname, fs_path, is_text) in &entries_clone {
            let mut f = std::fs::File::open(fs_path)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            if *is_text {
                let mut content = String::new();
                f.read_to_string(&mut content)?;
                let mut hasher = md5::Md5::new();
                Digest::update(&mut hasher, content.as_bytes());
                let hash = hex::encode(Digest::finalize(hasher));
                file_entries.push(ZipFileEntry {
                    filename: arcname.clone(),
                    md5: hash,
                    bytes: content.len() as i64,
                });
            } else {
                let mut hasher = md5::Md5::new();
                let mut buf = [0u8; 64 * 1024];
                let mut total: i64 = 0;
                loop {
                    let n = f.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    Digest::update(&mut hasher, &buf[..n]);
                    total += n as i64;
                }
                let hash = hex::encode(Digest::finalize(hasher));
                file_entries.push(ZipFileEntry {
                    filename: arcname.clone(),
                    md5: hash,
                    bytes: total,
                });
            }
        }
        Ok::<_, std::io::Error>(file_entries)
    })
    .await
    .map_err(|_| internal())?
    .map_err(|_| internal())?;
    Ok((zip_path, file_entries))
}

// ---------------------------------------------------------------------------
// Download de imagens do storage para o tempdir (ADR-0007 D1: zip autossuficiente).
// ---------------------------------------------------------------------------

/// Baixa imagens do storage para `tmp/images/<filename>`.
/// Diferente do export (3e): fail-closed em blob ausente (503) — o package
/// precisa do conjunto completo para treinar; o export 3e faz skip+eprintln em órfãs.
async fn materialize_images_from_storage(
    state: &AppState,
    tmp: &Path,
    image_rows: &[(Uuid, String, String, i32, i32, String)],
) -> Result<(), Response> {
    let images_dir = tmp.join("images");
    for (_id, filename, object_key, _width, _height, _split) in image_rows {
        let dest = images_dir.join(filename);
        if let Err(e) = state.storage.get_to_file(object_key, &dest).await {
            match e {
                crate::storage::StorageError::NotFound => {
                    return Err(err(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "storage_unavailable",
                        MSG_STORAGE_UNAVAILABLE,
                    ));
                }
                crate::storage::StorageError::Unavailable(_) => {
                    return Err(err(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "storage_unavailable",
                        MSG_STORAGE_UNAVAILABLE,
                    ));
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Handler: POST /api/datasets/:id/package
// ---------------------------------------------------------------------------

/// POST /api/datasets/:id/package — congela snapshot, materializa YOLO, gera
/// zip, faz PUT em packages/<version_id>/.
///
/// Status: 200 | 400 `invalid_request`/`engine_unsupported` | 404 `not_found` |
/// 503 `storage_unavailable`. 401 vem do gate.
pub async fn package_dataset(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    // 1. Parse id (nunca 400 — D8).
    let ds_id: Uuid = match parse_id(&id) {
        Some(v) => v,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };

    // 2. Parse body.
    let req: PackageRequest = match body {
        Ok(b) => match serde_json::from_slice(&b) {
            Ok(v) => v,
            Err(_) => {
                return err(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    MSG_INVALID_REQUEST,
                );
            }
        },
        Err(BytesRejection::FailedToBufferBody(FailedToBufferBody::LengthLimitError(_))) => {
            return err(
                StatusCode::PAYLOAD_TOO_LARGE,
                "invalid_request",
                MSG_INVALID_REQUEST,
            );
        }
        Err(_) => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                MSG_INVALID_REQUEST,
            );
        }
    };

    // 3. Validação: engine único suportado é "yolo".
    if req.engine != "yolo" {
        return err(
            StatusCode::BAD_REQUEST,
            "engine_unsupported",
            MSG_ENGINE_UNSUPPORTED,
        );
    }

    // 4. Dataset existe?
    let ds: Option<(Uuid, String, String, String, String)> = match sqlx::query_as(
        "SELECT id, slug, title, category, type, format FROM datasets WHERE id = $1",
    )
    .bind(ds_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };
    let (_ds_id, slug, _title, category, _format) = match ds {
        Some(r) => r,
        None => return err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND),
    };

    // 5. Classes ordenadas por idx.
    let class_rows: Vec<(Uuid, String, i32)> = match sqlx::query_as(
        "SELECT id, name, idx FROM classes WHERE dataset_id = $1 ORDER BY idx",
    )
    .bind(ds_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };

    // 6. Imagens ATIVAS com object_key (lixeira fora).
    type ImgTuple = (Uuid, String, String, i32, i32, String);
    let image_rows: Vec<ImgTuple> = match sqlx::query_as(
        "SELECT id, filename, object_key, width, height, split FROM images WHERE dataset_id = $1 AND deleted_at IS NULL ORDER BY created_at, id",
    )
    .bind(ds_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };

    // 7. Boxes das imagens ativas.
    type BoxTuple = (Uuid, i32, f64, f64, f64, f64);
    let box_rows: Vec<BoxTuple> = match sqlx::query_as(
        "SELECT b.image_id, c.idx, b.x, b.y, b.w, b.h \
         FROM boxes b \
         JOIN images i ON i.id = b.image_id \
         JOIN classes c ON c.id = b.class_id \
         WHERE i.dataset_id = $1 AND i.deleted_at IS NULL",
    )
    .bind(ds_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return internal(),
    };

    // 8. Monta snapshot manifest.
    let mut boxes_by_image: HashMap<Uuid, Vec<SnapshotBox>> = HashMap::new();
    for (img_id, class_idx, x, y, w, h) in &box_rows {
        boxes_by_image
            .entry(*img_id)
            .or_default()
            .push(SnapshotBox {
                class_idx: *class_idx,
                x: *x,
                y: *y,
                w: *w,
                h: *h,
            });
    }

    let mut labeled = 0usize;
    let snapshot_images: Vec<SnapshotImage> = image_rows
        .iter()
        .map(|(id, filename, _object_key, width, height, split)| {
            let boxes = boxes_by_image.remove(id).unwrap_or_default();
            if !boxes.is_empty() {
                labeled += 1;
            }
            SnapshotImage {
                filename: filename.clone(),
                split: split.clone(),
                width: *width,
                height: *height,
                boxes,
            }
        })
        .collect();

    let snapshot = PackageManifest {
        dataset: SnapshotDataset {
            id: ds_id,
            slug: slug.clone(),
            category,
            engine: "yolo".to_string(),
        },
        classes: class_rows
            .iter()
            .map(|(_id, name, idx)| SnapshotClass {
                idx: *idx,
                name: name.clone(),
            })
            .collect(),
        images: snapshot_images,
        counts: SnapshotCounts {
            images: image_rows.len(),
            labeled,
            classes: class_rows.len(),
        },
    };

    // 9. Gera version_id e manifest_json (usados nos PUTs; INSERT fica no passo 8 abaixo).
    let version_id = Uuid::new_v4();
    let manifest_json = serde_json::to_value(&snapshot).unwrap_or(serde_json::Value::Null);

    // 10. Materializa YOLO em tempdir + baixa imagens do storage + gera zip.
    let tmp = match tempfile::TempDir::new() {
        Ok(d) => d,
        Err(_) => return internal(),
    };
    let export_manifest = snapshot_to_export_manifest(&snapshot);
    if materialize_yolo_tree(tmp.path(), &export_manifest)
        .await
        .is_err()
    {
        return internal();
    }
    // Baixa binários das imagens do storage para o tempdir (fail-closed: 503 se blob ausente).
    if let Err(resp) = materialize_images_from_storage(&state, tmp.path(), &image_rows).await {
        return resp;
    }
    let (zip_path, file_entries) = match generate_package_zip(tmp.path(), &export_manifest).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    // 11. Calcula md5 e bytes do zip.
    let zip_bytes = match tokio::fs::read(&zip_path).await {
        Ok(b) => b,
        Err(_) => return internal(),
    };
    let zip_len = zip_bytes.len() as i64;
    let zip_md5 = {
        use md5::Digest;
        let hash = md5::Md5::digest(&zip_bytes);
        hex::encode(hash)
    };

    // 12–13–14. PUT zip, PUT manifest, INSERT — com compensação best-effort:
    // se PUTs ou INSERT falharem, limpa objetos órfãos no storage.
    let zip_key = format!("packages/{version_id}/dataset.zip");
    if state.storage.put(&zip_key, &zip_path).await.is_err() {
        let _ = state
            .storage
            .delete_prefix(&format!("packages/{version_id}/"))
            .await;
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_unavailable",
            MSG_STORAGE_UNAVAILABLE,
        );
    }

    // Monta manifest.json de transporte (snake_case) e faz PUT.
    // D1: files[] = entradas do zip (md5 + bytes por arquivo; md5_zip = md5 do zip).
    let files_json: Vec<serde_json::Value> = file_entries
        .iter()
        .map(|fe| {
            serde_json::json!({
                "filename": fe.filename,
                "md5": fe.md5,
                "bytes": fe.bytes,
            })
        })
        .collect();
    let transport_manifest = serde_json::json!({
        "dataset_id": ds_id.to_string(),
        "slug": slug,
        "category": snapshot.dataset.category,
        "engine": snapshot.dataset.engine,
        "files": files_json,
        "md5_zip": zip_md5,
        "bytes": zip_len,
        "chunks": null,
        "created_at": Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    });
    let manifest_key = format!("packages/{version_id}/manifest.json");
    let manifest_path = tmp.path().join("manifest.json");
    if tokio::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&transport_manifest).unwrap_or_else(|_| "{}".to_string()),
    )
    .await
    .is_err()
    {
        let _ = state
            .storage
            .delete_prefix(&format!("packages/{version_id}/"))
            .await;
        return internal();
    }
    if state
        .storage
        .put(&manifest_key, &manifest_path)
        .await
        .is_err()
    {
        let _ = state
            .storage
            .delete_prefix(&format!("packages/{version_id}/"))
            .await;
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_unavailable",
            MSG_STORAGE_UNAVAILABLE,
        );
    }

    // INSERT em dataset_versions — só após PUTs bem-sucedidos (doutrina: objeto→linha).
    let inserted =
        sqlx::query("INSERT INTO dataset_versions (id, dataset_id, manifest) VALUES ($1, $2, $3)")
            .bind(version_id)
            .bind(ds_id)
            .bind(&manifest_json)
            .execute(&state.pool)
            .await
            .is_ok();
    if !inserted {
        let _ = state
            .storage
            .delete_prefix(&format!("packages/{version_id}/"))
            .await;
        return internal();
    }

    // 15. Resposta 200 — files[] = entradas do zip (camelCase, D1 linha 175–178).
    let response_files: Vec<TransportFile> = file_entries
        .into_iter()
        .map(|fe| TransportFile {
            filename: fe.filename,
            md5: fe.md5,
            bytes: fe.bytes,
        })
        .collect();
    (
        StatusCode::OK,
        Json(PackageResponse {
            version_id: version_id.to_string(),
            key: zip_key,
            bytes: zip_len,
            md5_zip: zip_md5,
            files: response_files,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_manifest_snake_case() {
        let snap = PackageManifest {
            dataset: SnapshotDataset {
                id: Uuid::nil(),
                slug: "test".to_string(),
                category: "yolo".to_string(),
                engine: "yolo".to_string(),
            },
            classes: vec![],
            images: vec![],
            counts: SnapshotCounts {
                images: 0,
                labeled: 0,
                classes: 0,
            },
        };
        let v = serde_json::to_value(&snap).expect("json");
        assert!(v.get("dataset").is_some());
        assert!(v.get("classes").is_some());
        assert!(v.get("images").is_some());
        assert!(v.get("counts").is_some());
        // snake_case: sem camelCase keys.
        assert!(v.get("datasetId").is_none());
    }

    #[test]
    fn snapshot_to_export_manifest_roundtrip() {
        let snap = PackageManifest {
            dataset: SnapshotDataset {
                id: Uuid::nil(),
                slug: "pcb".to_string(),
                category: "yolo".to_string(),
                engine: "yolo".to_string(),
            },
            classes: vec![SnapshotClass {
                idx: 0,
                name: "solda_fria".to_string(),
            }],
            images: vec![SnapshotImage {
                filename: "img_0001.jpg".to_string(),
                split: "train".to_string(),
                width: 640,
                height: 480,
                boxes: vec![SnapshotBox {
                    class_idx: 0,
                    x: 0.5,
                    y: 0.5,
                    w: 0.2,
                    h: 0.3,
                }],
            }],
            counts: SnapshotCounts {
                images: 1,
                labeled: 1,
                classes: 1,
            },
        };
        let export = snapshot_to_export_manifest(&snap);
        assert_eq!(export.dataset.name, "pcb");
        assert_eq!(export.classes.len(), 1);
        assert_eq!(export.classes[0].name, "solda_fria");
        assert_eq!(export.images.len(), 1);
        assert_eq!(export.images[0].boxes.len(), 1);
        assert_eq!(export.images[0].boxes[0].class_idx, 0);
    }

    #[tokio::test]
    async fn handler_id_nao_uuid_404() {
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
        };
        let resp = package_dataset(
            axum::extract::State(state),
            axum::extract::Path("nao-e-uuid".to_string()),
            Ok(Bytes::from(r#"{"engine":"yolo"}"#)),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn handler_engine_unsupported_sem_banco() {
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
        };
        let resp = package_dataset(
            axum::extract::State(state),
            axum::extract::Path("00000000-0000-0000-0000-000000000000".to_string()),
            Ok(Bytes::from(r#"{"engine":"stable_diffusion"}"#)),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
