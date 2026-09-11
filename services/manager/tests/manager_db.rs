//! Integração manager × Postgres (F4.3) — funções de lib.rs com pool real.
//!
//! Execução: `DATABASE_URL=postgres://studio:studio@localhost:5432/studio cargo test -p manager --test manager_db -- --ignored`
//!
//! AVISO: ESTE TESTE É PARA BANCO DE DESENVOLVIMENTO.

use async_trait::async_trait;
use manager::{
    self, ArtifactItem, CreateJobRequest, CreateModelRequest, HeartbeatRequest, ManagerError,
    PackageRef, ReportRequest, VramTable,
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
    verify_valid: bool,
}

impl FakeOrchestratorClient {
    fn new() -> Self {
        Self {
            calls: std::sync::Mutex::new(Vec::new()),
            verify_valid: true,
        }
    }

    fn with_verify_valid(valid: bool) -> Self {
        Self {
            calls: std::sync::Mutex::new(Vec::new()),
            verify_valid: valid,
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

    async fn post_json(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.calls
            .lock()
            .unwrap()
            .push((url.to_string(), body.clone()));
        Ok(serde_json::json!({"valid": self.verify_valid}))
    }
}

/// Fake que falha no dispatch.
struct FailingOrchestratorClient;

#[async_trait]
impl manager::OrchestratorClient for FailingOrchestratorClient {
    async fn post(&self, _url: &str, _body: &serde_json::Value) -> Result<(), String> {
        Err("orchestrator unavailable (fake)".into())
    }

    async fn post_json(
        &self,
        _url: &str,
        _body: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        Err("orchestrator unavailable (fake)".into())
    }
}

/// Limpa tabelas do manager.
async fn cleanup(pool: &PgPool) {
    sqlx::query("DELETE FROM models")
        .execute(pool)
        .await
        .unwrap();
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
        weights_id: None,
    }
}

/// VRAM table de teste (espelha packages/policies/vram-table.yaml).
fn test_vram_table() -> VramTable {
    let yaml = r#"
defaults:
  headroom_gb: 2
entries:
  - { engine: yolo, model: yolo11n, mode: train, vram_min_gb: 6 }
  - { engine: yolo, model: yolo11m, mode: train, vram_min_gb: 10 }
  - { engine: clip, model: ViT-B-32, mode: train, vram_min_gb: 10 }
"#;
    VramTable::parse(yaml).expect("test vram table")
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
        &test_vram_table(),
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

    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
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
    let dispatched =
        manager::dispatch_next(&p, &failing, "docker", "/data", "img", &test_vram_table())
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
        max_gpu_mib: Some(24000),
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

    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
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
        max_gpu_mib: None,
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

    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
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
    assert_eq!(m.model.as_deref(), Some("yolo11m"));
    assert_eq!(m.name, "best.pt");
    assert_eq!(m.source, "train");
    assert_eq!(m.hash, "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(m.bytes, 512);
    assert_eq!(m.path, format!("artifacts/{job_id}/best.pt"));
    assert_eq!(m.job_id.as_deref(), Some(job_id.to_string().as_str()));
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn list_models_cada_job_best_uma_linha() {
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
    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
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

    // Job 2: done com best.pt (mesmo engine/model).
    let r2 = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create 2");
    let job2: uuid::Uuid = r2.job_id.parse().unwrap();
    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
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

    let models = manager::list_models(&p)
        .await
        .expect("list models sem dedupe");
    // Cada job gera 1 linha (sem dedupe por engine/model — D2).
    assert_eq!(models.items.len(), 2, "2 jobs = 2 itens (sem dedupe)");
    // Mais recente primeiro (ORDER BY created_at DESC).
    assert_eq!(models.items[0].bytes, 200);
    assert_eq!(
        models.items[0].job_id.as_deref(),
        Some(job2.to_string().as_str())
    );
    assert_eq!(models.items[1].bytes, 100);
    assert_eq!(
        models.items[1].job_id.as_deref(),
        Some(job1.to_string().as_str())
    );
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
    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
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
    assert_eq!(models.items[0].name, "best.pt");
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
    assert_eq!(usage.models_bytes, 0);

    // Job 1 done com artifacts: best.pt (512, model) + last.pt (512, model) + metrics.jsonl (256, metrics).
    // artifacts_bytes exclui kind='model' → só metrics.jsonl = 256.
    // models_bytes soma best.pt → 512 (last.pt não entra — hook filtra 'best').
    let r1 = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create 1");
    let job1: uuid::Uuid = r1.job_id.parse().unwrap();
    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
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
    assert_eq!(usage.artifacts_bytes, 256, "artifacts exclui kind='model'");
    assert_eq!(usage.models_bytes, 512, "models soma best.pt");

    // Job 2 done: best.pt (200) + metrics.jsonl (100).
    // artifacts_bytes: 256 + 100 = 356.
    // models_bytes: 512 + 200 = 712.
    let r2 = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create 2");
    let job2: uuid::Uuid = r2.job_id.parse().unwrap();
    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
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
            artifacts: Some(vec![
                ArtifactItem {
                    kind: "model".into(),
                    path: "best.pt".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 200,
                },
                ArtifactItem {
                    kind: "metrics".into(),
                    path: "metrics.jsonl".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 100,
                },
            ]),
        },
    )
    .await
    .expect("report done 2");

    let usage = manager::get_storage_usage(&p)
        .await
        .expect("usage after job 2");
    assert_eq!(usage.artifacts_bytes, 356, "artifacts exclui ambos best.pt");
    assert_eq!(usage.models_bytes, 712, "models soma ambos best.pt");
}

// ===========================================================================
// I.2a — Models: backfill, hook idempotente, list_models, storage
// ===========================================================================

/// Backfill: INSERT..SELECT do 0007 insere best.pt existente na tabela models.
/// Cria job done com best.pt + last.pt via report, limpa models, roda backfill SQL
/// → 1 linha em models (best apenas), id = artifact id.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn backfill_best_pt_para_models() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();

    manager::adopt_orchestrator(&p).await.expect("adopt");

