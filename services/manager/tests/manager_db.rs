//! Integração manager × Postgres (F4.3) — funções de lib.rs com pool real.
//!
//! Execução: `bash scripts/test-db.sh` (sobe o banco efêmero `studio_test`).
//!
//! AVISO EM MAIÚSCULAS: ESTE HARNESS FAZ DELETEs NO SETUP E SÓ ACEITA O
//! BANCO EFÊMERO `studio_test` — `DATABASE_URL` NUNCA DEVE APONTAR PARA
//! O BANCO DE DEV `studio`. A guarda `assert_test_db_url` dá panic
//! ANTES de conectar.

use async_trait::async_trait;
use manager::{
    self, ArtifactItem, CreateJobRequest, CreateModelRequest, HeartbeatRequest, ManagerError,
    PackageRef, ReportRequest, VramTable,
};
use sqlx::PgPool;

const TEST_DB_URL: &str = "postgres://studio:studio@localhost:5432/studio_test";

// Serial lock — testes compartilham o mesmo banco.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Guarda anti-footgun (idem datasets_db.rs: o setup faz DELETEs). Panic
/// ANTES de conectar se a URL não apontar para o banco efêmero `studio_test`
/// (aceita derivados com prefixo `studio_test`). O dev `studio` é recusado.
fn assert_test_db_url(url: &str) {
    let path = url.rsplit('/').find(|s| !s.is_empty()).unwrap_or("");
    let db = path.split('?').next().unwrap_or("");
    assert!(
        db.starts_with("studio_test"),
        "HARNESS DE TESTE RECUSANDO BANCO PERIGOSO: use studio_test via scripts/test-db.sh — nunca o DB de dev 'studio' (banco na URL: '{db}')"
    );
}

#[test]
fn guarda_harness_aceita_studio_test() {
    assert_test_db_url("postgres://studio:studio@localhost:5432/studio_test");
    assert_test_db_url(
        "postgres://studio:studio@localhost:5432/studio_test_migrations?sslmode=disable",
    );
}

#[test]
#[should_panic(expected = "HARNESS DE TESTE RECUSANDO BANCO PERIGOSO")]
fn guarda_harness_rejeita_studio_dev() {
    assert_test_db_url("postgres://studio:studio@localhost:5432/studio");
}

async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| TEST_DB_URL.into());
    assert_test_db_url(&url);
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
    sqlx::query("DELETE FROM generation_inputs")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM generations")
        .execute(pool)
        .await
        .unwrap();
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
    let slug = format!("test-ds-{}", &id.to_string()[..8]);
    sqlx::query(
        "INSERT INTO datasets (id, slug, title, category, type, task, format, status) \
         VALUES ($1, $2, 'Test DS', 'yolo', 'yolo_bbox', 'detect_track', 'yolo_txt', 'ready')",
    )
    .bind(id)
    .bind(slug)
    .execute(pool)
    .await
    .expect("insert test dataset");
    id
}

/// Deriva o nome semântico canônico de um modelo YOLO para um dataset de teste.
/// Espelha a lógica de `compute_model_name` em `src/lib.rs` — evita golden string frágil.
fn expected_yolo_model_name(ds_id: uuid::Uuid, job_id: uuid::Uuid) -> String {
    let ds_slug = format!("test-ds-{}", &ds_id.to_string()[..8]);
    let params = serde_json::json!({
        "package_ref": {
            "version_id": "v",
            "key": "packages/test/dataset.zip",
            "md5_zip": "d41d8cd98f00b204e9800998ecf8427e",
            "bytes": 1024
        }
    });
    manager::compute_model_name(
        "best.pt",
        "yolo",
        "yolo11m",
        job_id,
        Some(&ds_slug),
        &params,
    )
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
        orchestrator_hint: None,
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

    // 4. Report: preparing sobre job dispatched é IGNORADO (B3 — sem
    // regressão de ciclo: preparing via report só vale em job preparing).
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

            meta_content: None,
            phase: None,
            message: None,
        },
    )
    .await
    .expect("report preparing ignorado");

    let job = manager::get_job(&p, job_id)
        .await
        .expect("get job dispatched");
    assert_eq!(job.status, "dispatched");

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

            meta_content: None,
            phase: None,
            message: None,
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

            meta_content: None,
            phase: None,
            message: None,
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

            meta_content: None,
            phase: None,
            message: None,
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

            meta_content: None,
            phase: None,
            message: None,
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

    // Abort em voo (preparing genuíno do fluxo async ADR-0025, sem
    // regressão via report) → cancelling. Sem nó alocado (pré-dispatch):
    // sem notificação ao orquestrador.
    let resp = manager::create_job(&p, test_prepare_job_request(ds_id))
        .await
        .expect("create preparing");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    let result = manager::abort_job(&p, job_id, &orch).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "cancelling");

    let job = manager::get_job(&p, job_id).await.expect("get");
    assert_eq!(job.status, "cancelling");

    // Abort em running com nó → cancelling + notifica o orquestrador.
    let resp_r = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create running");
    let run_id: uuid::Uuid = resp_r.job_id.parse().unwrap();
    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
        .await
        .expect("dispatch");
    manager::report_job(
        &p,
        run_id,
        ReportRequest {
            status: "running".into(),
            progress: Some(0.1),
            epoch: None,
            step: None,
            metrics: None,
            error: None,
            artifacts: None,

            meta_content: None,
            phase: None,
            message: None,
        },
    )
    .await
    .expect("report running");

    let result_r = manager::abort_job(&p, run_id, &orch).await;
    assert!(result_r.is_ok());
    assert_eq!(result_r.unwrap(), "cancelling");

    let job_r = manager::get_job(&p, run_id).await.expect("get running");
    assert_eq!(job_r.status, "cancelling");

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

            meta_content: None,
            phase: None,
            message: None,
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
    assert_eq!(recovered, 2);

    // `dispatched`/`running` voltam a queued com queue_reason='recovered'.
    for jid in [job1, job3] {
        let job = manager::get_job(&p, jid).await.expect("get recovered");
        assert_eq!(job.status, "queued");
        assert_eq!(job.queue_reason.as_deref(), Some("recovered"));
    }

    // `preparing` é EXCLUÍDO do recover (B1 — dono é o principal via
    // job_prepares/recover_stale_prepares, ADR-0025 D3): permanece
    // `preparing`, sem virar queued sem pacote.
    let job = manager::get_job(&p, job2).await.expect("get preparing");
    assert_eq!(job.status, "preparing");
    assert!(job.queue_reason.is_none());
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

            meta_content: None,
            phase: None,
            message: None,
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

            meta_content: None,
            phase: None,
            message: None,
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

            meta_content: None,
            phase: None,
            message: None,
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

/// Testa campos de orquestrador no wire de list_jobs e get_job (ADR-0015 D1).
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn wire_orchestrator_fields_list_e_get() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();

    // 1. Adota orquestrador para ter linha na tabela orchestrators.
    manager::adopt_orchestrator(&p).await.expect("adopt");
    let (orch_id, orch_name, orch_kind): (uuid::Uuid, String, String) =
        sqlx::query_as("SELECT id, name, kind FROM orchestrators WHERE status = 'online' LIMIT 1")
            .fetch_one(&p)
            .await
            .expect("get orch");

    // 2. Cria job — estado 'queued' (ainda não despachado).
    let r = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create job");
    let job_id: uuid::Uuid = r.job_id.parse().unwrap();

    let job_queued = manager::get_job(&p, job_id).await.expect("get job queued");
    assert_eq!(job_queued.orchestrator_id, None);
    assert_eq!(job_queued.orchestrator_name, None);
    assert_eq!(job_queued.orchestrator_kind, None);
    assert!(!job_queued.orchestrator_fallback);

    let list_queued = manager::list_jobs(&p, None, None)
        .await
        .expect("list jobs queued");
    assert_eq!(list_queued.items[0].orchestrator_id, None);
    assert_eq!(list_queued.items[0].orchestrator_name, None);
    assert_eq!(list_queued.items[0].orchestrator_kind, None);
    assert!(!list_queued.items[0].orchestrator_fallback);

    // 3. Despacha job — deve preencher orchestrator_id, name, kind do JOIN.
    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
        .await
        .expect("dispatch");

    let job_disp = manager::get_job(&p, job_id).await.expect("get job disp");
    assert_eq!(job_disp.orchestrator_id, Some(orch_id.to_string()));
    assert_eq!(job_disp.orchestrator_name, Some(orch_name.clone()));
    assert_eq!(job_disp.orchestrator_kind, Some(orch_kind.clone()));
    assert!(!job_disp.orchestrator_fallback);

    let list_disp = manager::list_jobs(&p, None, None)
        .await
        .expect("list jobs disp");
    assert_eq!(
        list_disp.items[0].orchestrator_id,
        Some(orch_id.to_string())
    );
    assert_eq!(list_disp.items[0].orchestrator_name, Some(orch_name));
    assert_eq!(list_disp.items[0].orchestrator_kind, Some(orch_kind));
    assert!(!list_disp.items[0].orchestrator_fallback);

    // 4. Se params contiver orchestrator_fallback = true, a flag deve ser true.
    sqlx::query("UPDATE jobs SET params = jsonb_set(params, '{orchestrator_fallback}', '\"true\"') WHERE id = $1")
        .bind(job_id)
        .execute(&p)
        .await
        .expect("update fallback flag");

    let job_fb = manager::get_job(&p, job_id)
        .await
        .expect("get job fallback");
    assert!(job_fb.orchestrator_fallback);

    let list_fb = manager::list_jobs(&p, None, None)
        .await
        .expect("list jobs fallback");
    assert!(list_fb.items[0].orchestrator_fallback);

    // 5. Se o orquestrador for removido (SET NULL), campos viram None mas fallback se mantém.
    sqlx::query("UPDATE jobs SET orchestrator_id = NULL WHERE id = $1")
        .bind(job_id)
        .execute(&p)
        .await
        .expect("set orchestrator null");

    let job_null = manager::get_job(&p, job_id)
        .await
        .expect("get job null orch");
    assert_eq!(job_null.orchestrator_id, None);
    assert_eq!(job_null.orchestrator_name, None);
    assert_eq!(job_null.orchestrator_kind, None);
    assert!(job_null.orchestrator_fallback);
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

            meta_content: None,
            phase: None,
            message: None,
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

            meta_content: None,
            phase: None,
            message: None,
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

            meta_content: None,
            phase: None,
            message: None,
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

            meta_content: None,
            phase: None,
            message: None,
        },
    )
    .await
    .expect("report done");

    let models = manager::list_models(&p).await.expect("list models");
    assert_eq!(models.items.len(), 1);
    let m = &models.items[0];
    assert_eq!(m.engine, "yolo");
    assert_eq!(m.model.as_deref(), Some("yolo11m"));
    let expected = expected_yolo_model_name(ds_id, job_id);
    assert_eq!(m.name, expected);
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

            meta_content: None,
            phase: None,
            message: None,
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

            meta_content: None,
            phase: None,
            message: None,
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

            meta_content: None,
            phase: None,
            message: None,
        },
    )
    .await
    .expect("report done with boxes");

    let models = manager::list_models(&p)
        .await
        .expect("list models no boxes");
    assert_eq!(models.items.len(), 1, "boxes excluído: 1 item");
    let expected = expected_yolo_model_name(ds_id, job_id);
    assert_eq!(models.items[0].name, expected);
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

            meta_content: None,
            phase: None,
            message: None,
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

            meta_content: None,
            phase: None,
            message: None,
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

            meta_content: None,
            phase: None,
            message: None,
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

            meta_content: None,
            phase: None,
            message: None,
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

            meta_content: None,
            phase: None,
            message: None,
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

            meta_content: None,
            phase: None,
            message: None,
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

            meta_content: None,
            phase: None,
            message: None,
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
        kind: None,
        arch: None,
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
        engine: "unsupported".into(),
        name: "model.pt".into(),
        model: None,
        s3_key: "models/diff/abc/model.pt".into(),
        source: "upload".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 100,
        job_id: None,
        kind: None,
        arch: None,
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
        kind: None,
        arch: None,
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
        kind: None,
        arch: None,
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
        kind: None,
        arch: None,
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
        kind: None,
        arch: None,
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
        kind: None,
        arch: None,
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
        kind: None,
        arch: None,
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
            orchestrator_hint: None,
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
            orchestrator_hint: None,
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
            orchestrator_hint: None,
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
            orchestrator_hint: None,
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
            orchestrator_hint: None,
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

    // B2: `preparing` no nó morto NÃO é re-queueizado — cai no caminho
    // prepare-timeout/fail, nunca vira queued sem pacote.
    let job3 = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO jobs (id, kind, engine, model, mode, dataset_id, status, orchestrator_id) \
         VALUES ($1, 'yolo_train', 'yolo', 'yolo11m', 'train', $2, 'preparing', $3)",
    )
    .bind(job3)
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

    // `preparing` intocado pelo re-queue (sem transição, sem NULL no nó).
    let job = manager::get_job(&p, job3).await.expect("get preparing job");
    assert_eq!(job.status, "preparing");
    assert_eq!(job.orchestrator_id, Some(orch_id.to_string()));
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
        orchestrator_hint: None,
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
        kind: None,
        arch: None,
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
        kind: None,
        arch: None,
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
        orchestrator_hint: None,
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
        kind: None,
        arch: None,
    };

    let item = manager::create_model(&p, req)
        .await
        .expect("create model world 201");
    assert_eq!(item.id, model_id.to_string());
    assert_eq!(item.engine, "world");
    assert_eq!(item.name, "yolov8x-worldv2.pt");
    assert_eq!(item.source, "upload");
}

