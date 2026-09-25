//! Exclusão pontual e limpeza em lote de jobs terminais (AC-003, MM-13).

use sqlx::PgPool;
use std::collections::{BTreeSet, HashSet};
use uuid::Uuid;

use super::types::{CleanupResult, DeletedJob};
use crate::constants::TERMINAL_STATUSES;
use crate::error::ManagerError;

/// Monta a lista exata de chaves S3 a varrer num job (origem dupla:
/// artifacts + `models.s3_key` de órfãos de bytes, excluindo chaves de
/// gerações de todas as linhas do job, incl. trash) + conta modelos
/// expurgados/gerações preservadas. Deve rodar DENTRO da transação,
/// ANTES do `DELETE FROM jobs`.
pub async fn plan_job_sweep(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
) -> Result<(Vec<String>, Vec<String>, i64, i64), ManagerError> {
    // Chaves de gerações a PRESERVAR (s3_key + thumb_s3_key) — todas as linhas
    // do job, incl. trash (deleted_at preenchido; as linhas sobrevivem via
    // SET NULL da 0012 e continuam referenciando s3_key/thumb_s3_key).
    let gen_rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT s3_key, thumb_s3_key FROM generations \
         WHERE job_id = $1",
    )
    .bind(id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| ManagerError::Internal(format!("list generations to preserve: {e}")))?;
    let preserved: HashSet<String> = gen_rows
        .iter()
        .flat_map(|(k, t)| {
            let mut v = vec![k.clone()];
            if let Some(th) = t {
                v.push(th.clone());
            }
            v
        })
        .collect();
    let generations_preserved = gen_rows.len() as i64;

    // Chaves dos artifacts, excluindo as preservadas.
    let paths: Vec<String> =
        sqlx::query_scalar("SELECT path FROM job_artifacts WHERE job_id = $1 ORDER BY path, id")
            .bind(id)
            .fetch_all(&mut **tx)
            .await
            .map_err(|e| ManagerError::Internal(format!("list artifacts before delete: {e}")))?;

    // Chaves órfãs de bytes do catálogo `models` (s3_key NOT NULL, 0007) —
    // capturadas ANTES do DELETE, pois vivem fora do prefixo artifacts/{job}/.
    let model_keys: Vec<String> = sqlx::query_scalar("SELECT s3_key FROM models WHERE job_id = $1")
        .bind(id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| ManagerError::Internal(format!("list models before delete: {e}")))?;

    // União deduplicada e ordenada (artifacts + models), sem as preservadas.
    let object_keys: Vec<String> = {
        let mut set: BTreeSet<String> = BTreeSet::new();
        for p in &paths {
            let k = format!("artifacts/{id}/{p}");
            if !preserved.contains(&k) {
                set.insert(k);
            }
        }
        for k in &model_keys {
            if !preserved.contains(k) {
                set.insert(k.clone());
            }
        }
        set.into_iter().collect()
    };

    // Expurga linhas do catálogo de modelos derivadas deste job (D-a: sim).
    let models_deleted = sqlx::query("DELETE FROM models WHERE job_id = $1")
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(|e| ManagerError::Internal(format!("delete job models: {e}")))?
        .rows_affected() as i64;

    Ok((paths, object_keys, models_deleted, generations_preserved))
}

/// Apaga um job terminal e retorna a linha + as chaves S3 a varrer.
///
/// Guarda: `done|failed|cancelled` → apaga (FK `job_artifacts ON DELETE
/// CASCADE`; `generations` ficam com `job_id` NULL — galeria preservada);
/// qualquer outro status → [`ManagerError::NotDeletable`]; inexistente →
/// [`ManagerError::NotFound`].
pub async fn delete_job(pool: &PgPool, id: Uuid) -> Result<DeletedJob, ManagerError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| ManagerError::Internal(format!("begin delete job: {e}")))?;

    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM jobs WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| ManagerError::Internal(format!("lock job for delete: {e}")))?;

    let status = status.ok_or(ManagerError::NotFound)?;
    if !TERMINAL_STATUSES.contains(&status.as_str()) {
        return Err(ManagerError::NotDeletable);
    }

    let (artifacts, object_keys, models_deleted, generations_preserved) =
        plan_job_sweep(&mut tx, id).await?;

    sqlx::query("DELETE FROM jobs WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ManagerError::Internal(format!("delete job: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| ManagerError::Internal(format!("commit delete job: {e}")))?;

    Ok(DeletedJob {
        id: id.to_string(),
        status,
        artifacts,
        object_keys,
        models_deleted,
        generations_preserved,
    })
}

