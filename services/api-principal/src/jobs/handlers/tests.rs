use axum::{
    extract::{Path, Query},
    http::StatusCode,
    Json,
};

use super::*;
use crate::jobs::manager_client::{
    CreateJobResponse, InternalArtifact, InternalJob, InternalModel, MockManager,
};

#[test]
fn remap_metrics_camel_case() {
    let raw = serde_json::json!({
        "items": [
            {
                "epoch": 1,
                "box_loss": 0.5,
                "cls_loss": 0.3,
                "dfl_loss": 0.2,
                "mAP50": 0.8,
                "mAP50-95": 0.6
            },
            {
                "epoch": 2,
                "box_loss": 0.4,
                "cls_loss": 0.2,
                "dfl_loss": 0.1,
                "mAP50": 0.9,
                "mAP50-95": 0.7
            }
        ]
    });
    let items = remap_metrics(&raw);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].epoch, 1);
    assert_eq!(items[0].box_loss, 0.5);
    assert_eq!(items[0].cls_loss, 0.3);
    assert_eq!(items[0].dfl_loss, 0.2);
    assert_eq!(items[0].map50, 0.8);
    assert_eq!(items[0].map5095, 0.6);
    assert_eq!(items[1].epoch, 2);
    assert_eq!(items[1].map5095, 0.7);
}

#[test]
fn remap_metrics_empty_items() {
    let raw = serde_json::json!({ "items": [] });
    let items = remap_metrics(&raw);
    assert!(items.is_empty());
}

#[test]
fn remap_metrics_no_items_key() {
    let raw = serde_json::json!({});
    let items = remap_metrics(&raw);
    assert!(items.is_empty());
}

#[test]
fn remap_metrics_missing_epoch_skipped() {
    let raw = serde_json::json!({
        "items": [
            { "box_loss": 0.5, "cls_loss": 0.3, "dfl_loss": 0.2, "mAP50": 0.8, "mAP50-95": 0.6 }
        ]
    });
    let items = remap_metrics(&raw);
    assert!(items.is_empty());
}

#[test]
fn to_job_response_snake_to_camel() {
    let job = InternalJob {
        id: "550e8400-e29b-41d4-a716-446655440000".into(),
        kind: "yolo_train".into(),
        engine: "yolo".into(),
        model: "yolo11m".into(),
        mode: "train".into(),
        dataset_id: Some("550e8400-e29b-41d4-a716-446655440001".into()),
        status: "running".into(),
        queue_reason: None,
        queue_position: None,
        progress: Some(0.5),
        epoch: Some(5),
        step: Some(100),
        metrics: None,
        vram_min_gb: Some(4),
        orchestrator_id: Some("550e8400-e29b-41d4-a716-446655440002".into()),
        orchestrator_name: Some("node-gpu".into()),
        orchestrator_kind: Some("remoto".into()),
        orchestrator_fallback: true,
        created_at: "2026-01-01T00:00:00Z".into(),
        finished_at: None,
        error: None,
        params: None,
        phase: None,
        message: None,
    };
    let resp = to_job_response(job);
    assert_eq!(resp.id, "550e8400-e29b-41d4-a716-446655440000");
    assert_eq!(resp.kind, "yolo_train");
    assert_eq!(resp.status, "running");
    assert_eq!(resp.progress, Some(0.5));
    assert_eq!(resp.epoch, Some(5));
    assert_eq!(resp.orchestrator_name.as_deref(), Some("node-gpu"));
    assert_eq!(resp.orchestrator_kind.as_deref(), Some("remoto"));
    assert!(resp.orchestrator_fallback);
    assert!(resp.metrics.is_none());
}

#[test]
fn to_job_response_phase_from_columns() {
    // AC-006-A D4: wire phase/phaseMessage vêm das colunas do job.
    let job = InternalJob {
        status: "running".into(),
        phase: Some("loading_model".into()),
        message: Some("Carregando FLUX".into()),
        ..mock_job()
    };
    let resp = to_job_response(job);
    assert_eq!(resp.phase.as_deref(), Some("loading_model"));
    assert_eq!(resp.phase_message.as_deref(), Some("Carregando FLUX"));
}

#[test]
fn to_job_response_phase_fallback_status() {
    // AC-006-A D4: sem fase na coluna → fallback status-para-fase.
    let job = InternalJob {
        status: "running".into(),
        phase: None,
        message: None,
        ..mock_job()
    };
    let resp = to_job_response(job);
    assert_eq!(resp.phase.as_deref(), Some("running"));
    assert!(resp.phase_message.is_none());

    // P2-3: dispatched/cancelling retornam o status cru.
    let job = InternalJob {
        status: "dispatched".into(),
        phase: None,
        message: None,
        ..mock_job()
    };
    assert_eq!(to_job_response(job).phase.as_deref(), Some("dispatched"));
    let job = InternalJob {
        status: "cancelling".into(),
        phase: None,
        message: None,
        ..mock_job()
    };
    assert_eq!(to_job_response(job).phase.as_deref(), Some("cancelling"));
}

#[test]
fn validate_artifact_path_rejects_dotdot() {
    assert!(validate_artifact_path("../etc/passwd").is_err());
}

#[test]
fn validate_artifact_path_rejects_absolute() {
    assert!(validate_artifact_path("/etc/passwd").is_err());
}

#[test]
fn validate_artifact_path_rejects_backslash() {
    assert!(validate_artifact_path("..\\windows").is_err());
}

#[test]
fn validate_artifact_path_accepts_normal() {
    assert!(validate_artifact_path("best.pt").is_ok());
    assert!(validate_artifact_path("subdir/best.pt").is_ok());
}

