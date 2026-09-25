//! Constantes e funções puras de domínio do manager (MM-15).

/// Estados terminais — os únicos apagáveis.
pub const TERMINAL_STATUSES: [&str; 3] = ["done", "failed", "cancelled"];

#[inline]
pub fn is_terminal_status(status: &str) -> bool {
    TERMINAL_STATUSES.contains(&status)
}

/// Normaliza arquitetura de modelos de difusão para formato canônico string slice.
pub fn normalize_diffusion_arch_str(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "sdxl" => Some("sdxl"),
        "sd15" | "sd1.5" | "sd_15" | "sd-15" | "sd 15" => Some("sd15"),
        "flux" | "flux2" | "flux-2-klein" | "flux-2-klein-4b" | "flux2-klein-4b" => {
            Some("flux-2-klein-4b")
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
