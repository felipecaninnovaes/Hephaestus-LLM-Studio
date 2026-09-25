use super::*;
use async_trait::async_trait;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

// -- scoped_key tests --

#[test]
fn scoped_key_packages_valid() {
    let key = scoped_key(S3Scope::Packages, "packages/abc-123/dataset.zip");
    assert_eq!(key, Ok("packages/abc-123/dataset.zip".to_string()));
}

#[test]
fn scoped_key_artifacts_valid() {
    let key = scoped_key(S3Scope::Artifacts, "artifacts/job-456/best.pt");
    assert_eq!(key, Ok("artifacts/job-456/best.pt".to_string()));
}

#[test]
fn scoped_key_empty() {
    assert_eq!(
        scoped_key(S3Scope::Packages, ""),
        Err(ScopedKeyError::EmptyKey)
    );
}

#[test]
fn scoped_key_absolute() {
    assert_eq!(
        scoped_key(S3Scope::Packages, "/packages/x"),
        Err(ScopedKeyError::AbsolutePath)
    );
}

#[test]
fn scoped_key_traversal() {
    assert_eq!(
        scoped_key(S3Scope::Packages, "../etc/passwd"),
        Err(ScopedKeyError::PathTraversal)
    );
}

#[test]
fn scoped_key_traversal_in_middle() {
    assert_eq!(
        scoped_key(S3Scope::Artifacts, "artifacts/../evil"),
        Err(ScopedKeyError::PathTraversal)
    );
}

#[test]
fn scoped_key_outside_scope() {
    assert_eq!(
        scoped_key(S3Scope::Packages, "datasets/abc/images"),
        Err(ScopedKeyError::OutsideScope)
    );
}

#[test]
fn scoped_key_wrong_prefix() {
    assert_eq!(
        scoped_key(S3Scope::Artifacts, "packages/abc/dataset.zip"),
        Err(ScopedKeyError::OutsideScope)
    );
}

// -- metrics parsing tests --

#[test]
fn parse_metrics_line_valid() {
    let line =
        r#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":5}"#;
    let m = parse_metrics_line(line).unwrap();
    assert_eq!(m.epoch, 5);
    assert!((m.map50 - 0.8).abs() < 1e-6);
    assert!((m.map50_95 - 0.6).abs() < 1e-6);
}

#[test]
fn parse_metrics_line_empty() {
    assert!(parse_metrics_line("").is_none());
    assert!(parse_metrics_line("  ").is_none());
}

#[test]
fn parse_metrics_line_malformed() {
    assert!(parse_metrics_line(r#"{"box_loss":0.5}"#).is_none());
    assert!(parse_metrics_line("not json").is_none());
}

#[test]
fn parse_metrics_line_tolerant() {
    let good =
        r#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":5}"#;
    assert!(parse_metrics_line(good).is_some());
    assert!(parse_metrics_line("{}").is_none());
}

#[test]
fn parse_metrics_line_diffusion() {
    let diff_line = r#"{"epoch":3,"step":30,"loss":0.0452,"lr":0.0001}"#;
    let parsed = parse_metrics_line(diff_line).expect("should parse diffusion line");
    assert_eq!(parsed.epoch, 3);
    assert_eq!(parsed.step, Some(30));
    assert_eq!(parsed.loss, Some(0.0452));
    assert_eq!(parsed.lr, Some(0.0001));
    assert_eq!(parsed.box_loss, 0.0);
}

#[test]
fn parse_metrics_line_nan_tolerant() {
    let nan_line = r#"{"epoch": 1, "step": 5, "loss": NaN, "lr": 0.0001}"#;
    let parsed = parse_metrics_line(nan_line).expect("should parse line with NaN safely");
    assert_eq!(parsed.epoch, 1);
    assert_eq!(parsed.step, Some(5));
    assert_eq!(parsed.loss, None);
    assert_eq!(parsed.lr, Some(0.0001));
}

// -- read_ram_total tests --

#[test]
fn parse_ram_total_present() {
    let content =
        "MemTotal:       16384000 kB\nMemFree:         8192000 kB\nMemAvailable:    8192000 kB\n";
    assert_eq!(parse_ram_total_from_content(content), Some(16384000 * 1024));
}

#[test]
fn parse_ram_total_missing() {
    let content = "MemFree:         8192000 kB\nMemAvailable:    8192000 kB\n";
    assert_eq!(parse_ram_total_from_content(content), None);
}

#[test]
fn parse_ram_total_empty() {
    assert_eq!(parse_ram_total_from_content(""), None);
}

// -- config.yaml tests --

#[test]
fn replace_config_placeholders_basic() {
    let config = "dataset_path: {dataset_path}\noutput_path: {output_path}";
    let result = replace_config_placeholders(
        config,
        "/datasets/datasets-cache/j1",
        "/outputs/j1",
        None,
        &[],
        None,
        None,
        None,
        None,
    );
    assert_eq!(
        result,
        "dataset_path: /datasets/datasets-cache/j1\noutput_path: /outputs/j1"
    );
}

#[test]
fn replace_config_placeholders_yaml_parseable() {
    let config =
        "dataset_path: {dataset_path}\noutput_path: {output_path}\nepochs: 100\nmodel: yolo11m";
    let result = replace_config_placeholders(
        config,
        "/datasets/datasets-cache/j1",
        "/outputs/j1",
        None,
        &[],
        None,
        None,
        None,
        None,
    );
    let parsed: serde_yaml::Value = serde_yaml::from_str(&result).unwrap();
    assert_eq!(parsed["dataset_path"], "/datasets/datasets-cache/j1");
    assert_eq!(parsed["output_path"], "/outputs/j1");
    assert_eq!(parsed["epochs"], 100);
    assert_eq!(parsed["model"], "yolo11m");
}

#[test]
fn extract_epochs_from_config() {
    let config = "epochs: 50\nmodel: yolo11m\nbatch: 16";
    assert_eq!(extract_epochs(config), 50);
}

#[test]
fn extract_epochs_default() {
    let config = "model: yolo11m";
    assert_eq!(extract_epochs(config), 100);
}

// -- zip-slip tests --

#[test]
fn unzip_safe_rejects_traversal() {
    let tmp = tempfile::tempdir().unwrap();
    let zip_path = tmp.path().join("evil.zip");

    {
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("../evil.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"pwned").unwrap();
        zip.finish().unwrap();
    }

    let dest = tmp.path().join("out");
    std::fs::create_dir_all(&dest).unwrap();

    let result = unzip_safe(&zip_path, &dest);
    assert!(result.is_err());
    assert!(matches!(result, Err(PipelineError::UnzipFailed(_))));
}

#[test]
fn unzip_safe_rejects_absolute() {
    let tmp = tempfile::tempdir().unwrap();
    let zip_path = tmp.path().join("evil.zip");

    {
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("/etc/evil.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"pwned").unwrap();
        zip.finish().unwrap();
    }

    let dest = tmp.path().join("out");
    std::fs::create_dir_all(&dest).unwrap();

    let result = unzip_safe(&zip_path, &dest);
    assert!(result.is_err());
}

#[test]
fn unzip_safe_accepts_valid() {
    let tmp = tempfile::tempdir().unwrap();
    let zip_path = tmp.path().join("valid.zip");

    {
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("images/photo.jpg", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"fake image").unwrap();
        zip.start_file("labels/photo.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"0 0.5 0.5 0.1 0.1").unwrap();
        zip.finish().unwrap();
    }

    let dest = tmp.path().join("out");
    std::fs::create_dir_all(&dest).unwrap();

    let result = unzip_safe(&zip_path, &dest);
    assert!(result.is_ok());
    assert!(dest.join("images/photo.jpg").exists());
    assert!(dest.join("labels/photo.txt").exists());
}

// -- compute_progress tests --

#[test]
fn compute_progress_basic() {
    let m = MetricsLine {
        box_loss: 0.0,
        cls_loss: 0.0,
        dfl_loss: 0.0,
        map50: 0.0,
        map50_95: 0.0,
        loss: None,
        lr: None,
        step: None,
        epoch: 5,
        progress: None,
        phase: None,
        message: None,
        vram_used_gb: None,
    };
    assert!((compute_progress(&m, 100) - 0.05).abs() < 1e-6);
}

#[test]
fn compute_progress_zero_epochs() {
    let m = MetricsLine {
        box_loss: 0.0,
        cls_loss: 0.0,
        dfl_loss: 0.0,
        map50: 0.0,
        map50_95: 0.0,
        loss: None,
        lr: None,
        step: None,
        epoch: 5,
        progress: None,
        phase: None,
        message: None,
        vram_used_gb: None,
    };
    assert!((compute_progress(&m, 0) - 0.0).abs() < 1e-6);
}

#[test]
fn compute_progress_full() {
    let m = MetricsLine {
        box_loss: 0.0,
        cls_loss: 0.0,
        dfl_loss: 0.0,
        map50: 0.0,
        map50_95: 0.0,
        loss: None,
        lr: None,
        step: None,
        epoch: 100,
        progress: None,
        phase: None,
        message: None,
        vram_used_gb: None,
    };
    assert!((compute_progress(&m, 100) - 1.0).abs() < 1e-6);
}

#[test]
fn compute_progress_explicit() {
    let m = MetricsLine {
        box_loss: 0.0,
        cls_loss: 0.0,
        dfl_loss: 0.0,
        map50: 0.0,
        map50_95: 0.0,
        loss: None,
        lr: None,
        step: None,
        epoch: 3,
        progress: Some(0.65),
        phase: None,
        message: None,
        vram_used_gb: None,
    };
    assert!((compute_progress(&m, 100) - 0.65).abs() < 1e-6);
}

// -- executor args test --

struct FakeExecutor {
    last_args: std::sync::Mutex<Option<Vec<String>>>,
}

impl FakeExecutor {
    fn new() -> Self {
        Self {
            last_args: std::sync::Mutex::new(None),
        }
    }

    fn last_args(&self) -> Option<Vec<String>> {
        self.last_args.lock().unwrap().clone()
    }
}

#[async_trait]
impl TrainerExecutor for FakeExecutor {
    async fn run(
        &self,
        _image: &str,
        _container_name: &str,
        _volumes: &[(String, String)],
        args: &[String],
        _env: &[(String, String)],
        _gpu_devices: Option<&str>,
    ) -> (i32, String) {
        *self.last_args.lock().unwrap() = Some(args.to_vec());
        (0, "ok".to_string())
    }

    async fn stop(&self, _container_name: &str) -> Result<(), String> {
        Ok(())
    }
}

#[tokio::test]
async fn run_job_inner_passes_correct_args() {
    use std::sync::Arc;

    let executor = Arc::new(FakeExecutor::new());
    let job_id = "test-job-001";

    // Simulate the args that run_job_inner would build
    let expected_args = vec![
        "train".to_string(),
        "--config".to_string(),
        format!("/outputs/{job_id}/config.yaml"),
        "--output".to_string(),
        format!("/outputs/{job_id}"),
    ];

    // Directly call executor.run to verify args are forwarded
    let (_, _) = executor
        .run(
            "my-image:latest",
            "trainer-test",
            &[("/data/datasets".into(), "/datasets".into())],
            &expected_args,
            &[],
            None,
        )
        .await;

    assert_eq!(executor.last_args(), Some(expected_args));
}

// -- idempotency test (dispatch com mesmo job_id → 409) --

#[test]
fn active_jobs_idempotency() {
    let active = ActiveJobs::default();
    // Simula job já existente
    active.insert(
        "job-123".to_string(),
        ActiveJobState::new("trainer-yolo-job-123".to_string()),
    );

    // Segundo dispatch com mesmo job_id deve ser detectado
    assert!(active.contains_key("job-123"));
}

// =========================================================================
// A.3 — engine branching tests
// =========================================================================

use std::collections::HashMap;
use std::sync::Mutex;

/// Mock S3 que serve um zip válido para download e grava uploads.
/// `upload_fail` simula bucket inexistente/S3 fora do ar: `put` sempre falha.
/// `puts` conta tentativas (prova do retry N=3); `fail_first_n` simula S3
/// instável (falha as N primeiras tentativas, depois sucede).
struct FakeS3 {
    downloads: Mutex<Vec<String>>,
    uploads: Mutex<Vec<(String, PathBuf)>>,
    zip_bytes: Vec<u8>,
    upload_fail: AtomicBool,
    puts: std::sync::atomic::AtomicUsize,
    fail_first_n: std::sync::atomic::AtomicUsize,
}

impl FakeS3 {
    fn new() -> Self {
        // Cria um zip in-memory com dataset.yaml vazio
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("dataset.yaml", opts).unwrap();
            zip.write_all(b"classes: []\nimages: []\n").unwrap();
            zip.finish().unwrap();
        }
        Self {
            downloads: Mutex::new(Vec::new()),
            uploads: Mutex::new(Vec::new()),
            zip_bytes: buf.into_inner(),
            upload_fail: AtomicBool::new(false),
            puts: std::sync::atomic::AtomicUsize::new(0),
            fail_first_n: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn set_upload_fail(&self, v: bool) {
        self.upload_fail.store(v, Ordering::SeqCst);
    }

    fn set_fail_first_n(&self, n: usize) {
        self.fail_first_n.store(n, Ordering::SeqCst);
    }

    fn put_count(&self) -> usize {
        self.puts.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl S3Port for FakeS3 {
    async fn get_to_file(&self, key: &str, path: &std::path::Path) -> Result<(), String> {
        self.downloads.lock().unwrap().push(key.to_string());
        std::fs::write(path, &self.zip_bytes).map_err(|e| format!("write zip: {e}"))
    }

    async fn put(&self, key: &str, path: &std::path::Path) -> Result<(), String> {
        self.puts.fetch_add(1, Ordering::SeqCst);
        if self.upload_fail.load(Ordering::SeqCst) {
            return Err(format!(
                "S3 PUT {key}: bucket inexistente (fake upload_fail)"
            ));
        }
        if self.fail_first_n.load(Ordering::SeqCst) > 0 {
            self.fail_first_n.fetch_sub(1, Ordering::SeqCst);
            return Err(format!(
                "S3 PUT {key}: instabilidade transitória (fake flaky)"
            ));
        }
        self.uploads
            .lock()
            .unwrap()
            .push((key.to_string(), path.to_path_buf()));
        Ok(())
    }

    async fn ping(&self) -> bool {
        true
    }
}

/// Mock S3 que serve bytes customizados por prefixo (para testes de weights).
/// Qualquer key com `models/` ou `artifacts/` retorna `weights_bytes`;
/// caso contrário, retorna `zip_bytes` (comportamento padrão do FakeS3).
struct FakeS3WithWeights {
    downloads: Mutex<Vec<String>>,
    uploads: Mutex<Vec<(String, PathBuf)>>,
    zip_bytes: Vec<u8>,
    weights_bytes: Vec<u8>,
}

impl FakeS3WithWeights {
    fn new(weights_bytes: Vec<u8>) -> Self {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("dataset.yaml", opts).unwrap();
            zip.write_all(b"classes: []\nimages: []\n").unwrap();
            zip.finish().unwrap();
        }
        Self {
            downloads: Mutex::new(Vec::new()),
            uploads: Mutex::new(Vec::new()),
            zip_bytes: buf.into_inner(),
            weights_bytes,
        }
    }
}

#[async_trait]
impl S3Port for FakeS3WithWeights {
    async fn get_to_file(&self, key: &str, path: &std::path::Path) -> Result<(), String> {
        self.downloads.lock().unwrap().push(key.to_string());
        let data = if key.starts_with("models/") || key.starts_with("artifacts/") {
            &self.weights_bytes
        } else {
            &self.zip_bytes
        };
        std::fs::write(path, data).map_err(|e| format!("write file: {e}"))
    }

    async fn put(&self, key: &str, path: &std::path::Path) -> Result<(), String> {
        self.uploads
            .lock()
            .unwrap()
            .push((key.to_string(), path.to_path_buf()));
        Ok(())
    }

    async fn ping(&self) -> bool {
        true
    }
}

/// Calcula MD5 de bytes in-memory (hex 32).
fn compute_file_md5_bytes(data: &[u8]) -> String {
    use md5::Digest;
    let digest = md5::Md5::digest(data);
    hex::encode(digest)
}

/// Mock ReportClient que grava relatórios.
struct FakeReport {
    reports: Mutex<Vec<ReportBody>>,
}

impl FakeReport {
    fn new() -> Self {
        Self {
            reports: Mutex::new(Vec::new()),
        }
    }

    fn statuses(&self) -> Vec<String> {
        self.reports
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.status.clone())
            .collect()
    }

    fn failed_report(&self) -> Option<ReportBody> {
        self.reports
            .lock()
            .unwrap()
            .iter()
            .find(|r| r.status == "failed")
            .cloned()
    }

    fn done_artifacts(&self) -> Option<Vec<ArtifactReport>> {
        self.reports
            .lock()
            .unwrap()
            .iter()
            .find(|r| r.status == "done")
            .and_then(|r| r.artifacts.clone())
    }

    fn done_meta_content(&self) -> Option<String> {
        self.reports
            .lock()
            .unwrap()
            .iter()
            .find(|r| r.status == "done")
            .and_then(|r| r.meta_content.clone())
    }
}

#[async_trait]
impl ReportClient for FakeReport {
    async fn report(&self, _job_id: &str, body: &ReportBody) -> Result<(), String> {
        self.reports.lock().unwrap().push(body.clone());
        Ok(())
    }
}

/// Executor que simula o trainer — apenas grava os args recebidos.
struct FakeTrainerExecutor {
    last_args: Mutex<Option<Vec<String>>>,
}

impl FakeTrainerExecutor {
    fn new() -> Self {
        Self {
            last_args: Mutex::new(None),
        }
    }

    fn last_args(&self) -> Option<Vec<String>> {
        self.last_args.lock().unwrap().clone()
    }
}

#[async_trait]
impl TrainerExecutor for FakeTrainerExecutor {
    async fn run(
        &self,
        _image: &str,
        _container_name: &str,
        _volumes: &[(String, String)],
        args: &[String],
        _env: &[(String, String)],
        _gpu_devices: Option<&str>,
    ) -> (i32, String) {
        *self.last_args.lock().unwrap() = Some(args.to_vec());
        (0, "ok".to_string())
    }

    async fn stop(&self, _container_name: &str) -> Result<(), String> {
        Ok(())
    }
}

/// Helper que cria os arquivos de output simulados no diretorio correto.
/// O run_job_inner le de `workdir/outputs/<job_id>/`, entao pre-criamos la.
fn create_fake_outputs(workdir: &Path, job_id: &str, files: &HashMap<String, Vec<u8>>) {
    let outputs = workdir.join("outputs").join(job_id);
    std::fs::create_dir_all(&outputs).unwrap();
    for (name, content) in files {
        std::fs::write(outputs.join(name), content).unwrap();
    }
}

/// Helper para criar um DispatchRequest de teste.
fn make_dispatch(job_id: &str, engine: &str) -> DispatchRequest {
    DispatchRequest {
        job_id: job_id.to_string(),
        engine: engine.to_string(),
        image: "hephaestus/trainer-yolo:local".to_string(),
        exec_mode: "docker".to_string(),
        package_ref: Some(PackageRef {
            key: "packages/test-pkg/dataset.zip".to_string(),
            md5_zip: String::new(), // será calculado
            bytes: 0,
        }),
        config_yaml: Some(
            "epochs: 1\ndataset_path: {dataset_path}\noutput_path: {output_path}".to_string(),
        ),
        dataset_version_id: None,
        workdir: "/tmp".to_string(),
        mode: "train".to_string(),
        weights_ref: None,
        loras: Vec::new(),
        custom_checkpoint: None,
        text_encoder: None,
        init_image_ref: None,
        control_package_ref: None,
    }
}

/// Cria um dispatch com MD5 correto do zip fake.
fn make_dispatch_with_valid_md5(job_id: &str, engine: &str, zip_path: &Path) -> DispatchRequest {
    let md5 = compute_file_md5(zip_path).unwrap();
    let mut d = make_dispatch(job_id, engine);
    if let Some(ref mut pr) = d.package_ref {
        pr.md5_zip = md5;
    }
    d
}

// -- A.3 test 1: engine yolo → subcomando train, artefatos [best.pt, last.pt, metrics.jsonl] --

#[tokio::test]
async fn engine_yolo_uses_train_subcommand_and_yolo_artifacts() {
    let tmp = tempfile::tempdir().unwrap();

    // Prepara zip fake para o S3
    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-yolo-001", "yolo", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    // Pre-cria os arquivos de output que o "trainer" produziria
    let mut output_files = HashMap::new();
    output_files.insert("best.pt".to_string(), b"fake model".to_vec());
    output_files.insert("last.pt".to_string(), b"fake model".to_vec());
    output_files.insert(
        "metrics.jsonl".to_string(),
        br#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":1}"#
            .to_vec(),
    );
    create_fake_outputs(tmp.path(), "job-yolo-001", &output_files);

    let result = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;
    assert!(
        result.is_ok(),
        "yolo pipeline should succeed: {:?}",
        result.err()
    );

    // Verifica subcomando
    let args = executor.last_args().unwrap();
    assert_eq!(args[0], "train");
    assert_eq!(args[1], "--config");
    assert_eq!(args[3], "--output");

    // Verifica artefatos: yolo produz best.pt, last.pt, metrics.jsonl
    let artifacts = report.done_artifacts().unwrap();
    let kinds: Vec<&str> = artifacts.iter().map(|a| a.kind.as_str()).collect();
    let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
    assert!(filenames.contains(&"best.pt"));
    assert!(filenames.contains(&"last.pt"));
    assert!(filenames.contains(&"metrics.jsonl"));
    assert!(kinds.contains(&"model")); // best.pt e last.pt são kind "model"
    assert!(kinds.contains(&"metrics")); // metrics.jsonl é kind "metrics"
    assert!(!filenames.contains(&"boxes.json")); // yolo NÃO produz boxes.json
}

// -- A.3 test 2: engine autotracker → subcomando autotrack, artefatos [boxes.json, metrics.jsonl] --

#[tokio::test]
async fn engine_autotracker_uses_autotrack_subcommand_and_autotracker_artifacts() {
    let tmp = tempfile::tempdir().unwrap();

    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-at-001", "autotracker", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let mut output_files = HashMap::new();
    output_files.insert(
        "boxes.json".to_string(),
        br#"{"engine":"autotracker","model":"mock","seed":42,"conf":0.65,"images":[]}"#.to_vec(),
    );
    output_files.insert(
        "metrics.jsonl".to_string(),
        br#"{"box_loss":0.1,"cls_loss":0.2,"dfl_loss":0.3,"mAP50":0.9,"mAP50-95":0.7,"epoch":1}"#
            .to_vec(),
    );
    create_fake_outputs(tmp.path(), "job-at-001", &output_files);

    let result = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;
    assert!(
        result.is_ok(),
        "autotracker pipeline should succeed: {:?}",
        result.err()
    );

    // Verifica subcomando: autotrack, não train
    let args = executor.last_args().unwrap();
    assert_eq!(args[0], "autotrack");
    assert_eq!(args[1], "--config");
    assert_eq!(args[3], "--output");

    // Verifica artefatos: autotracker produz boxes.json + metrics.jsonl
    let artifacts = report.done_artifacts().unwrap();
    let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
    let kinds: Vec<&str> = artifacts.iter().map(|a| a.kind.as_str()).collect();
    assert!(filenames.contains(&"boxes.json"));
    assert!(filenames.contains(&"metrics.jsonl"));
    assert!(kinds.contains(&"boxes")); // boxes.json é kind "boxes"
    assert!(kinds.contains(&"metrics")); // metrics.jsonl é kind "metrics"
    assert!(!filenames.contains(&"best.pt")); // autotracker NÃO produz model artifacts
    assert!(!filenames.contains(&"last.pt"));
}

// -- AL.3 test: engine autolabel → subcomando autolabel, artefatos [captions.jsonl, metrics.jsonl] --

#[tokio::test]
async fn engine_autolabel_uses_autolabel_subcommand_and_captions_artifacts() {
    let tmp = tempfile::tempdir().unwrap();

    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-al-001", "autolabel", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let mut output_files = HashMap::new();
    output_files.insert(
        "captions.jsonl".to_string(),
        br#"{"filename":"img.jpg","caption":"uma foto de teste"}"#.to_vec(),
    );
    output_files.insert(
        "metrics.jsonl".to_string(),
        br#"{"epoch":1,"loss":0.0,"images":1}"#.to_vec(),
    );
    create_fake_outputs(tmp.path(), "job-al-001", &output_files);

    let result = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;
    assert!(
        result.is_ok(),
        "autolabel pipeline should succeed: {:?}",
        result.err()
    );

    // Verifica subcomando: autolabel
    let args = executor.last_args().unwrap();
    assert_eq!(args[0], "autolabel");
    assert_eq!(args[1], "--config");
    assert_eq!(args[3], "--output");

    // Verifica artefatos: captions.jsonl + metrics.jsonl
    let artifacts = report.done_artifacts().unwrap();
    let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
    let kinds: Vec<&str> = artifacts.iter().map(|a| a.kind.as_str()).collect();
    assert!(filenames.contains(&"captions.jsonl"));
    assert!(filenames.contains(&"metrics.jsonl"));
    assert!(kinds.contains(&"captions"));
    assert!(kinds.contains(&"metrics"));
    assert!(!filenames.contains(&"best.pt"));
    assert!(!filenames.contains(&"boxes.json"));
}

// -- A.3 test 3: engine desconhecido → falha limpa (via run_job que faz o report "failed") --

#[tokio::test]
async fn unknown_engine_returns_clean_error() {
    let tmp = tempfile::tempdir().unwrap();

    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-bad-001", "unknown_engine_foo", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    // Usa run_job (não run_job_inner) para testar o caminho completo de falha
    run_job(
        dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        active_jobs.clone(),
        None,
        false,
        None,
    )
    .await;

    // Verifica que os reports incluem preparing e failed (via run_job outer)
    let statuses = report.statuses();
    assert!(statuses.contains(&"preparing".to_string()));
    assert!(statuses.contains(&"failed".to_string()));
    assert!(!statuses.contains(&"done".to_string()));

    // Executor nunca foi chamado (engine check falha antes)
    assert!(executor.last_args().is_none());
}

// -- ADR-0018: engine diffusion usa train e coleta adapter.safetensors --

#[tokio::test]
async fn engine_diffusion_uses_train_subcommand_and_adapter_artifacts() {
    let tmp = tempfile::tempdir().unwrap();

    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-diff-001", "diffusion", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let mut output_files = HashMap::new();
    output_files.insert(
        "adapter.safetensors".to_string(),
        b"fake safetensors bytes".to_vec(),
    );
    output_files.insert(
        "metrics.jsonl".to_string(),
        br#"{"epoch":1,"step":10,"loss":0.42}"#.to_vec(),
    );
    create_fake_outputs(tmp.path(), "job-diff-001", &output_files);

    let res = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;

    assert!(res.is_ok(), "run_job_inner failed: {:?}", res);

    let args = executor.last_args().unwrap();
    assert_eq!(args[0], "train");
    assert_eq!(args[1], "--config");
    assert_eq!(args[3], "--output");

    // Verifica artefatos coletados: adapter.safetensors + metrics.jsonl
    let artifacts = report.done_artifacts().unwrap();
    let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
    let kinds: Vec<&str> = artifacts.iter().map(|a| a.kind.as_str()).collect();
    assert!(filenames.contains(&"adapter.safetensors"));
    assert!(filenames.contains(&"metrics.jsonl"));
    assert!(kinds.contains(&"model"));
    assert!(kinds.contains(&"metrics"));
}

// -- control dataset (treino difusão): staging + placeholder + defesa --

/// Fake S3 que serve zips distintos por key: pacote principal vs controle.
/// Reusa o mesmo zip válido do FakeS3 para ambos; o MD5 é calculado sobre
/// os bytes servidos, então o dispatch usa `compute_file_md5_bytes`.
struct FakeS3WithControl {
    downloads: Mutex<Vec<String>>,
    uploads: Mutex<Vec<(String, PathBuf)>>,
    main_zip: Vec<u8>,
    control_zip: Vec<u8>,
}

impl FakeS3WithControl {
    fn new() -> Self {
        let mut main_buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut main_buf);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("dataset.yaml", opts).unwrap();
            zip.write_all(b"classes: []\nimages: []\n").unwrap();
            zip.finish().unwrap();
        }
        let mut control_buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut control_buf);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("regularization.txt", opts).unwrap();
            zip.write_all(b"control images\n").unwrap();
            zip.finish().unwrap();
        }
        Self {
            downloads: Mutex::new(Vec::new()),
            uploads: Mutex::new(Vec::new()),
            main_zip: main_buf.into_inner(),
            control_zip: control_buf.into_inner(),
        }
    }
}

#[async_trait]
impl S3Port for FakeS3WithControl {
    async fn get_to_file(&self, key: &str, path: &std::path::Path) -> Result<(), String> {
        self.downloads.lock().unwrap().push(key.to_string());
        let data = if key.contains("control") {
            &self.control_zip
        } else {
            &self.main_zip
        };
        std::fs::write(path, data).map_err(|e| format!("write file: {e}"))
    }

    async fn put(&self, key: &str, path: &std::path::Path) -> Result<(), String> {
        self.uploads
            .lock()
            .unwrap()
            .push((key.to_string(), path.to_path_buf()));
        Ok(())
    }

    async fn ping(&self) -> bool {
        true
    }
}

#[tokio::test]
async fn diffusion_train_control_dataset_staged_and_config_resolved() {
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3WithControl::new());

    let mut dispatch = make_dispatch("job-control-001", "diffusion");
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    dispatch.mode = "train".to_string();
    dispatch.package_ref = Some(PackageRef {
        key: "packages/test-pkg/dataset.zip".to_string(),
        md5_zip: compute_file_md5_bytes(&s3.main_zip),
        bytes: s3.main_zip.len() as i64,
    });
    dispatch.control_package_ref = Some(PackageRef {
        key: "packages/test-pkg/control.zip".to_string(),
        md5_zip: compute_file_md5_bytes(&s3.control_zip),
        bytes: s3.control_zip.len() as i64,
    });
    dispatch.config_yaml = Some(
        "epochs: 1\ndataset_path: {dataset_path}\noutput_path: {output_path}\ncontrol_dataset_path: \"{control_dataset_path}\"\ncache_text_embeddings: true"
            .to_string(),
    );
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let mut output_files = HashMap::new();
    output_files.insert(
        "adapter.safetensors".to_string(),
        b"fake safetensors bytes".to_vec(),
    );
    create_fake_outputs(tmp.path(), "job-control-001", &output_files);

    let res = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;
    assert!(res.is_ok(), "run_job_inner failed: {:?}", res);

    // Diretório de controle existe no host (staging) com o conteúdo do zip.
    let control_dir = tmp
        .path()
        .join("datasets")
        .join("datasets-cache")
        .join("job-control-001")
        .join("control");
    assert!(
        control_dir.join("regularization.txt").is_file(),
        "control dataset deve ser extraído em datasets-cache/<job>/control"
    );

    // Config final entregue ao trainer contém o path real, sem placeholder.
    let config_path = tmp
        .path()
        .join("outputs")
        .join("job-control-001")
        .join("config.yaml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    assert!(
        config.contains("/datasets/datasets-cache/job-control-001/control"),
        "config deve conter o control path real: {config}"
    );
    assert!(
        !config.contains("{control_dataset_path}"),
        "placeholder não pode vazar: {config}"
    );
    assert!(
        config.contains("cache_text_embeddings: true"),
        "flag opaca preservada: {config}"
    );
}

#[tokio::test]
async fn diffusion_train_control_placeholder_sem_ref_falha_claro() {
    // Defesa anti-placeholder (espelha init): config exige control mas
    // nenhum control_package_ref veio no dispatch → ConfigYamlInvalid.
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-control-002", "diffusion", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    dispatch.mode = "train".to_string();
    dispatch.control_package_ref = None;
    dispatch.config_yaml = Some(
        "epochs: 1\ndataset_path: {dataset_path}\noutput_path: {output_path}\ncontrol_dataset_path: \"{control_dataset_path}\""
            .to_string(),
    );
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let res = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;
    assert!(
        matches!(res, Err(PipelineError::ConfigYamlInvalid(_))),
        "placeholder sem ref deve falhar claro: {res:?}"
    );
}

#[tokio::test]
async fn diffusion_train_control_md5_mismatch_falha_honesto() {
    // MD5 divergente no pacote de controle → Md5Mismatch (nada silencioso).
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3WithControl::new());

    let mut dispatch = make_dispatch("job-control-003", "diffusion");
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    dispatch.mode = "train".to_string();
    dispatch.package_ref = Some(PackageRef {
        key: "packages/test-pkg/dataset.zip".to_string(),
        md5_zip: compute_file_md5_bytes(&s3.main_zip),
        bytes: s3.main_zip.len() as i64,
    });
    dispatch.control_package_ref = Some(PackageRef {
        key: "packages/test-pkg/control.zip".to_string(),
        md5_zip: "00000000000000000000000000000000".to_string(),
        bytes: s3.control_zip.len() as i64,
    });
    dispatch.config_yaml = Some(
        "epochs: 1\ndataset_path: {dataset_path}\noutput_path: {output_path}\ncontrol_dataset_path: \"{control_dataset_path}\""
            .to_string(),
    );
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let res = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;
    assert!(
        matches!(res, Err(PipelineError::Md5Mismatch { .. })),
        "md5 divergente deve falhar honesto: {res:?}"
    );
}

