use super::*;
use uuid::Uuid;

#[test]
fn is_valid_md5_correct() {
    assert!(is_valid_md5("d41d8cd98f00b204e9800998ecf8427e"));
}

#[test]
fn is_valid_md5_uppercase() {
    assert!(!is_valid_md5("D41D8CD98F00B204E9800998ECF8427E"));
}

#[test]
fn is_valid_md5_wrong_length() {
    assert!(!is_valid_md5("d41d8cd98f00b204e9800998ecf8427"));
}

#[test]
fn is_valid_md5_non_hex() {
    assert!(!is_valid_md5("d41d8cd98f00b204e9800998ecf8427g"));
}

// -- done_artifacts_violation: defesa no_artifacts (incidente galeria vazia) --

fn art(kind: &str) -> ArtifactItem {
    ArtifactItem {
        kind: kind.into(),
        path: format!("{kind}.bin"),
        md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
        bytes: 10,
    }
}

#[test]
fn done_violation_diffusion_generate_sem_artifacts() {
    assert!(done_artifacts_violation("diffusion_generate", None).is_some());
    assert!(done_artifacts_violation("diffusion_generate", Some(&[])).is_some());
}

#[test]
fn done_violation_diffusion_generate_sem_generated() {
    let arts = vec![art("generated_meta"), art("generated_thumb")];
    assert!(done_artifacts_violation("diffusion_generate", Some(&arts)).is_some());
}

#[test]
fn done_violation_diffusion_generate_ok() {
    let arts = vec![art("generated"), art("generated_meta")];
    assert!(done_artifacts_violation("diffusion_generate", Some(&arts)).is_none());
}

#[test]
fn done_violation_yolo_train_vazio_preservado() {
    // Preservação (t7_ac006a_*, abort_em_voo_e_terminal): done vazio segue done.
    assert!(done_artifacts_violation("yolo_train", None).is_none());
    assert!(done_artifacts_violation("yolo_train", Some(&[])).is_none());
}

#[test]
fn done_violation_yolo_train_exige_modelo() {
    assert!(done_artifacts_violation("yolo_train", Some(&[art("model")])).is_none());
    let arts = vec![art("metrics")];
    assert!(done_artifacts_violation("yolo_train", Some(&arts)).is_some());
}

#[test]
fn done_violation_treinos_e_predicao_exigem_lista() {
    for kind in [
        "diffusion_train",
        "yolo_predict",
        "autotracker",
        "autolabel",
    ] {
        assert!(
            done_artifacts_violation(kind, None).is_some(),
            "{kind} vazio deve violar"
        );
        assert!(
            done_artifacts_violation(kind, Some(&[])).is_some(),
            "{kind} vazio deve violar"
        );
        assert!(
            done_artifacts_violation(kind, Some(&[art("model")])).is_none(),
            "{kind} com artefato deve passar"
        );
    }
}

#[test]
fn done_violation_kind_desconhecido_permissivo() {
    assert!(done_artifacts_violation("futura_engine_x", None).is_none());
}

#[test]
fn normalize_cleanup_statuses_default_sao_terminais() {
    let got = normalize_cleanup_statuses(None).unwrap();
    assert_eq!(
        got,
        vec!["done".to_string(), "failed".into(), "cancelled".into()]
    );
    // Lista vazia → mesmo default.
    let got2 = normalize_cleanup_statuses(Some(&Vec::new())).unwrap();
    assert_eq!(got2, got);
}

#[test]
fn normalize_cleanup_statuses_rejeita_nao_terminal() {
    let bad = vec!["running".to_string()];
    let err = normalize_cleanup_statuses(Some(&bad)).unwrap_err();
    assert!(matches!(err, ManagerError::InvalidRequest(_)));
    // Subset válido passa.
    let ok = vec!["done".to_string()];
    assert_eq!(
        normalize_cleanup_statuses(Some(&ok)).unwrap(),
        vec!["done".to_string()]
    );
}

#[test]
fn http_orchestrator_client_stores_token() {
    let client = HttpOrchestratorClient::new(Some("tok_test".into()));
    assert!(client.token.as_deref() == Some("tok_test"));
}

#[test]
fn http_orchestrator_client_none_token() {
    let client = HttpOrchestratorClient::new(None);
    assert!(client.token.is_none());
}

#[test]
fn auto_adopt_enabled_none_is_true() {
    assert!(auto_adopt_enabled(None));
}

#[test]
fn auto_adopt_enabled_one_is_true() {
    assert!(auto_adopt_enabled(Some("1")));
}

#[test]
fn auto_adopt_enabled_zero_is_false() {
    assert!(!auto_adopt_enabled(Some("0")));
}

#[test]
fn auto_adopt_enabled_empty_is_true() {
    assert!(auto_adopt_enabled(Some("")));
}

