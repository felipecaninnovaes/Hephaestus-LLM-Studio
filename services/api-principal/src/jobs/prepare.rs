//! Worker de preparação assíncrona de datasets (ADR-0025 D0–D4, fatia P4a).
//!
//! O request (`handlers.rs`) faz só validação barata (<1s): o manager aloca o
//! `jobId` com `status='preparing'` + `package_ref=null`, o BFF responde 202 e
//! agenda o empacotamento pesado (S3+zip, minutos em datasets grandes) num
//! `tokio::spawn` AQUI no api-principal — único dono de `StoragePort`+pool por
//! isolamento (manager/orchestrator ficam de fora).
//!
//! Invariantes:
//! - `PrepareSpec` nunca é logada na íntegra nem exposta em rota: `apiKey`
//!   (autolabel) NÃO é persistida — o worker nunca a consome (a `config_yaml`,
//!   que já carrega a chave como antes, é pré-gerada no aceite e enviada no
//!   `create_job`, tratamento idêntico ao legado; GET jobs nunca lê
//!   `job_prepares`).
//! - Compensação (D4): apagar `dataset_versions`/S3 SOMENTE se a versão foi
//!   criada neste attempt E `prepare_complete` nunca retornou Ok; NUNCA apagar
//!   versão reusada por fingerprint.
//! - Fingerprint: fórmula da ADR adaptada ao schema real — `images`,
//!   `boxes` e `classes` NÃO têm `updated_at` (só `images.created_at`);
//!   sensibilidade a edições in-place vem de sinais de conteúdo (`hashtext`
//!   de coords/nomes) em vez de `max(updated_at)`. DESVIO registrado no
//!   relatório da fatia.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use uuid::Uuid;

use crate::error::{err, MSG_INVALID_REQUEST, MSG_NOT_FOUND, MSG_QUEUE_UNAVAILABLE};
use crate::jobs::manager_client::ManagerError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// PrepareSpec — envelope persistido em `job_prepares.spec` (JSONB, camelCase)
// ---------------------------------------------------------------------------

/// Spec da preparação (wire camelCase como o resto de `/api/*`).
///
/// `params` carrega os inputs de `generate_*_config_yaml` por kind
/// (model/prompt/apiBase/...) para auditoria e re-spawn — EXCETO `apiKey`,
/// que o worker nunca consome (ver invariante no cabeçalho do módulo).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareSpec {
    /// Kind do manager (`yolo_train`, `autotracker`, `autolabel`,
    /// `diffusion_train`, `yolo_predict`).
    pub kind: String,
    pub dataset_id: Uuid,
    /// Escopo resolvido no aceite (`filter_class_id`/`image_ids`); `None` = ALL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_image_ids: Option<Vec<Uuid>>,
    pub fingerprint: String,
    pub engine: String,
    /// Afeta o conteúdo do pacote (ex.: diffusion; `None` nos demais).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_word: Option<String>,
    /// Inputs de config por kind (SEM segredos).
    #[serde(default)]
    pub params: serde_json::Value,
}

/// Monta `spec.params` do autolabel SEM `apiKey` (puro/testável).
///
/// O worker nunca regenera `config_yaml` (não há endpoint no manager para
/// config tardia — a config pré-gerada no aceite já foi enviada no
/// `create_job`); persistir o segredo seria risco gratuito. `apiBase`,
/// `openaiModel` e `reasoningEffort` não são segredos e ficam.
pub fn spec_params_autolabel(
    model: &str,
    prompt: Option<&str>,
    api_base: Option<&str>,
    openai_model: Option<&str>,
    reasoning_effort: Option<&str>,
    filter_class_id: Option<&str>,
    image_ids_count: Option<usize>,
) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "prompt": prompt,
        "apiBase": api_base,
        "openaiModel": openai_model,
        "reasoningEffort": reasoning_effort,
        "filterClassId": filter_class_id,
        "imageIdsCount": image_ids_count,
    })
}

// ---------------------------------------------------------------------------
// Bodies wire do worker (puros/testáveis — camelCase)
// ---------------------------------------------------------------------------

/// Body de `POST /internal/jobs/:id/prepare-complete` (wire real P3).
pub fn prepare_complete_body(
    version_id: &Uuid,
    key: &str,
    md5_zip: &str,
    bytes: i64,
) -> serde_json::Value {
    serde_json::json!({
        "datasetVersionId": version_id.to_string(),
        "packageRef": { "key": key, "md5Zip": md5_zip, "bytes": bytes },
    })
}

/// Body de `POST /internal/jobs/:id/prepare-fail`.
/// Códigos P4a: `storage_unavailable` | `build_error` | `timeout`.
pub fn prepare_fail_body(code: &str, message: &str) -> serde_json::Value {
    serde_json::json!({ "code": code, "message": message })
}

