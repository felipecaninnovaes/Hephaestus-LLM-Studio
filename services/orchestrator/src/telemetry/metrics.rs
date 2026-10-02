use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use crate::domain::models::ReportBody;

// ---------------------------------------------------------------------------
// Metrics parsing (D5 — contrato com F4.5, snake_case)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MetricsLine {
    #[serde(default)]
    pub box_loss: Option<f64>,
    #[serde(default)]
    pub cls_loss: Option<f64>,
    #[serde(default)]
    pub dfl_loss: Option<f64>,
    #[serde(rename = "mAP50", default)]
    pub map50: Option<f64>,
    #[serde(rename = "mAP50-95", default)]
    pub map50_95: Option<f64>,
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
    #[serde(default)]
    pub nan_count: Option<i64>,
    #[serde(default)]
    pub inf_count: Option<i64>,
    /// Passthrough de qualquer chave numérica finita do dict `metrics` da
    /// linha de telemetria que não é um campo YOLO/diffusion conhecido
    /// acima (ex.: `grad_norm` emitido pelo trainer-difusao). Preserva o
    /// valor exato sem inventar zeros para campos ausentes (bug do job
    /// c64b9b74-1c53-4111-a48a-7ab472283654).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, f64>,
}

impl MetricsLine {
    /// AC-006-A D1: linha de métrica de treino carrega ao menos um valor numérico.
    /// Sem valor ⇒ é evento de status (fase/mensagem de boot, progresso por imagem).
    /// Fase em qualquer linha promove `jobs.phase` (D2): métrica com `phase`
    /// continua métrica e carrega a fase junto no report.
    pub fn is_training_metric(&self) -> bool {
        matches!(self.loss, Some(x) if x.is_finite())
            || matches!(self.lr, Some(x) if x.is_finite())
            || matches!(self.box_loss, Some(x) if x != 0.0 && x.is_finite())
            || matches!(self.cls_loss, Some(x) if x != 0.0 && x.is_finite())
            || matches!(self.dfl_loss, Some(x) if x != 0.0 && x.is_finite())
            || matches!(self.map50, Some(x) if x != 0.0 && x.is_finite())
            || matches!(self.map50_95, Some(x) if x != 0.0 && x.is_finite())
            || matches!(self.nan_count, Some(n) if n > 0)
            || matches!(self.inf_count, Some(n) if n > 0)
            || !self.extra.is_empty()
    }

    pub fn to_report_json(&self) -> serde_json::Value {
        let mut obj = serde_json::json!({ "epoch": self.epoch });
        if let Some(box_loss) = self.box_loss {
            obj["box_loss"] = serde_json::json!(box_loss);
        }
        if let Some(cls_loss) = self.cls_loss {
            obj["cls_loss"] = serde_json::json!(cls_loss);
        }
        if let Some(dfl_loss) = self.dfl_loss {
            obj["dfl_loss"] = serde_json::json!(dfl_loss);
        }
        if let Some(map50) = self.map50 {
            obj["mAP50"] = serde_json::json!(map50);
        }
        if let Some(map50_95) = self.map50_95 {
            obj["mAP50-95"] = serde_json::json!(map50_95);
        }
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
        if let Some(phase) = &self.phase {
            obj["phase"] = serde_json::json!(phase);
        }
        if let Some(msg) = &self.message {
            obj["message"] = serde_json::json!(msg);
        }
        if let Some(vram) = self.vram_used_gb {
            obj["vram_used_gb"] = serde_json::json!(vram);
        }
        if let Some(nan_count) = self.nan_count {
            obj["nan_count"] = serde_json::json!(nan_count);
        }
        if let Some(inf_count) = self.inf_count {
            obj["inf_count"] = serde_json::json!(inf_count);
        }
        for (key, value) in &self.extra {
            obj[key] = serde_json::json!(value);
        }
        obj
    }
}

