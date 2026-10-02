//! Fan-out de eventos do manager via `pg_notify('job_events', …)` (fatia 1b,
//! spec `telemetria-observabilidade.md` §3.2/§3.3): substitui o polling de
//! 300 ms por cliente SSE por um único `PgListener` de fundo + broadcast
//! in-process por job.
//!
//! Fluxo:
//! 1. `run_job_events_listener` mantém UM `PgListener` ouvindo o canal
//!    `job_events` (reconecta com backoff exponencial se cair).
//! 2. Cada notice bruto só carrega um ponteiro (`{"jobId":...,"seq":...}` ou
//!    `{"jobId":...,"status":true}`) — nunca o dado em si.
//! 3. Rajadas do mesmo job dentro de 100 ms são coalescidas: UMA busca ao
//!    manager por janela, não por notice nem por cliente.
//! 4. `run_fallback_poller` cobre quedas do listener: poll de 5s
//!    compartilhado (um por job, não por cliente) enquanto `listening` é
//!    `false`.
//! 5. `JobEventsHub` guarda um `broadcast::Sender<JobEvent>` por job com
//!    assinantes ativos; o handler SSE (`stream_job_events`) assina/dessina.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use sqlx::postgres::PgListener;
use tokio::sync::{broadcast, Mutex};

use crate::jobs::handlers::helpers::to_job_response;
use crate::jobs::handlers::types::{JobTelemetryEvent, JobTelemetryEventExt};
use crate::jobs::manager_client::ManagerPort;

const CHANNEL: &str = "job_events";
const DEBOUNCE: Duration = Duration::from_millis(100);
const FALLBACK_POLL_INTERVAL: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Evento pronto para fan-out (já serializado) — o handler SSE só grava no
/// stream, sem buscar nada de novo.
#[derive(Debug, Clone)]
pub struct JobEvent {
    pub event: &'static str,
    /// `Some` só para `metrics` (vira `id:` SSE — `Last-Event-ID` na reconexão).
    pub id: Option<String>,
    pub data: String,
}

#[derive(Deserialize)]
struct RawPayload {
    #[serde(rename = "jobId")]
    job_id: String,
    seq: Option<i64>,
    #[serde(default)]
    status: bool,
}

#[derive(Default)]
struct PendingState {
    metrics: bool,
    status: bool,
    scheduled: bool,
}

/// Último estado conhecido de telemetria de um job — usado pra decidir se
/// um novo fetch mudou algo (mesma heurística do polling antigo, agora
/// centralizada no hub em vez de por-cliente).
struct TelemetryState {
    progress: f64,
    phase: String,
    phase_message: Option<String>,
    step: Option<i64>,
    epoch: Option<i32>,
    vram_used_gb: Option<f64>,
}

impl From<&JobTelemetryEvent> for TelemetryState {
    fn from(t: &JobTelemetryEvent) -> Self {
        Self {
            progress: t.progress,
            phase: t.phase.clone(),
            phase_message: t.phase_message.clone(),
            step: t.step,
            epoch: t.epoch,
            vram_used_gb: t.vram_used_gb,
        }
    }
}

/// Hub compartilhado (um por processo, vive em `AppState`).
pub struct JobEventsHub {
    channels: Mutex<HashMap<String, broadcast::Sender<JobEvent>>>,
    pending: Mutex<HashMap<String, PendingState>>,
    last_seq: Mutex<HashMap<String, i64>>,
    last_telemetry: Mutex<HashMap<String, TelemetryState>>,
    /// `true` enquanto o `PgListener` está conectado e com `LISTEN` ativo.
    pub listening: AtomicBool,
}