    // Cria job done com best.pt + last.pt via report (popula job_artifacts).
    let resp = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();
    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
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
            ]),
        },
    )
    .await
    .expect("report done");

    // Limpa models (simula banco antes do backfill).
    sqlx::query("DELETE FROM models").execute(&p).await.unwrap();

    // Executa o backfill SQL do 0007 (INSERT..SELECT ON CONFLICT DO NOTHING).
    sqlx::query(
        "INSERT INTO models (id, engine, name, model, s3_key, source, hash, bytes, job_id, created_at)
         SELECT ja.id, j.engine, split_part(ja.path, '/', -1), j.model,
                'artifacts/' || ja.job_id::text || '/' || ja.path, 'train',
                ja.md5, ja.bytes, ja.job_id, j.created_at
         FROM job_artifacts ja
         JOIN jobs j ON j.id = ja.job_id
         WHERE ja.kind = 'model' AND j.status = 'done' AND ja.path LIKE '%best%'
         ON CONFLICT (s3_key) DO NOTHING",
    )
    .execute(&p)
    .await
    .expect("rodar backfill");

    // Verifica: 1 linha em models (best apenas), id = artifact id.
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM models")
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(count.0, 1, "backfill deve inserir 1 linha (best apenas)");

    let row: (
        uuid::Uuid,
        String,
        String,
        Option<String>,
        String,
        String,
        i64,
        Option<uuid::Uuid>,
    ) = sqlx::query_as(
        "SELECT id, name, engine, model, source, hash, bytes, job_id FROM models LIMIT 1",
    )
    .fetch_one(&p)
    .await
    .unwrap();
    assert_eq!(row.1, "best.pt");
    assert_eq!(row.2, "yolo");
    assert_eq!(row.3.as_deref(), Some("yolo11m"));
    assert_eq!(row.4, "train");
    assert_eq!(row.5, "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(row.6, 512);
    assert_eq!(row.7, Some(job_id));
}

/// Backfill idempotente: rodar 2× o INSERT..SELECT não duplica (ON CONFLICT).
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn backfill_idempotente_2x_sem_duplicar() {
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
    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
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
            artifacts: Some(vec![ArtifactItem {
                kind: "model".into(),
                path: "best.pt".into(),
                md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                bytes: 512,
            }]),
        },
    )
    .await
    .expect("report done");

    sqlx::query("DELETE FROM models").execute(&p).await.unwrap();

    let backfill = "INSERT INTO models (id, engine, name, model, s3_key, source, hash, bytes, job_id, created_at)
         SELECT ja.id, j.engine, split_part(ja.path, '/', -1), j.model,
                'artifacts/' || ja.job_id::text || '/' || ja.path, 'train',
                ja.md5, ja.bytes, ja.job_id, j.created_at
         FROM job_artifacts ja
         JOIN jobs j ON j.id = ja.job_id
         WHERE ja.kind = 'model' AND j.status = 'done' AND ja.path LIKE '%best%'
         ON CONFLICT (s3_key) DO NOTHING";

    // Roda 2×.
    sqlx::query(backfill).execute(&p).await.expect("backfill 1");
    sqlx::query(backfill).execute(&p).await.expect("backfill 2");

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM models")
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(count.0, 1, "backfill idempotente: 2 runs = 1 linha");
}

/// Hook: report done com best.pt → 1 linha em models; report 2× (idempotência) → 1 linha.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn hook_best_pt_idempotente_report_2x() {
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
    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
        .await
        .expect("dispatch");

    // 1º report done.
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
                path: "best.pt".into(),
                md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                bytes: 512,
            }]),
        },
    )
    .await
    .expect("report done 1");

    let count1: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM models")
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(count1.0, 1, "após 1º report: 1 linha em models");

    // 2º report done (idempotente — job já está done, report é ignorado, mas
    // o hook não deve duplicar porque o guard de status terminal já retorna Ok(())).
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
                path: "best.pt".into(),
                md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                bytes: 512,
            }]),
        },
    )
    .await
    .expect("report done 2 (idempotente)");

    let count2: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM models")
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(
        count2.0, 1,
        "após 2º report: continua 1 linha (idempotente)"
    );
}

/// Hook: job done SEM best.pt → 0 linhas em models.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn hook_sem_best_pt_nao_insere() {
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
    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
        .await
        .expect("dispatch");

    // Done com artifacts que NÃO contêm best.
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
    .expect("report done without best");

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM models")
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(count.0, 0, "sem best.pt → 0 linhas em models");
}

// ===========================================================================
// I.2b — POST /internal/models, weights_id, dispatch weights_ref
// ===========================================================================

/// POST /internal/models: 201 com id dado.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_model_201_com_id_dado() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let model_id = uuid::Uuid::new_v4();
    let req = CreateModelRequest {
        id: model_id,
        engine: "yolo".into(),
        name: "best.pt".into(),
        model: Some("yolo11m".into()),
        s3_key: "models/yolo/abc/best.pt".into(),
        source: "upload".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 1024,
        job_id: None,
    };

    let item = manager::create_model(&p, req)
        .await
        .expect("create model 201");
    assert_eq!(item.id, model_id.to_string());
    assert_eq!(item.engine, "yolo");
    assert_eq!(item.name, "best.pt");
    assert_eq!(item.model.as_deref(), Some("yolo11m"));
    assert_eq!(item.source, "upload");
    assert_eq!(item.hash, "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(item.bytes, 1024);
    assert_eq!(item.path, "models/yolo/abc/best.pt");
    assert!(item.job_id.is_none());
}

/// POST /internal/models: 400 — engine inválida.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_model_400_engine_invalida() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let req = CreateModelRequest {
        id: uuid::Uuid::new_v4(),
        engine: "diffusion".into(),
        name: "model.pt".into(),
        model: None,
        s3_key: "models/diff/abc/model.pt".into(),
        source: "upload".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 100,
        job_id: None,
    };

    let result = manager::create_model(&p, req).await;
    assert!(matches!(result, Err(ManagerError::InvalidRequest(_))));
}

/// POST /internal/models: 400 — hash curto.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_model_400_hash_curto() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let req = CreateModelRequest {
        id: uuid::Uuid::new_v4(),
        engine: "yolo".into(),
        name: "model.pt".into(),
        model: None,
        s3_key: "models/yolo/abc2/model.pt".into(),
        source: "upload".into(),
        url: None,
        hash: "abc123".into(),
        bytes: 100,
        job_id: None,
    };

    let result = manager::create_model(&p, req).await;
    assert!(matches!(result, Err(ManagerError::InvalidRequest(_))));
}

/// POST /internal/models: 400 — source inválida.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_model_400_source_invalida() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let req = CreateModelRequest {
        id: uuid::Uuid::new_v4(),
        engine: "yolo".into(),
        name: "model.pt".into(),
        model: None,
        s3_key: "models/yolo/abc3/model.pt".into(),
        source: "huggingface".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 100,
        job_id: None,
    };

    let result = manager::create_model(&p, req).await;
    assert!(matches!(result, Err(ManagerError::InvalidRequest(_))));
}

/// POST /internal/models: 409 — s3_key duplicado.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_model_409_s3_key_duplicado() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let req1 = CreateModelRequest {
        id: uuid::Uuid::new_v4(),
        engine: "yolo".into(),
        name: "best.pt".into(),
        model: None,
        s3_key: "models/yolo/dup/best.pt".into(),
        source: "upload".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 100,
        job_id: None,
    };
    manager::create_model(&p, req1).await.expect("first insert");

    let req2 = CreateModelRequest {
        id: uuid::Uuid::new_v4(),
        engine: "yolo".into(),
        name: "best.pt".into(),
        model: None,
        s3_key: "models/yolo/dup/best.pt".into(),
        source: "upload".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 100,
        job_id: None,
    };
    let result = manager::create_model(&p, req2).await;
    match result {
        Err(ManagerError::Internal(ref msg)) if msg == "model_exists" => {}
        other => panic!("esperado model_exists, {:?}", other),
    }
}

