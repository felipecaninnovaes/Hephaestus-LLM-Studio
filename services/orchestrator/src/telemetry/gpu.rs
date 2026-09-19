// ---------------------------------------------------------------------------
// GPU telemetry: nvidia-smi com fallback silencioso (D7)
// ---------------------------------------------------------------------------

/// Resultado do parse do nvidia-smi.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuTelemetry {
    /// Nomes das GPUs visíveis (ex.: "NVIDIA GeForce RTX 3060").
    pub gpus: Vec<String>,
    /// VRAM total somada em MiB (nvidia-smi reporta MiB).
    pub vram_total: i64,
    /// VRAM usada somada em MiB.
    pub vram_used: i64,
    /// Maior VRAM total individual entre as GPUs visíveis (MiB).
    /// 1 job = 1 GPU (backend.md §6) — capacidade real de treino de 1 job.
    pub max_gpu_mib: i64,
}

/// Tenta rodar `nvidia-smi` e parsear o CSV de saída.
/// Retorna `None` se o binário não existir ou falhar (fallback silencioso).
pub async fn try_nvidia_smi() -> Option<GpuTelemetry> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tokio::process::Command::new("nvidia-smi")
            .args([
                "--query-gpu=name,memory.total,memory.used",
                "--format=csv,noheader,nounits",
            ])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_nvidia_smi_csv(&stdout)
}

/// Parseia CSV do nvidia-smi (nomes + soma de VRAM em MiB + max individual).
/// Linhas malformadas são ignoradas (skip silencioso).
pub fn parse_nvidia_smi_csv(csv: &str) -> Option<GpuTelemetry> {
    let mut gpus = Vec::new();
    let mut vram_total_mib: i64 = 0;
    let mut vram_used_mib: i64 = 0;
    let mut max_gpu_mib: i64 = 0;

    for line in csv.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Formato: "NVIDIA GeForce RTX 3060, 12288, 0"
        let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if parts.len() < 3 {
            continue; // linha malformada → ignora
        }
        let name = parts[0].to_string();
        let total: i64 = match parts[1].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let used: i64 = match parts[2].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        gpus.push(name);
        vram_total_mib += total;
        vram_used_mib += used;
        if total > max_gpu_mib {
            max_gpu_mib = total;
        }
    }

    if gpus.is_empty() {
        return None;
    }

    Some(GpuTelemetry {
        gpus,
        vram_total: vram_total_mib,
        vram_used: vram_used_mib,
        max_gpu_mib,
    })
}
