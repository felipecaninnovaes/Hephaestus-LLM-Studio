//! Processamento e persistência de métricas de jobs (MM-11).
//!
//! Fatia 1a (telemetria append-only, spec `telemetria-observabilidade.md`
//! §2.2/§3.1): os pontos são persistidos em `job_metric_points`
//! (migration 0020), NÃO mais em `jobs.metrics` (JSONB legado, O(n²) por
//! report — cada report relia todo o array para fazer merge). `jobs.metrics`
//! não é mais lida nem escrita por este módulo.
//!
//! A leitura compatível (wire `JobResponse.metrics` / `MetricsItem`,
//! consumida pelo web em `apps/web/lib/jobMetrics.ts`) é reconstruída em
//! `fetch_metrics_pivoted_batch`, pivotando os pontos por `(epoch, step)` —
//! essa reconstrução só é removida na fatia 4a.

use sqlx::{PgConnection, PgPool};
use std::collections::HashMap;
use uuid::Uuid;

use crate::error::ManagerError;

/// Normaliza um valor JSON recebido do orquestrador em um array de objetos
/// de métricas por epoch. Aceita: objeto único, array, ou `{"items":[...]}`.
pub fn normalize_metrics_to_array(value: &serde_json::Value) -> Vec<serde_json::Value> {
    if let Some(arr) = value.as_array() {
        return arr.clone();
    }
    if let Some(items) = value.get("items").and_then(|v| v.as_array()) {
        return items.clone();
    }
    if value.is_object() {
        return vec![value.clone()];
    }
    vec![]
}

/// Um ponto explodido de um item de métricas: toda chave numérica finita
/// fora `epoch`/`step`/`ts`/`timestamp` vira um ponto `(key, value)`.
struct RawPoint {
    epoch: Option<i32>,
    step: i64,
    key: String,
    value: f64,
    ts: Option<chrono::DateTime<chrono::Utc>>,
}

/// Explode os itens normalizados do payload do report em pontos individuais.
/// `epoch` é nullable — um item sem epoch NÃO é descartado (ao contrário do
/// legado `metrics_key`, que exigia epoch). `step` default 0. `ts` vem do
/// item (`ts`/`timestamp` RFC3339) se houver, senão `None` (grava `now()`).
fn explode_to_points(new_metrics: &serde_json::Value) -> Vec<RawPoint> {
    const SKIP_KEYS: [&str; 4] = ["epoch", "step", "ts", "timestamp"];
    let mut points = Vec::new();
    for item in normalize_metrics_to_array(new_metrics) {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let epoch = obj.get("epoch").and_then(|v| v.as_i64()).map(|e| e as i32);
        let step = obj.get("step").and_then(|v| v.as_i64()).unwrap_or(0);
        let ts = obj
            .get("ts")
            .or_else(|| obj.get("timestamp"))
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));
        for (key, value) in obj {
            if SKIP_KEYS.contains(&key.as_str()) {
                continue;
            }
            if let Some(n) = value.as_f64() {
                if n.is_finite() {
                    points.push(RawPoint {
                        epoch,
                        step,
                        key: key.clone(),
                        value: n,
                        ts,
                    });
                }
            }
        }
    }
    points
}

/// Insere um lote de pontos de métricas em `job_metric_points`, sob uma
/// conexão transacional aberta. Aloca `seq` via
/// `UPDATE jobs SET metric_seq = metric_seq + $n ... RETURNING metric_seq`
/// — o lock de linha serializa reports concorrentes do mesmo job — e numera
/// o lote `[ret-n+1 .. ret]`. Idempotente via
/// `ON CONFLICT ON CONSTRAINT job_metric_points_natural_key DO UPDATE`
/// (reenvio do outbox não duplica; a chave natural NÃO inclui `seq`, então
/// um reenvio apenas atualiza `value`/`ts`/`seq` da linha existente).
pub async fn insert_metrics_points_conn(
    conn: &mut PgConnection,
    id: Uuid,
    new_metrics: &serde_json::Value,
) -> Result<(), ManagerError> {
    let points = explode_to_points(new_metrics);
    if points.is_empty() {
        return Ok(());
    }
    let n = points.len() as i64;
    let ret_seq: i64 = sqlx::query_scalar(
        "UPDATE jobs SET metric_seq = metric_seq + $2 WHERE id = $1 RETURNING metric_seq",
    )
    .bind(id)
    .bind(n)
    .fetch_one(&mut *conn)
    .await
    .map_err(|e| ManagerError::Internal(format!("alloc metric_seq: {e}")))?;
    let start_seq = ret_seq - n + 1;

    for (i, p) in points.iter().enumerate() {
        let seq = start_seq + i as i64;
        sqlx::query(
            "INSERT INTO job_metric_points (job_id, seq, epoch, step, key, value, ts) \
             VALUES ($1, $2, $3, $4, $5, $6, COALESCE($7, now())) \
             ON CONFLICT ON CONSTRAINT job_metric_points_natural_key \
             DO UPDATE SET value = EXCLUDED.value, ts = EXCLUDED.ts, seq = EXCLUDED.seq",
        )
        .bind(id)
        .bind(seq)
        .bind(p.epoch)
        .bind(p.step)
        .bind(&p.key)
        .bind(p.value)
        .bind(p.ts)
        .execute(&mut *conn)
        .await
        .map_err(|e| ManagerError::Internal(format!("insert metric point: {e}")))?;
    }

    // pg_notify no canal job_events (fatia 1b) — dentro da MESMA transação
    // (visível ao listener só após commit; rollback descarta o notice).
    crate::notify::notify_metrics(&mut *conn, id, ret_seq).await?;

    Ok(())
}