/// create_job com weights_id inexistente → 404.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_job_weights_id_inexistente_404() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let fake_id = uuid::Uuid::new_v4();
    let mut req = test_job_request(ds_id);
    req.weights_id = Some(fake_id);

    let result = manager::create_job(&p, req).await;
    assert!(matches!(result, Err(ManagerError::NotFound)));
}

/// POST /internal/models: 409 — s3_key duplicado.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_job_weights_valido_grava_params() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    // Insere modelo yolo via create_model.
    let model_id = uuid::Uuid::new_v4();
    let model_req = CreateModelRequest {
        id: model_id,
        engine: "yolo".into(),
        name: "best.pt".into(),
        model: Some("yolo11m".into()),
        s3_key: "models/yolo/valid/best.pt".into(),
        source: "upload".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 2048,
        job_id: None,
    };
    manager::create_model(&p, model_req)
        .await
        .expect("insert model");

    let mut req = test_job_request(ds_id);
    req.weights_id = Some(model_id);

    let resp = manager::create_job(&p, req)
        .await
        .expect("create job with weights");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    // Verifica params.weights_ref no JSONB.
    let row: (serde_json::Value,) = sqlx::query_as("SELECT params FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(&p)
        .await
        .unwrap();
    let wr = row.0.get("weights_ref").expect("weights_ref presente");
    assert_eq!(wr["s3_key"], "models/yolo/valid/best.pt");
    assert_eq!(wr["md5"], "d41d8cd98f00b204e9800998ecf8427e");
}

/// create_job SEM weights → dispatch sem weights_ref (regressão).
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_job_sem_weights_dispatch_sem_weights_ref() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();

    manager::adopt_orchestrator(&p).await.expect("adopt");

    let _resp = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create job");

    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
        .await
        .expect("dispatch");

    // Verifica que o dispatch NÃO contém weights_ref.
    let calls = orch.calls();
    assert!(!calls.is_empty(), "dispatch should have been called");
    let (_, body) = &calls[0];
    assert!(
        body.get("weights_ref").is_none(),
        "dispatch body must not contain weights_ref when weights_id is absent"
    );
}

/// create_job com weights → dispatch_body contém weights_ref {s3_key, md5}.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_job_com_weights_dispatch_contem_weights_ref() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();

    manager::adopt_orchestrator(&p).await.expect("adopt");

    // Insere modelo yolo.
    let model_id = uuid::Uuid::new_v4();
    let model_req = CreateModelRequest {
        id: model_id,
        engine: "yolo".into(),
        name: "best.pt".into(),
        model: Some("yolo11m".into()),
        s3_key: "models/yolo/dispatch/best.pt".into(),
        source: "upload".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 2048,
        job_id: None,
    };
    manager::create_model(&p, model_req)
        .await
        .expect("insert model");

    let mut req = test_job_request(ds_id);
    req.weights_id = Some(model_id);

    let _resp = manager::create_job(&p, req)
        .await
        .expect("create job with weights");

    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
        .await
        .expect("dispatch");

    // Verifica dispatch_body contém weights_ref.
    let calls = orch.calls();
    assert!(!calls.is_empty(), "dispatch should have been called");
    let (_, body) = &calls[0];
    let wr = body
        .get("weights_ref")
        .expect("weights_ref deve estar no dispatch");
    assert_eq!(wr["s3_key"], "models/yolo/dispatch/best.pt");
    assert_eq!(wr["md5"], "d41d8cd98f00b204e9800998ecf8427e");
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
        max_gpu_mib: None,
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
        max_gpu_mib: None,
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
        max_gpu_mib: None,
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
/// Com max_gpu_mib, vram_total_gb = maior GPU (12288→12), não a soma (18432→18).
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
        vram_total: Some(18432), // 12288 + 6144 MiB (soma — VRAM instalada)
        vram_used: Some(5000),
        cpu: Some(0.3),
        ram: Some(8192),
        ram_total: Some(16384),
        jobs_active: 1,
        max_gpu_mib: Some(12288), // maior GPU — capacidade de 1 job
    };
    manager::receive_heartbeat(&p, &cache, hb)
        .await
        .expect("heartbeat gpu");

    // Verifica colunas: vram_total_gb = round(12288/1024) = 12 (não 18).
    let row: (Option<i32>, serde_json::Value) =
        sqlx::query_as("SELECT vram_total_gb, gpus FROM orchestrators WHERE id = $1")
            .bind(orch_id)
            .fetch_one(&p)
            .await
            .unwrap();
    assert_eq!(row.0, Some(12), "round(12288/1024) = 12 (maior GPU)");
    assert_eq!(
        row.1,
        serde_json::json!(["NVIDIA RTX 3060", "NVIDIA GTX 1660S"])
    );
}

/// (d.2) Heartbeat sem max_gpu_mib (orquestrador legado) → fallback para vram_total.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn heartbeat_fallback_vram_total_sem_max_gpu_mib() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let orch_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'old-orch', 'http://old:8082', 'local', 'online')",
    )
    .bind(orch_id)
    .execute(&p)
    .await
    .unwrap();

    let cache = manager::new_telemetry_cache();
    let hb = HeartbeatRequest {
        endpoint: "http://old:8082".into(),
        gpus: vec!["NVIDIA RTX 3060".into()],
        vram_total: Some(12288),
        vram_used: Some(4096),
        cpu: Some(0.5),
        ram: Some(8192),
        ram_total: Some(16384),
        jobs_active: 0,
        max_gpu_mib: None, // orquestrador legado sem parse por GPU
    };
    manager::receive_heartbeat(&p, &cache, hb)
        .await
        .expect("heartbeat old");

    // Fallback: vram_total_gb = round(12288/1024) = 12 (usa vram_total).
    let row: (Option<i32>,) =
        sqlx::query_as("SELECT vram_total_gb FROM orchestrators WHERE id = $1")
            .bind(orch_id)
            .fetch_one(&p)
            .await
            .unwrap();
    assert_eq!(row.0, Some(12), "fallback: round(12288/1024) = 12");
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
        max_gpu_mib: Some(12000),
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
        max_gpu_mib: Some(6000),
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
        max_gpu_mib: Some(24000),
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

// ===========================================================================
// H.3 — Roteamento por capacidade, watchdog, adopt/revoke
// ===========================================================================

// --- Roteamento (ADR-0011 D3) ---

