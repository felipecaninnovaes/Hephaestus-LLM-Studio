// ---------------------------------------------------------------------------
// GPU telemetry: nvidia-smi com fallback silencioso (D7, B1 Multi-GPU)
// ---------------------------------------------------------------------------

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

pub use heph_contracts::GpuDeviceTelemetry;

/// Resultado da amostragem do nvidia-smi.
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
    /// Dispositivos GPU individuais detalhados.
    pub devices: Vec<GpuDeviceTelemetry>,
}

/// Estado interno em cache para evitar invocações simultâneas/frequentes do nvidia-smi.
pub struct GpuSamplerCache {
    pub last_sampled: Option<Instant>,
    pub telemetry: Option<GpuTelemetry>,
}

/// Sampler compartilhado de GPU com cache de ~2s.
#[derive(Clone)]
pub struct GpuSampler {
    pub cache: Arc<RwLock<GpuSamplerCache>>,
    ttl: Duration,
}

impl Default for GpuSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuSampler {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(GpuSamplerCache {
                last_sampled: None,
                telemetry: None,
            })),
            ttl: Duration::from_millis(2000),
        }
    }

    /// Obtém telemetria recente em cache ou realiza nova amostragem via nvidia-smi.
    pub async fn sample(&self) -> Option<GpuTelemetry> {
        {
            let cache = self.cache.read().await;
            if let Some(last) = cache.last_sampled {
                if last.elapsed() < self.ttl {
                    return cache.telemetry.clone();
                }
            }
        }

        let mut cache = self.cache.write().await;
        // Double-check após adquirir o lock de escrita
        if let Some(last) = cache.last_sampled {
            if last.elapsed() < self.ttl {
                return cache.telemetry.clone();
            }
        }

        let fresh = run_nvidia_smi().await;
        cache.last_sampled = Some(Instant::now());
        cache.telemetry = fresh.clone();
        fresh
    }

    /// API de leitura por UUID para uso dos collectors (fatia B3).
    pub async fn get_device_by_uuid(&self, uuid: &str) -> Option<GpuDeviceTelemetry> {
        let telemetry = self.sample().await?;
        telemetry.devices.into_iter().find(|d| d.uuid == uuid)
    }
}

/// Executa `nvidia-smi` com timeout 2s e kill_on_drop.
pub async fn run_nvidia_smi() -> Option<GpuTelemetry> {
    let output = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::process::Command::new("nvidia-smi")
            .args([
                "--query-gpu=index,uuid,name,memory.total,memory.used,power.draw,utilization.gpu,temperature.gpu",
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

/// Compatibilidade: função de conveniência que roda uma amostragem direta.
pub async fn try_nvidia_smi() -> Option<GpuTelemetry> {
    run_nvidia_smi().await
}

fn parse_optional_f64(raw: &str) -> Option<f64> {
    let s = raw.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("[n/a]") || s.eq_ignore_ascii_case("[not supported]")
    {
        None
    } else {
        s.parse::<f64>().ok()
    }
}

fn parse_optional_i32(raw: &str) -> Option<i32> {
    let s = raw.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("[n/a]") || s.eq_ignore_ascii_case("[not supported]")
    {
        None
    } else {
        s.parse::<i32>().ok()
    }
}

/// Valida se string se parece com UUID de GPU nvidia (`GPU-...`)
fn is_valid_gpu_uuid(uuid: &str) -> bool {
    let s = uuid.trim();
    s.starts_with("GPU-") && s.len() >= 8 && s.len() <= 64
}

/// Parseia CSV do nvidia-smi de 8 colunas:
/// index, uuid, name, memory.total, memory.used, power.draw, utilization.gpu, temperature.gpu
///
/// Tolerante:
/// - Campos opcionais ([N/A], [Not Supported], vazio) -> None sem descartar a placa.
/// - Falha em index, uuid, name, memory.total, memory.used descarta só a linha afetada.
pub fn parse_nvidia_smi_csv(csv: &str) -> Option<GpuTelemetry> {
    let mut gpus = Vec::new();
    let mut devices = Vec::new();
    let mut vram_total_mib: i64 = 0;
    let mut vram_used_mib: i64 = 0;
    let mut max_gpu_mib: i64 = 0;

    for line in csv.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if parts.len() < 8 {
            // Compatibilidade retroativa com CSV legado de 3 colunas: "name, memory.total, memory.used"
            if parts.len() == 3 {
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
            continue;
        }

        // Coluna 0: index (u32)
        let index: u32 = match parts[0].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Coluna 1: uuid
        let uuid = parts[1].to_string();
        if !is_valid_gpu_uuid(&uuid) {
            continue;
        }

        // Coluna 2: name
        let name = parts[2].to_string();
        if name.is_empty() {
            continue;
        }

        // Coluna 3: memory.total (i64 MiB)
        let total: i64 = match parts[3].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Coluna 4: memory.used (i64 MiB)
        let used: i64 = match parts[4].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Colunas 5, 6, 7: opcionais
        let power_watts = parse_optional_f64(parts[5]);
        let gpu_utilization_pct = parse_optional_f64(parts[6]);
        let temperature_c = parse_optional_i32(parts[7]);

        let dev = GpuDeviceTelemetry {
            index,
            uuid,
            name: name.clone(),
            vram_total: total,
            vram_used: used,
            power_watts,
            gpu_utilization_pct,
            temperature_c,
        };

        devices.push(dev);
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
        devices,
    })
}
