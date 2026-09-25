//! Validação e salvamento de artefatos de jobs (MM-11).

use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::error::ManagerError;
use crate::jobs::lifecycle::is_valid_md5;
pub use heph_contracts::artifacts::ArtifactItem;
pub use heph_contracts::report::ReportBody as ReportRequest;

/// Defesa em profundidade (incidente galeria vazia): kinds que exigem artefato
/// não podem transitar para done sem eles.
pub fn done_artifacts_violation(kind: &str, artifacts: Option<&[ArtifactItem]>) -> Option<String> {
    let arts: &[ArtifactItem] = artifacts.unwrap_or(&[]);
    match kind {
        "diffusion_generate" => {
            if arts.is_empty() {
                return Some("diffusion_generate exige ao menos 1 artefato".into());
            }
            if !arts.iter().any(|a| a.kind == "generated") {
                return Some("diffusion_generate exige artefato kind 'generated'".into());
            }
            None
        }
        "yolo_train" => {
            if !arts.is_empty() && !arts.iter().any(|a| a.kind == "model") {
                return Some("yolo_train exige artefato kind 'model'".into());
            }
            None
        }
        "diffusion_train" | "yolo_predict" | "autotracker" | "autolabel" => {
            if arts.is_empty() {
                return Some(format!("{kind} exige ao menos 1 artefato"));
            }
            None
        }
        _ => None,
    }
}

/// Salva artefatos intermediários durante execução (running / preparing).
pub async fn save_intermediate_artifacts(
    pool: &PgPool,
    id: Uuid,
    artifacts: &[ArtifactItem],
) -> Result<(), ManagerError> {
    for art in artifacts {
        if is_valid_md5(&art.md5) && art.bytes >= 0 {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM job_artifacts WHERE job_id = $1 AND path = $2)",
            )
            .bind(id)
            .bind(&art.path)
            .fetch_one(pool)
            .await
            .unwrap_or(false);

            if !exists {
                let art_id = Uuid::new_v4();
                let _ = sqlx::query(
                    "INSERT INTO job_artifacts (id, job_id, kind, path, md5, bytes) VALUES ($1, $2, $3, $4, $5, $6)",
                )
                .bind(art_id)
                .bind(id)
                .bind(&art.kind)
                .bind(&art.path)
                .bind(&art.md5)
                .bind(art.bytes)
                .execute(pool)
                .await;
            }
        }
    }
    Ok(())
}

/// Valida e insere artefatos finais no término do job (bloco done) sob conexão transacional.
pub async fn validate_and_save_done_artifacts(
    conn: &mut PgConnection,
    id: Uuid,
    artifacts: &[ArtifactItem],
) -> Result<(), ManagerError> {
    for art in artifacts {
        if !is_valid_md5(&art.md5) {
            return Err(ManagerError::Internal(format!("invalid md5: {}", art.md5)));
        }
        if art.bytes < 0 {
            return Err(ManagerError::Internal(format!(
                "negative bytes: {}",
                art.bytes
            )));
        }
    }

    // Limpa artifacts existentes (idempotência).
    sqlx::query("DELETE FROM job_artifacts WHERE job_id = $1")
        .bind(id)
        .execute(&mut *conn)
        .await
        .map_err(|e| ManagerError::Internal(format!("delete old artifacts: {e}")))?;

    for art in artifacts {
        let art_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO job_artifacts (id, job_id, kind, path, md5, bytes) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(art_id)
        .bind(id)
        .bind(&art.kind)
        .bind(&art.path)
        .bind(&art.md5)
        .bind(art.bytes)
        .execute(&mut *conn)
        .await
        .map_err(|e| ManagerError::Internal(format!("insert artifact: {e}")))?;
    }

    Ok(())
}