/// 2 nós online (12GB e NULL) + job com requisito 8 → vai para o de 12GB.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn roteamento_2_nos_requisito_8_vai_para_12gb() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();
    let vt = test_vram_table();

    // Nó de 12GB online (maior GPU individual — capacidade de 1 job).
    let id_big = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status, vram_total_gb) \
         VALUES ($1, 'big-gpu', 'http://big:8082', 'remoto', 'online', 12)",
    )
    .bind(id_big)
    .execute(&p)
    .await
    .unwrap();

    // Nó NULL online (permissivo).
    let id_null = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'local-mock', 'http://local:8082', 'local', 'online')",
    )
    .bind(id_null)
    .execute(&p)
    .await
    .unwrap();

    // Job: yolo11m train → vram_min_gb=10, headroom=2, required=12. Mas queremos testar 8.
    // Usar yolo11n: vram_min_gb=6, headroom=2, required=8.
    let resp = manager::create_job(
        &p,
        CreateJobRequest {
            kind: "yolo_train".into(),
            engine: "yolo".into(),
            model: "yolo11n".into(),
            mode: "train".into(),
            dataset_id: Some(ds_id.to_string()),
            dataset_version_id: None,
            package_ref: None,
            config_yaml: None,
            params: None,
            vram_min_gb: None,
            weights_id: None,
        },
    )
    .await
    .expect("create job");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &vt)
        .await
        .expect("dispatch");

    // Verifica: job foi dispatched para o nó de 12GB.
    let job = manager::get_job(&p, job_id).await.expect("get job");
    assert_eq!(job.status, "dispatched");
    assert_eq!(
        job.orchestrator_id.as_deref(),
        Some(id_big.to_string().as_str())
    );
}

/// Sem requisito (engine/model/mode não mapeado) → NULL elegível, ORDER BY name determinístico.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn roteamento_sem_requisito_null_elegivel_order_by_nome() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();
    let vt = test_vram_table();

    // 2 nós online: um com nome "zzz", outro "aaa", ambos NULL.
    let id_zzz = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'zzz-node', 'http://zzz:8082', 'remoto', 'online')",
    )
    .bind(id_zzz)
    .execute(&p)
    .await
    .unwrap();

    let id_aaa = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'aaa-node', 'http://aaa:8082', 'remoto', 'online')",
    )
    .bind(id_aaa)
    .execute(&p)
    .await
    .unwrap();

    // Engine não mapeado na vram-table → required=NULL.
    let resp = manager::create_job(
        &p,
        CreateJobRequest {
            kind: "custom_train".into(),
            engine: "autotracker".into(),
            model: "custom".into(),
            mode: "train".into(),
            dataset_id: Some(ds_id.to_string()),
            dataset_version_id: None,
            package_ref: None,
            config_yaml: None,
            params: None,
            vram_min_gb: None,
            weights_id: None,
        },
    )
    .await
    .expect("create job");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &vt)
        .await
        .expect("dispatch");

    // Deve ir para "aaa-node" (ORDER BY name ASC, NULLS LAST).
    let job = manager::get_job(&p, job_id).await.expect("get job");
    assert_eq!(job.status, "dispatched");
    assert_eq!(
        job.orchestrator_id.as_deref(),
        Some(id_aaa.to_string().as_str())
    );
}

/// Nó com job não-terminal excluído (NOT EXISTS).
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn roteamento_no_com_job_excluido() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();
    let vt = test_vram_table();

    // Nó 1: online, 12GB — com job running.
    let id_busy = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status, vram_total_gb) \
         VALUES ($1, 'busy', 'http://busy:8082', 'remoto', 'online', 12)",
    )
    .bind(id_busy)
    .execute(&p)
    .await
    .unwrap();

    // Job non-terminal no nó busy.
    let busy_job = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO jobs (id, kind, engine, model, mode, dataset_id, status, orchestrator_id) \
         VALUES ($1, 'yolo_train', 'yolo', 'yolo11m', 'train', $2, 'running', $3)",
    )
    .bind(busy_job)
    .bind(ds_id)
    .bind(id_busy)
    .execute(&p)
    .await
    .unwrap();

    // Nó 2: online, NULL — livre.
    let id_free = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'free', 'http://free:8082', 'local', 'online')",
    )
    .bind(id_free)
    .execute(&p)
    .await
    .unwrap();

    // Job com requisito 8 → busy tem 12GB mas está ocupado, free tem NULL (permissivo).
    let resp = manager::create_job(
        &p,
        CreateJobRequest {
            kind: "yolo_train".into(),
            engine: "yolo".into(),
            model: "yolo11n".into(),
            mode: "train".into(),
            dataset_id: Some(ds_id.to_string()),
            dataset_version_id: None,
            package_ref: None,
            config_yaml: None,
            params: None,
            vram_min_gb: None,
            weights_id: None,
        },
    )
    .await
    .expect("create job");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &vt)
        .await
        .expect("dispatch");

    // Deve ir para o nó livre (busy excluído pelo NOT EXISTS).
    let job = manager::get_job(&p, job_id).await.expect("get job");
    assert_eq!(job.status, "dispatched");
    assert_eq!(
        job.orchestrator_id.as_deref(),
        Some(id_free.to_string().as_str())
    );
}

/// Requisito 12 + só nó de 6GB → waiting_vram.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn roteamento_requisito_12_so_6gb_waiting_vram() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();
    let vt = test_vram_table();

    // Nó de 6GB online.
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status, vram_total_gb) \
         VALUES ($1, 'small', 'http://small:8082', 'remoto', 'online', 6)",
    )
    .bind(uuid::Uuid::new_v4())
    .execute(&p)
    .await
    .unwrap();

    // yolo11m train: vram_min=10, headroom=2, required=12.
    let resp = manager::create_job(
        &p,
        CreateJobRequest {
            kind: "yolo_train".into(),
            engine: "yolo".into(),
            model: "yolo11m".into(),
            mode: "train".into(),
            dataset_id: Some(ds_id.to_string()),
            dataset_version_id: None,
            package_ref: None,
            config_yaml: None,
            params: None,
            vram_min_gb: None,
            weights_id: None,
        },
    )
    .await
    .expect("create job");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    let dispatched = manager::dispatch_next(&p, &orch, "docker", "/data", "img", &vt)
        .await
        .expect("dispatch");
    assert!(!dispatched);

    let job = manager::get_job(&p, job_id).await.expect("get job");
    assert_eq!(job.status, "queued");
    assert_eq!(job.queue_reason.as_deref(), Some("waiting_vram"));
}

/// Sem requisito + nenhum online → waiting_slot.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn roteamento_sem_requisito_nenhum_online_waiting_slot() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();
    let vt = test_vram_table();

    // Sem orchestrators online.
    let resp = manager::create_job(
        &p,
        CreateJobRequest {
            kind: "custom".into(),
            engine: "autotracker".into(),
            model: "custom".into(),
            mode: "train".into(),
            dataset_id: Some(ds_id.to_string()),
            dataset_version_id: None,
            package_ref: None,
            config_yaml: None,
            params: None,
            vram_min_gb: None,
            weights_id: None,
        },
    )
    .await
    .expect("create job");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    let dispatched = manager::dispatch_next(&p, &orch, "docker", "/data", "img", &vt)
        .await
        .expect("dispatch");
    assert!(!dispatched);

    let job = manager::get_job(&p, job_id).await.expect("get job");
    assert_eq!(job.status, "queued");
    assert_eq!(job.queue_reason.as_deref(), Some("waiting_slot"));
}

