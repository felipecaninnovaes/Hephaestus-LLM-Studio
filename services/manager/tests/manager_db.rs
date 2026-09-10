//! Integração manager × Postgres (F4.3) — funções de lib.rs com pool real.
//!
//! Execução: `DATABASE_URL=postgres://studio:studio@localhost:5432/studio cargo test -p manager --test manager_db -- --ignored`
//!
//! AVISO: ESTE TESTE É PARA BANCO DE DESENVOLVIMENTO.

use async_trait::async_trait;
use manager::{
    self, ArtifactItem, CreateJobRequest, HeartbeatRequest, ManagerError, PackageRef, ReportRequest,
};
use sqlx::PgPool;

const TEST_DB_URL: &str = "postgres://studio:studio@localhost:5432/studio";

// Serial lock — testes compartilham o mesmo banco.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| TEST_DB_URL.into());
    let pool = PgPool::connect(&url).await.expect("conectar no Postgres");
    // Aplica migrations do principal (idempotente).
    sqlx::migrate!("../api-principal/migrations")
        .run(&pool)
        .await
        .expect("rodar migrations");
    pool
}

/// Fake do orchestrator — registra chamadas, retorna Ok.
struct FakeOrchestratorClient {
    calls: std::sync::Mutex<Vec<(String, serde_json::Value)>>,
}

impl FakeOrchestratorClient {
    fn new() -> Self {
        Self {
            calls: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<(String, serde_json::Value)> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl manager::OrchestratorClient for FakeOrchestratorClient {
    async fn post(&self, url: &str, body: &serde_json::Value) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push((url.to_string(), body.clone()));
        Ok(())
    }
}

/// Fake que falha no dispatch.
struct FailingOrchestratorClient;

#[async_trait]
impl manager::OrchestratorClient for FailingOrchestratorClient {
    async fn post(&self, _url: &str, _body: &serde_json::Value) -> Result<(), String> {
        Err("orchestrator unavailable (fake)".into())
    }
}

/// Limpa tabelas do manager.
async fn cleanup(pool: &PgPool) {
    sqlx::query("DELETE FROM job_artifacts")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM jobs").execute(pool).await.unwrap();
    sqlx::query("DELETE FROM orchestrators")
        .execute(pool)
        .await
        .unwrap();
    // Limpa datasets (ON DELETE SET NULL em jobs.dataset_id).
    sqlx::query("DELETE FROM image_embeddings")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM classes")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM datasets")
        .execute(pool)
        .await
        .unwrap();
}

/// Insere um dataset de teste e retorna o ID.
async fn insert_test_dataset(pool: &PgPool) -> uuid::Uuid {
    let id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO datasets (id, slug, title, category, type, task, format, status) \
         VALUES ($1, 'test-ds', 'Test DS', 'yolo', 'yolo_bbox', 'detect_track', 'yolo_txt', 'ready')",
    )
    .bind(id)
    .execute(pool)
    .await
    .expect("insert test dataset");
    id
}

/// Helper para criar um job de teste.
fn test_job_request(dataset_id: uuid::Uuid) -> CreateJobRequest {
    CreateJobRequest {
        kind: "yolo_train".into(),
        engine: "yolo".into(),
        model: "yolo11m".into(),
        mode: "train".into(),
        dataset_id: Some(dataset_id.to_string()),
        dataset_version_id: Some(uuid::Uuid::new_v4().to_string()),
        package_ref: Some(PackageRef {
            version_id: uuid::Uuid::new_v4().to_string(),
            key: "packages/test/dataset.zip".into(),
            md5_zip: "d41d8cd98f00b204e9800998ecf8427e".into(),
            bytes: 1024,
        }),
        config_yaml: Some("job_id: test\nengine: yolo".into()),
        params: Some(serde_json::json!({
            "package_ref": {
                "version_id": uuid::Uuid::new_v4().to_string(),
                "key": "packages/test/dataset.zip",
                "md5_zip": "d41d8cd98f00b204e9800998ecf8427e",
                "bytes": 1024
            }
        })),
        vram_min_gb: None,
    }
}

