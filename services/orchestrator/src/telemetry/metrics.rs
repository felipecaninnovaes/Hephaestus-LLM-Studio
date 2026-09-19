use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::domain::models::ReportBody;

// ---------------------------------------------------------------------------
// Metrics parsing (D5 — contrato com F4.5, snake_case)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MetricsLine {
    #[serde(default)]
    pub box_loss: f64,
    #[serde(default)]
    pub cls_loss: f64,
    #[serde(default)]
    pub dfl_loss: f64,
    #[serde(rename = "mAP50", default)]
    pub map50: f64,
    #[serde(rename = "mAP50-95", default)]
    pub map50_95: f64,
    #[serde(default)]
    pub loss: Option<f64>,
    #[serde(default)]
    pub lr: Option<f64>,
    #[serde(default)]
    pub step: Option<i64>,
    pub epoch: i32,
    #[serde(default)]
    pub progress: Option<f64>,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub vram_used_gb: Option<f64>,
}

impl MetricsLine {
    /// AC-006-A D1: linha de métrica de treino carrega ao menos um valor numérico.
    /// Sem valor ⇒ é evento de status (fase/mensagem de boot, progresso por imagem).
    /// Fase em qualquer linha promove `jobs.phase` (D2): métrica com `phase`
    /// continua métrica e carrega a fase junto no report.
    pub fn is_training_metric(&self) -> bool {
        matches!(self.loss, Some(x) if x.is_finite())
            || matches!(self.lr, Some(x) if x.is_finite())
            || matches!(self.box_loss, x if x != 0.0 && x.is_finite())
            || matches!(self.cls_loss, x if x != 0.0 && x.is_finite())
            || matches!(self.dfl_loss, x if x != 0.0 && x.is_finite())
            || matches!(self.map50, x if x != 0.0 && x.is_finite())
            || matches!(self.map50_95, x if x != 0.0 && x.is_finite())
    }

    pub fn to_report_json(&self) -> serde_json::Value {
        let mut obj = serde_json::json!({
            "box_loss": self.box_loss,
            "cls_loss": self.cls_loss,
            "dfl_loss": self.dfl_loss,
            "mAP50": self.map50,
            "mAP50-95": self.map50_95,
            "epoch": self.epoch,
        });
        if let Some(loss) = self.loss {
            obj["loss"] = serde_json::json!(loss);
        }
        if let Some(lr) = self.lr {
            obj["lr"] = serde_json::json!(lr);
        }
        if let Some(step) = self.step {
            obj["step"] = serde_json::json!(step);
        }
        if let Some(p) = self.progress {
            obj["progress"] = serde_json::json!(p);
        }
        if let Some(ref phase) = self.phase {
            obj["phase"] = serde_json::json!(phase);
        }
        if let Some(ref msg) = self.message {
            obj["message"] = serde_json::json!(msg);
        }
        if let Some(vram) = self.vram_used_gb {
            obj["vram_used_gb"] = serde_json::json!(vram);
        }
        obj
    }
}

