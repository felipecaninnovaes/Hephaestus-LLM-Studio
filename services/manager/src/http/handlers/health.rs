//! Handlers de health, liveness, readiness e metrics do manager.

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};

use crate::http::state::AppState;

pub async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "service": "manager" }))
}

pub async fn ready(State(state): State<AppState>) -> Response {
    let db_ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .is_ok();

    if db_ok {
        (StatusCode::OK, Json(serde_json::json!({"status": "ready"}))).into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"status": "not ready"})),
        )
            .into_response()
    }
}

pub async fn metrics_handler(State(state): State<AppState>) -> Response {
    let pool_size = state.pool.size();
    let pool_idle = state.pool.num_idle();

    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT status, count(*) FROM jobs GROUP BY status")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

    let mut jobs_metrics = String::new();
    for (status, count) in rows {
        jobs_metrics.push_str(&format!(
            "hephaestus_manager_jobs_total{{status=\"{status}\"}} {count}\n"
        ));
    }

    let body = format!(
        "# HELP hephaestus_manager_up Service liveness\n\
         # TYPE hephaestus_manager_up gauge\n\
         hephaestus_manager_up 1\n\
         # HELP hephaestus_manager_db_pool_connections_total Total connections in DB pool\n\
         # TYPE hephaestus_manager_db_pool_connections_total gauge\n\
         hephaestus_manager_db_pool_connections_total {pool_size}\n\
         # HELP hephaestus_manager_db_pool_connections_idle Idle connections in DB pool\n\
         # TYPE hephaestus_manager_db_pool_connections_idle gauge\n\
         hephaestus_manager_db_pool_connections_idle {pool_idle}\n\
         # HELP hephaestus_manager_jobs_total Jobs count by status\n\
         # TYPE hephaestus_manager_jobs_total gauge\n\
         {jobs_metrics}"
    );

    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response()
}