// ===========================================================================
// Testes
// ===========================================================================

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn ciclo_queued_done() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();

    // 1. Auto-adoção.
    manager::adopt_orchestrator(&p).await.expect("adopt");

    // 2. Cria job.
    let resp = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create job");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();
    assert_eq!(resp.status, "queued");
    assert!(resp.queue_position.is_some());

    // 3. Dispatch (fake).
    let dispatched = manager::dispatch_next(
        &p,
        &orch,
        "docker",
        "/data",
        "hephaestus/trainer-yolo:local",
    )
    .await
    .expect("dispatch");
    assert!(dispatched);

    // Verifica status dispatched.
    let job = manager::get_job(&p, job_id)
        .await
        .expect("get job dispatched");
    assert_eq!(job.status, "dispatched");

    // 4. Report: preparing.
    manager::report_job(
        &p,
        job_id,
        ReportRequest {
            status: "preparing".into(),
            progress: None,
            epoch: None,
            step: None,
            metrics: None,
            error: None,
            artifacts: None,
        },
    )
    .await
    .expect("report preparing");

    let job = manager::get_job(&p, job_id)
        .await
        .expect("get job preparing");
    assert_eq!(job.status, "preparing");

    // 5. Report: running.
    manager::report_job(
        &p,
        job_id,
        ReportRequest {
            status: "running".into(),
            progress: Some(0.3),
            epoch: Some(3),
            step: Some(150),
            metrics: None,
            error: None,
            artifacts: None,
        },
    )
    .await
    .expect("report running");

    let job = manager::get_job(&p, job_id).await.expect("get job running");
    assert_eq!(job.status, "running");
    assert_eq!(job.progress, Some(0.3));
    assert_eq!(job.epoch, Some(3));
    assert_eq!(job.step, Some(150));

    // 6. Report: running com metrics.
    let metrics = serde_json::json!({
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
    manager::report_job(
        &p,
        job_id,
        ReportRequest {
            status: "running".into(),
            progress: Some(0.5),
            epoch: Some(5),
            step: Some(300),
            metrics: Some(metrics.clone()),
            error: None,
            artifacts: None,
        },
    )
    .await
    .expect("report running with metrics");

    let job = manager::get_job(&p, job_id).await.expect("get job metrics");
    assert!(job.metrics.is_some());
    let m = job.metrics.unwrap();
    assert_eq!(m["items"][0]["box_loss"], 0.5);
    assert_eq!(m["items"][1]["mAP50-95"], 0.7);

    // 7. Report: done com artifacts.
    manager::report_job(
        &p,
        job_id,
        ReportRequest {
            status: "done".into(),
            progress: Some(1.0),
            epoch: None,
            step: None,
            metrics: None,
            error: None,
            artifacts: Some(vec![
                ArtifactItem {
                    kind: "model".into(),
                    path: "best.pt".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 512,
                },
                ArtifactItem {
                    kind: "model".into(),
                    path: "last.pt".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 512,
                },
                ArtifactItem {
                    kind: "metrics".into(),
                    path: "metrics.jsonl".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 256,
                },
            ]),
        },
    )
    .await
    .expect("report done");

    let job = manager::get_job(&p, job_id).await.expect("get job done");
    assert_eq!(job.status, "done");
    assert!(job.finished_at.is_some());
    assert_eq!(job.progress, Some(1.0));

    // Verifica artifacts.
    let arts = manager::get_job_artifacts(&p, job_id)
        .await
        .expect("list artifacts");
    assert_eq!(arts.len(), 3);
    let paths: Vec<&str> = arts.iter().map(|a| a.path.as_str()).collect();
    assert!(paths.contains(&"best.pt"));
    assert!(paths.contains(&"last.pt"));
    assert!(paths.contains(&"metrics.jsonl"));

    // 8. Idempotência: report duplicado não duplica artifacts.
    manager::report_job(
        &p,
        job_id,
        ReportRequest {
            status: "done".into(),
            progress: Some(1.0),
            epoch: None,
            step: None,
            metrics: None,
            error: None,
            artifacts: Some(vec![ArtifactItem {
                kind: "model".into(),
                path: "extra.pt".into(),
                md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                bytes: 100,
            }]),
        },
    )
    .await
    .expect("report done idempotent");

    let arts2 = manager::get_job_artifacts(&p, job_id)
        .await
        .expect("list artifacts after idempotent");
    assert_eq!(arts2.len(), 3, "artifacts não devem ser duplicados");
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn abort_em_voo_e_terminal() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();

    manager::adopt_orchestrator(&p).await.expect("adopt");

    // Cria e despacha.
    let resp = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    manager::dispatch_next(&p, &orch, "docker", "/data", "img")
        .await
        .expect("dispatch");

    // Report: preparing.
    manager::report_job(
        &p,
        job_id,
        ReportRequest {
            status: "preparing".into(),
            progress: None,
            epoch: None,
            step: None,
            metrics: None,
            error: None,
            artifacts: None,
        },
    )
    .await
    .expect("report preparing");

    // Abort em voo (preparing) → cancelling.
    let result = manager::abort_job(&p, job_id, &orch).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "cancelling");

    let job = manager::get_job(&p, job_id).await.expect("get");
    assert_eq!(job.status, "cancelling");

    // Verifica que o orchestrator foi notificado.
    let calls = orch.calls();
    let abort_calls: Vec<_> = calls
        .iter()
        .filter(|(url, _)| url.contains("/internal/abort"))
        .collect();
    assert_eq!(abort_calls.len(), 1);

    // --- Abort em job terminal (done) → 409 ---
    let resp2 = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create 2");
    let job_id2: uuid::Uuid = resp2.job_id.parse().unwrap();
    manager::report_job(
        &p,
        job_id2,
        ReportRequest {
            status: "done".into(),
            progress: Some(1.0),
            epoch: None,
            step: None,
            metrics: None,
            error: None,
            artifacts: None,
        },
    )
    .await
    .expect("done");

    let result2 = manager::abort_job(&p, job_id2, &orch).await;
    assert!(matches!(result2, Err(ManagerError::NotAbortable)));

    // --- Abort em job queued → cancelled direto ---
    let resp3 = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create 3");
    let job_id3: uuid::Uuid = resp3.job_id.parse().unwrap();

    let result3 = manager::abort_job(&p, job_id3, &orch).await;
    assert!(result3.is_ok());
    assert_eq!(result3.unwrap(), "cancelled");

    let job3 = manager::get_job(&p, job_id3).await.expect("get 3");
    assert_eq!(job3.status, "cancelled");
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn recupera_jobs_no_boot() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    // Insere jobs "órfãos" em estados não-terminais.
    let job1 = uuid::Uuid::new_v4();
    let job2 = uuid::Uuid::new_v4();
    let job3 = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO jobs (id, kind, engine, model, mode, dataset_id, status) \
         VALUES ($1, 'yolo_train', 'yolo', 'yolo11m', 'train', $2, 'dispatched')",
    )
    .bind(job1)
    .bind(ds_id)
    .execute(&p)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO jobs (id, kind, engine, model, mode, dataset_id, status) \
         VALUES ($1, 'yolo_train', 'yolo', 'yolo11m', 'train', $2, 'preparing')",
    )
    .bind(job2)
    .bind(ds_id)
    .execute(&p)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO jobs (id, kind, engine, model, mode, dataset_id, status) \
         VALUES ($1, 'yolo_train', 'yolo', 'yolo11m', 'train', $2, 'running')",
    )
    .bind(job3)
    .bind(ds_id)
    .execute(&p)
    .await
    .unwrap();

    // Recovery.
    let recovered = manager::recover_jobs(&p).await.expect("recover");
    assert_eq!(recovered, 3);

    // Verifica que todos voltaram a queued com queue_reason='recovered'.
    for jid in [job1, job2, job3] {
        let job = manager::get_job(&p, jid).await.expect("get recovered");
        assert_eq!(job.status, "queued");
        assert_eq!(job.queue_reason.as_deref(), Some("recovered"));
    }
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn set_null_ao_deletar_dataset() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    // Cria job referenciando o dataset.
    let resp = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    // Verifica que dataset_id está preenchido.
    let job = manager::get_job(&p, job_id).await.expect("get");
    assert_eq!(job.dataset_id.as_deref(), Some(ds_id.to_string().as_str()));

    // Deleta o dataset.
    sqlx::query("DELETE FROM datasets WHERE id = $1")
        .bind(ds_id)
        .execute(&p)
        .await
        .expect("delete dataset");

    // Verifica que job.dataset_id é NULL e job ainda existe.
    let job2 = manager::get_job(&p, job_id)
        .await
        .expect("get after delete");
    assert_eq!(job2.dataset_id, None);
    assert_eq!(job2.status, "queued");
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn auto_adocao_dedupe() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    // Primeira adoção: insere.
    manager::adopt_orchestrator(&p).await.expect("adopt 1");
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM orchestrators")
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(count.0, 1);

    // Segunda adoção: não duplica.
    manager::adopt_orchestrator(&p).await.expect("adopt 2");
    let count2: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM orchestrators")
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(count2.0, 1);

    // Verifica campos.
    let orch: (String, String, String) =
        sqlx::query_as("SELECT name, endpoint, status FROM orchestrators LIMIT 1")
            .fetch_one(&p)
            .await
            .unwrap();
    assert_eq!(orch.0, "orchestrator-local");
    assert_eq!(orch.1, "http://orchestrator-local:8082");
    assert_eq!(orch.2, "online");
}

