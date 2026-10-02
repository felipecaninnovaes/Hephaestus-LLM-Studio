//! Integração OpenTelemetry (fatia 2a da spec de telemetria, `feature = "otel"`).
//!
//! Helper de init único para os 3 serviços (BFF, manager, orchestrator):
//! mesmo subscriber JSON + `EnvFilter` de sempre, mais uma camada
//! `tracing-opentelemetry` OPCIONAL que só existe quando
//! `OTEL_EXPORTER_OTLP_ENDPOINT` está definida (e não vazia). Sem a env, o
//! subscriber instalado é idêntico ao de antes da 2a — nenhum provider OTel
//! é criado, nenhum custo de runtime.
//!
//! # Continuidade de trace com a 2b
//!
//! O span `request` de cada serviço (`request_id_middleware`) já carrega
//! `trace_id`/`request_id` resolvidos por [`crate::RequestContext`]
//! (fatia 2b). Para que o span OTel exportado herde o MESMO `trace_id` —
//! tanto quando o `traceparent` de entrada é válido quanto quando o
//! `trace_id` foi derivado do `request_id` (sem header, ou header inválido)
//! — [`set_request_parent`] força um contexto pai OTel com
//! `trace_id = ctx.trace_id` sempre, usando o `span-id` real do
//! `traceparent` recebido como parent-id quando disponível (upstream com
//! OTel habilitado) ou um valor sintético caso contrário (span "pai"
//! fantasma, nunca exportado — só para o SDK aceitar o `trace_id` forçado).
//!
//! O `traceparent` de SAÍDA (`outbound_headers`, em `request_context.rs`)
//! carrega o `span_id` REAL do span OTel corrente quando há provider ativo
//! (via [`current_span_id_hex`]) — assim o próximo hop recebe o parent-id
//! verdadeiro e vira filho correto na árvore de spans exportada.

use opentelemetry::trace::{SpanContext, SpanId, TraceContextExt, TraceFlags, TraceId, TraceState};
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig as _;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;

use crate::request_context::parse_traceparent_span_id;
use crate::RequestContext;

/// Guarda o `SdkTracerProvider` (export batched assíncrono) enquanto o
/// serviço roda. `None` quando `OTEL_EXPORTER_OTLP_ENDPOINT` está ausente ou
/// o exporter falhou ao construir — `shutdown()` vira no-op nesse caso.
pub struct OtelGuard {
    provider: Option<SdkTracerProvider>,
}

impl OtelGuard {
    /// Flush final + encerramento ordenado do provider (graceful shutdown).
    /// Nunca derruba o processo: falha vira WARN.
    pub fn shutdown(&self) {
        if let Some(provider) = &self.provider {
            if let Err(e) = provider.shutdown() {
                tracing::warn!(error = %e, "falha ao encerrar o provider OTel (shutdown)");
            }
        }
    }
}

/// Instala o subscriber `tracing` global: JSON + `EnvFilter` (`RUST_LOG` ou
/// `default_filter` como fallback — igual ao padrão dos 3 serviços antes da
/// 2a) e, se `OTEL_EXPORTER_OTLP_ENDPOINT` estiver definida, a camada
/// `tracing-opentelemetry` com exporter OTLP/gRPC batched
/// (`OTEL_SERVICE_NAME`, default `"heph-service"`, vira `service.name` no
/// resource). Falha ao construir o exporter (endpoint inválido etc.) nunca
/// derruba o boot: WARN e o serviço sobe sem export OTel.
pub fn init_tracing(default_filter: &str) -> OtelGuard {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));
    let fmt_layer = tracing_subscriber::fmt::layer().json();

    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .ok()
        .filter(|s| !s.trim().is_empty());

    let Some(endpoint) = endpoint else {
        let subscriber = tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer);
        tracing::subscriber::set_global_default(subscriber)
            .expect("set_global_default: subscriber já instalado");
        return OtelGuard { provider: None };
    };

    let service_name =
        std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "heph-service".to_string());

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .build();

    let exporter = match exporter {
        Ok(e) => e,
        Err(e) => {
            let subscriber = tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer);
            tracing::subscriber::set_global_default(subscriber)
                .expect("set_global_default: subscriber já instalado");
            tracing::warn!(
                error = %e,
                endpoint = %endpoint,
                "falha ao criar exporter OTLP — OTel desabilitado, serviço segue sem export de traces"
            );
            return OtelGuard { provider: None };
        }
    };

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            Resource::builder()
                .with_attribute(KeyValue::new("service.name", service_name.clone()))
                .build(),
        )
        .build();

    let tracer = {
        use opentelemetry::trace::TracerProvider as _;
        provider.tracer(service_name)
    };
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    let subscriber = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .with(otel_layer);
    tracing::subscriber::set_global_default(subscriber)
        .expect("set_global_default: subscriber já instalado");

    tracing::info!(endpoint = %endpoint, "OTel: exportando traces via OTLP/gRPC");

    OtelGuard {
        provider: Some(provider),
    }
}

