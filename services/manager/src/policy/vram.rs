//! Políticas e resolução de VRAM mínima e headroom por engine/modelo/modo (MM-08).

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct VramTable {
    pub defaults: VramDefaults,
    pub entries: Vec<VramEntry>,
    #[serde(default)]
    pub features: Vec<VramFeature>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VramFeature {
    pub engine: String,
    pub feature: String,
    #[serde(default)]
    pub default: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VramDefaults {
    pub headroom_gb: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VramEntry {
    pub engine: String,
    pub model: String,
    pub mode: String,
    pub vram_min_gb: i32,
}

impl VramTable {
    /// Resolve o requisito VRAM para um job: vram_min_gb + headroom.
    /// Entrada faltante ⇒ None (permissivo).
    pub fn resolve_required_gb(&self, engine: &str, model: &str, mode: &str) -> Option<i32> {
        self.entries
            .iter()
            .find(|e| e.engine == engine && e.model == model && e.mode == mode)
            .map(|e| e.vram_min_gb + self.defaults.headroom_gb)
    }

    /// Parse a partir de string YAML. Fail-fast se inválido.
    pub fn parse(yaml: &str) -> Result<Self, String> {
        serde_yaml::from_str(yaml).map_err(|e| format!("vram-table parse: {e}"))
    }

    /// Carrega a tabela a partir de um arquivo em disco ou do fallback embutido no binário.
    pub fn load_or_default(path: Option<&str>) -> Result<Self, String> {
        let raw = match path {
            Some(p) => std::fs::read_to_string(p)
                .map_err(|e| format!("falha ao ler VRAM_TABLE_PATH={p}: {e}"))?,
            None => include_str!("../../../../packages/policies/vram-table.yaml").to_string(),
        };
        Self::parse(&raw)
    }
}