/// Agregação: 2 nós, 1 stale (>10s) → somente o fresco conta.
#[test]
fn agregacao_filtro_stale() {
    use super::TelemetryState;
    use chrono::{Duration, Utc};
    use std::collections::HashMap;

    let mut cache: HashMap<Uuid, TelemetryState> = HashMap::new();
    let now = Utc::now();

    // Nó fresco (heartbeat agora).
    cache.insert(
        Uuid::new_v4(),
        TelemetryState {
            endpoint: "http://fresh:8082".into(),
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

    // Nó stale (heartbeat 60s atrás).
    cache.insert(
        Uuid::new_v4(),
        TelemetryState {
            endpoint: "http://stale:8082".into(),
            measured: true,
            vram_used: Some(8000),
            vram_total: Some(24000),
            cpu: Some(0.9),
            ram: Some(16384),
            ram_total: Some(67108864000),
            gpus: vec!["RTX 4090".into()],
            jobs_active: 5,
            last_heartbeat: Some(now - Duration::seconds(60)),
        },
    );

    // Simula a lógica de filtro do get_telemetry (>1 nós).
    let mut vram_used_sum: Option<i64> = Some(0);
    let mut vram_total_sum: Option<i64> = Some(0);
    let mut gpus: Vec<String> = Vec::new();
    let mut jobs_active_sum: i32 = 0;
    let mut measured = false;

    for state in cache.values() {
        let is_fresh = state
            .last_heartbeat
            .map(|last| (now - last).num_seconds() <= 10)
            .unwrap_or(false);
        if is_fresh {
            measured = true;
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
    }

    assert!(measured);
    // Só o nó fresh conta.
    assert_eq!(vram_used_sum, Some(3000));
    assert_eq!(vram_total_sum, Some(12000));
    assert_eq!(jobs_active_sum, 1);
    assert_eq!(gpus, vec!["RTX 3060"]);
    // Nó stale NÃO entra na soma.
    assert!(!gpus.contains(&"RTX 4090".to_string()));
}

/// Agregação: 2 nós ambos stale → fallback (measured:false).
#[test]
fn agregacao_todos_stale_fallback() {
    use super::TelemetryState;
    use chrono::{Duration, Utc};
    use std::collections::HashMap;

    let mut cache: HashMap<Uuid, TelemetryState> = HashMap::new();
    let now = Utc::now();

    cache.insert(
        Uuid::new_v4(),
        TelemetryState {
            endpoint: "http://a:8082".into(),
            measured: true,
            vram_used: Some(3000),
            vram_total: Some(12000),
            gpus: vec!["GPU_A".into()],
            jobs_active: 1,
            last_heartbeat: Some(now - Duration::seconds(30)),
            ..Default::default()
        },
    );
    cache.insert(
        Uuid::new_v4(),
        TelemetryState {
            endpoint: "http://b:8082".into(),
            measured: true,
            vram_used: Some(2000),
            vram_total: Some(6000),
            gpus: vec!["GPU_B".into()],
            jobs_active: 2,
            last_heartbeat: Some(now - Duration::seconds(60)),
            ..Default::default()
        },
    );

    let mut measured = false;
    for state in cache.values() {
        let is_fresh = state
            .last_heartbeat
            .map(|last| (now - last).num_seconds() <= 10)
            .unwrap_or(false);
        if is_fresh {
            measured = true;
        }
    }

    assert!(!measured, "ambos stale → measured deve ser false");
}

#[test]
fn test_slugify() {
    assert_eq!(slugify("Meu Dataset Incrível!"), "meu-dataset-incrivel");
    assert_eq!(slugify("test__model--v1"), "test-model-v1");
    assert_eq!(
        slugify("   leading and trailing   "),
        "leading-and-trailing"
    );
    assert_eq!(slugify("cbr_pnk-123"), "cbr-pnk-123");
}

#[test]
fn test_compute_model_name_custom_output_name() {
    let job_id = Uuid::new_v4();
    let params = serde_json::json!({
        "output_name": "meu-personagem-v1"
    });
    let name = compute_model_name(
        "adapter.safetensors",
        "diffusion",
        "flux",
        job_id,
        Some("retratos"),
        &params,
    );
    assert_eq!(name, "meu-personagem-v1.safetensors");

    // Já vem com a extensão
    let params2 = serde_json::json!({
        "output_name": "meu-personagem-v1.safetensors"
    });
    let name2 = compute_model_name(
        "adapter.safetensors",
        "diffusion",
        "flux",
        job_id,
        Some("retratos"),
        &params2,
    );
    assert_eq!(name2, "meu-personagem-v1.safetensors");

    // Custom output para yolo com espaços
    let params3 = serde_json::json!({
        "output_name": "detector de pragas v2"
    });
    let name3 = compute_model_name(
        "weights/best.pt",
        "yolo",
        "yolo11m",
        job_id,
        Some("insetos"),
        &params3,
    );
    assert_eq!(name3, "detector-de-pragas-v2.pt");
}

#[test]
fn test_compute_model_name_semantic_defaults() {
    let job_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();

    // Diffusion com trigger word e dataset
    let params = serde_json::json!({
        "trigger_word": "cbrpnk"
    });
    let name = compute_model_name(
        "adapter.safetensors",
        "diffusion",
        "flux",
        job_id,
        Some("cyberpunk-city"),
        &params,
    );
    assert_eq!(name, "cyberpunk-city-flux2-cbrpnk.safetensors");

    // Diffusion sem trigger word (usa prefixo do job id)
    let params_no_trigger = serde_json::json!({});
    let name2 = compute_model_name(
        "adapter.safetensors",
        "diffusion",
        "sdxl",
        job_id,
        Some("cyberpunk-city"),
        &params_no_trigger,
    );
    assert_eq!(name2, "cyberpunk-city-sdxl-550e8400.safetensors");

    // YOLO com dataset
    let name_yolo = compute_model_name(
        "weights/best.pt",
        "yolo",
        "yolo11m",
        job_id,
        Some("veiculos-urbanos"),
        &serde_json::json!({}),
    );
    assert_eq!(name_yolo, "veiculos-urbanos-yolo11m-best.pt");

    // Fallback sem dataset
    let name_no_ds = compute_model_name(
        "adapter.safetensors",
        "diffusion",
        "sd15",
        job_id,
        None,
        &serde_json::json!({ "trigger_word": "estilo" }),
    );
    assert_eq!(name_no_ds, "sd15-estilo.safetensors");
}

#[test]
fn test_validate_update_model() {
    let ok = UpdateModelRequest {
        name: "novo-nome.safetensors".to_string(),
    };
    assert!(validate_update_model(&ok).is_ok());

    let empty = UpdateModelRequest {
        name: "   ".to_string(),
    };
    assert!(validate_update_model(&empty).is_err());

    let too_long = UpdateModelRequest {
        name: "a".repeat(256),
    };
    assert!(validate_update_model(&too_long).is_err());
}
#[test]
fn test_validate_create_model_text_encoder() {
    // Helper: request base válido diffusion.
    let base = |kind: Option<&str>, arch: Option<&str>| CreateModelRequest {
        id: Uuid::new_v4(),
        engine: "diffusion".to_string(),
        name: "enc.safetensors".to_string(),
        model: None,
        s3_key: "models/diffusion/x/enc.safetensors".to_string(),
        source: "upload".to_string(),
        url: None,
        hash: "d41d8cd98f00b204e9800998ecf8427e".to_string(),
        bytes: 100,
        job_id: None,
        kind: kind.map(|s| s.to_string()),
        arch: arch.map(|s| s.to_string()),
    };
    // text_encoder + flux-2 ⇒ ok.
    assert!(validate_create_model(&base(Some("text_encoder"), Some("flux-2-klein-4b"))).is_ok());
    // text_encoder + sdxl/sd15/ausente ⇒ 400.
    assert!(validate_create_model(&base(Some("text_encoder"), Some("sdxl"))).is_err());
    assert!(validate_create_model(&base(Some("text_encoder"), Some("sd15"))).is_err());
    assert!(validate_create_model(&base(Some("text_encoder"), None)).is_err());
    // checkpoint segue exigindo arch (inalterado).
    assert!(validate_create_model(&base(Some("checkpoint"), None)).is_err());
    assert!(validate_create_model(&base(Some("checkpoint"), Some("sdxl"))).is_ok());
}

// ── Bug 009: classificação kind/arch de treino de difusão ─────────────

#[test]
fn normalize_diffusion_arch_casos_suportados() {
    assert_eq!(normalize_diffusion_arch("sdxl"), Some("sdxl".into()));
    assert_eq!(normalize_diffusion_arch(" SDXL "), Some("sdxl".into()));
    assert_eq!(normalize_diffusion_arch("sd15"), Some("sd15".into()));
    assert_eq!(normalize_diffusion_arch("SD1.5"), Some("sd15".into()));
    assert_eq!(
        normalize_diffusion_arch("flux"),
        Some("flux-2-klein-4b".into())
    );
    assert_eq!(
        normalize_diffusion_arch("flux-2-klein-4b"),
        Some("flux-2-klein-4b".into())
    );
}

#[test]
fn normalize_diffusion_arch_desconhecido_e_none() {
    assert_eq!(normalize_diffusion_arch("unsupported"), None);
    assert_eq!(normalize_diffusion_arch(""), None);
    assert_eq!(normalize_diffusion_arch("yolo11m"), None);
}

#[test]
fn derive_diffusion_arch_prioridade_params_model_yaml() {
    // params.baseModel (camelCase do BFF) vence jobs.model.
    let p = serde_json::json!({"baseModel": "sd15"});
    assert_eq!(
        derive_diffusion_arch("sdxl", &p, Some("model: \"sdxl\"")),
        Some("sd15".into())
    );
    // snake_case legado também vale.
    let p2 = serde_json::json!({"base_model": "sdxl"});
    assert_eq!(
        derive_diffusion_arch("sd15", &p2, None),
        Some("sdxl".into())
    );
    // Sem params: cai para jobs.model.
    let p3 = serde_json::json!({});
    assert_eq!(
        derive_diffusion_arch("sdxl", &p3, None),
        Some("sdxl".into())
    );
    // Sem params nem model válido: extrai do config_yaml.
    assert_eq!(
        derive_diffusion_arch("", &p3, Some("job_id: \"x\"\nmodel: \"sd15\"\n")),
        Some("sd15".into())
    );
    // Nada derivável: None (não chuta).
    assert_eq!(derive_diffusion_arch("", &p3, None), None);
    assert_eq!(
        derive_diffusion_arch("???", &p3, Some("model: \"???\"")),
        None
    );
    // `openai_model:` (autolabel) não contamina a extração do yaml.
    assert_eq!(
        derive_diffusion_arch(
            "",
            &p3,
            Some("openai_model: \"gpt-4o\"\nmode: \"autolabel\"\n")
        ),
        None
    );
}

#[test]
fn classify_diffusion_model_kind_adapter_vs_checkpoint() {
    assert_eq!(classify_diffusion_model_kind("adapter.safetensors"), "lora");
    assert_eq!(
        classify_diffusion_model_kind("checkpoints/adapter_final.safetensors"),
        "lora"
    );
    assert_eq!(
        classify_diffusion_model_kind("meu-lora-v1.safetensors"),
        "lora"
    );
    assert_eq!(
        classify_diffusion_model_kind("best.safetensors"),
        "checkpoint"
    );
    assert_eq!(
        classify_diffusion_model_kind("merged-model.safetensors"),
        "checkpoint"
    );
}

// ── resolve_diffusion_image ──────────────────────────────────────────

/// Serializa testes que manipulam env global (DIFFUSION_TRAINER_IMAGE).
use std::sync::Mutex;
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Teste 1: env explícito `:local` com image `:gpu` → env vence.
#[test]
fn resolve_diffusion_image_env_local_vence_sobre_gpu() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::set_var(
        "DIFFUSION_TRAINER_IMAGE",
        "hephaestus/trainer-difusao:local",
    );
    let result = super::resolve_diffusion_image("hephaestus/trainer-yolo:gpu");
    assert_eq!(result, "hephaestus/trainer-difusao:local");
    std::env::remove_var("DIFFUSION_TRAINER_IMAGE");
}

/// Teste 2: env ausente + image `:gpu` → herança :gpu (TrueNAS preservado).
#[test]
fn resolve_diffusion_image_sem_env_herda_gpu() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::remove_var("DIFFUSION_TRAINER_IMAGE");
    let result = super::resolve_diffusion_image("hephaestus/trainer-yolo:gpu");
    assert_eq!(result, "hephaestus/trainer-difusao:gpu");
}

/// Teste 3: env ausente + image `:local` → herança :local.
#[test]
fn resolve_diffusion_image_sem_env_herda_local() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::remove_var("DIFFUSION_TRAINER_IMAGE");
    let result = super::resolve_diffusion_image("hephaestus/trainer-yolo:local");
    assert_eq!(result, "hephaestus/trainer-difusao:local");
}