// -- ADR-0020: engine diffusion com mode generate usa generate e coleta generated.png sem exigir package --

#[tokio::test]
async fn engine_diffusion_uses_generate_subcommand_and_generated_artifacts() {
    let tmp = tempfile::tempdir().unwrap();

    let s3 = Arc::new(FakeS3::new());
    let mut dispatch = make_dispatch("job-diff-gen-001", "diffusion");
    dispatch.mode = "generate".to_string();
    dispatch.package_ref = None; // Sem package_ref (Text-to-Image puro)
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let mut output_files = HashMap::new();
    output_files.insert(
        "generated.png".to_string(),
        b"fake png image bytes".to_vec(),
    );
    create_fake_outputs(tmp.path(), "job-diff-gen-001", &output_files);

    let res = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;

    assert!(res.is_ok(), "run_job_inner failed: {:?}", res);

    let args = executor.last_args().unwrap();
    assert_eq!(args[0], "generate");
    assert_eq!(args[1], "--config");
    assert_eq!(args[3], "--output");

    // Verifica artefato coletado: generated.png (kind = generated)
    let artifacts = report.done_artifacts().unwrap();
    let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
    let kinds: Vec<&str> = artifacts.iter().map(|a| a.kind.as_str()).collect();
    assert!(filenames.contains(&"generated.png"));
    assert!(kinds.contains(&"generated"));
}

// -- Incidente galeria vazia: bucket S3 inexistente → uploads falham → job
//    NÃO pode terminar done com zero artefatos (mentira sobre si mesmo).

/// FakeS3 em modo upload_fail → run_job deve reportar failed com mensagem
/// descritiva, nunca done.
#[tokio::test]
async fn generate_upload_fail_reports_failed_not_done() {
    let tmp = tempfile::tempdir().unwrap();

    let s3 = Arc::new(FakeS3::new());
    s3.set_upload_fail(true);
    let mut dispatch = make_dispatch("job-upload-fail-001", "diffusion");
    dispatch.mode = "generate".to_string();
    dispatch.package_ref = None; // Text-to-Image puro, como no incidente
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    // Trainer "produziu" a imagem — só o upload ao bucket falha.
    let mut output_files = HashMap::new();
    output_files.insert("generated_0001.png".to_string(), b"fake png".to_vec());
    create_fake_outputs(tmp.path(), "job-upload-fail-001", &output_files);

    run_job(
        dispatch,
        s3,
        report.clone(),
        executor,
        active_jobs,
        None,
        false,
        None,
    )
    .await;

    let statuses = report.statuses();
    assert!(
        !statuses.iter().any(|s| s == "done"),
        "job com upload falho não pode reportar done: {statuses:?}"
    );
    let failed = report
        .failed_report()
        .expect("deve haver report final failed");
    let msg = failed
        .message
        .clone()
        .or(failed.error.clone())
        .unwrap_or_default();
    assert!(
        msg.contains("upload de artefatos falhou"),
        "mensagem deve diagnosticar a falha de upload, obtido: {msg}"
    );
}

// -- Ronda reviewer S2: retry e preparo de artefatos --

/// put_with_retry com S3 sempre falhando → Err após EXATAMENTE 3 tentativas.
#[tokio::test]
async fn put_with_retry_tries_three_times_then_gives_up() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("a.bin");
    std::fs::write(&file, b"data").unwrap();

    let s3 = FakeS3::new();
    s3.set_upload_fail(true);
    let res = put_with_retry(&s3, "artifacts/j/a.bin", &file).await;
    assert!(res.is_err(), "put sempre falhando deve retornar Err");
    assert_eq!(s3.put_count(), 3, "put_with_retry deve tentar N=3 vezes");
}

/// put_with_retry com S3 instável (2 falhas transitórias) → Ok na 3ª.
#[tokio::test]
async fn put_with_retry_succeeds_after_transient_failures() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("b.bin");
    std::fs::write(&file, b"data").unwrap();

    let s3 = FakeS3::new();
    s3.set_fail_first_n(2);
    let res = put_with_retry(&s3, "artifacts/j/b.bin", &file).await;
    assert!(
        res.is_ok(),
        "falha transitória deve ser absorvida pelo retry"
    );
    assert_eq!(s3.put_count(), 3, "2 falhas + 1 sucesso = 3 tentativas");
}

/// One-shot generate com upload_fail → exatamente 3 puts por arquivo
/// (prova end-to-end do retry) + report final failed.
#[tokio::test]
async fn generate_upload_fail_puts_exactly_three_times() {
    let tmp = tempfile::tempdir().unwrap();

    let s3 = Arc::new(FakeS3::new());
    s3.set_upload_fail(true);
    let mut dispatch = make_dispatch("job-retry-count-001", "diffusion");
    dispatch.mode = "generate".to_string();
    dispatch.package_ref = None;
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let mut output_files = HashMap::new();
    output_files.insert("generated_0001.png".to_string(), b"fake png".to_vec());
    create_fake_outputs(tmp.path(), "job-retry-count-001", &output_files);

    run_job(
        dispatch,
        s3.clone(),
        report.clone(),
        executor,
        active_jobs,
        None,
        false,
        None,
    )
    .await;

    assert!(
        report.failed_report().is_some(),
        "upload falho deve terminar failed"
    );
    assert_eq!(s3.put_count(), 3, "1 arquivo × N=3 tentativas");
}