/// Body de `POST /internal/jobs/:id/report` (canal ADR-0024, fase `packaging_*`).
pub fn prepare_report_body(phase: &str, message: &str, progress: f64) -> serde_json::Value {
    serde_json::json!({
        "status": "preparing",
        "phase": phase,
        "message": message,
        "progress": progress,
    })
}

// ---------------------------------------------------------------------------
// Fingerprint — sha1 sobre o escopo + sinais de frescor do dataset
// ---------------------------------------------------------------------------

/// Hex sha1 puro sobre as partes já ordenadas (testável sem banco).
pub fn fingerprint_hex(
    dataset_id: &Uuid,
    sorted_ids: &[Uuid],
    scope_all: bool,
    image_count: i64,
    max_created_at: &str,
    boxes_count: i64,
    boxes_sig: i64,
    class_count: i64,
    classes_sig: i64,
    engine: &str,
    extra: &str,
) -> String {
    use sha1::Digest;
    let ids_part = if scope_all {
        "ALL".to_string()
    } else {
        let mut ids: Vec<String> = sorted_ids.iter().map(|u| u.to_string()).collect();
        ids.sort();
        ids.join(",")
    };
    let material = format!(
        "{dataset_id}|{ids_part}|{image_count}|{max_created_at}|{boxes_count}:{boxes_sig}|{class_count}:{classes_sig}|{engine}|{extra}"
    );
    let mut h = sha1::Sha1::new();
    h.update(material.as_bytes());
    hex::encode(h.finalize())
}

/// Calcula o fingerprint do dataset (só SQL barato, sem S3).
///
/// `resolved`: escopo do pacote (`None` = dataset inteiro). `extra`: string
/// que afeta o conteúdo do pacote (`trigger_word` no diffusion, `model` no
/// autolabel, `""` nos demais).
pub async fn fingerprint_for_dataset(
    pool: &sqlx::PgPool,
    dataset_id: Uuid,
    resolved: Option<&[Uuid]>,
    engine: &str,
    extra: &str,
) -> Result<String, sqlx::Error> {
    // Imagens do escopo: count + frescor (created_at — não há updated_at).
    let (image_count, max_created): (i64, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT count(*), max(created_at) FROM images \
         WHERE dataset_id = $1 AND deleted_at IS NULL \
           AND ($2::uuid[] IS NULL OR id = ANY($2))",
    )
    .bind(dataset_id)
    .bind(resolved)
    .fetch_one(pool)
    .await?;
    let max_created_str = max_created
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default();
    // Boxes do escopo: count + sinal de conteúdo (coords/classes; edições
    // in-place mudam o sig mesmo sem mudar o count).
    let (boxes_count, boxes_sig): (i64, i64) = sqlx::query_as(
        "SELECT count(*), COALESCE(SUM(hashtext( \
           b.id::text || '|' || b.x::text || '|' || b.y::text || '|' || b.w::text || '|' || b.h::text \
           || '|' || COALESCE(b.conf::text, '') || '|' || b.class_id::text)), 0) \
         FROM boxes b JOIN images i ON i.id = b.image_id \
         WHERE i.dataset_id = $1 AND i.deleted_at IS NULL \
           AND ($2::uuid[] IS NULL OR i.id = ANY($2))",
    )
    .bind(dataset_id)
    .bind(resolved)
    .fetch_one(pool)
    .await?;
    // Classes: count + sinal de nomes (rename muda o pacote mesmo sem mudar o count).
    let (class_count, classes_sig): (i64, i64) = sqlx::query_as(
        "SELECT count(*), COALESCE(SUM(hashtext(name || '|' || idx::text)), 0) \
         FROM classes WHERE dataset_id = $1",
    )
    .bind(dataset_id)
    .fetch_one(pool)
    .await?;
    Ok(fingerprint_hex(
        &dataset_id,
        resolved.unwrap_or(&[]),
        resolved.is_none(),
        image_count,
        &max_created_str,
        boxes_count,
        boxes_sig,
        class_count,
        classes_sig,
        engine,
        extra,
    ))
}

// ---------------------------------------------------------------------------
// Dedupe — submit com mesmo fingerprint e prepare ativo reaproveita o jobId
// ---------------------------------------------------------------------------

/// Dedupe (D3): job em `preparing` com mesmo `(dataset_id, fingerprint)` nos
/// últimos 30min → `Some(job_id)` (responder 202 sem criar nada).
pub async fn find_active_prepare(
    pool: &sqlx::PgPool,
    dataset_id: Uuid,
    fingerprint: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT job_id FROM job_prepares \
         WHERE dataset_id = $1 AND fingerprint = $2 AND state = 'preparing' \
           AND updated_at > now() - interval '30 minutes' \
         ORDER BY updated_at DESC LIMIT 1",
    )
    .bind(dataset_id)
    .bind(fingerprint)
    .fetch_optional(pool)
    .await
}

