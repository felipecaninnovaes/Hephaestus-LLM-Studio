//! Handler de exportação de métricas (GET /api/jobs/:id/export?format=csv|parquet).
//!
//! Spec: tasks/specs/telemetria-observabilidade.md §3.4, fatia 5c.
//! Colunas: seq,epoch,step,key,value,ts
//! Ordenado por seq crescente.
//!
//! Memória limitada:
//! - CSV: streaming em chunks paginando por `afterSeq`. Escape RFC 4180.
//! - Parquet: paginação por `afterSeq` montando RecordBatch com teto de 2M pontos (413).

use std::sync::Arc;

use arrow_array::builder::{Float64Builder, Int32Builder, Int64Builder, StringBuilder};
use arrow_array::RecordBatch;
use arrow_schema::{DataType, Field, Schema};
use axum::body::Bytes;
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::stream;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;
use serde::Deserialize;

use super::helpers::{not_found, parse_uuid, queue_unavailable};
use crate::error::err;
use crate::jobs::manager_client::ManagerError;
use crate::state::AppState;

pub const DEFAULT_MAX_PARQUET_POINTS: usize = 2_000_000;
pub const EXPORT_PAGE_SIZE: i64 = 10_000;

#[derive(Debug, Deserialize)]
pub struct JobExportQuery {
    pub format: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Parquet,
}

impl ExportFormat {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "csv" => Some(Self::Csv),
            "parquet" => Some(Self::Parquet),
            _ => None,
        }
    }
}

/// Escapa um valor de texto para CSV segundo a RFC 4180.
/// Se contiver vírgula, aspas duplas, \r ou \n, envolve em aspas e duplica aspas internas.
pub fn escape_csv_field(val: &str) -> String {
    if val.contains([',', '"', '\r', '\n']) {
        let escaped = val.replace('"', "\"\"");
        format!("\"{escaped}\"")
    } else {
        val.to_string()
    }
}

/// GET /api/jobs/:id/export?format=csv|parquet
pub async fn export_job_metrics(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<JobExportQuery>,
) -> Response {
    export_job_metrics_with_ceiling(state, id, q, DEFAULT_MAX_PARQUET_POINTS).await
}

pub async fn export_job_metrics_with_ceiling(
    state: AppState,
    id: String,
    q: JobExportQuery,
    max_parquet_points: usize,
) -> Response {
    if parse_uuid(&id).is_none() {
        return not_found();
    }

    let format_str = match q.format.as_deref() {
        Some(f) if !f.trim().is_empty() => f.trim(),
        _ => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_format",
                "formato inválido: esperado 'csv' ou 'parquet'",
            );
        }
    };

    let format = match ExportFormat::parse(format_str) {
        Some(f) => f,
        None => {
            return err(
                StatusCode::BAD_REQUEST,
                "invalid_format",
                "formato inválido: esperado 'csv' ou 'parquet'",
            );
        }
    };

    // Valida existência do job via manager (404 antes de exportar)
    let first_page = match state
        .manager
        .get_job_metric_points(&id, None, Some(EXPORT_PAGE_SIZE), None)
        .await
    {
        Ok(res) => res,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };

    match format {
        ExportFormat::Csv => export_csv_stream(state, id, first_page),
        ExportFormat::Parquet => export_parquet(state, id, first_page, max_parquet_points).await,
    }
}

fn export_csv_stream(
    state: AppState,
    id: String,
    first_page: heph_contracts::telemetry::MetricPointsResponse,
) -> Response {
    let filename = format!("job-{id}-metrics.csv");
    let last_seq = first_page.items.last().map(|p| p.seq).unwrap_or(0);
    let initial_state = (
        state,
        id,
        Some(first_page.items),
        first_page.max_seq,
        last_seq,
        true,  // precisa emitir header
        false, // finished
    );

    let stream = stream::unfold(
        initial_state,
        |(state, id, pending_items, max_seq, mut last_seq, send_header, mut finished)| async move {
            if finished {
                return None;
            }

            let mut out = String::new();
            if send_header {
                out.push_str("seq,epoch,step,key,value,ts\n");
            }

            let items = if let Some(items) = pending_items {
                items
            } else {
                match state
                    .manager
                    .get_job_metric_points(&id, Some(last_seq), Some(EXPORT_PAGE_SIZE), None)
                    .await
                {
                    Ok(resp) => resp.items,
                    Err(e) => {
                        tracing::error!(job_id=%id, error=%e, "falha ao paginar metric points para CSV export");
                        return Some((
                            Err(std::io::Error::other(e.to_string())),
                            (state, id, None, max_seq, last_seq, false, true),
                        ));
                    }
                }
            };

            if items.is_empty() {
                if !out.is_empty() {
                    return Some((
                        Ok(Bytes::from(out)),
                        (state, id, None, max_seq, last_seq, false, true),
                    ));
                }
                return None;
            }

            for p in &items {
                last_seq = p.seq;
                let epoch_str = p.epoch.map(|e| e.to_string()).unwrap_or_default();
                let escaped_key = escape_csv_field(&p.key);
                let escaped_ts = escape_csv_field(&p.ts);
                out.push_str(&format!(
                    "{},{},{},{},{},{}\n",
                    p.seq, epoch_str, p.step, escaped_key, p.value, escaped_ts
                ));
            }

            if last_seq >= max_seq {
                finished = true;
            }

            Some((
                Ok(Bytes::from(out)),
                (state, id, None, max_seq, last_seq, false, finished),
            ))
        },
    );

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        Body::from_stream(stream),
    )
        .into_response()
}