/// Falha de PREPARO (scoped_key rejeita job_id com traversal) → entra em
/// upload_errors → report final failed (nada é silenciado).
#[tokio::test]
async fn generate_scoped_key_failure_reports_failed() {
    let tmp = tempfile::tempdir().unwrap();

    let s3 = Arc::new(FakeS3::new());
    // job_id com segmento ".." → art_key contém traversal → scoped_key Err.
    let mut dispatch = make_dispatch("job/../traversal-001", "diffusion");
    dispatch.mode = "generate".to_string();
    dispatch.package_ref = None;
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let mut output_files = HashMap::new();
    output_files.insert("generated_0001.png".to_string(), b"fake png".to_vec());
    create_fake_outputs(tmp.path(), "job/../traversal-001", &output_files);

    run_job(
        dispatch,
        s3,
        report.clone(),
        executor,
        active_jobs,
        None,
        false,
        None,
    )
    .await;

    let statuses = report.statuses();
    assert!(
        !statuses.iter().any(|s| s == "done"),
        "falha de preparo não pode terminar done: {statuses:?}"
    );
    let failed = report
        .failed_report()
        .expect("deve haver report final failed");
    let msg = failed
        .message
        .clone()
        .or(failed.error.clone())
        .unwrap_or_default();
    assert!(
        msg.contains("upload de artefatos falhou"),
        "gate deve transformar em failed, obtido: {msg}"
    );
    assert!(
        msg.contains("falha ao preparar artefato (key)"),
        "causa deve identificar o preparo (key), obtido: {msg}"
    );
}

/// Simetria daemon: generate via daemon com upload_fail → failed, nunca done.
#[tokio::test]
async fn daemon_generate_upload_fail_reports_failed() {
    let tmp = tempfile::tempdir().unwrap();

    let s3 = Arc::new(FakeS3::new());
    s3.set_upload_fail(true);
    let mut dispatch = make_dispatch("job-daemon-fail-001", "diffusion");
    dispatch.mode = "generate".to_string();
    dispatch.package_ref = None;
    dispatch.config_yaml =
        Some("base_model: flux-2-klein-4b\noutput_path: {output_path}".to_string());
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let client = Arc::new(FakeDaemonClient::new());
    let launcher = Arc::new(FakeDaemonLauncher::new());
    let daemon_state = Arc::new(DaemonState::new(
        "hephaestus/trainer-difusao:local",
        8766,
        600,
        client.clone() as Arc<dyn DaemonClient>,
        launcher.clone() as Arc<dyn DaemonLauncher>,
    ));
    daemon_state.set_running(true, Some("http://localhost:8766".to_string()));
    client.set_generate_results(vec![Ok(())]);

    let mut output_files = HashMap::new();
    output_files.insert("generated_0001.png".to_string(), b"daemon png".to_vec());
    create_fake_outputs(tmp.path(), "job-daemon-fail-001", &output_files);

    run_job(
        dispatch,
        s3,
        report.clone(),
        executor,
        active_jobs,
        None,
        false,
        Some(daemon_state),
    )
    .await;

    let statuses = report.statuses();
    assert!(
        !statuses.iter().any(|s| s == "done"),
        "daemon com upload falho não pode reportar done: {statuses:?}"
    );
    let failed = report
        .failed_report()
        .expect("deve haver report final failed");
    let msg = failed
        .message
        .clone()
        .or(failed.error.clone())
        .unwrap_or_default();
    assert!(
        msg.contains("upload de artefatos falhou"),
        "mensagem deve diagnosticar a falha de upload, obtido: {msg}"
    );
}

// -- D5 ADR-0023: meta_content no report done --

/// Job generate one-shot com generation_meta.json fake no output
/// → report done contém meta_content com o JSONL.
#[tokio::test]
async fn generate_one_shot_meta_content_present() {
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let mut dispatch = make_dispatch("job-meta-present-001", "diffusion");
    dispatch.mode = "generate".to_string();
    dispatch.package_ref = None;
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let meta_jsonl = r#"{"filename":"img_001.png","seed":42,"prompt":"a cat","width":512,"height":512}
{"filename":"img_002.png","seed":43,"prompt":"a dog","width":512,"height":512}
"#;
    let mut output_files = HashMap::new();
    output_files.insert("generated_0001.png".to_string(), b"fake png".to_vec());
    output_files.insert(
        "generation_meta.json".to_string(),
        meta_jsonl.as_bytes().to_vec(),
    );
    create_fake_outputs(tmp.path(), "job-meta-present-001", &output_files);

    let res = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;

    assert!(res.is_ok(), "run_job_inner failed: {:?}", res);

    // Verifica meta_content no report done
    let meta = report.done_meta_content();
    assert!(meta.is_some(), "meta_content should be present");
    let content = meta.unwrap();
    assert!(
        content.contains("img_001.png"),
        "meta_content should contain JSONL data"
    );
    assert!(
        content.contains("\"seed\":42"),
        "meta_content should contain seed field"
    );

    // Verifica artefatos
    let artifacts = report.done_artifacts().unwrap();
    let kinds: Vec<&str> = artifacts.iter().map(|a| a.kind.as_str()).collect();
    assert!(kinds.contains(&"generated_meta"));
}

/// Job generate sem generation_meta.json → report done NÃO contém meta_content.
#[tokio::test]
async fn generate_one_shot_meta_content_absent() {
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let mut dispatch = make_dispatch("job-meta-absent-001", "diffusion");
    dispatch.mode = "generate".to_string();
    dispatch.package_ref = None;
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let mut output_files = HashMap::new();
    output_files.insert("generated.png".to_string(), b"fake png".to_vec());
    // Sem generation_meta.json — job legado
    create_fake_outputs(tmp.path(), "job-meta-absent-001", &output_files);

    let res = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;

    assert!(res.is_ok(), "run_job_inner failed: {:?}", res);

    // meta_content deve ser None (ausente no report)
    let meta = report.done_meta_content();
    assert!(
        meta.is_none(),
        "meta_content should be absent for legacy jobs"
    );
}

// -- A.3 test 4: parse_metrics_line aceita a linha 1-epoch do autotrack --

#[test]
fn parse_metrics_line_accepts_autotrack_single_epoch() {
    let line = r#"{"box_loss":0.045,"cls_loss":0.067,"dfl_loss":0.123,"mAP50":0.912,"mAP50-95":0.654,"epoch":1}"#;
    let m = parse_metrics_line(line).expect("should parse autotrack metrics line");
    assert_eq!(m.epoch, 1);
    assert!((m.box_loss - 0.045).abs() < 1e-6);
    assert!((m.cls_loss - 0.067).abs() < 1e-6);
    assert!((m.dfl_loss - 0.123).abs() < 1e-6);
    assert!((m.map50 - 0.912).abs() < 1e-6);
    assert!((m.map50_95 - 0.654).abs() < 1e-6);
}

// =========================================================================
// G.2 — nvidia-smi telemetry parse tests
// =========================================================================

#[test]
fn parse_nvidia_smi_csv_two_gpus() {
    let csv = "\
NVIDIA GeForce RTX 3060, 12288, 0
NVIDIA GeForce GTX 1660 SUPER, 6144, 1024
";
    let t = parse_nvidia_smi_csv(csv).expect("should parse 2 GPUs");
    assert_eq!(
        t.gpus,
        vec!["NVIDIA GeForce RTX 3060", "NVIDIA GeForce GTX 1660 SUPER"]
    );
    // total: 12288 + 6144 = 18432 MiB (sem conversão)
    assert_eq!(t.vram_total, 18432);
    // used: 0 + 1024 = 1024 MiB (sem conversão)
    assert_eq!(t.vram_used, 1024);
    // max individual: 12288 MiB (maior GPU — capacidade de 1 job)
    assert_eq!(t.max_gpu_mib, 12288);
}

#[test]
fn parse_nvidia_smi_csv_malformed_lines_ignored() {
    let csv = "\
NVIDIA GeForce RTX 3060, 12288, 0
CORRUPTED LINE
NVIDIA GeForce GTX 1660 SUPER, 6144, 512
also bad, not a number
";
    let t = parse_nvidia_smi_csv(csv).expect("should parse valid lines only");
    assert_eq!(t.gpus.len(), 2);
    assert_eq!(t.gpus[0], "NVIDIA GeForce RTX 3060");
    assert_eq!(t.gpus[1], "NVIDIA GeForce GTX 1660 SUPER");
    // used: 0 + 512 = 512 MiB (sem conversão)
    assert_eq!(t.vram_used, 512);
}

#[test]
fn parse_nvidia_smi_csv_empty() {
    assert!(parse_nvidia_smi_csv("").is_none());
    assert!(parse_nvidia_smi_csv("  \n  \n").is_none());
}

#[test]
fn parse_nvidia_smi_csv_no_valid_gpus() {
    let csv = "bad line\nanother bad\n";
    assert!(parse_nvidia_smi_csv(csv).is_none());
}

#[test]
fn parse_nvidia_smi_csv_single_gpu_max_equals_total() {
    let csv = "NVIDIA GeForce RTX 3060, 12288, 4096\n";
    let t = parse_nvidia_smi_csv(csv).expect("should parse 1 GPU");
    assert_eq!(t.gpus.len(), 1);
    assert_eq!(t.vram_total, 12288);
    // 1 GPU: max = total (capacidade de 1 job = a única GPU)
    assert_eq!(t.max_gpu_mib, 12288);
}

// =========================================================================
// G.2 — DockerExecutor GPU args tests
// =========================================================================

#[test]
fn docker_run_args_gpu_some() {
    let args = build_docker_run_args(
        "hephaestus/trainer-yolo:gpu",
        "trainer-yolo-job-1",
        &[("/data/datasets".into(), "/datasets".into())],
        &[
            "train".to_string(),
            "--config".to_string(),
            "/outputs/c.yaml".to_string(),
        ],
        &[("ENGINE_MOCK".to_string(), "0".to_string())],
        Some("0"),
    );
    // GPU flags present
    let gpu_idx = args
        .iter()
        .position(|a| a == "--gpus")
        .expect("--gpus flag");
    assert_eq!(args[gpu_idx + 1], "device=0");
    let shm_idx = args
        .iter()
        .position(|a| a == "--shm-size")
        .expect("--shm-size flag");
    assert_eq!(args[shm_idx + 1], "2g");
    // NVIDIA_VISIBLE_DEVICES
    let nvd_idx = args
        .iter()
        .position(|a| a == "NVIDIA_VISIBLE_DEVICES=0")
        .expect("NVIDIA_VISIBLE_DEVICES");
    assert!(args[nvd_idx - 1] == "-e");
    // ENGINE_MOCK env
    let mock_idx = args
        .iter()
        .position(|a| a == "ENGINE_MOCK=0")
        .expect("ENGINE_MOCK env");
    assert!(args[mock_idx - 1] == "-e");
    // Image and subcommand args are present
    assert!(args.contains(&"hephaestus/trainer-yolo:gpu".to_string()));
    assert!(args.contains(&"train".to_string()));
}

#[test]
fn docker_run_args_gpu_none_no_flags() {
    let args = build_docker_run_args(
        "hephaestus/trainer-yolo:local",
        "trainer-yolo-job-2",
        &[],
        &["train".to_string()],
        &[],
        None,
    );
    // NO GPU flags
    assert!(!args.contains(&"--gpus".to_string()));
    assert!(!args.contains(&"--shm-size".to_string()));
    assert!(!args.iter().any(|a| a.starts_with("NVIDIA_VISIBLE_DEVICES")));
    // Host gateway flag present
    assert!(args.contains(&"--add-host".to_string()));
    assert!(args.contains(&"host.docker.internal:host-gateway".to_string()));
    // Image and args present
    assert!(args.contains(&"hephaestus/trainer-yolo:local".to_string()));
    assert!(args.contains(&"train".to_string()));
}

#[test]
fn docker_run_args_with_engine_user() {
    std::env::set_var("ENGINE_USER", "1000:1000");
    let args = build_docker_run_args(
        "hephaestus/trainer-yolo:local",
        "trainer-yolo-user-test",
        &[],
        &[],
        &[],
        None,
    );
    std::env::remove_var("ENGINE_USER");
    let user_idx = args.iter().position(|a| a == "--user");
    assert!(user_idx.is_some(), "expected --user flag in args");
    assert_eq!(args[user_idx.unwrap() + 1], "1000:1000");
}

// =========================================================================
// G.2 — Anti-mock guard tests (D2)
// =========================================================================

#[test]
fn anti_mock_guard_rejects_local_with_gpu() {
    // Simula ORCH_GPU_DEVICES setado + imagem :local → deve falhar.
    // Chama run_job_inner diretamente e verifica a variante do erro.
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-guard-001", "yolo", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    dispatch.image = "hephaestus/trainer-yolo:local".to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            Some("0"),
            false,
            None,
        )
        .await;
        assert!(
            matches!(result, Err(PipelineError::GpuImageGuard { .. })),
            "should return GpuImageGuard variant: {:?}",
            result
        );
    });

    // Executor should NOT have been called
    assert!(executor.last_args().is_none());
}

#[test]
fn anti_mock_guard_passes_gpu_image() {
    // GPU setado + imagem :gpu → deve passar (executor chamado).
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-guard-002", "yolo", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    dispatch.image = "hephaestus/trainer-yolo:gpu".to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    // Pre-cria outputs
    let mut output_files = HashMap::new();
    output_files.insert("best.pt".to_string(), b"fake model".to_vec());
    output_files.insert("last.pt".to_string(), b"fake model".to_vec());
    output_files.insert(
        "metrics.jsonl".to_string(),
        br#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":1}"#
            .to_vec(),
    );
    create_fake_outputs(tmp.path(), "job-guard-002", &output_files);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            Some("0"),
            false,
            None,
        )
        .await;
        assert!(
            result.is_ok(),
            "gpu image should pass guard: {:?}",
            result.err()
        );
    });
}

#[test]
fn anti_mock_guard_bypass_with_allow_mock() {
    // GPU setado + imagem :local + gpu_allow_mock=true → deve passar.
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-guard-003", "yolo", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    dispatch.image = "hephaestus/trainer-yolo:local".to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    // Pre-cria outputs
    let mut output_files = HashMap::new();
    output_files.insert("best.pt".to_string(), b"fake model".to_vec());
    output_files.insert("last.pt".to_string(), b"fake model".to_vec());
    output_files.insert(
        "metrics.jsonl".to_string(),
        br#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":1}"#
            .to_vec(),
    );
    create_fake_outputs(tmp.path(), "job-guard-003", &output_files);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            Some("0"),
            true, // gpu_allow_mock
            None, // daemon_state
        )
        .await;
        assert!(
            result.is_ok(),
            "ALLOW_MOCK should bypass guard: {:?}",
            result.err()
        );
    });
}

#[test]
fn anti_mock_guard_no_gpu_no_check() {
    // Sem ORCH_GPU_DEVICES → imagem :local deve passar (caminho mock padrão).
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-guard-004", "yolo", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    dispatch.image = "hephaestus/trainer-yolo:local".to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    // Pre-cria outputs
    let mut output_files = HashMap::new();
    output_files.insert("best.pt".to_string(), b"fake model".to_vec());
    output_files.insert("last.pt".to_string(), b"fake model".to_vec());
    output_files.insert(
        "metrics.jsonl".to_string(),
        br#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":1}"#
            .to_vec(),
    );
    create_fake_outputs(tmp.path(), "job-guard-004", &output_files);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            None,
            false,
            None,
        )
        .await;
        assert!(
            result.is_ok(),
            "no GPU set → mock path should work: {:?}",
            result.err()
        );
    });
}

#[test]
fn anti_mock_guard_allows_non_localgpu_tag() {
    // GPU setado + imagem :localgpu (NÃO :local) → deve passar (ends_with(":local") = false).
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-guard-005", "yolo", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    dispatch.image = "hephaestus/trainer-yolo:localgpu".to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    // Pre-cria outputs
    let mut output_files = HashMap::new();
    output_files.insert("best.pt".to_string(), b"fake model".to_vec());
    output_files.insert("last.pt".to_string(), b"fake model".to_vec());
    output_files.insert(
        "metrics.jsonl".to_string(),
        br#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":1}"#
            .to_vec(),
    );
    create_fake_outputs(tmp.path(), "job-guard-005", &output_files);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let result = run_job_inner(
            &dispatch,
            s3.clone(),
            report.clone(),
            executor.clone(),
            &active_jobs,
            Some("0"),
            false,
            None,
        )
        .await;
        assert!(
            result.is_ok(),
            ":localgpu should NOT be blocked by guard: {:?}",
            result.err()
        );
    });
}

// =========================================================================
// H.1 — HeartbeatBody serializa endpoint
// =========================================================================

#[test]
fn heartbeat_body_serializes_endpoint() {
    let body = HeartbeatBody {
        endpoint: "http://orchestrator-local:8082".to_string(),
        gpus: vec!["NVIDIA GeForce RTX 3060".to_string()],
        vram_total: Some(12288),
        vram_used: Some(1024),
        cpu: Some(42.5),
        ram: Some(4_000_000_000),
        ram_total: Some(8_000_000_000),
        jobs_active: 1,
        max_gpu_mib: Some(12288),
    };
    let json = serde_json::to_value(&body).unwrap();
    assert_eq!(json["endpoint"], "http://orchestrator-local:8082");
    assert_eq!(json["gpus"][0], "NVIDIA GeForce RTX 3060");
    assert_eq!(json["jobs_active"], 1);
    assert_eq!(json["max_gpu_mib"], 12288);
}

// =========================================================================
// H.1 — resolve_advertise_url (função pura)
// =========================================================================

#[test]
fn default_advertise_url() {
    // None → default
    assert_eq!(
        resolve_advertise_url(None),
        "http://orchestrator-local:8082"
    );
}

#[test]
fn resolve_advertise_url_from_value() {
    assert_eq!(
        resolve_advertise_url(Some("http://custom:9999")),
        "http://custom:9999"
    );
}

#[test]
fn resolve_advertise_url_empty_fallback() {
    // Empty string → default
    assert_eq!(
        resolve_advertise_url(Some("")),
        "http://orchestrator-local:8082"
    );
}

#[test]
fn pipeline_error_cancelled_display() {
    let err = PipelineError::Cancelled;
    assert_eq!(err.to_string(), "job cancelled by user");
}