// ---------------------------------------------------------------------------
// Aceite — fluxo comum dos 5 submits (dedupe → create → insert → spawn → 202)
// ---------------------------------------------------------------------------

fn invalid_request() -> Response {
    err(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        MSG_INVALID_REQUEST,
    )
}

fn not_found() -> Response {
    err(StatusCode::NOT_FOUND, "not_found", MSG_NOT_FOUND)
}

fn queue_unavailable() -> Response {
    err(
        StatusCode::SERVICE_UNAVAILABLE,
        "queue_unavailable",
        MSG_QUEUE_UNAVAILABLE,
    )
}

/// Aceite assíncrono compartilhado pelos 5 submits com dataset.
///
/// `manager_body`: body do `create_job` montado pelo caller (kind/engine/
/// model/mode/config_yaml/params/vram/...) SEM `package_ref` (o helper injeta
/// `package_ref: null` + `params.prepare`). Ordem (D3): dedupe → create_job no
/// manager (ganha `job_id`) → INSERT local → spawn.
///
/// Anti-TOCTOU (B1): o SELECT dedupe acima é só fast-path — dois submits
/// simultâneos passam por ele juntos. A vitória única vem do índice
/// `job_prepares_dedupe` (migration 0016): o INSERT usa `ON CONFLICT DO
/// NOTHING`; 0 linhas ⇒ outro aceite venceu em voo ⇒ re-SELECT pega o
/// `job_id` vencedor, aborta-se o job recém-criado (best-effort) e responde-se
/// 202 com o vencedor. O micro-gasto (criar+abortar 1 job) é aceitável frente
/// a duplicar um build de dataset. Se o INSERT falhar por outro motivo:
/// `prepare_fail` best-effort + 503 (o watchdog do manager falha o job órfão
/// em 60min).
pub async fn accept_job_preparing(
    state: &AppState,
    spec: PrepareSpec,
    mut manager_body: serde_json::Value,
) -> Response {
    // 1. Dedupe fast-path: mesmo fingerprint com prepare ativo → mesmo jobId
    //    sem tocar no manager. (A janela TOCTOU aqui é fechada pelo INSERT
    //    com ON CONFLICT no passo 3.)
    match find_active_prepare(&state.pool, spec.dataset_id, &spec.fingerprint).await {
        Ok(Some(existing)) => {
            return (
                StatusCode::ACCEPTED,
                Json(serde_json::json!({
                    "jobId": existing.to_string(),
                    "status": "preparing",
                    "queuePosition": null,
                })),
            )
                .into_response();
        }
        Ok(None) => {}
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            );
        }
    }

    // 2. create_job em modo preparing (package_ref null + params.prepare opaco).
    manager_body["package_ref"] = serde_json::Value::Null;
    manager_body["params"]["prepare"] = serde_json::json!({
        "kind": spec.kind,
        "datasetId": spec.dataset_id.to_string(),
        "fingerprint": spec.fingerprint,
    });
    let created = match state.manager.create_job(&manager_body).await {
        Ok(r) => r,
        Err(ManagerError::NotFound) => return not_found(),
        Err(ManagerError::InvalidRequest(_)) => return invalid_request(),
        // Sem pacote no request, não há nada a compensar (D4).
        Err(ManagerError::Unavailable(_)) => return queue_unavailable(),
        Err(_) => return queue_unavailable(),
    };
    let job_id: Uuid = match created.job_id.parse() {
        Ok(u) => u,
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            );
        }
    };

    // 3. INSERT local com arbiter do índice parcial `job_prepares_dedupe`
    //    (dono P4a — migration 0016). 1 linha ⇒ vencemos; 0 linhas ⇒ outro
    //    aceite em voo registrou o mesmo fingerprint primeiro.
    let spec_json = serde_json::to_value(&spec).unwrap_or(serde_json::json!({}));
    let inserted = sqlx::query(
        "INSERT INTO job_prepares (job_id, dataset_id, fingerprint, spec) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (dataset_id, fingerprint) WHERE state = 'preparing' DO NOTHING",
    )
    .bind(job_id)
    .bind(spec.dataset_id)
    .bind(&spec.fingerprint)
    .bind(&spec_json)
    .execute(&state.pool)
    .await;
    let won = match inserted {
        Ok(r) => r.rows_affected() == 1,
        Err(_) => false,
    };
    if !won {
        // Disputa perdida OU erro local: re-SELECT decide. Vencedor distinto
        // do nosso job ⇒ aborta o recém-criado (best-effort) e responde 202
        // com o vencedor estável. Vencedor == nosso job (retry idempotente)
        // ⇒ prossegue como dono. Sem vencedor (vencedor concluiu entre o
        // INSERT e o SELECT) ⇒ tenta o INSERT uma vez (agora sem conflito);
        // se ainda assim 0 linhas, falha o job e responde 503.
        match find_active_prepare(&state.pool, spec.dataset_id, &spec.fingerprint).await {
            Ok(Some(winner)) if winner != job_id => {
                let _ = state.manager.abort_job(&created.job_id).await;
                return (
                    StatusCode::ACCEPTED,
                    Json(serde_json::json!({
                        "jobId": winner.to_string(),
                        "status": "preparing",
                        "queuePosition": null,
                    })),
                )
                    .into_response();
            }
            Ok(Some(_)) => {}
            Ok(None) => {
                // Se não há prepare ativo recente mas houve conflito no INSERT,
                // expira eventuais linhas zumbis antigas (>10 min sem heartbeat):
                let _ = sqlx::query(
                    "UPDATE job_prepares SET state = 'failed', updated_at = now() \
                     WHERE dataset_id = $1 AND fingerprint = $2 AND state = 'preparing' \
                       AND updated_at < now() - interval '10 minutes'",
                )
                .bind(spec.dataset_id)
                .bind(&spec.fingerprint)
                .execute(&state.pool)
                .await;

                let retry = sqlx::query(
                    "INSERT INTO job_prepares (job_id, dataset_id, fingerprint, spec) \
                     VALUES ($1, $2, $3, $4) \
                     ON CONFLICT (dataset_id, fingerprint) WHERE state = 'preparing' DO NOTHING",
                )
                .bind(job_id)
                .bind(spec.dataset_id)
                .bind(&spec.fingerprint)
                .bind(&spec_json)
                .execute(&state.pool)
                .await;
                let retry_won = retry.map(|r| r.rows_affected() == 1).unwrap_or(false);
                if !retry_won {
                    // Local falhou com job já alocado: falha o job no manager
                    // (best-effort) para não deixá-lo `preparing` até o
                    // watchdog de 60min.
                    let _ = state
                        .manager
                        .prepare_fail(
                            &created.job_id,
                            &prepare_fail_body("build_error", "falha ao registrar preparação"),
                        )
                        .await;
                    return queue_unavailable();
                }
            }
            Err(_) => {
                let _ = state
                    .manager
                    .prepare_fail(
                        &created.job_id,
                        &prepare_fail_body("build_error", "falha ao registrar preparação"),
                    )
                    .await;
                return queue_unavailable();
            }
        }
    }

    // 4. Spawn do worker + 202 imediato (<1s, sem S3 no request).
    spawn_prepare(state.clone(), job_id, spec);
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "jobId": created.job_id,
            "status": created.status,
            "queuePosition": created.queue_position,
        })),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Worker — tokio::spawn no api-principal (único dono de StoragePort+pool)