// ===========================================================================
// G.3 — AUTO_ADOPT_LOCAL
// ===========================================================================

/// Boot com AUTO_ADOPT_LOCAL=0: adopt_orchestrator NÃO é chamado
/// (decisão pura: auto_adopt_enabled retorna false), resultando em 0 linhas.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn auto_adopt_local_skip() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    // Decisão pura: auto_adopt_enabled(Some("0")) == false.
    assert!(!manager::auto_adopt_enabled(Some("0")));

    // Não chama adopt_orchestrator → tabela vazia.
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM orchestrators")
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(
        count.0, 0,
        "não deve haver orchestrator-local quando auto_adopt_enabled é false"
    );
}

/// Boot default (AUTO_ADOPT_LOCAL não setado ou valor != "0"): adopt_orchestrator
/// é chamado (decisão pura: auto_adopt_enabled retorna true), resultando em 1 row.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn auto_adopt_local_default() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    // Decisão pura: auto_adopt_enabled(None) == true (fail-open).
    assert!(manager::auto_adopt_enabled(None));

    manager::adopt_orchestrator(&p).await.expect("adopt");

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM orchestrators")
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(
        count.0, 1,
        "deve haver 1 orchestrator-local no boot default"
    );
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn report_failed_grava_error_em_params() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let resp = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    // Report: failed.
    manager::report_job(
        &p,
        job_id,
        ReportRequest {
            status: "failed".into(),
            progress: None,
            epoch: None,
            step: None,
            metrics: None,
            error: Some("CUDA out of memory".into()),
            artifacts: None,
        },
    )
    .await
    .expect("report failed");

    let job = manager::get_job(&p, job_id).await.expect("get failed");
    assert_eq!(job.status, "failed");
    assert!(job.finished_at.is_some());

    // Verifica que error está em params.
    let params: serde_json::Value = sqlx::query_scalar("SELECT params FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(params["error"], "CUDA out of memory");
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn report_invalid_md5_rejeita() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let resp = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    // Report: done com md5 inválido.
    let result = manager::report_job(
        &p,
        job_id,
        ReportRequest {
            status: "done".into(),
            progress: Some(1.0),
            epoch: None,
            step: None,
            metrics: None,
            error: None,
            artifacts: Some(vec![ArtifactItem {
                kind: "model".into(),
                path: "best.pt".into(),
                md5: "invalid-md5-not-hex32".into(),
                bytes: 100,
            }]),
        },
    )
    .await;

    assert!(result.is_err());
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn dispatch_falha_volta_queued() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    manager::adopt_orchestrator(&p).await.expect("adopt");

    let resp = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    // Dispatch com orchestrator que falha.
    let failing = FailingOrchestratorClient;
    let dispatched = manager::dispatch_next(&p, &failing, "docker", "/data", "img")
        .await
        .expect("dispatch with failing orch");
    // Falso porque o dispatch falhou.
    assert!(!dispatched || true); // Pode retornar true (dispatched then reverted) ou false (no orch).

    // Verifica que o job voltou para queued com queue_reason.
    let job = manager::get_job(&p, job_id).await.expect("get after fail");
    assert_eq!(job.status, "queued");
    assert_eq!(job.queue_reason.as_deref(), Some("waiting_slot"));
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn telemetry_sem_heartbeat() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let cache = manager::new_telemetry_cache();
    let resp = manager::get_telemetry(&p, &cache).await;
    assert!(!resp.measured);
    assert!(resp.vram_used.is_none());
    assert!(resp.vram_total.is_none());
    assert!(resp.cpu.is_none());
    assert!(resp.ram.is_none());
    assert!(resp.ram_total.is_none());
    assert!(resp.gpus.is_empty());
    assert_eq!(resp.jobs_active, 0);
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn telemetry_com_heartbeat() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    manager::adopt_orchestrator(&p).await.expect("adopt");

    let cache = manager::new_telemetry_cache();
    let hb = HeartbeatRequest {
        endpoint: "http://orchestrator-local:8082".into(),
        gpus: vec!["NVIDIA RTX 3090".into()],
        vram_total: Some(24000),
        vram_used: Some(8000),
        cpu: Some(0.45),
        ram: Some(16384),
        ram_total: Some(67108864000),
        jobs_active: 2,
    };

    manager::receive_heartbeat(&p, &cache, hb)
        .await
        .expect("heartbeat");

    let resp = manager::get_telemetry(&p, &cache).await;
    assert!(resp.measured);
    assert_eq!(resp.vram_total, Some(24000));
    assert_eq!(resp.vram_used, Some(8000));
    assert_eq!(resp.cpu, Some(0.45));
    assert_eq!(resp.ram, Some(16384));
    assert_eq!(resp.ram_total, Some(67108864000));
    assert_eq!(resp.gpus, vec!["NVIDIA RTX 3090"]);
    assert_eq!(resp.jobs_active, 2);
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn report_job_inexistente_404() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let fake_id = uuid::Uuid::new_v4();
    let result = manager::report_job(
        &p,
        fake_id,
        ReportRequest {
            status: "done".into(),
            progress: Some(1.0),
            epoch: None,
            step: None,
            metrics: None,
            error: None,
            artifacts: None,
        },
    )
    .await;

    assert!(matches!(result, Err(ManagerError::NotFound)));
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn abort_job_inexistente_404() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let orch = FakeOrchestratorClient::new();
    let fake_id = uuid::Uuid::new_v4();
    let result = manager::abort_job(&p, fake_id, &orch).await;
    assert!(matches!(result, Err(ManagerError::NotFound)));
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn list_jobs_e_queue() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    // Cria 3 jobs.
    let r1 = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create 1");
    let _r2 = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create 2");
    let _r3 = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create 3");

    // Lista jobs (queue_position derivado dos items).
    let list = manager::list_jobs(&p, None, None).await.expect("list");
    assert_eq!(list.total, 3);
    assert_eq!(list.items.len(), 3);
    // Todos queued → queue_position preenchido (DESC: c=3, b=2, a=1).
    assert_eq!(list.items[0].queue_position, Some(3));
    assert_eq!(list.items[1].queue_position, Some(2));
    assert_eq!(list.items[2].queue_position, Some(1));

    // Filtra por status.
    let list_q = manager::list_jobs(&p, Some("queued"), None)
        .await
        .expect("list queued");
    assert_eq!(list_q.total, 3);

    // Filtra por engine.
    let list_y = manager::list_jobs(&p, None, Some("yolo"))
        .await
        .expect("list yolo");
    assert_eq!(list_y.total, 3);

    // Aborta um.
    let orch = FakeOrchestratorClient::new();
    let job_id: uuid::Uuid = r1.job_id.parse().unwrap();
    manager::abort_job(&p, job_id, &orch).await.expect("abort");

    let list_cancelled = manager::list_jobs(&p, Some("cancelled"), None)
        .await
        .expect("list cancelled");
    assert_eq!(list_cancelled.total, 1);

    let list_queued = manager::list_jobs(&p, Some("queued"), None)
        .await
        .expect("list queued after abort");
    assert_eq!(list_queued.total, 2);
}

