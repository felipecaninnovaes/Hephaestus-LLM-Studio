//! Cache de dataset deduplicado por `md5_zip` (Pilar A —
//! `tasks/specs/no-gpu-reuso-dataset-embeds.md` §3A).
//!
//! Layout:
//! - `<workdir>/datasets/datasets-dedup/<md5_zip>/` — entrada compartilhada,
//!   promovida atomicamente via `rename` de `.tmp-<uuid>/` após download +
//!   validação MD5 + extração.
//! - `<workdir>/datasets/datasets-cache/<job_id>/` — visão por job: hardlinks
//!   de arquivos + diretórios novos da entrada dedup (o container do engine
//!   continua recebendo exatamente esse path, sem mudança de contrato).
//!
//! Single-flight: o orchestrator é processo único por nó, então um mutex
//! async em processo (chaveado por `md5_zip`) é suficiente para cobrir
//! download+validação+extração+promoção sem precisar de lock em disco.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex as StdMutex;
use tokio::sync::Mutex as AsyncMutex;

use crate::domain::errors::PipelineError;
use crate::ports::storage::S3Port;

use super::archive::{compute_file_md5, unzip_safe};
use super::create_dir_all_open;

/// Registro de locks de single-flight por `md5_zip`, compartilhado entre
/// jobs via `Arc` (um por processo orchestrator).
#[derive(Default, Clone)]
pub struct DatasetCacheLocks {
    locks: Arc<StdMutex<HashMap<String, Arc<AsyncMutex<()>>>>>,
}

impl DatasetCacheLocks {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock_for(&self, md5: &str) -> Arc<AsyncMutex<()>> {
        let mut map = self.locks.lock();
        map.entry(md5.to_string())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }
}

static GLOBAL_LOCKS: std::sync::LazyLock<DatasetCacheLocks> =
    std::sync::LazyLock::new(DatasetCacheLocks::new);

/// Registro de locks compartilhado pelo processo orchestrator (um por nó —
/// `run_job_inner` usa este singleton; testes isolados instanciam o seu
/// próprio `DatasetCacheLocks::new()` para não colidir entre si).
pub fn global_locks() -> &'static DatasetCacheLocks {
    &GLOBAL_LOCKS
}

fn dedup_root(workdir: &Path) -> PathBuf {
    workdir.join("datasets").join("datasets-dedup")
}

/// Garante que a entrada dedup `datasets-dedup/<md5_zip>/` existe, baixando e
/// extraindo somente em cache miss. Hit: pula download/extração, loga
/// `dataset cache hit` e atualiza o mtime da entrada (base do LRU). Miss:
/// baixa para `temp_dir/dataset.zip`, valida o MD5, extrai para
/// `datasets-dedup/.tmp-<uuid>/` e promove com `rename` atômico.
#[allow(clippy::too_many_arguments)]
pub async fn ensure_dataset_cached(
    locks: &DatasetCacheLocks,
    s3: &Arc<dyn S3Port>,
    workdir: &Path,
    scoped_key: &str,
    expected_md5: &str,
    job_id: &str,
    temp_dir: &Path,
    on_progress: Option<&(dyn Fn(u64, Option<u64>) + Send + Sync)>,
) -> Result<PathBuf, PipelineError> {
    let root = dedup_root(workdir);
    create_dir_all_open(&root).await?;
    let entry_dir = root.join(expected_md5);

    // Single-flight: só uma extração roda por md5_zip mesmo com N jobs
    // concorrentes pedindo o mesmo dataset.
    let lock = locks.lock_for(expected_md5);
    let _guard = lock.lock().await;

    if entry_dir.is_dir() {
        tracing::info!(md5 = %expected_md5, job_id = %job_id, "dataset cache hit");
        touch_mtime(&entry_dir).await;
        return Ok(entry_dir);
    }

    // Miss: baixa para arquivo temporário isolado do job.
    let zip_path = temp_dir.join("dataset.zip");
    s3.get_to_file_with_progress(scoped_key, &zip_path, on_progress)
        .await
        .map_err(|e| PipelineError::S3Download(format!("download package: {e}")))?;

    let actual_md5 = compute_file_md5(&zip_path)
        .map_err(|e| PipelineError::S3Download(format!("compute md5: {e}")))?;
    if actual_md5 != expected_md5 {
        return Err(PipelineError::Md5Mismatch {
            expected: expected_md5.to_string(),
            actual: actual_md5,
        });
    }
    tracing::info!(job_id = %job_id, md5 = %actual_md5, "Integridade do dataset validada com sucesso");

    let tmp_extract = root.join(format!(".tmp-{}", uuid::Uuid::new_v4()));
    create_dir_all_open(&tmp_extract).await?;
    unzip_safe(&zip_path, &tmp_extract)?;

    // Promoção atômica: rename .tmp-<uuid> -> <md5_zip> (mesmo filesystem).
    if let Err(e) = tokio::fs::rename(&tmp_extract, &entry_dir).await {
        // Sob o lock isso só pode colidir com uma promoção manual/externa;
        // best-effort: se a entrada já existe, aceita-a e limpa o tmp.
        if entry_dir.is_dir() {
            let _ = tokio::fs::remove_dir_all(&tmp_extract).await;
        } else {
            let _ = tokio::fs::remove_dir_all(&tmp_extract).await;
            return Err(PipelineError::Other(format!(
                "promote dataset cache entry {expected_md5}: {e}"
            )));
        }
    }

    Ok(entry_dir)
}