// ---------------------------------------------------------------------------

/// Agenda a preparação em background. O futuro nunca propaga pânico para o
/// runtime: `run_prepare` roda sob `catch_unwind` — pânico vira
/// `prepare_fail{build_error}` + linha `failed` (B2). Sem isso, um pânico
/// deixava a linha em `preparing` e o job empacado até o watchdog de 60min.
pub fn spawn_prepare(state: AppState, job_id: Uuid, spec: PrepareSpec) {
    tokio::spawn(async move {
        let caught = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(
            run_prepare(&state, job_id, &spec),
        ))
        .await;
        if let Err(payload) = caught {
            // O payload do pânico pode carregar dados do spec: nunca logar o
            // conteúdo, só o fato + kind (não-segredo, literal server-side).
            let hint = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(|s| s.as_str()))
                .unwrap_or("panic");
            tracing::error!(job_id = %job_id, kind = %spec.kind, panic = %hint, "worker de preparação panicou");
            fail_prepare(
                &state,
                job_id,
                None,
                false,
                "build_error",
                fail_message("build_error"),
            )
            .await;
        }
    });
}

/// Partes do pacote resolvidas (build novo ou reuso por fingerprint).
struct ResolvedPackage {
    version_id: Uuid,
    key: String,
    md5_zip: String,
    bytes: i64,
    /// `Some` ⇒ criada NESTE attempt (compensável); `None` ⇒ reusada (D4: intocável).
    created_this_attempt: bool,
}

/// Report de progresso best-effort (falha nunca aborta a preparação).
/// Toca `updated_at` junto: heartbeat do worker para o recovery —
///
/// `recover_stale_prepares` re-spawna `preparing` com `updated_at` > 10min;
/// sem heartbeat, builds de horas pareceriam órfãos e seriam re-spawnados.
async fn report_progress(state: &AppState, job_id: &str, message: &str, progress: f64) {
    touch_prepare(&state.pool, job_id).await;
    let _ = state
        .manager
        .report_phase(
            job_id,
            &prepare_report_body("packaging_dataset", message, progress),
        )
        .await;
}