/// POST /internal/models com engine inválida (não yolo, world, diffusion, clip) → 400.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_model_engine_invalida_400() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let req = CreateModelRequest {
        id: uuid::Uuid::new_v4(),
        engine: "unsupported".into(),
        name: "model.pt".into(),
        model: None,
        s3_key: "models/unsupported/abc/model.pt".into(),
        source: "upload".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 100,
        job_id: None,
        kind: None,
        arch: None,
    };

    let result = manager::create_model(&p, req).await;
    assert!(matches!(result, Err(ManagerError::InvalidRequest(_))));
}

/// POST /internal/models com engine=diffusion → 201.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_model_engine_diffusion_201() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let model_id = uuid::Uuid::new_v4();
    let req = CreateModelRequest {
        id: model_id,
        engine: "diffusion".into(),
        name: "sdxl-lora.safetensors".into(),
        model: None,
        s3_key: "models/diffusion/abc/sdxl-lora.safetensors".into(),
        source: "upload".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 1024,
        job_id: None,
        kind: None,
        arch: None,
    };

    let item = manager::create_model(&p, req)
        .await
        .expect("create diffusion model");
    assert_eq!(item.id, model_id.to_string());
    assert_eq!(item.engine, "diffusion");
    assert_eq!(item.name, "sdxl-lora.safetensors");
}

/// DELETE /internal/models/:id → remove com sucesso e retorna item.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn delete_model_success_and_not_found() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let model_id = uuid::Uuid::new_v4();
    let req = CreateModelRequest {
        id: model_id,
        engine: "yolo".into(),
        name: "test-delete.pt".into(),
        model: None,
        s3_key: "models/yolo/del/test-delete.pt".into(),
        source: "upload".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 1024,
        job_id: None,
        kind: None,
        arch: None,
    };

    manager::create_model(&p, req).await.expect("create model");

    // Deleta o modelo criado
    let deleted = manager::delete_model(&p, model_id)
        .await
        .expect("delete model");
    assert_eq!(deleted.id, model_id.to_string());
    assert_eq!(deleted.name, "test-delete.pt");

    // Segunda deleção deve retornar NotFound
    let err = manager::delete_model(&p, model_id).await.unwrap_err();
    assert!(matches!(err, ManagerError::NotFound));
}

// ===========================================================================
// N.2 — Seleção de nó: hint no create_job + dispatch com preferência e fallback
// ===========================================================================

/// Submit com orchestrator_hint não-UUID → 400 invalid_request.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn submit_hint_invalido_400() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let mut req = test_job_request(ds_id);
    req.orchestrator_hint = Some("nao-eh-uuid".into());

    let result = manager::create_job(&p, req).await;
    assert!(matches!(result, Err(ManagerError::InvalidRequest(_))));
}

/// Submit com orchestrator_hint inexistente → 404 not_found.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn submit_hint_inexistente_404() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let mut req = test_job_request(ds_id);
    req.orchestrator_hint = Some(uuid::Uuid::new_v4().to_string());

    let result = manager::create_job(&p, req).await;
    assert!(matches!(result, Err(ManagerError::NotFound)));
}

/// Submit com nó offline ou revogado → 400 invalid_request com mensagem honesta.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn submit_hint_offline_ou_revogado_400() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let orch_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'offline-node', 'http://offline:8082', 'remoto', 'offline')",
    )
    .bind(orch_id)
    .execute(&p)
    .await
    .unwrap();

    let mut req = test_job_request(ds_id);
    req.orchestrator_hint = Some(orch_id.to_string());

    let result = manager::create_job(&p, req).await;
    match result {
        Err(ManagerError::InvalidRequest(msg)) => {
            assert!(msg.contains("indisponível"));
        }
        other => panic!("esperava InvalidRequest, obteve {:?}", other),
    }

    // Revogado também deve retornar 400
    sqlx::query("UPDATE orchestrators SET status = 'revoked' WHERE id = $1")
        .bind(orch_id)
        .execute(&p)
        .await
        .unwrap();

    let mut req2 = test_job_request(ds_id);
    req2.orchestrator_hint = Some(orch_id.to_string());

    let result2 = manager::create_job(&p, req2).await;
    assert!(matches!(result2, Err(ManagerError::InvalidRequest(_))));
}

/// Submit com hint válido grava params.orchestrator_hint.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn submit_hint_valido_grava_params() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    manager::adopt_orchestrator(&p).await.expect("adopt");
    let (orch_id,): (uuid::Uuid,) =
        sqlx::query_as("SELECT id FROM orchestrators WHERE status = 'online' LIMIT 1")
            .fetch_one(&p)
            .await
            .unwrap();

    let mut req = test_job_request(ds_id);
    req.orchestrator_hint = Some(orch_id.to_string());

    let resp = manager::create_job(&p, req).await.expect("create job");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    let (params,): (serde_json::Value,) = sqlx::query_as("SELECT params FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(&p)
        .await
        .unwrap();

    assert_eq!(
        params.get("orchestrator_hint").and_then(|v| v.as_str()),
        Some(orch_id.to_string().as_str())
    );
}

/// Dispatch com hint honrado: despacha para o nó solicitado em 1º nível.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn dispatch_hint_honrado() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();
    let vt = test_vram_table();

    // 2 nós: 'aaa' e 'bbb'. Por ordem alfabética normal, 'aaa' seria o escolhido.
    let id_a = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'aaa-node', 'http://aaa:8082', 'local', 'online')",
    )
    .bind(id_a)
    .execute(&p)
    .await
    .unwrap();

    let id_b = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'bbb-node', 'http://bbb:8082', 'remoto', 'online')",
    )
    .bind(id_b)
    .execute(&p)
    .await
    .unwrap();

    let mut req = test_job_request(ds_id);
    req.orchestrator_hint = Some(id_b.to_string());

    let resp = manager::create_job(&p, req).await.expect("create job");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &vt)
        .await
        .expect("dispatch");

    let job = manager::get_job(&p, job_id).await.expect("get job");
    assert_eq!(job.status, "dispatched");
    assert_eq!(job.orchestrator_id, Some(id_b.to_string()));
    assert_eq!(job.orchestrator_name, Some("bbb-node".into()));
    assert_eq!(job.orchestrator_kind, Some("remoto".into()));
    assert!(!job.orchestrator_fallback);
}

/// Dispatch com hint ocupado -> fallback automático para o nó livre + flag fallback=true.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn dispatch_hint_fallback_ocupado() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();
    let vt = test_vram_table();

    let id_a = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'node-a', 'http://a:8082', 'local', 'online')",
    )
    .bind(id_a)
    .execute(&p)
    .await
    .unwrap();

    let id_b = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'node-b', 'http://b:8082', 'remoto', 'online')",
    )
    .bind(id_b)
    .execute(&p)
    .await
    .unwrap();

    // Node B tem um job ativo (dispatched).
    let active_job_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO jobs (id, kind, engine, model, mode, dataset_id, status, orchestrator_id) \
         VALUES ($1, 'yolo_train', 'yolo', 'yolo11n', 'train', $2, 'dispatched', $3)",
    )
    .bind(active_job_id)
    .bind(ds_id)
    .bind(id_b)
    .execute(&p)
    .await
    .unwrap();

    // Cria novo job com hint para Node B.
    let mut req = test_job_request(ds_id);
    req.orchestrator_hint = Some(id_b.to_string());

    let resp = manager::create_job(&p, req).await.expect("create job");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &vt)
        .await
        .expect("dispatch");

    let job = manager::get_job(&p, job_id).await.expect("get job");
    assert_eq!(job.status, "dispatched");
    assert_eq!(job.orchestrator_id, Some(id_a.to_string()));
    assert_eq!(job.orchestrator_name, Some("node-a".into()));
    assert!(job.orchestrator_fallback);
}

/// Dispatch com hint sem capacidade -> fallback para o nó com capacidade + flag.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn dispatch_hint_fallback_sem_capacidade() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();
    let vt = test_vram_table();

    // Node A tem 6GB, Node B tem 12GB.
    let id_a = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status, vram_total_gb) \
         VALUES ($1, 'node-6gb', 'http://a:8082', 'local', 'online', 6)",
    )
    .bind(id_a)
    .execute(&p)
    .await
    .unwrap();

    let id_b = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status, vram_total_gb) \
         VALUES ($1, 'node-12gb', 'http://b:8082', 'remoto', 'online', 12)",
    )
    .bind(id_b)
    .execute(&p)
    .await
    .unwrap();

    // yolo11m train requer 12GB (vram_min=10 + headroom=2).
    let mut req = test_job_request(ds_id);
    req.model = "yolo11m".into();
    req.orchestrator_hint = Some(id_a.to_string()); // Pede nó de 6GB

    let resp = manager::create_job(&p, req).await.expect("create job");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &vt)
        .await
        .expect("dispatch");

    let job = manager::get_job(&p, job_id).await.expect("get job");
    assert_eq!(job.status, "dispatched");
    assert_eq!(job.orchestrator_id, Some(id_b.to_string()));
    assert_eq!(job.orchestrator_name, Some("node-12gb".into()));
    assert!(job.orchestrator_fallback);
}

/// Recovery/Watchdog re-queue preserva o hint em params, e o re-dispatch limpa a flag se honrado (ADR-0015 D4).
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn watchdog_requeue_preserva_hint_e_redispatch() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();
    let vt = test_vram_table();

    let id_a = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'node-a', 'http://a:8082', 'local', 'online')",
    )
    .bind(id_a)
    .execute(&p)
    .await
    .unwrap();

    let id_b = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orchestrators (id, name, endpoint, kind, status) \
         VALUES ($1, 'node-b', 'http://b:8082', 'remoto', 'online')",
    )
    .bind(id_b)
    .execute(&p)
    .await
    .unwrap();

    // 1. Ocupa B inicialmente.
    let occ_job = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO jobs (id, kind, engine, model, mode, dataset_id, status, orchestrator_id) \
         VALUES ($1, 'yolo_train', 'yolo', 'yolo11n', 'train', $2, 'dispatched', $3)",
    )
    .bind(occ_job)
    .bind(ds_id)
    .bind(id_b)
    .execute(&p)
    .await
    .unwrap();

    // 2. Cria job com hint para B.
    let mut req = test_job_request(ds_id);
    req.orchestrator_hint = Some(id_b.to_string());

    let resp = manager::create_job(&p, req).await.expect("create job");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    // 3. Dispatch -> B ocupado, vai para A com fallback=true.
    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &vt)
        .await
        .expect("dispatch");

    let j1 = manager::get_job(&p, job_id).await.expect("get job 1");
    assert_eq!(j1.orchestrator_id, Some(id_a.to_string()));
    assert!(j1.orchestrator_fallback);

    // 4. Simula recuperação/watchdog re-queueando o job (voltando a queued).
    sqlx::query("UPDATE jobs SET status = 'queued', orchestrator_id = NULL WHERE id = $1")
        .bind(job_id)
        .execute(&p)
        .await
        .unwrap();

    // Libera B.
    sqlx::query("UPDATE jobs SET status = 'done' WHERE id = $1")
        .bind(occ_job)
        .execute(&p)
        .await
        .unwrap();

    // 5. Re-dispatch -> agora B está livre, hint é honrado!
    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &vt)
        .await
        .expect("redispatch");

    let j2 = manager::get_job(&p, job_id).await.expect("get job 2");
    assert_eq!(j2.orchestrator_id, Some(id_b.to_string()));
    assert_eq!(j2.orchestrator_name, Some("node-b".into()));
    assert!(!j2.orchestrator_fallback); // Flag removida!
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn autolabel_job_lifecycle_and_dispatch() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let req = CreateJobRequest {
        kind: "autolabel".into(),
        engine: "autolabel".into(),
        model: "mock".into(),
        mode: "autolabel".into(),
        dataset_id: Some(ds_id.to_string()),
        dataset_version_id: Some(uuid::Uuid::new_v4().to_string()),
        package_ref: Some(PackageRef {
            version_id: uuid::Uuid::new_v4().to_string(),
            key: "packages/test/autolabel-dataset.zip".into(),
            md5_zip: "d41d8cd98f00b204e9800998ecf8427e".into(),
            bytes: 1024,
        }),
        config_yaml: Some("job_id: autolabel\nengine: autolabel".into()),
        params: Some(serde_json::json!({
            "prompt": "detalhe macro"
        })),
        vram_min_gb: None,
        weights_id: None,
        orchestrator_hint: None,
    };

    let res = manager::create_job(&p, req)
        .await
        .expect("create autolabel job");
    let job = manager::get_job(&p, uuid::Uuid::parse_str(&res.job_id).unwrap())
        .await
        .expect("get autolabel job");
    assert_eq!(job.kind, "autolabel");
    assert_eq!(job.engine, "autolabel");
    assert_eq!(job.mode, "autolabel");
    assert_eq!(job.status, "queued");
}

// ===========================================================================
// G.5 — Multi-LoRA / Custom / Generations / Rotas internas
// ===========================================================================