#[tokio::test]
async fn list_jobs_handler_200() {
    let mut mock = MockManager::default();
    mock.list_jobs_result = Some((vec![mock_job()], 1));
    let state = test_state(mock);
    let resp = list_jobs(
        axum::extract::State(state),
        Query(JobsQuery {
            status: None,
            engine: None,
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn list_jobs_handler_503_manager_offline() {
    let mut mock = MockManager::default();
    mock.fail = true;
    let state = test_state(mock);
    let resp = list_jobs(
        axum::extract::State(state),
        Query(JobsQuery {
            status: None,
            engine: None,
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn list_queue_handler_200() {
    let mut mock = MockManager::default();
    mock.list_queue_result = Some(vec![crate::jobs::manager_client::InternalQueueItem {
        job_id: "550e8400-e29b-41d4-a716-446655440000".into(),
        position: 1,
        queue_reason: None,
    }]);
    let state = test_state(mock);
    let resp = list_queue(axum::extract::State(state)).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn get_job_handler_200() {
    let mut mock = MockManager::default();
    mock.get_job_result = Some(mock_job());
    let state = test_state(mock);
    let resp = get_job(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn get_job_handler_404_non_uuid() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = get_job(axum::extract::State(state), Path("not-a-uuid".to_string())).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_job_handler_404_not_found() {
    let mut mock = MockManager::default();
    mock.get_job_result = None; // NotFound
    let state = test_state(mock);
    let resp = get_job(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_job_handler_503_manager_offline() {
    let mut mock = MockManager::default();
    mock.fail = true;
    let state = test_state(mock);
    let resp = get_job(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn get_job_metrics_200_empty() {
    let mut mock = MockManager::default();
    let mut job = mock_job();
    job.metrics = None;
    mock.get_job_result = Some(job);
    let state = test_state(mock);
    let resp = get_job_metrics(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        axum::extract::Query(Default::default()),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn get_job_metrics_200_with_data() {
    let mut mock = MockManager::default();
    let mut job = mock_job();
    job.metrics = Some(serde_json::json!({
        "items": [{
            "epoch": 1,
            "box_loss": 0.5,
            "cls_loss": 0.3,
            "dfl_loss": 0.2,
            "mAP50": 0.8,
            "mAP50-95": 0.6
        }]
    }));
    mock.get_job_result = Some(job);
    let state = test_state(mock);
    let resp = get_job_metrics(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        axum::extract::Query(Default::default()),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

/// maxPoints fora de [1,10000] ⇒ 400 invalid_max_points (0, negativo, 10001).
#[tokio::test]
async fn get_job_metrics_max_points_fora_da_faixa_400() {
    for bad in [0_i64, -1, 10_001] {
        let mut mock = MockManager::default();
        mock.get_job_result = Some(mock_job());
        let state = test_state(mock);
        let resp = get_job_metrics(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
            axum::extract::Query(JobMetricsQuery {
                after_seq: None,
                max_points: Some(bad),
                keys: None,
            }),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "maxPoints={bad} deveria ser 400"
        );
    }
}

/// maxPoints no limite (1 e 10000) ⇒ aceito, proxy para o manager.
#[tokio::test]
async fn get_job_metrics_max_points_limites_aceitos() {
    for ok in [1_i64, 10_000] {
        let mut mock = MockManager::default();
        mock.metric_points_result = Some(heph_contracts::telemetry::MetricPointsResponse {
            items: vec![],
            max_seq: 0,
            downsampled: false,
        });
        let state = test_state(mock);
        let resp = get_job_metrics(
            axum::extract::State(state),
            Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
            axum::extract::Query(JobMetricsQuery {
                after_seq: None,
                max_points: Some(ok),
                keys: None,
            }),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "maxPoints={ok} deveria ser 200"
        );
    }
}

#[tokio::test]
async fn list_artifacts_handler_200() {
    let mut mock = MockManager::default();
    mock.list_artifacts_result = Some(vec![InternalArtifact {
        id: "550e8400-e29b-41d4-a716-446655440003".into(),
        kind: "model".into(),
        path: "best.pt".into(),
        md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 1024,
    }]);
    let state = test_state(mock);
    let resp = list_artifacts(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn get_telemetry_handler_200() {
    let mut mock = MockManager::default();
    mock.get_telemetry_result = Some(crate::jobs::manager_client::InternalTelemetry {
        measured: false,
        vram_used: None,
        vram_total: None,
        cpu: Some(0.5),
        ram: Some(1024),
        ram_total: None,
        gpus: vec![],
        jobs_active: 0,
    });
    let state = test_state(mock);
    let resp = get_telemetry(axum::extract::State(state)).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn get_telemetry_handler_503_manager_offline() {
    let mut mock = MockManager::default();
    mock.fail = true;
    let state = test_state(mock);
    let resp = get_telemetry(axum::extract::State(state)).await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn get_telemetry_handler_ram_total() {
    let mut mock = MockManager::default();
    mock.get_telemetry_result = Some(crate::jobs::manager_client::InternalTelemetry {
        measured: true,
        vram_used: None,
        vram_total: None,
        cpu: Some(0.5),
        ram: Some(1024),
        ram_total: Some(8_000_000_000),
        gpus: vec![],
        jobs_active: 0,
    });
    let state = test_state(mock);
    let resp = get_telemetry(axum::extract::State(state)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["ramTotal"], serde_json::json!(8_000_000_000_i64));
}

// --- POST /api/jobs/yolo unit tests (F4.2b) ---

#[tokio::test]
async fn submit_yolo_job_400_empty_body() {
    let mock = MockManager::default();
    let state = test_state(mock);
    // Empty bytes → invalid JSON → 400.
    let resp = submit_yolo_job(axum::extract::State(state), Ok(axum::body::Bytes::from(""))).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_yolo_job_400_invalid_body() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_yolo_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(r#"{"invalid"}"#)),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_yolo_job_400_unknown_fields() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_yolo_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"datasetId":"00000000-0000-0000-0000-000000000000","extra":1}"#,
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_yolo_job_400_invalid_model() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_yolo_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"datasetId":"00000000-0000-0000-0000-000000000000","model":"resnet50"}"#,
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_yolo_job_404_non_uuid() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_yolo_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(r#"{"datasetId":"not-a-uuid"}"#)),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn submit_yolo_job_503_manager_offline() {
    let mut mock = MockManager::default();
    mock.fail = true;
    let state = test_state(mock);
    // Note: this will 503 at the manager call because the dataset doesn't exist
    // but the mock will fail first.
    let resp = submit_yolo_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"datasetId":"00000000-0000-0000-0000-000000000000"}"#,
        )),
    )
    .await;
    // With connect_lazy pool, dataset check will fail → 500 internal,
    // but if mock.fail is true the manager will 503.
    // The actual status depends on whether the pool check succeeds.
    assert!(
        resp.status() == StatusCode::SERVICE_UNAVAILABLE
            || resp.status() == StatusCode::INTERNAL_SERVER_ERROR,
        "expected 503 or 500, got {}",
        resp.status()
    );
}

// --- POST /api/jobs/yolo weights tests (D5 ADR-0012) ---

#[tokio::test]
async fn submit_yolo_job_400_weights_not_uuid() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_yolo_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"datasetId":"00000000-0000-0000-0000-000000000000","weights":"not-a-uuid"}"#,
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_yolo_job_400_orchestrator_id_not_uuid() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_yolo_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"datasetId":"00000000-0000-0000-0000-000000000000","model":"yolo11n","epochs":10,"batch":16,"orchestratorId":"not-a-uuid"}"#,
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_yolo_job_weights_uuid_valid() {
    // weights válido: passa validação pura (sem DB = 503 ou not_found no dataset check).
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_yolo_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"datasetId":"00000000-0000-0000-0000-000000000000","weights":"550e8400-e29b-41d4-a716-446655440000"}"#,
        )),
    )
    .await;
    // Com pool lazy, dataset check falha → 500 ou not_found dependendo do timing.
    // O importante é que NÃO é 400 (weights UUID é válido).
    assert_ne!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "weights UUID should not trigger 400"
    );
}

#[tokio::test]
async fn submit_yolo_job_extra_key_rejected_with_deny_unknown_fields() {
    // confirmar que deny_unknown_fields funciona (mock tolera extras no yaml
    // mas o serde rejeita no parse do body).
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_yolo_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"datasetId":"00000000-0000-0000-0000-000000000000","extraKey":"value"}"#,
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// --- POST /api/jobs/:id/abort unit tests (F4.2b) ---

// --- POST /api/jobs/autotracker unit tests (ADR-0008 A.2) ---

#[tokio::test]
async fn submit_autotracker_job_400_empty_body() {
    let mock = MockManager::default();
    let state = test_state(mock);
    // Empty bytes → invalid JSON → 400.
    let resp =
        submit_autotracker_job(axum::extract::State(state), Ok(axum::body::Bytes::from(""))).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_autotracker_job_400_invalid_body() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_autotracker_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(r#"{"invalid"}"#)),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_autotracker_job_400_unknown_fields() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_autotracker_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"datasetId":"00000000-0000-0000-0000-000000000000","extra":1}"#,
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_autotracker_job_400_invalid_model() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_autotracker_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"datasetId":"00000000-0000-0000-0000-000000000000","model":"resnet50"}"#,
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_autotracker_job_404_non_uuid() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_autotracker_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(r#"{"datasetId":"not-a-uuid"}"#)),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// --- POST /api/jobs/autotracker modelId tests (ADR-0014 D2/D6) ---

#[tokio::test]
async fn submit_autotracker_job_202_no_model_id_mock() {
    // Sem modelId → mock, comportamento atual intocado.
    // Com pool lazy, vai falhar no DB antes de reach manager (500).
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_autotracker_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"datasetId":"00000000-0000-0000-0000-000000000000"}"#,
        )),
    )
    .await;
    // Com pool lazy, vai falhar no DB (500) — mas NÃO deve ser 400.
    assert_ne!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "valid body without modelId should not trigger 400"
    );
}

#[tokio::test]
async fn submit_autotracker_job_202_with_valid_model_id() {
    // Com modelId válido → passa validação (não retorna 400).
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_autotracker_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"datasetId":"00000000-0000-0000-0000-000000000000","modelId":"550e8400-e29b-41d4-a716-446655440000"}"#,
        )),
    )
    .await;
    // Com pool lazy, vai falhar no DB (500) — mas NÃO deve ser 400.
    assert_ne!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "valid body with modelId should not trigger 400"
    );
}

#[tokio::test]
async fn submit_autotracker_job_400_model_id_not_uuid() {
    // modelId não-UUID → 400 `invalid_request`.
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_autotracker_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"datasetId":"00000000-0000-0000-0000-000000000000","modelId":"not-a-uuid"}"#,
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_autotracker_job_400_orchestrator_id_not_uuid() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_autotracker_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"datasetId":"00000000-0000-0000-0000-000000000000","orchestratorId":"not-a-uuid"}"#,
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_autotracker_job_manager_not_found_compensates() {
    // ADR-0014 D6: manager NotFound → 404 + compensação.
    // NOTA: test_state usa pool lazy que falha no DB ANTES de reach create_job.
    // O mapeamento NotFound→404 é coberto pelo test-db; este teste valida o
    // caminho de falha do pool (500), não o mapeamento do manager.
    let mut mock = MockManager::default();
    mock.create_job_not_found = true;
    let state = test_state(mock);
    let resp = submit_autotracker_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"datasetId":"00000000-0000-0000-0000-000000000000","modelId":"550e8400-e29b-41d4-a716-446655440000"}"#,
        )),
    )
    .await;
    // Pool lazy falha no DB → 500 (não 404 nem 400).
    assert_eq!(
        resp.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "lazy pool DB failure should return 500"
    );
}

#[tokio::test]
async fn submit_autotracker_job_manager_invalid_request_compensates() {
    // ADR-0014 D6: manager InvalidRequest → 400 + compensação.
    // NOTA: test_state usa pool lazy que falha no DB ANTES de reach create_job.
    // O mapeamento InvalidRequest→400 é coberto pelo test-db; este teste valida o
    // caminho de falha do pool (500), não o mapeamento do manager.
    let mut mock = MockManager::default();
    mock.create_job_invalid_request = Some("engine mismatch".into());
    let state = test_state(mock);
    let resp = submit_autotracker_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"datasetId":"00000000-0000-0000-0000-000000000000","modelId":"550e8400-e29b-41d4-a716-446655440000"}"#,
        )),
    )
    .await;
    // Pool lazy falha no DB → 500 (não 400 nem 503).
    assert_eq!(
        resp.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "lazy pool DB failure should return 500"
    );
}

#[tokio::test]
async fn submit_autotracker_job_manager_fail_compensates() {
    // ADR-0014 D6: manager Unavailable → 503 + compensação.
    // NOTA: test_state usa pool lazy que falha no DB ANTES de reach create_job.
    // O mapeamento Unavailable→503 é coberto pelo test-db; este teste valida o
    // caminho de falha do pool (500), não o mapeamento do manager.
    let mut mock = MockManager::default();
    mock.fail = true;
    let state = test_state(mock);
    let resp = submit_autotracker_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"datasetId":"00000000-0000-0000-0000-000000000000","modelId":"550e8400-e29b-41d4-a716-446655440000"}"#,
        )),
    )
    .await;
    // Pool lazy falha no DB → 500 (não 503).
    assert_eq!(
        resp.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "lazy pool DB failure should return 500"
    );
}