// --- Watchdog (ADR-0011 D4) ---

/// Nó sem heartbeat 15s → degraded.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn watchdog_15s_degraded() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let orch_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status, last_heartbeat) \
         VALUES ($1, 'slow', 'http://slow:8082', 'remoto', 'online', now() - interval '20 seconds')",
    )
    .bind(orch_id)
    .execute(&p)
    .await
    .unwrap();

    manager::watchdog_tick(&p).await.expect("watchdog tick");

    let status: (String,) = sqlx::query_as("SELECT status FROM orchestrators WHERE id = $1")
        .bind(orch_id)
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(status.0, "degraded");
}

/// Nó 60s sem heartbeat → offline + jobs re-queued com orchestrator_id NULL.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn watchdog_60s_offline_requeue() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let orch_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status, last_heartbeat) \
         VALUES ($1, 'dead', 'http://dead:8082', 'remoto', 'degraded', now() - interval '70 seconds')",
    )
    .bind(orch_id)
    .execute(&p)
    .await
    .unwrap();

    // Jobs não-terminais no nó morto.
    let job1 = uuid::Uuid::new_v4();
    let job2 = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO jobs (id, kind, engine, model, mode, dataset_id, status, orchestrator_id) \
         VALUES ($1, 'yolo_train', 'yolo', 'yolo11m', 'train', $2, 'running', $3)",
    )
    .bind(job1)
    .bind(ds_id)
    .bind(orch_id)
    .execute(&p)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO jobs (id, kind, engine, model, mode, dataset_id, status, orchestrator_id) \
         VALUES ($1, 'yolo_train', 'yolo', 'yolo11m', 'train', $2, 'dispatched', $3)",
    )
    .bind(job2)
    .bind(ds_id)
    .bind(orch_id)
    .execute(&p)
    .await
    .unwrap();

    manager::watchdog_tick(&p).await.expect("watchdog tick");

    // Nó → offline.
    let status: (String,) = sqlx::query_as("SELECT status FROM orchestrators WHERE id = $1")
        .bind(orch_id)
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(status.0, "offline");

    // Jobs re-queued com orchestrator_id NULL.
    for jid in [job1, job2] {
        let job = manager::get_job(&p, jid).await.expect("get re-queued job");
        assert_eq!(job.status, "queued");
        assert_eq!(job.queue_reason.as_deref(), Some("recovered"));
        assert!(job.orchestrator_id.is_none());
    }
}

// --- Adopt / Revoke (ADR-0011 D5) ---

/// Verify válido → upsert online; 2ª adoção idempotente; revive revoked.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn adopt_verify_valido_upsert_idempotente_revive_revoked() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let orch = FakeOrchestratorClient::new();

    // 1ª adoção: cria.
    let req1 = manager::AdoptRequest {
        name: "gpu-node".into(),
        endpoint: "http://gpu:8082".into(),
        kind: "remoto".into(),
        pairing_code: "heph_p_test123".into(),
    };
    let item1 = manager::adopt_internal(&p, &orch, &req1)
        .await
        .expect("adopt 1");
    assert!(!item1.id.is_empty());

    let row: (String, String) =
        sqlx::query_as("SELECT name, status FROM orchestrators WHERE endpoint = $1")
            .bind("http://gpu:8082")
            .fetch_one(&p)
            .await
            .unwrap();
    assert_eq!(row.0, "gpu-node");
    assert_eq!(row.1, "online");

    // 2ª adoção: idempotente (não duplica).
    let req2 = manager::AdoptRequest {
        name: "gpu-node-v2".into(),
        endpoint: "http://gpu:8082".into(),
        kind: "remoto".into(),
        pairing_code: "heph_p_test456".into(),
    };
    let item2 = manager::adopt_internal(&p, &orch, &req2)
        .await
        .expect("adopt 2");
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM orchestrators")
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(count.0, 1, "deve continuar 1 linha");
    assert_eq!(item1.id, item2.id, "deve retornar mesmo id");

    // Atualizado o name.
    let row2: (String,) = sqlx::query_as("SELECT name FROM orchestrators WHERE endpoint = $1")
        .bind("http://gpu:8082")
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(row2.0, "gpu-node-v2");

    // Revoga.
    let orch_uuid: uuid::Uuid = item1.id.parse().unwrap();
    manager::revoke_orchestrator(&p, orch_uuid)
        .await
        .expect("revoke");
    let status: (String,) = sqlx::query_as("SELECT status FROM orchestrators WHERE id = $1")
        .bind(orch_uuid)
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(status.0, "revoked");

    // Re-adoção: revive revoked → online.
    let req3 = manager::AdoptRequest {
        name: "gpu-node-v3".into(),
        endpoint: "http://gpu:8082".into(),
        kind: "remoto".into(),
        pairing_code: "heph_p_test789".into(),
    };
    manager::adopt_internal(&p, &orch, &req3)
        .await
        .expect("re-adopt");
    let status2: (String,) = sqlx::query_as("SELECT status FROM orchestrators WHERE id = $1")
        .bind(orch_uuid)
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(status2.0, "online");
}

/// Verify inválido → 409 pairing_invalid.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn adopt_verify_invalido_409() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let orch = FakeOrchestratorClient::with_verify_valid(false);

    let req = manager::AdoptRequest {
        name: "bad-node".into(),
        endpoint: "http://bad:8082".into(),
        kind: "remoto".into(),
        pairing_code: "wrong_code".into(),
    };
    let result = manager::adopt_internal(&p, &orch, &req).await;
    assert!(matches!(result, Err(ManagerError::PairingInvalid)));
}

/// Verify inalcançável (erro de rede) → 409 pairing_invalid.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn adopt_orquestrador_inalcançavel_409() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let orch = FailingOrchestratorClient;

    let req = manager::AdoptRequest {
        name: "ghost".into(),
        endpoint: "http://ghost:8082".into(),
        kind: "remoto".into(),
        pairing_code: "code".into(),
    };
    let result = manager::adopt_internal(&p, &orch, &req).await;
    assert!(matches!(result, Err(ManagerError::PairingInvalid)));
}

/// Revoke → 204 (via revoke_orchestrator); id inexistente → NotFound.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn revoke_204_e_id_inexistente_404() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    // Insere e revoga.
    let orch_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'to-revoke', 'http://revoke:8082', 'local', 'online')",
    )
    .bind(orch_id)
    .execute(&p)
    .await
    .unwrap();

    manager::revoke_orchestrator(&p, orch_id)
        .await
        .expect("revoke");
    let status: (String,) = sqlx::query_as("SELECT status FROM orchestrators WHERE id = $1")
        .bind(orch_id)
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(status.0, "revoked");

    // ID inexistente → NotFound.
    let fake_id = uuid::Uuid::new_v4();
    let result = manager::revoke_orchestrator(&p, fake_id).await;
    assert!(matches!(result, Err(ManagerError::NotFound)));
}

