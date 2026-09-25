use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::ActiveJobs;

/// Helper para obter a idade em segundos de um container via `docker inspect` (§P2-2).
async fn container_age_secs(id: &str) -> Option<u64> {
    let output = tokio::process::Command::new("docker")
        .args(["inspect", "--format", "{{.Created}}", id])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let created_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let dt = aws_smithy_types::date_time::DateTime::from_str(
        &created_str,
        aws_smithy_types::date_time::Format::DateTime,
    )
    .ok()?;

    let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();

    let created_secs = dt.secs().max(0) as u64;
    Some(now_secs.saturating_sub(created_secs))
}

/// Verifica se um container pertence a algum job ativo em `active_jobs`.
pub fn is_container_active(active_jobs: &ActiveJobs, name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() {
        return false;
    }
    active_jobs.iter().any(|entry| {
        let job_id = entry.key();
        let val = entry.value();
        (!val.container_name.is_empty()
            && (val.container_name == name
                || val.container_name.contains(name)
                || name.contains(&val.container_name)))
            || (!job_id.is_empty() && name.contains(job_id))
    })
}

/// Reconcilia e encerra containers órfãos (`trainer-*`) que não pertencem a jobs ativos.
/// Retorna a quantidade de containers órfãos removidos (§P2-2).
pub async fn reconcile_orphan_containers(active_jobs: &ActiveJobs) -> usize {
    tracing::debug!("reconciliando containers de treino com jobs ativos...");
    let output = tokio::process::Command::new("docker")
        .args([
            "ps",
            "--filter",
            "name=trainer-",
            "--format",
            "{{.ID}}\t{{.Names}}",
        ])
        .output()
        .await;

    let mut removed_count = 0;

    match output {
        Ok(out) if out.status.success() => {
            let container_output = String::from_utf8_lossy(&out.stdout);
            for line in container_output.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let mut parts = line.split('\t');
                let id = parts.next().unwrap_or("").trim();
                let name = parts.next().unwrap_or("").trim();
                if id.is_empty() {
                    continue;
                }

                let name_to_check = if name.is_empty() { id } else { name };
                if !is_container_active(active_jobs, name_to_check) {
                    // Proteção contra race condition: ignora containers criados há menos de 5 minutos (300s)
                    if let Some(age_secs) = container_age_secs(id).await {
                        if age_secs < 300 {
                            tracing::debug!(
                                "reaper: container {name_to_check} ({id}) criado há apenas {age_secs}s (< 300s); aguardando maturidade"
                            );
                            continue;
                        }
                    }

                    // Tenta parada graciosa com tolerância de 5s antes da remoção forçada
                    let _ = tokio::process::Command::new("docker")
                        .args(["stop", "--time", "5", id])
                        .output()
                        .await;

                    let rm_res = tokio::process::Command::new("docker")
                        .args(["rm", "-f", id])
                        .output()
                        .await;

                    match rm_res {
                        Ok(rm_out) if rm_out.status.success() => {
                            removed_count += 1;
                            tracing::warn!(
                                "reaper: removido container órfão {name_to_check} ({id})"
                            );
                        }
                        Ok(rm_out) => {
                            tracing::error!(
                                "reaper: falha ao remover container órfão {name_to_check} ({id}): {}",
                                String::from_utf8_lossy(&rm_out.stderr)
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                "reaper: falha ao executar docker rm para {name_to_check} ({id}): {e}"
                            );
                        }
                    }
                }
            }
        }
        Ok(out) => {
            tracing::warn!(
                "docker ps retornou erro ao verificar órfãos: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Err(e) => {
            tracing::warn!("falha ao executar docker ps para checar órfãos: {e}");
        }
    }

    removed_count
}

/// Spawna um sweeper periódico para reconciliar containers órfãos e limpar workdirs antigos (§P2-2).
pub fn spawn_periodic_sweeper(
    active_jobs: ActiveJobs,
    workdir: PathBuf,
    interval: Duration,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if *shutdown_rx.borrow() {
            return;
        }
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let _ = reconcile_orphan_containers(&active_jobs).await;
                    sweep_orphan_workdirs(&workdir, Duration::from_secs(24 * 3600)).await;
                }
                res = shutdown_rx.changed() => {
                    if res.is_err() || *shutdown_rx.borrow() {
                        tracing::info!("sweeper worker: shutdown signal recebido, encerrando");
                        break;
                    }
                }
            }
        }
    })
}

/// Varre e encerra containers órfãos de treino (`trainer-*`) no boot do orquestrador.
pub async fn sweep_orphan_trainer_containers() {
    tracing::info!("verificando containers órfãos de treino no boot...");
    let output = tokio::process::Command::new("docker")
        .args(["ps", "-q", "--filter", "name=trainer-"])
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => {
            let container_ids = String::from_utf8_lossy(&out.stdout);
            let ids: Vec<&str> = container_ids
                .lines()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();
            if ids.is_empty() {
                tracing::info!("nenhum container órfão encontrado no boot");
            } else {
                tracing::warn!(
                    "encontrados {} containers órfãos: {:?}. Encerrando...",
                    ids.len(),
                    ids
                );
                for id in ids {
                    let _ = tokio::process::Command::new("docker")
                        .args(["stop", "--time", "5", id])
                        .output()
                        .await;
                    let stop_res = tokio::process::Command::new("docker")
                        .args(["rm", "-f", id])
                        .output()
                        .await;
                    if let Err(e) = stop_res {
                        tracing::error!("falha ao remover container órfão {id}: {e}");
                    } else {
                        tracing::info!("container órfão {id} removido com sucesso");
                    }
                }
            }
        }
        Ok(out) => {
            tracing::warn!(
                "docker ps retornou erro ao verificar órfãos: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Err(e) => {
            tracing::warn!("falha ao executar docker ps para checar órfãos: {e}");
        }
    }
}

/// Varre e limpa diretórios antigos de cache de datasets no workdir (> 24h).
pub async fn sweep_orphan_workdirs(workdir: &Path, max_age: std::time::Duration) {
    let cache_dir = workdir.join("datasets").join("datasets-cache");
    if let Ok(mut entries) = tokio::fs::read_dir(&cache_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(meta) = entry.metadata().await {
                if let Ok(modified) = meta.modified() {
                    if let Ok(age) = modified.elapsed() {
                        if age > max_age {
                            let _ = tokio::fs::remove_dir_all(entry.path()).await;
                        }
                    }
                }
            }
        }
    }
}