// --- POST /api/jobs/predict unit tests (Fatia J — ADR-0013 D0/D1/D8) ---

#[tokio::test]
async fn submit_predict_job_400_empty_body() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp =
        submit_predict_job(axum::extract::State(state), Ok(axum::body::Bytes::from(""))).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_predict_job_400_invalid_body() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_predict_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(r#"{"invalid"}"#)),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_predict_job_400_unknown_fields() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_predict_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"modelId":"00000000-0000-0000-0000-000000000000","datasetId":"00000000-0000-0000-0000-000000000001","extra":1}"#,
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_predict_job_400_model_id_not_uuid() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_predict_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"modelId":"not-a-uuid","datasetId":"00000000-0000-0000-0000-000000000001"}"#,
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_predict_job_400_orchestrator_id_not_uuid() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_predict_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"modelId":"00000000-0000-0000-0000-000000000000","datasetId":"00000000-0000-0000-0000-000000000001","orchestratorId":"not-a-uuid"}"#,
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_predict_job_400_conf_out_of_range() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_predict_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"modelId":"00000000-0000-0000-0000-000000000000","datasetId":"00000000-0000-0000-0000-000000000001","conf":1.5}"#,
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_predict_job_404_dataset_id_non_uuid() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_predict_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"modelId":"00000000-0000-0000-0000-000000000000","datasetId":"not-a-uuid"}"#,
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn submit_predict_job_valid_body_passes_validation() {
    // Prova que body com modelId UUID + conf válido passa validação (não retorna 400).
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_predict_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"modelId":"550e8400-e29b-41d4-a716-446655440000","datasetId":"550e8400-e29b-41d4-a716-446655440001"}"#,
        )),
    )
    .await;
    // Com pool lazy, vai falhar no DB (500) — mas NÃO deve ser 400.
    assert_ne!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "valid body should not trigger 400"
    );
}

#[tokio::test]
async fn submit_predict_job_valid_body_with_custom_conf() {
    // Prova que body com conf customizado passa validação.
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_predict_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"modelId":"550e8400-e29b-41d4-a716-446655440000","datasetId":"550e8400-e29b-41d4-a716-446655440001","conf":0.9}"#,
        )),
    )
    .await;
    assert_ne!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "valid body with conf=0.9 should not trigger 400"
    );
}

// --- POST /api/jobs/predict compensation tests (A1 — Fatia J review J.6) ---

#[tokio::test]
async fn submit_predict_job_manager_not_found_compensates() {
    // A1: manager NotFound → 404 + compensação (delete_prefix chamado).
    let mut mock = MockManager::default();
    mock.create_job_not_found = true;
    let state = test_state(mock);
    let resp = submit_predict_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"modelId":"550e8400-e29b-41d4-a716-446655440000","datasetId":"550e8400-e29b-41d4-a716-446655440001"}"#,
        )),
    )
    .await;
    // Com pool lazy, vai falhar no DB antes de reach create_job (500).
    // NÃO deve ser 400.
    assert_ne!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "manager NotFound should not trigger 400"
    );
}

#[tokio::test]
async fn submit_predict_job_manager_invalid_request_compensates() {
    // A1: manager InvalidRequest → 400 + compensação (delete_prefix chamado).
    let mut mock = MockManager::default();
    mock.create_job_invalid_request = Some("engine mismatch".into());
    let state = test_state(mock);
    let resp = submit_predict_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"modelId":"550e8400-e29b-41d4-a716-446655440000","datasetId":"550e8400-e29b-41d4-a716-446655440001"}"#,
        )),
    )
    .await;
    assert_ne!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "manager InvalidRequest should not trigger 400 at validation"
    );
}

#[tokio::test]
async fn submit_predict_job_manager_fail_compensates() {
    // A1: manager Unavailable (fail=true) → 503 + compensação (delete_prefix chamado).
    let mut mock = MockManager::default();
    mock.fail = true;
    let state = test_state(mock);
    let resp = submit_predict_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"modelId":"550e8400-e29b-41d4-a716-446655440000","datasetId":"550e8400-e29b-41d4-a716-446655440001"}"#,
        )),
    )
    .await;
    assert!(
        resp.status() == StatusCode::SERVICE_UNAVAILABLE
            || resp.status() == StatusCode::INTERNAL_SERVER_ERROR,
        "expected 503 or 500, got {}",
        resp.status()
    );
}

// --- POST /api/jobs/:id/abort unit tests (F4.2b) ---