impl JobEventsHub {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            channels: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            last_seq: Mutex::new(HashMap::new()),
            last_telemetry: Mutex::new(HashMap::new()),
            listening: AtomicBool::new(false),
        })
    }

    /// Assina eventos de um job (cria o canal se for o 1º assinante).
    pub async fn subscribe(&self, job_id: &str) -> broadcast::Receiver<JobEvent> {
        let mut channels = self.channels.lock().await;
        if let Some(tx) = channels.get(job_id) {
            tx.subscribe()
        } else {
            let (tx, rx) = broadcast::channel(64);
            channels.insert(job_id.to_string(), tx);
            rx
        }
    }

    /// Chamado pelo handler ao desconectar: remove o canal do mapa quando
    /// este era o último assinante (broadcast::Sender nunca fecha sozinho —
    /// `receiver_count() == 0` é o único jeito confiável de saber).
    pub async fn unsubscribe_if_empty(&self, job_id: &str) {
        let mut channels = self.channels.lock().await;
        if let Some(tx) = channels.get(job_id) {
            if tx.receiver_count() == 0 {
                channels.remove(job_id);
                self.last_seq.lock().await.remove(job_id);
                self.last_telemetry.lock().await.remove(job_id);
            }
        }
    }

    async fn has_subscribers(&self, job_id: &str) -> bool {
        self.channels
            .lock()
            .await
            .get(job_id)
            .map(|tx| tx.receiver_count() > 0)
            .unwrap_or(false)
    }

    /// Jobs atualmente com >=1 assinante (usado pelo fallback poller).
    async fn subscribed_job_ids(&self) -> Vec<String> {
        self.channels
            .lock()
            .await
            .iter()
            .filter(|(_, tx)| tx.receiver_count() > 0)
            .map(|(id, _)| id.clone())
            .collect()
    }
    /// Faz refetch de status + métricas (com afterSeq=last_seq) para TODOS os jobs
    /// atualmente com assinantes. Usado na reconexão do listener para cobrir a janela
    /// de silêncio/queda.
    pub async fn refetch_subscribed_jobs(&self, manager: &dyn ManagerPort) {
        for job_id in self.subscribed_job_ids().await {
            process_job(self, manager, &job_id, true, true).await;
        }
    }

    /// Despacha um evento diretamente aos canais (público para o hub e testes).
    pub fn dispatch_sync(
        channels: &HashMap<String, broadcast::Sender<JobEvent>>,
        job_id: &str,
        event: JobEvent,
    ) {
        if let Some(tx) = channels.get(job_id) {
            let _ = tx.send(event);
        }
    }

    /// Helper para testes: envia evento para os assinantes de um job.
    #[cfg(test)]
    pub async fn emit_test_event(&self, job_id: &str, event: JobEvent) {
        let channels = self.channels.lock().await;
        Self::dispatch_sync(&channels, job_id, event);
    }

    /// `afterSeq` conhecido pelo hub pra um job (usado na reconexão com
    /// `Last-Event-ID` pra decidir o delta inicial, e entre fetches
    /// sucessivos de métricas).
    pub async fn known_seq(&self, job_id: &str) -> Option<i64> {
        self.last_seq.lock().await.get(job_id).copied()
    }

    pub async fn set_known_seq(&self, job_id: &str, seq: i64) {
        self.last_seq.lock().await.insert(job_id.to_string(), seq);
    }
}

/// Agenda (com coalescência) o processamento de um job após um notice.
/// Rajadas dentro de `DEBOUNCE` colapsam numa única busca ao manager.
async fn schedule(
    hub: Arc<JobEventsHub>,
    manager: Arc<dyn ManagerPort>,
    job_id: String,
    is_metrics: bool,
    is_status: bool,
) {
    let mut pending = hub.pending.lock().await;
    let entry = pending.entry(job_id.clone()).or_default();
    entry.metrics |= is_metrics;
    entry.status |= is_status;
    if entry.scheduled {
        return;
    }
    entry.scheduled = true;
    drop(pending);

    let hub2 = Arc::clone(&hub);
    let manager2 = Arc::clone(&manager);
    tokio::spawn(async move {
        tokio::time::sleep(DEBOUNCE).await;
        let (do_metrics, do_status) = {
            let mut pending = hub2.pending.lock().await;
            match pending.remove(&job_id) {
                Some(p) => (p.metrics, p.status),
                None => (false, false),
            }
        };
        process_job(&hub2, manager2.as_ref(), &job_id, do_metrics, do_status).await;
    });
}