/// Define o contexto pai OTel do span `request` a partir do `trace_id` já
/// resolvido pela 2b (`ctx.trace_id`) — SEMPRE, mesmo sem `traceparent` de
/// entrada válido — e do `span-id` real do `traceparent` recebido quando
/// presente. No-op silencioso se `ctx.trace_id` não for 32 hex válido (nunca
/// deveria acontecer — `RequestContext` garante o formato) ou se não houver
/// camada OTel instalada (sem env, `set_parent` retorna erro ignorado).
pub fn set_request_parent(
    span: &tracing::Span,
    ctx: &RequestContext,
    incoming_traceparent: Option<&str>,
) {
    let Ok(trace_id) = TraceId::from_hex(&ctx.trace_id) else {
        return;
    };
    if trace_id == TraceId::INVALID {
        return;
    }
    let parent_span_id = incoming_traceparent
        .and_then(parse_traceparent_span_id)
        .and_then(|hex| SpanId::from_hex(&hex).ok())
        .filter(|id| *id != SpanId::INVALID)
        .or_else(|| SpanId::from_hex(&ctx.span_id).ok())
        .unwrap_or(SpanId::INVALID);

    let parent_cx = opentelemetry::Context::new().with_remote_span_context(SpanContext::new(
        trace_id,
        parent_span_id,
        TraceFlags::SAMPLED,
        true,
        TraceState::default(),
    ));
    let _ = span.set_parent(parent_cx);
}

/// `span_id` (16 hex) do span OTel ativo na task atual, se houver provider
/// instalado e span válido — usado para o `traceparent` de SAÍDA carregar o
/// id real do span corrente em vez do sintético da 2b.
pub fn current_span_id_hex() -> Option<String> {
    let cx = tracing::Span::current().context();
    let span_context = cx.span().span_context().clone();
    if span_context.is_valid() {
        Some(span_context.span_id().to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
    use tracing_subscriber::layer::SubscriberExt;

    /// Monta um `SdkTracerProvider` com exporter in-memory e instala um
    /// subscriber `tracing` SCOPED (não global — `set_default`, revertido no
    /// drop do guard) com a camada `tracing-opentelemetry` ligada a ele.
    fn in_memory_subscriber() -> (
        InMemorySpanExporter,
        SdkTracerProvider,
        tracing::subscriber::DefaultGuard,
    ) {
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let tracer = {
            use opentelemetry::trace::TracerProvider as _;
            provider.tracer("heph-contracts-test")
        };
        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
        let subscriber = tracing_subscriber::registry().with(otel_layer);
        let guard = tracing::subscriber::set_default(subscriber);
        (exporter, provider, guard)
    }

    #[test]
    fn com_traceparent_valido_span_herda_trace_id_e_parent_span_id() {
        let (exporter, provider, _guard) = in_memory_subscriber();

        let incoming = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let ctx = RequestContext::from_incoming(None, Some(incoming));
        assert_eq!(ctx.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");

        {
            let span = tracing::info_span!("request", trace_id = %ctx.trace_id);
            set_request_parent(&span, &ctx, Some(incoming));
            let _enter = span.enter();
        }

        provider.force_flush().expect("flush");
        let spans = exporter.get_finished_spans().expect("spans");
        assert_eq!(spans.len(), 1);
        let span_data = &spans[0];
        assert_eq!(
            format!("{:032x}", span_data.span_context.trace_id()),
            ctx.trace_id,
            "trace_id do span OTel deve ser IGUAL ao trace_id do RequestContext (2b)"
        );
        assert_eq!(
            format!("{:016x}", span_data.parent_span_id),
            "00f067aa0ba902b7",
            "parent-id do span OTel deve ser o span-id REAL do traceparent recebido"
        );
    }

    #[test]
    fn sem_traceparent_span_ainda_usa_trace_id_derivado_do_request_id() {
        let (exporter, provider, _guard) = in_memory_subscriber();

        let ctx = RequestContext::from_incoming(Some("meu-request-id-fixo"), None);

        {
            let span = tracing::info_span!("request", trace_id = %ctx.trace_id);
            set_request_parent(&span, &ctx, None);
            let _enter = span.enter();
        }

        provider.force_flush().expect("flush");
        let spans = exporter.get_finished_spans().expect("spans");
        assert_eq!(spans.len(), 1);
        assert_eq!(
            format!("{:032x}", spans[0].span_context.trace_id()),
            ctx.trace_id,
            "sem traceparent de entrada, o span OTel ainda deve usar o trace_id \
             derivado do request_id pela 2b (mesmo valor nos logs e no OTel)"
        );
    }

    #[test]
    fn sem_provider_otel_set_parent_e_current_span_id_sao_no_ops() {
        // Nenhum subscriber OTel instalado (subscriber default do test
        // harness) — `set_request_parent`/`current_span_id_hex` não devem
        // panicar nem produzir efeito observável.
        let ctx = RequestContext::generate();
        let span = tracing::info_span!("request", trace_id = %ctx.trace_id);
        set_request_parent(&span, &ctx, None);
        let _enter = span.enter();
        assert_eq!(current_span_id_hex(), None);
    }
}