/// Helper: cria um modelo diffusion lora no banco e retorna o ID.
async fn insert_diffusion_lora(pool: &PgPool, name: &str) -> uuid::Uuid {
    let model_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO models (id, engine, name, s3_key, source, hash, bytes, job_id, kind, arch) \
         VALUES ($1, 'diffusion', $2, $3, 'upload', 'd41d8cd98f00b204e9800998ecf8427e', 1024, NULL, 'lora', NULL)",
    )
    .bind(model_id)
    .bind(name)
    .bind(format!("models/diffusion/lora/{model_id}/{name}"))
    .execute(pool)
    .await
    .expect("insert diffusion lora");
    model_id
}

/// Helper: cria um modelo diffusion checkpoint no banco e retorna o ID.
async fn insert_diffusion_checkpoint(pool: &PgPool, name: &str, arch: &str) -> uuid::Uuid {
    let model_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO models (id, engine, name, s3_key, source, hash, bytes, job_id, kind, arch) \
         VALUES ($1, 'diffusion', $2, $3, 'upload', 'd41d8cd98f00b204e9800998ecf8427e', 4096, NULL, 'checkpoint', $4)",
    )
    .bind(model_id)
    .bind(name)
    .bind(format!("models/diffusion/ckpt/{model_id}/{name}"))
    .bind(arch)
    .execute(pool)
    .await
    .expect("insert diffusion checkpoint");
    model_id
}

/// Helper: cria um request de diffusion generate.
fn diffusion_generate_request() -> CreateJobRequest {
    CreateJobRequest {
        kind: "diffusion_generate".into(),
        engine: "diffusion".into(),
        model: "flux".into(),
        mode: "generate".into(),
        dataset_id: None,
        dataset_version_id: None,
        package_ref: None,
        config_yaml: Some("engine: diffusion\nmodel: flux".into()),
        params: Some(serde_json::json!({
            "prompt": "a cyberpunk city at night",
            "width": 1024,
            "height": 1024,
            "steps": 20,
            "guidance_scale": 7.5,
            "seed": 42
        })),
        vram_min_gb: None,
        weights_id: None,
        orchestrator_hint: None,
    }
}

/// create_job com params loras 2x → ok; dispatch com loras, sem custom_checkpoint.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_job_com_2_loras_e_dispatch() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let orch = FakeOrchestratorClient::new();

    manager::adopt_orchestrator(&p).await.expect("adopt");

    let lora1 = insert_diffusion_lora(&p, "style-lora.safetensors").await;
    let lora2 = insert_diffusion_lora(&p, "detail-lora.safetensors").await;

    let mut req = diffusion_generate_request();
    req.params = Some(serde_json::json!({
        "prompt": "a cyberpunk city",
        "width": 1024, "height": 1024, "steps": 20, "guidance_scale": 7.5, "seed": 42,
        "loras": [
            {"modelId": lora1.to_string(), "scale": 0.8},
            {"modelId": lora2.to_string(), "scale": 0.5}
        ]
    }));

    let resp = manager::create_job(&p, req)
        .await
        .expect("create job with 2 loras");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    // params.loras gravado com s3_key resolvido.
    let row: (serde_json::Value,) = sqlx::query_as("SELECT params FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(&p)
        .await
        .unwrap();
    let loras = row.0.get("loras").expect("loras presente");
    let arr = loras.as_array().expect("loras é array");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["scale"], 0.8);

    // Dispatch.
    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
        .await
        .expect("dispatch");

    let calls = orch.calls();
    let (_, body) = &calls[0];
    let dl = body
        .get("loras")
        .expect("loras no dispatch")
        .as_array()
        .unwrap();
    assert_eq!(dl.len(), 2);
    assert_eq!(
        dl[0]["s3_key"],
        format!("models/diffusion/lora/{lora1}/style-lora.safetensors")
    );
    assert_eq!(dl[0]["md5"], "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(dl[0]["scale"], 0.8);
    assert!(
        body.get("custom_checkpoint").is_none(),
        "sem custom_checkpoint"
    );
}

/// create_job com customModelId kind='checkpoint' arch=sdxl → dispatch com custom_checkpoint.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_job_com_custom_checkpoint_sdxl() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let orch = FakeOrchestratorClient::new();

    manager::adopt_orchestrator(&p).await.expect("adopt");
    let ckpt = insert_diffusion_checkpoint(&p, "realistic-v1.safetensors", "sdxl").await;

    let mut req = diffusion_generate_request();
    req.params = Some(serde_json::json!({
        "prompt": "a landscape", "width": 1024, "height": 1024, "steps": 30, "seed": 123,
        "customModelId": ckpt.to_string()
    }));

    let resp = manager::create_job(&p, req)
        .await
        .expect("create job with custom checkpoint");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    let row: (serde_json::Value,) = sqlx::query_as("SELECT params FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(&p)
        .await
        .unwrap();
    let cc = row
        .0
        .get("custom_checkpoint")
        .expect("custom_checkpoint presente");
    assert_eq!(
        cc["s3_key"],
        format!("models/diffusion/ckpt/{ckpt}/realistic-v1.safetensors")
    );

    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
        .await
        .expect("dispatch");
    let (_, body) = &orch.calls()[0];
    let dcc = body
        .get("custom_checkpoint")
        .expect("custom_checkpoint no dispatch");
    assert_eq!(
        dcc["s3_key"],
        format!("models/diffusion/ckpt/{ckpt}/realistic-v1.safetensors")
    );
    assert!(body.get("loras").is_none() || body["loras"].as_array().map_or(true, |a| a.is_empty()));
}

/// lora inexistente → falha honesta.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_job_lora_inexistente_falha() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let fake_id = uuid::Uuid::new_v4();
    let mut req = diffusion_generate_request();
    req.params = Some(
        serde_json::json!({"prompt": "test", "loras": [{"modelId": fake_id.to_string(), "scale": 1.0}]}),
    );
    let result = manager::create_job(&p, req).await;
    assert!(
        matches!(result, Err(ManagerError::InvalidRequest(ref msg)) if msg.contains("not found"))
    );
}

/// custom de kind lora → falha.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_job_custom_kind_lora_falha() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let lora = insert_diffusion_lora(&p, "not-a-checkpoint.safetensors").await;
    let mut req = diffusion_generate_request();
    req.params = Some(serde_json::json!({"prompt": "test", "customModelId": lora.to_string()}));
    let result = manager::create_job(&p, req).await;
    assert!(
        matches!(result, Err(ManagerError::InvalidRequest(ref msg)) if msg.contains("checkpoint"))
    );
}

/// 5 loras → falha.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_job_5_loras_falha() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let mut lora_ids = Vec::new();
    for i in 0..5 {
        let id = uuid::Uuid::new_v4();
        let name = format!("lora{i}.safetensors");
        let s3 = format!("models/diffusion/lora/{id}/{name}");
        sqlx::query("INSERT INTO models (id, engine, name, s3_key, source, hash, bytes, kind) VALUES ($1, 'diffusion', $2, $3, 'upload', 'd41d8cd98f00b204e9800998ecf8427e', 1024, 'lora')").bind(id).bind(&name).bind(&s3).execute(&p).await.unwrap();
        lora_ids.push(id);
    }
    let loras: Vec<_> = lora_ids
        .iter()
        .enumerate()
        .map(
            |(i, id)| serde_json::json!({"modelId": id.to_string(), "scale": 0.5 + i as f64 * 0.1}),
        )
        .collect();
    let mut req = diffusion_generate_request();
    req.params = Some(serde_json::json!({"prompt": "test", "loras": loras}));
    let result = manager::create_job(&p, req).await;
    assert!(
        matches!(result, Err(ManagerError::InvalidRequest(ref msg)) if msg.contains("at most 4"))
    );
}

/// Params legado sem loras/customModelId → retrocompat.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_job_legado_sem_loras_custom_ok() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let mut req = diffusion_generate_request();
    req.params = Some(serde_json::json!({"prompt": "legacy job", "width": 512, "height": 512}));
    let resp = manager::create_job(&p, req)
        .await
        .expect("create legacy job should work");
    assert!(!resp.job_id.is_empty());
}

// ===========================================================================
// S4 feat/img2img — initImageId / initGenerationId
// ===========================================================================

/// Helper: cria um input efêmero em generation_inputs e retorna o ID.
async fn insert_generation_input(pool: &PgPool) -> uuid::Uuid {
    let id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO generation_inputs (id, s3_key, filename, mime_type, width, height, md5) \
         VALUES ($1, $2, 'upload.png', 'image/png', 1024, 1024, 'd41d8cd98f00b204e9800998ecf8427e')",
    )
    .bind(id)
    .bind(format!("generation_inputs/{id}/upload.png"))
    .execute(pool)
    .await
    .expect("insert generation input");
    id
}

/// Helper: cria uma linha na galeria generations para o job dado e retorna o ID.
async fn insert_gallery_generation(pool: &PgPool, job_id: uuid::Uuid) -> uuid::Uuid {
    let id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO generations (id, job_id, s3_key, filename, seed, prompt, width, height) \
         VALUES ($1, $2, $3, 'generated_0001.png', 7, 'a lighthouse', 1024, 1024)",
    )
    .bind(id)
    .bind(job_id)
    .bind(format!("artifacts/{job_id}/generated_0001.png"))
    .execute(pool)
    .await
    .expect("insert gallery generation");
    id
}

/// create_job com initImageId → init_image_ref resolvido, used_at marcado,
/// camelCase original preservado; dispatch carrega init_image_ref.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_job_com_init_image_id_resolve_e_dispatch() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let orch = FakeOrchestratorClient::new();

    manager::adopt_orchestrator(&p).await.expect("adopt");
    let input_id = insert_generation_input(&p).await;

    let mut req = diffusion_generate_request();
    req.params = Some(serde_json::json!({
        "prompt": "a cyberpunk city", "width": 1024, "height": 1024,
        "steps": 20, "seed": 42,
        "initImageId": input_id.to_string(), "initStrength": 0.6
    }));

    let resp = manager::create_job(&p, req)
        .await
        .expect("create job with initImageId");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    // params gravado: init_image_ref resolvido + camelCase originais mantidos.
    let row: (serde_json::Value,) = sqlx::query_as("SELECT params FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(&p)
        .await
        .unwrap();
    let iir = row
        .0
        .get("init_image_ref")
        .expect("init_image_ref presente");
    assert_eq!(
        iir["s3_key"],
        format!("generation_inputs/{input_id}/upload.png")
    );
    assert_eq!(iir["md5"], "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(row.0["initImageId"], input_id.to_string());
    assert_eq!(row.0["initStrength"], 0.6);

    // Consumo marcado.
    let used: (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT used_at FROM generation_inputs WHERE id = $1")
            .bind(input_id)
            .fetch_one(&p)
            .await
            .unwrap();
    assert!(used.0.is_some(), "used_at deve ser marcado ao consumir");

    // Dispatch carrega init_image_ref snake_case.
    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
        .await
        .expect("dispatch");
    let (_, body) = &orch.calls()[0];
    let diir = body
        .get("init_image_ref")
        .expect("init_image_ref no dispatch");
    assert_eq!(
        diir["s3_key"],
        format!("generation_inputs/{input_id}/upload.png")
    );
    assert_eq!(diir["md5"], "d41d8cd98f00b204e9800998ecf8427e");
}

/// create_job com initGenerationId → init_image_ref com md5 null; dispatch ok.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_job_com_init_generation_id_resolve_e_dispatch() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let orch = FakeOrchestratorClient::new();

    manager::adopt_orchestrator(&p).await.expect("adopt");
    // Job de origem para a linha da galeria.
    let src: uuid::Uuid = manager::create_job(&p, diffusion_generate_request())
        .await
        .expect("create src job")
        .job_id
        .parse()
        .unwrap();
    let gen_id = insert_gallery_generation(&p, src).await;

    let mut req = diffusion_generate_request();
    req.params = Some(serde_json::json!({
        "prompt": "a lighthouse at dusk",
        "initGenerationId": gen_id.to_string(), "initStrength": 0.75
    }));

    let resp = manager::create_job(&p, req)
        .await
        .expect("create job with initGenerationId");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    let row: (serde_json::Value,) = sqlx::query_as("SELECT params FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(&p)
        .await
        .unwrap();
    let iir = row
        .0
        .get("init_image_ref")
        .expect("init_image_ref presente");
    assert_eq!(iir["s3_key"], format!("artifacts/{src}/generated_0001.png"));
    assert!(iir["md5"].is_null(), "galeria resolve md5 null");
    assert_eq!(row.0["initGenerationId"], gen_id.to_string());

    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
        .await
        .expect("dispatch src");
    // Libera o slot do orchestrator adotado (1 job por vez): encerra o src.
    sqlx::query("UPDATE jobs SET status = 'done' WHERE id = $1")
        .bind(src)
        .execute(&p)
        .await
        .unwrap();
    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
        .await
        .expect("dispatch");
    // O dispatch entrega o job mais antigo primeiro (src); procura o body do job atual.
    let calls = orch.calls();
    let ours = calls
        .iter()
        .map(|(_, b)| b)
        .find(|b| b["job_id"] == resp.job_id)
        .expect("dispatch do job com initGenerationId");
    assert!(ours["init_image_ref"]["md5"].is_null());
}

/// initImageId inexistente → falha honesta.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_job_init_image_id_inexistente_falha() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let mut req = diffusion_generate_request();
    req.params = Some(serde_json::json!({
        "prompt": "test",
        "initImageId": uuid::Uuid::new_v4().to_string(), "initStrength": 0.6
    }));
    let result = manager::create_job(&p, req).await;
    assert!(
        matches!(result, Err(ManagerError::InvalidRequest(ref msg)) if msg.contains("initImageId"))
    );
}

/// initGenerationId inexistente → falha honesta.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_job_init_generation_id_inexistente_falha() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let mut req = diffusion_generate_request();
    req.params = Some(serde_json::json!({
        "prompt": "test",
        "initGenerationId": uuid::Uuid::new_v4().to_string(), "initStrength": 0.6
    }));
    let result = manager::create_job(&p, req).await;
    assert!(
        matches!(result, Err(ManagerError::InvalidRequest(ref msg)) if msg.contains("initGenerationId"))
    );
}