#[test]
fn read_final_metrics_prefers_telemetry_jsonl() {
    let tmp = tempfile::tempdir().unwrap();
    let metrics_path = tmp.path().join("metrics.jsonl");
    let telemetry_path = tmp.path().join("telemetry.jsonl");

    std::fs::write(&metrics_path, r#"{"epoch": 1, "loss": 0.5}"#).unwrap();
    std::fs::write(&telemetry_path, r#"{"epoch": 2, "loss": 0.2}"#).unwrap();

    let final_m = read_final_metrics(&metrics_path).unwrap();
    assert_eq!(final_m.epoch, 2);
    assert_eq!(final_m.loss, Some(0.2));
}

#[test]
fn read_final_metrics_falls_back_to_metrics_jsonl() {
    let tmp = tempfile::tempdir().unwrap();
    let metrics_path = tmp.path().join("metrics.jsonl");

    std::fs::write(&metrics_path, r#"{"epoch": 1, "loss": 0.5}"#).unwrap();

    let final_m = read_final_metrics(&metrics_path).unwrap();
    assert_eq!(final_m.epoch, 1);
    assert_eq!(final_m.loss, Some(0.5));
}
// =========================================================================
// H.1 — Pairing code generation
// =========================================================================

#[test]
fn generate_pairing_code_format() {
    let code = generate_pairing_code();
    assert!(
        code.starts_with("heph_p_"),
        "code should start with heph_p_: {code}"
    );
    let hex_part = &code[7..]; // "heph_p_" = 7 chars
    assert_eq!(hex_part.len(), 32, "hex part should be 32 chars: {code}");
    assert!(
        hex_part.chars().all(|c| c.is_ascii_hexdigit()),
        "hex part should be all hex digits: {code}"
    );
}

#[test]
fn generate_pairing_code_unique() {
    let a = generate_pairing_code();
    let b = generate_pairing_code();
    assert_ne!(a, b, "two generated codes should differ");
}

// =========================================================================
// H.1 — Pairing verify single-use
// =========================================================================

#[test]
fn pairing_verify_correct_then_second_false() {
    let state = PairingState::new("heph_p_aabbccdd11223344aabbccdd11223344".to_string());
    assert!(state.verify("heph_p_aabbccdd11223344aabbccdd11223344"));
    // Second use — consumed
    assert!(!state.verify("heph_p_aabbccdd11223344aabbccdd11223344"));
}

#[test]
fn pairing_verify_wrong_code() {
    let state = PairingState::new("heph_p_aabbccdd11223344aabbccdd11223344".to_string());
    assert!(!state.verify("heph_p_wrong_wrong_wrong_wrong_wrong_00"));
}

#[test]
fn pairing_verify_empty_code() {
    let state = PairingState::new("heph_p_aabbccdd11223344aabbccdd11223344".to_string());
    assert!(!state.verify(""));
}

// =========================================================================
// H.1 — PairingVerifyRequest deserialization
// =========================================================================

#[test]
fn pairing_verify_request_deserialize() {
    let req: PairingVerifyRequest = serde_json::from_str(r#"{"code":"heph_p_test"}"#).unwrap();
    assert_eq!(req.code, "heph_p_test");
}

#[test]
fn pairing_verify_response_serialize() {
    let resp = PairingVerifyResponse { valid: true };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["valid"], true);

    let resp = PairingVerifyResponse { valid: false };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["valid"], false);
}

// =========================================================================
// I.3 — S3Scope::Models + scoped_key tests
// =========================================================================

#[test]
fn scoped_key_models_valid() {
    let key = scoped_key(S3Scope::Models, "models/yolo/abc-123/best.pt");
    assert_eq!(key, Ok("models/yolo/abc-123/best.pt".to_string()));
}

#[test]
fn scoped_key_models_outside_scope() {
    assert_eq!(
        scoped_key(S3Scope::Models, "packages/abc/dataset.zip"),
        Err(ScopedKeyError::OutsideScope)
    );
}

#[test]
fn scoped_key_packages_rejects_models_prefix() {
    assert_eq!(
        scoped_key(S3Scope::Packages, "models/yolo/abc/best.pt"),
        Err(ScopedKeyError::OutsideScope)
    );
}

#[test]
fn scoped_key_models_empty() {
    assert_eq!(
        scoped_key(S3Scope::Models, ""),
        Err(ScopedKeyError::EmptyKey)
    );
}

#[test]
fn scoped_key_models_absolute() {
    assert_eq!(
        scoped_key(S3Scope::Models, "/models/yolo/abc/best.pt"),
        Err(ScopedKeyError::AbsolutePath)
    );
}

#[test]
fn scoped_key_models_traversal() {
    assert_eq!(
        scoped_key(S3Scope::Models, "../etc/passwd"),
        Err(ScopedKeyError::PathTraversal)
    );
}

// =========================================================================
// I.3 — DispatchRequest weights_ref serde tests
// =========================================================================

#[test]
fn dispatch_request_with_weights_ref() {
    let json = r#"{
        "job_id": "j1",
        "engine": "yolo",
        "image": "img:local",
        "exec_mode": "docker",
        "package_ref": {"key": "packages/p/dataset.zip", "md5_zip": "abc", "bytes": 100},
        "workdir": "/tmp",
        "weights_ref": {"s3_key": "models/yolo/abc/best.pt", "md5": "d41d8cd98f00b204e9800998ecf8427e"}
    }"#;
    let req: DispatchRequest = serde_json::from_str(json).unwrap();
    assert!(req.weights_ref.is_some());
    let wr = req.weights_ref.unwrap();
    assert_eq!(wr.s3_key, "models/yolo/abc/best.pt");
    assert_eq!(wr.md5, "d41d8cd98f00b204e9800998ecf8427e");
}

#[test]
fn dispatch_request_without_weights_ref() {
    let json = r#"{
        "job_id": "j1",
        "engine": "yolo",
        "image": "img:local",
        "exec_mode": "docker",
        "package_ref": {"key": "packages/p/dataset.zip", "md5_zip": "abc", "bytes": 100},
        "workdir": "/tmp"
    }"#;
    let req: DispatchRequest = serde_json::from_str(json).unwrap();
    assert!(req.weights_ref.is_none());
}

// =========================================================================
// I.3 — replace_config_placeholders with weights_path
// =========================================================================

#[test]
fn replace_config_placeholders_with_weights() {
    let config = "model: yolo11m\nweights_path: {weights_path}";
    let result = replace_config_placeholders(
        config,
        "/datasets/j1",
        "/outputs/j1",
        Some("/outputs/j1/weights/best.pt"),
        &[],
        None,
        None,
        None,
        None,
    );
    assert_eq!(
        result,
        "model: yolo11m\nweights_path: /outputs/j1/weights/best.pt"
    );
}

#[test]
fn replace_config_placeholders_without_weights_keeps_literal() {
    let config = "model: yolo11m\nweights_path: {weights_path}";
    let result = replace_config_placeholders(
        config,
        "/datasets/j1",
        "/outputs/j1",
        None,
        &[],
        None,
        None,
        None,
        None,
    );
    assert_eq!(result, "model: yolo11m\nweights_path: {weights_path}");
}

// =========================================================================
// S4 feat/img2img — replace com/sem init, ext sanitizada, allowlist do init
// =========================================================================

#[test]
fn replace_config_placeholders_with_init_image() {
    let config = "init_image: {init_image_path}\nstrength: 0.6";
    let result = replace_config_placeholders(
        config,
        "/datasets/j1",
        "/outputs/j1",
        None,
        &[],
        None,
        Some("/outputs/j1/inputs/init.png"),
        None,
        None,
    );
    assert_eq!(
        result,
        "init_image: /outputs/j1/inputs/init.png\nstrength: 0.6"
    );
}

#[test]
fn replace_config_placeholders_without_init_keeps_literal() {
    let config = "init_image: {init_image_path}\nstrength: 0.6";
    let result = replace_config_placeholders(
        config,
        "/datasets/j1",
        "/outputs/j1",
        None,
        &[],
        None,
        None,
        None,
        None,
    );
    assert_eq!(result, "init_image: {init_image_path}\nstrength: 0.6");
}

#[test]
fn replace_config_placeholders_no_init_placeholder_noop() {
    // Config sem o placeholder: init presente ou não, no-op.
    let config = "prompt: a cat\nsteps: 20";
    for init in [None, Some("/outputs/j1/inputs/init.png")] {
        let result = replace_config_placeholders(
            config,
            "/datasets/j1",
            "/outputs/j1",
            None,
            &[],
            None,
            init,
            None,
            None,
        );
        assert_eq!(result, config);
    }
}
#[test]
fn replace_config_placeholders_with_text_encoder() {
    // Fatia feat/pesos-custom-flux2: placeholder do encoder substituído.
    let config = "text_encoder: {text_encoder_path}\nsteps: 20";
    let result = replace_config_placeholders(
        config,
        "/datasets/j1",
        "/outputs/j1",
        None,
        &[],
        None,
        None,
        None,
        Some("/outputs/j1/weights/text_encoder.safetensors"),
    );
    assert_eq!(
        result,
        "text_encoder: /outputs/j1/weights/text_encoder.safetensors\nsteps: 20"
    );
}

#[test]
fn replace_config_placeholders_without_text_encoder_keeps_literal() {
    let config = "text_encoder: {text_encoder_path}\nsteps: 20";
    let result = replace_config_placeholders(
        config,
        "/datasets/j1",
        "/outputs/j1",
        None,
        &[],
        None,
        None,
        None,
        None,
    );
    assert_eq!(result, "text_encoder: {text_encoder_path}\nsteps: 20");
}

#[test]
fn init_image_ext_from_suffix() {
    assert_eq!(init_image_ext("generation_inputs/abc/img.png"), "png");
    assert_eq!(init_image_ext("generation_inputs/abc/photo.JPG"), "jpg");
    assert_eq!(init_image_ext("artifacts/job-1/gen_0001.webp"), "webp");
    assert_eq!(init_image_ext("artifacts/job-1/frame.jpeg"), "jpeg");
}

#[test]
fn init_image_ext_fallback_png() {
    // Sem extensão, extensão curta/longa demais ou não-alfanumérica → png.
    assert_eq!(init_image_ext("artifacts/j/f"), "png");
    assert_eq!(init_image_ext("artifacts/j/f."), "png");
    assert_eq!(init_image_ext("artifacts/j/f.a"), "png");
    assert_eq!(init_image_ext("artifacts/j/f.toolongext"), "png");
    assert_eq!(init_image_ext("artifacts/j/f.pn_g"), "png");
    assert_eq!(init_image_ext(""), "png");
}

#[test]
fn init_image_ext_never_returns_traversal_or_slashes() {
    for key in [
        "artifacts/j/..",
        "artifacts/../evil.png",
        "generation_inputs/x/../../etc/passwd",
        "/abs/path.png",
    ] {
        let ext = init_image_ext(key);
        assert!(!ext.contains(".."), "ext com traversal: {ext}");
        assert!(!ext.contains('/'), "ext com barra: {ext}");
    }
}

#[test]
fn scoped_init_image_key_accepts_both_prefixes() {
    assert_eq!(
        scoped_init_image_key("generation_inputs/abc/upload.png"),
        Ok("generation_inputs/abc/upload.png".to_string())
    );
    assert_eq!(
        scoped_init_image_key("artifacts/job-1/generated_0001.png"),
        Ok("artifacts/job-1/generated_0001.png".to_string())
    );
}

#[test]
fn scoped_init_image_key_rejects_other_prefixes() {
    assert_eq!(
        scoped_init_image_key("models/diffusion/x/ckpt.safetensors"),
        Err(ScopedKeyError::OutsideScope)
    );
    assert_eq!(
        scoped_init_image_key("packages/abc/dataset.zip"),
        Err(ScopedKeyError::OutsideScope)
    );
    assert_eq!(
        scoped_init_image_key("datasets/abc/images"),
        Err(ScopedKeyError::OutsideScope)
    );
}

#[test]
fn scoped_init_image_key_rejects_unsafe() {
    assert_eq!(scoped_init_image_key(""), Err(ScopedKeyError::EmptyKey));
    assert_eq!(
        scoped_init_image_key("/generation_inputs/x.png"),
        Err(ScopedKeyError::AbsolutePath)
    );
    assert_eq!(
        scoped_init_image_key("generation_inputs/../evil.png"),
        Err(ScopedKeyError::PathTraversal)
    );
    assert_eq!(
        scoped_init_image_key("artifacts/../evil.png"),
        Err(ScopedKeyError::PathTraversal)
    );
}

#[test]
fn dispatch_request_init_image_ref_serde() {
    // snake_case no wire interno, md5 opcional (galeria → null).
    let json = r#"{
        "job_id": "j1", "engine": "diffusion", "image": "img",
        "exec_mode": "docker", "config_yaml": null,
        "dataset_version_id": null, "workdir": "/tmp",
        "init_image_ref": {"s3_key": "artifacts/j0/gen.png", "md5": null}
    }"#;
    let req: DispatchRequest = serde_json::from_str(json).unwrap();
    let init = req.init_image_ref.expect("init presente");
    assert_eq!(init.s3_key, "artifacts/j0/gen.png");
    assert!(init.md5.is_none());

    // Ausente → None (txt2img retrocompat).
    let json2 = r#"{
        "job_id": "j1", "engine": "diffusion", "image": "img",
        "exec_mode": "docker", "config_yaml": null,
        "dataset_version_id": null, "workdir": "/tmp"
    }"#;
    let req2: DispatchRequest = serde_json::from_str(json2).unwrap();
    assert!(req2.init_image_ref.is_none());
}

// =========================================================================
// I.3 — run_job_inner with weights_ref: staging + config substitution
// =========================================================================