/// Helper para inserir pontos de métricas adquirindo uma transação própria
/// do pool (usado no status running/preparing, fora da transação `done`).
pub async fn insert_metrics_points(
    pool: &PgPool,
    id: Uuid,
    new_metrics: &serde_json::Value,
) -> Result<(), ManagerError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| ManagerError::Internal(format!("begin metrics tx: {e}")))?;
    insert_metrics_points_conn(&mut tx, id, new_metrics).await?;
    tx.commit()
        .await
        .map_err(|e| ManagerError::Internal(format!("commit metrics tx: {e}")))?;
    Ok(())
}

/// Reconstrói o wire legado `{"items": [...]}` (compat `JobResponse.metrics`
/// / `MetricsItem`, fatia 1a — removido na 4a) para um lote de jobs, pivotando
/// `job_metric_points` por `(epoch, step)`: um objeto por par, com as chaves
/// de volta como campos (espelha o item original que foi explodido no
/// insert). Uma única query para todos os ids (evita N+1 em `list_jobs`).
pub async fn fetch_metrics_pivoted_batch(
    pool: &PgPool,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, serde_json::Value>, ManagerError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    type PivotRow = (Uuid, Option<i32>, i64, String, f64);
    let rows: Vec<PivotRow> = sqlx::query_as(
        "SELECT job_id, epoch, step, key, value FROM job_metric_points \
         WHERE job_id = ANY($1) ORDER BY job_id, epoch NULLS FIRST, step, seq",
    )
    .bind(ids)
    .fetch_all(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("fetch metric points: {e}")))?;

    // Agrupa em blocos contíguos por job_id (já ordenado por job_id na query).
    type PivotGroup = (Option<i32>, i64, String, f64);
    let mut by_job: Vec<(Uuid, Vec<PivotGroup>)> = Vec::new();
    for (job_id, epoch, step, key, value) in rows {
        match by_job.last_mut() {
            Some((jid, pts)) if *jid == job_id => pts.push((epoch, step, key, value)),
            _ => by_job.push((job_id, vec![(epoch, step, key, value)])),
        }
    }

    let mut out = HashMap::new();
    for (job_id, points) in by_job {
        let mut items: Vec<serde_json::Value> = Vec::new();
        let mut idx = 0;
        while idx < points.len() {
            let (epoch, step, _, _) = points[idx];
            let mut obj = serde_json::Map::new();
            while idx < points.len() && points[idx].0 == epoch && points[idx].1 == step {
                obj.insert(points[idx].2.clone(), serde_json::json!(points[idx].3));
                idx += 1;
            }
            if let Some(e) = epoch {
                obj.insert("epoch".to_string(), serde_json::json!(e));
            }
            obj.insert("step".to_string(), serde_json::json!(step));
            items.push(serde_json::Value::Object(obj));
        }
        out.insert(job_id, serde_json::json!({"items": items}));
    }
    Ok(out)
}

/// Resultado de uma linha bruta de `job_metric_points` (uso interno).
struct PointRow {
    seq: i64,
    epoch: Option<i32>,
    step: i64,
    key: String,
    value: f64,
    ts: chrono::DateTime<chrono::Utc>,
}

/// `(seq, epoch, step, key, value, ts)` — linha bruta de ponto já achatada
/// para o wire (`get_job_metric_points` monta o DTO a partir disso).
type RawPointRow = (
    i64,
    Option<i32>,
    i64,
    String,
    f64,
    chrono::DateTime<chrono::Utc>,
);