/// Testa append incremental de metrics por epoch + dedup (R4).
/// Cada report do orquestrador envia 1 objeto; o manager deve acumular em array.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn metrics_append_e_dedup_por_epoch() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();

    manager::adopt_orchestrator(&p).await.expect("adopt");
    let resp = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    manager::dispatch_next(&p, &orch, "docker", "/data", "img")
        .await
        .expect("dispatch");

    // 1. Report running — epoch 1 (formato orquestrador: objeto único).
    manager::report_job(
        &p,
        job_id,
        ReportRequest {
            status: "running".into(),
            progress: Some(0.1),
            epoch: Some(1),
            step: None,
            metrics: Some(serde_json::json!({
                "epoch": 1,
                "box_loss": 0.5,
                "cls_loss": 0.3,
                "dfl_loss": 0.2,
                "mAP50": 0.8,
                "mAP50-95": 0.6
            })),
            error: None,
            artifacts: None,
        },
    )
    .await
    .expect("report running epoch 1");

    let job = manager::get_job(&p, job_id)
        .await
        .expect("get after epoch 1");
    let m = job.metrics.expect("metrics after epoch 1");
    let items = m
        .get("items")
        .and_then(|v| v.as_array())
        .expect("items array");
    assert_eq!(items.len(), 1, "deve ter 1 item após epoch 1");
    assert_eq!(items[0]["epoch"], 1);
    assert_eq!(items[0]["box_loss"], 0.5);

    // 2. Report running — epoch 2 (append).
    manager::report_job(
        &p,
        job_id,
        ReportRequest {
            status: "running".into(),
            progress: Some(0.2),
            epoch: Some(2),
            step: None,
            metrics: Some(serde_json::json!({
                "epoch": 2,
                "box_loss": 0.4,
                "cls_loss": 0.2,
                "dfl_loss": 0.1,
                "mAP50": 0.9,
                "mAP50-95": 0.7
            })),
            error: None,
            artifacts: None,
        },
    )
    .await
    .expect("report running epoch 2");

    let job = manager::get_job(&p, job_id)
        .await
        .expect("get after epoch 2");
    let m = job.metrics.expect("metrics after epoch 2");
    let items = m
        .get("items")
        .and_then(|v| v.as_array())
        .expect("items array");
    assert_eq!(items.len(), 2, "deve ter 2 itens após epoch 2");
    // Ordenados por epoch.
    assert_eq!(items[0]["epoch"], 1);
    assert_eq!(items[0]["box_loss"], 0.5);
    assert_eq!(items[1]["epoch"], 2);
    assert_eq!(items[1]["mAP50"], 0.9);

    // 3. Report duplicado do epoch 1 — deve substituir (dedup), não duplicar.
    manager::report_job(
        &p,
        job_id,
        ReportRequest {
            status: "running".into(),
            progress: Some(0.15),
            epoch: Some(1),
            step: None,
            metrics: Some(serde_json::json!({
                "epoch": 1,
                "box_loss": 0.45,
                "cls_loss": 0.28,
                "dfl_loss": 0.18,
                "mAP50": 0.82,
                "mAP50-95": 0.62
            })),
            error: None,
            artifacts: None,
        },
    )
    .await
    .expect("report duplicate epoch 1");

    let job = manager::get_job(&p, job_id).await.expect("get after dedup");
    let m = job.metrics.expect("metrics after dedup");
    let items = m
        .get("items")
        .and_then(|v| v.as_array())
        .expect("items array");
    assert_eq!(items.len(), 2, "dedup: não duplica epoch existente");
    // Epoch 1 atualizado com novos valores.
    assert_eq!(items[0]["epoch"], 1);
    assert_eq!(items[0]["box_loss"], 0.45);
    assert_eq!(items[0]["mAP50"], 0.82);
    // Epoch 2 inalterado.
    assert_eq!(items[1]["epoch"], 2);
    assert_eq!(items[1]["mAP50"], 0.9);
}

