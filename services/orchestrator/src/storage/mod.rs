//! Camada de storage do orchestrator (Fatia 5): chaves S3 escopadas,
//! arquivo seguro (md5/zip), cliente S3 real e cache local de pesos.
//!
//! Compatibilidade pública preservada via re-exports em `crate::lib`.

pub mod archive;
pub mod cache;
pub mod dataset_cache;
pub mod embeds_cache;
pub mod output_purge;
pub mod s3;
pub mod scope;

use std::path::Path;

use crate::domain::errors::PipelineError;

/// Cria `path` recursivamente e garante modo 0o777 no diretório resultante.
///
/// Invariant cross-container: engines GPU rodam como uid 1000 (`USER studio`
/// nas imagens) enquanto o orquestrador roda como root. Um diretório criado
/// por root nasce 0755 (umask 022) e o 0777 da raiz do dataset compartilhado
/// NÃO se propaga a filhos — o engine receberia EACCES ao gravar telemetria
/// e artefatos no próprio diretório de job (ver docs/PITFALLS.md, infra).
pub async fn create_dir_all_open(path: &Path) -> Result<(), PipelineError> {
    tokio::fs::create_dir_all(path)
        .await
        .map_err(|e| PipelineError::Other(format!("create {}: {e}", path.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o777))
            .await
            .map_err(|e| PipelineError::Other(format!("chmod {}: {e}", path.display())))?;
    }
    Ok(())
}

/// Lista de diretórios compartilhados no volume `outputs` que precisam existir
/// e ter permissões abertas (0o777 em cada nível) para engines rodando como uid 1000.
///
/// Consumidores:
/// - `.text_embeds_cache`: cache de text embeddings compartilhado (pilar B, PITFALLS:51)
/// - `.cache/huggingface`: `HF_HOME` (`services/orchestrator/src/app/mod.rs:1022`, daemon `main.rs:148`)
/// - `.cache/huggingface/hub`: `HF_HUB_CACHE`, `TRANSFORMERS_CACHE`, `DIFFUSERS_CACHE` (`app/mod.rs:1026-1035`, daemon `main.rs:152-161`)
/// - `.cache/torch`: `TORCH_HOME` (`app/mod.rs:1038`, daemon `main.rs:164`)
/// - `.cache/quantized`: cache de quantização do loader de difusão (`engines/trainer-difusao/src/trainer_difusao/loaders/quant_cache.py:87-91`)
pub const SHARED_ENGINE_CACHE_DIRS: &[&str] = &[
    ".text_embeds_cache",
    ".cache",
    ".cache/huggingface",
    ".cache/huggingface/hub",
    ".cache/torch",
    ".cache/quantized",
];

/// Garante 0o777 em cada nível dos diretórios compartilhados do engine sob `outputs_root`.
///
/// Invariant cross-container: engines rodam como uid 1000 (`USER studio`) enquanto
/// o orquestrador roda como root. Se `.cache` for criado pelo root com umask 022
/// (0755), ou já existir de volume anterior sem permissão de escrita global, o engine
/// recebe `[Errno 13] Permission denied: '/outputs/.cache'` ao inicializar HF/Torch/quantized.
///
/// Esta função cria cada nível se faltar e aplica chmod 0o777 em todos os níveis,
/// inclusive quando o diretório já existia com 0755.
pub async fn ensure_shared_engine_dirs(outputs_root: &Path) -> Result<(), PipelineError> {
    for rel in SHARED_ENGINE_CACHE_DIRS {
        let dir = outputs_root.join(rel);
        create_dir_all_open(&dir).await?;
    }
    Ok(())
}

pub use archive::*;
pub use cache::*;
pub use dataset_cache::*;
pub use embeds_cache::*;
pub use output_purge::*;
pub use s3::*;
pub use scope::*;