/// GET /internal/jobs/:id/metrics (fatia 1a §3.4) — pontos brutos com
/// filtro `afterSeq`/`keys` e downsampling `maxPoints` (min/max por bucket,
/// POR key, bucketizado sobre a ordem `(epoch NULLS FIRST, step)`).
///
/// Retorna `(items, max_seq, downsampled)`. `max_seq` é `jobs.metric_seq`
/// (contador do job, independente dos filtros) — o cliente usa para saber
/// quanto falta buscar. `downsampled` só é `true` quando alguma key foi
/// reduzida pelo bucketing.
///
/// Chamador garante que o job existe (404 antes de chamar esta função).
pub async fn fetch_metric_points_raw(
    pool: &PgPool,
    job_id: Uuid,
    after_seq: Option<i64>,
    keys: Option<&[String]>,
    max_points: Option<i64>,
) -> Result<(Vec<RawPointRow>, i64, bool), ManagerError> {
    let max_seq: i64 = sqlx::query_scalar("SELECT metric_seq FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("read metric_seq: {e}")))?;

    let rows: Vec<RawPointRow> = sqlx::query_as(
        "SELECT seq, epoch, step, key, value, ts FROM job_metric_points \
         WHERE job_id = $1 AND seq > $2 AND ($3::text[] IS NULL OR key = ANY($3)) \
         ORDER BY key, epoch NULLS FIRST, step, seq",
    )
    .bind(job_id)
    .bind(after_seq.unwrap_or(0))
    .bind(keys)
    .fetch_all(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("fetch metric points: {e}")))?;

    let rows: Vec<PointRow> = rows
        .into_iter()
        .map(|(seq, epoch, step, key, value, ts)| PointRow {
            seq,
            epoch,
            step,
            key,
            value,
            ts,
        })
        .collect();

    let Some(max_points) = max_points.filter(|&n| n > 0) else {
        let mut out: Vec<_> = rows
            .into_iter()
            .map(|p| (p.seq, p.epoch, p.step, p.key, p.value, p.ts))
            .collect();
        out.sort_by_key(|p| p.0);
        return Ok((out, max_seq, false));
    };

    // Agrupa em blocos contíguos por key (já ordenado por key na query).
    let mut by_key: Vec<(String, Vec<PointRow>)> = Vec::new();
    for row in rows {
        match by_key.last_mut() {
            Some((k, pts)) if *k == row.key => pts.push(row),
            _ => by_key.push((row.key.clone(), vec![row])),
        }
    }

    let mut downsampled = false;
    let mut out: Vec<RawPointRow> = Vec::new();
    for (_, points) in by_key {
        let len = points.len() as i64;
        if len <= max_points {
            for p in points {
                out.push((p.seq, p.epoch, p.step, p.key, p.value, p.ts));
            }
            continue;
        }
        downsampled = true;
        let num_buckets = (max_points / 2).max(1);
        let mut bucket_start = 0usize;
        for b in 0..num_buckets {
            let bucket_end = (((b + 1) * len) / num_buckets) as usize;
            if bucket_start >= bucket_end || bucket_start >= points.len() {
                break;
            }
            let bucket = &points[bucket_start..bucket_end.min(points.len())];
            if let (Some(min_p), Some(max_p)) = (
                bucket.iter().min_by(|a, b| a.value.total_cmp(&b.value)),
                bucket.iter().max_by(|a, b| a.value.total_cmp(&b.value)),
            ) {
                out.push((
                    min_p.seq,
                    min_p.epoch,
                    min_p.step,
                    min_p.key.clone(),
                    min_p.value,
                    min_p.ts,
                ));
                if max_p.seq != min_p.seq {
                    out.push((
                        max_p.seq,
                        max_p.epoch,
                        max_p.step,
                        max_p.key.clone(),
                        max_p.value,
                        max_p.ts,
                    ));
                }
            }
            bucket_start = bucket_end;
        }
    }
    out.sort_by_key(|p| p.0);
    Ok((out, max_seq, downsampled))
}

/// GET /internal/jobs/:id/metrics — wrapper de serviço: checa existência do
/// job (404 honesto) e monta o DTO de wire a partir de `fetch_metric_points_raw`.
pub async fn get_job_metric_points(
    pool: &PgPool,
    job_id: Uuid,
    after_seq: Option<i64>,
    keys: Option<&[String]>,
    max_points: Option<i64>,
) -> Result<heph_contracts::telemetry::MetricPointsResponse, ManagerError> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM jobs WHERE id = $1)")
        .bind(job_id)
        .fetch_one(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("check job: {e}")))?;
    if !exists {
        return Err(ManagerError::NotFound);
    }

    let (rows, max_seq, downsampled) =
        fetch_metric_points_raw(pool, job_id, after_seq, keys, max_points).await?;
    let items = rows
        .into_iter()
        .map(
            |(seq, epoch, step, key, value, ts)| heph_contracts::telemetry::MetricPointWithKey {
                seq,
                epoch,
                step,
                key,
                value,
                ts: ts.to_rfc3339(),
            },
        )
        .collect();
    Ok(heph_contracts::telemetry::MetricPointsResponse {
        items,
        max_seq,
        downsampled,
    })
}