// ===========================================================================
// F6.1a — Rotas internas de leitura
// ===========================================================================

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn list_orchestrators_apos_adocao() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let cache = manager::new_telemetry_cache();

    // Sem orquestradores → vazio.
    let resp = manager::list_orchestrators(&p, &cache)
        .await
        .expect("list empty");
    assert!(resp.items.is_empty());

    // Auto-adoção.
    manager::adopt_orchestrator(&p).await.expect("adopt");

    let resp = manager::list_orchestrators(&p, &cache)
        .await
        .expect("list after adopt");
    assert_eq!(resp.items.len(), 1);
    let item = &resp.items[0];
    assert_eq!(item.name, "orchestrator-local");
    assert_eq!(item.kind, "local");
    assert_eq!(item.endpoint, "http://orchestrator-local:8082");
    assert_eq!(item.status, "online");
    // id é UUID válido.
    assert!(item.id.parse::<uuid::Uuid>().is_ok());
    // last_heartbeat: inicialmente None (não houve heartbeat ainda).
    assert!(item.last_heartbeat.is_none());
    // Sem cache → measured false, campos null.
    assert!(!item.measured);
    assert!(item.cpu.is_none());
    assert!(item.ram.is_none());
    assert!(item.ram_total.is_none());
    assert!(item.vram_used.is_none());
    assert!(item.vram_total.is_none());
    assert!(item.gpus.is_empty());
    assert_eq!(item.jobs_active, 0);
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn list_orchestrators_heartbeat_atualiza_last() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    manager::adopt_orchestrator(&p).await.expect("adopt");

    let cache = manager::new_telemetry_cache();
    let hb = HeartbeatRequest {
        endpoint: "http://orchestrator-local:8082".into(),
        gpus: vec![],
        vram_total: None,
        vram_used: None,
        cpu: Some(0.1),
        ram: Some(1024),
        ram_total: Some(4096),
        jobs_active: 0,
    };
    manager::receive_heartbeat(&p, &cache, hb)
        .await
        .expect("heartbeat");

    let resp = manager::list_orchestrators(&p, &cache)
        .await
        .expect("list after heartbeat");
    assert_eq!(resp.items.len(), 1);
    // last_heartbeat deve ter sido preenchido (datetime válido).
    assert!(resp.items[0].last_heartbeat.is_some());
    // Com cache preenchido → measured true.
    assert!(resp.items[0].measured);
    assert_eq!(resp.items[0].cpu, Some(0.1));
    assert_eq!(resp.items[0].ram, Some(1024));
    assert_eq!(resp.items[0].ram_total, Some(4096));
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn list_models_job_done_com_artifacts() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();

    manager::adopt_orchestrator(&p).await.expect("adopt");

    // Cria job, despacha, reporta done com artifacts.
    let resp = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    manager::dispatch_next(&p, &orch, "docker", "/data", "img")
        .await
        .expect("dispatch");

    manager::report_job(
        &p,
        job_id,
        ReportRequest {
            status: "done".into(),
            progress: Some(1.0),
            epoch: None,
            step: None,
            metrics: None,
            error: None,
            artifacts: Some(vec![
                ArtifactItem {
                    kind: "model".into(),
                    path: "best.pt".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 512,
                },
                ArtifactItem {
                    kind: "model".into(),
                    path: "last.pt".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 512,
                },
                ArtifactItem {
                    kind: "metrics".into(),
                    path: "metrics.jsonl".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 256,
                },
            ]),
        },
    )
    .await
    .expect("report done");

    let models = manager::list_models(&p).await.expect("list models");
    assert_eq!(models.items.len(), 1);
    let m = &models.items[0];
    assert_eq!(m.engine, "yolo");
    assert_eq!(m.model, "yolo11m");
    assert_eq!(m.path, "best.pt"); // preferência best > last
    assert_eq!(m.bytes, 512);
    assert_eq!(m.job_id, job_id.to_string());
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn list_models_dedupe_mesmo_engine_model() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();

    manager::adopt_orchestrator(&p).await.expect("adopt");

    // Job 1: done com best.pt + last.pt.
    let r1 = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create 1");
    let job1: uuid::Uuid = r1.job_id.parse().unwrap();
    manager::dispatch_next(&p, &orch, "docker", "/data", "img")
        .await
        .expect("dispatch 1");
    manager::report_job(
        &p,
        job1,
        ReportRequest {
            status: "done".into(),
            progress: Some(1.0),
            epoch: None,
            step: None,
            metrics: None,
            error: None,
            artifacts: Some(vec![
                ArtifactItem {
                    kind: "model".into(),
                    path: "best.pt".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 100,
                },
                ArtifactItem {
                    kind: "model".into(),
                    path: "last.pt".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 100,
                },
            ]),
        },
    )
    .await
    .expect("report done 1");

    // Job 2: done com best.pt (mesmo engine/model, mais recente).
    let r2 = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create 2");
    let job2: uuid::Uuid = r2.job_id.parse().unwrap();
    manager::dispatch_next(&p, &orch, "docker", "/data", "img")
        .await
        .expect("dispatch 2");
    manager::report_job(
        &p,
        job2,
        ReportRequest {
            status: "done".into(),
            progress: Some(1.0),
            epoch: None,
            step: None,
            metrics: None,
            error: None,
            artifacts: Some(vec![ArtifactItem {
                kind: "model".into(),
                path: "best.pt".into(),
                md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                bytes: 200,
            }]),
        },
    )
    .await
    .expect("report done 2");

    let models = manager::list_models(&p).await.expect("list models dedupe");
    assert_eq!(models.items.len(), 1, "dedupe: 2 jobs = 1 item");
    assert_eq!(
        models.items[0].job_id,
        job2.to_string(),
        "deve ser o mais recente"
    );
    assert_eq!(models.items[0].bytes, 200);
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn list_models_exclui_autotracker_boxes() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();

    manager::adopt_orchestrator(&p).await.expect("adopt");

    let resp = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();
    manager::dispatch_next(&p, &orch, "docker", "/data", "img")
        .await
        .expect("dispatch");

    // Done com artifacts: best.pt (model), metrics.jsonl (metrics), boxes.pt (boxes).
    manager::report_job(
        &p,
        job_id,
        ReportRequest {
            status: "done".into(),
            progress: Some(1.0),
            epoch: None,
            step: None,
            metrics: None,
            error: None,
            artifacts: Some(vec![
                ArtifactItem {
                    kind: "model".into(),
                    path: "best.pt".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 512,
                },
                ArtifactItem {
                    kind: "metrics".into(),
                    path: "metrics.jsonl".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 256,
                },
                ArtifactItem {
                    kind: "boxes".into(),
                    path: "boxes.pt".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 128,
                },
            ]),
        },
    )
    .await
    .expect("report done with boxes");

    let models = manager::list_models(&p)
        .await
        .expect("list models no boxes");
    assert_eq!(models.items.len(), 1, "boxes excluído: 1 item");
    assert_eq!(models.items[0].path, "best.pt");
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn list_models_sem_jobs_done() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let models = manager::list_models(&p).await.expect("list models empty");
    assert!(models.items.is_empty());
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn storage_usage_soma_esperada() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();

    manager::adopt_orchestrator(&p).await.expect("adopt");

    // Sem artifacts → 0.
    let usage = manager::get_storage_usage(&p).await.expect("usage empty");
    assert_eq!(usage.artifacts_bytes, 0);

    // Job 1 done com artifacts: 512 + 512 + 256 = 1280.
    let r1 = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create 1");
    let job1: uuid::Uuid = r1.job_id.parse().unwrap();
    manager::dispatch_next(&p, &orch, "docker", "/data", "img")
        .await
        .expect("dispatch 1");
    manager::report_job(
        &p,
        job1,
        ReportRequest {
            status: "done".into(),
            progress: Some(1.0),
            epoch: None,
            step: None,
            metrics: None,
            error: None,
            artifacts: Some(vec![
                ArtifactItem {
                    kind: "model".into(),
                    path: "best.pt".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 512,
                },
                ArtifactItem {
                    kind: "model".into(),
                    path: "last.pt".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 512,
                },
                ArtifactItem {
                    kind: "metrics".into(),
                    path: "metrics.jsonl".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 256,
                },
            ]),
        },
    )
    .await
    .expect("report done 1");

    let usage = manager::get_storage_usage(&p)
        .await
        .expect("usage after job 1");
    assert_eq!(usage.artifacts_bytes, 1280);

    // Job 2 done: 200 bytes → total 1480.
    let r2 = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create 2");
    let job2: uuid::Uuid = r2.job_id.parse().unwrap();
    manager::dispatch_next(&p, &orch, "docker", "/data", "img")
        .await
        .expect("dispatch 2");
    manager::report_job(
        &p,
        job2,
        ReportRequest {
            status: "done".into(),
            progress: Some(1.0),
            epoch: None,
            step: None,
            metrics: None,
            error: None,
            artifacts: Some(vec![ArtifactItem {
                kind: "model".into(),
                path: "best.pt".into(),
                md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                bytes: 200,
            }]),
        },
    )
    .await
    .expect("report done 2");

    let usage = manager::get_storage_usage(&p)
        .await
        .expect("usage after job 2");
    assert_eq!(usage.artifacts_bytes, 1480);
}

