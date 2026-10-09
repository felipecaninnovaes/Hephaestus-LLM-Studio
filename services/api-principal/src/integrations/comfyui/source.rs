//! Resolve a origem do envio (modelo do catálogo ou artefato de job) para
//! chave S3, nome remoto, tamanho registrado e arquitetura.

use sqlx::PgPool;
use uuid::Uuid;

use super::repository;
use super::types::ExportSourceRef;
use crate::jobs::manager_client::{ManagerError, ManagerPort};

#[derive(Debug, PartialEq)]
pub struct ResolvedSource {
    pub model_id: Option<Uuid>,
    pub job_id: Option<Uuid>,
    pub artifact_id: Option<Uuid>,
    pub key: String,
    pub filename: String,
    pub bytes: i64,
    pub arch: Option<String>,
    /// Alpha de treino do job de origem (fallback quando o header não traz `lora_alpha`).
    pub alpha: Option<f64>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ResolveError {
    NotFound,
    /// Existe, mas não é um LoRA `.safetensors` exportável (422).
    Unsupported,
    Unavailable,
}

/// Nome aceito pelo custom node: `[A-Za-z0-9._-]`, ≤ 200, termina em
/// `.safetensors`, sem `..`.
pub fn remote_filename(raw: &str) -> String {
    const EXT: &str = ".safetensors";
    let base = raw.rsplit('/').next().unwrap_or(raw);
    let stem = base.strip_suffix(EXT).unwrap_or(base);
    // Uma passada: caractere fora do conjunto vira `_`; `.` só entra se o
    // anterior não for `.` (nada de `..`); `.` no fim sai (evita `..safetensors`).
    let mut clean = String::with_capacity(stem.len().min(200));
    for c in stem.chars() {
        if clean.len() >= 200 - EXT.len() {
            break;
        }
        let c = if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            c
        } else {
            '_'
        };
        if c == '.' && clean.ends_with('.') {
            continue;
        }
        clean.push(c);
    }
    let clean = clean.trim_end_matches('.');
    let clean = if clean.is_empty() { "lora" } else { clean };
    format!("{clean}{EXT}")
}

/// Slug canônico de arquitetura a partir de `models.arch`/`jobs.model`.
pub fn canonical_arch(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "sdxl" => Some("sdxl"),
        "sd15" | "sd1.5" | "sd_15" | "sd-15" | "sd 15" => Some("sd15"),
        "flux-2-klein-9b" | "flux2-klein-9b" => Some("flux-2-klein-9b"),
        "flux" | "flux2" | "flux-2-klein" | "flux-2-klein-4b" | "flux2-klein-4b" => {
            Some("flux-2-klein-4b")
        }
        "qwen" | "qwen-image" | "qwen-image-2.1" | "qwen2.1" | "qwen_image" | "qwen-image-2-1" => {
            Some("qwen-image-2.1")
        }
        _ => None,
    }
}

/// Alpha de treino em `jobs.params` (`alpha`, `lora_alpha` ou `loraAlpha`).
fn job_alpha(job: &crate::jobs::manager_client::InternalJob) -> Option<f64> {
    let params = job.params.as_ref()?;
    ["alpha", "lora_alpha", "loraAlpha"]
        .iter()
        .find_map(|k| params.get(k))
        .and_then(|v| v.as_f64().or_else(|| v.as_str()?.trim().parse().ok()))
}

fn is_lora_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".safetensors") && (lower.contains("lora") || lower.contains("adapter"))
}

pub async fn resolve(
    pool: &PgPool,
    manager: &dyn ManagerPort,
    src: &ExportSourceRef,
) -> Result<ResolvedSource, ResolveError> {
    match src {
        ExportSourceRef::Model(id) => resolve_model(pool, manager, *id).await,
        ExportSourceRef::Artifact {
            job_id,
            artifact_id,
        } => resolve_artifact(pool, manager, *job_id, *artifact_id).await,
    }
}

async fn resolve_model(
    pool: &PgPool,
    manager: &dyn ManagerPort,
    id: Uuid,
) -> Result<ResolvedSource, ResolveError> {
    let m = repository::get_model(pool, id)
        .await
        .map_err(|_| ResolveError::Unavailable)?
        .ok_or(ResolveError::NotFound)?;
    if m.kind.as_deref() != Some("lora") || !m.s3_key.ends_with(".safetensors") {
        return Err(ResolveError::Unsupported);
    }
    // Best-effort: sem o job (apagado/manager fora) segue sem alpha/arch derivados.
    let job = match m.job_id {
        Some(j) => manager.get_job(&j.to_string()).await.ok(),
        None => None,
    };
    let arch = m
        .arch
        .as_deref()
        .and_then(canonical_arch)
        .or_else(|| job.as_ref().and_then(|j| canonical_arch(&j.model)));
    Ok(ResolvedSource {
        model_id: Some(id),
        job_id: None,
        artifact_id: None,
        filename: remote_filename(&m.name),
        arch: arch.map(str::to_string),
        alpha: job.as_ref().and_then(job_alpha),
        key: m.s3_key,
        bytes: m.bytes,
    })
}

