//! Manifesto de upload e purga de `outputs/<job_id>/` (Pilar C —
//! `tasks/specs/no-gpu-reuso-dataset-embeds.md` §3C).
//!
//! - Manifesto (`outputs/<job_id>/.heph-uploaded`): uma linha por path
//!   relativo, append-only, gravada SOMENTE após upload confirmado ao S3
//!   (intermediário ou final). Falha segura: path ausente do manifesto nunca
//!   é purgado.
//! - Marcador terminal (`outputs/<job_id>/.heph-terminal`): gravado quando o
//!   job atinge um estado terminal neste nó (done/failed/cancelled),
//!   inclusive nos caminhos de erro. Jobs sem marcador NUNCA são purgados.
//! - Sweeper: jobs com `.heph-terminal` mais antigo que `ttl` têm os arquivos
//!   listados no manifesto + `text_embeds_cache/` + `weights/` removidos.
//!   `config.yaml`, `metrics.jsonl`, `logs/telemetry.jsonl`, os dois
//!   marcadores e qualquer arquivo fora do manifesto são sempre mantidos.

use std::path::Path;

pub const MANIFEST_FILENAME: &str = ".heph-uploaded";
pub const TERMINAL_MARKER_FILENAME: &str = ".heph-terminal";

/// Paths sempre preservados na purga, relativos a `outputs/<job_id>/`.
const KEEPERS: &[&str] = &[
    "config.yaml",
    "metrics.jsonl",
    "logs/telemetry.jsonl",
    MANIFEST_FILENAME,
    TERMINAL_MARKER_FILENAME,
];

/// Diretórios de staging sempre removidos na purga (além do manifesto).
const PURGE_STAGING_DIRS: &[&str] = &["text_embeds_cache", "weights"];

/// Valida que `rel_path` permanece dentro do diretório do job: recusa `..`
/// e paths absolutos. Usado tanto ao gravar quanto ao ler o manifesto.
fn is_safe_relative(rel_path: &str) -> bool {
    if rel_path.is_empty() {
        return false;
    }
    let path = Path::new(rel_path);
    if path.is_absolute() {
        return false;
    }
    !path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
}

/// Registra `rel_path` no manifesto do job (append-only), APÓS upload
/// confirmado ao S3. Rejeita silenciosamente (loga warn) paths fora do
/// diretório do job — nunca escreve `..`/absoluto no manifesto.
pub async fn append_manifest(job_outputs_dir: &Path, rel_path: &str) {
    if !is_safe_relative(rel_path) {
        tracing::warn!(
            rel_path = %rel_path,
            "manifesto de upload recusou path inseguro (fora do diretório do job)"
        );
        return;
    }
    let manifest_path = job_outputs_dir.join(MANIFEST_FILENAME);
    use tokio::io::AsyncWriteExt;
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&manifest_path)
        .await;
    match file {
        Ok(mut f) => {
            let line = format!("{rel_path}\n");
            if let Err(e) = f.write_all(line.as_bytes()).await {
                tracing::warn!(job_dir = %job_outputs_dir.display(), error = %e, "falha ao gravar manifesto de upload");
            }
        }
        Err(e) => {
            tracing::warn!(job_dir = %job_outputs_dir.display(), error = %e, "falha ao abrir manifesto de upload");
        }
    }
}

/// Grava o marcador terminal (`.heph-terminal`) — idempotente, best-effort.
/// Chamado em TODO caminho terminal do job neste nó (done/failed/cancelled),
/// inclusive erros antes do upload de qualquer artefato.
pub async fn write_terminal_marker(job_outputs_dir: &Path) {
    let marker_path = job_outputs_dir.join(TERMINAL_MARKER_FILENAME);
    if let Err(e) = tokio::fs::write(&marker_path, b"").await {
        tracing::warn!(job_dir = %job_outputs_dir.display(), error = %e, "falha ao gravar marcador terminal");
    }
}

/// Lê o manifesto do job, retornando os paths relativos válidos (já
/// filtrados por `is_safe_relative` — defesa em profundidade contra um
/// manifesto corrompido/editado manualmente).
async fn read_manifest(job_outputs_dir: &Path) -> Vec<String> {
    let manifest_path = job_outputs_dir.join(MANIFEST_FILENAME);
    match tokio::fs::read_to_string(&manifest_path).await {
        Ok(content) => content
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .filter(|l| is_safe_relative(l))
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Purga um único job dir se seu marcador terminal for mais antigo que
/// `ttl`. Retorna `true` se a purga rodou (independente de ter removido
/// algo). Jobs sem marcador terminal nunca são tocados.
async fn purge_job_dir_if_due(job_dir: &Path, ttl: std::time::Duration) -> bool {
    let marker_path = job_dir.join(TERMINAL_MARKER_FILENAME);
    let meta = match tokio::fs::metadata(&marker_path).await {
        Ok(m) => m,
        Err(_) => return false, // sem marcador terminal → nunca purga
    };
    let mtime = match meta.modified() {
        Ok(t) => t,
        Err(_) => return false,
    };
    let age = match mtime.elapsed() {
        Ok(a) => a,
        Err(_) => return false, // mtime no futuro (clock skew) → não purga ainda
    };
    if age < ttl {
        return false;
    }

    // Remove arquivos listados no manifesto (paths já validados), exceto os
    // KEEPERS — defesa em profundidade caso um deles seja listado por erro.
    for rel_path in read_manifest(job_dir).await {
        if KEEPERS.contains(&rel_path.as_str()) {
            continue;
        }
        let target = job_dir.join(&rel_path);
        let _ = tokio::fs::remove_file(&target).await;
    }

    // Remove staging pesado sempre (independente do manifesto).
    for staging in PURGE_STAGING_DIRS {
        let target = job_dir.join(staging);
        let _ = tokio::fs::remove_dir_all(&target).await;
    }

    true
}

/// Varre `<workdir>/outputs/` e purga todo job cujo `.heph-terminal` tenha
/// idade >= `ttl`. Mantém sempre os `KEEPERS` e qualquer arquivo nunca
/// listado no manifesto (ignorados por não estarem no manifesto). Retorna a
/// quantidade de jobs purgados.
pub async fn sweep_output_purge(workdir: &Path, ttl: std::time::Duration) -> usize {
    let outputs_root = workdir.join("outputs");
    let mut entries = match tokio::fs::read_dir(&outputs_root).await {
        Ok(e) => e,
        Err(_) => return 0,
    };

    let mut purged = 0usize;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let is_dir = entry.metadata().await.map(|m| m.is_dir()).unwrap_or(false);
        if !is_dir {
            continue;
        }
        // `.text_embeds_cache` é compartilhado entre jobs (Pilar B), não um
        // dir de job — nunca passa pela purga por job.
        if path.file_name().and_then(|n| n.to_str()) == Some(".text_embeds_cache") {
            continue;
        }
        if purge_job_dir_if_due(&path, ttl).await {
            purged += 1;
            tracing::info!(job_dir = %path.display(), "outputs purgados (pós-TTL terminal)");
        }
    }
    purged
}

/// Export de utilitário para testes externos que precisem validar a função
/// de sanitização isoladamente.
#[cfg(test)]
pub(crate) fn is_safe_relative_for_test(rel_path: &str) -> bool {
    is_safe_relative(rel_path)
}