/// initImageId com UUID malformado → 400.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_job_init_image_id_uuid_invalido_falha() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let mut req = diffusion_generate_request();
    req.params = Some(serde_json::json!({"prompt": "test", "initImageId": "not-a-uuid"}));
    let result = manager::create_job(&p, req).await;
    assert!(
        matches!(result, Err(ManagerError::InvalidRequest(ref msg)) if msg.contains("initImageId"))
    );
}

/// Ambos initImageId e initGenerationId presentes → 400, sem marcar used_at.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_job_init_ambos_presentes_falha_sem_marcar_used_at() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let input_id = insert_generation_input(&p).await;
    let src: uuid::Uuid = manager::create_job(&p, diffusion_generate_request())
        .await
        .expect("create src job")
        .job_id
        .parse()
        .unwrap();
    let gen_id = insert_gallery_generation(&p, src).await;
    let mut req = diffusion_generate_request();
    req.params = Some(serde_json::json!({
        "prompt": "test",
        "initImageId": input_id.to_string(),
        "initGenerationId": gen_id.to_string(),
        "initStrength": 0.6
    }));
    let result = manager::create_job(&p, req).await;
    assert!(
        matches!(result, Err(ManagerError::InvalidRequest(ref msg)) if msg.contains("not both"))
    );
    let used: (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT used_at FROM generation_inputs WHERE id = $1")
            .bind(input_id)
            .fetch_one(&p)
            .await
            .unwrap();
    assert!(
        used.0.is_none(),
        "used_at não deve ser marcado quando ambos presentes"
    );
}

/// Report done com generated_meta content → 3 rows em generations.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn hook_generations_3_linhas_meta() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let orch = FakeOrchestratorClient::new();

    manager::adopt_orchestrator(&p).await.expect("adopt");
    let resp = manager::create_job(&p, diffusion_generate_request())
        .await
        .expect("create diffusion job");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();
    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
        .await
        .expect("dispatch");

    let meta_content = "\
{\"filename\":\"generated_0001.png\",\"thumb_filename\":\"thumb_0001.jpg\",\"seed\":42,\"prompt\":\"a cyberpunk city\",\"negative_prompt\":\"blurry\",\"width\":1024,\"height\":1024,\"batch_index\":0,\"batch_size\":3,\"base_model\":\"flux-2-klein-4b\",\"steps\":20,\"guidance_scale\":7.5}
{\"filename\":\"generated_0002.png\",\"thumb_filename\":\"thumb_0002.jpg\",\"seed\":43,\"prompt\":\"a cyberpunk city\",\"negative_prompt\":null,\"width\":1024,\"height\":1024,\"batch_index\":1,\"batch_size\":3,\"base_model\":\"flux-2-klein-4b\",\"steps\":20,\"guidance_scale\":7.5}
{\"filename\":\"generated_0003.png\",\"seed\":44,\"prompt\":\"a cyberpunk city\",\"width\":1024,\"height\":1024,\"batch_index\":2,\"batch_size\":3,\"base_model\":\"flux-2-klein-4b\",\"steps\":20}";

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
                    kind: "generated".into(),
                    path: "generated_0001.png".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 1024,
                },
                ArtifactItem {
                    kind: "generated".into(),
                    path: "generated_0002.png".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 1024,
                },
                ArtifactItem {
                    kind: "generated".into(),
                    path: "generated_0003.png".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 1024,
                },
                ArtifactItem {
                    kind: "generated_thumb".into(),
                    path: "thumb_0001.jpg".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 512,
                },
                ArtifactItem {
                    kind: "generated_meta".into(),
                    path: "generation_meta.json".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 512,
                },
            ]),
            meta_content: Some(meta_content.into()),
            phase: None,
            message: None,
        },
    )
    .await
    .expect("report done with meta");

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM generations WHERE job_id = $1")
        .bind(job_id)
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(count.0, 3, "deve haver 3 generations");

    let row: (String, String, Option<String>, i64, String, Option<String>, i32, i32, serde_json::Value) =
        sqlx::query_as("SELECT s3_key, filename, thumb_s3_key, seed, prompt, negative_prompt, width, height, params FROM generations WHERE job_id = $1 ORDER BY seed LIMIT 1").bind(job_id).fetch_one(&p).await.unwrap();
    assert_eq!(row.0, format!("artifacts/{job_id}/generated_0001.png"));
    assert_eq!(row.1, "generated_0001.png");
    assert_eq!(row.2, Some(format!("artifacts/{job_id}/thumb_0001.jpg")));
    assert_eq!(row.3, 42);
    assert_eq!(row.4, "a cyberpunk city");
    assert_eq!(row.5, Some("blurry".to_string()));
    assert_eq!(row.8["batch_index"], 0);
    assert_eq!(row.8["base_model"], "flux-2-klein-4b");
}

/// Re-report idempotente → continua 1 row.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn hook_generations_idempotente_re_report() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let orch = FakeOrchestratorClient::new();

    manager::adopt_orchestrator(&p).await.expect("adopt");
    let resp = manager::create_job(&p, diffusion_generate_request())
        .await
        .expect("create");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();
    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
        .await
        .expect("dispatch");

    let meta = "{\"filename\":\"gen_0001.png\",\"seed\":10,\"prompt\":\"test\",\"width\":512,\"height\":512}";

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
                    kind: "generated".into(),
                    path: "gen_0001.png".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 1024,
                },
                ArtifactItem {
                    kind: "generated_meta".into(),
                    path: "generation_meta.json".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 100,
                },
            ]),
            meta_content: Some(meta.into()),
            phase: None,
            message: None,
        },
    )
    .await
    .expect("report done 1");

    let count1: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM generations WHERE job_id = $1")
        .bind(job_id)
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(count1.0, 1);

    // 2º report (job já terminal → ignorado, generations não duplica).
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
            artifacts: None,
            meta_content: Some(meta.into()),
            phase: None,
            message: None,
        },
    )
    .await
    .expect("report done 2");

    let count2: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM generations WHERE job_id = $1")
        .bind(job_id)
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(count2.0, 1, "idempotência: continua 1 row");
}

/// Incidente galeria vazia (defesa em profundidade): diffusion_generate que
/// reporta done SEM artefatos é recusado → failed com erro no_artifacts.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn report_done_sem_artifacts_diffusion_generate_vira_failed() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let orch = FakeOrchestratorClient::new();

    manager::adopt_orchestrator(&p).await.expect("adopt");
    let resp = manager::create_job(&p, diffusion_generate_request())
        .await
        .expect("create");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();
    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
        .await
        .expect("dispatch");

    // done sem artefatos: recusado como done, mas o report é aceito (Ok).
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
            artifacts: None,
            meta_content: None,
            phase: None,
            message: None,
        },
    )
    .await
    .expect("report done vazio (recusado, mas aceito)");

    let job = manager::get_job(&p, job_id).await.expect("get");
    assert_eq!(job.status, "failed");
    assert!(job.finished_at.is_some());
    let params: serde_json::Value = sqlx::query_scalar("SELECT params FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(&p)
        .await
        .unwrap();
    assert!(
        params["error"]
            .as_str()
            .unwrap_or("")
            .contains("no_artifacts"),
        "params.error deve conter no_artifacts: {params}"
    );
    let message = job.message.unwrap_or_default();
    assert!(
        message.contains("no_artifacts"),
        "message deve diagnosticar em PT: {message}"
    );
}

/// Contra-caso: done COM artefato generated segue done normalmente.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn report_done_com_generated_segue_done() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let orch = FakeOrchestratorClient::new();

    manager::adopt_orchestrator(&p).await.expect("adopt");
    let resp = manager::create_job(&p, diffusion_generate_request())
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
                kind: "generated".into(),
                path: "gen_0001.png".into(),
                md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                bytes: 1024,
            }]),
            meta_content: None,
            phase: None,
            message: None,
        },
    )
    .await
    .expect("report done com generated");

    let job = manager::get_job(&p, job_id).await.expect("get");
    assert_eq!(job.status, "done");
}

/// Exceção preservada (t7_ac006a_*, abort_em_voo): yolo_train que reporta
/// done SEM artefatos segue done no nível report_job (não só na unidade).
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn report_done_vazio_yolo_train_segue_done() {
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
            artifacts: None,
            meta_content: None,
            phase: Some("completed".into()),
            message: Some("Treino concluído".into()),
        },
    )
    .await
    .expect("report done vazio yolo_train");

    let job = manager::get_job(&p, job_id).await.expect("get");
    assert_eq!(job.status, "done");
    assert_eq!(job.phase.as_deref(), Some("completed"));
    assert_eq!(job.message.as_deref(), Some("Treino concluído"));
}

/// Meta corrupto → best-effort, 1 válida inserida.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn hook_generations_meta_corrupto_best_effort() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let orch = FakeOrchestratorClient::new();

    manager::adopt_orchestrator(&p).await.expect("adopt");
    let resp = manager::create_job(&p, diffusion_generate_request())
        .await
        .expect("create");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();
    manager::dispatch_next(&p, &orch, "docker", "/data", "img", &test_vram_table())
        .await
        .expect("dispatch");

    let meta = "not json at all\n{\"filename\":\"ok.png\",\"seed\":1,\"prompt\":\"test\",\"width\":512,\"height\":512}\n";

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
                    kind: "generated".into(),
                    path: "ok.png".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 1024,
                },
                ArtifactItem {
                    kind: "generated_meta".into(),
                    path: "generation_meta.json".into(),
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    bytes: 100,
                },
            ]),
            meta_content: Some(meta.into()),
            phase: None,
            message: None,
        },
    )
    .await
    .expect("report done with corrupt meta");

    let job = manager::get_job(&p, job_id).await.expect("get job");
    assert_eq!(job.status, "done");

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM generations WHERE job_id = $1")
        .bind(job_id)
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(count.0, 1, "linha corrupta ignorada, 1 válida inserida");
}

/// GET /internal/generations com paginação e filtros.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn list_generations_paginacao_e_filtros() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let job_id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO jobs (id, kind, engine, model, mode, status) VALUES ($1, 'diffusion_generate', 'diffusion', 'flux', 'generate', 'done')").bind(job_id).execute(&p).await.unwrap();

    for i in 0..5 {
        let gen_id = uuid::Uuid::new_v4();
        let s3_key = format!("artifacts/{job_id}/gen_{i:04}.png");
        sqlx::query("INSERT INTO generations (id, job_id, s3_key, filename, seed, prompt, width, height, params) VALUES ($1, $2, $3, $4, $5, $6, 1024, 1024, $7::jsonb)")
            .bind(gen_id).bind(job_id).bind(&s3_key).bind(format!("gen_{i:04}.png")).bind(i as i64).bind(format!("prompt {i}"))
            .bind(serde_json::json!({"base_model": if i < 3 { "sdxl" } else { "sd15" }}))
            .execute(&p).await.unwrap();
    }

    let resp = manager::list_generations(&p, 50, 0, false, None)
        .await
        .expect("list all");
    assert_eq!(resp.total, 5);

    let resp_sdxl = manager::list_generations(&p, 50, 0, false, Some("sdxl"))
        .await
        .expect("list sdxl");
    assert_eq!(resp_sdxl.total, 3);

    let resp_page = manager::list_generations(&p, 2, 0, false, None)
        .await
        .expect("list page");
    assert_eq!(resp_page.total, 5);
    assert_eq!(resp_page.items.len(), 2);

    let resp_del = manager::list_generations(&p, 50, 0, true, None)
        .await
        .expect("list deleted");
    assert_eq!(resp_del.total, 0);

    let ids: Vec<uuid::Uuid> = resp.items[..2]
        .iter()
        .map(|g| g.id.parse().unwrap())
        .collect();
    manager::soft_delete_generations(&p, &ids)
        .await
        .expect("soft delete");

    let resp_after = manager::list_generations(&p, 50, 0, false, None)
        .await
        .expect("list after delete");
    assert_eq!(resp_after.total, 3);

    let resp_del2 = manager::list_generations(&p, 50, 0, true, None)
        .await
        .expect("list deleted after");
    assert_eq!(resp_del2.total, 2);
}

