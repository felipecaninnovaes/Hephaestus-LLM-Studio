//! Construção e montagem do roteador HTTP (MM-06).

use axum::{
    middleware,
    routing::{delete, get, post},
    Router,
};
use tower_http::trace::TraceLayer;

use super::handlers::*;
use super::middleware::{auth_middleware, request_id_middleware};
use super::state::AppState;

pub fn build_router(state: AppState) -> Router {
    let api = Router::new()
        .route("/internal/jobs", post(create_job_handler).get(list_jobs_handler))
        .route(
            "/internal/jobs/:id",
            get(get_job_handler).delete(delete_job_handler),
        )
        .route("/internal/jobs/cleanup", post(cleanup_jobs_handler))
        .route("/internal/jobs/:id/artifacts", get(list_artifacts_handler))
        .route("/internal/jobs/:id/abort", post(abort_job_handler))
        .route("/internal/jobs/:id/report", post(report_job_handler))
        .route(
            "/internal/jobs/:id/prepare-complete",
            post(prepare_complete_handler),
        )
        .route(
            "/internal/jobs/:id/prepare-fail",
            post(prepare_fail_handler),
        )
        .route(
            "/internal/jobs/:id/prepare-cancel",
            post(prepare_cancel_handler),
        )
        .route("/internal/heartbeat", post(heartbeat_handler))
        .route("/internal/telemetry", get(telemetry_handler))
        .route("/internal/orchestrators", get(list_orchestrators_handler))
        .route("/internal/adopt", post(adopt_handler))
        .route("/internal/orchestrators/:id/revoke", post(revoke_handler))
        .route(
            "/internal/models",
            get(list_models_handler).post(create_model_handler),
        )
        .route(
            "/internal/models/:id",
            delete(delete_model_handler).patch(update_model_handler),
        )
        .route("/internal/storage/usage", get(get_storage_usage_handler))
        .route("/internal/generations", get(list_generations_handler))
        .route("/internal/generations/:id", get(get_generation_handler))
        .route(
            "/internal/generations/delete",
            post(delete_generations_handler),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/metrics", get(metrics_handler))
        .merge(api)
        .layer(middleware::from_fn(request_id_middleware))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
