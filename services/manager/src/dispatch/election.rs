//! Eleição e seleção de nós orquestradores elegíveis com locking FOR UPDATE OF o (MM-14).
fn select_best_gpu_for_devices(
    devices: &[heph_contracts::GpuDeviceTelemetry],
    required_gb: Option<i32>,
) -> Option<String> {
    if devices.is_empty() {
        return None;
    }
    let chosen = match required_gb {
        Some(req_gb) => {
            let req_mib = (req_gb as i64) * 1024;
            let mut valid: Vec<_> = devices.iter().filter(|d| d.vram_total >= req_mib).collect();
            valid.sort_by(|a, b| {
                a.vram_total
                    .cmp(&b.vram_total)
                    .then_with(|| a.vram_used.cmp(&b.vram_used))
                    .then_with(|| a.index.cmp(&b.index))
            });
            valid.first().cloned()
        }
        None => {
            let mut valid: Vec<_> = devices.iter().collect();
            valid.sort_by(|a, b| {
                b.vram_total
                    .cmp(&a.vram_total)
                    .then_with(|| a.vram_used.cmp(&b.vram_used))
                    .then_with(|| a.index.cmp(&b.index))
            });
            valid.first().cloned()
        }
    };
    chosen.map(|d| d.uuid.clone())
}

use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::ManagerError;

/// Representa o nó orquestrador eleito para execução do job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElectedOrchestrator {
    pub id: Uuid,
    pub endpoint: String,
    pub fallback_used: bool,
    pub gpu_device: Option<String>,
}