/// Heartbeat do worker (B2): `updated_at=now()` para o recovery distinguir
/// worker vivo de órfão. Best-effort como o resto do worker.
async fn touch_prepare(pool: &sqlx::PgPool, job_id: &str) {
    let _ = sqlx::query("UPDATE job_prepares SET updated_at = now() WHERE job_id = $1::uuid")
        .bind(job_id)
        .execute(pool)
        .await;
}

async fn mark_prepare_cancelled(pool: &sqlx::PgPool, job_id: Uuid) {
    let _ = sqlx::query(
        "UPDATE job_prepares SET state = 'cancelled', updated_at = now() WHERE job_id = $1",
    )
    .bind(job_id)
    .execute(pool)
    .await;
}

async fn mark_prepare_done(pool: &sqlx::PgPool, job_id: Uuid) {
    let _ =
        sqlx::query("UPDATE job_prepares SET state = 'done', updated_at = now() WHERE job_id = $1")
            .bind(job_id)
            .execute(pool)
            .await;
}

/// Falha a preparação: compensa versão própria (D4), chama `prepare_fail`
/// (best-effort) e marca a linha local. `message` é estática — nunca contém
/// segredo, path ou conteúdo do spec.
async fn fail_prepare(
    state: &AppState,
    job_id: Uuid,
    created_version: Option<Uuid>,
    completed: bool,
    code: &str,
    message: &str,
) {
    // D4: apaga versão+S3 SOMENTE se criada neste attempt E complete nunca
    // chamado com sucesso; versão reusada é intocável.
    if !completed {
        if let Some(vid) = created_version {
            let _ = state
                .storage
                .delete_prefix(&format!("packages/{vid}/"))
                .await;
            let _ = sqlx::query("DELETE FROM dataset_versions WHERE id = $1")
                .bind(vid)
                .execute(&state.pool)
                .await;
        }
    }
    let _ = state
        .manager
        .prepare_fail(&job_id.to_string(), &prepare_fail_body(code, message))
        .await;
    let _ = sqlx::query(
        "UPDATE job_prepares SET state = 'failed', error = $2, updated_at = now() \
         WHERE job_id = $1",
    )
    .bind(job_id)
    .bind(format!("{code}: {message}"))
    .execute(&state.pool)
    .await;
}

/// Tenta reuso de `dataset_versions` pelo fingerprint do manifest.
///
/// A P4b grava `fingerprint` como chave de topo do manifest (JSONB e
/// `manifest.json` de transporte) na `build_package_filtered`; no hit,
/// `md5_zip`/`bytes` vêm do `manifest.json` no storage
/// (`packages/<vid>/manifest.json`). Se a leitura falhar, trata como miss.
/// (`build_package_diffusion` ainda não grava fingerprint — diffusion sempre
/// constrói; ver `build_for_spec`.)
async fn try_reuse_package(state: &AppState, spec: &PrepareSpec) -> Option<ResolvedPackage> {
    let vid: Uuid = sqlx::query_scalar(
        "SELECT id FROM dataset_versions \
         WHERE dataset_id = $1 AND manifest ->> 'fingerprint' = $2 LIMIT 1",
    )
    .bind(spec.dataset_id)
    .bind(&spec.fingerprint)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten()?;
    let raw = state
        .storage
        .get(&format!("packages/{vid}/manifest.json"))
        .await
        .ok()?;
    let manifest: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    let md5_zip = manifest.get("md5_zip")?.as_str()?;
    let bytes = manifest.get("bytes")?.as_i64()?;
    if md5_zip.is_empty() || bytes < 0 {
        return None;
    }
    Some(ResolvedPackage {
        version_id: vid,
        key: format!("packages/{vid}/dataset.zip"),
        md5_zip: md5_zip.to_string(),
        bytes,
        created_this_attempt: false,
    })
}