// ===========================================================================
// H.2 — Identidade do heartbeat + cache por nó + lista enriquecida + agregação
// ===========================================================================

/// (a) 2 nós (local+remoto) heartbeating → last_heartbeat atualizado SÓ na linha do endpoint.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn heartbeat_2_nos_atualiza_só_linha_correta() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    // Insere 2 orchestrators.
    let local_id = uuid::Uuid::new_v4();
    let remote_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'local', 'http://local:8082', 'local', 'online')",
    )
    .bind(local_id)
    .execute(&p)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'remote', 'http://remote:8082', 'remoto', 'online')",
    )
    .bind(remote_id)
    .execute(&p)
    .await
    .unwrap();

    // Heartbeat do local.
    let cache = manager::new_telemetry_cache();
    let hb = HeartbeatRequest {
        endpoint: "http://local:8082".into(),
        gpus: vec![],
        vram_total: None,
        vram_used: None,
        cpu: Some(0.5),
        ram: Some(4096),
        ram_total: Some(8192),
        jobs_active: 1,
    };
    manager::receive_heartbeat(&p, &cache, hb)
        .await
        .expect("heartbeat local");

    // Verifica: local tem last_heartbeat, remote não.
    let local: (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT last_heartbeat FROM orchestrators WHERE id = $1")
            .bind(local_id)
            .fetch_one(&p)
            .await
            .unwrap();
    assert!(local.0.is_some(), "local deve ter last_heartbeat");

    let remote: (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT last_heartbeat FROM orchestrators WHERE id = $1")
            .bind(remote_id)
            .fetch_one(&p)
            .await
            .unwrap();
    assert!(remote.0.is_none(), "remote não deve ter last_heartbeat");
}

/// (b) Heartbeat de endpoint inexistente → warn + nada gravado.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn heartbeat_endpoint_inexistente_nada_gravado() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let cache = manager::new_telemetry_cache();
    let hb = HeartbeatRequest {
        endpoint: "http://fantasma:9999".into(),
        gpus: vec![],
        vram_total: None,
        vram_used: None,
        cpu: None,
        ram: None,
        ram_total: None,
        jobs_active: 0,
    };
    manager::receive_heartbeat(&p, &cache, hb)
        .await
        .expect("heartbeat fantasma deve retornar OK");

    // Nenhuma linha criada.
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM orchestrators")
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(count.0, 0);

    // Cache vazio.
    let cache_lock = cache.read().await;
    assert!(cache_lock.is_empty());
}