/// Atualiza o mtime da entrada dedup (base do LRU de eviction) — equivalente
/// ao `touch` do `os.utime` do lado engine para os embeds.
async fn touch_mtime(path: &Path) {
    let path = path.to_path_buf();
    let _ = tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(&path)?;
        file.set_modified(std::time::SystemTime::now())
    })
    .await;
}

/// Constrói a visão por job (`datasets-cache/<job_id>/`) a partir da entrada
/// dedup: arquivos viram hardlinks (O(1), sem cópia de bytes), diretórios são
/// recriados com `create_dir_all_open` (0o777 — PITFALLS:51, engine roda uid
/// 1000). Implementado em Rust percorrendo a árvore (em vez de `cp -al`
/// subprocess) para manter erros tipados em `PipelineError`, não depender de
/// flags GNU-specific presentes na imagem do orquestrador, e aplicar o modo
/// 0o777 diretamente em cada diretório criado sem depender do umask herdado
/// pelo subprocesso.
pub async fn build_job_view(dedup_entry: &Path, job_view: &Path) -> Result<(), PipelineError> {
    let dedup_entry = dedup_entry.to_path_buf();
    let job_view = job_view.to_path_buf();
    tokio::task::spawn_blocking(move || hardlink_tree_blocking(&dedup_entry, &job_view))
        .await
        .map_err(|e| PipelineError::Other(format!("join hardlink task: {e}")))??;
    Ok(())
}

fn hardlink_tree_blocking(src: &Path, dst: &Path) -> Result<(), PipelineError> {
    create_dir_all_open_sync(dst)?;
    for entry in std::fs::read_dir(src)
        .map_err(|e| PipelineError::Other(format!("read_dir {}: {e}", src.display())))?
    {
        let entry = entry.map_err(|e| PipelineError::Other(format!("read_dir entry: {e}")))?;
        let ty = entry
            .file_type()
            .map_err(|e| PipelineError::Other(format!("file_type: {e}")))?;
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            hardlink_tree_blocking(&entry.path(), &dst_path)?;
        } else if ty.is_file() {
            std::fs::hard_link(entry.path(), &dst_path).map_err(|e| {
                PipelineError::Other(format!("hard_link {}: {e}", entry.path().display()))
            })?;
        }
    }
    Ok(())
}

fn create_dir_all_open_sync(path: &Path) -> Result<(), PipelineError> {
    std::fs::create_dir_all(path)
        .map_err(|e| PipelineError::Other(format!("create {}: {e}", path.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o777))
            .map_err(|e| PipelineError::Other(format!("chmod {}: {e}", path.display())))?;
    }
    Ok(())
}

/// Varre `datasets-dedup/.tmp-*` remanescentes de crash/kill no meio da
/// promoção e remove no boot.
pub async fn sweep_dataset_cache_tmp(workdir: &Path) {
    let root = dedup_root(workdir);
    let mut entries = match tokio::fs::read_dir(&root).await {
        Ok(e) => e,
        Err(_) => return,
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Some(name) = entry.file_name().to_str() {
            if name.starts_with(".tmp-") {
                let _ = tokio::fs::remove_dir_all(entry.path()).await;
            }
        }
    }
}

/// Soma recursiva de bytes de um diretório (best-effort: entradas ilegíveis
/// são ignoradas, não abortam a contagem).
async fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Ok(meta) = entry.metadata().await {
                    if meta.is_dir() {
                        stack.push(entry.path());
                    } else {
                        total += meta.len();
                    }
                }
            }
        }
    }
    total
}

/// Eviction LRU (por mtime) de entradas `datasets-dedup/<md5>/` enquanto o
/// total exceder `max_gb`, pulando qualquer `md5_zip` em `active_md5s`
/// (jobs em execução no nó) mesmo que seja a entrada mais antiga. Retorna a
/// quantidade de entradas removidas.
pub async fn evict_dataset_cache(
    workdir: &Path,
    max_gb: f64,
    active_md5s: &HashSet<String>,
) -> usize {
    let root = dedup_root(workdir);
    let mut entries = match tokio::fs::read_dir(&root).await {
        Ok(e) => e,
        Err(_) => return 0,
    };

    let mut all: Vec<(PathBuf, String, std::time::SystemTime, u64)> = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let meta = match entry.metadata().await {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(".tmp-") {
            continue;
        }
        let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        let size = dir_size(&path).await;
        all.push((path, name, mtime, size));
    }

    let max_bytes = (max_gb * 1024.0 * 1024.0 * 1024.0) as u64;
    let mut total: u64 = all.iter().map(|(_, _, _, s)| *s).sum();
    if total <= max_bytes {
        return 0;
    }

    let mut evictable: Vec<_> = all
        .into_iter()
        .filter(|(_, name, _, _)| !active_md5s.contains(name))
        .collect();
    evictable.sort_by_key(|(_, _, mtime, _)| *mtime);

    let mut removed = 0usize;
    for (path, name, _, size) in evictable {
        if total <= max_bytes {
            break;
        }
        if tokio::fs::remove_dir_all(&path).await.is_ok() {
            total = total.saturating_sub(size);
            removed += 1;
            tracing::info!(md5 = %name, "dataset cache entry evicted (LRU)");
        }
    }
    removed
}