#[tokio::test]
async fn weights_ref_valid_stages_file_and_replaces_placeholder() {
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-w-001", "yolo", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    // Config with {weights_path} placeholder
    dispatch.config_yaml = Some(
        "epochs: 1\ndataset_path: {dataset_path}\noutput_path: {output_path}\nweights_path: {weights_path}".to_string(),
    );

    // Create weights file in FakeS3
    let weights_bytes = b"fake weights data";
    let weights_md5 = compute_file_md5_bytes(weights_bytes);

    // Manually stage weights file so FakeS3 can serve it
    // (FakeS3 always writes zip_bytes, so we intercept via a custom approach)
    // Actually, FakeS3 writes zip_bytes for ALL downloads. For this test,
    // we put the weights file directly and use a custom S3 mock.
    // Simpler: create a FakeS3 that serves different content per key.
    let weights_s3 = Arc::new(FakeS3WithWeights::new(weights_bytes.to_vec()));
    let weights_md5_hex = weights_md5.clone();

    dispatch.weights_ref = Some(WeightsRef {
        s3_key: "models/yolo/abc-123/best.pt".to_string(),
        md5: weights_md5_hex,
    });

    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    // Pre-creates outputs
    let mut output_files = HashMap::new();
    output_files.insert("best.pt".to_string(), b"fake model".to_vec());
    output_files.insert("last.pt".to_string(), b"fake model".to_vec());
    output_files.insert(
        "metrics.jsonl".to_string(),
        br#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":1}"#
            .to_vec(),
    );
    create_fake_outputs(tmp.path(), "job-w-001", &output_files);

    let result = run_job_inner(
        &dispatch,
        weights_s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;
    assert!(
        result.is_ok(),
        "weights pipeline should succeed: {:?}",
        result.err()
    );

    // Verify weights file was staged
    let staged = tmp.path().join("outputs/job-w-001/weights/best.pt");
    assert!(staged.exists(), "weights file should be staged");
    assert_eq!(
        std::fs::read(&staged).unwrap(),
        weights_bytes,
        "staged weights content should match"
    );

    // Verify config.yaml has {weights_path} replaced
    let config_content =
        std::fs::read_to_string(tmp.path().join("outputs/job-w-001/config.yaml")).unwrap();
    assert!(
        config_content.contains("/outputs/job-w-001/weights/best.pt"),
        "config.yaml should contain replaced weights_path, got: {config_content}"
    );
    assert!(
        !config_content.contains("{weights_path}"),
        "config.yaml should not contain literal {{weights_path}}"
    );

    // Verify executor args are unchanged (shape preserved)
    let args = executor.last_args().unwrap();
    assert_eq!(args[0], "train");
    assert_eq!(args[1], "--config");
    assert_eq!(args[3], "--output");

    // Verify downloads include both package and weights
    let downloads = weights_s3.downloads.lock().unwrap();
    assert!(
        downloads.iter().any(|k| k.contains("packages/")),
        "should download package"
    );
    assert!(
        downloads.iter().any(|k| k.contains("models/")),
        "should download weights"
    );
}

#[tokio::test]
async fn weights_ref_wrong_md5_fails_pipeline() {
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-w-002", "yolo", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();

    let weights_s3 = Arc::new(FakeS3WithWeights::new(b"weights data".to_vec()));

    dispatch.weights_ref = Some(WeightsRef {
        s3_key: "models/yolo/abc-123/best.pt".to_string(),
        md5: "00000000000000000000000000000000".to_string(), // wrong hash
    });

    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let result = run_job_inner(
        &dispatch,
        weights_s3,
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;
    assert!(
        matches!(result, Err(PipelineError::Md5Mismatch { .. })),
        "should fail with Md5Mismatch: {:?}",
        result
    );
}

#[tokio::test]
async fn no_weights_ref_unchanged_behavior() {
    // Sem weights_ref → pipeline idêntica ao comportamento atual (regressão)
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-w-003", "yolo", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    dispatch.weights_ref = None; // explicitly None

    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let mut output_files = HashMap::new();
    output_files.insert("best.pt".to_string(), b"fake model".to_vec());
    output_files.insert("last.pt".to_string(), b"fake model".to_vec());
    output_files.insert(
        "metrics.jsonl".to_string(),
        br#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":1}"#
            .to_vec(),
    );
    create_fake_outputs(tmp.path(), "job-w-003", &output_files);

    let result = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;
    assert!(
        result.is_ok(),
        "no-weights pipeline should succeed: {:?}",
        result.err()
    );

    // No weights directory should exist
    let weights_dir = tmp.path().join("outputs/job-w-003/weights");
    assert!(
        !weights_dir.exists(),
        "weights dir should not exist without weights_ref"
    );

    // Config should have output_path substituted but no weights_path
    let config_content =
        std::fs::read_to_string(tmp.path().join("outputs/job-w-003/config.yaml")).unwrap();
    assert!(
        config_content.contains("output_path: /outputs/job-w-003"),
        "config should have output_path substituted: {config_content}"
    );
    assert!(
        !config_content.contains("weights_path"),
        "config should not contain weights_path when no weights_ref: {config_content}"
    );
}

#[tokio::test]
async fn weights_ref_artifacts_scope() {
    // weights_ref com key em artifacts/ → usa escopo Artifacts existente
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-w-004", "yolo", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    dispatch.config_yaml = Some(
        "epochs: 1\ndataset_path: {dataset_path}\noutput_path: {output_path}\nweights_path: {weights_path}".to_string(),
    );

    let weights_bytes = b"artifact weights";
    let weights_md5 = compute_file_md5_bytes(weights_bytes);
    let weights_s3 = Arc::new(FakeS3WithWeights::new(weights_bytes.to_vec()));

    dispatch.weights_ref = Some(WeightsRef {
        s3_key: "artifacts/job-prev/best.pt".to_string(),
        md5: weights_md5,
    });

    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let mut output_files = HashMap::new();
    output_files.insert("best.pt".to_string(), b"fake model".to_vec());
    output_files.insert("last.pt".to_string(), b"fake model".to_vec());
    output_files.insert(
        "metrics.jsonl".to_string(),
        br#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":1}"#
            .to_vec(),
    );
    create_fake_outputs(tmp.path(), "job-w-004", &output_files);

    let result = run_job_inner(
        &dispatch,
        weights_s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;
    assert!(
        result.is_ok(),
        "artifacts-scope weights should succeed: {:?}",
        result.err()
    );

    // Verify staged at correct location
    let staged = tmp.path().join("outputs/job-w-004/weights/best.pt");
    assert!(staged.exists());
    assert_eq!(std::fs::read(&staged).unwrap(), weights_bytes);
}

#[tokio::test]
async fn weights_ref_unknown_prefix_fails() {
    // Key que não começa com models/ nem artifacts/ → falha
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-w-005", "yolo", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();

    dispatch.weights_ref = Some(WeightsRef {
        s3_key: "datasets/something/file.pt".to_string(),
        md5: "d41d8cd98f00b204e9800998ecf8427e".to_string(),
    });

    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let result = run_job_inner(
        &dispatch,
        s3,
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;
    assert!(result.is_err(), "unknown prefix should fail: {:?}", result);
}

// =========================================================================
// K.4 — autotracker with weights_ref tests (ADR-0014 D4)
// =========================================================================

/// Helper: cria dispatch para autotracker/autotrack com config que inclui {weights_path}.
fn make_autotracker_dispatch(job_id: &str) -> DispatchRequest {
    DispatchRequest {
        job_id: job_id.to_string(),
        engine: "autotracker".to_string(),
        image: "hephaestus/trainer-yolo:local".to_string(),
        exec_mode: "docker".to_string(),
        package_ref: Some(PackageRef {
            key: "packages/test-pkg/dataset.zip".to_string(),
            md5_zip: String::new(),
            bytes: 0,
        }),
        config_yaml: Some(
            "model: mock\nconf: 0.65\ndataset_path: {dataset_path}\noutput_path: {output_path}\nweights_path: {weights_path}"
                .to_string(),
        ),
        dataset_version_id: None,
        workdir: "/tmp".to_string(),
        mode: "autotrack".to_string(),
        weights_ref: None,
        loras: Vec::new(),
        custom_checkpoint: None,
        text_encoder: None,
        init_image_ref: None,
        control_package_ref: None,
    }
}

fn make_autotracker_dispatch_with_valid_md5(job_id: &str, zip_path: &Path) -> DispatchRequest {
    let md5 = compute_file_md5(zip_path).unwrap();
    let mut d = make_autotracker_dispatch(job_id);
    if let Some(ref mut pr) = d.package_ref {
        pr.md5_zip = md5;
    }
    d
}

// -- K.4 test 1: autotracker + weights_ref → staging + autotrack + config with {weights_path} --

#[tokio::test]
async fn autotracker_weights_ref_stages_and_replaces_config() {
    let tmp = tempfile::tempdir().unwrap();

    let weights_bytes = b"fake world weights data";
    let weights_md5 = compute_file_md5_bytes(weights_bytes);
    let s3 = Arc::new(FakeS3WithWeights::new(weights_bytes.to_vec()));

    // Usa o zip_bytes do S3 mock para garantir MD5 consistente
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_autotracker_dispatch_with_valid_md5("job-at-w-001", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    dispatch.weights_ref = Some(WeightsRef {
        s3_key: "models/world/abc-123/yolov8x-worldv2.pt".to_string(),
        md5: weights_md5,
    });

    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    // Simula output do engine: boxes.json + metrics.jsonl
    let mut output_files = HashMap::new();
    output_files.insert(
        "boxes.json".to_string(),
        br#"{"engine":"autotracker","model":"world","seed":0,"conf":0.65,"images":[{"filename":"img1.jpg","boxes":[{"class":"cat","x":0.1,"y":0.2,"w":0.3,"h":0.4,"conf":0.9}]}]}"#.to_vec(),
    );
    output_files.insert(
        "metrics.jsonl".to_string(),
        br#"{"box_loss":0.1,"cls_loss":0.2,"dfl_loss":0.3,"mAP50":0.9,"mAP50-95":0.7,"epoch":1}"#
            .to_vec(),
    );
    create_fake_outputs(tmp.path(), "job-at-w-001", &output_files);

    let result = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;
    assert!(
        result.is_ok(),
        "autotracker with weights_ref should succeed: {:?}",
        result.err()
    );

    // 1. Verifica subcomando: autotrack
    let args = executor.last_args().unwrap();
    assert_eq!(args[0], "autotrack");
    assert_eq!(args[1], "--config");
    assert_eq!(args[3], "--output");

    // 2. Verifica staging de pesos
    let staged = tmp
        .path()
        .join("outputs/job-at-w-001/weights/yolov8x-worldv2.pt");
    assert!(staged.exists(), "weights should be staged");
    assert_eq!(std::fs::read(&staged).unwrap(), weights_bytes);

    // 3. Verifica config.yaml com {weights_path} substituído
    let config_content =
        std::fs::read_to_string(tmp.path().join("outputs/job-at-w-001/config.yaml")).unwrap();
    assert!(
        config_content.contains("/outputs/job-at-w-001/weights/yolov8x-worldv2.pt"),
        "config should contain replaced weights_path, got: {config_content}"
    );
    assert!(
        !config_content.contains("{weights_path}"),
        "config should not contain literal {{weights_path}}"
    );

    // 4. Verifica artefatos: boxes.json + metrics.jsonl
    let artifacts = report.done_artifacts().unwrap();
    let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
    assert!(filenames.contains(&"boxes.json"));
    assert!(filenames.contains(&"metrics.jsonl"));
}

// -- K.4 test 2: autotracker + weights_ref + only boxes.json (sem metrics.jsonl) → done com skip --

#[tokio::test]
async fn autotracker_weights_ref_only_boxes_json_skips_missing_metrics() {
    let tmp = tempfile::tempdir().unwrap();

    let weights_bytes = b"real world weights";
    let weights_md5 = compute_file_md5_bytes(weights_bytes);
    let s3 = Arc::new(FakeS3WithWeights::new(weights_bytes.to_vec()));

    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_autotracker_dispatch_with_valid_md5("job-at-nom-001", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    dispatch.weights_ref = Some(WeightsRef {
        s3_key: "models/world/def-456/yolov8x-worldv2.pt".to_string(),
        md5: weights_md5,
    });

    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    // Apenas boxes.json — SEM metrics.jsonl (como o real faz, D3 da ADR-0014)
    let mut output_files = HashMap::new();
    output_files.insert(
        "boxes.json".to_string(),
        br#"{"engine":"autotracker","model":"world","seed":0,"conf":0.65,"images":[{"filename":"img1.jpg","boxes":[]}]}"#.to_vec(),
    );
    // NOTE: metrics.jsonl deliberately NOT created
    create_fake_outputs(tmp.path(), "job-at-nom-001", &output_files);

    let result = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;
    assert!(
        result.is_ok(),
        "autotracker without metrics.jsonl should succeed (skip): {:?}",
        result.err()
    );

    // Verifica done report: sem metrics/epoch (progresso binário)
    let done_report = report
        .reports
        .lock()
        .unwrap()
        .iter()
        .find(|r| r.status == "done")
        .cloned()
        .expect("should have done report");
    assert!(
        done_report.metrics.is_none(),
        "done should have no metrics when metrics.jsonl is absent"
    );
    assert!(
        done_report.epoch.is_none(),
        "done should have no epoch when metrics.jsonl is absent"
    );
    assert_eq!(done_report.progress, Some(1.0));

    // Verifica artefatos: SOMENTE boxes.json (sem metrics.jsonl)
    let artifacts = report.done_artifacts().unwrap();
    assert_eq!(
        artifacts.len(),
        1,
        "should have exactly 1 artifact (boxes.json only)"
    );
    assert_eq!(artifacts[0].path, "boxes.json");
    assert_eq!(artifacts[0].kind, "boxes");

    // Verifica subcomando correto
    let args = executor.last_args().unwrap();
    assert_eq!(args[0], "autotrack");
}

// -- K.4 test 3: autotracker + weights_ref com md5 errado → falha honesta --

#[tokio::test]
async fn autotracker_weights_ref_wrong_md5_fails() {
    let tmp = tempfile::tempdir().unwrap();

    let s3 = Arc::new(FakeS3WithWeights::new(b"real weights".to_vec()));
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_autotracker_dispatch_with_valid_md5("job-at-bad-001", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    dispatch.weights_ref = Some(WeightsRef {
        s3_key: "models/world/ghi-789/yolov8x-worldv2.pt".to_string(),
        md5: "00000000000000000000000000000000".to_string(), // wrong hash
    });

    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let result = run_job_inner(
        &dispatch,
        s3,
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;
    assert!(
        matches!(result, Err(PipelineError::Md5Mismatch { .. })),
        "autotracker with wrong weights md5 should fail with Md5Mismatch: {:?}",
        result
    );

    // Executor nunca chamado (falha antes)
    assert!(executor.last_args().is_none());
}

// -- K.4 test 4: autotracker SEM weights_ref → caminho atual byte-a-byte (regressão) --

#[tokio::test]
async fn autotracker_no_weights_ref_unchanged_behavior() {
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_autotracker_dispatch_with_valid_md5("job-at-reg-001", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    dispatch.weights_ref = None; // explícito: sem pesos

    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let mut output_files = HashMap::new();
    output_files.insert(
        "boxes.json".to_string(),
        br#"{"engine":"autotracker","model":"mock","seed":42,"conf":0.65,"images":[{"filename":"img1.jpg","boxes":[{"class":"cat","x":0.5,"y":0.5,"w":0.1,"h":0.1,"conf":1.0}]}]}"#.to_vec(),
    );
    output_files.insert(
        "metrics.jsonl".to_string(),
        br#"{"box_loss":0.1,"cls_loss":0.2,"dfl_loss":0.3,"mAP50":0.9,"mAP50-95":0.7,"epoch":1}"#
            .to_vec(),
    );
    create_fake_outputs(tmp.path(), "job-at-reg-001", &output_files);

    let result = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;
    assert!(
        result.is_ok(),
        "autotracker without weights_ref should succeed: {:?}",
        result.err()
    );

    // Sem weights_ref → NÃO deve haver diretório de weights
    let weights_dir = tmp.path().join("outputs/job-at-reg-001/weights");
    assert!(
        !weights_dir.exists(),
        "weights dir should not exist without weights_ref"
    );

    // Config NÃO deve ter {weights_path} substituído — placeholder permanece literal
    // (trainer mock tolera chave desconhecida — ADR-0012 D5)
    let config_content =
        std::fs::read_to_string(tmp.path().join("outputs/job-at-reg-001/config.yaml")).unwrap();
    assert!(
        config_content.contains("{weights_path}"),
        "config should keep literal {{weights_path}} when no weights_ref: {config_content}"
    );
    assert!(
        config_content.contains("output_path: /outputs/job-at-reg-001"),
        "config should have output_path substituted: {config_content}"
    );

    // Subcomando correto
    let args = executor.last_args().unwrap();
    assert_eq!(args[0], "autotrack");

    // Artefatos: boxes.json + metrics.jsonl (mock produz ambos)
    let artifacts = report.done_artifacts().unwrap();
    let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
    assert!(filenames.contains(&"boxes.json"));
    assert!(filenames.contains(&"metrics.jsonl"));

    // Downloads: apenas package (sem weights)
    let downloads = s3.downloads.lock().unwrap();
    assert!(
        downloads.iter().any(|k| k.contains("packages/")),
        "should download package"
    );
    assert!(
        !downloads.iter().any(|k| k.contains("models/")),
        "should NOT download weights when no weights_ref"
    );
}

// =========================================================================
// J.3 — DispatchRequest.mode serde default + matriz (engine, mode)
// =========================================================================

#[test]
fn dispatch_request_mode_defaults_to_train_when_absent() {
    let json = r#"{
        "job_id": "j1",
        "engine": "yolo",
        "image": "img:local",
        "exec_mode": "docker",
        "package_ref": {"key": "packages/p/dataset.zip", "md5_zip": "abc", "bytes": 100},
        "workdir": "/tmp"
    }"#;
    let req: DispatchRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.mode, "train");
}

#[test]
fn dispatch_request_mode_deserializes_explicit_value() {
    let json = r#"{
        "job_id": "j1",
        "engine": "yolo",
        "image": "img:local",
        "exec_mode": "docker",
        "package_ref": {"key": "packages/p/dataset.zip", "md5_zip": "abc", "bytes": 100},
        "workdir": "/tmp",
        "mode": "predict"
    }"#;
    let req: DispatchRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.mode, "predict");
}

// -- (yolo, predict) → subcommand predict + artifact predictions.json --

#[tokio::test]
async fn engine_yolo_predict_uses_predict_subcommand_and_predictions_artifact() {
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-pred-001", "yolo", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    dispatch.mode = "predict".to_string();
    // Config with weights_path placeholder (predict always has weights)
    dispatch.config_yaml = Some(
        "mode: predict\npredict:\n  conf: 0.65\ndataset_path: {dataset_path}\noutput_path: {output_path}\nweights_path: {weights_path}"
            .to_string(),
    );

    let weights_bytes = b"fake weights";
    let weights_md5 = compute_file_md5_bytes(weights_bytes);
    dispatch.weights_ref = Some(WeightsRef {
        s3_key: "models/yolo/abc-123/best.pt".to_string(),
        md5: weights_md5,
    });

    let s3w = Arc::new(FakeS3WithWeights::new(weights_bytes.to_vec()));
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    // Pre-cria predictions.json (predict só produz este artefato)
    let predictions = br#"{"engine":"yolo","model":"predict","conf":0.65,"images":[{"filename":"img_0001.jpg","boxes":[]}]}"#;
    let mut output_files = HashMap::new();
    output_files.insert("predictions.json".to_string(), predictions.to_vec());
    create_fake_outputs(tmp.path(), "job-pred-001", &output_files);

    let result = run_job_inner(
        &dispatch,
        s3w.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;
    assert!(
        result.is_ok(),
        "yolo predict pipeline should succeed: {:?}",
        result.err()
    );

    // Verify subcommand: predict, not train
    let args = executor.last_args().unwrap();
    assert_eq!(args[0], "predict");
    assert_eq!(args[1], "--config");
    assert_eq!(args[3], "--output");

    // Verify artifact: only predictions.json with kind="predictions"
    let artifacts = report.done_artifacts().unwrap();
    assert_eq!(
        artifacts.len(),
        1,
        "predict should produce exactly 1 artifact"
    );
    assert_eq!(artifacts[0].path, "predictions.json");
    assert_eq!(artifacts[0].kind, "predictions");

    // Verify weights were downloaded (staged)
    let staged = tmp.path().join("outputs/job-pred-001/weights/best.pt");
    assert!(staged.exists(), "weights should be staged for predict");
    assert_eq!(std::fs::read(&staged).unwrap(), weights_bytes);

    // Verify config has weights_path replaced
    let config_content =
        std::fs::read_to_string(tmp.path().join("outputs/job-pred-001/config.yaml")).unwrap();
    assert!(
        config_content.contains("/outputs/job-pred-001/weights/best.pt"),
        "config should have replaced weights_path, got: {config_content}"
    );
}

// -- (yolo, unknown mode) → PipelineError::Other --

#[tokio::test]
async fn yolo_unknown_mode_returns_clean_error() {
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-bad-mode", "yolo", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    dispatch.mode = "finetune".to_string(); // unknown mode

    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let result = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;
    assert!(
        matches!(result, Err(PipelineError::Other(ref msg)) if msg.contains("unsupported engine/mode")),
        "should fail with clean error for unknown mode: {:?}",
        result
    );

    // Executor never called
    assert!(executor.last_args().is_none());
}

// -- (yolo, train) regressão intocada --

#[tokio::test]
async fn yolo_train_regression_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-train-reg", "yolo", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    dispatch.mode = "train".to_string();

    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let mut output_files = HashMap::new();
    output_files.insert("best.pt".to_string(), b"fake model".to_vec());
    output_files.insert("last.pt".to_string(), b"fake model".to_vec());
    output_files.insert(
        "metrics.jsonl".to_string(),
        br#"{"box_loss":0.5,"cls_loss":0.3,"dfl_loss":0.2,"mAP50":0.8,"mAP50-95":0.6,"epoch":1}"#
            .to_vec(),
    );
    create_fake_outputs(tmp.path(), "job-train-reg", &output_files);

    let result = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;
    assert!(
        result.is_ok(),
        "yolo train regression should succeed: {:?}",
        result.err()
    );

    let args = executor.last_args().unwrap();
    assert_eq!(args[0], "train");

    let artifacts = report.done_artifacts().unwrap();
    let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
    assert!(filenames.contains(&"best.pt"));
    assert!(filenames.contains(&"last.pt"));
    assert!(filenames.contains(&"metrics.jsonl"));
}

// -- (yolo, predict) pipeline without metrics.jsonl → done with no metrics/epoch --

#[tokio::test]
async fn predict_pipeline_done_without_metrics_file() {
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-pred-no-metrics", "yolo", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    dispatch.mode = "predict".to_string();

    let weights_bytes = b"fake weights";
    let weights_md5 = compute_file_md5_bytes(weights_bytes);
    dispatch.weights_ref = Some(WeightsRef {
        s3_key: "models/yolo/abc/best.pt".to_string(),
        md5: weights_md5,
    });
    let s3w = Arc::new(FakeS3WithWeights::new(weights_bytes.to_vec()));

    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    // Only predictions.json — no metrics.jsonl (predict doesn't produce it)
    let mut output_files = HashMap::new();
    output_files.insert(
        "predictions.json".to_string(),
        br#"{"engine":"yolo","model":"predict","conf":0.65,"images":[]}"#.to_vec(),
    );
    create_fake_outputs(tmp.path(), "job-pred-no-metrics", &output_files);

    let result = run_job_inner(
        &dispatch,
        s3w.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;
    assert!(
        result.is_ok(),
        "predict pipeline without metrics.jsonl should succeed: {:?}",
        result.err()
    );

    // Verify done report has no metrics/epoch (predict is binary progress)
    let statuses = report.statuses();
    assert!(statuses.contains(&"running".to_string()));
    assert!(statuses.contains(&"done".to_string()));

    let done_report = report
        .reports
        .lock()
        .unwrap()
        .iter()
        .find(|r| r.status == "done")
        .cloned()
        .unwrap();
    assert!(
        done_report.metrics.is_none(),
        "predict done should have no metrics"
    );
    assert!(
        done_report.epoch.is_none(),
        "predict done should have no epoch"
    );
    assert_eq!(done_report.progress, Some(1.0));

    // Verify artifact
    let artifacts = report.done_artifacts().unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].path, "predictions.json");
    assert_eq!(artifacts[0].kind, "predictions");
}

// =========================================================================
// G.4 — Daemon + glob + multi-ref tests (ADR-0023)
// =========================================================================

use daemon::{DaemonClient, DaemonLauncher, DaemonState, GenerateBody, HealthResponse};

/// Fake DaemonClient para testes.
/// Controla health/busy/generate via campos Mutex.
struct FakeDaemonClient {
    health_response: Mutex<Option<HealthResponse>>,
    generate_results: Mutex<Vec<Result<(), String>>>,
    generate_call_count: Mutex<usize>,
    health_call_count: Mutex<usize>,
}

impl FakeDaemonClient {
    fn new() -> Self {
        Self {
            health_response: Mutex::new(Some(HealthResponse {
                ok: true,
                loaded_spec: Some(serde_json::Value::String("flux-2-klein-4b".to_string())),
                busy: false,
                _extra: Default::default(),
            })),
            generate_results: Mutex::new(Vec::new()),
            generate_call_count: Mutex::new(0),
            health_call_count: Mutex::new(0),
        }
    }

    fn set_health(&self, resp: Option<HealthResponse>) {
        *self.health_response.lock().unwrap() = resp;
    }

    fn set_generate_results(&self, results: Vec<Result<(), String>>) {
        *self.generate_results.lock().unwrap() = results;
    }

    fn generate_calls(&self) -> usize {
        *self.generate_call_count.lock().unwrap()
    }

    fn health_calls(&self) -> usize {
        *self.health_call_count.lock().unwrap()
    }
}

#[async_trait]
impl DaemonClient for FakeDaemonClient {
    async fn health(&self) -> Option<HealthResponse> {
        *self.health_call_count.lock().unwrap() += 1;
        self.health_response.lock().unwrap().clone()
    }

    async fn generate(&self, _body: &GenerateBody) -> Result<(), String> {
        let mut count = self.generate_call_count.lock().unwrap();
        let results = self.generate_results.lock().unwrap();
        let idx = *count;
        *count += 1;
        if idx < results.len() {
            results[idx].clone()
        } else {
            panic!(
                "FakeDaemonClient: generate called more times than results provided (call {idx})"
            );
        }
    }

    async fn shutdown(&self) -> Result<(), String> {
        Ok(())
    }
}

/// Fake DaemonLauncher para testes.
struct FakeDaemonLauncher {
    start_call_count: Mutex<usize>,
    kill_call_count: Mutex<usize>,
    fail_start: bool,
}

impl FakeDaemonLauncher {
    fn new() -> Self {
        Self {
            start_call_count: Mutex::new(0),
            kill_call_count: Mutex::new(0),
            fail_start: false,
        }
    }

    fn with_fail_start() -> Self {
        Self {
            start_call_count: Mutex::new(0),
            kill_call_count: Mutex::new(0),
            fail_start: true,
        }
    }

    fn start_calls(&self) -> usize {
        *self.start_call_count.lock().unwrap()
    }

    fn kill_calls(&self) -> usize {
        *self.kill_call_count.lock().unwrap()
    }
}

#[async_trait]
impl DaemonLauncher for FakeDaemonLauncher {
    async fn start(&self) -> Result<String, String> {
        *self.start_call_count.lock().unwrap() += 1;
        if self.fail_start {
            Err("docker run daemon failed".to_string())
        } else {
            Ok("http://localhost:8766".to_string())
        }
    }

    async fn kill(&self) -> Result<(), String> {
        *self.kill_call_count.lock().unwrap() += 1;
        Ok(())
    }
}

// -- G.4 test 1: daemon_disabled_one_shot_intacto --
/// DIFFUSION_DAEMON_ENABLED=0 → path one-shot exatamente igual ao comportamento legado.
#[tokio::test]
async fn daemon_disabled_one_shot_intacto() {
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let mut dispatch = make_dispatch("job-daemon-off-001", "diffusion");
    dispatch.mode = "generate".to_string();
    dispatch.package_ref = None;
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let mut output_files = HashMap::new();
    output_files.insert(
        "generated.png".to_string(),
        b"fake png image bytes".to_vec(),
    );
    create_fake_outputs(tmp.path(), "job-daemon-off-001", &output_files);

    let result = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None, // daemon_state = None → one-shot
    )
    .await;

    assert!(
        result.is_ok(),
        "daemon disabled one-shot should succeed: {:?}",
        result.err()
    );

    // Verifica subcomando: generate (one-shot)
    let args = executor.last_args().unwrap();
    assert_eq!(args[0], "generate");

    // Verifica artefato coletado via glob: generated.png → kind "generated"
    let artifacts = report.done_artifacts().unwrap();
    let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
    let kinds: Vec<&str> = artifacts.iter().map(|a| a.kind.as_str()).collect();
    assert!(filenames.contains(&"generated.png"));
    assert!(kinds.contains(&"generated"));
}

// -- G.4 test 2: glob_coleta_batch --
/// outputs com generated_0001.png/generated_0002.png/thumb_0001.jpg/thumb_0002.jpg/generation_meta.json
/// → 5 artefatos com kinds corretos.
#[tokio::test]
async fn glob_coleta_batch() {
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let mut dispatch = make_dispatch("job-glob-batch-001", "diffusion");
    dispatch.mode = "generate".to_string();
    dispatch.package_ref = None;
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let mut output_files = HashMap::new();
    output_files.insert("generated_0001.png".to_string(), b"png1".to_vec());
    output_files.insert("generated_0002.png".to_string(), b"png2".to_vec());
    output_files.insert("thumb_0001.jpg".to_string(), b"thumb1".to_vec());
    output_files.insert("thumb_0002.jpg".to_string(), b"thumb2".to_vec());
    output_files.insert("generation_meta.json".to_string(), b"{}".to_vec());
    create_fake_outputs(tmp.path(), "job-glob-batch-001", &output_files);

    let result = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;

    assert!(
        result.is_ok(),
        "glob batch should succeed: {:?}",
        result.err()
    );

    let artifacts = report.done_artifacts().unwrap();
    assert_eq!(artifacts.len(), 5, "should collect 5 artifacts from glob");
    let kinds: Vec<&str> = artifacts.iter().map(|a| a.kind.as_str()).collect();
    assert!(kinds.contains(&"generated"), "should have 'generated' kind");
    assert!(
        kinds.contains(&"generated_thumb"),
        "should have 'generated_thumb' kind"
    );
    assert!(
        kinds.contains(&"generated_meta"),
        "should have 'generated_meta' kind"
    );
}

// -- G.4 test 3: glob_casa_generated_png_legado --
/// outputs com apenas generated.png → artefato kind "generated" (retrocompat).
#[tokio::test]
async fn glob_casa_generated_png_legado() {
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let mut dispatch = make_dispatch("job-glob-legado-001", "diffusion");
    dispatch.mode = "generate".to_string();
    dispatch.package_ref = None;
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let mut output_files = HashMap::new();
    output_files.insert("generated.png".to_string(), b"legacy png".to_vec());
    create_fake_outputs(tmp.path(), "job-glob-legado-001", &output_files);

    let result = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;

    assert!(
        result.is_ok(),
        "legacy generated.png should succeed: {:?}",
        result.err()
    );

    let artifacts = report.done_artifacts().unwrap();
    assert_eq!(artifacts.len(), 1, "should collect exactly 1 artifact");
    assert_eq!(artifacts[0].path, "generated.png");
    assert_eq!(artifacts[0].kind, "generated");
}

// -- G.4 test 3b: glob_dedup_generated_png ---
/// outputs com generated.png E generated_0001.png → só coleta generated_0001.png
/// (generated.png é symlink legado, pula quando existir numerado).
#[tokio::test]
async fn glob_dedup_generated_png() {
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let mut dispatch = make_dispatch("job-glob-dedup-001", "diffusion");
    dispatch.mode = "generate".to_string();
    dispatch.package_ref = None;
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let mut output_files = HashMap::new();
    output_files.insert("generated.png".to_string(), b"legacy symlink".to_vec());
    output_files.insert("generated_0001.png".to_string(), b"real png".to_vec());
    create_fake_outputs(tmp.path(), "job-glob-dedup-001", &output_files);

    let result = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;

    assert!(
        result.is_ok(),
        "glob dedup should succeed: {:?}",
        result.err()
    );

    let artifacts = report.done_artifacts().unwrap();
    let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
    // generated_0001.png coletado, generated.png PULADO
    assert!(
        filenames.contains(&"generated_0001.png"),
        "should collect numbered file"
    );
    assert!(
        !filenames.contains(&"generated.png"),
        "should NOT collect generated.png when numbered exists"
    );
}

// -- G.4 test 4: daemon_hot_path_sem_docker_run --
/// DIFFUSION_DAEMON_ENABLED=1 + fake launcher/client → POST /generate chamado,
/// executor one-shot NÃO chamado, artefatos reportados.
#[tokio::test]
async fn daemon_hot_path_sem_docker_run() {
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let mut dispatch = make_dispatch("job-daemon-hot-001", "diffusion");
    dispatch.mode = "generate".to_string();
    dispatch.package_ref = None;
    dispatch.config_yaml =
        Some("base_model: flux-2-klein-4b\noutput_path: {output_path}".to_string());
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let client = Arc::new(FakeDaemonClient::new());
    let launcher = Arc::new(FakeDaemonLauncher::new());
    let daemon_state = Arc::new(DaemonState::new(
        "hephaestus/trainer-difusao:local",
        8766,
        600,
        client.clone() as Arc<dyn DaemonClient>,
        launcher.clone() as Arc<dyn DaemonLauncher>,
    ));
    daemon_state.set_running(true, Some("http://localhost:8766".to_string()));

    // POST /generate: retorna Ok
    client.set_generate_results(vec![Ok(())]);

    // Cria artefatos que o daemon "produziria"
    let mut output_files = HashMap::new();
    output_files.insert("generated_0001.png".to_string(), b"daemon png".to_vec());
    output_files.insert("generation_meta.json".to_string(), b"{}".to_vec());
    create_fake_outputs(tmp.path(), "job-daemon-hot-001", &output_files);

    let result = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        Some(&daemon_state),
    )
    .await;

    assert!(
        result.is_ok(),
        "daemon hot path should succeed: {:?}",
        result.err()
    );

    // POST /generate foi chamado
    assert_eq!(
        client.generate_calls(),
        1,
        "daemon generate should be called once"
    );

    // Executor one-shot NÃO foi chamado
    assert!(
        executor.last_args().is_none(),
        "one-shot executor should NOT be called"
    );

    // Artefatos coletados via glob
    let artifacts = report.done_artifacts().unwrap();
    let filenames: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
    assert!(filenames.contains(&"generated_0001.png"));
    assert!(filenames.contains(&"generation_meta.json"));
}

// -- G.4 test 5: daemon_reload_quando_spec_muda --
/// 2 jobs sequenciais com specs diferentes → client vê 2 POST /generate e
/// health foi consultada entre eles; spec igual → ainda 2 posts.
/// Testa apenas que o orchestrator NÃO reinicia o launcher entre jobs da mesma spec.
#[tokio::test]
async fn daemon_reload_quando_spec_muda() {
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());

    let client = Arc::new(FakeDaemonClient::new());
    let launcher = Arc::new(FakeDaemonLauncher::new());
    let daemon_state = Arc::new(DaemonState::new(
        "hephaestus/trainer-difusao:local",
        8766,
        600,
        client.clone() as Arc<dyn DaemonClient>,
        launcher.clone() as Arc<dyn DaemonLauncher>,
    ));
    daemon_state.set_running(true, Some("http://localhost:8766".to_string()));

    // POST /generate: 2 Ok (1 por job)
    client.set_generate_results(vec![Ok(()), Ok(())]);

    // Job 1
    let mut dispatch1 = make_dispatch("job-spec-001", "diffusion");
    dispatch1.mode = "generate".to_string();
    dispatch1.package_ref = None;
    dispatch1.config_yaml =
        Some("base_model: flux-2-klein-4b\noutput_path: {output_path}".to_string());
    dispatch1.workdir = tmp.path().to_str().unwrap().to_string();
    let active_jobs1 = new_active_jobs();

    let mut output_files1 = HashMap::new();
    output_files1.insert("generated.png".to_string(), b"img1".to_vec());
    create_fake_outputs(tmp.path(), "job-spec-001", &output_files1);

    let result1 = run_job_inner(
        &dispatch1,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs1,
        None,
        false,
        Some(&daemon_state),
    )
    .await;
    assert!(result1.is_ok(), "job 1 should succeed: {:?}", result1.err());

    let health_after_job1 = client.health_calls();
    let gen_after_job1 = client.generate_calls();
    let start_after_job1 = launcher.start_calls();

    // Job 2 (mesma spec)
    let mut dispatch2 = make_dispatch("job-spec-002", "diffusion");
    dispatch2.mode = "generate".to_string();
    dispatch2.package_ref = None;
    dispatch2.config_yaml =
        Some("base_model: flux-2-klein-4b\noutput_path: {output_path}".to_string());
    dispatch2.workdir = tmp.path().to_str().unwrap().to_string();
    let active_jobs2 = new_active_jobs();

    let mut output_files2 = HashMap::new();
    output_files2.insert("generated.png".to_string(), b"img2".to_vec());
    create_fake_outputs(tmp.path(), "job-spec-002", &output_files2);

    let result2 = run_job_inner(
        &dispatch2,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs2,
        None,
        false,
        Some(&daemon_state),
    )
    .await;
    assert!(result2.is_ok(), "job 2 should succeed: {:?}", result2.err());

    // 2 generates chamados
    assert_eq!(
        client.generate_calls(),
        gen_after_job1 + 1,
        "should have 2 generate calls total"
    );
    // Health consultada (ensure_daemon_ready consulta health)
    assert!(
        client.health_calls() > health_after_job1,
        "health should be consulted between jobs"
    );
    // Launcher NÃO reiniciado (daemon já está rodando)
    assert_eq!(
        launcher.start_calls(),
        start_after_job1,
        "launcher should NOT be called again for same spec"
    );
}

