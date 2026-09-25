//! Casos de uso e orquestração de pipelines (Fatia 6).
//!
//! Compõe os estágios cirúrgicos de `stages::*`: o pipeline monolítico
//! `run_job_inner` vive aqui, delgado, delegando coleção de artefatos,
//! staging de pesos, templating de config e ramificação de subcomando.

pub mod stages;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::daemon;
use crate::domain::errors::PipelineError;
use crate::domain::models::{ArtifactReport, DispatchRequest, ReportBody};
use crate::ports::executor::TrainerExecutor;
use crate::ports::reporter::ReportClient;
use crate::ports::storage::S3Port;
use crate::storage::{
    compute_file_md5, create_dir_all_open, init_image_ext, put_with_retry, scoped_init_image_key,
    scoped_key, unzip_safe, S3Scope,
};
use crate::{tail_jsonl_lines, telemetry_report_for_line};

use stages::collector::{
    collect_diffusion_artifacts, read_final_metrics, read_generation_meta_content,
    stream_metrics_and_samples, upload_telemetry_snapshot,
};
use stages::config::{extract_epochs, replace_config_placeholders};
use stages::execute::resolve_subcommand_args;
use stages::weights::resolve_and_stage_weight;

// ---------------------------------------------------------------------------
// Active jobs tracking (para abort — D4)
// ---------------------------------------------------------------------------

pub struct ActiveJobState {
    pub container_name: String,
    pub cancelled: AtomicBool,
}

