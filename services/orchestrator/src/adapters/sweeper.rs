use std::path::Path;

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