#[tokio::test]
async fn abort_job_404_non_uuid() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = abort_job(axum::extract::State(state), Path("not-a-uuid".to_string())).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn abort_job_200() {
    let mut mock = MockManager::default();
    mock.abort_job_result = Some(crate::jobs::manager_client::AbortJobResponse {
        job_id: None,
        status: "cancelling".to_string(),
    });
    let state = test_state(mock);
    let resp = abort_job(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn abort_job_404_manager_not_found() {
    let mock = MockManager::default(); // abort_job_result = None → NotFound
    let state = test_state(mock);
    let resp = abort_job(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn abort_job_503_manager_offline() {
    let mut mock = MockManager::default();
    mock.fail = true;
    let state = test_state(mock);
    let resp = abort_job(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

// =========================================================================
// apply_autotracker_boxes tests (ADR-0008 D1)
// =========================================================================

fn autotracker_job_done() -> InternalJob {
    InternalJob {
        id: "550e8400-e29b-41d4-a716-446655440000".into(),
        kind: "autotracker".into(),
        engine: "autotracker".into(),
        model: "mock".into(),
        mode: "autotrack".into(),
        dataset_id: Some("550e8400-e29b-41d4-a716-446655440001".into()),
        status: "done".into(),
        queue_reason: None,
        queue_position: None,
        progress: Some(1.0),
        epoch: None,
        step: None,
        metrics: None,
        vram_min_gb: None,
        orchestrator_id: None,
        orchestrator_name: None,
        orchestrator_kind: None,
        orchestrator_fallback: false,
        created_at: "2026-01-01T00:00:00Z".into(),
        finished_at: Some("2026-01-01T01:00:00Z".into()),
        error: None,
        params: None,
        phase: None,
        message: None,
    }
}

#[allow(dead_code)]
fn boxes_artifact() -> InternalArtifact {
    let json_data = br#"{"engine":"autotracker","model":"mock","seed":42,"conf":0.65,"images":[{"filename":"img_0001.jpg","boxes":[{"class":"solda_fria","x":0.1,"y":0.2,"w":0.3,"h":0.4,"conf":0.96}]}]}"#;
    let md5 = format!(
        "{:x}",
        md5::Digest::finalize({
            use md5::Digest;
            let mut h = md5::Md5::new();
            md5::Digest::update(&mut h, json_data);
            h
        })
    );
    InternalArtifact {
        id: "aaaa-bbbb-cccc-dddd".into(),
        kind: "boxes".into(),
        path: "boxes.json".into(),
        md5,
        bytes: json_data.len() as i64,
    }
}

#[tokio::test]
async fn apply_404_non_uuid() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = apply_autotracker_boxes(
        axum::extract::State(state),
        Path("nao-e-uuid".to_string()),
        Ok(axum::body::Bytes::from_static(b"{}")),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn apply_400_empty_body() {
    let mut mock = MockManager::default();
    mock.get_job_result = Some(autotracker_job_done());
    let state = test_state(mock);
    // Empty bytes fail JSON parse → 400 invalid_request.
    let resp = apply_autotracker_boxes(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        Ok(axum::body::Bytes::new()),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn apply_400_invalid_body() {
    let mut mock = MockManager::default();
    mock.get_job_result = Some(autotracker_job_done());
    let state = test_state(mock);
    let resp = apply_autotracker_boxes(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        Ok(axum::body::Bytes::from_static(b"not json")),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn apply_400_unknown_fields() {
    let mut mock = MockManager::default();
    mock.get_job_result = Some(autotracker_job_done());
    let state = test_state(mock);
    let resp = apply_autotracker_boxes(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        Ok(axum::body::Bytes::from_static(b"{\"extra\":1}")),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn apply_404_not_autotracker_engine() {
    let mut mock = MockManager::default();
    let mut job = autotracker_job_done();
    job.engine = "yolo".into();
    mock.get_job_result = Some(job);
    let state = test_state(mock);
    let resp = apply_autotracker_boxes(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        Ok(axum::body::Bytes::from_static(b"{}")),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn apply_409_job_not_done_running() {
    let mut mock = MockManager::default();
    let mut job = autotracker_job_done();
    job.status = "running".into();
    mock.get_job_result = Some(job);
    let state = test_state(mock);
    let resp = apply_autotracker_boxes(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        Ok(axum::body::Bytes::from_static(b"{}")),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "job_not_done");
}

#[tokio::test]
async fn apply_409_job_not_done_queued() {
    let mut mock = MockManager::default();
    let mut job = autotracker_job_done();
    job.status = "queued".into();
    mock.get_job_result = Some(job);
    let state = test_state(mock);
    let resp = apply_autotracker_boxes(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        Ok(axum::body::Bytes::from_static(b"{}")),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "job_not_done");
}

#[tokio::test]
async fn apply_409_dataset_not_ready_null() {
    let mut mock = MockManager::default();
    let mut job = autotracker_job_done();
    job.dataset_id = None;
    mock.get_job_result = Some(job);
    let state = test_state(mock);
    let resp = apply_autotracker_boxes(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        Ok(axum::body::Bytes::from_static(b"{}")),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "dataset_not_ready");
}

#[tokio::test]
async fn apply_503_manager_offline() {
    let mut mock = MockManager::default();
    mock.fail = true;
    let state = test_state(mock);
    let resp = apply_autotracker_boxes(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        Ok(axum::body::Bytes::from_static(b"{}")),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn apply_404_job_not_found() {
    let mock = MockManager::default(); // get_job_result = None → NotFound
    let state = test_state(mock);
    let resp = apply_autotracker_boxes(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        Ok(axum::body::Bytes::from_static(b"{}")),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn apply_404_no_boxes_artifact() {
    let mut mock = MockManager::default();
    mock.get_job_result = Some(autotracker_job_done());
    mock.list_artifacts_result = Some(vec![]); // no boxes artifact
    let state = test_state(mock);
    let resp = apply_autotracker_boxes(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        Ok(axum::body::Bytes::from_static(b"{}")),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn apply_image_id_non_uuid() {
    let mut mock = MockManager::default();
    mock.get_job_result = Some(autotracker_job_done());
    let state = test_state(mock);
    let resp = apply_autotracker_boxes(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        Ok(axum::body::Bytes::from_static(
            b"{\"imageId\":\"not-uuid\"}",
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// --- Autolabel unit tests (ADR-0016 D0/D1) ---

#[tokio::test]
async fn submit_autolabel_job_400_empty_body() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp =
        submit_autolabel_job(axum::extract::State(state), Ok(axum::body::Bytes::from(""))).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_autolabel_job_400_unknown_fields() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_autolabel_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from_static(
            br#"{"datasetId":"550e8400-e29b-41d4-a716-446655440000","extra":1}"#,
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_autolabel_job_400_invalid_model() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_autolabel_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from_static(
            br#"{"datasetId":"550e8400-e29b-41d4-a716-446655440000","model":"gpt-4"}"#,
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_autolabel_job_404_non_uuid() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_autolabel_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from_static(
            br#"{"datasetId":"nao-eh-uuid"}"#,
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn submit_autolabel_job_400_orchestrator_id_not_uuid() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_autolabel_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from_static(
            br#"{"datasetId":"550e8400-e29b-41d4-a716-446655440000","orchestratorId":"bad-uuid"}"#,
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn apply_autolabel_captions_404_non_uuid() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = apply_autolabel_captions(
        axum::extract::State(state),
        Path("nao-eh-uuid".to_string()),
        Ok(axum::body::Bytes::from_static(b"{}")),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn apply_autolabel_captions_400_unknown_fields() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = apply_autolabel_captions(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        Ok(axum::body::Bytes::from_static(b"{\"unknown\":true}")),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn preview_autolabel_captions_404_non_uuid() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp =
        preview_autolabel_captions(axum::extract::State(state), Path("nao-eh-uuid".to_string()))
            .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn preview_autolabel_captions_404_job_not_found() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = preview_autolabel_captions(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// --- helpers ---

fn mock_job() -> InternalJob {
    InternalJob {
        id: "550e8400-e29b-41d4-a716-446655440000".into(),
        kind: "yolo_train".into(),
        engine: "yolo".into(),
        model: "yolo11m".into(),
        mode: "train".into(),
        dataset_id: None,
        status: "queued".into(),
        queue_reason: None,
        queue_position: None,
        progress: None,
        epoch: None,
        step: None,
        metrics: None,
        vram_min_gb: None,
        orchestrator_id: None,
        orchestrator_name: None,
        orchestrator_kind: None,
        orchestrator_fallback: false,
        created_at: "2026-01-01T00:00:00Z".into(),
        finished_at: None,
        error: None,
        params: None,
        phase: None,
        message: None,
    }
}

fn test_state(manager: MockManager) -> crate::state::AppState {
    crate::state::AppState {
        pool: sqlx::PgPool::connect_lazy("postgres://n/n").expect("lazy"),
        jwt_secret: [0x42; 32],
        secure_cookie: false,
        setup_required: false,
        storage: std::sync::Arc::new(crate::storage::MockStorage::new()),
        storage_config: crate::storage::StorageConfig {
            bucket: "heph-test".into(),
            public_endpoint: None,
            url_ttl_secs: 60,
        },
        embedder: std::sync::Arc::new(crate::search::MockEmbedder::new()),
        embedding_model: "ViT-B-32".to_string(),
        manager: std::sync::Arc::new(manager),
        model_download_allowed_hosts: vec![],
        job_events: crate::jobs::events_hub::JobEventsHub::new(),
    }
}

// =========================================================================
// Diffusion Generate v2 handler tests (ADR-0023 D2/D3/D4)
// =========================================================================

#[tokio::test]
async fn submit_diffusion_generate_202_with_batch_and_loras() {
    let uuid_lora = "550e8400-e29b-41d4-a716-446655440000";
    let body_json = serde_json::json!({
        "prompt": "a test prompt",
        "batchSize": 4,
        "loras": [{"modelId": uuid_lora, "scale": 0.8}],
        "baseModel": "sdxl"
    });

    let mut mock = MockManager::default();
    mock.create_job_result = Some(CreateJobResponse {
        job_id: "job-123".into(),
        status: "queued".into(),
        queue_position: Some(1),
    });
    let state = test_state(mock);

    let resp = submit_diffusion_generate_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            serde_json::to_string(&body_json).unwrap(),
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
}

#[tokio::test]
async fn submit_diffusion_generate_params_contain_batch_size_and_loras() {
    let uuid_lora = "550e8400-e29b-41d4-a716-446655440000";
    let body_json = serde_json::json!({
        "prompt": "test",
        "batchSize": 2,
        "loras": [{"modelId": uuid_lora, "scale": 1.0}],
        "baseModel": "flux-2-klein-4b"
    });

    let mut mock = MockManager::default();
    mock.create_job_result = Some(CreateJobResponse {
        job_id: "job-456".into(),
        status: "queued".into(),
        queue_position: None,
    });
    // Guarda referência para verificar body depois
    let mock_arc = std::sync::Arc::new(mock);
    let mock_ref = std::sync::Arc::clone(&mock_arc);
    let mut state = test_state(MockManager::default());
    state.manager = mock_arc;

    let resp = submit_diffusion_generate_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            serde_json::to_string(&body_json).unwrap(),
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    // Verifica params camelCase no body enviado ao manager
    let body = mock_ref.last_create_job_body();
    let body = body.expect("create_job body captured");
    let params = body["params"].as_object().expect("params object");
    assert_eq!(params.get("batchSize"), Some(&serde_json::json!(2)));
    assert!(params.get("loras").is_some(), "missing loras in params");
    assert!(
        params.get("customModelId").is_some(),
        "missing customModelId in params"
    );
}

#[tokio::test]
async fn submit_diffusion_generate_vram_min_custom_sdxl() {
    use crate::jobs::manager_client::InternalModel;

    let custom_id = "550e8400-e29b-41d4-a716-446655440099";
    let body_json = serde_json::json!({
        "prompt": "test",
        "customModelId": custom_id,
        "quantization": "4bit"
    });

    let mut mock = MockManager::default();
    mock.list_models_result = Some(vec![InternalModel {
        id: custom_id.into(),
        name: "my-sdxl.safetensors".into(),
        engine: "diffusion".into(),
        model: None,
        source: "upload".into(),
        md5: "abc123".into(),
        bytes: 6_500_000_000,
        path: "models/diffusion/custom/my-sdxl.safetensors".into(),
        job_id: None,
        created_at: "2026-09-15T00:00:00Z".into(),
        kind: Some("checkpoint".into()),
        arch: Some("sdxl".into()),
    }]);
    mock.create_job_result = Some(CreateJobResponse {
        job_id: "job-789".into(),
        status: "queued".into(),
        queue_position: None,
    });
    let mock_arc = std::sync::Arc::new(mock);
    let mock_ref = std::sync::Arc::clone(&mock_arc);
    let mut state = test_state(MockManager::default());
    state.manager = mock_arc;

    let resp = submit_diffusion_generate_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            serde_json::to_string(&body_json).unwrap(),
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    // vram_min para sdxl + 4bit = 8 (espelhamento first-class)
    let body = mock_ref.last_create_job_body();
    let body = body.expect("create_job body captured");
    assert_eq!(body["vram_min_gb"], 8);
}

#[tokio::test]
async fn submit_diffusion_generate_custom_not_found_400() {
    let body_json = serde_json::json!({
        "prompt": "test",
        "customModelId": "550e8400-e29b-41d4-a716-446655440099"
    });

    let mut mock = MockManager::default();
    // Lista vazia — modelo não encontrado
    mock.list_models_result = Some(vec![]);
    let state = test_state(mock);

    let resp = submit_diffusion_generate_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            serde_json::to_string(&body_json).unwrap(),
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_diffusion_generate_custom_not_checkpoint_400() {
    let custom_id = "550e8400-e29b-41d4-a716-446655440099";
    let body_json = serde_json::json!({
        "prompt": "test",
        "customModelId": custom_id
    });

    let mut mock = MockManager::default();
    mock.list_models_result = Some(vec![InternalModel {
        id: custom_id.into(),
        name: "my-lora.safetensors".into(),
        engine: "diffusion".into(),
        model: None,
        source: "upload".into(),
        md5: "abc123".into(),
        bytes: 100_000,
        path: "models/diffusion/lora/my-lora.safetensors".into(),
        job_id: None,
        created_at: "2026-09-15T00:00:00Z".into(),
        kind: Some("lora".into()), // kind=lora, não checkpoint
        arch: Some("sdxl".into()),
    }]);
    let state = test_state(mock);

    let resp = submit_diffusion_generate_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            serde_json::to_string(&body_json).unwrap(),
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_diffusion_generate_custom_unsupported_arch_400() {
    // Fatia feat/pesos-custom-flux2: flux-2-klein-4b custom AGORA é aceito
    // (transformer swap); arch verdadeiramente desconhecida ⇒ 400
    // `unsupported_architecture`. Teste legado atualizado.
    let custom_id = "550e8400-e29b-41d4-a716-446655440099";
    let body_json = serde_json::json!({
        "prompt": "test",
        "customModelId": custom_id
    });

    let mut mock = MockManager::default();
    mock.list_models_result = Some(vec![InternalModel {
        id: custom_id.into(),
        name: "my-weird.safetensors".into(),
        engine: "diffusion".into(),
        model: None,
        source: "upload".into(),
        md5: "abc123".into(),
        bytes: 6_500_000_000,
        path: "models/diffusion/custom/my-weird.safetensors".into(),
        job_id: None,
        created_at: "2026-09-15T00:00:00Z".into(),
        kind: Some("checkpoint".into()),
        arch: Some("pixart".into()), // arch desconhecida
    }]);
    let state = test_state(mock);

    let resp = submit_diffusion_generate_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            serde_json::to_string(&body_json).unwrap(),
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    // Verifica code "unsupported_architecture"
    let (parts, body) = resp.into_parts();
    let _ = parts;
    let body_bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["code"], "unsupported_architecture");
}
#[tokio::test]
async fn submit_diffusion_generate_custom_flux2_202() {
    // Fatia feat/pesos-custom-flux2: checkpoint flux-2-klein-4b ⇒ 202,
    // arch efetivo flux-2-klein-4b no body do manager.
    let custom_id = "550e8400-e29b-41d4-a716-446655440099";
    let body_json = serde_json::json!({
        "prompt": "test",
        "customModelId": custom_id,
        "quantization": "4bit"
    });

    let mut mock = MockManager::default();
    mock.list_models_result = Some(vec![InternalModel {
        id: custom_id.into(),
        name: "my-flux2.safetensors".into(),
        engine: "diffusion".into(),
        model: None,
        source: "upload".into(),
        md5: "abc123".into(),
        bytes: 6_500_000_000,
        path: "models/diffusion/custom/my-flux2.safetensors".into(),
        job_id: None,
        created_at: "2026-09-15T00:00:00Z".into(),
        kind: Some("checkpoint".into()),
        arch: Some("flux-2-klein-4b".into()),
    }]);
    mock.create_job_result = Some(CreateJobResponse {
        job_id: "job-flux2".into(),
        status: "queued".into(),
        queue_position: None,
    });
    let mock_arc = std::sync::Arc::new(mock);
    let mock_ref = std::sync::Arc::clone(&mock_arc);
    let mut state = test_state(MockManager::default());
    state.manager = mock_arc;

    let resp = submit_diffusion_generate_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            serde_json::to_string(&body_json).unwrap(),
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let body = mock_ref.last_create_job_body();
    let body = body.expect("create_job body captured");
    assert_eq!(body["model"], "flux-2-klein-4b");
    // flux-2 + 4bit = 8 (mesma tabela first-class).
    assert_eq!(body["vram_min_gb"], 8);
}

// =========================================================================
// Diffusion Generate img2img handler tests (fatia feat/img2img)
// =========================================================================

#[tokio::test]
async fn submit_diffusion_generate_202_forwards_init_image_id_and_strength() {
    let init_id = "550e8400-e29b-41d4-a716-446655440010";
    let body_json = serde_json::json!({
        "prompt": "img2img test",
        "baseModel": "sdxl",
        "initImageId": init_id,
        "initStrength": 0.8
    });

    let mut mock = MockManager::default();
    mock.create_job_result = Some(CreateJobResponse {
        job_id: "job-img2img".into(),
        status: "queued".into(),
        queue_position: None,
    });
    let mock_arc = std::sync::Arc::new(mock);
    let mock_ref = std::sync::Arc::clone(&mock_arc);
    let mut state = test_state(MockManager::default());
    state.manager = mock_arc;

    let resp = submit_diffusion_generate_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            serde_json::to_string(&body_json).unwrap(),
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    // Params camelCase no body ao manager: o id que veio + strength.
    let body = mock_ref.last_create_job_body();
    let body = body.expect("create_job body captured");
    let params = body["params"].as_object().expect("params object");
    assert_eq!(params.get("initImageId"), Some(&serde_json::json!(init_id)));
    assert!(
        params.get("initGenerationId").is_none(),
        "initGenerationId não deve ser enviado quando initImageId veio"
    );
    // f32 no JSON (0.8f32 ⇒ 0.800000011920929) — compara com epsilon.
    let got_strength = params
        .get("initStrength")
        .and_then(|v| v.as_f64())
        .expect("initStrength numérico");
    assert!(
        (got_strength - 0.8).abs() < 1e-6,
        "initStrength divergente: {got_strength}"
    );
    // Config carrega o placeholder (nunca o id real).
    let config = body["config_yaml"].as_str().expect("config_yaml string");
    assert!(config.contains("init_image_path: \"{init_image_path}\""));
    assert!(!config.contains(init_id));
}

#[tokio::test]
async fn submit_diffusion_generate_202_forwards_init_generation_id_null_strength() {
    let gen_id = "550e8400-e29b-41d4-a716-446655440011";
    let body_json = serde_json::json!({
        "prompt": "img2img gallery test",
        "baseModel": "sdxl",
        "initGenerationId": gen_id
    });

    let mut mock = MockManager::default();
    mock.create_job_result = Some(CreateJobResponse {
        job_id: "job-img2img-2".into(),
        status: "queued".into(),
        queue_position: None,
    });
    let mock_arc = std::sync::Arc::new(mock);
    let mock_ref = std::sync::Arc::clone(&mock_arc);
    let mut state = test_state(MockManager::default());
    state.manager = mock_arc;

    let resp = submit_diffusion_generate_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            serde_json::to_string(&body_json).unwrap(),
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let body = mock_ref.last_create_job_body();
    let body = body.expect("create_job body captured");
    let params = body["params"].as_object().expect("params object");
    assert_eq!(
        params.get("initGenerationId"),
        Some(&serde_json::json!(gen_id))
    );
    assert!(
        params.get("initImageId").is_none(),
        "initImageId não deve ser enviado quando initGenerationId veio"
    );
    // Strength ausente ⇒ null (sem default local; manager aplica 0.6).
    assert_eq!(params.get("initStrength"), Some(&serde_json::Value::Null));
}

#[tokio::test]
async fn submit_diffusion_generate_202_txt2img_omite_init() {
    let body_json = serde_json::json!({
        "prompt": "txt2img puro",
        "baseModel": "sdxl"
    });

    let mut mock = MockManager::default();
    mock.create_job_result = Some(CreateJobResponse {
        job_id: "job-txt2img".into(),
        status: "queued".into(),
        queue_position: None,
    });
    let mock_arc = std::sync::Arc::new(mock);
    let mock_ref = std::sync::Arc::clone(&mock_arc);
    let mut state = test_state(MockManager::default());
    state.manager = mock_arc;

    let resp = submit_diffusion_generate_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            serde_json::to_string(&body_json).unwrap(),
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let body = mock_ref.last_create_job_body();
    let body = body.expect("create_job body captured");
    let params = body["params"].as_object().expect("params object");
    assert!(params.get("initImageId").is_none());
    assert!(params.get("initGenerationId").is_none());
    assert!(params.get("initStrength").is_none());
}

#[tokio::test]
async fn submit_diffusion_generate_400_init_xor_e_orfa() {
    let mock = MockManager::default();
    let state = test_state(mock);

    // Ambos os ids ⇒ 400.
    let resp = submit_diffusion_generate_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"prompt":"x","initImageId":"550e8400-e29b-41d4-a716-446655440010","initGenerationId":"550e8400-e29b-41d4-a716-446655440011"}"#,
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Strength órfã ⇒ 400.
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_diffusion_generate_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"prompt":"x","initStrength":0.7}"#,
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Strength fora da faixa ⇒ 400.
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = submit_diffusion_generate_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            r#"{"prompt":"x","initImageId":"550e8400-e29b-41d4-a716-446655440010","initStrength":1.5}"#,
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// =========================================================================
// DELETE /api/jobs/:id unit tests (AC-003)
// =========================================================================

#[tokio::test]
async fn delete_job_404_non_uuid() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = delete_job(axum::extract::State(state), Path("not-a-uuid".to_string())).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_job_200() {
    let mut mock = MockManager::default();
    mock.delete_job_result = Some(serde_json::json!({
        "id": "550e8400-e29b-41d4-a716-446655440000",
        "status": "done",
        "artifacts": ["best.pt"],
        "object_keys": ["artifacts/550e8400-e29b-41d4-a716-446655440000/best.pt"],
        "models_deleted": 1,
        "generations_preserved": 0
    }));
    let state = test_state(mock);
    let resp = delete_job(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["id"], "550e8400-e29b-41d4-a716-446655440000");
    assert_eq!(json["status"], "done");
    assert!(json["artifacts"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("best.pt")));
    // wire deve ser camelCase (contract D1), nunca snake_case do manager.
    assert_eq!(json["objectKeys"].as_array().unwrap().len(), 1);
    assert_eq!(json["modelsDeleted"], 1);
    assert_eq!(json["generationsPreserved"], 0);
    assert!(
        json.get("object_keys").is_none(),
        "snake_case não deve vazar no wire"
    );
}

#[tokio::test]
async fn delete_job_404_manager_not_found() {
    let mock = MockManager::default(); // delete_job_result = None → NotFound
    let state = test_state(mock);
    let resp = delete_job(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_job_409_not_terminal() {
    let mut mock = MockManager::default();
    mock.delete_not_terminal = true;
    let state = test_state(mock);
    let resp = delete_job(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "job_not_terminal");
}

// --- POST /api/jobs/diffusion (treino) — gate do encoder ---
// DB-backed (`--ignored` + DATABASE_URL=studio_test, mesmo contrato do
// datasets_db): o gate do encoder roda DEPOIS das checagens de dataset
// (passos 3-5 do handler), então exige estado real no Postgres.

/// Conecta no banco efêmero studio_test (guarda anti-footgun), roda as
/// migrations e semeia dataset + 1 imagem ativa.
async fn db_state_with_dataset(manager: MockManager) -> (crate::state::AppState, uuid::Uuid) {
    let url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL é obrigatório para este teste --ignored");
    assert!(
        url.contains("/studio_test") || url.ends_with("studio_test"),
        "só o banco efêmero studio_test (nunca o dev 'studio'): {url}"
    );
    let pool = sqlx::PgPool::connect(&url)
        .await
        .expect("conectar studio_test");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations");
    let ds_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO datasets (id, slug, title, category, type, task, format, status) \
         VALUES ($1, $2, $3, 'difusao', 'difusao_lora', 'caption', 'captions', 'ready')",
    )
    .bind(ds_id)
    .bind(format!("encoder-gate-{ds_id}"))
    .bind("encoder gate test")
    .execute(&pool)
    .await
    .expect("dataset semeado");
    sqlx::query(
        "INSERT INTO images (dataset_id, filename, object_key, bytes, width, height, md5, sha256, media_type) \
         VALUES ($1, 'a.png', $2, 10, 16, 16, md5(random()::text), md5(random()::text) || md5(random()::text), 'png')",
    )
    .bind(ds_id)
    .bind(format!("datasets/{ds_id}/x/a.png"))
    .execute(&pool)
    .await
    .expect("imagem semeada");
    let mut state = test_state(manager);
    state.pool = pool;
    (state, ds_id)
}

#[tokio::test]
#[ignore]
async fn submit_diffusion_train_encoder_flux_alias_passes_gate() {
    // baseModel "flux" (alias legado do preset FLUX.2 na UI) + encoder
    // EXISTENTE (kind=text_encoder): o gate NÃO deve rejeitar com 400
    // `textEncoderModelId requires arch 'flux-2-klein-4b'`. O mock não tem
    // create_job_result ⇒ após o gate o fluxo chega ao manager e responde
    // 503 `queue_unavailable` (pré-fix aqui seria 400 no gate).
    let enc_id = "550e8400-e29b-41d4-a716-446655440101";
    let mut manager = MockManager::default();
    manager.list_models_result = Some(vec![InternalModel {
        id: enc_id.into(),
        name: "my-encoder.safetensors".into(),
        engine: "diffusion".into(),
        model: None,
        source: "upload".into(),
        md5: "abc123".into(),
        bytes: 1_000_000,
        path: "models/diffusion/custom/my-encoder.safetensors".into(),
        job_id: None,
        created_at: "2026-09-15T00:00:00Z".into(),
        kind: Some("text_encoder".into()),
        arch: Some("flux-2-klein-4b".into()),
    }]);
    let (state, ds_id) = db_state_with_dataset(manager).await;
    let body_json = serde_json::json!({
        "datasetId": ds_id,
        "baseModel": "flux",
        "textEncoderModelId": enc_id
    });
    let resp = submit_diffusion_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            serde_json::to_string(&body_json).unwrap(),
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
#[ignore]
async fn submit_diffusion_train_encoder_sdxl_400() {
    // baseModel "sdxl" + encoder existente (kind=text_encoder) ⇒ gate
    // rejeita com 400 `invalid_request`.
    let enc_id = "550e8400-e29b-41d4-a716-446655440102";
    let mut manager = MockManager::default();
    manager.list_models_result = Some(vec![InternalModel {
        id: enc_id.into(),
        name: "my-encoder.safetensors".into(),
        engine: "diffusion".into(),
        model: None,
        source: "upload".into(),
        md5: "abc123".into(),
        bytes: 1_000_000,
        path: "models/diffusion/custom/my-encoder.safetensors".into(),
        job_id: None,
        created_at: "2026-09-15T00:00:00Z".into(),
        kind: Some("text_encoder".into()),
        arch: Some("flux-2-klein-4b".into()),
    }]);
    let (state, ds_id) = db_state_with_dataset(manager).await;
    let body_json = serde_json::json!({
        "datasetId": ds_id,
        "baseModel": "sdxl",
        "textEncoderModelId": enc_id
    });
    let resp = submit_diffusion_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            serde_json::to_string(&body_json).unwrap(),
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[ignore]
async fn submit_diffusion_train_weights_id_mirrors_top_level_for_resume() {
    // Regressão: `weights` do request de treino precisa aparecer também em
    // `manager_body["weights_id"]` (top-level) — é o único campo que o
    // manager (`jobs::create::create_job`) lê para resolver `weights_ref` e
    // staged o checkpoint/optimizer state do resume. Sem o espelho
    // top-level, o resume treinava do zero (bug pré-existente, corrigido aqui).
    let (mut state, ds_id) = db_state_with_dataset(MockManager::default()).await;
    let mut mock = MockManager::default();
    mock.create_job_result = Some(CreateJobResponse {
        job_id: "550e8400-e29b-41d4-a716-446655440888".into(),
        status: "queued".into(),
        queue_position: None,
    });
    let mock_arc = std::sync::Arc::new(mock);
    let mock_ref = std::sync::Arc::clone(&mock_arc);
    state.manager = mock_arc;

    let weights_id = "550e8400-e29b-41d4-a716-446655440777";
    let body_json = serde_json::json!({
        "datasetId": ds_id,
        "baseModel": "flux",
        "weights": weights_id,
        "epochOffset": 1
    });
    let resp = submit_diffusion_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            serde_json::to_string(&body_json).unwrap(),
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let body = mock_ref.last_create_job_body();
    let body = body.expect("create_job body captured");
    assert_eq!(
        body["weights_id"],
        serde_json::json!(weights_id),
        "weights_id top-level ausente — manager não resolve weights_ref, resume treina do zero"
    );
    // params.weights (legado, exibição no frontend) permanece intacto.
    assert_eq!(body["params"]["weights"], serde_json::json!(weights_id));
}

#[tokio::test]
#[ignore]
async fn submit_diffusion_train_weights_id_absent_without_weights() {
    // Sem `weights` no request (treino inicial) — `weights_id` ausente do
    // manager_body; comportamento pré-existente preservado (resolução
    // opcional no manager).
    let (mut state, ds_id) = db_state_with_dataset(MockManager::default()).await;
    let mut mock = MockManager::default();
    mock.create_job_result = Some(CreateJobResponse {
        job_id: "550e8400-e29b-41d4-a716-446655440999".into(),
        status: "queued".into(),
        queue_position: None,
    });
    let mock_arc = std::sync::Arc::new(mock);
    let mock_ref = std::sync::Arc::clone(&mock_arc);
    state.manager = mock_arc;

    let body_json = serde_json::json!({
        "datasetId": ds_id,
        "baseModel": "flux"
    });
    let resp = submit_diffusion_job(
        axum::extract::State(state),
        Ok(axum::body::Bytes::from(
            serde_json::to_string(&body_json).unwrap(),
        )),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let body = mock_ref.last_create_job_body();
    let body = body.expect("create_job body captured");
    assert!(body.get("weights_id").is_none());
}

#[tokio::test]
async fn delete_job_503_manager_offline() {
    let mut mock = MockManager::default();
    mock.fail = true;
    let state = test_state(mock);
    let resp = delete_job(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

// =========================================================================
// POST /api/jobs/cleanup unit tests (AC-003)
// =========================================================================

#[tokio::test]
async fn cleanup_jobs_200() {
    let mut mock = MockManager::default();
    mock.cleanup_result = Some(serde_json::json!({
        "deleted": 2,
        "jobs": [
            {
                "id": "550e8400-e29b-41d4-a716-446655440000",
                "status": "done",
                "artifacts": ["a.bin"],
                "object_keys": ["artifacts/550e8400-e29b-41d4-a716-446655440000/a.bin"],
                "models_deleted": 1,
                "generations_preserved": 0
            },
            {
                "id": "550e8400-e29b-41d4-a716-446655440001",
                "status": "failed",
                "artifacts": [],
                "object_keys": ["models/yolo/550e8400-e29b-41d4-a716-446655440001/p.pt"],
                "models_deleted": 1,
                "generations_preserved": 2
            }
        ],
        "object_keys": [
            "artifacts/550e8400-e29b-41d4-a716-446655440000/a.bin",
            "models/yolo/550e8400-e29b-41d4-a716-446655440001/p.pt"
        ]
    }));
    let state = test_state(mock);
    let resp = cleanup_jobs(
        axum::extract::State(state),
        Ok(Json(serde_json::json!({"olderThanDays": 30}))),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["deleted"], 2);
    assert!(json["jobs"].as_array().unwrap().len() == 2);
    // wire aninhado em camelCase (contract D1): objectKeys agregado + por job.
    let top_keys = json["objectKeys"].as_array().expect("objectKeys presente");
    assert!(!top_keys.is_empty(), "objectKeys agregado não-vazio");
    assert!(top_keys.contains(&serde_json::json!(
        "artifacts/550e8400-e29b-41d4-a716-446655440000/a.bin"
    )));
    assert!(top_keys.contains(&serde_json::json!(
        "models/yolo/550e8400-e29b-41d4-a716-446655440001/p.pt"
    )));
    assert!(
        json["jobs"][0]["modelsDeleted"].is_number(),
        "modelsDeleted numérico no job aninhado"
    );
    assert_eq!(json["jobs"][0]["modelsDeleted"], 1);
    assert!(
        json["jobs"][0]["objectKeys"].is_array(),
        "objectKeys presente no job aninhado"
    );
    assert_eq!(json["jobs"][1]["generationsPreserved"], 2);
    assert!(
        json["jobs"][1]["objectKeys"].is_array(),
        "objectKeys presente no segundo job aninhado"
    );
    assert!(
        json.get("object_keys").is_none(),
        "snake_case não deve vazar no wire"
    );
    assert!(
        json["jobs"][0].get("models_deleted").is_none(),
        "snake_case não deve vazar no job aninhado"
    );
}

#[tokio::test]
async fn cleanup_jobs_400_invalid_request() {
    let mut mock = MockManager::default();
    mock.cleanup_invalid_request = Some("invalid statuses".into());
    let state = test_state(mock);
    let resp = cleanup_jobs(
        axum::extract::State(state),
        Ok(Json(serde_json::json!({"statuses": ["invalid"]}))),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn cleanup_jobs_503_manager_offline() {
    let mut mock = MockManager::default();
    mock.fail = true;
    let state = test_state(mock);
    let resp = cleanup_jobs(axum::extract::State(state), Ok(Json(serde_json::json!({})))).await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn get_job_logs_source_omitted_defaults_to_telemetry() {
    let mut mock = MockManager::default();
    mock.get_job_result = Some(mock_job());
    mock.list_artifacts_result = Some(vec![]);
    let state = test_state(mock);
    let resp = get_job_logs(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        Query(JobLogsQuery {
            offset: None,
            limit: None,
            source: None,
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn get_job_logs_source_telemetry_explicit() {
    let mut mock = MockManager::default();
    mock.get_job_result = Some(mock_job());
    mock.list_artifacts_result = Some(vec![]);
    let state = test_state(mock);
    let resp = get_job_logs(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        Query(JobLogsQuery {
            offset: None,
            limit: None,
            source: Some("telemetry".to_string()),
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn get_job_logs_source_run() {
    let mut mock = MockManager::default();
    mock.get_job_result = Some(mock_job());
    mock.list_artifacts_result = Some(vec![]);
    let state = test_state(mock);
    let resp = get_job_logs(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        Query(JobLogsQuery {
            offset: None,
            limit: None,
            source: Some("run".to_string()),
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn get_job_logs_source_invalid_400_never_falls_back_to_telemetry() {
    // Nenhum mock de job/artifacts configurado — se a validação de source
    // caísse por engano no default `telemetry`, o teste ainda passaria por
    // acidente (get_job_result=None → 404); por isso a asserção é
    // especificamente BAD_REQUEST com o código invalid_source, não só
    // "não é 200".
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = get_job_logs(
        axum::extract::State(state),
        Path("550e8400-e29b-41d4-a716-446655440000".to_string()),
        Query(JobLogsQuery {
            offset: None,
            limit: None,
            source: Some("bogus".to_string()),
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "invalid_source");
}

// ---------------------------------------------------------------------------
// Fatia 5b — GET /api/jobs/:id/lineage
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_job_lineage_handler_200() {
    use crate::jobs::manager_client::InternalLineage;
    use heph_contracts::{LineageEdge, LineageNode};

    let job_id = "550e8400-e29b-41d4-a716-446655440000";
    let parent_id = "550e8400-e29b-41d4-a716-446655440001";
    let mut mock = MockManager::default();
    mock.lineage_by_id.insert(
        job_id.to_string(),
        InternalLineage {
            nodes: vec![
                LineageNode {
                    id: format!("job:{job_id}"),
                    kind: "job".into(),
                    label: "train flux".into(),
                    status: Some("done".into()),
                    created_at: Some("2026-10-01T00:00:00Z".into()),
                    epoch: None,
                },
                LineageNode {
                    id: format!("job:{parent_id}"),
                    kind: "job".into(),
                    label: "train flux".into(),
                    status: Some("done".into()),
                    created_at: Some("2026-09-01T00:00:00Z".into()),
                    epoch: None,
                },
            ],
            edges: vec![LineageEdge {
                from: format!("job:{job_id}"),
                to: format!("job:{parent_id}"),
                kind: "resumed_by".into(),
            }],
        },
    );
    let state = test_state(mock);
    let resp = get_job_lineage(axum::extract::State(state), Path(job_id.to_string())).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(json["edges"][0]["from"], format!("job:{job_id}"));
    assert_eq!(json["edges"][0]["to"], format!("job:{parent_id}"));
    assert_eq!(json["edges"][0]["kind"], "resumed_by");
    // Wire camelCase: createdAt, nunca created_at.
    assert_eq!(json["nodes"][0]["createdAt"], "2026-10-01T00:00:00Z");
    assert!(json["nodes"][0].get("created_at").is_none());
}

#[tokio::test]
async fn get_job_lineage_handler_404_job_inexistente() {
    let job_id = "550e8400-e29b-41d4-a716-446655440000";
    let mock = MockManager::default(); // lineage_by_id vazio ⇒ NotFound.
    let state = test_state(mock);
    let resp = get_job_lineage(axum::extract::State(state), Path(job_id.to_string())).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_job_lineage_handler_404_id_nao_uuid() {
    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = get_job_lineage(axum::extract::State(state), Path("not-a-uuid".to_string())).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_job_lineage_handler_503_manager_offline() {
    let job_id = "550e8400-e29b-41d4-a716-446655440000";
    let mut mock = MockManager::default();
    mock.fail = true;
    let state = test_state(mock);
    let resp = get_job_lineage(axum::extract::State(state), Path(job_id.to_string())).await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}
#[tokio::test]
async fn stream_job_events_subscribes_first_and_discards_stale_metrics() {
    use futures_util::StreamExt;
    use http::HeaderMap;

    let job_id = "550e8400-e29b-41d4-a716-446655440000";
    let mut mock = MockManager::default();
    let mut j = mock_job();
    j.id = job_id.into();
    j.status = "running".into();
    j.phase = Some("training".into());
    mock.get_job_result = Some(j);
    // delta inicial para Last-Event-ID: 5 retornará seq 5 e seq 6
    mock.metric_points_result = Some(heph_contracts::telemetry::MetricPointsResponse {
        items: vec![heph_contracts::telemetry::MetricPointWithKey {
            seq: 6,
            key: "loss".into(),
            step: 60,
            epoch: Some(1),
            value: 0.4,
            ts: "2026-01-01T00:00:01Z".into(),
        }],
        max_seq: 6,
        downsampled: false,
    });

    let state = test_state(mock);

    // Envia um evento durante a janela entre subscribe e envio
    // simulando evento com seq 6 já no broadcast
    let mut headers = HeaderMap::new();
    headers.insert("last-event-id", "5".parse().unwrap());

    // Se despacharmos no hub um evento com seq 6 (igual ao delta retornado no snapshot),
    // o stream deve descartar por seq <= último enviado (last_sent_seq=6).
    // E um evento seq 7 subsequente deve ser entregue!
    let hub = std::sync::Arc::clone(&state.job_events);
    let job_id_str = job_id.to_string();

    let resp = stream_job_events(
        axum::extract::State(state),
        Path(job_id_str.clone()),
        headers,
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);

    // Envia evento no hub com seq 6 (deve ser descartado) e seq 7 (deve passar)
    hub.emit_test_event(
        &job_id_str,
        crate::jobs::events_hub::JobEvent {
            event: "metrics",
            id: Some("6".into()),
            data: "stale-data-seq-6".into(),
        },
    )
    .await;
    hub.emit_test_event(
        &job_id_str,
        crate::jobs::events_hub::JobEvent {
            event: "metrics",
            id: Some("7".into()),
            data: "fresh-data-seq-7".into(),
        },
    )
    .await;
    // Consome o body do SSE
    let mut body = resp.into_body().into_data_stream();
    // 1o chunk: snapshot
    let chunk1 = body.next().await.unwrap().unwrap();
    let text1 = String::from_utf8_lossy(&chunk1);
    assert!(text1.contains("event: snapshot"));

    // 2o chunk: metrics seq 6 (do delta inicial)
    let chunk2 = body.next().await.unwrap().unwrap();
    let text2 = String::from_utf8_lossy(&chunk2);
    assert!(text2.contains("event: metrics"));
    assert!(text2.contains("id: 6"));

    // 3o chunk: metrics seq 7 (seq 6 do broadcast foi descartado!)
    let chunk3 = body.next().await.unwrap().unwrap();
    let text3 = String::from_utf8_lossy(&chunk3);
    assert!(text3.contains("event: metrics"));
    assert!(text3.contains("id: 7"));
    assert!(text3.contains("fresh-data-seq-7"));
}

#[tokio::test]
async fn stream_job_events_handles_lagged_receiver_with_delta_refetch() {
    use futures_util::StreamExt;
    use http::HeaderMap;

    let job_id = "550e8400-e29b-41d4-a716-446655440001";
    let mut mock = MockManager::default();
    let mut j = mock_job();
    j.id = job_id.into();
    j.status = "running".into();
    j.phase = Some("training".into());
    mock.get_job_result = Some(j);
    let dynamic_cb = std::sync::Arc::clone(&mock.dynamic_metric_points);
    *dynamic_cb.lock().unwrap() = Some(Box::new(|after| {
        // Ao receber lagged, busca delta a partir do seq conhecido
        Ok(heph_contracts::telemetry::MetricPointsResponse {
            items: vec![heph_contracts::telemetry::MetricPointWithKey {
                seq: 75,
                key: "loss".into(),
                step: 750,
                epoch: Some(2),
                value: 0.15,
                ts: "2026-01-01T00:01:00Z".into(),
            }],
            max_seq: 75,
            downsampled: false,
        })
    }));

    let state = test_state(mock);
    let hub = std::sync::Arc::clone(&state.job_events);
    let job_id_str = job_id.to_string();

    // Assina também um segundo cliente rápido que NÃO vai ficar lagged
    let mut fast_rx = hub.subscribe(&job_id_str).await;

    let resp = stream_job_events(
        axum::extract::State(state.clone()),
        Path(job_id_str.clone()),
        HeaderMap::new(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);

    // O canal do broadcast tem capacidade 64.
    // Vamos inundar com 75 eventos para forçar RecvError::Lagged no receiver do stream lento!
    for i in 1..=75 {
        hub.emit_test_event(
            &job_id_str,
            crate::jobs::events_hub::JobEvent {
                event: "metrics",
                id: Some(i.to_string()),
                data: format!("data-{}", i),
            },
        )
        .await;
    }

    // O segundo cliente (fast_rx) drena suas mensagens sem sofrer queda no canal
    let mut fast_received_count = 0;
    loop {
        match fast_rx.try_recv() {
            Ok(_) => fast_received_count += 1,
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {
                // Pode ter tido lag das primeiras mensagens mas continua recebendo as mais recentes
                continue;
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
            Err(tokio::sync::broadcast::error::TryRecvError::Closed) => panic!("fast_rx closed"),
        }
    }
    assert!(fast_received_count > 0, "cliente rápido continua recebendo");

    // Agora consome o stream SSE que sofreu lag
    let mut body = resp.into_body().into_data_stream();
    // 1o chunk: snapshot
    let chunk1 = body.next().await.unwrap().unwrap();
    let text1 = String::from_utf8_lossy(&chunk1);
    assert!(text1.contains("event: snapshot"));

    // 2o chunk: delta recuperado via refetch após o Lagged!
    let chunk2 = body.next().await.unwrap().unwrap();
    let text2 = String::from_utf8_lossy(&chunk2);
    assert!(
        text2.contains("event: metrics"),
        "recebeu evento refetched: {text2}"
    );
    assert!(text2.contains("id: 75"));
}

// ---------------------------------------------------------------------------
// Fatia 5c — GET /api/jobs/:id/export?format=csv|parquet
// ---------------------------------------------------------------------------

#[tokio::test]
async fn export_job_metrics_handler_400_invalid_format() {
    use crate::jobs::handlers::export::{export_job_metrics, JobExportQuery};

    let job_id = "550e8400-e29b-41d4-a716-446655440000";
    let mock = MockManager::default();
    let state = test_state(mock);

    // Formato ausente
    let resp = export_job_metrics(
        axum::extract::State(state.clone()),
        Path(job_id.to_string()),
        Query(JobExportQuery { format: None }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Formato desconhecido
    let resp = export_job_metrics(
        axum::extract::State(state),
        Path(job_id.to_string()),
        Query(JobExportQuery {
            format: Some("json".to_string()),
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn export_job_metrics_handler_404_id_nao_uuid() {
    use crate::jobs::handlers::export::{export_job_metrics, JobExportQuery};

    let mock = MockManager::default();
    let state = test_state(mock);
    let resp = export_job_metrics(
        axum::extract::State(state),
        Path("not-a-uuid".to_string()),
        Query(JobExportQuery {
            format: Some("csv".to_string()),
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn export_job_metrics_handler_404_job_inexistente() {
    use crate::jobs::handlers::export::{export_job_metrics, JobExportQuery};

    let job_id = "550e8400-e29b-41d4-a716-446655440000";
    let mock = MockManager::default(); // metric_points_result é None ⇒ NotFound
    let state = test_state(mock);
    let resp = export_job_metrics(
        axum::extract::State(state),
        Path(job_id.to_string()),
        Query(JobExportQuery {
            format: Some("csv".to_string()),
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn export_job_metrics_handler_200_csv_exact_header_and_rows() {
    use crate::jobs::handlers::export::{export_job_metrics, JobExportQuery};
    use axum::http::header;
    use heph_contracts::telemetry::{MetricPointWithKey, MetricPointsResponse};

    let job_id = "550e8400-e29b-41d4-a716-446655440000";
    let mut mock = MockManager::default();
    mock.metric_points_result = Some(MetricPointsResponse {
        items: vec![
            MetricPointWithKey {
                seq: 1,
                epoch: Some(1),
                step: 10,
                key: "loss".to_string(),
                value: 0.5432,
                ts: "2026-10-02T10:00:00Z".to_string(),
            },
            MetricPointWithKey {
                seq: 2,
                epoch: None, // epoch nullable test
                step: 20,
                key: "lr".to_string(),
                value: 0.001,
                ts: "2026-10-02T10:01:00Z".to_string(),
            },
        ],
        max_seq: 2,
        downsampled: false,
    });

    let state = test_state(mock);
    let resp = export_job_metrics(
        axum::extract::State(state),
        Path(job_id.to_string()),
        Query(JobExportQuery {
            format: Some("csv".to_string()),
        }),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap(),
        "text/csv; charset=utf-8"
    );
    assert_eq!(
        resp.headers()
            .get(header::CONTENT_DISPOSITION)
            .unwrap()
            .to_str()
            .unwrap(),
        format!("attachment; filename=\"job-{job_id}-metrics.csv\"")
    );

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], "seq,epoch,step,key,value,ts");
    assert_eq!(lines[1], "1,1,10,loss,0.5432,2026-10-02T10:00:00Z");
    assert_eq!(lines[2], "2,,20,lr,0.001,2026-10-02T10:01:00Z");
}

#[tokio::test]
async fn export_job_metrics_handler_200_parquet_roundtrip() {
    use crate::jobs::handlers::export::{export_job_metrics, JobExportQuery};
    use axum::http::header;
    use heph_contracts::telemetry::{MetricPointWithKey, MetricPointsResponse};
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    let job_id = "550e8400-e29b-41d4-a716-446655440000";
    let mut mock = MockManager::default();
    mock.metric_points_result = Some(MetricPointsResponse {
        items: vec![
            MetricPointWithKey {
                seq: 1,
                epoch: Some(1),
                step: 10,
                key: "loss".to_string(),
                value: 0.5432,
                ts: "2026-10-02T10:00:00Z".to_string(),
            },
            MetricPointWithKey {
                seq: 2,
                epoch: None,
                step: 20,
                key: "lr".to_string(),
                value: 0.001,
                ts: "2026-10-02T10:01:00Z".to_string(),
            },
        ],
        max_seq: 2,
        downsampled: false,
    });

    let state = test_state(mock);
    let resp = export_job_metrics(
        axum::extract::State(state),
        Path(job_id.to_string()),
        Query(JobExportQuery {
            format: Some("parquet".to_string()),
        }),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap(),
        "application/vnd.apache.parquet"
    );
    assert_eq!(
        resp.headers()
            .get(header::CONTENT_DISPOSITION)
            .unwrap()
            .to_str()
            .unwrap(),
        format!("attachment; filename=\"job-{job_id}-metrics.parquet\"")
    );

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();

    // Ler o parquet de volta usando parquet crate para validar contagem e tipos
    let reader_builder = ParquetRecordBatchReaderBuilder::try_new(body).unwrap();
    let mut reader = reader_builder.build().unwrap();
    let batch = reader.next().unwrap().unwrap();

    assert_eq!(batch.num_rows(), 2);
    assert_eq!(batch.num_columns(), 6);

    // Checar schema
    assert_eq!(batch.schema().field(0).name(), "seq");
    assert_eq!(batch.schema().field(1).name(), "epoch");
    assert_eq!(batch.schema().field(2).name(), "step");
    assert_eq!(batch.schema().field(3).name(), "key");
    assert_eq!(batch.schema().field(4).name(), "value");
    assert_eq!(batch.schema().field(5).name(), "ts");

    use arrow_array::cast::AsArray;
    use arrow_array::Array;
    let seq_col = batch
        .column(0)
        .as_primitive::<arrow_array::types::Int64Type>();
    let epoch_col = batch
        .column(1)
        .as_primitive::<arrow_array::types::Int32Type>();
    let key_col = batch.column(3).as_string::<i32>();
    let val_col = batch
        .column(4)
        .as_primitive::<arrow_array::types::Float64Type>();

    assert_eq!(seq_col.value(0), 1);
    assert_eq!(seq_col.value(1), 2);
    assert!(!epoch_col.is_null(0));
    assert_eq!(epoch_col.value(0), 1);
    assert!(epoch_col.is_null(1));
    assert_eq!(key_col.value(0), "loss");
    assert_eq!(key_col.value(1), "lr");
    assert_eq!(val_col.value(0), 0.5432);
    assert_eq!(val_col.value(1), 0.001);
}
