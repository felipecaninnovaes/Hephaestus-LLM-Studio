//! MD5 helper e descompactação zip segura (zip-slip protection).

use std::path::Path;

use crate::domain::errors::PipelineError;

/// Calcula MD5 hex de um arquivo usando streaming em chunks de 64 KiB.
pub fn compute_file_md5(path: &Path) -> Result<String, String> {
    use md5::Digest;
    use std::io::Read;

    let mut file =
        std::fs::File::open(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut hasher = md5::Md5::new();
    let mut buffer = [0u8; 64 * 1024];

    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| format!("read {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(hex::encode(hasher.finalize()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn compute_file_md5_large_file_streaming() {
        use md5::Digest;
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("large.bin");

        // 256 KiB (> 4x o buffer de 64 KiB) de dados pseudo-aleatórios determinísticos
        let mut data = Vec::with_capacity(256 * 1024);
        for i in 0..(256 * 1024) {
            data.push((i % 251) as u8);
        }
        std::fs::File::create(&file_path)
            .unwrap()
            .write_all(&data)
            .unwrap();

        let expected_digest = md5::Md5::digest(&data);
        let expected_md5 = hex::encode(expected_digest);

        let actual_md5 = compute_file_md5(&file_path).expect("md5 should succeed");
        assert_eq!(actual_md5, expected_md5);
    }
}