/// Faz a(s) busca(s) necessária(s) ao manager (UMA por tipo, não por
/// cliente) e distribui o resultado pronto aos assinantes atuais do job.
async fn process_job(
    hub: &JobEventsHub,
    manager: &dyn ManagerPort,
    job_id: &str,
    do_metrics: bool,
    do_status: bool,
) {
    if !hub.has_subscribers(job_id).await {
        return;
    }

    if do_metrics {
        let after = hub.known_seq(job_id).await;
        if let Ok(resp) = manager
            .get_job_metric_points(job_id, after, None, None)
            .await
        {
            if !resp.items.is_empty() || after.is_none() {
                hub.set_known_seq(job_id, resp.max_seq).await;
                if !resp.items.is_empty() {
                    let data = serde_json::json!({
                        "items": resp.items,
                        "maxSeq": resp.max_seq,
                    })
                    .to_string();
                    let event = JobEvent {
                        event: "metrics",
                        id: Some(resp.max_seq.to_string()),
                        data,
                    };
                    let channels = hub.channels.lock().await;
                    JobEventsHub::dispatch_sync(&channels, job_id, event);
                }
            }
        }
    }

    if do_status {
        if let Ok(job) = manager.get_job(job_id).await {
            let jr = to_job_response(job);
            let telemetry = JobTelemetryEvent::from_job_response(&jr);
            let is_terminal = matches!(jr.status.as_str(), "done" | "failed" | "cancelled");

            let changed = {
                let mut last = hub.last_telemetry.lock().await;
                let changed = match last.get(job_id) {
                    None => true,
                    Some(prev) => {
                        (telemetry.progress - prev.progress).abs() > 0.0001
                            || telemetry.phase != prev.phase
                            || telemetry.phase_message != prev.phase_message
                            || telemetry.step != prev.step
                            || telemetry.epoch != prev.epoch
                            || (telemetry.vram_used_gb.unwrap_or(0.0)
                                - prev.vram_used_gb.unwrap_or(0.0))
                            .abs()
                                > 0.05
                    }
                } || is_terminal;
                last.insert(job_id.to_string(), TelemetryState::from(&telemetry));
                changed
            };

            if changed {
                let event_type = if is_terminal { "finished" } else { "telemetry" };
                let data = serde_json::to_string(&telemetry).unwrap_or_default();
                let event = JobEvent {
                    event: event_type,
                    id: None,
                    data,
                };
                let channels = hub.channels.lock().await;
                JobEventsHub::dispatch_sync(&channels, job_id, event);
            }
        }
    }
}

/// Task de fundo: UM `PgListener` em `job_events`, reconecta com backoff
/// exponencial (1s..30s) se a conexão cair.
pub async fn run_job_events_listener(
    database_url: String,
    hub: Arc<JobEventsHub>,
    manager: Arc<dyn ManagerPort>,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        match PgListener::connect(&database_url).await {
            Ok(mut listener) => {
                if let Err(e) = listener.listen(CHANNEL).await {
                    tracing::warn!("LISTEN {CHANNEL} falhou: {e}");
                    hub.listening.store(false, Ordering::SeqCst);
                } else {
                    tracing::info!("PgListener conectado — LISTEN {CHANNEL} ativo");
                    hub.refetch_subscribed_jobs(manager.as_ref()).await;
                    hub.listening.store(true, Ordering::SeqCst);
                    backoff = Duration::from_secs(1);

                    loop {
                        match listener.try_recv().await {
                            Ok(Some(notice)) => {
                                if let Ok(payload) =
                                    serde_json::from_str::<RawPayload>(notice.payload())
                                {
                                    schedule(
                                        Arc::clone(&hub),
                                        Arc::clone(&manager),
                                        payload.job_id,
                                        payload.seq.is_some(),
                                        payload.status,
                                    )
                                    .await;
                                }
                            }
                            Ok(None) => {
                                // Conexão caiu e o sqlx reconectou (ou está reconectando).
                                tracing::warn!(
                                    "PgListener detectou desconexão (try_recv retornou Ok(None))"
                                );
                                hub.listening.store(false, Ordering::SeqCst);
                                // Ao voltar a receber/reconectar, refaz status + delta de todos os jobs com assinantes
                                hub.refetch_subscribed_jobs(manager.as_ref()).await;
                                hub.listening.store(true, Ordering::SeqCst);
                            }
                            Err(e) => {
                                tracing::warn!("PgListener desconectado com erro: {e}");
                                hub.listening.store(false, Ordering::SeqCst);
                                break;
                            }
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("PgListener::connect falhou: {e}");
                hub.listening.store(false, Ordering::SeqCst);
            }
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// Task de fundo: poll de 5s compartilhado (um fetch por job com
/// assinantes, NÃO por cliente) — só age enquanto o `PgListener` está
/// desconectado (`hub.listening == false`), cobrindo a janela de queda.
pub async fn run_fallback_poller(hub: Arc<JobEventsHub>, manager: Arc<dyn ManagerPort>) {
    let mut interval = tokio::time::interval(FALLBACK_POLL_INTERVAL);
    loop {
        interval.tick().await;
        if hub.listening.load(Ordering::SeqCst) {
            continue;
        }
        for job_id in hub.subscribed_job_ids().await {
            process_job(&hub, manager.as_ref(), &job_id, true, true).await;
        }
    }
}

#[cfg(test)]
#[path = "events_hub_tests.rs"]
mod tests;