impl ActiveJobState {
    pub fn new(container_name: String) -> Self {
        Self {
            container_name,
            cancelled: AtomicBool::new(false),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

pub type ActiveJobs = Arc<dashmap::DashMap<String, ActiveJobState>>;

/// Cria uma nova instância de ActiveJobs.
pub fn new_active_jobs() -> ActiveJobs {
    Arc::new(dashmap::DashMap::new())
}

// ---------------------------------------------------------------------------
// Job pipeline (§11/:262–270, D5/D6/D8)
// ---------------------------------------------------------------------------

/// Executa o pipeline completo de um job (async, chamado como task).
///
/// 1. Reporta `preparing`
/// 2. Baixa package.zip via S3 (scoped), valida md5
/// 3. Descompacta (zip-slip safe)
/// 4. Monta config.yaml com paths reais
/// 5. Reporta `running`
/// 6. Executa trainer (docker ou subprocess)
/// 7. Coleta métricas incrementalmente
/// 8. Sobe artefatos ao bucket
/// 9. Reporta `done` ou `failed`
/// 10. Limpa tempdir
pub async fn run_job(
    dispatch: DispatchRequest,
    s3: Arc<dyn S3Port>,
    report_client: Arc<dyn ReportClient>,
    executor: Arc<dyn TrainerExecutor>,
    active_jobs: ActiveJobs,
    gpu_devices: Option<String>,
    gpu_allow_mock: bool,
    daemon_state: Option<Arc<daemon::DaemonState>>,
) {
    let job_id = dispatch.job_id.clone();
    let report_for_error = Arc::clone(&report_client);
    let result = run_job_inner(
        &dispatch,
        s3,
        report_client,
        executor,
        &active_jobs,
        gpu_devices.as_deref(),
        gpu_allow_mock,
        daemon_state.as_deref(),
    )
    .await;

    if let Err(err) = result {
        let is_cancelled = matches!(err, PipelineError::Cancelled);
        let terminal_status = if is_cancelled { "cancelled" } else { "failed" };
        let terminal_phase = if is_cancelled { "cancelled" } else { "error" };
        let err_msg = err.to_string();
        if let Err(report_err) = report_for_error
            .report(
                &job_id,
                &ReportBody {
                    status: terminal_status.to_string(),
                    progress: None,
                    epoch: None,
                    step: None,
                    metrics: None,
                    error: Some(err_msg.clone()),
                    artifacts: None,
                    meta_content: None,
                    phase: Some(terminal_phase.to_string()),
                    message: Some(if is_cancelled {
                        "Job cancelado pelo usuário".to_string()
                    } else {
                        err_msg
                    }),
                },
            )
            .await
        {
            tracing::warn!(
                job_id = %job_id,
                report_error = %report_err,
                "falha ao reportar erro terminal do job"
            );
        }
    }

    // Cleanup pós-job do cache do dataset descompactado:
    let job_workdir = std::path::PathBuf::from(&dispatch.workdir);
    let dataset_dir = job_workdir
        .join("datasets")
        .join("datasets-cache")
        .join(&job_id);
    let _ = tokio::fs::remove_dir_all(&dataset_dir).await;
    active_jobs.remove(&job_id);
}

pub async fn run_job_inner(
    dispatch: &DispatchRequest,
    s3: Arc<dyn S3Port>,
    report_client: Arc<dyn ReportClient>,
    executor: Arc<dyn TrainerExecutor>,
    active_jobs: &ActiveJobs,
    gpu_devices: Option<&str>,
    gpu_allow_mock: bool,
    daemon_state: Option<&daemon::DaemonState>,
) -> Result<(), PipelineError> {
    let job_id = &dispatch.job_id;
    let job_workdir = PathBuf::from(&dispatch.workdir);

    // paths internos = mounts do compose (/data/datasets, /data/outputs);
    // envs ORCH_VOL_* = NOME do volume docker (com prefixo do projeto) para o docker run.
    let vol_datasets =
        std::env::var("ORCH_VOL_DATASETS").unwrap_or_else(|_| "infra_datasets".into());
    let vol_outputs = std::env::var("ORCH_VOL_OUTPUTS").unwrap_or_else(|_| "infra_outputs".into());

    // Cria diretórios de trabalho — paths FIXOS no filesystem do orquestrador.
    // O compose monta infra_datasets em /data/datasets e infra_outputs em /data/outputs.
    let datasets_cache = job_workdir
        .join("datasets")
        .join("datasets-cache")
        .join(job_id);
    let outputs = job_workdir.join("outputs").join(job_id);
    let temp_dir = job_workdir.join("tmp").join(job_id);
    let weights_cache_dir = job_workdir.join("outputs").join(".weights-cache");
    create_dir_all_open(&datasets_cache).await?;
    create_dir_all_open(&outputs).await?;
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .map_err(|e| PipelineError::Other(format!("create temp: {e}")))?;

    // Guarda anti-mock (D2): fail-fast antes de downloads/reports.
    if gpu_devices.is_some() && !gpu_allow_mock {
        if dispatch.image.ends_with(":local") {
            return Err(PipelineError::GpuImageGuard {
                image: dispatch.image.clone(),
            });
        }
    }

    // 1. Report preparing
    report_client
        .report(
            job_id,
            &ReportBody {
                status: "preparing".to_string(),
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
        .map_err(|e| PipelineError::ReportFailed(format!("report preparing: {e}")))?;

    // Preempção: ANTES de despachar treino, se daemon idle → kill (D1)
    if dispatch.mode != "generate" {
        if let Some(ds) = daemon_state {
            daemon::maybe_preempt_daemon(ds).await;
        }
    }

    fn make_progress_reporter(
        report_client: &Arc<dyn ReportClient>,
        job_id: &str,
        phase: &'static str,
        prefix_msg: &'static str,
        base_progress: f64,
        progress_span: f64,
    ) -> impl Fn(u64, Option<u64>) + Send + Sync + 'static {
        let rc = Arc::clone(report_client);
        let jid = job_id.to_string();
        move |downloaded: u64, total: Option<u64>| {
            let dl_mb = downloaded as f64 / (1024.0 * 1024.0);
            let (msg, prog) = if let Some(tot) = total {
                let tot_mb = tot as f64 / (1024.0 * 1024.0);
                let pct = if tot > 0 {
                    (downloaded as f64 / tot as f64).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (
                    format!(
                        "{prefix_msg} ({:.1} MB / {:.1} MB · {:.0}%)...",
                        dl_mb,
                        tot_mb,
                        pct * 100.0
                    ),
                    base_progress + progress_span * pct,
                )
            } else {
                (
                    format!("{prefix_msg} ({:.1} MB)...", dl_mb),
                    base_progress + 0.01,
                )
            };
            let rc_spawn = Arc::clone(&rc);
            let jid_spawn = jid.clone();
            tracing::info!(job_id = %jid_spawn, phase = %phase, "{msg}");
            tokio::spawn(async move {
                let _ = rc_spawn
                    .report(
                        &jid_spawn,
                        &ReportBody {
                            status: "running".to_string(),
                            progress: Some(prog),
                            epoch: None,
                            step: None,
                            metrics: None,
                            error: None,
                            artifacts: None,
                            meta_content: None,
                            phase: Some(phase.to_string()),
                            message: Some(msg),
                        },
                    )
                    .await;
            });
        }
    }

    // 2. Download package.zip via S3 (scoped — D2 barreira principal) se presente
    if let Some(pr) = dispatch.package_ref.as_ref() {
        let zip_path = temp_dir.join("dataset.zip");
        let key = scoped_key(S3Scope::Packages, &pr.key)
            .map_err(|e| PipelineError::S3Download(format!("invalid package key: {e}")))?;

        let on_dl = make_progress_reporter(
            &report_client,
            job_id,
            "downloading_dataset",
            "Baixando dataset",
            0.01,
            0.05,
        );
        tracing::info!(job_id = %job_id, key = %key, "Iniciando download do dataset...");
        s3.get_to_file_with_progress(&key, &zip_path, Some(&on_dl))
            .await
            .map_err(|e| PipelineError::S3Download(format!("download package: {e}")))?;

        // 3. Verify MD5 (crash do job se divergir — D4)
        let _ = report_client
            .report(
                job_id,
                &ReportBody {
                    status: "running".to_string(),
                    progress: Some(0.06),
                    epoch: None,
                    step: None,
                    metrics: None,
                    error: None,
                    artifacts: None,
                    meta_content: None,
                    phase: Some("downloading_dataset".to_string()),
                    message: Some("Validando integridade do dataset (MD5)...".to_string()),
                },
            )
            .await;
        let actual_md5 = compute_file_md5(&zip_path)
            .map_err(|e| PipelineError::S3Download(format!("compute md5: {e}")))?;
        if actual_md5 != pr.md5_zip {
            return Err(PipelineError::Md5Mismatch {
                expected: pr.md5_zip.clone(),
                actual: actual_md5,
            });
        }
        tracing::info!(job_id = %job_id, md5 = %actual_md5, "Integridade do dataset validada com sucesso");

        // 4. Unzip (zip-slip safe, padrão import 3e)
        let _ = report_client
            .report(
                job_id,
                &ReportBody {
                    status: "running".to_string(),
                    progress: Some(0.07),
                    epoch: None,
                    step: None,
                    metrics: None,
                    error: None,
                    artifacts: None,
                    meta_content: None,
                    phase: Some("extracting_dataset".to_string()),
                    message: Some("Descompactando dataset no cache do nó...".to_string()),
                },
            )
            .await;
        tracing::info!(job_id = %job_id, "Descompactando dataset no cache do nó...");
        unzip_safe(&zip_path, &datasets_cache)?;
    }

    // 5. Download e staging de pesos (fine-tune — ADR-0012 D5)
    //    Pesos ficam em outputs/<job_id>/weights/<filename> (volume outputs já montado).
    let mut weights_staged_path: Option<String> = None;
    if let Some(weights_ref) = dispatch.weights_ref.as_ref() {
        // Extrai filename do path (models/yolo/<id>/best.pt → best.pt)
        let filename = weights_ref.s3_key.rsplit('/').next().ok_or_else(|| {
            PipelineError::S3Download("weights_ref key has no filename".to_string())
        })?;

        let weights_dir = outputs.join("weights");
        create_dir_all_open(&weights_dir).await?;

        let weights_file = weights_dir.join(filename);
        let on_w = make_progress_reporter(
            &report_client,
            job_id,
            "downloading_weights",
            "Baixando pesos do modelo",
            0.07,
            0.03,
        );
        resolve_and_stage_weight(
            &s3,
            &weights_cache_dir,
            &weights_file,
            &weights_ref.s3_key,
            &weights_ref.md5,
            Some(&on_w),
        )
        .await?;
        weights_staged_path = Some(format!("/outputs/{job_id}/weights/{filename}"));
    }

    // 5b. Download e staging de LoRAs multi-ref (D3 — ADR-0023)
    //     Pesos ficam em outputs/<job_id>/weights/lora_0.safetensors, lora_1.safetensors, ...
    let mut lora_staged_paths: Vec<String> = Vec::new();
    let weights_dir = outputs.join("weights");
    if !dispatch.loras.is_empty() {
        create_dir_all_open(&weights_dir).await?;
    }
    for (i, lora) in dispatch.loras.iter().enumerate() {
        let lora_file = weights_dir.join(format!("lora_{i}.safetensors"));
        resolve_and_stage_weight(
            &s3,
            &weights_cache_dir,
            &lora_file,
            &lora.s3_key,
            &lora.md5,
            None,
        )
        .await?;
        lora_staged_paths.push(format!("/outputs/{job_id}/weights/lora_{i}.safetensors"));
    }

    // 5c. Download e staging de custom checkpoint (D4 — ADR-0023)
    //     Pesos ficam em outputs/<job_id>/weights/custom.safetensors
    let mut custom_staged_path: Option<String> = None;
    if let Some(custom) = dispatch.custom_checkpoint.as_ref() {
        create_dir_all_open(&weights_dir).await?;
        let custom_file = weights_dir.join("custom.safetensors");
        let on_c = make_progress_reporter(
            &report_client,
            job_id,
            "downloading_weights",
            "Baixando checkpoint custom",
            0.07,
            0.03,
        );
        resolve_and_stage_weight(
            &s3,
            &weights_cache_dir,
            &custom_file,
            &custom.s3_key,
            &custom.md5,
            Some(&on_c),
        )
        .await?;
        custom_staged_path = Some(format!("/outputs/{job_id}/weights/custom.safetensors"));
    }
    // 5c2. Download e staging do text encoder custom (fatia feat/pesos-custom-flux2).
    //     Fica em outputs/<job_id>/weights/text_encoder.safetensors (mesmo
    //     mecanismo weights_ref: escopo models/|artifacts/, md5 obrigatório,
    //     falha honesta em qualquer etapa — nunca fallback silencioso p/ o oficial).
    let mut text_encoder_staged_path: Option<String> = None;
    if let Some(encoder) = dispatch.text_encoder.as_ref() {
        create_dir_all_open(&weights_dir).await?;
        let encoder_file = weights_dir.join("text_encoder.safetensors");
        resolve_and_stage_weight(
            &s3,
            &weights_cache_dir,
            &encoder_file,
            &encoder.s3_key,
            &encoder.md5,
            None,
        )
        .await?;
        text_encoder_staged_path = Some(format!(
            "/outputs/{job_id}/weights/text_encoder.safetensors"
        ));
    }

    // 5d. Download e staging da imagem inicial img2img (S4 — feat/img2img).
    //     Fica em outputs/<job_id>/inputs/init.<ext> (ext sanitizada do s3_key,
    //     fallback `png`). O path REAL do host entra no real_config — o
    //     config.yaml já é montado via volume /outputs, como weights/custom.
    let mut init_staged_path: Option<String> = None;
    if let Some(init) = dispatch.init_image_ref.as_ref() {
        let scoped_ikey = scoped_init_image_key(&init.s3_key)
            .map_err(|e| PipelineError::S3Download(format!("invalid init_image_ref key: {e}")))?;
        let inputs_dir = outputs.join("inputs");
        create_dir_all_open(&inputs_dir).await?;
        let ext = init_image_ext(&init.s3_key);
        let init_file = inputs_dir.join(format!("init.{ext}"));
        s3.get_to_file(&scoped_ikey, &init_file)
            .await
            .map_err(|e| PipelineError::S3Download(format!("download init image: {e}")))?;
        let actual_md5 = compute_file_md5(&init_file)
            .map_err(|e| PipelineError::S3Download(format!("compute init image md5: {e}")))?;
        match init.md5.as_deref() {
            Some(expected) => {
                if actual_md5 != expected {
                    return Err(PipelineError::Md5Mismatch {
                        expected: expected.to_string(),
                        actual: actual_md5,
                    });
                }
            }
            // Origem galeria: sem hash de referência — só registra o calculado.
            None => {
                tracing::info!(
                    "init image from gallery has no expected md5, staged md5={actual_md5}"
                );
            }
        }
        init_staged_path = Some(format!("/outputs/{job_id}/inputs/init.{ext}"));
    }
    // 5e. Download e staging do dataset de controle/regularização (treino difusão).
    //     Mesmo caminho do pacote principal: zip no escopo Packages + md5_zip
    //     obrigatório, extraído (zip-slip safe) para datasets-cache/<job_id>/control.
    //     O path REAL do host entra no real_config via {control_dataset_path}.
    //     Falha em qualquer etapa = job falha honesto (S3Download/Md5Mismatch/UnzipFailed).
    let mut control_staged_path: Option<String> = None;
    if let Some(control) = dispatch.control_package_ref.as_ref() {
        let control_zip = temp_dir.join("control.zip");
        let control_key = scoped_key(S3Scope::Packages, &control.key)
            .map_err(|e| PipelineError::S3Download(format!("invalid control package key: {e}")))?;
        s3.get_to_file(&control_key, &control_zip)
            .await
            .map_err(|e| PipelineError::S3Download(format!("download control package: {e}")))?;
        let actual_md5 = compute_file_md5(&control_zip)
            .map_err(|e| PipelineError::S3Download(format!("compute control md5: {e}")))?;
        if actual_md5 != control.md5_zip {
            return Err(PipelineError::Md5Mismatch {
                expected: control.md5_zip.clone(),
                actual: actual_md5,
            });
        }
        let control_dir = datasets_cache.join("control");
        create_dir_all_open(&control_dir).await?;
        unzip_safe(&control_zip, &control_dir)?;
        control_staged_path = Some(format!("/datasets/datasets-cache/{job_id}/control"));
    }

    // 6. Monta config.yaml REAL — substitui placeholders (§8/:102)
    let total_epochs = dispatch
        .config_yaml
        .as_deref()
        .map(extract_epochs)
        .unwrap_or(100);

    if let Some(config_yaml) = dispatch.config_yaml.as_ref() {
        // Dentro do container trainer: /datasets/datasets-cache/<job_id> e /outputs/<job_id>
        // (via -v volumes nomeados montados no compose).
        let dataset_path = format!("/datasets/datasets-cache/{job_id}");
        let output_path = format!("/outputs/{job_id}");
        let real_config = replace_config_placeholders(
            config_yaml,
            &dataset_path,
            &output_path,
            weights_staged_path.as_deref(),
            &lora_staged_paths,
            custom_staged_path.as_deref(),
            init_staged_path.as_deref(),
            control_staged_path.as_deref(),
            text_encoder_staged_path.as_deref(),
        );

        // O config exige init mas nenhum init_image_ref veio no dispatch:
        // falha explícita em vez de deixar o placeholder vazar (S4).
        if real_config.contains("{init_image_path}") {
            return Err(PipelineError::ConfigYamlInvalid(
                "config.yaml requires {init_image_path} but no init_image_ref was provided"
                    .to_string(),
            ));
        }

        // O config exige control mas nenhum control_package_ref veio no dispatch:
        // falha explícita em vez de deixar o placeholder vazar (espelha S4).
        if real_config.contains("{control_dataset_path}") {
            return Err(PipelineError::ConfigYamlInvalid(
                "config.yaml requires {control_dataset_path} but no control_package_ref was provided"
                    .to_string(),
            ));
        }

        // O config exige text encoder custom mas nenhum veio no dispatch:
        // falha explícita em vez de vazar o placeholder ou cair no oficial
        // (fallback silencioso proibido — fatia feat/pesos-custom-flux2).
        if real_config.contains("{text_encoder_path}") {
            return Err(PipelineError::ConfigYamlInvalid(
                "config.yaml requires {text_encoder_path} but no text_encoder was provided"
                    .to_string(),
            ));
        }

        // Valida que é YAML parseável (D6)
        let _: serde_yaml::Value = serde_yaml::from_str(&real_config).map_err(|e| {
            PipelineError::ConfigYamlInvalid(format!("config.yaml parse error: {e}"))
        })?;

        // Escreve no output_path (trainer lê de lá)
        let config_path = outputs.join("config.yaml");
        tokio::fs::write(&config_path, &real_config)
            .await
            .map_err(|e| PipelineError::ConfigYamlInvalid(format!("write config.yaml: {e}")))?;

        // Grava training_config.json para reproducibilidade e download pelo usuário (apenas treino de difusão)
        if dispatch.engine == "diffusion" && dispatch.mode == "train" {
            if let Ok(json_val) = serde_yaml::from_str::<serde_json::Value>(&real_config) {
                if let Ok(json_str) = serde_json::to_string_pretty(&json_val) {
                    let _ = tokio::fs::write(outputs.join("training_config.json"), json_str).await;
                }
            }
        }
    }

    // 6. Report running
    report_client
        .report(
            job_id,
            &ReportBody {
                status: "running".to_string(),
                progress: Some(0.0),
                epoch: Some(0),
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
        .map_err(|e| PipelineError::ReportFailed(format!("report running: {e}")))?;

    // =========================================================================
    // DAEMON PATH: Diffusion generate com daemon habilitado (D1)
    // =========================================================================
    if dispatch.engine == "diffusion" && dispatch.mode == "generate" && daemon_state.is_some() {
        let ds = daemon_state.unwrap();

        // Deriva spec-alvo do config yaml (D1: loaded_spec from config)
        let _target_spec = dispatch.config_yaml.as_deref().unwrap_or("default");

        // (a) Se daemon não está de pé → sobe, aguarda /health 200
        let _daemon_url = daemon::ensure_daemon_ready(ds, _target_spec)
            .await
            .map_err(|e| PipelineError::DaemonLaunchFailed(e))?;

        // Telemetry path (D1)
        let telemetry_abs = outputs.join("telemetry.jsonl");

        // Config yaml como string JSON para o daemon — usa o real_config
        // (mesmo config com placeholders substituídos que o one-shot grava em config.yaml)
        let config_str = dispatch
            .config_yaml
            .as_ref()
            .map(|cy| {
                replace_config_placeholders(
                    cy,
                    &format!("/datasets/datasets-cache/{job_id}"),
                    &format!("/outputs/{job_id}"),
                    weights_staged_path.as_deref(),
                    &lora_staged_paths,
                    custom_staged_path.as_deref(),
                    init_staged_path.as_deref(),
                    control_staged_path.as_deref(),
                    text_encoder_staged_path.as_deref(),
                )
            })
            .unwrap_or_default();

        // Mesmo guarda do one-shot: placeholder de init sem init_image_ref
        // falha explícita em vez de vazar para o daemon (S4).
        if config_str.contains("{init_image_path}") {
            return Err(PipelineError::ConfigYamlInvalid(
                "config.yaml requires {init_image_path} but no init_image_ref was provided"
                    .to_string(),
            ));
        }

        // Defesa anti-placeholder do control (espelha a de init): config exige
        // control mas nenhum control_package_ref veio no dispatch.
        if config_str.contains("{control_dataset_path}") {
            return Err(PipelineError::ConfigYamlInvalid(
                "config.yaml requires {control_dataset_path} but no control_package_ref was provided"
                    .to_string(),
            ));
        }

        // Defesa anti-placeholder do text encoder (fatia feat/pesos-custom-flux2).
        if config_str.contains("{text_encoder_path}") {
            return Err(PipelineError::ConfigYamlInvalid(
                "config.yaml requires {text_encoder_path} but no text_encoder was provided"
                    .to_string(),
            ));
        }

        let body = daemon::GenerateBody {
            config: config_str,
            output_dir: outputs.to_str().unwrap_or_default().to_string(),
            telemetry_path: telemetry_abs.to_str().unwrap_or_default().to_string(),
        };

        // (c) Tail de telemetry.jsonl durante o POST /generate: o HTTP retorna
        // só no 200 (fim da geração), então sem tail o job fica em 0.0 até
        // done. Mesmos events/phases do one-shot: a task lê telemetry.jsonl
        // via `tail_jsonl_lines` e reporta cada linha com
        // `telemetry_report_for_line` (== formato do collector do one-shot).
        // A task só observa o arquivo, nunca toca no client/launcher.
        let telemetry_report_client = Arc::clone(&report_client);
        let telemetry_path_clone = telemetry_abs.clone();
        let telemetry_job_id = job_id.clone();
        // C2a: upload periódico do snapshot de logs também no caminho daemon
        // (a cada ~5s de ticks de 500ms, só quando o arquivo cresce; o report
        // do artefato é anunciado uma única vez — dedupe por path no manager).
        let telemetry_s3 = Arc::clone(&s3);
        let telemetry_handle = tokio::spawn(async move {
            let mut lines_read: usize = 0;
            let mut interval = tokio::time::interval(Duration::from_millis(500));
            let mut upload_ticks: u32 = 0;
            let mut telemetry_uploaded_bytes: i64 = 0;
            let mut telemetry_artifact_reported = false;
            loop {
                interval.tick().await;
                let (new_lines, new_offset) = tail_jsonl_lines(&telemetry_path_clone, lines_read);
                lines_read = new_offset;
                for m in new_lines {
                    let body = telemetry_report_for_line(&m, total_epochs);
                    let _ = telemetry_report_client
                        .report(&telemetry_job_id, &body)
                        .await;
                }
                upload_ticks += 1;
                if upload_ticks >= 10 {
                    upload_ticks = 0;
                    if let Ok(meta) = std::fs::metadata(&telemetry_path_clone) {
                        let size = meta.len() as i64;
                        if size > telemetry_uploaded_bytes {
                            if let Some(rep) = upload_telemetry_snapshot(
                                &telemetry_s3,
                                &telemetry_job_id,
                                &telemetry_path_clone,
                            )
                            .await
                            {
                                telemetry_uploaded_bytes = size;
                                if !telemetry_artifact_reported {
                                    telemetry_artifact_reported = true;
                                    let _ = telemetry_report_client
                                        .report(
                                            &telemetry_job_id,
                                            &ReportBody {
                                                status: "running".to_string(),
                                                progress: None,
                                                epoch: None,
                                                step: None,
                                                metrics: None,
                                                error: None,
                                                artifacts: Some(vec![rep]),
                                                meta_content: None,
                                                phase: None,
                                                message: None,
                                            },
                                        )
                                        .await;
                                }
                            }
                        }
                    }
                }
            }
        });

        // POST /generate com retry em 409 (D1: 2 retries com backoff curto).
        // O tail acima emite progresso enquanto este await bloqueia.
        let mut last_err = String::new();
        let mut succeeded = false;
        for attempt in 0..3 {
            let client = ds.client.read().unwrap().clone();
            match client.generate(&body).await {
                Ok(()) => {
                    succeeded = true;
                    ds.touch();
                    break;
                }
                Err(e) if e == "busy" => {
                    if attempt < 2 {
                        // Backoff curto antes de retry
                        tokio::time::sleep(Duration::from_millis(500 * (attempt as u64 + 1))).await;
                        last_err = "busy".to_string();
                        continue;
                    } else {
                        last_err = "busy".to_string();
                    }
                }
                Err(e) => {
                    // Erro diferente de busy → tail para, falha honesta
                    telemetry_handle.abort();
                    return Err(PipelineError::Other(format!("daemon generate: {e}")));
                }
            }
        }

        // Para o tail: sucesso e busy-exausto convergem abaixo (done / DaemonBusy).
        telemetry_handle.abort();

        if !succeeded {
            if last_err == "busy" {
                return Err(PipelineError::DaemonBusy);
            }
            return Err(PipelineError::Other(format!(
                "daemon generate failed: {last_err}"
            )));
        }

        // (d) Coleta artefatos do output_dir igual one-shot (glob unificado — P0-4)
        let (mut artifacts, upload_errors) =
            collect_diffusion_artifacts(&s3, job_id, &outputs).await;

        // C2a: snapshot final dos logs (cobre o intervalo desde o último tick
        // periódico; best-effort, não entra no gate de upload_errors).
        if let Some(rep) = upload_telemetry_snapshot(&s3, job_id, &telemetry_abs).await {
            artifacts.push(rep);
        }

        // Incidente galeria vazia: upload persistente falhou → o job falhou do
        // ponto de vista do usuário; reportar done seria mentira.
        if !upload_errors.is_empty() {
            return Err(PipelineError::Other(format!(
                "upload de artefatos falhou: {}",
                upload_errors.join("; ")
            )));
        }

        // 11. Report done — inclui meta_content se generation_meta.json existe (D5 ADR-0023)
        let final_metrics = read_final_metrics(&outputs.join("metrics.jsonl"));
        let meta_content = read_generation_meta_content(&outputs);
        report_client
            .report(
                job_id,
                &ReportBody {
                    status: "done".to_string(),
                    progress: Some(1.0),
                    epoch: final_metrics.as_ref().map(|m| m.epoch),
                    step: final_metrics
                        .as_ref()
                        .and_then(|m| m.step.map(|s| s as i32)),
                    // AC-006-A D1: somente métricas de treino entram no array.
                    metrics: final_metrics
                        .as_ref()
                        .filter(|m| m.is_training_metric())
                        .map(|m| m.to_report_json()),
                    error: None,
                    artifacts: if artifacts.is_empty() {
                        None
                    } else {
                        Some(artifacts)
                    },
                    meta_content,
                    phase: Some("completed".to_string()),
                    message: Some("Treino concluído".to_string()),
                },
            )
            .await
            .map_err(|e| PipelineError::ReportFailed(format!("report done: {e}")))?;

        // 12. Cleanup tempdir
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        return Ok(());
    }

    // =========================================================================
    // ONE-SHOT PATH: Executor docker/subprocess (comportamento legado)
    // =========================================================================
    // 7. Execute trainer (D5 :301–309)
    let container_name = format!("trainer-{}-{}", dispatch.engine, job_id);
    let active_state = ActiveJobState::new(container_name.clone());
    active_jobs.insert(job_id.to_string(), active_state);

    let volumes = vec![
        (vol_datasets, "/datasets".to_string()),
        (vol_outputs, "/outputs".to_string()),
    ];

    // Env extras para o executor (D7): ENGINE_MOCK=0 quando GPU habilitada.
    let mut exec_env: Vec<(String, String)> = Vec::new();
    if gpu_devices.is_some() {
        exec_env.push(("ENGINE_MOCK".to_string(), "0".to_string()));
    }
    // Persistência de cache de modelos (Hugging Face / PyTorch) no volume montado /outputs/.cache
    if dispatch.engine == "diffusion" {
        // Reduz fragmentação de VRAM (OOM de alocções grandes com modelo 4-bit
        // carregado em GPU apertada).
        exec_env.push((
            "PYTORCH_CUDA_ALLOC_CONF".to_string(),
            "expandable_segments:True".to_string(),
        ));
        exec_env.push((
            "HF_HOME".to_string(),
            "/outputs/.cache/huggingface".to_string(),
        ));
        exec_env.push((
            "HF_HUB_CACHE".to_string(),
            "/outputs/.cache/huggingface/hub".to_string(),
        ));
        exec_env.push((
            "TRANSFORMERS_CACHE".to_string(),
            "/outputs/.cache/huggingface/hub".to_string(),
        ));
        exec_env.push((
            "DIFFUSERS_CACHE".to_string(),
            "/outputs/.cache/huggingface/hub".to_string(),
        ));
        exec_env.push((
            "TORCH_HOME".to_string(),
            "/outputs/.cache/torch".to_string(),
        ));
        exec_env.push(("HF_HUB_DISABLE_XET".to_string(), "1".to_string()));
        exec_env.push(("HF_HUB_ENABLE_HF_TRANSFER".to_string(), "0".to_string()));
        if let Ok(v) = std::env::var("ENABLE_TEXT_ENCODER_UNLOAD") {
            if !v.trim().is_empty() {
                exec_env.push((
                    "ENABLE_TEXT_ENCODER_UNLOAD".to_string(),
                    v.trim().to_string(),
                ));
            }
        }
    }

    // Repassa token do Hugging Face para download de modelos restritos/gated
    if let Ok(token) =
        std::env::var("HF_TOKEN").or_else(|_| std::env::var("HUGGING_FACE_HUB_TOKEN"))
    {
        if !token.is_empty() {
            exec_env.push(("HF_TOKEN".to_string(), token.clone()));
            exec_env.push(("HUGGING_FACE_HUB_TOKEN".to_string(), token));
        }
    }

    // Repassa FLUX_MODEL_ID customizado se definido no nó
    if let Ok(model_id) = std::env::var("FLUX_MODEL_ID") {
        if !model_id.is_empty() {
            exec_env.push(("FLUX_MODEL_ID".to_string(), model_id));
        }
    }

    // Spawn metrics collector & sample streamer (estágio unificado — collector.rs)
    let metrics_path = outputs.join("metrics.jsonl");
    let metrics_report_client = Arc::clone(&report_client);
    let metrics_s3 = Arc::clone(&s3);
    let metrics_handle = tokio::spawn(stream_metrics_and_samples(
        metrics_s3,
        metrics_report_client,
        job_id.to_string(),
        metrics_path.clone(),
        outputs.join("samples"),
        outputs.join("checkpoints"),
        total_epochs,
        dispatch.engine == "diffusion",
    ));

    // Ramifica subcomando e artefatos por (engine, mode) — ADR-0013 D6 (estágio execute.rs)
    let subcommand_args = resolve_subcommand_args(&dispatch.engine, &dispatch.mode, job_id)?;

    tracing::info!(
        job_id = %job_id,
        container = %container_name,
        "Inicializando container de execução na GPU..."
    );
    let _ = report_client
        .report(
            job_id,
            &ReportBody {
                status: "running".to_string(),
                progress: Some(0.08),
                epoch: None,
                step: None,
                metrics: None,
                error: None,
                artifacts: None,
                meta_content: None,
                phase: Some("starting_container".to_string()),
                message: Some("Inicializando container de execução na GPU...".to_string()),
            },
        )
        .await;

    let (exit_code, logs) = executor
        .run(
            &dispatch.image,
            &container_name,
            &volumes,
            &subcommand_args,
            &exec_env,
            gpu_devices,
        )
        .await;

    // Cancela metrics collector
    metrics_handle.abort();

    let was_cancelled = active_jobs
        .get(job_id)
        .map(|e| e.is_cancelled())
        .unwrap_or(false);
    active_jobs.remove(job_id);

    if was_cancelled {
        return Err(PipelineError::Cancelled);
    }
    // 8. Check exit code
    if exit_code != 0 {
        let logs_tail = logs.lines().rev().take(20).collect::<Vec<_>>().join("\n");
        return Err(PipelineError::DockerFailed {
            exit_code,
            logs_tail,
        });
    }

    // 9. Upload artifacts para S3 (D8 — artifacts/<job_id>/)
    let metrics_filename =
        if outputs.join("telemetry.jsonl").is_file() && !outputs.join("metrics.jsonl").is_file() {
            "telemetry.jsonl"
        } else {
            "metrics.jsonl"
        };
    let artifact_specs: Vec<(&str, &str)> = match (dispatch.engine.as_str(), dispatch.mode.as_str())
    {
        ("yolo", "train") => vec![
            ("best.pt", "model"),
            ("last.pt", "model"),
            (metrics_filename, "metrics"),
        ],
        ("yolo", "predict") => vec![("predictions.json", "predictions")],
        ("autotracker", _) => vec![("boxes.json", "boxes"), (metrics_filename, "metrics")],
        ("autolabel", _) => vec![
            ("captions.jsonl", "captions"),
            (metrics_filename, "metrics"),
        ],
        ("diffusion", "generate") => vec![], // glob abaixo (D2 ADR-0023)
        ("diffusion", _) => vec![
            ("adapter.safetensors", "model"),
            (metrics_filename, "metrics"),
        ],
        // Já validado acima — seguro unreachable
        _ => unreachable!("unsupported engine/mode validated earlier"),
    };

    let mut artifacts = Vec::new();
    // Uploads com retry; falhas persistentes viram failed no gate abaixo
    // (incidente galeria vazia) — sem abortar o resto do loop.
    let mut upload_errors: Vec<String> = Vec::new();

    for (filename, kind) in artifact_specs {
        let file_path = outputs.join(filename);
        if file_path.exists() {
            let art_key = format!("artifacts/{job_id}/{filename}");
            let art_key = scoped_key(S3Scope::Artifacts, &art_key)
                .map_err(|e| PipelineError::ArtifactUpload(format!("artifact key: {e}")))?;
            let md5 = compute_file_md5(&file_path)
                .map_err(|e| PipelineError::ArtifactUpload(format!("md5 {filename}: {e}")))?;
            let bytes = std::fs::metadata(&file_path)
                .map(|m| m.len() as i64)
                .unwrap_or(0);

            put_with_retry(s3.as_ref(), &art_key, &file_path)
                .await
                .map_err(|e| PipelineError::ArtifactUpload(format!("upload {filename}: {e}")))?;

            artifacts.push(ArtifactReport {
                kind: kind.to_string(),
                path: filename.to_string(),
                md5,
                bytes,
            });
        }
    }

    // Coleta glob para diffusion generate (D2 — ADR-0023, estágio unificado — P0-4)
    if dispatch.engine == "diffusion" && dispatch.mode == "generate" {
        let (glob_artifacts, glob_errors) =
            collect_diffusion_artifacts(&s3, job_id, &outputs).await;
        artifacts.extend(glob_artifacts);
        upload_errors.extend(glob_errors);
    }

    // Se for difusão, escaneia também samples/, checkpoints/ por época e modelos safetensors adicionais
    if dispatch.engine == "diffusion" {
        let samples_dir = outputs.join("samples");
        if samples_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&samples_dir) {
                let mut sample_files: Vec<std::path::PathBuf> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.is_file()
                            && !p
                                .file_name()
                                .and_then(|n| n.to_str())
                                .map(|n| {
                                    n.starts_with('.')
                                        || n.ends_with(".tmp")
                                        || n.ends_with(".part")
                                })
                                .unwrap_or(false)
                            && p.extension()
                                .and_then(|e| e.to_str())
                                .map(|ext| {
                                    matches!(
                                        ext.to_ascii_lowercase().as_str(),
                                        "png" | "jpg" | "jpeg" | "webp"
                                    )
                                })
                                .unwrap_or(false)
                    })
                    .collect();
                sample_files.sort();

                for s_path in sample_files {
                    if let Some(s_name) = s_path.file_name().and_then(|n| n.to_str()) {
                        let rel_path = format!("samples/{s_name}");
                        let art_key = format!("artifacts/{job_id}/{rel_path}");
                        match scoped_key(S3Scope::Artifacts, &art_key) {
                            Ok(scoped) => match compute_file_md5(&s_path) {
                                Ok(md5) => {
                                    let bytes = std::fs::metadata(&s_path)
                                        .map(|m| m.len() as i64)
                                        .unwrap_or(0);
                                    match put_with_retry(s3.as_ref(), &scoped, &s_path).await {
                                        Ok(()) => artifacts.push(ArtifactReport {
                                            kind: "sample".to_string(),
                                            path: rel_path,
                                            md5,
                                            bytes,
                                        }),
                                        Err(e) => upload_errors.push(format!("{rel_path}: {e}")),
                                    }
                                }
                                Err(e) => upload_errors.push(format!(
                                    "{rel_path}: falha ao preparar artefato (md5): {e}"
                                )),
                            },
                            Err(e) => upload_errors
                                .push(format!("{rel_path}: falha ao preparar artefato (key): {e}")),
                        }
                    }
                }
            }
        }

        // Escaneia checkpoints por época em outputs/checkpoints/
        let checkpoints_dir = outputs.join("checkpoints");
        if checkpoints_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&checkpoints_dir) {
                let mut ckpt_files: Vec<std::path::PathBuf> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.is_file()
                            && !p
                                .file_name()
                                .and_then(|n| n.to_str())
                                .map(|n| {
                                    n.starts_with('.')
                                        || n.ends_with(".tmp")
                                        || n.ends_with(".part")
                                })
                                .unwrap_or(false)
                            && p.extension()
                                .and_then(|e| e.to_str())
                                .map(|ext| ext.eq_ignore_ascii_case("safetensors"))
                                .unwrap_or(false)
                    })
                    .collect();
                ckpt_files.sort();

                for c_path in ckpt_files {
                    if let Some(c_name) = c_path.file_name().and_then(|n| n.to_str()) {
                        let rel_path = format!("checkpoints/{c_name}");
                        let art_key = format!("artifacts/{job_id}/{rel_path}");
                        match scoped_key(S3Scope::Artifacts, &art_key) {
                            Ok(scoped) => match compute_file_md5(&c_path) {
                                Ok(md5) => {
                                    let bytes = std::fs::metadata(&c_path)
                                        .map(|m| m.len() as i64)
                                        .unwrap_or(0);
                                    match put_with_retry(s3.as_ref(), &scoped, &c_path).await {
                                        Ok(()) => artifacts.push(ArtifactReport {
                                            kind: "checkpoint".to_string(),
                                            path: rel_path,
                                            md5,
                                            bytes,
                                        }),
                                        Err(e) => upload_errors.push(format!("{rel_path}: {e}")),
                                    }
                                }
                                Err(e) => upload_errors.push(format!(
                                    "{rel_path}: falha ao preparar artefato (md5): {e}"
                                )),
                            },
                            Err(e) => upload_errors
                                .push(format!("{rel_path}: falha ao preparar artefato (key): {e}")),
                        }
                    }
                }
            }
        }