async fn resolve_artifact(
    pool: &PgPool,
    manager: &dyn ManagerPort,
    job_id: Uuid,
    artifact_id: Uuid,
) -> Result<ResolvedSource, ResolveError> {
    let jid = job_id.to_string();
    let map_err = |e: ManagerError| match e {
        ManagerError::NotFound => ResolveError::NotFound,
        _ => ResolveError::Unavailable,
    };
    let arts = manager.list_artifacts(&jid).await.map_err(map_err)?;
    let aid = artifact_id.to_string();
    let art = arts
        .iter()
        .find(|a| a.id == aid)
        .ok_or(ResolveError::NotFound)?;
    if art.path.contains("..") || art.path.starts_with('/') || art.path.starts_with('\\') {
        return Err(ResolveError::NotFound);
    }
    let job = manager.get_job(&jid).await.map_err(map_err)?;
    if job.engine != "diffusion" || !is_lora_path(&art.path) {
        return Err(ResolveError::Unsupported);
    }
    let key = format!("artifacts/{jid}/{}", art.path);
    let model = repository::get_model_by_key(pool, &key)
        .await
        .map_err(|_| ResolveError::Unavailable)?;
    let arch = model
        .as_ref()
        .and_then(|m| m.arch.as_deref())
        .and_then(canonical_arch)
        .or_else(|| canonical_arch(&job.model))
        .map(str::to_string);
    let name = model.map(|m| m.name).unwrap_or_else(|| art.path.clone());
    Ok(ResolvedSource {
        model_id: None,
        job_id: Some(job_id),
        artifact_id: Some(artifact_id),
        filename: remote_filename(&name),
        key,
        bytes: art.bytes,
        arch,
        alpha: job_alpha(&job),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nome_remoto_e_sempre_aceito_pelo_custom_node() {
        assert_eq!(
            remote_filename("meu lora v1.safetensors"),
            "meu_lora_v1.safetensors"
        );
        assert_eq!(
            remote_filename("outputs/x/base_epoch_003.safetensors"),
            "base_epoch_003.safetensors"
        );
        assert_eq!(remote_filename("a..b"), "a.b.safetensors");
        assert_eq!(remote_filename("çãõ"), "___.safetensors");
        assert_eq!(remote_filename(""), "lora.safetensors");
        let long = remote_filename(&"x".repeat(500));
        assert_eq!(long.len(), 200);
        assert!(long.ends_with(".safetensors"));
    }

    #[test]
    fn nome_remoto_nunca_gera_ponto_duplo_nem_termina_em_ponto() {
        let mut cases = vec![
            "modelo.".to_string(),
            "modelo...".to_string(),
            "modelo.safetensors.".to_string(),
            "a....b".to_string(),
            "...".to_string(),
            ".safetensors".to_string(),
            "..safetensors".to_string(),
            "名前のlora é ótimo".to_string(),
            "x.".repeat(300),
            format!("{}.", "y".repeat(187)),
            "z".repeat(500),
        ];
        cases.extend(["", "/", "a/b/.."].map(String::from));
        for raw in cases {
            let n = remote_filename(&raw);
            assert!(n.ends_with(".safetensors"), "{raw:?} -> {n}");
            assert!(n.len() <= 200, "{raw:?} -> {n}");
            assert!(!n.contains(".."), "{raw:?} -> {n}");
            assert!(!n.starts_with('.') || n.len() > 12, "{raw:?} -> {n}");
            assert!(
                n.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')),
                "{raw:?} -> {n}"
            );
        }
        assert_eq!(remote_filename("modelo."), "modelo.safetensors");
        assert_eq!(remote_filename("..."), "lora.safetensors");
        assert_eq!(remote_filename("名前"), "__.safetensors");
    }

    #[test]
    fn arch_canonica_aceita_aliases() {
        assert_eq!(canonical_arch("flux"), Some("flux-2-klein-4b"));
        assert_eq!(canonical_arch("FLUX-2-KLEIN-9B"), Some("flux-2-klein-9b"));
        assert_eq!(canonical_arch("yolov8"), None);
    }
}