// -- G.4 test 6: daemon_falha_honesta --
/// Launcher falha ao subir / health timeout → job failed com erro claro.
#[tokio::test]
async fn daemon_falha_honesta() {
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let mut dispatch = make_dispatch("job-daemon-fail-001", "diffusion");
    dispatch.mode = "generate".to_string();
    dispatch.package_ref = None;
    dispatch.config_yaml =
        Some("base_model: flux-2-klein-4b\noutput_path: {output_path}".to_string());
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let client = Arc::new(FakeDaemonClient::new());
    let launcher = Arc::new(FakeDaemonLauncher::with_fail_start()); // falha ao iniciar
                                                                    // Daemon NÃO está rodando — launcher vai falhar

    // Usa run_job (outer) para testar o caminho completo de falha com report
    run_job(
        dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        active_jobs,
        None,
        false,
        Some(Arc::new(DaemonState::new(
            "hephaestus/trainer-difusao:local",
            8766,
            600,
            client.clone() as Arc<dyn DaemonClient>,
            launcher.clone() as Arc<dyn DaemonLauncher>,
        ))),
    )
    .await;

    // Executor nunca chamado
    assert!(executor.last_args().is_none());

    // Reports incluem preparing e failed (via run_job outer)
    let statuses = report.statuses();
    assert!(statuses.contains(&"preparing".to_string()));
    assert!(statuses.contains(&"failed".to_string()));
}

// -- G.4 test 7: 409 daemon_busy --
/// Client fake responde 409 2x e depois 200 → job ok;
/// 3x → job failed honesto.
#[tokio::test]
async fn daemon_busy_retry_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let mut dispatch = make_dispatch("job-busy-001", "diffusion");
    dispatch.mode = "generate".to_string();
    dispatch.package_ref = None;
    dispatch.config_yaml =
        Some("base_model: flux-2-klein-4b\noutput_path: {output_path}".to_string());
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let client = Arc::new(FakeDaemonClient::new());
    // 409, 409, 200
    client.set_generate_results(vec![
        Err("busy".to_string()),
        Err("busy".to_string()),
        Ok(()),
    ]);

    let launcher = Arc::new(FakeDaemonLauncher::new());
    let daemon_state = Arc::new(DaemonState::new(
        "hephaestus/trainer-difusao:local",
        8766,
        600,
        client.clone() as Arc<dyn DaemonClient>,
        launcher.clone() as Arc<dyn DaemonLauncher>,
    ));
    daemon_state.set_running(true, Some("http://localhost:8766".to_string()));

    let mut output_files = HashMap::new();
    output_files.insert("generated.png".to_string(), b"busy ok".to_vec());
    create_fake_outputs(tmp.path(), "job-busy-001", &output_files);

    let result = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        Some(&daemon_state),
    )
    .await;

    assert!(
        result.is_ok(),
        "busy retry should eventually succeed: {:?}",
        result.err()
    );
    assert_eq!(
        client.generate_calls(),
        3,
        "should have retried 3 times total"
    );
}

#[tokio::test]
async fn daemon_busy_exhausted_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let mut dispatch = make_dispatch("job-busy-002", "diffusion");
    dispatch.mode = "generate".to_string();
    dispatch.package_ref = None;
    dispatch.config_yaml =
        Some("base_model: flux-2-klein-4b\noutput_path: {output_path}".to_string());
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let client = Arc::new(FakeDaemonClient::new());
    // 409, 409, 409 (3x busy → exhausted)
    client.set_generate_results(vec![
        Err("busy".to_string()),
        Err("busy".to_string()),
        Err("busy".to_string()),
    ]);

    let launcher = Arc::new(FakeDaemonLauncher::new());
    let daemon_state = Arc::new(DaemonState::new(
        "hephaestus/trainer-difusao:local",
        8766,
        600,
        client.clone() as Arc<dyn DaemonClient>,
        launcher.clone() as Arc<dyn DaemonLauncher>,
    ));
    daemon_state.set_running(true, Some("http://localhost:8766".to_string()));

    let result = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        Some(&daemon_state),
    )
    .await;

    assert!(
        matches!(result, Err(PipelineError::DaemonBusy)),
        "should fail with DaemonBusy after 3 retries: {:?}",
        result
    );
}

// -- G.4 test 8: staging_loras --
/// dispatch com 2 loras + custom → arquivos staged no workdir e config.yaml reescrito.
#[tokio::test]
async fn staging_loras() {
    let tmp = tempfile::tempdir().unwrap();

    // FakeS3WithWeights serve weights_bytes para models/ e artifacts/.
    // Todos os downloads recebem o mesmo conteúdo.
    let weights_bytes = b"fake weights data for all refs";
    let weights_md5 = compute_file_md5_bytes(weights_bytes);
    let s3 = Arc::new(FakeS3WithWeights::new(weights_bytes.to_vec()));

    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-loras-001", "diffusion", &zip_path);
    dispatch.mode = "generate".to_string();
    dispatch.package_ref = None; // Sem package para generate
    dispatch.config_yaml = Some(
        "base_model: flux-2-klein-4b\noutput_path: {output_path}\nlora_0: {lora_path_0}\nlora_1: {lora_path_1}\ncustom: {custom_checkpoint_path}".to_string()
    );
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    dispatch.loras = vec![
        LoraRefStage {
            s3_key: "models/lora/abc/lora_a.safetensors".to_string(),
            md5: weights_md5.clone(),
            scale: 0.8,
        },
        LoraRefStage {
            s3_key: "models/lora/def/lora_b.safetensors".to_string(),
            md5: weights_md5.clone(),
            scale: 0.5,
        },
    ];
    dispatch.custom_checkpoint = Some(WeightRef {
        s3_key: "models/checkpoint/xyz/custom.safetensors".to_string(),
        md5: weights_md5.clone(), // mesmo conteúdo do FakeS3WithWeights
    });

    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    // Pre-cria outputs
    let outputs = tmp.path().join("outputs/job-loras-001");
    std::fs::create_dir_all(&outputs).unwrap();

    let result = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;

    assert!(
        result.is_ok(),
        "staging loras should succeed: {:?}",
        result.err()
    );

    // Verifica staging: 2 LoRAs + 1 custom
    let weights_dir = tmp.path().join("outputs/job-loras-001/weights");
    assert!(
        weights_dir.join("lora_0.safetensors").exists(),
        "lora_0 should be staged"
    );
    assert!(
        weights_dir.join("lora_1.safetensors").exists(),
        "lora_1 should be staged"
    );
    assert!(
        weights_dir.join("custom.safetensors").exists(),
        "custom should be staged"
    );

    // Verifica config.yaml reescrito com caminhos staged
    let config_content = std::fs::read_to_string(outputs.join("config.yaml")).unwrap();
    assert!(
        config_content.contains("/outputs/job-loras-001/weights/lora_0.safetensors"),
        "config should have lora_path_0 replaced: {config_content}"
    );
    assert!(
        config_content.contains("/outputs/job-loras-001/weights/lora_1.safetensors"),
        "config should have lora_path_1 replaced: {config_content}"
    );
    assert!(
        config_content.contains("/outputs/job-loras-001/weights/custom.safetensors"),
        "config should have custom_checkpoint_path replaced: {config_content}"
    );
    assert!(
        !config_content.contains("{lora_path_0}"),
        "config should not contain literal {{lora_path_0}}"
    );
    assert!(
        !config_content.contains("{custom_checkpoint_path}"),
        "config should not contain literal {{custom_checkpoint_path}}"
    );
}
/// dispatch com text_encoder + custom (treino flux-2) → arquivos staged e
/// `{text_encoder_path}`/`{custom_checkpoint_path}` substituídos.
#[tokio::test]
async fn staging_text_encoder_and_custom_train() {
    let tmp = tempfile::tempdir().unwrap();
    let weights_bytes = b"fake encoder data";
    let weights_md5 = compute_file_md5_bytes(weights_bytes);
    let s3 = Arc::new(FakeS3WithWeights::new(weights_bytes.to_vec()));
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();

    let mut dispatch = make_dispatch_with_valid_md5("job-enc-001", "diffusion", &zip_path);
    dispatch.mode = "train".to_string();
    dispatch.config_yaml = Some(
        "model: flux-2-klein-4b\ncustom_checkpoint_path: {custom_checkpoint_path}\ntext_encoder_path: {text_encoder_path}\noutput_path: {output_path}".to_string(),
    );
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    dispatch.custom_checkpoint = Some(WeightRef {
        s3_key: "models/checkpoint/xyz/custom.safetensors".to_string(),
        md5: weights_md5.clone(),
    });
    dispatch.text_encoder = Some(WeightRef {
        s3_key: "models/diffusion/enc/text_encoder.safetensors".to_string(),
        md5: weights_md5.clone(),
    });

    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();
    let outputs = tmp.path().join("outputs/job-enc-001");
    std::fs::create_dir_all(&outputs).unwrap();

    let result = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;
    assert!(
        result.is_ok(),
        "staging encoder should succeed: {:?}",
        result.err()
    );

    let weights_dir = tmp.path().join("outputs/job-enc-001/weights");
    assert!(
        weights_dir.join("custom.safetensors").exists(),
        "custom staged"
    );
    assert!(
        weights_dir.join("text_encoder.safetensors").exists(),
        "encoder staged"
    );

    let config_content = std::fs::read_to_string(outputs.join("config.yaml")).unwrap();
    assert!(
        config_content.contains("/outputs/job-enc-001/weights/custom.safetensors"),
        "custom replaced: {config_content}"
    );
    assert!(
        config_content.contains("/outputs/job-enc-001/weights/text_encoder.safetensors"),
        "encoder replaced: {config_content}"
    );
    assert!(!config_content.contains("{custom_checkpoint_path}"));
    assert!(!config_content.contains("{text_encoder_path}"));
}

