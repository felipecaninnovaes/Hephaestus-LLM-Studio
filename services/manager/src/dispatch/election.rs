//! Eleição e seleção de nós orquestradores elegíveis com locking FOR UPDATE OF o (MM-14).

use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::ManagerError;

/// Representa o nó orquestrador eleito para execução do job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElectedOrchestrator {
    pub id: Uuid,
    pub endpoint: String,
    pub fallback_used: bool,
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
) -> Result<Option<ElectedOrchestrator>, ManagerError> {
    let mut selected_orch: Option<(Uuid, String)> = None;
    let mut fallback_used = false;

    // 1. Tenta nó indicado pelo orchestrator_hint se presente.
    if let Some(hint_id) = hint {
        let hinted: Option<(Uuid, String)> = sqlx::query_as(
            "SELECT o.id, o.endpoint FROM orchestrators o \
             WHERE o.id = $1 AND o.status = 'online' \
               AND NOT EXISTS (SELECT 1 FROM jobs j \
                               WHERE j.orchestrator_id = o.id \
                                 AND j.status IN ('dispatched','running','cancelling')) \
               AND ($2::int IS NULL OR (o.vram_total_gb IS NOT NULL AND o.vram_total_gb >= $2)) \
             FOR UPDATE OF o",
        )
        .bind(hint_id)
        .bind(required_gb)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|e| ManagerError::Internal(format!("find hinted orchestrator: {e}")))?;

        if let Some(o) = hinted {
            selected_orch = Some(o);
        } else {
            fallback_used = true;
        }
    }

    // 2. Fallback de eleição geral se nenhum nó foi selecionado pelo hint.
    if selected_orch.is_none() {
        let eligible: Option<(Uuid, String)> = sqlx::query_as(
            "SELECT o.id, o.endpoint FROM orchestrators o \
             WHERE o.status = 'online' \
               AND NOT EXISTS (SELECT 1 FROM jobs j \
                               WHERE j.orchestrator_id = o.id \
                                 AND j.status IN ('dispatched','running','cancelling')) \
               AND ($1::int IS NULL OR (o.vram_total_gb IS NOT NULL AND o.vram_total_gb >= $1)) \
             ORDER BY (o.vram_total_gb IS NULL) ASC, \
                      o.vram_total_gb DESC NULLS LAST, \
                      o.name ASC \
             LIMIT 1 \
             FOR UPDATE OF o",
        )
        .bind(required_gb)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|e| ManagerError::Internal(format!("find orchestrator: {e}")))?;

        selected_orch = eligible;
    }

    // 3. Resultado ou marcação de queue_reason.
    match selected_orch {
        Some((id, endpoint)) => Ok(Some(ElectedOrchestrator {
            id,
            endpoint,
            fallback_used,
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