/// Seleciona um nó orquestrador elegível dentro da transação ativa.
///
/// Invariante #4:
/// - Bloqueio exclusivo no nó eleito via `FOR UPDATE OF o`.
/// - Suporte a `orchestrator_hint` solicitado pelo usuário.
/// - Fallback ordenado por capacidade VRAM e nome se o nó pedido não estiver elegível.
/// - Atualização de `queue_reason` (`waiting_vram` ou `waiting_slot`) caso nenhum nó seja eleito.
pub async fn select_eligible_orchestrator(
    conn: &mut PgConnection,
    job_id: Uuid,
    hint: Option<Uuid>,
    required_gb: Option<i32>,
    telemetry_cache: &crate::nodes::TelemetryCache,
    manual_gpu_device: Option<&str>,
) -> Result<Option<ElectedOrchestrator>, ManagerError> {
    let mut selected_orch: Option<(Uuid, String)> = None;
    let mut selected_gpu: Option<String> = manual_gpu_device.map(|s| s.to_string());
    let mut fallback_used = false;

    // 1. Tenta nó indicado pelo orchestrator_hint se presente.
    if let Some(hint_id) = hint {
        let hinted: Option<(Uuid, String, Option<serde_json::Value>)> = sqlx::query_as(
            "SELECT o.id, o.endpoint, o.gpu_devices FROM orchestrators o \
             WHERE o.id = $1 AND o.status = 'online' \
               AND NOT EXISTS (SELECT 1 FROM jobs j \
                               WHERE j.orchestrator_id = o.id \
                                 AND j.status IN ('dispatched','running','cancelling')) \
               AND ($2::int IS NULL OR o.vram_total_gb IS NULL OR o.vram_total_gb >= $2) \
             FOR UPDATE OF o",
        )
        .bind(hint_id)
        .bind(required_gb)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|e| ManagerError::Internal(format!("find hinted orchestrator: {e}")))?;

        if let Some((id, endpoint, db_gpu_devices)) = hinted {
            selected_orch = Some((id, endpoint));
            // Se gpu manual não foi fixada no submit (nó escolhido, GPU automática),
            // aplica a mesma regra automática para escolher a melhor GPU deste nó:
            if selected_gpu.is_none() {
                let cache_devices = {
                    let c = telemetry_cache.read().await;
                    c.get(&id).map(|s| s.gpu_devices.clone())
                };
                let devices = match cache_devices {
                    Some(devs) if !devs.is_empty() => devs,
                    _ => db_gpu_devices
                        .and_then(|val| serde_json::from_value(val).ok())
                        .unwrap_or_default(),
                };
                selected_gpu = select_best_gpu_for_devices(&devices, required_gb);
            }
        } else if manual_gpu_device.is_some() {
            // Decisão 4: Para GPU manual, o nó do hint precisa estar elegível; se não, o job espera (não cai em fallback para outro nó)
            let reason = if required_gb.is_some() {
                "waiting_vram"
            } else {
                "waiting_slot"
            };
            sqlx::query("UPDATE jobs SET queue_reason = $2 WHERE id = $1 AND status = 'queued'")
                .bind(job_id)
                .bind(reason)
                .execute(&mut *conn)
                .await
                .map_err(|e| ManagerError::Internal(format!("set queue reason: {e}")))?;
            return Ok(None);
        } else {
            fallback_used = true;
        }
    }

    // 2. Fallback de eleição geral se nenhum nó foi selecionado pelo hint (apenas se não for GPU manual).
    if selected_orch.is_none() && manual_gpu_device.is_none() {
        type EligibleNodeRow = (Uuid, String, Option<i32>, Option<serde_json::Value>, String);
        // Seleciona todos os nós online e sem job ativo com lock FOR UPDATE OF o
        let eligible_nodes: Vec<EligibleNodeRow> = sqlx::query_as(
            "SELECT o.id, o.endpoint, o.vram_total_gb, o.gpu_devices, o.name FROM orchestrators o \
             WHERE o.status = 'online' \
               AND NOT EXISTS (SELECT 1 FROM jobs j \
                               WHERE j.orchestrator_id = o.id \
                                 AND j.status IN ('dispatched','running','cancelling')) \
               AND ($1::int IS NULL OR o.vram_total_gb IS NULL OR o.vram_total_gb >= $1) \
             FOR UPDATE OF o",
        )
        .bind(required_gb)
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| ManagerError::Internal(format!("find eligible orchestrators: {e}")))?;

        if !eligible_nodes.is_empty() {
            let cache_guard = telemetry_cache.read().await;

            // Para cada nó, encontra a melhor GPU (menor placa com vram_total >= required, empate menor vram_used; required desconhecido -> maior placa)
            struct NodeCandidate {
                id: Uuid,
                endpoint: String,
                chosen_gpu: Option<String>,
                gpu_vram_total: i64,
                gpu_vram_used: i64,
                has_gpu_devices: bool,
                node_vram_gb: Option<i32>,
                name: String,
            }

            let mut candidates: Vec<NodeCandidate> = Vec::new();

            for (node_id, endpoint, node_vram_gb, db_gpu_devices, name) in eligible_nodes {
                let cache_devices = cache_guard.get(&node_id).map(|s| s.gpu_devices.clone());
                let devices = match cache_devices {
                    Some(devs) if !devs.is_empty() => devs,
                    _ => db_gpu_devices
                        .and_then(|val| serde_json::from_value(val).ok())
                        .unwrap_or_default(),
                };

                if devices.is_empty() {
                    candidates.push(NodeCandidate {
                        id: node_id,
                        endpoint,
                        chosen_gpu: None,
                        gpu_vram_total: 0,
                        gpu_vram_used: 0,
                        has_gpu_devices: false,
                        node_vram_gb,
                        name,
                    });
                } else {
                    let chosen_uuid = select_best_gpu_for_devices(&devices, required_gb);
                    if let Some(ref uuid) = chosen_uuid {
                        let dev = devices.iter().find(|d| d.uuid == *uuid).unwrap();
                        candidates.push(NodeCandidate {
                            id: node_id,
                            endpoint,
                            chosen_gpu: chosen_uuid,
                            gpu_vram_total: dev.vram_total,
                            gpu_vram_used: dev.vram_used,
                            has_gpu_devices: true,
                            node_vram_gb,
                            name,
                        });
                    }
                }
            }

            // Ordena candidatos
            candidates.sort_by(|a, b| match required_gb {
                Some(_) => b
                    .has_gpu_devices
                    .cmp(&a.has_gpu_devices)
                    .then_with(|| a.gpu_vram_total.cmp(&b.gpu_vram_total))
                    .then_with(|| a.gpu_vram_used.cmp(&b.gpu_vram_used))
                    .then_with(|| b.node_vram_gb.cmp(&a.node_vram_gb))
                    .then_with(|| a.name.cmp(&b.name)),
                None => b
                    .has_gpu_devices
                    .cmp(&a.has_gpu_devices)
                    .then_with(|| b.gpu_vram_total.cmp(&a.gpu_vram_total))
                    .then_with(|| a.gpu_vram_used.cmp(&b.gpu_vram_used))
                    .then_with(|| b.node_vram_gb.cmp(&a.node_vram_gb))
                    .then_with(|| a.name.cmp(&b.name)),
            });

            if let Some(best) = candidates.into_iter().next() {
                selected_orch = Some((best.id, best.endpoint));
                selected_gpu = best.chosen_gpu;
            }
        }
    }

    // 3. Resultado ou marcação de queue_reason.
    match selected_orch {
        Some((id, endpoint)) => Ok(Some(ElectedOrchestrator {
            id,
            endpoint,
            fallback_used,
            gpu_device: selected_gpu,
        })),
        None => {
            let reason = if required_gb.is_some() {
                "waiting_vram"
            } else {
                "waiting_slot"
            };
            sqlx::query("UPDATE jobs SET queue_reason = $2 WHERE id = $1 AND status = 'queued'")
                .bind(job_id)
                .bind(reason)
                .execute(&mut *conn)
                .await
                .map_err(|e| ManagerError::Internal(format!("set queue reason: {e}")))?;

            Ok(None)
        }
    }
}
