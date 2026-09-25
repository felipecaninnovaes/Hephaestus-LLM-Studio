//! Hook de catalogação de modelos produzidos e helpers de nomenclatura (ADR-0012, ADR-0022, MM-11).

use sqlx::PgConnection;
use uuid::Uuid;

use super::artifacts::ArtifactItem;
use crate::constants::{classify_diffusion_model_kind, normalize_diffusion_arch};
use crate::error::ManagerError;

/// Sanitiza uma string para slug seguro (apenas a-z, 0-9 e hífen).
pub fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_dash = true; // evita dash inicial
    for c in s.chars() {
        let normalized = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => {
                'a'
            }
            'è' | 'é' | 'ê' | 'ë' | 'È' | 'É' | 'Ê' | 'Ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' => 'i',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' => 'o',
            'ù' | 'ú' | 'û' | 'ü' | 'Ù' | 'Ú' | 'Û' | 'Ü' => 'u',
            'ç' | 'Ç' => 'c',
            'ñ' | 'Ñ' => 'n',
            other => other.to_ascii_lowercase(),
        };
        if normalized.is_ascii_alphanumeric() {
            out.push(normalized);
            last_dash = false;
        } else if (normalized == '-' || normalized == '_' || normalized == ' ') && !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    if out.ends_with('-') {
        out.pop();
    }
    out
}

/// Deriva o `arch` de um job de treino de difusão, por prioridade:
///
/// 1. `params.baseModel` (wire camelCase do BFF) / `params.base_model` (legado);
/// 2. coluna `jobs.model` (= base_model do treino);
/// 3. linha `model: "x"` do `config_yaml` gerado pelo api-principal.
///
/// Retorna None se nenhuma fonte normalizar (não chuta).
pub fn derive_diffusion_arch(
    job_model: &str,
    params: &serde_json::Value,
    config_yaml: Option<&str>,
) -> Option<String> {
    for key in ["baseModel", "base_model"] {
        if let Some(s) = params.get(key).and_then(|v| v.as_str()) {
            if let Some(arch) = normalize_diffusion_arch(s) {
                return Some(arch);
            }
        }
    }
    if let Some(arch) = normalize_diffusion_arch(job_model) {
        return Some(arch);
    }
    if let Some(yaml) = config_yaml {
        for line in yaml.lines() {
            if let Some(rest) = line.trim().strip_prefix("model:") {
                let v = rest.trim().trim_matches('"').trim_matches('\'').trim();
                if let Some(arch) = normalize_diffusion_arch(v) {
                    return Some(arch);
                }
            }
        }
    }
    None
}

/// Deriva o nome do modelo registrado na tabela models (ADR-0022 D0).
pub fn compute_model_name(
    art_path: &str,
    engine: &str,
    model: &str,
    job_id: Uuid,
    dataset_slug: Option<&str>,
    params: &serde_json::Value,
) -> String {
    let default_filename = art_path.rsplit('/').next().unwrap_or(art_path);
    let ext = default_filename.rsplit('.').next().unwrap_or("");

    // 1. Se o usuário forneceu output_name explicitamente (D1)
    if let Some(out_name) = params
        .get("output_name")
        .or_else(|| params.get("outputName"))
        .and_then(|v| v.as_str())
    {
        let clean = out_name.trim();
        if !clean.is_empty() {
            let (base_name, user_ext) = if let Some((base, user_ext)) = clean.rsplit_once('.') {
                if user_ext.eq_ignore_ascii_case("safetensors")
                    || user_ext.eq_ignore_ascii_case("pt")
                {
                    (base, Some(user_ext))
                } else {
                    (clean, None)
                }
            } else {
                (clean, None)
            };
            let slugged_base = slugify(base_name);
            if !slugged_base.is_empty() {
                let final_ext = user_ext.unwrap_or(ext);
                if !final_ext.is_empty() {
                    return format!("{slugged_base}.{final_ext}");
                }
                return slugged_base;
            }
        }
    }

    // 2. Derivação semântica inteligente (D0)
    let job_hex = job_id.to_string();
    let short_id = &job_hex[..8.min(job_hex.len())];

    let ds_slug = dataset_slug.map(slugify).filter(|s| !s.is_empty());

    let clean_model = match model.to_ascii_lowercase().as_str() {
        "flux" | "flux-2-klein-4b" => "flux2".to_string(),
        "sdxl" => "sdxl".to_string(),
        "sd15" => "sd15".to_string(),
        "qwen" | "qwen-image" | "qwen-image-2.1" | "qwen2.1" | "qwen_image" | "qwen-image-2-1" => {
            "qwen2.1".to_string()
        }
        other => slugify(other),
    };

    if engine == "diffusion" {
        let trigger = params
            .get("trigger_word")
            .or_else(|| params.get("triggerWord"))
            .and_then(|v| v.as_str())
            .map(slugify)
            .filter(|s| !s.is_empty());

        let suffix = trigger.as_deref().unwrap_or(short_id);
        let ext_str = if ext.is_empty() { "safetensors" } else { ext };

        if let Some(ds) = ds_slug {
            format!("{ds}-{clean_model}-{suffix}.{ext_str}")
        } else {
            format!("{clean_model}-{suffix}.{ext_str}")
        }
    } else if engine == "yolo" {
        let ext_str = if ext.is_empty() { "pt" } else { ext };
        if let Some(ds) = ds_slug {
            format!("{ds}-{clean_model}-best.{ext_str}")
        } else {
            format!("{clean_model}-{short_id}-best.{ext_str}")
        }
    } else {
        default_filename.to_string()
    }
}