/// Chaves do dict `metrics`/topo já mapeadas para campos conhecidos de
/// `MetricsLine` — excluídas do passthrough genérico em `extra` para não
/// duplicar.
const KNOWN_METRIC_KEYS: &[&str] = &[
    "box_loss", "cls_loss", "dfl_loss", "mAP50", "mAP50-95", "loss", "lr", "step", "epoch",
    "progress",
];

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
    // C2b (RD-022/ADR-0023): telemetry.jsonl aninha loss/lr sob "metrics"
    // (engine-kit/telemetry.py, trainer-difusao/common_pkg/metrics.py); só o
    // espelho legado metrics.jsonl achata no topo. Topo vence; aninhado é fallback.
    let nested = v.get("metrics");
    let diag = v.get("diagnostics");
    let num = |key: &str| -> Option<f64> {
        v.get(key)
            .and_then(|x| x.as_f64())
            .or_else(|| nested.and_then(|m| m.get(key)).and_then(|x| x.as_f64()))
    };
    let int = |key: &str| -> Option<i64> {
        v.get(key)
            .and_then(|x| x.as_i64())
            .or_else(|| nested.and_then(|m| m.get(key)).and_then(|x| x.as_i64()))
    };
    let diag_int = |key_camel: &str, key_snake: &str| -> Option<i64> {
        diag.and_then(|d| d.get(key_camel).or_else(|| d.get(key_snake)))
            .and_then(|x| x.as_i64())
            .or_else(|| int(key_snake))
            .or_else(|| int(key_camel))
    };
    let epoch = int("epoch").or_else(|| {
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
    // Passthrough genérico: TODAS as chaves numéricas finitas do dict
    // `metrics` aninhado que não são campos YOLO/diffusion já conhecidos
    // (ex.: `grad_norm` do trainer-difusao, spec fatia 3b). Sem isso o
    // report descartava silenciosamente qualquer chave nova emitida pelo
    // engine.
    let mut extra = BTreeMap::new();
    if let Some(m) = nested.and_then(|m| m.as_object()) {
        for (key, value) in m {
            if KNOWN_METRIC_KEYS.contains(&key.as_str()) {
                continue;
            }
            if let Some(x) = value.as_f64() {
                if x.is_finite() {
                    extra.insert(key.clone(), x);
                }
            }
        }
    }

    // Achatamento de systemMetrics (fatia 3a):
    // systemMetrics {cpuPct, ramUsedGb, diskReadMbS, diskWriteMbS} ->
    // sys.cpu_pct, sys.ram_used_gb, sys.disk_read_mb_s, sys.disk_write_mb_s
    let sys_metrics = v.get("systemMetrics").or_else(|| v.get("system_metrics"));
    if let Some(sm) = sys_metrics.and_then(|s| s.as_object()) {
        let field_mappings = [
            ("cpuPct", "sys.cpu_pct"),
            ("cpu_pct", "sys.cpu_pct"),
            ("ramUsedGb", "sys.ram_used_gb"),
            ("ram_used_gb", "sys.ram_used_gb"),
            ("diskReadMbS", "sys.disk_read_mb_s"),
            ("disk_read_mb_s", "sys.disk_read_mb_s"),
            ("diskWriteMbS", "sys.disk_write_mb_s"),
            ("disk_write_mb_s", "sys.disk_write_mb_s"),
        ];
        for (src_key, dest_key) in field_mappings {
            if extra.contains_key(dest_key) {
                continue;
            }
            if let Some(val) = sm.get(src_key).and_then(|x| x.as_f64()) {
                if val.is_finite() {
                    extra.insert(dest_key.to_string(), val);
                }
            }
        }
    }
    Some(MetricsLine {
        box_loss: num("box_loss"),
        cls_loss: num("cls_loss"),
        dfl_loss: num("dfl_loss"),
        map50: num("mAP50"),
        map50_95: num("mAP50-95"),
        loss: num("loss"),
        lr: num("lr"),
        step: int("step"),
        epoch,
        progress: v.get("progress").and_then(|p| p.as_f64()),
        phase,
        message,
        vram_used_gb,
        nan_count: diag_int("nanCount", "nan_count"),
        inf_count: diag_int("infCount", "inf_count"),
        extra,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Linha telemetry.jsonl canônica: loss/lr aninhados sob "metrics".
    fn nested_line() -> &'static str {
        r#"{"timestamp":"2026-09-20T00:00:00Z","phase":"training","phaseMessage":"Treinando","progress":0.5,"step":30,"epoch":3,"metrics":{"loss":0.0452,"lr":0.0001}}"#
    }

    #[test]
    fn parse_telemetry_nested_metrics() {
        let m = parse_metrics_line(nested_line()).expect("linha aninhada deve parsear");
        assert_eq!(m.epoch, 3);
        assert_eq!(m.step, Some(30));
        assert_eq!(m.loss, Some(0.0452));
        assert_eq!(m.lr, Some(0.0001));
        assert!(m.is_training_metric());
        let report = telemetry_report_for_line(&m, 10);
        assert!(report.metrics.is_some());
    }

    /// Regressão do bug confirmado no smoke GPU real (job
    /// c64b9b74-1c53-4111-a48a-7ab472283654, SD1.5): uma linha de difusão
    /// (`metrics.grad_norm`) deve repassar `grad_norm` no report e NÃO
    /// inventar `box_loss`/`mAP50`/etc. com 0; uma linha YOLO real continua
    /// trazendo suas próprias chaves inalteradas.
    #[test]
    fn diffusion_line_passes_grad_norm_without_fabricating_yolo_zeros() {
        let line = r#"{"timestamp":"2026-09-20T00:00:00Z","phase":"training","progress":0.5,"step":30,"epoch":3,"metrics":{"loss":0.0452,"lr":0.0001,"grad_norm":1.23},"diagnostics":{"nanCount":0,"infCount":0}}"#;
        let m = parse_metrics_line(line).expect("linha de difusão deve parsear");
        assert_eq!(m.extra.get("grad_norm"), Some(&1.23));
        assert_eq!(m.box_loss, None);
        assert_eq!(m.map50, None);
        let json = m.to_report_json();
        assert_eq!(json["grad_norm"], 1.23);
        assert!(json.get("box_loss").is_none());
        assert!(json.get("mAP50").is_none());
        assert!(json.get("mAP50-95").is_none());
    }

    #[test]
    fn yolo_line_unchanged_by_extra_passthrough() {
        let line = r#"{"box_loss":0.045,"cls_loss":0.067,"dfl_loss":0.123,"mAP50":0.912,"mAP50-95":0.654,"epoch":1}"#;
        let m = parse_metrics_line(line).expect("linha YOLO deve parsear");
        assert_eq!(m.box_loss, Some(0.045));
        assert_eq!(m.cls_loss, Some(0.067));
        assert_eq!(m.dfl_loss, Some(0.123));
        assert_eq!(m.map50, Some(0.912));
        assert_eq!(m.map50_95, Some(0.654));
        assert!(m.extra.is_empty());
        let json = m.to_report_json();
        assert_eq!(json["box_loss"], 0.045);
        assert_eq!(json["mAP50-95"], 0.654);
        assert!(json.get("grad_norm").is_none());
    }

    #[test]
    fn parse_legacy_flat_unchanged() {
        let line = r#"{"epoch":3,"step":30,"loss":0.0452,"lr":0.0001}"#;
        let m = parse_metrics_line(line).expect("linha flat deve parsear");
        assert_eq!(m.epoch, 3);
        assert_eq!(m.step, Some(30));
        assert_eq!(m.loss, Some(0.0452));
        assert_eq!(m.lr, Some(0.0001));
        assert!(m.is_training_metric());
    }

    #[test]
    fn parse_top_wins_over_nested() {
        let line = r#"{"epoch":3,"step":30,"loss":0.09,"metrics":{"loss":0.0452,"lr":0.0001}}"#;
        let m = parse_metrics_line(line).expect("deve parsear");
        assert_eq!(m.loss, Some(0.09));
        assert_eq!(m.lr, Some(0.0001));
    }

    #[test]
    fn parse_empty_and_status_not_training() {
        assert!(parse_metrics_line("{}").is_none());
        let status = r#"{"phase":"preparing","phaseMessage":"Preparando","progress":0.05,"step":0,"epoch":0}"#;
        let m = parse_metrics_line(status).expect("evento de status deve parsear");
        assert_eq!(m.epoch, 0);
        assert!(!m.is_training_metric());
        let report = telemetry_report_for_line(&m, 10);
        assert!(report.metrics.is_none());
    }

    #[test]
    fn parse_diagnostics_nan_and_inf_flattening() {
        let line =
            r#"{"epoch":1,"step":10,"diagnostics":{"nanCount":3,"infCount":1,"gradNormL2":null}}"#;
        let m = parse_metrics_line(line).expect("diagnostics line should parse");
        assert_eq!(m.nan_count, Some(3));
        assert_eq!(m.inf_count, Some(1));
        assert!(m.is_training_metric());
        let json = m.to_report_json();
        assert_eq!(json["nan_count"], 3);
        assert_eq!(json["inf_count"], 1);
    }

    #[test]
    fn parse_diagnostics_absent_or_null_does_not_break() {
        let line = r#"{"epoch":1,"step":10,"loss":0.5,"diagnostics":null}"#;
        let m = parse_metrics_line(line).expect("line with null diagnostics should parse");
        assert_eq!(m.nan_count, None);
        assert_eq!(m.inf_count, None);
        assert_eq!(m.loss, Some(0.5));
        let json = m.to_report_json();
        assert!(json.get("nan_count").is_none());
    }
    #[test]
    fn parse_system_metrics_flattening() {
        let line = r#"{
            "timestamp": "2026-10-02T12:00:00Z",
            "phase": "training",
            "progress": 0.5,
            "step": 10,
            "epoch": 1,
            "systemMetrics": {
                "cpuPct": 45.2,
                "ramUsedGb": 12.8,
                "diskReadMbS": 150.5,
                "diskWriteMbS": 35.0
            }
        }"#;
        let m = parse_metrics_line(line).expect("systemMetrics line should parse");
        assert!(m.is_training_metric());
        assert_eq!(m.extra.get("sys.cpu_pct"), Some(&45.2));
        assert_eq!(m.extra.get("sys.ram_used_gb"), Some(&12.8));
        assert_eq!(m.extra.get("sys.disk_read_mb_s"), Some(&150.5));
        assert_eq!(m.extra.get("sys.disk_write_mb_s"), Some(&35.0));

        let report = telemetry_report_for_line(&m, 10);
        let metrics = report.metrics.expect("metrics should be present");
        assert_eq!(metrics["sys.cpu_pct"], 45.2);
        assert_eq!(metrics["sys.ram_used_gb"], 12.8);
        assert_eq!(metrics["sys.disk_read_mb_s"], 150.5);
        assert_eq!(metrics["sys.disk_write_mb_s"], 35.0);
    }

    #[test]
    fn parse_system_metrics_absent_does_not_produce_sys_keys() {
        let line = r#"{
            "timestamp": "2026-10-02T12:00:00Z",
            "phase": "training",
            "progress": 0.5,
            "step": 10,
            "epoch": 1,
            "metrics": {
                "loss": 0.05
            }
        }"#;
        let m = parse_metrics_line(line).expect("line without systemMetrics should parse");
        assert_eq!(m.extra.get("sys.cpu_pct"), None);
        assert_eq!(m.extra.get("sys.ram_used_gb"), None);
        assert_eq!(m.extra.get("sys.disk_read_mb_s"), None);
        assert_eq!(m.extra.get("sys.disk_write_mb_s"), None);
        assert!(!m.extra.keys().any(|k| k.starts_with("sys.")));

        let report = telemetry_report_for_line(&m, 10);
        let metrics = report.metrics.expect("metrics should be present");
        assert!(metrics.get("sys.cpu_pct").is_none());
    }
}
