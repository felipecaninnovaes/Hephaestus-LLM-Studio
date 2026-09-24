//! Processamento, agregação e persistência de métricas de jobs (MM-11).

use std::collections::HashMap;
use sqlx::{PgConnection, PgPool};
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

/// Extrai uma chave numérica estável (epoch * 1_000_000 + step) de um objeto de métricas.
/// Permite rastrear múltiplos passos e fases de preparação dentro da mesma época.
pub fn metrics_key(value: &serde_json::Value) -> Option<i64> {
    let epoch = value.get("epoch").and_then(|v| v.as_i64())?;
    let step = value.get("step").and_then(|v| v.as_i64()).unwrap_or(0);
    Some(epoch * 1_000_000 + step)
}

/// Faz upsert incremental de metrics sob uma conexão transacional aberta.
pub async fn upsert_metrics_conn(
    conn: &mut PgConnection,
    id: Uuid,
    new_metrics: &serde_json::Value,
) -> Result<(), ManagerError> {
    // Lê array existente (NULL → vazio via COALESCE).
    let existing: serde_json::Value =
        sqlx::query_scalar("SELECT COALESCE(metrics, '[]'::jsonb) FROM jobs WHERE id = $1")
            .bind(id)
            .fetch_one(&mut *conn)
            .await
            .map_err(|e| ManagerError::Internal(format!("read metrics: {e}")))?;

    // Mapa chave → objeto (dedup por (epoch, step)).
    let mut metrics_map: HashMap<i64, serde_json::Value> = HashMap::new();

    // 1. Itens existentes.
    for item in normalize_metrics_to_array(&existing) {
        if let Some(k) = metrics_key(&item) {
            metrics_map.insert(k, item);
        }
    }

    // 2. Itens novos (substitui se chave repetida).
    for item in normalize_metrics_to_array(new_metrics) {
        if let Some(k) = metrics_key(&item) {
            metrics_map.insert(k, item);
        }
    }

    // 3. Ordena por chave cronológica e grava como {"items": [...]}.
    let mut items: Vec<serde_json::Value> = metrics_map.into_values().collect();
    items.sort_by_key(|v| metrics_key(v).unwrap_or(0));

    let merged = serde_json::json!({"items": items});
    sqlx::query("UPDATE jobs SET metrics = $2 WHERE id = $1")
        .bind(id)
        .bind(&merged)
        .execute(&mut *conn)
        .await
        .map_err(|e| ManagerError::Internal(format!("write metrics: {e}")))?;

    Ok(())
}

/// Helper para upsert de metrics adquirindo conexão do pool (usado no status running/preparing).
pub async fn upsert_metrics(
    pool: &PgPool,
    id: Uuid,
    new_metrics: &serde_json::Value,
) -> Result<(), ManagerError> {
    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| ManagerError::Internal(format!("acquire conn for metrics: {e}")))?;
    upsert_metrics_conn(&mut conn, id, new_metrics).await
}
