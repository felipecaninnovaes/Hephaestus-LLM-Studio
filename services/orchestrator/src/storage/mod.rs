//! Camada de storage do orchestrator (Fatia 5): chaves S3 escopadas,
//! arquivo seguro (md5/zip), cliente S3 real e cache local de pesos.
//!
//! Compatibilidade pública preservada via re-exports em `crate::lib`.

pub mod archive;
pub mod cache;
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

pub use archive::*;
pub use cache::*;
pub use s3::*;
pub use scope::*;