async fn export_parquet(
    state: AppState,
    id: String,
    first_page: heph_contracts::telemetry::MetricPointsResponse,
    max_parquet_points: usize,
) -> Response {
    let filename = format!("job-{id}-metrics.parquet");
    let max_seq = first_page.max_seq;
    let mut all_points = first_page.items;
    let mut last_seq = all_points.last().map(|p| p.seq).unwrap_or(0);

    if all_points.len() > max_parquet_points {
        return err(
            StatusCode::PAYLOAD_TOO_LARGE,
            "export_too_large",
            "job excede o teto de pontos para exportação em parquet",
        );
    }

    while last_seq < max_seq {
        match state
            .manager
            .get_job_metric_points(&id, Some(last_seq), Some(EXPORT_PAGE_SIZE), None)
            .await
        {
            Ok(resp) => {
                if resp.items.is_empty() {
                    break;
                }
                for p in resp.items {
                    last_seq = p.seq;
                    all_points.push(p);
                    if all_points.len() > max_parquet_points {
                        return err(
                            StatusCode::PAYLOAD_TOO_LARGE,
                            "export_too_large",
                            "job excede o teto de pontos para exportação em parquet",
                        );
                    }
                }
            }
            Err(e) => {
                tracing::error!(job_id=%id, error=%e, "falha ao paginar metric points para parquet");
                return queue_unavailable();
            }
        }
    }

    let bytes = match tokio::task::spawn_blocking(move || build_parquet_bytes(&all_points)).await {
        Ok(Ok(b)) => b,
        Ok(Err(e)) => {
            tracing::error!(job_id=%id, error=%e, "falha ao codificar parquet");
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "falha ao gerar arquivo parquet",
            );
        }
        Err(e) => {
            tracing::error!(job_id=%id, error=%e, "join error ao gerar parquet");
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "falha ao gerar arquivo parquet",
            );
        }
    };

    (
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                "application/vnd.apache.parquet".to_string(),
            ),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
            (header::CONTENT_LENGTH, bytes.len().to_string()),
        ],
        bytes,
    )
        .into_response()
}

pub fn build_parquet_bytes(
    points: &[heph_contracts::telemetry::MetricPointWithKey],
) -> Result<Bytes, String> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("seq", DataType::Int64, false),
        Field::new("epoch", DataType::Int32, true),
        Field::new("step", DataType::Int64, false),
        Field::new("key", DataType::Utf8, false),
        Field::new("value", DataType::Float64, false),
        Field::new("ts", DataType::Utf8, false),
    ]));

    let mut seq_builder = Int64Builder::with_capacity(points.len());
    let mut epoch_builder = Int32Builder::with_capacity(points.len());
    let mut step_builder = Int64Builder::with_capacity(points.len());
    let mut key_builder = StringBuilder::with_capacity(points.len(), points.len() * 8);
    let mut value_builder = Float64Builder::with_capacity(points.len());
    let mut ts_builder = StringBuilder::with_capacity(points.len(), points.len() * 25);

    for p in points {
        seq_builder.append_value(p.seq);
        epoch_builder.append_option(p.epoch);
        step_builder.append_value(p.step);
        key_builder.append_value(&p.key);
        value_builder.append_value(p.value);
        ts_builder.append_value(&p.ts);
    }

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(seq_builder.finish()),
            Arc::new(epoch_builder.finish()),
            Arc::new(step_builder.finish()),
            Arc::new(key_builder.finish()),
            Arc::new(value_builder.finish()),
            Arc::new(ts_builder.finish()),
        ],
    )
    .map_err(|e| format!("batch error: {e}"))?;

    let mut buf = Vec::new();
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(&mut buf, schema, Some(props))
        .map_err(|e| format!("writer init error: {e}"))?;

    writer
        .write(&batch)
        .map_err(|e| format!("write error: {e}"))?;
    writer.close().map_err(|e| format!("close error: {e}"))?;

    Ok(Bytes::from(buf))
}
