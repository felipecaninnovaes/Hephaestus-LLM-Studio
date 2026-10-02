//! Contexto de correlação (`x-request-id` + `traceparent` W3C) compartilhado
//! pelos 3 serviços (BFF, manager, orchestrator) — fatia 2b da spec de
//! telemetria. Sem OpenTelemetry (reservado para a fatia 2a): apenas um
//! `task_local` + helpers de header, para que o mesmo `request_id` atravesse
//! toda a cadeia de chamadas e cada serviço possa anexar `job_id`/`trace_id`
//! aos seus spans `tracing`.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

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
    /// `x-request-id` e `traceparent` (ambos opcionais). Headers mal-
    /// formados são descartados silenciosamente (debug log) e substituídos
    /// por valores gerados — um header ruim nunca deve quebrar a requisição.
    pub fn from_incoming(request_id: Option<&str>, traceparent: Option<&str>) -> Self {
        let request_id = request_id
            .and_then(|raw| match sanitize_request_id(raw) {
                Some(v) => Some(v),
                None => {
                    tracing::debug!(raw = %raw, "x-request-id inválido descartado, gerando novo");
                    None
                }
            })
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        let trace_id = traceparent
            .and_then(|raw| match parse_traceparent_trace_id(raw) {
                Some(v) => Some(v),
                None => {
                    tracing::debug!(raw = %raw, "traceparent inválido descartado, derivando novo trace_id");
                    None
                }
            })
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
/// de entrada — ex.: dispatch loop, heartbeat). Com `feature = "otel"` e
/// provider ativo, o `span_id` do `traceparent` é o do span OTel corrente
/// (não o sintético da 2b) — o próximo hop vira filho correto na árvore de
/// spans exportada (fatia 2a).
pub fn outbound_headers() -> [(&'static str, String); 2] {
    let ctx = current_or_generate();
    #[cfg(feature = "otel")]
    let traceparent = crate::otel::current_span_id_hex()
        .map(|span_id| format!("00-{}-{}-01", ctx.trace_id, span_id))
        .unwrap_or_else(|| ctx.traceparent());
    #[cfg(not(feature = "otel"))]
    let traceparent = ctx.traceparent();
    [
        ("x-request-id", ctx.request_id.clone()),
        ("traceparent", traceparent),
    ]
}

// ---------------------------------------------------------------------------
// Registro job_id → contexto de origem (manager): permite que o loop de
// background `dispatch_next` continue o MESMO request_id/trace_id da
// requisição de submit, sem precisar de coluna no banco. FIFO com teto +
// TTL — nunca cresce sem limite mesmo se um caller esquecer de limpar.
// ---------------------------------------------------------------------------

const JOB_CONTEXT_MAX_ENTRIES: usize = 512;
const JOB_CONTEXT_TTL: Duration = Duration::from_secs(3600);

struct JobContextEntry {
    ctx: RequestContext,
    inserted_at: Instant,
}

struct JobContextRegistry {
    max_entries: usize,
    ttl: Duration,
    order: VecDeque<String>,
    map: HashMap<String, JobContextEntry>,
}

impl JobContextRegistry {
    fn new(max_entries: usize, ttl: Duration) -> Self {
        Self {
            max_entries,
            ttl,
            order: VecDeque::new(),
            map: HashMap::new(),
        }
    }

    /// Remove do FRONT (mais antigas primeiro — `order` é FIFO de inserção e
    /// o TTL é constante, então a entrada mais antiga sempre expira primeiro)
    /// todas as entradas já expiradas.
    fn evict_expired(&mut self) {
        while let Some(front) = self.order.front() {
            match self.map.get(front) {
                Some(entry) if entry.inserted_at.elapsed() > self.ttl => {
                    let key = self.order.pop_front().expect("front checked above");
                    self.map.remove(&key);
                }
                _ => break,
            }
        }
    }

    fn insert(&mut self, job_id: String, ctx: RequestContext) {
        self.evict_expired();
        if self.map.remove(&job_id).is_some() {
            self.order.retain(|k| k != &job_id);
        }
        while self.map.len() >= self.max_entries {
            match self.order.pop_front() {
                Some(oldest) => {
                    self.map.remove(&oldest);
                }
                None => break,
            }
        }
        self.order.push_back(job_id.clone());
        self.map.insert(
            job_id,
            JobContextEntry {
                ctx,
                inserted_at: Instant::now(),
            },
        );
    }

    fn remove(&mut self, job_id: &str) -> Option<RequestContext> {
        self.evict_expired();
        let entry = self.map.remove(job_id)?;
        self.order.retain(|k| k != job_id);
        Some(entry.ctx)
    }

    fn forget(&mut self, job_id: &str) {
        self.map.remove(job_id);
        self.order.retain(|k| k != job_id);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.map.len()
    }
}

static JOB_CONTEXTS: LazyLock<Mutex<JobContextRegistry>> = LazyLock::new(|| {
    Mutex::new(JobContextRegistry::new(
        JOB_CONTEXT_MAX_ENTRIES,
        JOB_CONTEXT_TTL,
    ))
});

/// Associa o contexto de correlação corrente ao `job_id` recém-criado.
pub fn remember_for_job(job_id: &str) {
    let ctx = current_or_generate();
    JOB_CONTEXTS
        .lock()
        .expect("job_contexts lock poisoned")
        .insert(job_id.to_string(), (*ctx).clone());
}

/// Recupera (e remove) o contexto lembrado para `job_id`, se houver e não
/// tiver expirado (TTL de 1h — jobs não ficam presos na fila por tanto
/// tempo em uso normal; se acontecer, cai no fallback de gerar um novo).
pub fn recall_for_job(job_id: &str) -> Option<RequestContext> {
    JOB_CONTEXTS
        .lock()
        .expect("job_contexts lock poisoned")
        .remove(job_id)
}

/// Remove explicitamente o registro de `job_id` (ex.: abort/prepare-fail/
/// transição terminal) — o job nunca mais vai ser despachado, então o
/// contexto lembrado não serve mais pra nada; evita ocupar espaço no
/// registro até o TTL expirar.
pub fn forget_for_job(job_id: &str) {
    JOB_CONTEXTS
        .lock()
        .expect("job_contexts lock poisoned")
        .forget(job_id);
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

/// Valida um `x-request-id` recebido: ≤128 bytes, charset
/// `[A-Za-z0-9._:-]` (sem controle/newline/espaço — header HTTP seguro e
/// previsível em logs). Inválido ⇒ `None` (caller gera um novo).
fn sanitize_request_id(raw: &str) -> Option<String> {
    if raw.is_empty() || raw.len() > 128 {
        return None;
    }
    let valid = raw
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'));
    if valid {
        Some(raw.to_string())
    } else {
        None
    }
}

/// Valida um `traceparent` W3C versão `00`: `00-<32 hex>-<16 hex>-<2 hex>`,
/// trace-id e parent-id (span-id) não podem ser todos-zero (reservados pelo
/// spec como inválidos); `flags` aceita QUALQUER valor hex de 2 dígitos (não
/// só `01` — o bit sampled é só um dos flags definidos, outros bits/valores
/// são válidos e devem ser preservados por implementações futuras, mas como
/// não propagamos o span recebido isso não importa aqui). Retorna só o
/// `trace_id` (o `span_id` recebido não é nosso — geramos um novo por hop).
fn parse_traceparent_trace_id(traceparent: &str) -> Option<String> {
    let mut parts = traceparent.split('-');
    let version = parts.next()?;
    let trace_id = parts.next()?;
    let span_id = parts.next()?;
    let flags = parts.next()?;
    if parts.next().is_some() {
        // Versão 00 tem exatamente 4 campos; campos extras só são válidos em
        // versões futuras (que não reconhecemos) — rejeita por segurança.
        return None;
    }
    if version != "00" {
        return None;
    }
    if !is_non_zero_hex(trace_id, 32) {
        return None;
    }
    if !is_non_zero_hex(span_id, 16) {
        return None;
    }
    if flags.len() != 2 || !flags.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(trace_id.to_ascii_lowercase())
}

/// Mesmo parsing/validação de [`parse_traceparent_trace_id`], mas retorna o
/// `span-id` (parent-id) do `traceparent` — usado só pela integração OTel
/// (`feature = "otel"`, `crate::otel::set_request_parent`) para linkar o
/// span raiz deste hop ao span real do upstream, quando presente.
pub(crate) fn parse_traceparent_span_id(traceparent: &str) -> Option<String> {
    let mut parts = traceparent.split('-');
    let version = parts.next()?;
    let trace_id = parts.next()?;
    let span_id = parts.next()?;
    let flags = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    if version != "00" {
        return None;
    }
    if !is_non_zero_hex(trace_id, 32) {
        return None;
    }
    if !is_non_zero_hex(span_id, 16) {
        return None;
    }
    if flags.len() != 2 || !flags.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(span_id.to_ascii_lowercase())
}

fn is_non_zero_hex(s: &str, expected_len: usize) -> bool {
    s.len() == expected_len
        && s.bytes().all(|b| b.is_ascii_hexdigit())
        && !s.bytes().all(|b| b == b'0')
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

    // -- request_id sanitization -------------------------------------------

    #[test]
    fn request_id_with_newline_is_regenerated() {
        let ctx = RequestContext::from_incoming(Some("evil\nrequest-id"), None);
        assert_ne!(ctx.request_id, "evil\nrequest-id");
        // Gerado: UUID v4 válido.
        assert!(Uuid::parse_str(&ctx.request_id).is_ok());
    }

    #[test]
    fn request_id_over_128_bytes_is_regenerated() {
        let too_long = "a".repeat(129);
        let ctx = RequestContext::from_incoming(Some(&too_long), None);
        assert_ne!(ctx.request_id, too_long);
        assert!(Uuid::parse_str(&ctx.request_id).is_ok());
    }

    #[test]
    fn request_id_at_128_bytes_is_preserved() {
        let exactly = "a".repeat(128);
        let ctx = RequestContext::from_incoming(Some(&exactly), None);
        assert_eq!(ctx.request_id, exactly);
    }

    #[test]
    fn request_id_valid_charset_is_preserved() {
        let valid = "smoke-2b_test.id:123";
        let ctx = RequestContext::from_incoming(Some(valid), None);
        assert_eq!(ctx.request_id, valid);
    }

    #[test]
    fn request_id_with_space_is_regenerated() {
        let ctx = RequestContext::from_incoming(Some("has space"), None);
        assert!(Uuid::parse_str(&ctx.request_id).is_ok());
    }

    // -- traceparent validation ----------------------------------------------

    #[test]
    fn traceparent_wrong_version_regenerates() {
        let tp = format!("01-{}-{}-01", "a".repeat(32), "b".repeat(16));
        let ctx = RequestContext::from_incoming(Some("req-x"), Some(&tp));
        assert_eq!(ctx.trace_id, derive_trace_id("req-x"));
    }

    #[test]
    fn traceparent_zero_trace_id_regenerates() {
        let tp = format!("00-{}-{}-01", "0".repeat(32), "b".repeat(16));
        let ctx = RequestContext::from_incoming(Some("req-x"), Some(&tp));
        assert_eq!(ctx.trace_id, derive_trace_id("req-x"));
    }

    #[test]
    fn traceparent_zero_span_id_regenerates() {
        let tp = format!("00-{}-{}-01", "a".repeat(32), "0".repeat(16));
        let ctx = RequestContext::from_incoming(Some("req-x"), Some(&tp));
        assert_eq!(ctx.trace_id, derive_trace_id("req-x"));
    }

    #[test]
    fn traceparent_non_hex_regenerates() {
        let tp = format!("00-{}-{}-01", "z".repeat(32), "b".repeat(16));
        let ctx = RequestContext::from_incoming(Some("req-x"), Some(&tp));
        assert_eq!(ctx.trace_id, derive_trace_id("req-x"));
    }

    #[test]
    fn traceparent_wrong_lengths_regenerate() {
        let tp = format!("00-{}-{}-01", "a".repeat(31), "b".repeat(16));
        let ctx = RequestContext::from_incoming(Some("req-x"), Some(&tp));
        assert_eq!(ctx.trace_id, derive_trace_id("req-x"));
    }

    #[test]
    fn traceparent_any_flags_value_is_valid() {
        // W3C: QUALQUER valor hex de 2 dígitos é válido em `flags`, não só `01`.
        let tp = format!("00-{}-{}-ff", "a".repeat(32), "b".repeat(16));
        let ctx = RequestContext::from_incoming(Some("req-x"), Some(&tp));
        assert_eq!(ctx.trace_id, "a".repeat(32));
    }

    #[test]
    fn traceparent_bad_flags_regenerates() {
        let tp = format!("00-{}-{}-0", "a".repeat(32), "b".repeat(16)); // flags com 1 char só
        let ctx = RequestContext::from_incoming(Some("req-x"), Some(&tp));
        assert_eq!(ctx.trace_id, derive_trace_id("req-x"));
    }

    #[test]
    fn traceparent_extra_fields_regenerates() {
        let tp = format!("00-{}-{}-01-extra", "a".repeat(32), "b".repeat(16));
        let ctx = RequestContext::from_incoming(Some("req-x"), Some(&tp));
        assert_eq!(ctx.trace_id, derive_trace_id("req-x"));
    }

    // -- job context registry: FIFO + TTL + forget --------------------------

    #[test]
    fn registry_fifo_evicts_oldest_at_cap() {
        let mut reg = JobContextRegistry::new(2, Duration::from_secs(3600));
        reg.insert("job-1".into(), RequestContext::generate());
        reg.insert("job-2".into(), RequestContext::generate());
        reg.insert("job-3".into(), RequestContext::generate());
        assert_eq!(reg.len(), 2);
        assert!(reg.remove("job-1").is_none(), "oldest evicted at cap");
        assert!(reg.remove("job-2").is_some());
        assert!(reg.remove("job-3").is_some());
    }

    #[test]
    fn registry_ttl_expires_entries() {
        let mut reg = JobContextRegistry::new(512, Duration::from_millis(5));
        reg.insert("job-1".into(), RequestContext::generate());
        std::thread::sleep(Duration::from_millis(20));
        assert!(reg.remove("job-1").is_none(), "entry must have expired");
    }

    #[test]
    fn registry_forget_removes_entry() {
        let mut reg = JobContextRegistry::new(512, Duration::from_secs(3600));
        reg.insert("job-1".into(), RequestContext::generate());
        reg.forget("job-1");
        assert!(reg.remove("job-1").is_none());
    }

    #[test]
    fn registry_recall_removes_entry_once() {
        let mut reg = JobContextRegistry::new(512, Duration::from_secs(3600));
        reg.insert("job-1".into(), RequestContext::generate());
        assert!(reg.remove("job-1").is_some());
        assert!(reg.remove("job-1").is_none(), "recall consumes the entry");
    }

    #[tokio::test]
    async fn public_remember_recall_forget_roundtrip() {
        let ctx = RequestContext::generate();
        let request_id = ctx.request_id.clone();
        let job_id = format!("test-job-{}", Uuid::new_v4());
        let job_id_clone = job_id.clone();
        // Spawn-free: `scope` roda a asserção dentro do contexto corrente.
        ctx.scope(async move {
            remember_for_job(&job_id_clone);
        })
        .await;
        let recalled = recall_for_job(&job_id).expect("remembered");
        assert_eq!(recalled.request_id, request_id);
        assert!(recall_for_job(&job_id).is_none(), "consumed on recall");

        // forget em entrada já ausente é no-op seguro.
        forget_for_job(&job_id);
    }
}
