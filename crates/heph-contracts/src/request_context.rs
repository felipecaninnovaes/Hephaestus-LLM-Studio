//! Contexto de correlação (`x-request-id` + `traceparent` W3C) compartilhado
//! pelos 3 serviços (BFF, manager, orchestrator) — fatia 2b da spec de
//! telemetria. Sem OpenTelemetry (reservado para a fatia 2a): apenas um
//! `task_local` + helpers de header, para que o mesmo `request_id` atravesse
//! toda a cadeia de chamadas e cada serviço possa anexar `job_id`/`trace_id`
//! aos seus spans `tracing`.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use uuid::Uuid;

tokio::task_local! {
    static CURRENT: Arc<RequestContext>;
}

/// Correlação de uma operação (requisição HTTP de entrada ou operação de
/// background sem request associado, ex.: dispatch loop / heartbeat).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestContext {
    /// Valor do header `x-request-id` (recebido ou gerado).
    pub request_id: String,
    /// `trace-id` de 32 hex do `traceparent` W3C.
    pub trace_id: String,
    /// `span-id` de 16 hex do `traceparent` W3C (único por hop).
    pub span_id: String,
}

impl RequestContext {
    /// Gera um contexto novo (nenhuma informação de entrada disponível —
    /// ex.: operação de background iniciada pelo próprio serviço).
    pub fn generate() -> Self {
        let request_id = Uuid::new_v4().to_string();
        let trace_id = derive_trace_id(&request_id);
        let span_id = new_span_id();
        Self {
            request_id,
            trace_id,
            span_id,
        }
    }

    /// Constrói o contexto de uma requisição de entrada a partir dos headers
    /// `x-request-id` e `traceparent` (ambos opcionais).
    pub fn from_incoming(request_id: Option<&str>, traceparent: Option<&str>) -> Self {
        let request_id = request_id
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        let trace_id = traceparent
            .and_then(parse_traceparent_trace_id)
            .unwrap_or_else(|| derive_trace_id(&request_id));

        Self {
            request_id,
            trace_id,
            span_id: new_span_id(),
        }
    }

    /// Header `traceparent` W3C (`00-<trace_id>-<span_id>-01`) deste contexto.
    pub fn traceparent(&self) -> String {
        format!("00-{}-{}-01", self.trace_id, self.span_id)
    }

    /// Roda `f` com este contexto disponível via [`current`]/[`current_or_generate`].
    pub async fn scope<F>(self, f: F) -> F::Output
    where
        F: Future,
    {
        CURRENT.scope(Arc::new(self), f).await
    }
}

/// Contexto ativo na task atual, se houver (propagado via [`RequestContext::scope`]).
pub fn current() -> Option<Arc<RequestContext>> {
    CURRENT.try_with(Arc::clone).ok()
}

/// Contexto ativo, ou um novo gerado na hora (operação de background avulsa:
/// cada chamada ganha seu próprio `request_id`/`trace_id`).
pub fn current_or_generate() -> Arc<RequestContext> {
    current().unwrap_or_else(|| Arc::new(RequestContext::generate()))
}

/// Pares de header (`x-request-id`, `traceparent`) para anexar numa chamada
/// de saída, usando o contexto corrente (ou um gerado, se não houver request
/// de entrada — ex.: dispatch loop, heartbeat).
pub fn outbound_headers() -> [(&'static str, String); 2] {
    let ctx = current_or_generate();
    [
        ("x-request-id", ctx.request_id.clone()),
        ("traceparent", ctx.traceparent()),
    ]
}

/// Registro em memória do processo (manager) correlacionando `job_id` →
/// contexto que o criou, para que uma operação de background que opere
/// sobre esse job (ex.: `dispatch_next`) continue o MESMO `request_id`/
/// `trace_id` em vez de gerar um novo — sem precisar de coluna no banco.
/// Teto pequeno sem TTL (homelab, single-process): jobs nunca ficam na fila
/// por muito tempo em uso normal; se o teto for atingido, descarta a entrada
/// mais antiga (volta ao fallback de gerar um novo contexto).
const JOB_CONTEXT_MAX_ENTRIES: usize = 512;

static JOB_CONTEXTS: std::sync::LazyLock<Mutex<HashMap<String, RequestContext>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

fn job_contexts() -> &'static Mutex<HashMap<String, RequestContext>> {
    &JOB_CONTEXTS
}