/// Constrói o pacote conforme o kind (interface congelada da P4b: o 4º
/// argumento `fingerprint: Option<&str>` é gravado no manifest pela
/// `build_package_filtered`; `build_package_diffusion` ainda não recebe
/// fingerprint — reuso diffusion fica para a fusão P4a+P4b).
/// `Err(code)` = código `prepare_fail` (`storage_unavailable` se o build
/// respondeu 503, senão `build_error`).
async fn build_for_spec(
    state: &AppState,
    spec: &PrepareSpec,
) -> Result<ResolvedPackage, &'static str> {
    let build = match spec.kind.as_str() {
        "yolo_train" | "autotracker" | "yolo_predict" => {
            crate::datasets::package::build_package_filtered(
                state,
                spec.dataset_id,
                None,
                Some(&spec.fingerprint),
            )
            .await
        }
        "autolabel" => {
            crate::datasets::package::build_package_filtered(
                state,
                spec.dataset_id,
                spec.resolved_image_ids.as_deref(),
                Some(&spec.fingerprint),
            )
            .await
        }
        "diffusion_train" => {
            crate::datasets::package::build_package_diffusion(
                state,
                spec.dataset_id,
                spec.trigger_word.as_deref(),
            )
            .await
        }
        _ => {
            return Err("build_error");
        }
    };
    match build {
        Ok(p) => {
            let version_id: Uuid = p.version_id.parse().map_err(|_| "build_error")?;
            Ok(ResolvedPackage {
                version_id,
                key: p.key,
                md5_zip: p.md5_zip,
                bytes: p.bytes,
                created_this_attempt: true,
            })
        }
        Err(resp) => {
            if resp.status() == StatusCode::SERVICE_UNAVAILABLE {
                Err("storage_unavailable")
            } else {
                Err("build_error")
            }
        }
    }
}

fn fail_message(code: &str) -> &'static str {
    match code {
        "storage_unavailable" => "armazenamento indisponível durante o empacotamento",
        "timeout" => "tempo esgotado no empacotamento",
        _ => "falha ao empacotar dataset",
    }
}

/// Corpo do worker: cancel-check → reuso/build → complete/fail.
async fn run_prepare(state: &AppState, job_id: Uuid, spec: &PrepareSpec) {
    let job_id_str = job_id.to_string();
    touch_prepare(&state.pool, &job_id_str).await;
    // Gancho de teste (B2): kind reservado que panica de propósito para
    // exercitar o `catch_unwind` do `spawn_prepare`. Inalcançável via HTTP —
    // os handlers constroem `PrepareSpec` com kinds literais server-side
    // (`yolo_train`, `autotracker`, `autolabel`, `diffusion_train`,
    // `yolo_predict`); só chega aqui via linha semeada direto no banco
    // (mesmo nível de confiança de qualquer escrita no DB).
    if spec.kind == "__test_panic__" {
        panic!("prepare worker panic hook (test-only kind)");
    }
    report_progress(state, &job_id_str, "preparando dataset", 0.02).await;

    // (a) Cancelamento: abort em `preparing` ⇒ `cancelling` no manager.
    match state.manager.get_job(&job_id_str).await {
        Ok(j) if j.status == "cancelling" || j.status == "cancelled" => {
            mark_prepare_cancelled(&state.pool, job_id).await;
            let _ = state.manager.prepare_cancel(&job_id_str).await;
            return;
        }
        Err(ManagerError::NotFound) => {
            fail_prepare(
                state,
                job_id,
                None,
                false,
                "build_error",
                fail_message("build_error"),
            )
            .await;
            return;
        }
        _ => {}
    }

    // (b) Reuso por fingerprint; miss ⇒ build.
    let pkg = match try_reuse_package(state, spec).await {
        Some(p) => {
            report_progress(state, &job_id_str, "Reutilizando pacote existente em cache", 0.6).await;
            p
        }
        None => {
            report_progress(state, &job_id_str, "Empacotando dataset (extraindo imagens e metadados)...", 0.15).await;
            // Fase longa (S3+zip, minutos em datasets grandes): heartbeat
            // antes e depois para o recovery não ver `updated_at` parado.
            touch_prepare(&state.pool, &job_id_str).await;
            let built = build_for_spec(state, spec).await;
            touch_prepare(&state.pool, &job_id_str).await;
            match built {
                Ok(p) => {
                    let mb = p.bytes as f64 / (1024.0 * 1024.0);
                    let msg = format!("Pacote gerado ({:.1} MB) e sincronizado com sucesso", mb);
                    report_progress(state, &job_id_str, &msg, 0.95).await;
                    p
                }
                Err(code) => {
                    fail_prepare(state, job_id, None, false, code, fail_message(code)).await;
                    return;
                }
            }
        }
    };
    // `created_this_attempt` só é falso no reuso (D4: versão reusada é
    // intocável pela compensação).
    let created_version = if pkg.created_this_attempt {
        Some(pkg.version_id)
    } else {
        None
    };

    // Cancel-check tardio: evita complete após abort concorrente.
    if let Ok(j) = state.manager.get_job(&job_id_str).await {
        if j.status == "cancelling" || j.status == "cancelled" {
            // Versão própria recém-criada seria órfã: compensa (complete nunca
            // chamado) e marca cancelled local.
            if let Some(vid) = created_version {
                let _ = state
                    .storage
                    .delete_prefix(&format!("packages/{vid}/"))
                    .await;
                let _ = sqlx::query("DELETE FROM dataset_versions WHERE id = $1")
                    .bind(vid)
                    .execute(&state.pool)
                    .await;
            }
            mark_prepare_cancelled(&state.pool, job_id).await;
            let _ = state.manager.prepare_cancel(&job_id_str).await;
            return;
        }
    }

    // Sucesso: complete (preparing→queued) + linha done.
    let body = prepare_complete_body(&pkg.version_id, &pkg.key, &pkg.md5_zip, pkg.bytes);
    match state.manager.prepare_complete(&job_id_str, &body).await {
        Ok(()) => {
            mark_prepare_done(&state.pool, job_id).await;
        }
        Err(_) => {
            // Complete nunca confirmado ⇒ compensa versão própria (D4);
            // prepare_fail é best-effort (409 se o job saiu de `preparing`).
            fail_prepare(
                state,
                job_id,
                created_version,
                false,
                "build_error",
                fail_message("build_error"),
            )
            .await;
        }
    }
}