/// Teste 4: env customizado → usa exatamente o env.
#[test]
fn resolve_diffusion_image_env_custom() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::set_var("DIFFUSION_TRAINER_IMAGE", "meu-registry/exemplo:tag");
    let result = super::resolve_diffusion_image("hephaestus/trainer-yolo:gpu");
    assert_eq!(result, "meu-registry/exemplo:tag");
    std::env::remove_var("DIFFUSION_TRAINER_IMAGE");
}
/// RD-020: Verifica extração e compatibilidade de control_package_ref de params com PackageRef.
#[test]
fn control_package_ref_extracted_from_params() {
    let params = serde_json::json!({
        "control_package_ref": {
            "key": "packages/ctrl/ctrl.zip",
            "md5_zip": "0123456789abcdef0123456789abcdef",
            "bytes": 1024
        }
    });
    let cpr = params.get("control_package_ref").cloned();
    assert!(cpr.is_some());
    let pkg: heph_contracts::PackageRef =
        serde_json::from_value(cpr.unwrap()).expect("parse PackageRef");
    assert_eq!(pkg.key, "packages/ctrl/ctrl.zip");
    assert_eq!(pkg.md5_zip, "0123456789abcdef0123456789abcdef");
    assert_eq!(pkg.bytes, 1024);
}
