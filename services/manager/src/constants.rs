//! Constantes e funções puras de domínio do manager (MM-15).

/// Estados terminais — os únicos apagáveis.
pub const TERMINAL_STATUSES: [&str; 3] = ["done", "failed", "cancelled"];

#[inline]
pub fn is_terminal_status(status: &str) -> bool {
    TERMINAL_STATUSES.contains(&status)
}

pub const ARCH_FLUX2_KLEIN_4B: &str = "flux-2-klein-4b";
pub const ARCH_FLUX2_KLEIN_9B: &str = "flux-2-klein-9b";

/// Arch canônico da família FLUX.2 Klein (4B ou 9B)? Regras compartilhadas
/// (samplers, emissão de YAML) valem para a família; text encoder custom e
/// checkpoint custom são regras só-4B e ficam explícitas nos chamadores.
pub fn is_flux2_klein_family(arch: &str) -> bool {
    arch == ARCH_FLUX2_KLEIN_4B || arch == ARCH_FLUX2_KLEIN_9B
}

/// Normaliza arquitetura de modelos de difusão para formato canônico string slice.
pub fn normalize_diffusion_arch_str(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "sdxl" => Some("sdxl"),
        "sd15" | "sd1.5" | "sd_15" | "sd-15" | "sd 15" => Some("sd15"),
        "flux-2-klein-9b" | "flux2-klein-9b" => Some(ARCH_FLUX2_KLEIN_9B),
        "flux" | "flux2" | "flux-2-klein" | "flux-2-klein-4b" | "flux2-klein-4b" => {
            Some(ARCH_FLUX2_KLEIN_4B)
        }
        "qwen" | "qwen-image" | "qwen-image-2.1" | "qwen2.1" | "qwen_image" | "qwen-image-2-1" => {
            Some("qwen-image-2.1")
        }
        _ => None,
    }
}

/// Normaliza arquitetura de modelos de difusão para formato canônico String (retrocompatibilidade).
pub fn normalize_diffusion_arch(raw: &str) -> Option<String> {
    normalize_diffusion_arch_str(raw).map(|s| s.to_string())
}

/// LoRA com `arch` não nulo e diferente do arch efetivo da geração é
/// incompatível (shapes distintos entre 4B e 9B). `arch` NULL (legado) passa.
/// Ambos os lados são normalizados (alias legado `flux` ⇒ 4B).
pub fn lora_arch_mismatch(lora_arch: Option<&str>, effective_arch: &str) -> bool {
    let Some(lora) = lora_arch else {
        return false;
    };
    let lora_norm = normalize_diffusion_arch_str(lora).unwrap_or(lora);
    let eff_norm = normalize_diffusion_arch_str(effective_arch).unwrap_or(effective_arch);
    lora_norm != eff_norm
}

/// Classifica o kind de um artefato de difusão baseado no caminho do arquivo.
pub fn classify_diffusion_model_kind(art_path: &str) -> &'static str {
    let lower = art_path.to_ascii_lowercase();
    if lower.contains("adapter") || lower.contains("lora") {
        "lora"
    } else {
        "checkpoint"
    }
}

pub const ERROR_PREPARE_TIMEOUT: &str = "prepare_timeout";
pub const ERROR_RECOVERED: &str = "recovered";
pub const ERROR_ORCHESTRATOR_OFFLINE: &str = "orchestrator_offline";