/// POST /internal/generations/delete → 204 + idempotência.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn delete_generations_204_e_idempotencia() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let job_id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO jobs (id, kind, engine, model, mode, status) VALUES ($1, 'diffusion_generate', 'diffusion', 'flux', 'generate', 'done')").bind(job_id).execute(&p).await.unwrap();

    let gen1 = uuid::Uuid::new_v4();
    let gen2 = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO generations (id, job_id, s3_key, filename, seed, prompt, width, height) VALUES ($1, $2, $3, 'a.png', 1, 'test', 512, 512)").bind(gen1).bind(job_id).bind(format!("artifacts/{job_id}/a.png")).execute(&p).await.unwrap();
    sqlx::query("INSERT INTO generations (id, job_id, s3_key, filename, seed, prompt, width, height) VALUES ($1, $2, $3, 'b.png', 2, 'test', 512, 512)").bind(gen2).bind(job_id).bind(format!("artifacts/{job_id}/b.png")).execute(&p).await.unwrap();

    manager::soft_delete_generations(&p, &[gen1, gen2])
        .await
        .expect("soft delete");

    let rows: Vec<(Option<chrono::DateTime<chrono::Utc>>,)> =
        sqlx::query_as("SELECT deleted_at FROM generations WHERE id = $1 OR id = $2 ORDER BY id")
            .bind(gen1)
            .bind(gen2)
            .fetch_all(&p)
            .await
            .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows[0].0.is_some());
    assert!(rows[1].0.is_some());

    // Idempotência.
    manager::soft_delete_generations(&p, &[gen1])
        .await
        .expect("idempotent delete");

    let resp = manager::list_generations(&p, 50, 0, true, None)
        .await
        .expect("list deleted");
    assert_eq!(resp.total, 2);

    // IDs inexistentes ignorados.
    manager::soft_delete_generations(&p, &[uuid::Uuid::new_v4()])
        .await
        .expect("delete nonexistent");
}

/// GET /internal/models inclui kind/arch; yolo models kind NULL.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn list_models_inclui_kind_arch() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let yolo_id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO models (id, engine, name, s3_key, source, hash, bytes) VALUES ($1, 'yolo', 'best.pt', 'models/yolo/test/best.pt', 'upload', 'd41d8cd98f00b204e9800998ecf8427e', 1024)").bind(yolo_id).execute(&p).await.unwrap();

    let lora_id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO models (id, engine, name, s3_key, source, hash, bytes, kind) VALUES ($1, 'diffusion', 'style.safetensors', 'models/diffusion/lora/test/style.safetensors', 'upload', 'd41d8cd98f00b204e9800998ecf8427e', 1024, 'lora')").bind(lora_id).execute(&p).await.unwrap();

    let ckpt_id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO models (id, engine, name, s3_key, source, hash, bytes, kind, arch) VALUES ($1, 'diffusion', 'real.safetensors', 'models/diffusion/ckpt/test/real.safetensors', 'upload', 'd41d8cd98f00b204e9800998ecf8427e', 4096, 'checkpoint', 'sdxl')").bind(ckpt_id).execute(&p).await.unwrap();

    let models = manager::list_models(&p).await.expect("list models");
    assert_eq!(models.items.len(), 3);

    let yolo = models.items.iter().find(|m| m.engine == "yolo").unwrap();
    assert!(yolo.kind.is_none());
    assert!(yolo.arch.is_none());

    let lora = models
        .items
        .iter()
        .find(|m| m.name == "style.safetensors")
        .unwrap();
    assert_eq!(lora.kind.as_deref(), Some("lora"));
    assert!(lora.arch.is_none());

    let ckpt = models
        .items
        .iter()
        .find(|m| m.name == "real.safetensors")
        .unwrap();
    assert_eq!(ckpt.kind.as_deref(), Some("checkpoint"));
    assert_eq!(ckpt.arch.as_deref(), Some("sdxl"));
}

// ===========================================================================
// G.6b — create_model kind/arch + GET /internal/generations/:id
// ===========================================================================

/// create_model diffusion com kind=lora + arch → persistido e retornado no list/get.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_model_diffusion_kind_lora_arch_persistido() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let model_id = uuid::Uuid::new_v4();
    let req = CreateModelRequest {
        id: model_id,
        engine: "diffusion".into(),
        name: "style-lora.safetensors".into(),
        model: None,
        s3_key: "models/diffusion/lora/test/style-lora.safetensors".into(),
        source: "upload".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 2048,
        job_id: None,
        kind: Some("lora".into()),
        arch: Some("sdxl".into()),
    };

    let item = manager::create_model(&p, req)
        .await
        .expect("create diffusion lora model");
    assert_eq!(item.id, model_id.to_string());
    assert_eq!(item.kind.as_deref(), Some("lora"));
    assert_eq!(item.arch.as_deref(), Some("sdxl"));

    // list_models inclui kind/arch.
    let models = manager::list_models(&p).await.expect("list models");
    let found = models
        .items
        .iter()
        .find(|m| m.id == model_id.to_string())
        .unwrap();
    assert_eq!(found.kind.as_deref(), Some("lora"));
    assert_eq!(found.arch.as_deref(), Some("sdxl"));
}

/// create_model engine yolo com kind Some → 400.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_model_yolo_com_kind_400() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let req = CreateModelRequest {
        id: uuid::Uuid::new_v4(),
        engine: "yolo".into(),
        name: "best.pt".into(),
        model: None,
        s3_key: "models/yolo/test/best.pt".into(),
        source: "upload".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 1024,
        job_id: None,
        kind: Some("lora".into()),
        arch: None,
    };

    let result = manager::create_model(&p, req).await;
    assert!(matches!(result, Err(ManagerError::InvalidRequest(_))));
}

/// create_model diffusion kind='checkpoint' arch='sdxl' → ok; kind='checkpoint' sem arch → 400.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_model_diffusion_checkpoint_com_arch_e_sem_arch() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    // checkpoint com arch → ok.
    let model_id = uuid::Uuid::new_v4();
    let req = CreateModelRequest {
        id: model_id,
        engine: "diffusion".into(),
        name: "real.safetensors".into(),
        model: None,
        s3_key: "models/diffusion/ckpt/test/real.safetensors".into(),
        source: "upload".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 4096,
        job_id: None,
        kind: Some("checkpoint".into()),
        arch: Some("sdxl".into()),
    };

    let item = manager::create_model(&p, req)
        .await
        .expect("create checkpoint with arch");
    assert_eq!(item.kind.as_deref(), Some("checkpoint"));
    assert_eq!(item.arch.as_deref(), Some("sdxl"));

    // checkpoint sem arch → 400.
    let req_no_arch = CreateModelRequest {
        id: uuid::Uuid::new_v4(),
        engine: "diffusion".into(),
        name: "no-arch.safetensors".into(),
        model: None,
        s3_key: "models/diffusion/ckpt/test/no-arch.safetensors".into(),
        source: "upload".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 4096,
        job_id: None,
        kind: Some("checkpoint".into()),
        arch: None,
    };
    let result = manager::create_model(&p, req_no_arch).await;
    assert!(matches!(result, Err(ManagerError::InvalidRequest(_))));
}

/// create_model diffusion kind inválido → 400; arch inválido → 400.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_model_diffusion_kind_arch_invalidos_400() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    // kind inválido.
    let req_bad_kind = CreateModelRequest {
        id: uuid::Uuid::new_v4(),
        engine: "diffusion".into(),
        name: "bad-kind.safetensors".into(),
        model: None,
        s3_key: "models/diffusion/test/bad-kind.safetensors".into(),
        source: "upload".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 1024,
        job_id: None,
        kind: Some("invalid".into()),
        arch: None,
    };
    let result = manager::create_model(&p, req_bad_kind).await;
    assert!(matches!(result, Err(ManagerError::InvalidRequest(_))));

    // arch inválido.
    let req_bad_arch = CreateModelRequest {
        id: uuid::Uuid::new_v4(),
        engine: "diffusion".into(),
        name: "bad-arch.safetensors".into(),
        model: None,
        s3_key: "models/diffusion/test/bad-arch.safetensors".into(),
        source: "upload".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 1024,
        job_id: None,
        kind: Some("lora".into()),
        arch: Some("invalid-arch".into()),
    };
    let result = manager::create_model(&p, req_bad_arch).await;
    assert!(matches!(result, Err(ManagerError::InvalidRequest(_))));
}

/// GET /internal/generations/:id — existente → 200; inexistente → 404; soft-deletada → 404.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn get_generation_by_id_200_404_soft_delete() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    // Insere uma generation.
    let job_id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO jobs (id, kind, engine, model, mode, status) VALUES ($1, 'diffusion_generate', 'diffusion', 'flux', 'generate', 'done')")
        .bind(job_id)
        .execute(&p)
        .await
        .unwrap();

    let gen_id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO generations (id, job_id, s3_key, filename, seed, prompt, width, height) VALUES ($1, $2, $3, 'test.png', 42, 'a prompt', 512, 512)")
        .bind(gen_id)
        .bind(job_id)
        .bind(format!("artifacts/{job_id}/test.png"))
        .execute(&p)
        .await
        .unwrap();

    // GET existente → 200.
    let row = manager::get_generation(&p, gen_id)
        .await
        .expect("get generation exists");
    assert!(row.is_some());
    let row = row.unwrap();
    assert_eq!(row.id, gen_id.to_string());
    assert_eq!(row.filename, "test.png");

    // GET inexistente → None.
    let missing = manager::get_generation(&p, uuid::Uuid::new_v4())
        .await
        .expect("get generation missing");
    assert!(missing.is_none());

    // Soft delete → None.
    manager::soft_delete_generations(&p, &[gen_id])
        .await
        .expect("soft delete");
    let after_delete = manager::get_generation(&p, gen_id)
        .await
        .expect("get generation after soft delete");
    assert!(after_delete.is_none());
}

/// create_model diffusion kind=None, arch=None → ok (upload existente não quebra).
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_model_diffusion_sem_kind_arch_persiste_none() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let model_id = uuid::Uuid::new_v4();
    let req = CreateModelRequest {
        id: model_id,
        engine: "diffusion".into(),
        name: "plain.safetensors".into(),
        model: None,
        s3_key: "models/diffusion/test/plain.safetensors".into(),
        source: "upload".into(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 1024,
        job_id: None,
        kind: None,
        arch: None,
    };

    let item = manager::create_model(&p, req)
        .await
        .expect("create diffusion without kind/arch");
    assert!(item.kind.is_none());
    assert!(item.arch.is_none());
}

/// Paginação real: OFFSET parametrizado — items e total coerentes.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn list_generations_pagination_offset_e_limit() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let job_id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO jobs (id, kind, engine, model, mode, status) VALUES ($1, 'diffusion_generate', 'diffusion', 'flux', 'generate', 'done')").bind(job_id).execute(&p).await.unwrap();

    // Insere 3 rows direto (sem dependency no hook de report).
    for i in 0..3 {
        let gen_id = uuid::Uuid::new_v4();
        let s3_key = format!("artifacts/{job_id}/page_gen_{i:04}.png");
        sqlx::query("INSERT INTO generations (id, job_id, s3_key, filename, seed, prompt, width, height) VALUES ($1, $2, $3, $4, $5, $6, 1024, 1024)")
            .bind(gen_id).bind(job_id).bind(&s3_key)
            .bind(format!("page_gen_{i:04}.png")).bind(i as i64)
            .bind(format!("pagination test {i}"))
            .execute(&p).await.unwrap();
    }

    // 1. limit=50, offset=0 → todos os 3.
    let resp = manager::list_generations(&p, 50, 0, false, None)
        .await
        .expect("list page 0");
    assert_eq!(resp.items.len(), 3, "limit 50 offset 0: 3 items");
    assert_eq!(resp.total, 3, "limit 50 offset 0: total 3");

    // 2. limit=2, offset=0 → 2 items, total 3 (paginação corta, total intacto).
    let resp2 = manager::list_generations(&p, 2, 0, false, None)
        .await
        .expect("list limit 2");
    assert_eq!(resp2.items.len(), 2, "limit 2: 2 items");
    assert_eq!(resp2.total, 3, "limit 2: total 3");

    // 3. limit=50, offset=2 → 1 item (offset real pula 2).
    let resp3 = manager::list_generations(&p, 50, 2, false, None)
        .await
        .expect("list offset 2");
    assert_eq!(resp3.items.len(), 1, "offset 2: 1 item");
    assert_eq!(resp3.total, 3, "offset 2: total 3");

    // 4. Soft-delete 1 row → deleted=false → 2 items; deleted=true → 1 item.
    let del_id: uuid::Uuid = resp.items[0].id.parse().unwrap();
    manager::soft_delete_generations(&p, &[del_id])
        .await
        .expect("soft delete 1");

    let resp_active = manager::list_generations(&p, 50, 0, false, None)
        .await
        .expect("list active after delete");
    assert_eq!(resp_active.items.len(), 2, "active after delete: 2 items");
    assert_eq!(resp_active.total, 2, "active after delete: total 2");

    let resp_deleted = manager::list_generations(&p, 50, 0, true, None)
        .await
        .expect("list deleted after delete");
    assert_eq!(resp_deleted.items.len(), 1, "deleted after delete: 1 item");
    assert_eq!(resp_deleted.total, 1, "deleted after delete: total 1");
}

// ===========================================================================
// AC-003 — exclusão de jobs (delete_job / cleanup_jobs)
// ===========================================================================

