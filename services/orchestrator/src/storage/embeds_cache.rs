//! Eviction LRU do cache compartilhado de text-embeds (Pilar B —
//! `tasks/specs/no-gpu-reuso-dataset-embeds.md` §3B).
//!
//! A escrita/leitura dos `.pt` é responsabilidade da engine (trainer-difusao,
//! via `TEXT_EMBEDS_CACHE_DIR`); o orchestrator só cria o diretório raiz no
//! boot (`create_dir_all_open`, PITFALLS:51), repassa a env ao container e
//! roda a eviction LRU por orçamento aqui.

use std::path::{Path, PathBuf};

fn embeds_root(outputs_root: &Path) -> PathBuf {
    outputs_root.join(".text_embeds_cache")
}

/// Soma recursiva dos bytes de todo `.pt` sob `root` mais a lista de
/// candidatos `(path, mtime, bytes)` ordenável por LRU.
async fn collect_pt_files(root: &Path) -> Vec<(PathBuf, std::time::SystemTime, u64)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(e) => e,
            Err(_) => continue,
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let meta = match entry.metadata().await {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("pt") {
                let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                out.push((path, mtime, meta.len()));
            }
        }
    }
    out
}

/// Eviction LRU (por mtime, atualizado pela engine via `os.utime` em cada
/// cache hit) de arquivos `.pt` sob `<outputs>/.text_embeds_cache/` enquanto
/// o total exceder `max_gb`. Remove o diretório de namespace quando ele fica
/// vazio após a remoção (preserva `namespace.json` enquanto houver qualquer
/// `.pt` no namespace). Retorna a quantidade de arquivos removidos.
pub async fn evict_text_embeds_cache(outputs_root: &Path, max_gb: f64) -> usize {
    let root = embeds_root(outputs_root);
    let mut files = collect_pt_files(&root).await;
    if files.is_empty() {
        return 0;
    }

    let max_bytes = (max_gb * 1024.0 * 1024.0 * 1024.0) as u64;
    let mut total: u64 = files.iter().map(|(_, _, s)| *s).sum();
    if total <= max_bytes {
        return 0;
    }

    files.sort_by_key(|(_, mtime, _)| *mtime);

    let mut removed = 0usize;
    let mut touched_dirs: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for (path, _, size) in files {
        if total <= max_bytes {
            break;
        }
        if let Some(parent) = path.parent() {
            touched_dirs.insert(parent.to_path_buf());
        }
        if tokio::fs::remove_file(&path).await.is_ok() {
            total = total.saturating_sub(size);
            removed += 1;
            tracing::info!(path = %path.display(), "text embeds cache entry evicted (LRU)");
        }
    }

    // Remove diretórios de namespace que ficaram vazios (sem nenhum .pt
    // remanescente) — inclusive `namespace.json`, pois sem embeds não há
    // mais auditoria útil a preservar.
    for dir in touched_dirs {
        if dir == root {
            continue;
        }
        let mut has_pt = false;
        if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
            while let Ok(Some(e)) = entries.next_entry().await {
                if e.path().extension().and_then(|x| x.to_str()) == Some("pt") {
                    has_pt = true;
                    break;
                }
            }
        }
        if !has_pt {
            let _ = tokio::fs::remove_dir_all(&dir).await;
        }
    }

    removed
}