        // Escaneia qualquer outro *.safetensors na raiz de outputs/ (ex: nome semântico configurado)
        if let Ok(entries) = std::fs::read_dir(&outputs) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    if let Some(f_name) = p.file_name().and_then(|n| n.to_str()) {
                        if !f_name.starts_with('.')
                            && !f_name.ends_with(".tmp")
                            && !f_name.ends_with(".part")
                            && f_name != "adapter.safetensors"
                            && p.extension()
                                .and_then(|e| e.to_str())
                                .map(|ext| ext.eq_ignore_ascii_case("safetensors"))
                                .unwrap_or(false)
                        {
                            let art_key = format!("artifacts/{job_id}/{f_name}");
                            match scoped_key(S3Scope::Artifacts, &art_key) {
                                Ok(scoped) => match compute_file_md5(&p) {
                                    Ok(md5) => {
                                        let bytes = std::fs::metadata(&p)
                                            .map(|m| m.len() as i64)
                                            .unwrap_or(0);
                                        match put_with_retry(s3.as_ref(), &scoped, &p).await {
                                            Ok(()) => artifacts.push(ArtifactReport {
                                                kind: "model".to_string(),
                                                path: f_name.to_string(),
                                                md5,
                                                bytes,
                                            }),
                                            Err(e) => upload_errors.push(format!("{f_name}: {e}")),
                                        }
                                    }
                                    Err(e) => upload_errors.push(format!(
                                        "{f_name}: falha ao preparar artefato (md5): {e}"
                                    )),
                                },
                                Err(e) => upload_errors.push(format!(
                                    "{f_name}: falha ao preparar artefato (key): {e}"
                                )),
                            }
                        }
                    }
                }
            }
        }
    }

    // Se for treino de difusão e existir training_config.json, inclui nos artefatos com kind "config"
    if dispatch.engine == "diffusion" && dispatch.mode == "train" {
        let training_config_path = outputs.join("training_config.json");
        if training_config_path.is_file() {
            let art_key = format!("artifacts/{job_id}/training_config.json");
            match scoped_key(S3Scope::Artifacts, &art_key) {
                Ok(scoped) => match compute_file_md5(&training_config_path) {
                    Ok(md5) => {
                        let bytes = std::fs::metadata(&training_config_path)
                            .map(|m| m.len() as i64)
                            .unwrap_or(0);
                        match put_with_retry(s3.as_ref(), &scoped, &training_config_path).await {
                            Ok(()) => artifacts.push(ArtifactReport {
                                kind: "config".to_string(),
                                path: "training_config.json".to_string(),
                                md5,
                                bytes,
                            }),
                            Err(e) => upload_errors.push(format!("training_config.json: {e}")),
                        }
                    }
                    Err(e) => upload_errors.push(format!(
                        "training_config.json: falha ao preparar artefato (md5): {e}"
                    )),
                },
                Err(e) => upload_errors.push(format!(
                    "training_config.json: falha ao preparar artefato (key): {e}"
                )),
            }
        }
    }

    // Incidente galeria vazia: upload persistente falhou → o job falhou do
    // ponto de vista do usuário; reportar done seria mentira.
    if !upload_errors.is_empty() {
        return Err(PipelineError::Other(format!(
            "upload de artefatos falhou: {}",
            upload_errors.join("; ")
        )));
    }

    // C2a: snapshot final dos logs no caminho one-shot (o loop live pode ter
    // parado com o tick no meio; best-effort).
    let telemetry_oneshot = metrics_path.with_file_name("telemetry.jsonl");
    if telemetry_oneshot.is_file() {
        if let Some(rep) = upload_telemetry_snapshot(&s3, job_id, &telemetry_oneshot).await {
            artifacts.push(rep);
        }
    }

    // 10. Lê métricas finais para o report done
    let final_metrics = read_final_metrics(&metrics_path);

    // 11. Report done — inclui meta_content se generation_meta.json existe (D5 ADR-0023)
    let meta_content = read_generation_meta_content(&outputs);
    report_client
        .report(
            job_id,
            &ReportBody {
                status: "done".to_string(),
                progress: Some(1.0),
                epoch: final_metrics.as_ref().map(|m| m.epoch),
                step: final_metrics
                    .as_ref()
                    .and_then(|m| m.step.map(|s| s as i32)),
                // AC-006-A D1: somente métricas de treino entram no array.
                metrics: final_metrics
                    .as_ref()
                    .filter(|m| m.is_training_metric())
                    .map(|m| m.to_report_json()),
                error: None,
                artifacts: if artifacts.is_empty() {
                    None
                } else {
                    Some(artifacts)
                },
                meta_content,
                phase: Some("completed".to_string()),
                message: Some("Treino concluído".to_string()),
            },
        )
        .await
        .map_err(|e| PipelineError::ReportFailed(format!("report done: {e}")))?;

    // 12. Cleanup tempdir (datasets-cache e outputs persistem — volumes §8)
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;

    Ok(())
}