/// Associa o contexto de correlação corrente ao `job_id` recém-criado.
pub fn remember_for_job(job_id: &str) {
    let ctx = current_or_generate();
    let mut map = job_contexts().lock().expect("job_contexts lock poisoned");
    if map.len() >= JOB_CONTEXT_MAX_ENTRIES {
        if let Some(oldest) = map.keys().next().cloned() {
            map.remove(&oldest);
        }
    }
    map.insert(job_id.to_string(), (*ctx).clone());
}

/// Recupera (e remove) o contexto lembrado para `job_id`, se houver.
pub fn recall_for_job(job_id: &str) -> Option<RequestContext> {
    job_contexts()
        .lock()
        .expect("job_contexts lock poisoned")
        .remove(job_id)
}

fn new_span_id() -> String {
    let bytes = Uuid::new_v4().into_bytes();
    bytes[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// Deriva um `trace_id` de 32 hex, estável para o mesmo `request_id`, para
/// que os 3 serviços cheguem ao mesmo `trace_id` sem precisar de um
/// `traceparent` de entrada (ex.: primeira chamada do BFF).
fn derive_trace_id(request_id: &str) -> String {
    // Se o request_id já é um UUID (caso comum: gerado por um dos serviços),
    // seu valor sem hífens já é 32 hex — usa direto.
    if let Ok(uuid) = Uuid::parse_str(request_id) {
        return uuid.simple().to_string();
    }
    // Caso contrário (request_id arbitrário vindo de um cliente externo),
    // deriva 32 hex determinísticos via hash — mesmo request_id sempre
    // produz o mesmo trace_id nos 3 serviços.
    let mut h1 = DefaultHasher::new();
    request_id.hash(&mut h1);
    let a = h1.finish();

    let mut h2 = DefaultHasher::new();
    (request_id, "heph-trace-salt").hash(&mut h2);
    let b = h2.finish();

    format!("{a:016x}{b:016x}")
}

fn parse_traceparent_trace_id(traceparent: &str) -> Option<String> {
    let mut parts = traceparent.split('-');
    let version = parts.next()?;
    let trace_id = parts.next()?;
    let span_id = parts.next()?;
    let _flags = parts.next()?;
    if version.len() != 2 || trace_id.len() != 32 || span_id.len() != 16 {
        return None;
    }
    if !trace_id.chars().all(|c| c.is_ascii_hexdigit()) || trace_id == "0".repeat(32) {
        return None;
    }
    Some(trace_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traceparent_format() {
        let ctx = RequestContext {
            request_id: "req-1".into(),
            trace_id: "a".repeat(32),
            span_id: "b".repeat(16),
        };
        assert_eq!(
            ctx.traceparent(),
            format!("00-{}-{}-01", "a".repeat(32), "b".repeat(16))
        );
    }

    #[test]
    fn same_request_id_same_trace_id() {
        let a = RequestContext::from_incoming(Some("abc-123"), None);
        let b = RequestContext::from_incoming(Some("abc-123"), None);
        assert_eq!(a.trace_id, b.trace_id);
        assert_ne!(a.span_id, b.span_id);
    }

    #[test]
    fn uuid_request_id_strips_to_trace_id() {
        let id = Uuid::new_v4();
        let ctx = RequestContext::from_incoming(Some(&id.to_string()), None);
        assert_eq!(ctx.trace_id, id.simple().to_string());
    }

    #[test]
    fn reuses_incoming_traceparent_trace_id() {
        let incoming_trace = "1".repeat(32);
        let tp = format!("00-{incoming_trace}-{}-01", "2".repeat(16));
        let ctx = RequestContext::from_incoming(Some("req-1"), Some(&tp));
        assert_eq!(ctx.trace_id, incoming_trace);
    }

    #[tokio::test]
    async fn scope_makes_context_current() {
        let ctx = RequestContext::generate();
        let request_id = ctx.request_id.clone();
        ctx.scope(async {
            assert_eq!(current().unwrap().request_id, request_id);
        })
        .await;
        assert!(current().is_none());
    }
}