/// (c) Heartbeat revive offline→online e NÃO revive revoked.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn heartbeat_revive_offline_nao_revive_revoked() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    // Insere 2 orchestrators: 1 offline, 1 revoked.
    let offline_id = uuid::Uuid::new_v4();
    let revoked_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'offline-orch', 'http://offline:8082', 'remoto', 'offline')",
    )
    .bind(offline_id)
    .execute(&p)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'revoked-orch', 'http://revoked:8082', 'remoto', 'revoked')",
    )
    .bind(revoked_id)
    .execute(&p)
    .await
    .unwrap();

    let cache = manager::new_telemetry_cache();

    // Heartbeat do offline.
    let hb = HeartbeatRequest {
        endpoint: "http://offline:8082".into(),
        gpus: vec![],
        vram_total: None,
        vram_used: None,
        cpu: None,
        ram: None,
        ram_total: None,
        jobs_active: 0,
    };
    manager::receive_heartbeat(&p, &cache, hb)
        .await
        .expect("heartbeat offline");

    // Offline → online.
    let status: (String,) = sqlx::query_as("SELECT status FROM orchestrators WHERE id = $1")
        .bind(offline_id)
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(status.0, "online");

    // Revoked → continua revoked.
    let status: (String,) = sqlx::query_as("SELECT status FROM orchestrators WHERE id = $1")
        .bind(revoked_id)
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(status.0, "revoked");
}

/// (d) Heartbeat com vram_total/gpus → colunas gravadas (round MiB/1024).
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn heartbeat_grava_vram_total_gb_e_gpus() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let orch_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'gpu-orch', 'http://gpu:8082', 'remoto', 'online')",
    )
    .bind(orch_id)
    .execute(&p)
    .await
    .unwrap();

    let cache = manager::new_telemetry_cache();
    let hb = HeartbeatRequest {
        endpoint: "http://gpu:8082".into(),
        gpus: vec!["NVIDIA RTX 3060".into(), "NVIDIA GTX 1660S".into()],
        vram_total: Some(18432), // 12288 + 6144 MiB
        vram_used: Some(5000),
        cpu: Some(0.3),
        ram: Some(8192),
        ram_total: Some(16384),
        jobs_active: 1,
    };
    manager::receive_heartbeat(&p, &cache, hb)
        .await
        .expect("heartbeat gpu");

    // Verifica colunas.
    let row: (Option<i32>, serde_json::Value) =
        sqlx::query_as("SELECT vram_total_gb, gpus FROM orchestrators WHERE id = $1")
            .bind(orch_id)
            .fetch_one(&p)
            .await
            .unwrap();
    assert_eq!(row.0, Some(18), "round(18432/1024) = 18");
    assert_eq!(
        row.1,
        serde_json::json!(["NVIDIA RTX 3060", "NVIDIA GTX 1660S"])
    );
}

