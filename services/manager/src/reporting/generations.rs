//! Hook de catalogação de gerações produzidas por jobs de difusão (ADR-0023, MM-11).

use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::ManagerError;
use super::artifacts::ReportRequest;

/// Hook generations (D5 — ADR-0023): job diffusion generate done com
/// artefato generated_meta → parse JSONL → INSERT em generations.
/// Executa sob conexão transacional. Best-effort: falha de parse/log não impede o report done.
pub async fn hook_generations_on_done(
    conn: &mut PgConnection,
    id: Uuid,
    report: &ReportRequest,
) -> Result<(), ManagerError> {
    let job_meta: Option<(String, String)> = match sqlx::query_as::<_, (String, String)>(
        "SELECT engine, mode FROM jobs WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await
    {
        Ok(opt) => opt,
        Err(e) => {
            tracing::warn!("hook generations: falha ao ler engine/mode do job {id}: {e}");
            None
        }
    };

    let (engine, mode) = match job_meta {
        Some(m) => m,
        None => return Ok(()),
    };

    if engine != "diffusion" || mode != "generate" {
        return Ok(());
    }

    // Procura artefato generated_meta nos artifacts do report.
    let meta_artifact = report
        .artifacts
        .as_ref()
        .and_then(|arts| arts.iter().find(|a| a.kind == "generated_meta"));

    if meta_artifact.is_none() {
        return Ok(());
    }

    // Usa meta_content enviado pelo orquestrador no report.
    // Retrocompat: se meta_content não vier, tenta ler de job_artifacts.content.
    let content: Option<String> = if report.meta_content.is_some() {
        report.meta_content.clone()
    } else {
        sqlx::query_scalar(
            "SELECT content FROM job_artifacts WHERE job_id = $1 AND kind = 'generated_meta' LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
        .ok()
        .flatten()
    };

    if let Some(jsonl_content) = content {
        for line in jsonl_content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<serde_json::Value>(line) {
                Ok(entry) => {
                    let filename = entry
                        .get("filename")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if filename.is_empty() {
                        continue;
                    }
                    let s3_key = format!("artifacts/{id}/{filename}");
                    let thumb_s3_key = entry
                        .get("thumb_filename")
                        .or_else(|| entry.get("thumb"))
                        .and_then(|v| v.as_str())
                        .map(|t| format!("artifacts/{id}/{t}"));
                    let seed = entry.get("seed").and_then(|v| v.as_i64()).unwrap_or(0);
                    let prompt = entry.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
                    let negative_prompt = entry.get("negative_prompt").and_then(|v| v.as_str());
                    let width = entry
                        .get("width")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(512) as i32;
                    let height = entry
                        .get("height")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(512) as i32;

                    // Params = RESTO da linha (batch_index, batch_size, etc.)
                    let mut gen_params = entry.clone();
                    if let Some(obj) = gen_params.as_object_mut() {
                        obj.remove("filename");
                        obj.remove("thumb_filename");
                        obj.remove("thumb");
                        obj.remove("seed");
                        obj.remove("prompt");
                        obj.remove("negative_prompt");
                        obj.remove("width");
                        obj.remove("height");
                    }

                    let result = sqlx::query(
                        "INSERT INTO generations \
                         (id, job_id, s3_key, thumb_s3_key, filename, seed, \
                          prompt, negative_prompt, width, height, params) \
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
                         ON CONFLICT (s3_key) DO NOTHING",
                    )
                    .bind(Uuid::new_v4())
                    .bind(id)
                    .bind(&s3_key)
                    .bind(&thumb_s3_key)
                    .bind(filename)
                    .bind(seed)
                    .bind(prompt)
                    .bind(negative_prompt)
                    .bind(width)
                    .bind(height)
                    .bind(&gen_params)
                    .execute(&mut *conn)
                    .await;

                    if let Err(e) = result {
                        tracing::warn!(
                            job_id = %id,
                            s3_key = %s3_key,
                            error = %e,
                            "falha ao inserir generation (best-effort)"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        job_id = %id,
                        error = %e,
                        "hook generations: falha ao parsear linha do meta (best-effort)"
                    );
                }
            }
        }
    } else {
        tracing::warn!(
            job_id = %id,
            "hook generations: generated_meta sem content em job_artifacts"
        );
    }

    Ok(())
}
