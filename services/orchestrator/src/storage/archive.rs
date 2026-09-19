//! MD5 helper e descompactação zip segura (zip-slip protection).

use std::path::Path;

use crate::domain::errors::PipelineError;

/// Calcula MD5 hex de um arquivo.
pub fn compute_file_md5(path: &Path) -> Result<String, String> {
    use md5::Digest;
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let digest = md5::Md5::digest(&bytes);
    Ok(hex::encode(digest))
}

/// Descompacta um zip em `dest`, recusando entradas com `..` ou caminhos absolutos.
pub fn unzip_safe(zip_path: &Path, dest: &Path) -> Result<(), PipelineError> {
    let file = std::fs::File::open(zip_path)
        .map_err(|e| PipelineError::UnzipFailed(format!("open zip: {e}")))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| PipelineError::UnzipFailed(format!("read zip: {e}")))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| PipelineError::UnzipFailed(format!("read entry: {e}")))?;

        let entry_name = entry.name().to_string();

        // Zip-slip protection (padrão import 3e)
        if entry_name.contains("..") || entry_name.starts_with('/') {
            return Err(PipelineError::UnzipFailed(format!(
                "unsafe zip entry: {entry_name}"
            )));
        }

        let out_path = dest.join(&entry_name);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)
                .map_err(|e| PipelineError::UnzipFailed(format!("create dir: {e}")))?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| PipelineError::UnzipFailed(format!("create parent: {e}")))?;
            }
            let mut out_file = std::fs::File::create(&out_path)
                .map_err(|e| PipelineError::UnzipFailed(format!("create file: {e}")))?;
            std::io::copy(&mut entry, &mut out_file)
                .map_err(|e| PipelineError::UnzipFailed(format!("write file: {e}")))?;
        }
    }

    Ok(())
}