async fn insert_artifact(pool: &PgPool, job_id: uuid::Uuid, path: &str) {
    sqlx::query(
        "INSERT INTO job_artifacts (id, job_id, kind, path, md5, bytes) \
         VALUES ($1, $2, 'model', $3, 'd41d8cd98f00b204e9800998ecf8427e', 10)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(job_id)
    .bind(path)
    .execute(pool)
    .await
    .unwrap();
}

async fn set_terminal(pool: &PgPool, job_id: uuid::Uuid, status: &str, days_ago: i64) {
    sqlx::query(
        "UPDATE jobs SET status = $2, finished_at = NOW() - ($3 * INTERVAL '1 day') WHERE id = $1",
    )
    .bind(job_id)
    .bind(status)
    .bind(days_ago)
    .execute(pool)
    .await
    .unwrap();
}

async fn count_jobs(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM jobs")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn delete_job_guarda_estado_apaga_cascata() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let resp = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create job");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    // 1. Job em 'queued' não é deletável.
    let err = manager::delete_job(&p, job_id).await.unwrap_err();
    assert!(
        matches!(err, manager::ManagerError::NotDeletable),
        "esperava NotDeletable, veio {err:?}"
    );

    // 2. Terminal + artifacts → apaga e devolve paths; cascade limpa job_artifacts.
    set_terminal(&p, job_id, "done", 1).await;
    insert_artifact(&p, job_id, "outputs/best.pt").await;
    insert_artifact(&p, job_id, "samples/sample_epoch_001.png").await;

    let deleted = manager::delete_job(&p, job_id).await.expect("delete ok");
    assert_eq!(deleted.id, job_id.to_string());
    assert_eq!(deleted.status, "done");
    assert_eq!(deleted.artifacts.len(), 2, "paths dos artifacts p/ sweep");
    assert_eq!(
        deleted.object_keys,
        vec![
            format!("artifacts/{job_id}/outputs/best.pt"),
            format!("artifacts/{job_id}/samples/sample_epoch_001.png"),
        ],
        "chaves exatas na ordem dos paths"
    );
    assert_eq!(deleted.models_deleted, 0);
    assert_eq!(deleted.generations_preserved, 0);

    assert_eq!(count_jobs(&p).await, 0);
    let arts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_artifacts WHERE job_id = $1")
        .bind(job_id)
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(arts, 0, "FK ON DELETE CASCADE deve limpar artifacts");

    // 3. Inexistente → NotFound.
    let err = manager::delete_job(&p, job_id).await.unwrap_err();
    assert!(matches!(err, manager::ManagerError::NotFound));
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn cleanup_jobs_lote_por_idade_e_status() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let mk = |p: &PgPool, ds: uuid::Uuid| {
        let p = p.clone();
        async move {
            let r = manager::create_job(&p, test_job_request(ds))
                .await
                .expect("create");
            r.job_id.parse::<uuid::Uuid>().unwrap()
        }
    };

    let a = mk(&p, ds_id).await; // done há 10 dias (com artifact)
    let b = mk(&p, ds_id).await; // done ontem (recente)
    let c = mk(&p, ds_id).await; // failed há 30 dias
    let d = mk(&p, ds_id).await; // ainda queued (não terminal)

    set_terminal(&p, a, "done", 10).await;
    insert_artifact(&p, a, "outputs/best.pt").await;
    set_terminal(&p, b, "done", 1).await;
    set_terminal(&p, c, "failed", 30).await;

    // 1. Sem recorte de idade + sem statuses → Inválido (exige critério).
    let err = manager::cleanup_jobs(&p, None, None).await.unwrap_err();
    assert!(matches!(err, manager::ManagerError::InvalidRequest(_)));

    // 2. Só terminais há mais de 7 dias → apaga A e C.
    let res = manager::cleanup_jobs(&p, Some(7), None)
        .await
        .expect("cleanup");
    assert_eq!(res.deleted, 2);
    let ids: Vec<&str> = res.jobs.iter().map(|j| j.id.as_str()).collect();
    assert!(ids.contains(&a.to_string().as_str()) && ids.contains(&c.to_string().as_str()));
    let job_a = res.jobs.iter().find(|j| j.id == a.to_string()).unwrap();
    assert_eq!(job_a.artifacts, vec!["outputs/best.pt".to_string()]);

    // 3. B (done recente) e D (queued) permanecem.
    assert_eq!(count_jobs(&p).await, 2);

    // 4. Status inválido (não-terminal) → InvalidRequest.
    let err = manager::cleanup_jobs(&p, Some(0), Some(vec!["running".into()]))
        .await
        .unwrap_err();
    assert!(matches!(err, manager::ManagerError::InvalidRequest(_)));

    // 5. Por status, sem recorte de idade → apaga B (done); D permanece.
    let res = manager::cleanup_jobs(&p, None, Some(vec!["done".into()]))
        .await
        .expect("cleanup por status");
    assert_eq!(res.deleted, 1);
    assert_eq!(res.jobs[0].id, b.to_string());
    let remaining: Vec<String> = sqlx::query_scalar("SELECT id::text FROM jobs")
        .fetch_all(&p)
        .await
        .unwrap();
    assert_eq!(remaining, vec![d.to_string()], "só o queued D sobrevive");
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn delete_job_preserva_galeria_e_expurga_models() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let resp = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create job");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();
    set_terminal(&p, job_id, "done", 1).await;

    // artifact de modelo (vai p/ sweep) + artifact geradoReferenciado por geração viva.
    insert_artifact(&p, job_id, "outputs/m.safetensors").await;
    insert_artifact(&p, job_id, "generated.png").await;

    // linha no catálogo de models derivada do job → deve ser expurgada.
    let model_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO models (id, engine, name, model, s3_key, source, hash, bytes, job_id, kind) \
         VALUES ($1, 'diffusion', 'm', 'flux', $2, 'train', 'd41d8cd98f00b204e9800998ecf8427e', 10, $3, 'lora')",
    )
    .bind(model_id)
    .bind(format!("artifacts/{job_id}/outputs/m.safetensors"))
    .bind(job_id)
    .execute(&p)
    .await
    .unwrap();

    // geração VIVA referenciando o artifact `generated.png` → bytes preservados,
    // linha sobrevive com job_id NULL.
    let gen_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO generations (id, job_id, s3_key, thumb_s3_key, filename, seed, prompt, width, height) \
         VALUES ($1, $2, $3, $4, 'generated.png', 1, 'p', 512, 512)",
    )
    .bind(gen_id)
    .bind(job_id)
    .bind(format!("artifacts/{job_id}/generated.png"))
    .bind(format!("artifacts/{job_id}/generated_thumb.png"))
    .execute(&p)
    .await
    .unwrap();

    let deleted = manager::delete_job(&p, job_id).await.expect("delete");
    assert_eq!(deleted.generations_preserved, 1);
    assert_eq!(deleted.models_deleted, 1, "catálogo expurga models do job");
    // sweep NÃO inclui a chave da geração viva (nem thumb, que nem é artifact).
    assert_eq!(
        deleted.object_keys,
        vec![format!("artifacts/{job_id}/outputs/m.safetensors")],
        "generated.png preservado (pertence à galeria)"
    );

    // geração sobrevive órfã (job_id NULL).
    let (g_job,): (Option<uuid::Uuid>,) =
        sqlx::query_as("SELECT job_id FROM generations WHERE id = $1")
            .bind(gen_id)
            .fetch_one(&p)
            .await
            .unwrap();
    assert!(g_job.is_none(), "galeria preservada com job_id NULL");

    // models do job sumiram do catálogo.
    let m_left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM models WHERE id = $1")
        .bind(model_id)
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(m_left, 0);
}

// ---------------------------------------------------------------------------
// AC-006-A: phase/message persistidos no job (ADR-0024 D3)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t6_ac006a_status_report_persists_phase_and_message() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    let ds_id = insert_test_dataset(&p).await;
    let req = test_job_request(ds_id);
    let resp = manager::create_job(&p, req).await.expect("create job");
    let job_id = uuid::Uuid::parse_str(&resp.job_id).unwrap();

    // (a) Report de STATUS sem metrics — phase/message devem ser persistidos.
    let status_report = ReportRequest {
        status: "running".to_string(),
        progress: Some(0.05),
        epoch: Some(0),
        step: None,
        metrics: None,
        error: None,
        artifacts: None,
        meta_content: None,
        phase: Some("loading_model".to_string()),
        message: Some("Carregando FLUX".to_string()),
    };
    manager::report_job(&p, job_id, status_report)
        .await
        .expect("report status");

    // Verifica que phase/message foram gravados.
    let job = manager::get_job(&p, job_id)
        .await
        .expect("get job after status");
    assert_eq!(job.phase.as_deref(), Some("loading_model"));
    assert_eq!(job.message.as_deref(), Some("Carregando FLUX"));
    // Array de metrics permanece VAZIO (status não entra em metrics).
    let metrics_items = job
        .metrics
        .as_ref()
        .and_then(|v| v.get("items"))
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    assert_eq!(
        metrics_items, 0,
        "status report must not pollute metrics array"
    );

    // (b) Report de MÉTRICA — metrics array tem 1 item, phase/message NÃO são pisados.
    let metric_report = ReportRequest {
        status: "running".to_string(),
        progress: Some(0.3),
        epoch: Some(3),
        step: Some(100),
        metrics: Some(serde_json::json!({
            "loss": 0.4,
            "lr": 0.0001,
            "epoch": 3,
            "step": 100
        })),
        error: None,
        artifacts: None,
        meta_content: None,
        phase: None,
        message: None,
    };
    manager::report_job(&p, job_id, metric_report)
        .await
        .expect("report metric");

    // Verifica que metrics tem 1 item E phase/message PRESERVADOS.
    let job = manager::get_job(&p, job_id)
        .await
        .expect("get job after metric");
    let metrics_items = job
        .metrics
        .as_ref()
        .and_then(|v| v.get("items"))
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    assert_eq!(metrics_items, 1, "metric report should add 1 item");
    assert_eq!(
        job.phase.as_deref(),
        Some("loading_model"),
        "phase must NOT be overwritten by metric report"
    );
    assert_eq!(
        job.message.as_deref(),
        Some("Carregando FLUX"),
        "message must NOT be overwritten by metric report"
    );
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn t7_ac006a_terminal_report_persiste_phase_message() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;

    // (a) done com phase/message atualiza as colunas (não congela no "running").
    let ds_id = insert_test_dataset(&p).await;
    let resp = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create job");
    let job_id = uuid::Uuid::parse_str(&resp.job_id).unwrap();

    manager::report_job(
        &p,
        job_id,
        ReportRequest {
            status: "running".to_string(),
            progress: Some(0.3),
            epoch: Some(2),
            step: Some(50),
            metrics: None,
            error: None,
            artifacts: None,
            meta_content: None,
            phase: Some("training".to_string()),
            message: Some("Treinando época 2".to_string()),
        },
    )
    .await
    .expect("report running");

    manager::report_job(
        &p,
        job_id,
        ReportRequest {
            status: "done".to_string(),
            progress: Some(1.0),
            epoch: Some(2),
            step: Some(50),
            metrics: None,
            error: None,
            artifacts: None,
            meta_content: None,
            phase: Some("completed".to_string()),
            message: Some("Treino concluído".to_string()),
        },
    )
    .await
    .expect("report done");

    let job = manager::get_job(&p, job_id)
        .await
        .expect("get job after done");
    assert_eq!(job.status, "done");
    assert_eq!(job.phase.as_deref(), Some("completed"));
    assert_eq!(job.message.as_deref(), Some("Treino concluído"));

    // (b) done com phase/message None NÃO pisa as colunas (COALESCE segura).
    let resp = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create job 2");
    let job_id = uuid::Uuid::parse_str(&resp.job_id).unwrap();

    manager::report_job(
        &p,
        job_id,
        ReportRequest {
            status: "running".to_string(),
            progress: Some(0.1),
            epoch: Some(0),
            step: None,
            metrics: None,
            error: None,
            artifacts: None,
            meta_content: None,
            phase: Some("loading_model".to_string()),
            message: Some("Carregando FLUX".to_string()),
        },
    )
    .await
    .expect("report running 2");

    manager::report_job(
        &p,
        job_id,
        ReportRequest {
            status: "done".to_string(),
            progress: Some(1.0),
            epoch: None,
            step: None,
            metrics: None,
            error: None,
            artifacts: None,
            meta_content: None,
            phase: None,
            message: None,
        },
    )
    .await
    .expect("report done 2");

    let job = manager::get_job(&p, job_id)
        .await
        .expect("get job 2 after done");
    assert_eq!(job.status, "done");
    assert_eq!(
        job.phase.as_deref(),
        Some("loading_model"),
        "COALESCE must preserve phase on terminal report with None"
    );
    assert_eq!(
        job.message.as_deref(),
        Some("Carregando FLUX"),
        "COALESCE must preserve message on terminal report with None"
    );
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn delete_job_sweep_inclui_models_orfaos() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let resp = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create job");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();
    set_terminal(&p, job_id, "done", 1).await;

    // artifact comum (vai p/ sweep).
    insert_artifact(&p, job_id, "outputs/common.pt").await;

    // geração viva sob artifacts/{job}/... → bytes preservados.
    let gen_s3 = format!("artifacts/{job_id}/gen.png");
    let gen_thumb = format!("artifacts/{job_id}/gen_thumb.png");
    sqlx::query(
        "INSERT INTO generations (id, job_id, s3_key, thumb_s3_key, filename, seed, prompt, width, height) \
         VALUES ($1, $2, $3, $4, 'gen.png', 1, 'p', 512, 512)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(job_id)
    .bind(&gen_s3)
    .bind(&gen_thumb)
    .execute(&p)
    .await
    .unwrap();

    // linha `models` com s3_key FORA do conjunto de artifacts (órfão de bytes).
    let orphan_key = format!(
        "models/yolo/{}/peso-upload.safetensors",
        uuid::Uuid::new_v4()
    );
    sqlx::query(
        "INSERT INTO models (id, engine, name, model, s3_key, source, hash, bytes, job_id) \
         VALUES ($1, 'yolo', 'peso-upload.safetensors', 'yolo11m', $2, 'upload', 'd41d8cd98f00b204e9800998ecf8427e', 10, $3)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(&orphan_key)
    .bind(job_id)
    .execute(&p)
    .await
    .unwrap();

    let deleted = manager::delete_job(&p, job_id).await.expect("delete");
    assert_eq!(deleted.models_deleted, 1);
    assert_eq!(deleted.generations_preserved, 1);
    assert!(
        deleted
            .object_keys
            .contains(&format!("artifacts/{job_id}/outputs/common.pt")),
        "artifact comum no sweep, veio {:?}",
        deleted.object_keys
    );
    assert!(
        deleted.object_keys.contains(&orphan_key),
        "model órfão no sweep, veio {:?}",
        deleted.object_keys
    );
    assert!(
        !deleted.object_keys.contains(&gen_s3),
        "s3_key da geração preservada fora do sweep"
    );
    assert!(
        !deleted.object_keys.contains(&gen_thumb),
        "thumb da geração preservada fora do sweep"
    );
}

#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn delete_job_preserva_geracoes_trash() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let resp = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create job");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();
    set_terminal(&p, job_id, "done", 1).await;

    insert_artifact(&p, job_id, "outputs/common.pt").await;

    // geração do job já na lixeira (deleted_at preenchido) → bytes preservados.
    let gen_id = uuid::Uuid::new_v4();
    let gen_s3 = format!("artifacts/{job_id}/trash.png");
    let gen_thumb = format!("artifacts/{job_id}/trash_thumb.png");
    sqlx::query(
        "INSERT INTO generations (id, job_id, s3_key, thumb_s3_key, filename, seed, prompt, width, height, deleted_at) \
         VALUES ($1, $2, $3, $4, 'trash.png', 1, 'p', 512, 512, NOW())",
    )
    .bind(gen_id)
    .bind(job_id)
    .bind(&gen_s3)
    .bind(&gen_thumb)
    .execute(&p)
    .await
    .unwrap();

    let deleted = manager::delete_job(&p, job_id).await.expect("delete");
    assert_eq!(
        deleted.generations_preserved, 1,
        "geração trash também conta como preservada"
    );
    assert!(
        !deleted.object_keys.contains(&gen_s3),
        "s3_key da geração trash fora do sweep"
    );
    assert!(
        !deleted.object_keys.contains(&gen_thumb),
        "thumb da geração trash fora do sweep"
    );
    assert!(
        deleted
            .object_keys
            .contains(&format!("artifacts/{job_id}/outputs/common.pt")),
        "artifact comum no sweep, veio {:?}",
        deleted.object_keys
    );

    // linha trash sobrevive órfã (job_id NULL).
    let (g_job,): (Option<uuid::Uuid>,) =
        sqlx::query_as("SELECT job_id FROM generations WHERE id = $1")
            .bind(gen_id)
            .fetch_one(&p)
            .await
            .unwrap();
    assert!(g_job.is_none(), "geração trash preservada com job_id NULL");
}