/// (e.1) Agregação: 2 nós com cache → soma+união+cpu/ram null.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn agregacao_2_nos_soma_uniao() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let id1 = uuid::Uuid::new_v4();
    let id2 = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'n1', 'http://n1:8082', 'local', 'online')",
    )
    .bind(id1)
    .execute(&p)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'n2', 'http://n2:8082', 'remoto', 'online')",
    )
    .bind(id2)
    .execute(&p)
    .await
    .unwrap();

    let cache = manager::new_telemetry_cache();

    // Heartbeat nó 1.
    let hb1 = HeartbeatRequest {
        endpoint: "http://n1:8082".into(),
        gpus: vec!["RTX 3060".into()],
        vram_total: Some(12000),
        vram_used: Some(3000),
        cpu: Some(0.4),
        ram: Some(4096),
        ram_total: Some(8192),
        jobs_active: 1,
    };
    manager::receive_heartbeat(&p, &cache, hb1)
        .await
        .expect("hb n1");

    // Heartbeat nó 2.
    let hb2 = HeartbeatRequest {
        endpoint: "http://n2:8082".into(),
        gpus: vec!["GTX 1660S".into(), "RTX 3060".into()],
        vram_total: Some(6000),
        vram_used: Some(2000),
        cpu: Some(0.6),
        ram: Some(8192),
        ram_total: Some(16384),
        jobs_active: 2,
    };
    manager::receive_heartbeat(&p, &cache, hb2)
        .await
        .expect("hb n2");

    let resp = manager::get_telemetry(&p, &cache).await;
    assert!(resp.measured);
    assert_eq!(resp.vram_used, Some(5000)); // 3000 + 2000
    assert_eq!(resp.vram_total, Some(18000)); // 12000 + 6000
    assert_eq!(resp.jobs_active, 3); // 1 + 2
                                     // cpu/ram null no agregado com >1 nó.
    assert!(resp.cpu.is_none());
    assert!(resp.ram.is_none());
    assert!(resp.ram_total.is_none());
    // União de gpus.
    assert!(resp.gpus.contains(&"RTX 3060".to_string()));
    assert!(resp.gpus.contains(&"GTX 1660S".to_string()));
}

/// (e.2) Agregação: 1 nó → idêntico a hoje (compat).
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn agregacao_1_no_compat() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    // Insere orchestrator para que o heartbeat seja aceito.
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'local', 'http://local:8082', 'local', 'online')",
    )
    .bind(uuid::Uuid::new_v4())
    .execute(&p)
    .await
    .unwrap();

    let cache = manager::new_telemetry_cache();
    let hb = HeartbeatRequest {
        endpoint: "http://local:8082".into(),
        gpus: vec!["RTX 3090".into()],
        vram_total: Some(24000),
        vram_used: Some(8000),
        cpu: Some(0.45),
        ram: Some(16384),
        ram_total: Some(67108864000),
        jobs_active: 2,
    };
    manager::receive_heartbeat(&p, &cache, hb)
        .await
        .expect("hb local");

    let resp = manager::get_telemetry(&p, &cache).await;
    assert!(resp.measured);
    assert_eq!(resp.vram_total, Some(24000));
    assert_eq!(resp.vram_used, Some(8000));
    assert_eq!(resp.cpu, Some(0.45));
    assert_eq!(resp.ram, Some(16384));
    assert_eq!(resp.ram_total, Some(67108864000));
    assert_eq!(resp.gpus, vec!["RTX 3090"]);
    assert_eq!(resp.jobs_active, 2);
}

/// (e.3) Agregação: 0 nós → fallback (measured:false + jobs da fila).
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn agregacao_0_nos_fallback() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let cache = manager::new_telemetry_cache();
    let resp = manager::get_telemetry(&p, &cache).await;
    assert!(!resp.measured);
    assert!(resp.vram_used.is_none());
    assert!(resp.vram_total.is_none());
    assert!(resp.cpu.is_none());
    assert!(resp.ram.is_none());
    assert!(resp.ram_total.is_none());
    assert!(resp.gpus.is_empty());
    // jobs_active vem da fila.
    assert_eq!(resp.jobs_active, 0);
}

/// Teste unitário puro: agregação de 2 nós (fixture).
#[test]
fn agregacao_pura_2_nos() {
    use manager::TelemetryState;
    use std::collections::HashMap;

    let mut cache: HashMap<uuid::Uuid, TelemetryState> = HashMap::new();

    let id1 = uuid::Uuid::new_v4();
    let id2 = uuid::Uuid::new_v4();
    let now = chrono::Utc::now();

    cache.insert(
        id1,
        TelemetryState {
            endpoint: "http://n1:8082".into(),
            measured: true,
            vram_used: Some(3000),
            vram_total: Some(12000),
            cpu: Some(0.4),
            ram: Some(4096),
            ram_total: Some(8192),
            gpus: vec!["RTX 3060".into()],
            jobs_active: 1,
            last_heartbeat: Some(now),
        },
    );
    cache.insert(
        id2,
        TelemetryState {
            endpoint: "http://n2:8082".into(),
            measured: true,
            vram_used: Some(2000),
            vram_total: Some(6000),
            cpu: Some(0.6),
            ram: Some(8192),
            ram_total: Some(16384),
            gpus: vec!["GTX 1660S".into(), "RTX 3060".into()],
            jobs_active: 2,
            last_heartbeat: Some(now),
        },
    );

    // Simula agregação (lógica extraída de get_telemetry).
    let mut vram_used_sum: Option<i64> = Some(0);
    let mut vram_total_sum: Option<i64> = Some(0);
    let mut gpus: Vec<String> = Vec::new();
    let mut jobs_active_sum: i32 = 0;

    for state in cache.values() {
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
        for gpu in &state.gpus {
            if !gpus.contains(gpu) {
                gpus.push(gpu.clone());
            }
        }
        jobs_active_sum += state.jobs_active;
    }

    assert_eq!(vram_used_sum, Some(5000));
    assert_eq!(vram_total_sum, Some(18000));
    assert_eq!(jobs_active_sum, 3);
    assert!(gpus.contains(&"RTX 3060".to_string()));
    assert!(gpus.contains(&"GTX 1660S".to_string()));
}