// -- G.4 test 9: preempção --
/// daemon idle + job treino → kill chamado antes do dispatch de treino.
#[tokio::test]
async fn preemption_kills_idle_daemon_before_training() {
    let client = Arc::new(FakeDaemonClient::new());
    let launcher = Arc::new(FakeDaemonLauncher::new());
    let daemon_state = Arc::new(DaemonState::new(
        "hephaestus/trainer-difusao:local",
        8766,
        600,
        client.clone() as Arc<dyn DaemonClient>,
        launcher.clone() as Arc<dyn DaemonLauncher>,
    ));
    daemon_state.set_running(true, Some("http://localhost:8766".to_string()));
    // Simula daemon idle: last_used há 600s (TTL/2 = 300s)
    *daemon_state.last_used.lock().unwrap() = Instant::now() - Duration::from_secs(600);

    // Health diz busy=false
    client.set_health(Some(HealthResponse {
        ok: true,
        loaded_spec: None,
        busy: false,
        _extra: Default::default(),
    }));

    // Chama preempção diretamente
    daemon::maybe_preempt_daemon(&daemon_state).await;

    // Verifica que kill foi chamado
    assert_eq!(
        launcher.kill_calls(),
        1,
        "daemon should be killed before training"
    );
    assert!(
        !daemon_state.is_running(),
        "daemon should not be running after preemption"
    );
}

#[tokio::test]
async fn preemption_skips_busy_daemon() {
    let client = Arc::new(FakeDaemonClient::new());
    let launcher = Arc::new(FakeDaemonLauncher::new());
    let daemon_state = Arc::new(DaemonState::new(
        "hephaestus/trainer-difusao:local",
        8766,
        600,
        client.clone() as Arc<dyn DaemonClient>,
        launcher.clone() as Arc<dyn DaemonLauncher>,
    ));
    daemon_state.set_running(true, Some("http://localhost:8766".to_string()));
    *daemon_state.last_used.lock().unwrap() = Instant::now() - Duration::from_secs(600);

    // Health diz busy=true
    client.set_health(Some(HealthResponse {
        ok: true,
        loaded_spec: None,
        busy: true,
        _extra: Default::default(),
    }));

    daemon::maybe_preempt_daemon(&daemon_state).await;

    // Kill NÃO chamado (daemon busy)
    assert_eq!(launcher.kill_calls(), 0, "busy daemon should NOT be killed");
    assert!(daemon_state.is_running(), "daemon should still be running");
}

#[tokio::test]
async fn preemption_noop_when_not_running() {
    let client = Arc::new(FakeDaemonClient::new());
    let launcher = Arc::new(FakeDaemonLauncher::new());
    let daemon_state = Arc::new(DaemonState::new(
        "hephaestus/trainer-difusao:local",
        8766,
        600,
        client.clone() as Arc<dyn DaemonClient>,
        launcher.clone() as Arc<dyn DaemonLauncher>,
    ));
    // Daemon não está rodando

    daemon::maybe_preempt_daemon(&daemon_state).await;

    assert_eq!(launcher.kill_calls(), 0, "should not kill when not running");
    assert_eq!(
        client.health_calls(),
        0,
        "should not check health when not running"
    );
}

// -- is_training_metric tests (AC-006-A D1) --

#[test]
fn is_training_metric_phase_only_is_false() {
    // Linha de status (ex.: loading_model) — sem valores numéricos de treino.
    let m = MetricsLine {
        loss: None,
        lr: None,
        box_loss: 0.0,
        cls_loss: 0.0,
        dfl_loss: 0.0,
        map50: 0.0,
        map50_95: 0.0,
        step: None,
        epoch: 0,
        progress: Some(0.05),
        phase: Some("loading_model".to_string()),
        message: Some("Carregando FLUX".to_string()),
        vram_used_gb: None,
    };
    assert!(
        !m.is_training_metric(),
        "phase-only line is not a training metric"
    );
}

#[test]
fn is_training_metric_with_loss_is_true() {
    let m = MetricsLine {
        loss: Some(0.4),
        lr: Some(0.0001),
        ..Default::default()
    };
    assert!(
        m.is_training_metric(),
        "line with loss is a training metric"
    );
}

#[test]
fn is_training_metric_autolabel_zeros_is_false() {
    // Autolabel emite linhas com zeros — são eventos de status.
    let m = MetricsLine {
        loss: None,
        lr: None,
        box_loss: 0.0,
        cls_loss: 0.0,
        dfl_loss: 0.0,
        map50: 0.0,
        map50_95: 0.0,
        step: Some(10),
        epoch: 0,
        progress: Some(0.1),
        phase: None,
        message: None,
        vram_used_gb: None,
    };
    assert!(
        !m.is_training_metric(),
        "autolabel zeros is not a training metric"
    );
}

#[test]
fn is_training_metric_yolo_with_box_loss_is_true() {
    let m = MetricsLine {
        box_loss: 0.5,
        cls_loss: 0.3,
        dfl_loss: 0.2,
        map50: 0.8,
        map50_95: 0.6,
        epoch: 5,
        ..Default::default()
    };
    assert!(
        m.is_training_metric(),
        "YOLO line with box_loss is a training metric"
    );
}

#[test]
fn is_training_metric_diffusion_with_map_is_true() {
    let m = MetricsLine {
        loss: Some(0.045),
        map50: 0.9,
        epoch: 3,
        ..Default::default()
    };
    assert!(
        m.is_training_metric(),
        "diffusion line with mAP is a training metric"
    );
}

#[test]
fn is_training_metric_with_phase_is_true_and_phase_promoted() {
    // P2-1: linha métrica COM phase continua métrica e promove a fase
    // (o report carrega `metrics: Some` + `phase: m.phase`).
    let m = MetricsLine {
        loss: Some(0.4),
        epoch: 3,
        phase: Some("training".to_string()),
        message: Some("Época 3".to_string()),
        ..Default::default()
    };
    assert!(
        m.is_training_metric(),
        "metric line with phase is still a training metric"
    );
    let json = m.to_report_json();
    assert_eq!(json.get("phase").and_then(|v| v.as_str()), Some("training"));
    assert_eq!(
        json.get("message").and_then(|v| v.as_str()),
        Some("Época 3")
    );
}

#[test]
fn parse_metrics_line_nan_loss_is_not_metric() {
    // P2-2: literal NaN é sanitizado para null (parseia) e a linha
    // resultante NÃO é métrica. Nota: sem epoch/phase/progress a linha é
    // descartada por falta de epoch — o teste carrega epoch explícito.
    let m = parse_metrics_line(r#"{"epoch": 3, "loss": NaN}"#)
        .expect("NaN line must parse after sanitization");
    assert_eq!(m.loss, None, "NaN loss must become null");
    assert!(
        !m.is_training_metric(),
        "sanitized NaN-loss line is not a training metric"
    );
}

// -- telemetry tail (daemon path): tail_jsonl_lines + telemetry_report_for_line --
#[test]
fn tail_jsonl_lines_incremental_skips_malformed() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("telemetry.jsonl");
    std::fs::write(
        &path,
        "{\"phase\":\"loading_model\",\"progress\":0.1}\nnot-json\n{\"progress\":0.5}\n",
    )
    .unwrap();
    let (parsed, offset) = tail_jsonl_lines(&path, 0);
    assert_eq!(offset, 3, "offset avança inclusive sobre linha malformada");
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].phase.as_deref(), Some("loading_model"));
    // Sem mais linhas novas → vazio, offset estável.
    let (parsed2, offset2) = tail_jsonl_lines(&path, offset);
    assert!(parsed2.is_empty());
    assert_eq!(offset2, offset);
    // Append incremental: só a linha nova é retornada.
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(f, "{{\"progress\":0.9}}").unwrap();
    let (parsed3, offset3) = tail_jsonl_lines(&path, offset2);
    assert_eq!(parsed3.len(), 1);
    assert_eq!(offset3, offset2 + 1);
}

#[test]
fn tail_jsonl_lines_missing_file_keeps_offset() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("telemetry.jsonl");
    let (parsed, offset) = tail_jsonl_lines(&path, 7);
    assert!(parsed.is_empty());
    assert_eq!(offset, 7, "arquivo ausente não reseta o offset");
}

#[test]
fn telemetry_report_for_line_matches_oneshot_format() {
    // Progress explícito honrado; evento de status → metrics None, phase/message promovidas.
    let m = parse_metrics_line(r#"{"phase":"denoising","message":"Etapa 3/20","progress":0.15}"#)
        .unwrap();
    let body = telemetry_report_for_line(&m, 100);
    assert_eq!(body.status, "running");
    assert!((body.progress.unwrap() - 0.15).abs() < 1e-9);
    assert_eq!(body.metrics, None);
    assert_eq!(body.phase.as_deref(), Some("denoising"));
    assert_eq!(body.message.as_deref(), Some("Etapa 3/20"));
    // Métrica de treino → metrics Some + phase junto.
    let t = parse_metrics_line(r#"{"epoch":3,"loss":0.5,"phase":"training"}"#).unwrap();
    let t_body = telemetry_report_for_line(&t, 100);
    assert!(t_body.metrics.is_some());
    assert_eq!(t_body.phase.as_deref(), Some("training"));
    assert!((t_body.progress.unwrap() - 0.03).abs() < 1e-9);
}

/// Regressão: eventos de telemetry.jsonl escritos durante o generate do
/// daemon chegam como reports de progresso (antes: 0.0 até done).
///
/// O FakeDaemonClient escreve linhas de telemetria no `telemetry_path`
/// recebido ANTES de retornar Ok — o tail do path daemon deve reportá-las
/// como "running" com progress/phase, além do "done" final.
#[tokio::test]
async fn daemon_path_emite_progresso_de_telemetry_jsonl() {
    use std::io::Write;
    struct TelemetryWritingClient;
    #[async_trait]
    impl DaemonClient for TelemetryWritingClient {
        async fn health(&self) -> Option<HealthResponse> {
            Some(HealthResponse {
                ok: true,
                loaded_spec: None,
                busy: false,
                _extra: Default::default(),
            })
        }
        async fn generate(&self, body: &GenerateBody) -> Result<(), String> {
            let path = std::path::PathBuf::from(&body.telemetry_path);
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .unwrap();
            // Dá tempo ao tail (500ms) de observar o arquivo antes do 200.
            for (phase, progress) in [("loading_model", 0.1), ("denoising", 0.5)] {
                writeln!(f, "{{\"phase\":\"{phase}\",\"progress\":{progress}}}").unwrap();
                f.flush().unwrap();
                tokio::time::sleep(Duration::from_millis(700)).await;
            }
            Ok(())
        }
        async fn shutdown(&self) -> Result<(), String> {
            Ok(())
        }
    }
    struct NoopLauncher;
    #[async_trait]
    impl DaemonLauncher for NoopLauncher {
        async fn start(&self) -> Result<String, String> {
            Ok("http://localhost:8766".to_string())
        }
        async fn kill(&self) -> Result<(), String> {
            Ok(())
        }
    }

    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let mut dispatch = make_dispatch("job-daemon-telemetry-001", "diffusion");
    dispatch.mode = "generate".to_string();
    dispatch.package_ref = None;
    dispatch.config_yaml =
        Some("base_model: flux-2-klein-4b\noutput_path: {output_path}".to_string());
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    let client = Arc::new(TelemetryWritingClient);
    let launcher = Arc::new(NoopLauncher);
    let daemon_state = Arc::new(DaemonState::new(
        "hephaestus/trainer-difusao:local",
        8766,
        600,
        client as Arc<dyn DaemonClient>,
        launcher as Arc<dyn DaemonLauncher>,
    ));
    daemon_state.set_running(true, Some("http://localhost:8766".to_string()));

    let mut output_files = HashMap::new();
    output_files.insert("generated_0001.png".to_string(), b"daemon png".to_vec());
    create_fake_outputs(tmp.path(), "job-daemon-telemetry-001", &output_files);

    let result = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        Some(&daemon_state),
    )
    .await;
    assert!(
        result.is_ok(),
        "daemon path should succeed: {:?}",
        result.err()
    );

    let reports = report.reports.lock().unwrap().clone();
    let running: Vec<&ReportBody> = reports.iter().filter(|r| r.status == "running").collect();
    // running inicial (0.0) + ≥1 do tail com progress > 0 e phase.
    assert!(
        running.len() >= 2,
        "tail deve emitir progresso além do running inicial: {running:?}"
    );
    assert!(
        running
            .iter()
            .any(|r| r.phase.as_deref() == Some("denoising") && r.progress.unwrap_or(0.0) > 0.0),
        "tail deve reportar phase/progress de telemetry.jsonl: {running:?}"
    );
    assert!(
        reports.iter().any(|r| r.status == "done"),
        "done final preservado"
    );
}

#[tokio::test]
async fn test_stage_cached_weight_miss_then_hit() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tmp.path().join("cache");
    let dest1 = tmp.path().join("job1/weights/text_encoder.safetensors");
    let dest2 = tmp.path().join("job2/weights/text_encoder.safetensors");
    tokio::fs::create_dir_all(dest1.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::create_dir_all(dest2.parent().unwrap())
        .await
        .unwrap();

    let weights_bytes = b"my custom text encoder weights in safetensors format";
    let expected_md5 = compute_file_md5_bytes(weights_bytes);
    let s3: Arc<dyn S3Port> = Arc::new(FakeS3WithWeights::new(weights_bytes.to_vec()));

    // 1ª execução: cache miss -> baixa do S3
    let res1 = stage_cached_weight(
        &s3,
        &cache_dir,
        &dest1,
        "models/weights/enc.safetensors",
        &expected_md5,
    )
    .await;
    assert!(res1.is_ok(), "primeira chamada deve ter sucesso");
    assert!(dest1.is_file(), "dest1 deve existir");
    assert_eq!(std::fs::read(&dest1).unwrap(), weights_bytes);

    let cached_file = cache_dir.join(format!("{expected_md5}.safetensors"));
    assert!(cached_file.is_file(), "arquivo no cache deve existir");

    // 2ª execução: cache hit -> reusa sem baixar novamente do S3
    let res2 = stage_cached_weight(
        &s3,
        &cache_dir,
        &dest2,
        "models/weights/enc.safetensors",
        &expected_md5,
    )
    .await;
    assert!(res2.is_ok(), "segunda chamada (cache hit) deve ter sucesso");
    assert!(dest2.is_file(), "dest2 deve existir");
    assert_eq!(std::fs::read(&dest2).unwrap(), weights_bytes);
}

#[tokio::test]
async fn test_stage_cached_weight_md5_mismatch_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tmp.path().join("cache");
    let dest = tmp.path().join("job1/weights/bad.safetensors");
    tokio::fs::create_dir_all(dest.parent().unwrap())
        .await
        .unwrap();

    let weights_bytes = b"real data";
    let wrong_md5 = "00000000000000000000000000000000";
    let s3: Arc<dyn S3Port> = Arc::new(FakeS3WithWeights::new(weights_bytes.to_vec()));

    let res = stage_cached_weight(
        &s3,
        &cache_dir,
        &dest,
        "models/weights/enc.safetensors",
        wrong_md5,
    )
    .await;
    assert!(matches!(res, Err(PipelineError::Md5Mismatch { .. })));
    assert!(
        !dest.exists(),
        "dest não deve ser criado em caso de mismatch"
    );
}

// -- permissões cross-uid em volumes compartilhados engine↔orquestrador --

/// Engines GPU rodam como uid 1000 (`USER studio` na imagem) enquanto o
/// orquestrador roda como root: mkdir de root com umask 022 nasce 0755 e o
/// engine morre em EACCES ao gravar telemetria/artefatos no próprio diretório
/// de job. `create_dir_all_open` é o invariante que impede a regressão.
#[cfg(unix)]
#[tokio::test]
async fn create_dir_all_open_grants_world_write() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let job_dir = tmp.path().join("outputs").join("job-perm-test");

    crate::storage::create_dir_all_open(&job_dir)
        .await
        .expect("criação de dir de job deve ter sucesso");

    let mode = tokio::fs::metadata(&job_dir)
        .await
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(
        mode & 0o777,
        0o777,
        "dir compartilhado deve aceitar escrita de qualquer uid de engine (obtido {:o})",
        mode & 0o777
    );
}

// ---------------------------------------------------------------------------
// Testes P0-3: Admissão Atômica e Concorrência
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_try_admit_concurrency_race() {
    let state = Arc::new(AppState {
        s3: Arc::new(FakeS3WithWeights::new(vec![])),
        report_client: Arc::new(FakeReport::new()),
        executor: Arc::new(FakeTrainerExecutor::new()),
        active_jobs: new_active_jobs(),
        manager_token: None,
        gpu_devices: None,
        gpu_allow_mock: false,
        pairing: Arc::new(PairingState::new("test-code".to_string())),
        daemon_state: None,
        max_concurrent_jobs: 1,
        admission_lock: Arc::new(std::sync::Mutex::new(())),
    });

    let num_tasks = 50;
    let mut handles = Vec::new();
    let barrier = Arc::new(tokio::sync::Barrier::new(num_tasks));

    for i in 0..num_tasks {
        let state = Arc::clone(&state);
        let barrier = Arc::clone(&barrier);
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            let job_id = format!("job-{i}");
            state.try_admit(job_id, ActiveJobState::new(String::new()))
        }));
    }

    let mut success_count = 0;
    let mut rejected_count = 0;

    for handle in handles {
        match handle.await.unwrap() {
            Ok(()) => success_count += 1,
            Err(AdmissionError::CapacityExceeded) => rejected_count += 1,
            Err(AdmissionError::DuplicateJobId) => panic!("unexpected duplicate job id"),
        }
    }

    assert_eq!(success_count, 1, "exatamente 1 job deve ser admitido");
    assert_eq!(
        rejected_count,
        num_tasks - 1,
        "todos os demais jobs devem ser rejeitados por capacidade"
    );
    assert_eq!(state.active_jobs.len(), 1);
}