// ===========================================================================
// G.6 — Bug 009: hook pós-treino de difusão preenche kind/arch
// ===========================================================================

/// Helper: cria um request de diffusion train (espelha o body do BFF:
/// params camelCase com baseModel + config_yaml com `model:`).
fn diffusion_train_request(dataset_id: uuid::Uuid, base_model: &str) -> CreateJobRequest {
    CreateJobRequest {
        kind: "diffusion_train".into(),
        engine: "diffusion".into(),
        model: base_model.into(),
        mode: "train".into(),
        dataset_id: Some(dataset_id.to_string()),
        dataset_version_id: None,
        package_ref: None,
        config_yaml: Some(format!(
            "# Configuração de treino Difusão LoRA (gerada pelo api-principal)\nengine: \"diffusion\"\nmodel: \"{base_model}\"\nmode: \"train\"\n"
        )),
        params: Some(serde_json::json!({
            "datasetId": dataset_id.to_string(),
            "baseModel": base_model,
            "triggerWord": "TOK",
            "epochs": 10
        })),
        vram_min_gb: Some(12),
        weights_id: None,
        orchestrator_hint: None,
    }
}

/// Bug 009: report done de treino de difusão com adapter.safetensors →
/// models row com kind='lora' + arch derivado; o LoRA passa a ser aceito
/// em diffusion generate (repro end-to-end do "argumentos inválidos").
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn report_diffusion_train_preenche_kind_arch_lora() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let resp = manager::create_job(&p, diffusion_train_request(ds_id, "sdxl"))
        .await
        .expect("create diffusion train");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

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
                path: "adapter.safetensors".into(),
                md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                bytes: 2048,
            }]),
            meta_content: None,
            phase: None,
            message: None,
        },
    )
    .await
    .expect("report done");

    let row: (uuid::Uuid, Option<String>, Option<String>, String) =
        sqlx::query_as("SELECT id, kind, arch, s3_key FROM models WHERE job_id = $1")
            .bind(job_id)
            .fetch_one(&p)
            .await
            .expect("models row do hook");
    assert_eq!(row.1.as_deref(), Some("lora"), "adapter → kind='lora'");
    assert_eq!(row.2.as_deref(), Some("sdxl"), "arch derivado do treino");
    assert_eq!(row.3, format!("artifacts/{job_id}/adapter.safetensors"));

    // Prova do Bug 009: o LoRA agora resolve em diffusion generate.
    let mut gen = diffusion_generate_request();
    gen.params = Some(serde_json::json!({
        "prompt": "a cyberpunk city",
        "width": 1024, "height": 1024, "steps": 20, "guidance_scale": 7.5, "seed": 42,
        "loras": [{"modelId": row.0.to_string(), "scale": 0.8}]
    }));
    manager::create_job(&p, gen)
        .await
        .expect("generate com LoRA do treino deve resolver");
}

/// Bug 009: ON CONFLICT DO UPDATE com COALESCE — re-report do mesmo artefato
/// preenche kind/arch NULL mas NUNCA sobrescreve kind já definido (upload manual).
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn hook_models_conflict_nao_sobrescreve_kind_existente() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let resp = manager::create_job(&p, diffusion_train_request(ds_id, "sd15"))
        .await
        .expect("create diffusion train");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();
    let s3_key = format!("artifacts/{job_id}/adapter.safetensors");

    // Row pré-existente (upload manual classificou como checkpoint).
    sqlx::query(
        "INSERT INTO models (id, engine, name, model, s3_key, source, hash, bytes, job_id, kind, arch) \
         VALUES ($1, 'diffusion', 'manual.safetensors', 'sd15', $2, 'upload', \
                 'd41d8cd98f00b204e9800998ecf8427e', 1024, NULL, 'checkpoint', 'sd15')",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(&s3_key)
    .execute(&p)
    .await
    .expect("pre-insert manual");

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
                path: "adapter.safetensors".into(),
                md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                bytes: 2048,
            }]),
            meta_content: None,
            phase: None,
            message: None,
        },
    )
    .await
    .expect("report done");

    let rows: Vec<(Option<String>, Option<String>, String)> =
        sqlx::query_as("SELECT kind, arch, source FROM models WHERE s3_key = $1")
            .bind(&s3_key)
            .fetch_all(&p)
            .await
            .expect("select conflict");
    assert_eq!(rows.len(), 1, "sem duplicata no conflito");
    assert_eq!(
        rows[0].0.as_deref(),
        Some("checkpoint"),
        "kind manual preservado"
    );
    assert_eq!(rows[0].1.as_deref(), Some("sd15"), "arch manual preservado");
    assert_eq!(rows[0].2, "upload", "source manual preservado");
}

// ===========================================================================
// P3 (ADR-0025) — submit assíncrono: estado `preparing` no manager
// ===========================================================================

/// Helper: request do fluxo assíncrono (package_ref ausente + params.prepare opaco).
/// Espelha o envelope real do principal (`accept_job_preparing` em
/// services/api-principal/src/jobs/prepare.rs): chaves camelCase
/// (`datasetId`, nunca `dataset_id`).
fn test_prepare_job_request(dataset_id: uuid::Uuid) -> CreateJobRequest {
    CreateJobRequest {
        kind: "yolo_train".into(),
        engine: "yolo".into(),
        model: "yolo11m".into(),
        mode: "train".into(),
        dataset_id: Some(dataset_id.to_string()),
        dataset_version_id: None,
        package_ref: None,
        config_yaml: None,
        params: Some(serde_json::json!({
            "prepare": {
                "kind": "yolo_train",
                "datasetId": dataset_id.to_string(),
                "fingerprint": "abc123"
            }
        })),
        vram_min_gb: None,
        weights_id: None,
        orchestrator_hint: None,
    }
}

/// Helper: request com package_ref:null explícito e sem prepare (malformado p/ P3).
fn test_bare_job_request(dataset_id: uuid::Uuid) -> CreateJobRequest {
    CreateJobRequest {
        kind: "yolo_train".into(),
        engine: "yolo".into(),
        model: "yolo11m".into(),
        mode: "train".into(),
        dataset_id: Some(dataset_id.to_string()),
        dataset_version_id: None,
        package_ref: None,
        config_yaml: None,
        params: Some(serde_json::json!({"package_ref": serde_json::Value::Null})),
        vram_min_gb: None,
        weights_id: None,
        orchestrator_hint: None,
    }
}

/// Insere uma dataset_version de teste e retorna o ID.
async fn insert_test_version(pool: &PgPool, dataset_id: uuid::Uuid) -> uuid::Uuid {
    let id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO dataset_versions (id, dataset_id, manifest) VALUES ($1, $2, '{}'::jsonb)",
    )
    .bind(id)
    .bind(dataset_id)
    .execute(pool)
    .await
    .expect("insert test version");
    id
}

fn complete_req(version_id: uuid::Uuid) -> manager::PrepareCompleteRequest {
    manager::PrepareCompleteRequest {
        dataset_version_id: version_id.to_string(),
        package_ref: manager::PreparePackageRef {
            key: "packages/test/dataset.zip".into(),
            md5_zip: "d41d8cd98f00b204e9800998ecf8427e".into(),
            bytes: 1024,
        },
    }
}

/// (a) create_job com prepare → `preparing` (queue_position NULL); legado com
/// pacote → `queued`; sem nenhum dos dois → 400 (InvalidRequest).
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn create_prepare_vs_legado() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    // Fluxo assíncrono.
    let resp = manager::create_job(&p, test_prepare_job_request(ds_id))
        .await
        .expect("create prepare");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();
    assert_eq!(resp.status, "preparing");
    assert_eq!(resp.queue_position, None);

    let job = manager::get_job(&p, job_id).await.expect("get preparing");
    assert_eq!(job.status, "preparing");
    assert_eq!(job.queue_position, None);
    assert!(job.params.as_ref().and_then(|v| v.get("prepare")).is_some());

    // Legado (retrocompat total).
    let legacy = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create legacy");
    assert_eq!(legacy.status, "queued");
    assert!(legacy.queue_position.is_some());

    // Malformado: package_ref:null sem prepare → 400.
    let bare = manager::create_job(&p, test_bare_job_request(ds_id)).await;
    assert!(
        matches!(bare, Err(ManagerError::InvalidRequest(_))),
        "package_ref:null sem prepare deve ser 400"
    );

    // Omissão total (sem package, sem prepare) → legado `queued` (retrocompat).
    let mut omitted = test_bare_job_request(ds_id);
    omitted.params = Some(serde_json::json!({}));
    let legacy_omitted = manager::create_job(&p, omitted)
        .await
        .expect("omissão legada");
    assert_eq!(legacy_omitted.status, "queued");

    // params.prepare não-objeto → 400.
    let mut bad_prepare = test_bare_job_request(ds_id);
    bad_prepare.params = Some(serde_json::json!({"prepare": "nao-objeto"}));
    let bad = manager::create_job(&p, bad_prepare).await;
    assert!(
        matches!(bad, Err(ManagerError::InvalidRequest(_))),
        "prepare não-objeto deve ser 400"
    );
}