/// Parse tolerante de uma linha de metrics.jsonl ou telemetry.jsonl.
/// Linhas malformadas são ignoradas (skip silencioso).
pub fn parse_metrics_line(line: &str) -> Option<MetricsLine> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    // Sanitiza literais float não-padrão (ex.: : NaN ou : Infinity emitidos por runtimes legados/Python)
    let clean_line = if line.contains("NaN") || line.contains("Infinity") {
        line.replace(": NaN", ": null")
            .replace(": -NaN", ": null")
            .replace(": Infinity", ": null")
            .replace(": -Infinity", ": null")
    } else {
        line.to_string()
    };
    let v: serde_json::Value = serde_json::from_str(&clean_line).ok()?;
    let epoch = v.get("epoch").and_then(|e| e.as_i64()).or_else(|| {
        if v.get("phase").is_some() || v.get("progress").is_some() {
            Some(0)
        } else {
            None
        }
    })? as i32;
    let phase = v
        .get("phase")
        .and_then(|p| p.as_str())
        .map(|s| s.to_string());
    let message = v
        .get("phaseMessage")
        .or_else(|| v.get("message"))
        .and_then(|m| m.as_str())
        .map(|s| s.to_string());
    let vram_used_gb = v
        .get("vramUsedGb")
        .or_else(|| v.get("vram_used_gb"))
        .and_then(|x| x.as_f64());
    Some(MetricsLine {
        box_loss: v.get("box_loss").and_then(|x| x.as_f64()).unwrap_or(0.0),
        cls_loss: v.get("cls_loss").and_then(|x| x.as_f64()).unwrap_or(0.0),
        dfl_loss: v.get("dfl_loss").and_then(|x| x.as_f64()).unwrap_or(0.0),
        map50: v.get("mAP50").and_then(|x| x.as_f64()).unwrap_or(0.0),
        map50_95: v.get("mAP50-95").and_then(|x| x.as_f64()).unwrap_or(0.0),
        loss: v.get("loss").and_then(|x| x.as_f64()),
        lr: v.get("lr").and_then(|x| x.as_f64()),
        step: v.get("step").and_then(|x| x.as_i64()),
        epoch,
        progress: v.get("progress").and_then(|p| p.as_f64()),
        phase,
        message,
        vram_used_gb,
    })
}

/// Calcula progress a partir de uma linha de métricas.
/// Se a linha contiver `progress` explícito (ex.: emitido pelo autolabel ou outro runner),
/// honra esse valor diretamente; caso contrário calcula (epoch / total_epochs).
pub fn compute_progress(line: &MetricsLine, total_epochs: i32) -> f64 {
    if let Some(p) = line.progress {
        return p.clamp(0.0, 1.0);
    }
    if total_epochs <= 0 {
        return 0.0;
    }
    ((line.epoch as f64) / (total_epochs as f64)).clamp(0.0, 1.0)
}

/// Lê linhas novas de um arquivo JSONL a partir de um offset (contagem de linhas).
/// Retorna `(linhas_parseadas, novo_offset)`. O offset SEMPRE avança para o
/// total de linhas lidas — inclusive sobre linhas malformadas (skip silencioso
/// via `parse_metrics_line`), para não reprocessar lixo a cada tick.
///
/// Arquivo ausente ou ilegível → `(vec![], offset)` sem erro: o produtor pode
/// ainda não ter criado o arquivo (ex.: daemon ainda carregando pipeline).
///
/// Compartilhada entre o collector do one-shot (`metrics.jsonl`) e o tail do
/// path daemon (`telemetry.jsonl`, D1 — ADR-0023): mesmo formato de relatório
/// nos dois paths.
pub fn tail_jsonl_lines(path: &Path, lines_read: usize) -> (Vec<MetricsLine>, usize) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return (Vec::new(), lines_read),
    };
    let lines: Vec<&str> = content.lines().collect();
    // Arquivo truncado (rotação): recomeça do zero em vez de pular tudo.
    let start = if lines.len() >= lines_read {
        lines_read
    } else {
        0
    };
    let mut parsed = Vec::new();
    for line in &lines[start..] {
        if let Some(m) = parse_metrics_line(line) {
            parsed.push(m);
        }
    }
    (parsed, lines.len())
}

/// Constrói o `ReportBody` de progresso para uma linha de telemetria/metrics.
///
/// Formato idêntico ao do collector do one-shot: status "running", progress
/// via `compute_progress` (honra `progress` explícito da linha), métrica só
/// quando `is_training_metric()`, e `phase`/`message` promovidas via COALESCE
/// no `report_job` do manager.
pub fn telemetry_report_for_line(line: &MetricsLine, total_epochs: i32) -> ReportBody {
    let progress = compute_progress(line, total_epochs);
    let is_metric = line.is_training_metric();
    ReportBody {
        status: "running".to_string(),
        progress: Some(progress),
        epoch: Some(line.epoch),
        step: line.step.map(|s| s as i32),
        metrics: if is_metric {
            Some(line.to_report_json())
        } else {
            None
        },
        error: None,
        artifacts: None,
        meta_content: None,
        phase: line.phase.clone(),
        message: line.message.clone(),
    }
}
