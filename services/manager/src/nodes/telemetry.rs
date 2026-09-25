//! Leitura e agregação de telemetria dos nós (ADR-0011, MM-10).

use chrono::Utc;
use serde::Serialize;
use sqlx::PgPool;

use super::cache::{node_stale_timeout_secs, TelemetryCache};

#[derive(Debug, Clone, Serialize)]
pub struct TelemetryResponse {
    pub measured: bool,
    pub vram_used: Option<i64>,
    pub vram_total: Option<i64>,
    pub cpu: Option<f64>,
    pub ram: Option<i64>,
    pub ram_total: Option<i64>,
    pub gpus: Vec<String>,
    pub jobs_active: i32,
}

/// Retorna telemetria do cache (agregação global).
pub async fn get_telemetry(pool: &PgPool, cache: &TelemetryCache) -> TelemetryResponse {
    let cache = cache.read().await;
    let now = Utc::now();

    // 0 nós no cache → fallback (comportamento atual: measured:false + jobs da fila).
    if cache.is_empty() {
        let jobs_active: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM jobs WHERE status NOT IN ('done', 'failed', 'cancelled')",
        )
        .fetch_one(pool)
        .await
        .unwrap_or((0,));

        return TelemetryResponse {
            measured: false,
            vram_used: None,
            vram_total: None,
            cpu: None,
            ram: None,
            ram_total: None,
            gpus: vec![],
            jobs_active: jobs_active.0 as i32,
        };
    }

    // 1 nó → exatamente o de hoje (compat total).
    if cache.len() == 1 {
        let state = cache.values().next().unwrap();
        let measured = state
            .last_heartbeat
            .map(|last| (now - last).num_seconds() <= node_stale_timeout_secs())
            .unwrap_or(false);
        // ADR D2.3/R5: nó sem heartbeat fresco → mesmo fallback do 0-nós.
        if !measured {
            let jobs_active: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM jobs WHERE status NOT IN ('done', 'failed', 'cancelled')",
            )
            .fetch_one(pool)
            .await
            .unwrap_or((0,));
            return TelemetryResponse {
                measured: false,
                vram_used: None,
                vram_total: None,
                cpu: None,
                ram: None,
                ram_total: None,
                gpus: vec![],
                jobs_active: jobs_active.0 as i32,
            };
        }
        return TelemetryResponse {
            measured,
            vram_used: state.vram_used,
            vram_total: state.vram_total,
            cpu: state.cpu,
            ram: state.ram,
            ram_total: state.ram_total,
            gpus: state.gpus.clone(),
            jobs_active: state.jobs_active,
        };
    }

    // >1 nós → agregação SOMENTE de entradas frescas (heartbeat ≤ 10s).
    // Entradas stale (offline ou sem heartbeat recente) são ignoradas na soma/união.
    // Se NENHUMA for fresca mas houver entradas → fallback (mesmo do 0-nós).
    let mut vram_used_sum: Option<i64> = Some(0);
    let mut vram_total_sum: Option<i64> = Some(0);
    let mut gpus: Vec<String> = Vec::new();
    let mut jobs_active_sum: i32 = 0;
    let mut measured = false;

    for state in cache.values() {
        let is_fresh = state
            .last_heartbeat
            .map(|last| (now - last).num_seconds() <= node_stale_timeout_secs())
            .unwrap_or(false);

        if is_fresh {
            measured = true;

            // vram_used/vram_total: soma dos Some (None → None global).
            match (vram_used_sum, state.vram_used) {
                (Some(acc), Some(val)) => vram_used_sum = Some(acc + val),
                (Some(_), None) => vram_used_sum = None,
                (None, _) => {}
            }
            match (vram_total_sum, state.vram_total) {
                (Some(acc), Some(val)) => vram_total_sum = Some(acc + val),
                (Some(_), None) => vram_total_sum = None,
                (None, _) => {}
            }

            // gpus: união (ordem estável por nó).
            for gpu in &state.gpus {
                if !gpus.contains(gpu) {
                    gpus.push(gpu.clone());
                }
            }

            // jobs_active: soma.
            jobs_active_sum += state.jobs_active;
        }
    }

    // Nenhuma entrada fresca → fallback (nulls + jobs da fila).
    if !measured {
        let jobs_active: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM jobs WHERE status NOT IN ('done', 'failed', 'cancelled')",
        )
        .fetch_one(pool)
        .await
        .unwrap_or((0,));

        return TelemetryResponse {
            measured: false,
            vram_used: None,
            vram_total: None,
            cpu: None,
            ram: None,
            ram_total: None,
            gpus: vec![],
            jobs_active: jobs_active.0 as i32,
        };
    }

    TelemetryResponse {
        measured: true,
        vram_used: vram_used_sum,
        vram_total: vram_total_sum,
        cpu: None,       // Sem média ponderada de CPU multi-nó na v1.
        ram: None,       // Sem soma de RAM multi-nó na v1.
        ram_total: None, // Sem soma de RAM total multi-nó na v1.
        gpus,
        jobs_active: jobs_active_sum,
    }
}