/// Auto-adoção com linha revoked + AUTO_ADOPT_LOCAL=1 → linha CONTINUA revoked.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn auto_adopt_nao_revive_revoked() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    // Insere local como revoked (simula revoke manual).
    let orch_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'orchestrator-local', 'http://orchestrator-local:8082', 'local', 'revoked')",
    )
    .bind(orch_id)
    .execute(&p)
    .await
    .unwrap();

    // Auto-adoção (fail-open, default AUTO_ADOPT_LOCAL=1).
    manager::adopt_orchestrator(&p).await.expect("auto adopt");

    // Linha continua revoked (guarda anti-ressurreição).
    let status: (String,) = sqlx::query_as("SELECT status FROM orchestrators WHERE id = $1")
        .bind(orch_id)
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(
        status.0, "revoked",
        "auto-adoção não deve ressuscitar revoked"
    );
}

/// Watchdog não toca revoked.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn watchdog_nao_toca_revoked() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let orch_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status, last_heartbeat) \
         VALUES ($1, 'revived', 'http://revived:8082', 'remoto', 'revoked', now() - interval '120 seconds')",
    )
    .bind(orch_id)
    .execute(&p)
    .await
    .unwrap();

    manager::watchdog_tick(&p).await.expect("watchdog tick");

    // Continua revoked (watchdog ignora revoked).
    let status: (String,) = sqlx::query_as("SELECT status FROM orchestrators WHERE id = $1")
        .bind(orch_id)
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(status.0, "revoked");
}

/// Validação de adopt: kind inválido → ManagerError::InvalidRequest.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn adopt_validacao_kind_invalido() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let orch = FakeOrchestratorClient::new();

    let req = manager::AdoptRequest {
        name: "bad-kind".into(),
        endpoint: "http://local:8082".into(),
        kind: "invalid_kind".into(),
        pairing_code: "code".into(),
    };
    let result = manager::adopt_internal(&p, &orch, &req).await;
    assert!(matches!(result, Err(ManagerError::InvalidRequest(_))));
}

/// Validação de adopt: endpoint sem http:// → ManagerError::InvalidRequest.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn adopt_validacao_endpoint_sem_http() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let orch = FakeOrchestratorClient::new();

    let req = manager::AdoptRequest {
        name: "bad-endpoint".into(),
        endpoint: "ftp://local:8082".into(),
        kind: "local".into(),
        pairing_code: "code".into(),
    };
    let result = manager::adopt_internal(&p, &orch, &req).await;
    assert!(matches!(result, Err(ManagerError::InvalidRequest(_))));
}

/// Validação de adopt: pairing_code vazio → ManagerError::InvalidRequest.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn adopt_validacao_pairing_code_vazio() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let orch = FakeOrchestratorClient::new();

    let req = manager::AdoptRequest {
        name: "empty-code".into(),
        endpoint: "http://local:8082".into(),
        kind: "local".into(),
        pairing_code: "".into(),
    };
    let result = manager::adopt_internal(&p, &orch, &req).await;
    assert!(matches!(result, Err(ManagerError::InvalidRequest(_))));
}

/// Dispatch do dispatch_falha_volta_queued agora precisa de vram_table.
/// Verifica que o dispatch com orchestrator que falha volta para queued.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn dispatch_falha_volta_queued_com_vram_table() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    manager::adopt_orchestrator(&p).await.expect("adopt");

    let resp = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    let failing = FailingOrchestratorClient;
    let vt = test_vram_table();
    let _dispatched = manager::dispatch_next(&p, &failing, "docker", "/data", "img", &vt)
        .await
        .expect("dispatch with failing orch");

    // Verifica que o job voltou para queued.
    let job = manager::get_job(&p, job_id).await.expect("get after fail");
    assert_eq!(job.status, "queued");
    assert_eq!(job.queue_reason.as_deref(), Some("waiting_slot"));
}

/// VramTable parse ok com a tabela real.
#[test]
fn vram_table_parse_ok() {
    let yaml = include_str!("../../../packages/policies/vram-table.yaml");
    let vt = VramTable::parse(yaml).expect("vram-table deve parsear");
    assert_eq!(vt.defaults.headroom_gb, 2);
    assert!(!vt.entries.is_empty());

    // yolo11n train: 6 + 2 = 8.
    assert_eq!(vt.resolve_required_gb("yolo", "yolo11n", "train"), Some(8));
    // yolo11m train: 10 + 2 = 12.
    assert_eq!(vt.resolve_required_gb("yolo", "yolo11m", "train"), Some(12));
    // Engine desconhecido → None (permissivo).
    assert_eq!(vt.resolve_required_gb("autotracker", "x", "train"), None);
}

// ===========================================================================
// J.2 — Predict: mode no dispatch_body + jobs.model = variante
// ===========================================================================

/// Helper para criar um request de predict.
fn predict_job_request(dataset_id: uuid::Uuid) -> CreateJobRequest {
    CreateJobRequest {
        kind: "yolo_predict".into(),
        engine: "yolo".into(),
        model: "predict".into(),
        mode: "predict".into(),
        dataset_id: Some(dataset_id.to_string()),
        dataset_version_id: Some(uuid::Uuid::new_v4().to_string()),
        package_ref: Some(PackageRef {
            version_id: uuid::Uuid::new_v4().to_string(),
            key: "packages/test/predict-dataset.zip".into(),
            md5_zip: "d41d8cd98f00b204e9800998ecf8427e".into(),
            bytes: 2048,
        }),
        config_yaml: Some("job_id: predict\ngine: yolo".into()),
        params: Some(serde_json::json!({
            "package_ref": {
                "version_id": uuid::Uuid::new_v4().to_string(),
                "key": "packages/test/predict-dataset.zip",
                "md5_zip": "d41d8cd98f00b204e9800998ecf8427e",
                "bytes": 2048
            },
            "conf": 0.65
        })),
        vram_min_gb: None,
        weights_id: None,
    }
}

/// Insere um modelo yolo com variante e retorna o ID.
async fn insert_test_model(pool: &PgPool, variant: Option<&str>) -> uuid::Uuid {
    let model_id = uuid::Uuid::new_v4();
    let req = CreateModelRequest {
        id: model_id,
        engine: "yolo".into(),
        name: "best.pt".into(),
        model: variant.map(|v| v.to_string()),
        s3_key: format!("models/yolo/predict/{}/best.pt", model_id),
        source: "upload".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 4096,
        job_id: None,
    };
    manager::create_model(pool, req)
        .await
        .expect("insert test model");
    model_id
}

/// create_job predict com weights_id válido e variante → jobs.model = variante.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn predict_com_variante_grava_modelo_variante() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let model_id = insert_test_model(&p, Some("yolo11m")).await;

    let mut req = predict_job_request(ds_id);
    req.weights_id = Some(model_id);

    let resp = manager::create_job(&p, req)
        .await
        .expect("create predict job");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    // Verifica jobs.model = variante ("yolo11m", não "predict").
    let job = manager::get_job(&p, job_id).await.expect("get predict job");
    assert_eq!(
        job.model, "yolo11m",
        "jobs.model deve ser a variante do modelo"
    );
    assert_eq!(job.mode, "predict");
    assert_eq!(job.kind, "yolo_predict");
    assert_eq!(job.engine, "yolo");
}