/// Hook: registra best.pt/safetensors na tabela models (ADR-0012 D1).
/// Executa sob conexão transacional. Best-effort: falha loga warn e não aborta.
pub async fn hook_models_on_done(
    conn: &mut PgConnection,
    id: Uuid,
    artifacts: &[ArtifactItem],
) -> Result<(), ManagerError> {
    let best_models: Vec<_> = artifacts
        .iter()
        .filter(|a| {
            a.kind == "model"
                && (a.path.contains("best")
                    || a.path.contains("adapter")
                    || a.path.ends_with(".safetensors"))
        })
        .collect();

    if best_models.is_empty() {
        return Ok(());
    }

    type JobModelHookInfo = (
        String,
        String,
        String,
        String,
        Option<Uuid>,
        serde_json::Value,
        Option<String>,
    );

    let job_info: Option<JobModelHookInfo> = match sqlx::query_as::<_, JobModelHookInfo>(
        "SELECT engine, model, mode, kind, dataset_id, params, config_yaml FROM jobs WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await
    {
        Ok(opt) => opt,
        Err(e) => {
            tracing::warn!(
                "hook models: falha ao ler engine/model/dataset_id/params do job {id}: {e}"
            );
            None
        }
    };

    let (engine, model, mode, kind, dataset_id, job_params, config_yaml) = match job_info {
        Some(info) => info,
        None => return Ok(()),
    };

    let dataset_slug: Option<String> = if let Some(ds_id) = dataset_id {
        match sqlx::query_scalar::<_, String>("SELECT slug FROM datasets WHERE id = $1")
            .bind(ds_id)
            .fetch_optional(&mut *conn)
            .await
        {
            Ok(opt) => opt,
            Err(e) => {
                tracing::warn!("hook models: falha ao ler slug do dataset {ds_id}: {e}");
                None
            }
        }
    } else {
        None
    };

    let is_diffusion_train =
        engine == "diffusion" && (mode == "train" || kind == "diffusion_train");
    let train_arch: Option<String> = if is_diffusion_train {
        match derive_diffusion_arch(&model, &job_params, config_yaml.as_deref()) {
            Some(a) => Some(a),
            None => {
                tracing::warn!(
                    job_id = %id,
                    "hook models: arch indeterminável p/ treino de difusão (kind será 'lora', arch NULL)"
                );
                None
            }
        }
    } else {
        None
    };

    for art in best_models {
        let s3_key = format!("artifacts/{id}/{}", art.path);
        let model_name = compute_model_name(
            &art.path,
            &engine,
            &model,
            id,
            dataset_slug.as_deref(),
            &job_params,
        );
        let (art_kind, art_arch): (Option<String>, Option<String>) = if is_diffusion_train {
            (
                Some(classify_diffusion_model_kind(&art.path).to_string()),
                train_arch.clone(),
            )
        } else {
            (None, None)
        };

        let result = sqlx::query(
            "INSERT INTO models (id, engine, name, model, s3_key, source, hash, bytes, job_id, kind, arch) \
             VALUES ($1, $2, $3, $4, $5, 'train', $6, $7, $8, $9, $10) \
             ON CONFLICT (s3_key) DO UPDATE SET kind = COALESCE(models.kind, EXCLUDED.kind), arch = COALESCE(models.arch, EXCLUDED.arch) WHERE models.kind IS NULL OR models.arch IS NULL",
        )
        .bind(Uuid::new_v4())
        .bind(&engine)
        .bind(&model_name)
        .bind(Some(model.clone()))
        .bind(&s3_key)
        .bind(&art.md5)
        .bind(art.bytes)
        .bind(id)
        .bind(&art_kind)
        .bind(&art_arch)
        .execute(&mut *conn)
        .await;

        if let Err(e) = result {
            tracing::warn!(
                job_id = %id,
                s3_key = %s3_key,
                error = %e,
                "falha ao registrar modelo na tabela models (best-effort)"
            );
        }
    }

    Ok(())
}
