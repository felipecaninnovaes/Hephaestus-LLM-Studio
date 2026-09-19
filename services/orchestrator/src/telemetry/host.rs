// ---------------------------------------------------------------------------
// Telemetry (CPU/RAM from /proc — D9, LIMITAÇÃO documentada E3)
// ---------------------------------------------------------------------------

/// Lê CPU usage do /proc/stat.
///
/// LIMITAÇÃO: em container Linux, /proc reflete o host em single-node.
/// Em ambiente multi-node, os valores podem não corresponder ao host real.
pub fn read_cpu() -> f64 {
    let content = match std::fs::read_to_string("/proc/stat") {
        Ok(c) => c,
        Err(_) => return 0.0,
    };

    let first_line = match content.lines().next() {
        Some(l) => l,
        None => return 0.0,
    };

    // Formato: "cpu  user nice system idle iowait irq softirq steal"
    let parts: Vec<u64> = first_line
        .split_whitespace()
        .skip(1)
        .filter_map(|s| s.parse().ok())
        .collect();

    if parts.len() < 4 {
        return 0.0;
    }

    let idle = parts[3];
    let total: u64 = parts.iter().sum();

    if total == 0 {
        return 0.0;
    }

    ((total - idle) as f64 / total as f64) * 100.0
}

/// Lê RAM usada do /proc/meminfo (em bytes).
///
/// LIMITAÇÃO: mesma do CPU — reflete o host em Linux single-node.
pub fn read_ram() -> i64 {
    let content = match std::fs::read_to_string("/proc/meminfo") {
        Ok(c) => c,
        Err(_) => return 0,
    };

    let mut total = 0i64;
    let mut available = 0i64;

    for line in content.lines() {
        if let Some(v) = line.strip_prefix("MemTotal:") {
            total = v
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0)
                * 1024; // kB → B
        }
        if let Some(v) = line.strip_prefix("MemAvailable:") {
            available = v
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0)
                * 1024; // kB → B
        }
    }

    total - available
}

/// Lê RAM total do /proc/meminfo (em bytes).
///
/// Returns `None` se a leitura ou parse falhar (nunca pânico, nunca valor inventado).
pub fn read_ram_total() -> Option<i64> {
    let content = std::fs::read_to_string("/proc/meminfo").ok()?;
    parse_ram_total_from_content(&content)
}

/// Parseia o conteúdo de /proc/meminfo e devolve MemTotal em bytes.
/// Parseia o conteúdo de /proc/meminfo e devolve MemTotal em bytes.
///
/// Visibilidade pública para re-export via `telemetry` (testes usam via `super::*`).
pub fn parse_ram_total_from_content(content: &str) -> Option<i64> {
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("MemTotal:") {
            let kb: i64 = v.split_whitespace().next().and_then(|s| s.parse().ok())?;
            return Some(kb * 1024); // kB → B
        }
    }
    None
}