// ---------------------------------------------------------------------------
// Recovery no boot — re-spawna prepares órfãos (espelha `recover_jobs`)
// ---------------------------------------------------------------------------

/// Puro/testável: `new_attempts` (< 3) ainda pode re-spawnar.
pub fn should_respawn(new_attempts: i32) -> bool {
    new_attempts < 3
}

/// Recovery no boot (D3): `preparing` com `updated_at` > 10min ⇒ `attempts+1`;
/// `< 3` re-spawna com o spec salvo, senão `prepare_fail{timeout}`.
///
/// Best-effort como o recover do manager: nunca derruba o boot (retorna
/// `Err` só para log). Retorna `(respawned, failed)`.
pub async fn recover_stale_prepares(state: &AppState) -> Result<(usize, usize), sqlx::Error> {
    let rows: Vec<(Uuid, serde_json::Value, i32)> = sqlx::query_as(
        "SELECT job_id, spec, attempts FROM job_prepares \
         WHERE state = 'preparing' AND updated_at < now() - interval '10 minutes'",
    )
    .fetch_all(&state.pool)
    .await?;
    let mut respawned = 0usize;
    let mut failed = 0usize;
    for (job_id, spec_json, attempts) in rows {
        let new_attempts = attempts + 1;
        if should_respawn(new_attempts) {
            match serde_json::from_value::<PrepareSpec>(spec_json) {
                Ok(spec) => {
                    sqlx::query(
                        "UPDATE job_prepares SET attempts = $2, updated_at = now() \
                         WHERE job_id = $1",
                    )
                    .bind(job_id)
                    .bind(new_attempts)
                    .execute(&state.pool)
                    .await?;
                    spawn_prepare(state.clone(), job_id, spec);
                    respawned += 1;
                }
                Err(_) => {
                    // Spec corrompido: não há como re-spawnar.
                    fail_prepare(
                        state,
                        job_id,
                        None,
                        false,
                        "build_error",
                        "spec de preparação inválido",
                    )
                    .await;
                    failed += 1;
                }
            }
        } else {
            sqlx::query(
                "UPDATE job_prepares SET attempts = $2, updated_at = now() WHERE job_id = $1",
            )
            .bind(job_id)
            .bind(new_attempts)
            .execute(&state.pool)
            .await?;
            fail_prepare(
                state,
                job_id,
                None,
                false,
                "timeout",
                fail_message("timeout"),
            )
            .await;
            failed += 1;
        }
    }
    Ok((respawned, failed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    #[test]
    fn fingerprint_hex_deterministico() {
        let ds = uid(1);
        let ids = vec![uid(10), uid(20)];
        let a = fingerprint_hex(
            &ds,
            &ids,
            false,
            2,
            "2026-09-17T00:00:00Z",
            3,
            42,
            1,
            7,
            "yolo",
            "",
        );
        let b = fingerprint_hex(
            &ds,
            &ids,
            false,
            2,
            "2026-09-17T00:00:00Z",
            3,
            42,
            1,
            7,
            "yolo",
            "",
        );
        assert_eq!(a, b, "mesmas entradas ⇒ mesmo fingerprint");
        assert_eq!(a.len(), 40, "sha1 hex tem 40 chars");
    }

    #[test]
    fn fingerprint_hex_sensivel_a_cada_sinal() {
        let ds = uid(1);
        let ids = vec![uid(10), uid(20)];
        let base = fingerprint_hex(
            &ds,
            &ids,
            false,
            2,
            "2026-01-01T00:00:00Z",
            0,
            0,
            1,
            5,
            "yolo",
            "",
        );
        // Nova imagem no escopo (count + frescor mudam).
        assert_ne!(
            base,
            fingerprint_hex(
                &ds,
                &ids,
                false,
                3,
                "2026-09-17T00:00:00Z",
                0,
                0,
                1,
                5,
                "yolo",
                ""
            )
        );
        // Edição in-place de box (só o sig muda).
        assert_ne!(
            base,
            fingerprint_hex(
                &ds,
                &ids,
                false,
                2,
                "2026-01-01T00:00:00Z",
                3,
                99,
                1,
                5,
                "yolo",
                ""
            )
        );
        // Rename de classe (só o sig muda).
        assert_ne!(
            base,
            fingerprint_hex(
                &ds,
                &ids,
                false,
                2,
                "2026-01-01T00:00:00Z",
                0,
                0,
                1,
                6,
                "yolo",
                ""
            )
        );
        // Ordem das ids não importa (ordenadas dentro da função).
        let rev = vec![uid(20), uid(10)];
        assert_eq!(
            base,
            fingerprint_hex(
                &ds,
                &rev,
                false,
                2,
                "2026-01-01T00:00:00Z",
                0,
                0,
                1,
                5,
                "yolo",
                ""
            )
        );
        // Escopo ALL ≠ escopo explícito; engine/extra distinguem.
        assert_ne!(
            base,
            fingerprint_hex(
                &ds,
                &[],
                true,
                2,
                "2026-01-01T00:00:00Z",
                0,
                0,
                1,
                5,
                "yolo",
                ""
            )
        );
        assert_ne!(
            base,
            fingerprint_hex(
                &ds,
                &ids,
                false,
                2,
                "2026-01-01T00:00:00Z",
                0,
                0,
                1,
                5,
                "diffusion",
                "gato"
            )
        );
    }

    #[test]
    fn prepare_spec_camel_case_roundtrip() {
        let spec = PrepareSpec {
            kind: "autolabel".into(),
            dataset_id: uid(7),
            resolved_image_ids: Some(vec![uid(3)]),
            fingerprint: "abc".into(),
            engine: "autolabel".into(),
            trigger_word: None,
            params: serde_json::json!({"model": "mock"}),
        };
        let v = serde_json::to_value(&spec).expect("serialize");
        assert_eq!(v["datasetId"], serde_json::json!(uid(7).to_string()));
        assert!(v.get("triggerWord").is_none(), "None é omitido");
        assert!(v["resolvedImageIds"].as_array().unwrap().len() == 1);
        let back: PrepareSpec = serde_json::from_value(v).expect("roundtrip");
        assert_eq!(back.kind, "autolabel");
        assert_eq!(back.resolved_image_ids.unwrap().len(), 1);
    }

    #[test]
    fn spec_params_autolabel_nunca_carrega_apikey() {
        let p = spec_params_autolabel(
            "openai",
            Some("descreva"),
            Some("https://api.openai.com/v1"),
            Some("gpt-4o"),
            Some("low"),
            None,
            Some(3),
        );
        let s = serde_json::to_string(&p).expect("serialize");
        assert!(!s.contains("apiKey"), "apiKey não é persistida no spec");
        assert!(!s.contains("api_key"), "nem em snake_case");
        assert!(!s.contains("sk-"), "nenhum valor de segredo");
        assert_eq!(p["model"], "openai");
        assert_eq!(p["prompt"], "descreva");
        assert_eq!(p["apiBase"], "https://api.openai.com/v1");
    }

    #[test]
    fn bodies_wire_camel_case() {
        let vid = uid(9);
        let c = prepare_complete_body(
            &vid,
            "packages/x/dataset.zip",
            "d41d8cd98f00b204e9800998ecf8427e",
            10,
        );
        assert_eq!(c["datasetVersionId"], vid.to_string());
        assert_eq!(c["packageRef"]["key"], "packages/x/dataset.zip");
        assert_eq!(
            c["packageRef"]["md5Zip"],
            "d41d8cd98f00b204e9800998ecf8427e"
        );
        assert_eq!(c["packageRef"]["bytes"], 10);
        assert!(c.get("dataset_version_id").is_none(), "sem snake_case");
        let f = prepare_fail_body("timeout", "tempo esgotado no empacotamento");
        assert_eq!(f["code"], "timeout");
        let r = prepare_report_body("packaging_dataset", "empacotando dataset", 0.1);
        assert_eq!(r["status"], "preparing");
        assert_eq!(r["phase"], "packaging_dataset");
        assert_eq!(r["progress"], 0.1);
    }

    #[test]
    fn should_respawn_so_abaixo_de_3() {
        assert!(should_respawn(1));
        assert!(should_respawn(2));
        assert!(!should_respawn(3));
        assert!(!should_respawn(99));
    }

    #[tokio::test]
    async fn prepare_cancel_mock_recebe_chamada() {
        use crate::jobs::manager_client::ManagerPort;
        let mock = crate::jobs::manager_client::MockManager::default();
        mock.prepare_cancel("job-123")
            .await
            .expect("prepare_cancel");
        let calls = mock.prepare_cancel_calls.lock().unwrap().clone();
        assert_eq!(calls, vec!["job-123"]);
    }
}
