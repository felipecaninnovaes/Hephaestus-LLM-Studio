//! Templating de `config.yaml` e extração de epochs (D6).
//!
//! Movido verbatim de `crate::lib` (Fatia 6): interpolação de placeholders
//! de paths reais de staging + leitura do total de epochs para progresso.

/// Substitui placeholders no config.yaml.
///
/// Suporta:
/// - `{dataset_path}`, `{output_path}` — sempre
/// - `{weights_path}` — weights legado (fine-tune)
/// - `{lora_path_0}`...`{lora_path_N}` — LoRAs multi-ref (D3)
/// - `{custom_checkpoint_path}` — checkpoint custom (D4)
/// - `{text_encoder_path}` — text encoder custom (fatia feat/pesos-custom-flux2)
/// - `{init_image_path}` — imagem inicial img2img (S4 — feat/img2img)
/// - `{control_dataset_path}` — dataset de regularização/controle (treino difusão)
///
/// Placeholders absentes no yaml são ignorados (no-op tolerante).
pub fn replace_config_placeholders(
    config: &str,
    dataset_path: &str,
    output_path: &str,
    weights_path: Option<&str>,
    lora_paths: &[String],
    custom_checkpoint_path: Option<&str>,
    init_image_path: Option<&str>,
    control_dataset_path: Option<&str>,
    text_encoder_path: Option<&str>,
) -> String {
    let mut result = config
        .replace("{dataset_path}", dataset_path)
        .replace("{output_path}", output_path);

    match weights_path {
        Some(wp) => result = result.replace("{weights_path}", wp),
        None => {}
    }

    for (i, path) in lora_paths.iter().enumerate() {
        let placeholder = format!("{{lora_path_{i}}}");
        result = result.replace(&placeholder, path);
    }

    if let Some(cp) = custom_checkpoint_path {
        result = result.replace("{custom_checkpoint_path}", cp);
    }

    if let Some(tp) = text_encoder_path {
        result = result.replace("{text_encoder_path}", tp);
    }

    if let Some(ip) = init_image_path {
        result = result.replace("{init_image_path}", ip);
    }

    if let Some(cd) = control_dataset_path {
        result = result.replace("{control_dataset_path}", cd);
    }

    result
}

/// Substitui placeholders (versão legada — sem multi-ref).
/// Mantida para compatibilidade interna.
pub fn replace_config_placeholders_legacy(
    config: &str,
    dataset_path: &str,
    output_path: &str,
    weights_path: Option<&str>,
) -> String {
    replace_config_placeholders(
        config,
        dataset_path,
        output_path,
        weights_path,
        &[],
        None,
        None,
        None,
        None,
    )
}
/// Extrai o valor de `epochs` do config.yaml (para cálculo de progress).
pub fn extract_epochs(config_yaml: &str) -> i32 {
    let parsed = serde_yaml::from_str::<serde_yaml::Value>(config_yaml).ok();
    if let Some(v) = parsed.as_ref() {
        if let Some(ep) = v.get("epochs").and_then(|e| e.as_i64()) {
            return ep as i32;
        }
        if let Some(ep) = v
            .get("lora")
            .and_then(|l| l.get("epochs"))
            .and_then(|e| e.as_i64())
        {
            return ep as i32;
        }
    }
    100
}