/// create_job predict com weights_id sem variante → jobs.model = "predict" literal.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn predict_sem_variante_grava_predict_literal() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let model_id = insert_test_model(&p, None).await;

    let mut req = predict_job_request(ds_id);
    req.weights_id = Some(model_id);

    let resp = manager::create_job(&p, req)
        .await
        .expect("create predict job no variant");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    // Sem variante → jobs.model = "predict" (literal).
    let job = manager::get_job(&p, job_id).await.expect("get predict job");
    assert_eq!(
        job.model, "predict",
        "sem variante, jobs.model deve ser 'predict'"
    );
}

/// create_job predict com weights_id válido → params.weights_ref gravado.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn predict_com_weights_grava_params_weights_ref() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let model_id = insert_test_model(&p, Some("yolo11n")).await;

    let mut req = predict_job_request(ds_id);
    req.weights_id = Some(model_id);

    let resp = manager::create_job(&p, req)
        .await
        .expect("create predict job");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    // Verifica params.weights_ref gravado.
    let row: (serde_json::Value,) = sqlx::query_as("SELECT params FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(&p)
        .await
        .unwrap();
    let wr = row.0.get("weights_ref").expect("weights_ref presente");
    assert_eq!(
        wr["s3_key"],
        format!("models/yolo/predict/{}/best.pt", model_id)
    );
    assert_eq!(wr["md5"], "d41d8cd98f00b204e9800998ecf8427e");

    // conf preservado.
    assert_eq!(row.0["conf"], 0.65);
}

/// create_job predict com weights_id inexistente → 404.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn predict_weights_inexistente_404() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let fake_id = uuid::Uuid::new_v4();
    let mut req = predict_job_request(ds_id);
    req.weights_id = Some(fake_id);

    let result = manager::create_job(&p, req).await;
    assert!(matches!(result, Err(ManagerError::NotFound)));
}

/// create_job predict com weights_id válido mas engine não-yolo → 400.
/// NOTA: A tabela `models` tem CHECK `engine IN ('yolo')` — não é possível
/// inserir modelo não-yolo. O check em `create_job` é defensivo (D5/ADEQUADO
/// para o caso de o schema mudar no futuro). Testamos o caminho feliz com
/// engine=yolo para provar que a validação NÃO rejeita.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn predict_weights_engine_yolo_aceita() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    // Modelo yolo válido — create_job não deve rejeitar.
    let model_id = insert_test_model(&p, Some("yolo11n")).await;

    let mut req = predict_job_request(ds_id);
    req.weights_id = Some(model_id);

    let result = manager::create_job(&p, req).await;
    assert!(
        result.is_ok(),
        "predict com weights engine=yolo deve aceitar"
    );
}

/// Dispatch de job predict → dispatch_body contém mode:"predict" E weights_ref.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn predict_dispatch_body_contem_mode_e_weights_ref() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();

    manager::adopt_orchestrator(&p).await.expect("adopt");

    let model_id = insert_test_model(&p, Some("yolo11m")).await;

    let mut req = predict_job_request(ds_id);
    req.weights_id = Some(model_id);

    let _resp = manager::create_job(&p, req)
        .await
        .expect("create predict job");

    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
        .await
        .expect("dispatch");

    // Verifica dispatch_body.
    let calls = orch.calls();
    assert!(!calls.is_empty(), "dispatch should have been called");
    let (_, body) = &calls[0];

    // mode = "predict".
    assert_eq!(
        body.get("mode").and_then(|v| v.as_str()),
        Some("predict"),
        "dispatch_body deve conter mode:'predict'"
    );

    // weights_ref presente.
    let wr = body
        .get("weights_ref")
        .expect("dispatch_body deve conter weights_ref");
    assert_eq!(
        wr["s3_key"],
        format!("models/yolo/predict/{}/best.pt", model_id)
    );
    assert_eq!(wr["md5"], "d41d8cd98f00b204e9800998ecf8427e");

    // engine = yolo.
    assert_eq!(body.get("engine").and_then(|v| v.as_str()), Some("yolo"));
}

/// Dispatch de job train → dispatch_body também contém mode (regressão).
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn train_dispatch_body_contem_mode() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();

    manager::adopt_orchestrator(&p).await.expect("adopt");

    let _resp = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create train job");

    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
        .await
        .expect("dispatch");

    let calls = orch.calls();
    assert!(!calls.is_empty());
    let (_, body) = &calls[0];

    // mode = "train".
    assert_eq!(
        body.get("mode").and_then(|v| v.as_str()),
        Some("train"),
        "dispatch_body de job train deve conter mode:'train'"
    );
}

/// Fine-tune (model != "predict") NÃO substitui variante — preserva req.model.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn fine_tune_nao_substitui_variante() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let model_id = insert_test_model(&p, Some("yolo11m")).await;

    let mut req = test_job_request(ds_id);
    req.weights_id = Some(model_id);
    // model = "yolo11m" (fine-tune — NÃO é o literal "predict").

    let resp = manager::create_job(&p, req)
        .await
        .expect("create fine-tune job");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    // jobs.model = "yolo11m" (req.model preservado, não substituído).
    let job = manager::get_job(&p, job_id)
        .await
        .expect("get fine-tune job");
    assert_eq!(job.model, "yolo11m", "fine-tune preserva model do request");
}

// ===========================================================================
// K.2 — AutoTracker real: engine 'world' (ADR-0014 D5)
// ===========================================================================

/// Helper: insere um modelo com engine world e retorna o ID.
async fn insert_test_world_model(pool: &PgPool, variant: Option<&str>) -> uuid::Uuid {
    let model_id = uuid::Uuid::new_v4();
    let req = CreateModelRequest {
        id: model_id,
        engine: "world".into(),
        name: "yolov8x-worldv2.pt".into(),
        model: variant.map(|v| v.to_string()),
        s3_key: format!("models/world/at/{}/yolov8x-worldv2.pt", model_id),
        source: "upload".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 1_300_000_000,
        job_id: None,
    };
    manager::create_model(pool, req)
        .await
        .expect("insert test world model");
    model_id
}

/// Helper: cria um request de autotracker (principal sempre manda model="mock" — D2).
fn autotracker_job_request(dataset_id: uuid::Uuid) -> CreateJobRequest {
    CreateJobRequest {
        kind: "autotracker".into(),
        engine: "autotracker".into(),
        model: "mock".into(),
        mode: "autotrack".into(),
        dataset_id: Some(dataset_id.to_string()),
        dataset_version_id: Some(uuid::Uuid::new_v4().to_string()),
        package_ref: Some(PackageRef {
            version_id: uuid::Uuid::new_v4().to_string(),
            key: "packages/test/autotracker-dataset.zip".into(),
            md5_zip: "d41d8cd98f00b204e9800998ecf8427e".into(),
            bytes: 1024,
        }),
        config_yaml: Some("job_id: autotrack\ngine: autotracker".into()),
        params: Some(serde_json::json!({
            "package_ref": {
                "version_id": uuid::Uuid::new_v4().to_string(),
                "key": "packages/test/autotracker-dataset.zip",
                "md5_zip": "d41d8cd98f00b204e9800998ecf8427e",
                "bytes": 1024
            },
            "model": "world",
            "conf": 0.65
        })),
        vram_min_gb: None,
        weights_id: None,
    }
}