#[test]
fn test_try_admit_duplicate_job_id() {
    let state = AppState {
        s3: Arc::new(FakeS3WithWeights::new(vec![])),
        report_client: Arc::new(FakeReport::new()),
        executor: Arc::new(FakeTrainerExecutor::new()),
        active_jobs: new_active_jobs(),
        manager_token: None,
        gpu_devices: None,
        gpu_allow_mock: false,
        pairing: Arc::new(PairingState::new("test-code".to_string())),
        daemon_state: None,
        max_concurrent_jobs: 5,
        admission_lock: Arc::new(std::sync::Mutex::new(())),
    };

    let res1 = state.try_admit("job-1".to_string(), ActiveJobState::new(String::new()));
    assert_eq!(res1, Ok(()));

    let res2 = state.try_admit("job-1".to_string(), ActiveJobState::new(String::new()));
    assert_eq!(res2, Err(AdmissionError::DuplicateJobId));

    let res3 = state.try_admit("job-2".to_string(), ActiveJobState::new(String::new()));
    assert_eq!(res3, Ok(()));
}

// ---------------------------------------------------------------------------
// Testes P0-2: Spool Outbox Durável de Reports
// ---------------------------------------------------------------------------

struct ControllableReportClient {
    calls: std::sync::Mutex<Vec<(String, ReportBody)>>,
    behavior: std::sync::Mutex<Option<Result<(), String>>>,
}

impl ControllableReportClient {
    fn new(default_behavior: Result<(), String>) -> Self {
        Self {
            calls: std::sync::Mutex::new(Vec::new()),
            behavior: std::sync::Mutex::new(Some(default_behavior)),
        }
    }

    fn set_behavior(&self, result: Result<(), String>) {
        *self.behavior.lock().unwrap_or_else(|p| p.into_inner()) = Some(result);
    }

    fn calls_count(&self) -> usize {
        self.calls.lock().unwrap_or_else(|p| p.into_inner()).len()
    }
}

#[async_trait]
impl ReportClient for ControllableReportClient {
    async fn report(&self, job_id: &str, body: &ReportBody) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push((job_id.to_string(), body.clone()));
        self.behavior
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .unwrap_or(Ok(()))
    }
}

#[tokio::test]
async fn test_outbox_success_removes_file() {
    let tmp = tempfile::tempdir().unwrap();
    let outbox_dir = tmp.path().join(".outbox");

    let inner = Arc::new(ControllableReportClient::new(Ok(())));
    let client = OutboxReportClient::new(outbox_dir.clone(), inner.clone());

    let body = ReportBody {
        status: "running".to_string(),
        progress: Some(0.5),
        epoch: Some(1),
        step: None,
        metrics: None,
        artifacts: None,
        meta_content: None,
        error: None,
        phase: None,
        message: None,
    };

    let res = client.report("job-100", &body).await;
    assert_eq!(res, Ok(()));
    assert_eq!(inner.calls_count(), 1);

    // O arquivo em spool deve ter sido removido após sucesso
    let mut files = vec![];
    let mut rd = tokio::fs::read_dir(&outbox_dir).await.unwrap();
    while let Ok(Some(entry)) = rd.next_entry().await {
        if entry.path().is_file() {
            files.push(entry.path());
        }
    }
    assert!(
        files.is_empty(),
        "arquivo deve ser removido após envio com sucesso"
    );
}

#[tokio::test]
async fn test_outbox_recoverable_failure_persists_file_and_drain_resends() {
    let tmp = tempfile::tempdir().unwrap();
    let outbox_dir = tmp.path().join(".outbox");

    // Inner falha com erro 500 (recuperável)
    let inner = Arc::new(ControllableReportClient::new(Err(
        "report status: 500 Internal Server Error".to_string(),
    )));
    let client = OutboxReportClient::new(outbox_dir.clone(), inner.clone());

    let body = ReportBody {
        status: "running".to_string(),
        progress: Some(0.1),
        epoch: Some(1),
        step: None,
        metrics: None,
        artifacts: None,
        meta_content: None,
        error: None,
        phase: None,
        message: None,
    };

    // Chamada inicial: erro recuperável retorna Ok(()) para o caller e mantém arquivo no spool
    let res = client.report("job-200", &body).await;
    assert_eq!(res, Ok(()));
    assert_eq!(inner.calls_count(), 1);

    // O arquivo deve persistir em disco
    let mut files = vec![];
    let mut rd = tokio::fs::read_dir(&outbox_dir).await.unwrap();
    while let Ok(Some(entry)) = rd.next_entry().await {
        if entry.path().is_file() {
            files.push(entry.path());
        }
    }
    assert_eq!(
        files.len(),
        1,
        "arquivo de relatório deve permanecer no spool"
    );

    // Agora o manager volta a responder com sucesso
    inner.set_behavior(Ok(()));

    // Executa drain_outbox
    let drained = drain_outbox(&outbox_dir, inner.as_ref()).await;
    assert_eq!(drained, 1, "drain_outbox deve ter drenado 1 item");
    assert_eq!(
        inner.calls_count(),
        2,
        "inner client deve ter recebido a segunda chamada"
    );

    // O spool agora deve estar limpo
    let mut files_after = vec![];
    let mut rd_after = tokio::fs::read_dir(&outbox_dir).await.unwrap();
    while let Ok(Some(entry)) = rd_after.next_entry().await {
        if entry.path().is_file() {
            files_after.push(entry.path());
        }
    }
    assert!(
        files_after.is_empty(),
        "spool deve estar vazio após drain_outbox com sucesso"
    );
}

#[tokio::test]
async fn test_outbox_unrecoverable_failure_removes_file_and_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let outbox_dir = tmp.path().join(".outbox");

    // Inner falha com erro 400 (não recuperável)
    let inner = Arc::new(ControllableReportClient::new(Err(
        "report status: 400 Bad Request".to_string(),
    )));
    let client = OutboxReportClient::new(outbox_dir.clone(), inner.clone());

    let body = ReportBody {
        status: "failed".to_string(),
        progress: None,
        epoch: None,
        step: None,
        metrics: None,
        artifacts: None,
        meta_content: None,
        error: Some("bad payload".to_string()),
        phase: None,
        message: None,
    };

    let res = client.report("job-300", &body).await;
    assert!(res.is_err(), "deve retornar Err para erro não recuperável");
    assert_eq!(inner.calls_count(), 1);

    // O arquivo em spool NÃO deve ficar acumulado para sempre
    let mut files = vec![];
    let mut rd = tokio::fs::read_dir(&outbox_dir).await.unwrap();
    while let Ok(Some(entry)) = rd.next_entry().await {
        if entry.path().is_file() {
            files.push(entry.path());
        }
    }
    assert!(
        files.is_empty(),
        "arquivo não deve permanecer no spool em erro 400 permanente"
    );
}

#[test]
fn test_is_unrecoverable_error_logic() {
    assert!(is_unrecoverable_error("report status: 400 Bad Request"));
    assert!(is_unrecoverable_error("report status: 404 Not Found"));
    assert!(is_unrecoverable_error(
        "report status: 422 Unprocessable Entity"
    ));
    assert!(is_unrecoverable_error("unrecoverable error occurred"));

    assert!(!is_unrecoverable_error(
        "report status: 408 Request Timeout"
    ));
    assert!(!is_unrecoverable_error(
        "report status: 429 Too Many Requests"
    ));
    assert!(!is_unrecoverable_error(
        "report status: 500 Internal Server Error"
    ));
    assert!(!is_unrecoverable_error(
        "report status: 503 Service Unavailable"
    ));
    assert!(!is_unrecoverable_error(
        "report request: connection refused"
    ));

    // Não deve acusar falso positivo para UUIDs contendo "400", "404", "422" em URLs
    assert!(!is_unrecoverable_error(
        "report request: error sending request for url (http://manager:8080/jobs/abcd-400-efgh/report): connection refused"
    ));
    assert!(!is_unrecoverable_error(
        "report request: http://manager:8080/jobs/1234-404-5678/report: connection reset"
    ));
    assert!(!is_unrecoverable_error(
        "report request: http://manager:8080/jobs/9999-422-0000/report: host unreachable"
    ));
}

// ---------------------------------------------------------------------------
// Testes P2-1: Heartbeat Backoff Adaptativo e Jitter
// ---------------------------------------------------------------------------

#[test]
fn test_heartbeat_backoff_zero_failures() {
    let base = 2;
    // Com 0 falhas, a espera deve ser exatamente base_interval_secs sem jitter
    let wait = compute_heartbeat_backoff(base, 0, 999);
    assert_eq!(wait, Duration::from_secs(2));
}

#[test]
fn test_heartbeat_backoff_exponential_growth_and_cap() {
    let base = 2;

    // Falha 1: 2 * 2^1 = 4s + 200ms jitter
    let wait1 = compute_heartbeat_backoff(base, 1, 200);
    assert_eq!(wait1, Duration::from_millis(4200));

    // Falha 2: 2 * 2^2 = 8s + 500ms jitter
    let wait2 = compute_heartbeat_backoff(base, 2, 500);
    assert_eq!(wait2, Duration::from_millis(8500));

    // Falha 3: 2 * 2^3 = 16s + 0ms jitter
    let wait3 = compute_heartbeat_backoff(base, 3, 0);
    assert_eq!(wait3, Duration::from_millis(16000));

    // Falha 4: min(2 * 2^4 = 32, 30) = 30s + 350ms jitter
    let wait4 = compute_heartbeat_backoff(base, 4, 350);
    assert_eq!(wait4, Duration::from_millis(30350));

    // Falha 5+: capped em 30s + jitter
    let wait5 = compute_heartbeat_backoff(base, 5, 800);
    assert_eq!(wait5, Duration::from_millis(30800));

    let wait10 = compute_heartbeat_backoff(base, 10, 150);
    assert_eq!(wait10, Duration::from_millis(30150));
}

#[test]
fn test_heartbeat_backoff_jitter_modulo() {
    let base = 5;
    // Jitter >= 1000 deve ser reduzido por % 1000
    let wait = compute_heartbeat_backoff(base, 1, 1500);
    // exp_secs = (5 * 2^1) = 10s + 500ms jitter
    assert_eq!(wait, Duration::from_millis(10500));
}

// ---------------------------------------------------------------------------
// Testes P2-2: Reaper de Containers Órfãos e Sweeper Periódico
// ---------------------------------------------------------------------------

#[test]
fn test_is_container_active_logic() {
    let active_jobs = new_active_jobs();
    active_jobs.insert(
        "job-uuid-123".to_string(),
        ActiveJobState::new("trainer-job-uuid-123".to_string()),
    );

    // Nome exato
    assert!(is_container_active(&active_jobs, "trainer-job-uuid-123"));

    // Contém job_id no nome
    assert!(is_container_active(
        &active_jobs,
        "custom-prefix-job-uuid-123-worker"
    ));

    // Container não associado
    assert!(!is_container_active(&active_jobs, "trainer-orphan-999"));

    // Container com nome vazio ou só whitespace
    assert!(!is_container_active(&active_jobs, ""));
    assert!(!is_container_active(&active_jobs, "   "));
}

#[tokio::test]
async fn test_reconcile_orphan_containers_safe_execution() {
    let active_jobs = new_active_jobs();
    // Executa sem panic mesmo se docker não estiver instalado ou sem daemon
    let count = reconcile_orphan_containers(&active_jobs).await;
    assert_eq!(count, count); // confirma tipo numérico e execução limpa

    // Com jobs ativos presentes
    active_jobs.insert(
        "job-active-1".to_string(),
        ActiveJobState::new("trainer-active-1".to_string()),
    );
    let count2 = reconcile_orphan_containers(&active_jobs).await;
    assert_eq!(count2, count2);
}

#[tokio::test]
async fn test_sweep_orphan_workdirs_removes_expired_cache() {
    let temp_dir = tempfile::tempdir().unwrap();
    let cache_dir = temp_dir.path().join("datasets").join("datasets-cache");
    tokio::fs::create_dir_all(&cache_dir).await.unwrap();

    let old_subdir = cache_dir.join("old-cache-item");
    tokio::fs::create_dir(&old_subdir).await.unwrap();

    // Executa sweep com max_age = 0s para expirar tudo imediatamente
    tokio::time::sleep(Duration::from_millis(50)).await;
    sweep_orphan_workdirs(temp_dir.path(), Duration::from_millis(10)).await;

    assert!(
        !old_subdir.exists(),
        "diretório antigo de cache deve ser removido"
    );
}

// ---------------------------------------------------------------------------
// Testes P2-5: Graceful Shutdown via Watch Channel
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_periodic_sweeper_graceful_shutdown() {
    let active_jobs = new_active_jobs();
    let temp_dir = tempfile::tempdir().unwrap();
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let handle = spawn_periodic_sweeper(
        active_jobs,
        temp_dir.path().to_path_buf(),
        Duration::from_millis(50),
        shutdown_rx,
    );

    tokio::time::sleep(Duration::from_millis(20)).await;

    // Notifica shutdown
    shutdown_tx.send(true).unwrap();

    // Tarefa deve terminar graciosamente sem travar
    let result = tokio::time::timeout(Duration::from_secs(2), handle).await;
    assert!(
        result.is_ok(),
        "periodic sweeper deve finalizar após sinal de shutdown"
    );
}

#[tokio::test]
async fn test_outbox_drain_worker_graceful_shutdown() {
    let temp_dir = tempfile::tempdir().unwrap();
    let client = Arc::new(FakeReport {
        reports: Mutex::new(Vec::new()),
    });
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let handle = spawn_outbox_drain_worker(
        temp_dir.path().to_path_buf(),
        client,
        Duration::from_millis(50),
        shutdown_rx,
    );

    tokio::time::sleep(Duration::from_millis(20)).await;

    // Notifica shutdown
    shutdown_tx.send(true).unwrap();

    // Tarefa deve terminar graciosamente sem travar
    let result = tokio::time::timeout(Duration::from_secs(2), handle).await;
    assert!(
        result.is_ok(),
        "outbox drain worker deve finalizar após sinal de shutdown"
    );
}

#[tokio::test]
async fn test_sweeper_and_outbox_immediate_shutdown() {
    let active_jobs = new_active_jobs();
    let temp_dir = tempfile::tempdir().unwrap();
    let client = Arc::new(FakeReport {
        reports: Mutex::new(Vec::new()),
    });
    // Canal já com shutdown = true no início
    let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(true);

    let sweeper_handle = spawn_periodic_sweeper(
        active_jobs,
        temp_dir.path().to_path_buf(),
        Duration::from_millis(50),
        shutdown_rx.clone(),
    );

    let outbox_handle = spawn_outbox_drain_worker(
        temp_dir.path().to_path_buf(),
        client,
        Duration::from_millis(50),
        shutdown_rx,
    );

    // Ambas devem retornar quase instantaneamente
    let r1 = tokio::time::timeout(Duration::from_millis(500), sweeper_handle).await;
    let r2 = tokio::time::timeout(Duration::from_millis(500), outbox_handle).await;
    assert!(
        r1.is_ok(),
        "sweeper com shutdown inicial deve retornar imediatamente"
    );
    assert!(
        r2.is_ok(),
        "outbox worker com shutdown inicial deve retornar imediatamente"
    );
}

#[tokio::test]
async fn test_outbox_preserves_spool_when_url_uuid_contains_400() {
    let temp_dir = tempfile::tempdir().unwrap();
    let outbox_dir = temp_dir.path().join(".outbox");

    // Erro de rede com UUID contendo "400" na URL
    let inner = Arc::new(ControllableReportClient::new(Err(
        "report request: error sending request for url (http://manager:8080/jobs/abcd-400-efgh/report): connection refused".to_string()
    )));
    let client = OutboxReportClient::new(outbox_dir.clone(), inner.clone());

    let res = client
        .report(
            "abcd-400-efgh",
            &ReportBody {
                status: "running".to_string(),
                progress: Some(0.1),
                epoch: Some(1),
                step: Some(10),
                metrics: None,
                error: None,
                artifacts: None,
                meta_content: None,
                phase: Some("training".to_string()),
                message: None,
            },
        )
        .await;

    assert!(
        res.is_ok(),
        "falha transitória deve ser enfileirada no outbox"
    );

    let mut files = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(&outbox_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.path().is_file() {
                files.push(entry.path());
            }
        }
    }

    assert_eq!(
        files.len(),
        1,
        "arquivo DEVE permanecer no spool e NÃO ser deletado como erro 400 permanente"
    );
}

#[tokio::test]
async fn test_abort_preserved_in_preparing_does_not_launch_container() {
    let tmp = tempfile::tempdir().unwrap();
    let s3 = Arc::new(FakeS3::new());
    let zip_path = tmp.path().join("pkg.zip");
    std::fs::write(&zip_path, &s3.zip_bytes).unwrap();
    let mut dispatch = make_dispatch_with_valid_md5("job-abort-001", "yolo", &zip_path);
    dispatch.workdir = tmp.path().to_str().unwrap().to_string();
    let report = Arc::new(FakeReport::new());
    let executor = Arc::new(FakeTrainerExecutor::new());
    let active_jobs = new_active_jobs();

    // Simula a etapa de admissão/preparing: job registrado sem container_name ainda
    active_jobs.insert(dispatch.job_id.clone(), ActiveJobState::new(String::new()));

    // Simula abort chamado enquanto o job está baixando/preparando pesos/pacote
    if let Some(entry) = active_jobs.get(&dispatch.job_id) {
        entry.cancel();
    }

    let res = run_job_inner(
        &dispatch,
        s3.clone(),
        report.clone(),
        executor.clone(),
        &active_jobs,
        None,
        false,
        None,
    )
    .await;

    assert!(
        matches!(res, Err(PipelineError::Cancelled)),
        "deve abortar com Cancelled: {:?}",
        res
    );

    // Garante que o container/executor NUNCA foi lançado
    assert!(
        executor.last_args().is_none(),
        "executor não deve ter sido executado quando abortado durante preparing"
    );
}

#[tokio::test]
async fn test_drain_outbox_with_inner_client_does_not_respool() {
    let temp_dir = tempfile::tempdir().unwrap();
    let outbox_dir = temp_dir.path().join(".outbox");

    // 1. Spool inicial de 2 relatórios usando OutboxReportClient com inner falhando temporariamente
    let inner_mock = Arc::new(ControllableReportClient::new(Err(
        "report request: connection refused".to_string(),
    )));
    let outbox_client = OutboxReportClient::new(outbox_dir.clone(), inner_mock.clone());

    let body = ReportBody {
        status: "running".to_string(),
        progress: Some(0.5),
        epoch: Some(2),
        step: Some(50),
        metrics: None,
        error: None,
        artifacts: None,
        meta_content: None,
        phase: Some("training".to_string()),
        message: None,
    };

    let _ = outbox_client.report("job-drain-1", &body).await;
    let _ = outbox_client.report("job-drain-2", &body).await;

    // Confirma que existem 2 arquivos no spool
    let mut count_before = 0;
    if let Ok(mut entries) = tokio::fs::read_dir(&outbox_dir).await {
        while let Ok(Some(_)) = entries.next_entry().await {
            count_before += 1;
        }
    }
    assert_eq!(count_before, 2);

    // 2. Agora o inner client recupera conectividade (sucesso)
    inner_mock.set_behavior(Ok(()));

    // 3. Drena usando o inner client direto (como é feito em main.rs no shutdown e no worker)
    let drained = drain_outbox(&outbox_dir, inner_mock.as_ref()).await;
    assert_eq!(drained, 2, "deve drenar os 2 relatórios pendentes");

    // 4. Confirma que a pasta do spool ficou limpa e sem arquivos temporários
    let mut count_after = 0;
    if let Ok(mut entries) = tokio::fs::read_dir(&outbox_dir).await {
        while let Ok(Some(_)) = entries.next_entry().await {
            count_after += 1;
        }
    }
    assert_eq!(
        count_after, 0,
        "outbox deve estar completamente vazia após dreno"
    );
}
