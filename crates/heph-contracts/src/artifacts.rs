use serde::{Deserialize, Serialize};

/// Artefato emitido por uma execução e reportado ao manager/BFF.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactItem {
    pub kind: String,
    pub path: String,
    pub md5: String,
    pub bytes: i64,
}

/// Alias de compatibilidade com o orchestrator.
pub type ArtifactReport = ArtifactItem;