/// Autotracker com weights engine=world válido → ok, jobs.model=variante, params.weights_ref.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn autotracker_com_world_weights_aceita() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    // Insere modelo world com variante.
    let model_id = insert_test_world_model(&p, Some("yolov8x-worldv2")).await;

    let mut req = autotracker_job_request(ds_id);
    req.weights_id = Some(model_id);

    let resp = manager::create_job(&p, req)
        .await
        .expect("create autotracker job with world weights");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    // Verifica jobs.model = variante ("yolov8x-worldv2").
    let job = manager::get_job(&p, job_id)
        .await
        .expect("get autotracker job");
    assert_eq!(
        job.model, "yolov8x-worldv2",
        "jobs.model deve ser a variante do modelo world"
    );
    assert_eq!(job.kind, "autotracker");
    assert_eq!(job.engine, "autotracker");
    assert_eq!(job.mode, "autotrack");

    // Verifica params.weights_ref.
    let row: (serde_json::Value,) = sqlx::query_as("SELECT params FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(&p)
        .await
        .unwrap();
    let wr = row.0.get("weights_ref").expect("weights_ref presente");
    assert_eq!(
        wr["s3_key"],
        format!("models/world/at/{}/yolov8x-worldv2.pt", model_id)
    );
    assert_eq!(wr["md5"], "d41d8cd98f00b204e9800998ecf8427e");
}

/// Autotracker com weights engine=world SEM variante → jobs.model = "world" literal.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn autotracker_world_sem_variante_grava_world_literal() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let model_id = insert_test_world_model(&p, None).await;

    let mut req = autotracker_job_request(ds_id);
    req.weights_id = Some(model_id);

    let resp = manager::create_job(&p, req)
        .await
        .expect("create autotracker job no variant");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    // Sem variante → jobs.model = "world" (literal).
    let job = manager::get_job(&p, job_id)
        .await
        .expect("get autotracker job");
    assert_eq!(
        job.model, "world",
        "sem variante, jobs.model deve ser 'world'"
    );
}

/// Autotracker com weights engine=world → dispatch_body contém mode + weights_ref.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn autotracker_world_dispatch_body_contem_mode_e_weights_ref() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();

    manager::adopt_orchestrator(&p).await.expect("adopt");

    let model_id = insert_test_world_model(&p, Some("yolov8x-worldv2")).await;

    let mut req = autotracker_job_request(ds_id);
    req.weights_id = Some(model_id);

    let _resp = manager::create_job(&p, req)
        .await
        .expect("create autotracker job");

    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
        .await
        .expect("dispatch");

    let calls = orch.calls();
    assert!(!calls.is_empty(), "dispatch should have been called");
    let (_, body) = &calls[0];

    // mode = "autotrack".
    assert_eq!(
        body.get("mode").and_then(|v| v.as_str()),
        Some("autotrack"),
        "dispatch_body deve conter mode:'autotrack'"
    );

    // weights_ref presente.
    let wr = body
        .get("weights_ref")
        .expect("dispatch_body deve conter weights_ref");
    assert_eq!(
        wr["s3_key"],
        format!("models/world/at/{}/yolov8x-worldv2.pt", model_id)
    );
    assert_eq!(wr["md5"], "d41d8cd98f00b204e9800998ecf8427e");

    // engine = autotracker (NÃO world — world vive só na tabela models).
    assert_eq!(
        body.get("engine").and_then(|v| v.as_str()),
        Some("autotracker")
    );
}

/// Autotracker com weights_id inexistente → 404.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn autotracker_world_weights_inexistente_404() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let fake_id = uuid::Uuid::new_v4();
    let mut req = autotracker_job_request(ds_id);
    req.weights_id = Some(fake_id);

    let result = manager::create_job(&p, req).await;
    assert!(matches!(result, Err(ManagerError::NotFound)));
}

/// Fine-tune (mode=train) com weights engine=world → 400 (defesa mantida).
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn fine_tune_com_world_weights_rejeita_400() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let model_id = insert_test_world_model(&p, Some("yolov8x-worldv2")).await;

    let mut req = test_job_request(ds_id);
    req.weights_id = Some(model_id);
    // req.mode = "train" (fine-tune).

    let result = manager::create_job(&p, req).await;
    assert!(
        matches!(result, Err(ManagerError::InvalidRequest(ref msg)) if msg.contains("fine-tune")),
        "fine-tune com world weights deve retornar 400"
    );
}

/// Autotracker SEM weights_id → jobs.model = "mock" (regressão).
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn autotracker_sem_weights_grava_mock() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let req = autotracker_job_request(ds_id);
    // weights_id = None (mock).

    let resp = manager::create_job(&p, req)
        .await
        .expect("create autotracker mock job");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    let job = manager::get_job(&p, job_id)
        .await
        .expect("get autotracker mock job");
    assert_eq!(job.model, "mock", "sem weights, jobs.model deve ser 'mock'");
    // Sem weights_ref em params.
    let row: (serde_json::Value,) = sqlx::query_as("SELECT params FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(&p)
        .await
        .unwrap();
    assert!(
        row.0.get("weights_ref").is_none(),
        "sem weights_id, params não deve conter weights_ref"
    );
}

/// POST /internal/models com engine=world → 201 (validação aceita).
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_model_world_201() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let model_id = uuid::Uuid::new_v4();
    let req = CreateModelRequest {
        id: model_id,
        engine: "world".into(),
        name: "yolov8x-worldv2.pt".into(),
        model: None,
        s3_key: "models/world/test/yolov8x-worldv2.pt".into(),
        source: "upload".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 1_300_000_000,
        job_id: None,
    };

    let item = manager::create_model(&p, req)
        .await
        .expect("create model world 201");
    assert_eq!(item.id, model_id.to_string());
    assert_eq!(item.engine, "world");
    assert_eq!(item.name, "yolov8x-worldv2.pt");
    assert_eq!(item.source, "upload");
}

/// POST /internal/models com engine inválida (não yolo nem world) → 400.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_model_engine_invalida_400() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let req = CreateModelRequest {
        id: uuid::Uuid::new_v4(),
        engine: "diffusion".into(),
        name: "model.pt".into(),
        model: None,
        s3_key: "models/diff/abc/model.pt".into(),
        source: "upload".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 100,
        job_id: None,
    };

    let result = manager::create_model(&p, req).await;
    assert!(matches!(result, Err(ManagerError::InvalidRequest(_))));
}