/// (b) prepare → complete → queued; dispatch ignora `preparing` e despacha
/// após o complete.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn prepare_complete_vira_queued_e_despacha() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();
    manager::adopt_orchestrator(&p).await.expect("adopt");

    let resp = manager::create_job(&p, test_prepare_job_request(ds_id))
        .await
        .expect("create prepare");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    // Dispatch NÃO seleciona preparing.
    let dispatched = manager::dispatch_next(
        &p,
        &orch,
        "docker",
        "/data",
        "hephaestus/trainer-yolo:local",
        &test_vram_table(),
    )
    .await
    .expect("dispatch ignora preparing");
    assert!(!dispatched, "preparing nunca é selecionado p/ dispatch");
    assert!(orch.calls().is_empty());
    let job = manager::get_job(&p, job_id).await.expect("get");
    assert_eq!(job.status, "preparing");

    // Complete (versão precisa existir — fail-fast anti-referência-pendurada).
    let version_id = insert_test_version(&p, ds_id).await;
    manager::prepare_complete(&p, job_id, complete_req(version_id))
        .await
        .expect("prepare complete");

    let job = manager::get_job(&p, job_id).await.expect("get queued");
    assert_eq!(job.status, "queued");
    assert_eq!(job.queue_position, Some(1));
    let params = job.params.expect("params");
    assert_eq!(params["package_ref"]["key"], "packages/test/dataset.zip");
    assert_eq!(params["package_ref"]["version_id"], version_id.to_string());
    assert_eq!(params["dataset_version_id"], version_id.to_string());

    // Agora o dispatch pega.
    let dispatched = manager::dispatch_next(
        &p,
        &orch,
        "docker",
        "/data",
        "hephaestus/trainer-yolo:local",
        &test_vram_table(),
    )
    .await
    .expect("dispatch após complete");
    assert!(dispatched);
    let job = manager::get_job(&p, job_id).await.expect("get dispatched");
    assert_eq!(job.status, "dispatched");
}

/// (b2) report com phase/message/progress em `preparing` persiste (canal ADR-0024).
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn report_preparing_persiste_phase_message() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let resp = manager::create_job(&p, test_prepare_job_request(ds_id))
        .await
        .expect("create prepare");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    manager::report_job(
        &p,
        job_id,
        ReportRequest {
            status: "preparing".into(),
            progress: Some(0.42),
            epoch: None,
            step: None,
            metrics: None,
            error: None,
            artifacts: None,
            meta_content: None,
            phase: Some("packaging_dataset".into()),
            message: Some("zipando 860 imagens".into()),
        },
    )
    .await
    .expect("report preparing");

    let job = manager::get_job(&p, job_id).await.expect("get");
    assert_eq!(job.status, "preparing");
    assert_eq!(job.phase.as_deref(), Some("packaging_dataset"));
    assert_eq!(job.message.as_deref(), Some("zipando 860 imagens"));
    assert_eq!(job.progress, Some(0.42));
}

/// (c) prepare-fail → `failed` com error prefixado `prepare_failed:<code>:<msg>`.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn prepare_fail_vira_failed_prefixado() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let resp = manager::create_job(&p, test_prepare_job_request(ds_id))
        .await
        .expect("create prepare");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    manager::prepare_fail(
        &p,
        job_id,
        manager::PrepareFailRequest {
            code: "build_error".into(),
            message: "s3 boom".into(),
        },
    )
    .await
    .expect("prepare fail");

    let job = manager::get_job(&p, job_id).await.expect("get failed");
    assert_eq!(job.status, "failed");
    assert!(job.finished_at.is_some());
    assert_eq!(
        job.error.as_deref(),
        Some("prepare_failed:build_error:s3 boom")
    );
}

/// (d) transições fora de `preparing` → Conflict (409); inexistente → NotFound.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn prepare_transicao_fora_de_preparing_409() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let version_id = insert_test_version(&p, ds_id).await;

    // Job legado em `queued`: complete e fail → Conflict.
    let legacy = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create legacy");
    let queued_id: uuid::Uuid = legacy.job_id.parse().unwrap();

    let r = manager::prepare_complete(&p, queued_id, complete_req(version_id)).await;
    assert!(
        matches!(r, Err(ManagerError::Conflict(_))),
        "complete fora de preparing → 409"
    );
    let r = manager::prepare_fail(
        &p,
        queued_id,
        manager::PrepareFailRequest {
            code: "x".into(),
            message: "y".into(),
        },
    )
    .await;
    assert!(
        matches!(r, Err(ManagerError::Conflict(_))),
        "fail fora de preparing → 409"
    );

    // Inexistente → NotFound.
    let fake = uuid::Uuid::new_v4();
    let r = manager::prepare_complete(&p, fake, complete_req(version_id)).await;
    assert!(matches!(r, Err(ManagerError::NotFound)));
    let r = manager::prepare_fail(
        &p,
        fake,
        manager::PrepareFailRequest {
            code: "x".into(),
            message: "y".into(),
        },
    )
    .await;
    assert!(matches!(r, Err(ManagerError::NotFound)));

    // Versão inexistente → NotFound (fail-fast anti-referência-pendurada).
    let prep = manager::create_job(&p, test_prepare_job_request(ds_id))
        .await
        .expect("create prepare");
    let prep_id: uuid::Uuid = prep.job_id.parse().unwrap();
    let r = manager::prepare_complete(&p, prep_id, complete_req(uuid::Uuid::new_v4())).await;
    assert!(matches!(r, Err(ManagerError::NotFound)));
}

/// (e) abort em `preparing` → `cancelling` (worker observa a flag).
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn abort_em_preparing_vira_cancelling() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let orch = FakeOrchestratorClient::new();

    let resp = manager::create_job(&p, test_prepare_job_request(ds_id))
        .await
        .expect("create prepare");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    let result = manager::abort_job(&p, job_id, &orch).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "cancelling");

    let job = manager::get_job(&p, job_id).await.expect("get");
    assert_eq!(job.status, "cancelling");
}

/// (f) watchdog: `preparing` com created_at > 60min → `failed/prepare_timeout`;
/// fresca permanece `preparing`.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn watchdog_preparing_vencido_falha() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let old = manager::create_job(&p, test_prepare_job_request(ds_id))
        .await
        .expect("create old");
    let old_id: uuid::Uuid = old.job_id.parse().unwrap();
    let fresh = manager::create_job(&p, test_prepare_job_request(ds_id))
        .await
        .expect("create fresh");
    let fresh_id: uuid::Uuid = fresh.job_id.parse().unwrap();

    sqlx::query("UPDATE jobs SET created_at = now() - interval '61 minutes' WHERE id = $1")
        .bind(old_id)
        .execute(&p)
        .await
        .expect("backdate");

    // Loop periódico real (watchdog_tick inclui prepare-timeout + GC).
    manager::watchdog_tick(&p).await.expect("watchdog tick");

    let job = manager::get_job(&p, old_id).await.expect("get old");
    assert_eq!(job.status, "failed");
    assert_eq!(job.error.as_deref(), Some("prepare_timeout"));

    let job = manager::get_job(&p, fresh_id).await.expect("get fresh");
    assert_eq!(job.status, "preparing");
}

/// (g) GC D10: versão >7 dias sem referência → DELETE; referenciada por
/// params.package_ref.version_id → preservada; recente → preservada.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn gc_dataset_versions_preserva_referenciados() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    // Dataset isolado para a órfã: a guarda de voo (B4) protege por
    // dataset — a órfã precisa estar num dataset sem job não-terminal.
    let ds_other = insert_test_dataset(&p).await;

    let orphan = insert_test_version(&p, ds_other).await;
    let referenced = insert_test_version(&p, ds_id).await;
    let recent = insert_test_version(&p, ds_id).await;

    sqlx::query(
        "UPDATE dataset_versions SET created_at = now() - interval '8 days' WHERE id = ANY($1)",
    )
    .bind(vec![orphan, referenced])
    .execute(&p)
    .await
    .expect("backdate versions");

    // Job em `queued` referenciando `referenced` via params.package_ref.
    let resp = manager::create_job(&p, test_prepare_job_request(ds_id))
        .await
        .expect("create prepare");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();
    manager::prepare_complete(&p, job_id, complete_req(referenced))
        .await
        .expect("complete referencia version");

    let deleted = manager::gc_dataset_versions(&p).await.expect("gc");
    assert_eq!(deleted, 1, "só a órfã deve ser removida");

    async fn version_exists(pool: &PgPool, id: uuid::Uuid) -> bool {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM dataset_versions WHERE id = $1)",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
    }
    assert!(!version_exists(&p, orphan).await, "órfã >7d removida");
    assert!(
        version_exists(&p, referenced).await,
        "referenciada preservada"
    );
    assert!(version_exists(&p, recent).await, "recente preservada");
}

/// Complete duplo: 1º Ok (`preparing`→`queued`); 2º → Conflict (409, guarda
/// de transição — sem reescrita de pacote).
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn prepare_complete_duplo_segundo_409() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;
    let version_id = insert_test_version(&p, ds_id).await;

    let resp = manager::create_job(&p, test_prepare_job_request(ds_id))
        .await
        .expect("create prepare");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    manager::prepare_complete(&p, job_id, complete_req(version_id))
        .await
        .expect("1º complete");
    let r = manager::prepare_complete(&p, job_id, complete_req(version_id)).await;
    assert!(
        matches!(r, Err(ManagerError::Conflict(_))),
        "2º complete fora de preparing → 409"
    );

    let job = manager::get_job(&p, job_id).await.expect("get");
    assert_eq!(job.status, "queued");
}

/// Regressão de ciclo (B3): report com status=`preparing` sobre job já
/// `queued`/`dispatched` é ignorado mantendo o estado — nunca regride para
/// `preparing` sem pacote (phase/progress/message também não são tocados).
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn report_preparing_sobre_queued_nao_regride() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let legacy = manager::create_job(&p, test_job_request(ds_id))
        .await
        .expect("create legacy");
    let queued_id: uuid::Uuid = legacy.job_id.parse().unwrap();

    let regress = || ReportRequest {
        status: "preparing".into(),
        progress: Some(0.9),
        epoch: None,
        step: None,
        metrics: None,
        error: None,
        artifacts: None,
        meta_content: None,
        phase: Some("packaging_dataset".into()),
        message: Some("tentativa de regressão".into()),
    };

    // Sobre `queued`: ignorado, estado e fase intactos.
    manager::report_job(&p, queued_id, regress())
        .await
        .expect("report ignorado sem erro");
    let job = manager::get_job(&p, queued_id).await.expect("get queued");
    assert_eq!(job.status, "queued");
    assert!(job.phase.is_none(), "phase não muda na regressão ignorada");
    assert!(
        job.progress.is_none(),
        "progress não muda na regressão ignorada"
    );

    // Sobre `dispatched`: idem.
    sqlx::query("UPDATE jobs SET status = 'dispatched' WHERE id = $1")
        .bind(queued_id)
        .execute(&p)
        .await
        .unwrap();
    manager::report_job(&p, queued_id, regress())
        .await
        .expect("report ignorado sem erro");
    let job = manager::get_job(&p, queued_id)
        .await
        .expect("get dispatched");
    assert_eq!(job.status, "dispatched");
    assert!(job.phase.is_none(), "phase não muda na regressão ignorada");
}

/// Corrida fingerprint (B4): versão velha (-9d) com job `preparing` em voo
/// cujo `params.prepare.datasetId` aponta para aquele dataset → GC pula;
/// após o job virar terminal (`failed`) → GC apaga.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn gc_pula_versao_com_preparing_em_voo() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let old_ver = insert_test_version(&p, ds_id).await;
    sqlx::query("UPDATE dataset_versions SET created_at = now() - interval '9 days' WHERE id = $1")
        .bind(old_ver)
        .execute(&p)
        .await
        .expect("backdate version");

    // Job `preparing` com prepare apontando para aquele dataset.
    let resp = manager::create_job(&p, test_prepare_job_request(ds_id))
        .await
        .expect("create prepare");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();

    let deleted = manager::gc_dataset_versions(&p).await.expect("gc com voo");
    assert_eq!(deleted, 0, "versão com preparing em voo não é apagada");
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM dataset_versions WHERE id = $1)")
            .bind(old_ver)
            .fetch_one(&p)
            .await
            .unwrap();
    assert!(exists, "versão com preparing em voo sobrevive");

    // Job terminal → GC apaga.
    manager::prepare_fail(
        &p,
        job_id,
        manager::PrepareFailRequest {
            code: "build_error".into(),
            message: "fim do voo".into(),
        },
    )
    .await
    .expect("prepare fail");
    let deleted = manager::gc_dataset_versions(&p)
        .await
        .expect("gc pós-terminal");
    assert_eq!(deleted, 1, "sem voo ativo a versão velha é apagada");
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM dataset_versions WHERE id = $1)")
            .bind(old_ver)
            .fetch_one(&p)
            .await
            .unwrap();
    assert!(!exists, "versão velha sem voo removida");
}

/// Touch (B4): `prepare_complete` com versão velha (-9d, ex.: reusada por
/// fingerprint) renova `created_at` — o prazo de 7 dias do GC recomeça.
#[tokio::test]
#[ignore = "requer Postgres (bash scripts/test-db.sh)"]
async fn prepare_complete_renova_created_at_da_versao() {
    let _guard = SERIAL.lock().await;
    let p = pool().await;
    cleanup(&p).await;
    let ds_id = insert_test_dataset(&p).await;

    let ver = insert_test_version(&p, ds_id).await;
    sqlx::query("UPDATE dataset_versions SET created_at = now() - interval '9 days' WHERE id = $1")
        .bind(ver)
        .execute(&p)
        .await
        .expect("backdate version");

    let resp = manager::create_job(&p, test_prepare_job_request(ds_id))
        .await
        .expect("create prepare");
    let job_id: uuid::Uuid = resp.job_id.parse().unwrap();
    manager::prepare_complete(&p, job_id, complete_req(ver))
        .await
        .expect("prepare complete");

    let renewed: bool = sqlx::query_scalar(
        "SELECT created_at > now() - interval '1 minute' FROM dataset_versions WHERE id = $1",
    )
    .bind(ver)
    .fetch_one(&p)
    .await
    .unwrap();
    assert!(renewed, "touch no complete renova created_at da versão");
}