/// Normaliza e valida os statuses pedidos numa limpeza em lote.
/// `None`/vazio → todos os terminais; qualquer status não-terminal → erro.
pub fn normalize_cleanup_statuses(
    statuses: Option<&Vec<String>>,
) -> Result<Vec<String>, ManagerError> {
    let list = match statuses {
        None => Vec::new(),
        Some(v) => v.clone(),
    };
    let list = if list.is_empty() {
        TERMINAL_STATUSES.iter().map(|s| s.to_string()).collect()
    } else {
        for s in &list {
            if !TERMINAL_STATUSES.contains(&s.as_str()) {
                return Err(ManagerError::InvalidRequest(format!(
                    "cleanup só aceita estados terminais (done|failed|cancelled), recebido: {s}"
                )));
            }
        }
        list
    };
    Ok(list)
}

/// Limpeza em lote de jobs terminais (AC-003).
///
/// `older_than_days`: apaga apenas jobs terminais há mais de N dias
/// (`COALESCE(finished_at, created_at)`); `None` = sem recorte temporal.
/// Exige pelo menos um critério (dias ou statuses) para não apagar o
/// histórico inteiro por omissão. Retorna as linhas apagadas com as chaves
/// S3 a varrer (exclui gerações preservadas).
pub async fn cleanup_jobs(
    pool: &PgPool,
    older_than_days: Option<i64>,
    statuses: Option<Vec<String>>,
) -> Result<CleanupResult, ManagerError> {
    if older_than_days.is_none() && statuses.as_ref().is_none_or(Vec::is_empty) {
        return Err(ManagerError::InvalidRequest(
            "cleanup exige pelo menos um critério (olderThanDays ou statuses)".into(),
        ));
    }
    if let Some(d) = older_than_days {
        if d < 0 {
            return Err(ManagerError::InvalidRequest(
                "olderThanDays deve ser >= 0".into(),
            ));
        }
    }
    let statuses = normalize_cleanup_statuses(statuses.as_ref())?;

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| ManagerError::Internal(format!("begin cleanup: {e}")))?;

    // Seleciona os candidatos (lock pessimista p/ não corrida com report/dispatch).
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, status FROM jobs \
         WHERE status = ANY($1) \
           AND ($2::bigint IS NULL OR COALESCE(finished_at, created_at) < NOW() - ($2::bigint * INTERVAL '1 day')) \
         FOR UPDATE SKIP LOCKED",
    )
    .bind(&statuses)
    .bind(older_than_days)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| ManagerError::Internal(format!("select jobs to cleanup: {e}")))?;

    let mut jobs = Vec::with_capacity(rows.len());
    let mut all_keys: Vec<String> = Vec::new();
    for (id, status) in rows {
        let (artifacts, object_keys, models_deleted, generations_preserved) =
            plan_job_sweep(&mut tx, id).await?;

        sqlx::query("DELETE FROM jobs WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| ManagerError::Internal(format!("delete job in cleanup: {e}")))?;

        all_keys.extend(object_keys.iter().cloned());
        jobs.push(DeletedJob {
            id: id.to_string(),
            status,
            artifacts,
            object_keys,
            models_deleted,
            generations_preserved,
        });
    }

    tx.commit()
        .await
        .map_err(|e| ManagerError::Internal(format!("commit cleanup: {e}")))?;

    let deleted = jobs.len() as i64;
    Ok(CleanupResult {
        deleted,
        jobs,
        object_keys: all_keys,
    })
}
