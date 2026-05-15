use std::collections::HashMap;

use opentelemetry::propagation::TextMapPropagator;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{ExporterBuildError, SpanExporter, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::{Layer, Registry, layer::SubscriberExt, util::SubscriberInitExt};

use super::devenv_layer::DevenvLayer;
use super::span_ids::SpanIdLayer;
use super::{
    Level, OtlpProtocol, TraceOutputSpec, TracingGuard, build_cli_layer, create_filter,
    create_local_boxed_layer,
};
use url::Url;

/// Guard that shuts down an OTEL tracer provider on drop.
///
/// Uses a runtime `Handle` to enter the runtime context for async flush.
/// The runtime itself is stored separately in `TracingGuard` and must
/// outlive all `OtelGuard` instances.
struct OtelGuard {
    provider: SdkTracerProvider,
    runtime_handle: tokio::runtime::Handle,
}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        let _guard = self.runtime_handle.enter();
        if let Err(e) = self.provider.shutdown() {
            eprintln!("warning: failed to shut down OpenTelemetry tracer provider: {e}");
        }
    }
}

/// Initialize tracing with a mix of local and OTLP output specs.
///
/// All layers (CLI, local exports, OTLP exports) are collected into a single
/// `Vec<Box<dyn Layer>>` and composed onto one `Registry`.
pub(super) fn init_tracing_unified(level: Level, specs: &[TraceOutputSpec]) -> TracingGuard {
    // The OTLP exporter and batch processor need a tokio runtime.
    // This is called before the application's main runtime exists, so we
    // create a lightweight dedicated runtime.
    // Uses multi-thread (not current-thread) because the batch exporter spawns
    // background flush tasks via tokio::spawn that need a worker thread to drive
    // them without an explicit block_on loop.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .thread_name("otel")
        .build()
        .expect("Failed to create OpenTelemetry runtime");

    let _rt_guard = runtime.enter();
    let runtime_handle = runtime.handle().clone();

    // Providers must be dropped (flushed) before the runtime.
    // Vec drops front-to-back, so we push OtelGuards first, runtime last.
    let mut guards: Vec<Box<dyn Send>> = Vec::new();

    let mut layers: Vec<Box<dyn Layer<_> + Send + Sync>> = Vec::new();

    layers.push(build_cli_layer());

    // Render layers
    for spec in specs
        .iter()
        .filter(|s| matches!(s, TraceOutputSpec::Render(_, _)))
    {
        if let Some(layer) = create_local_boxed_layer(spec) {
            layers.push(layer);
        }
    }

    // OTLP layers — each gets its own provider but shares the runtime
    let resource = Resource::builder().with_service_name("devenv").build();
    for spec in specs {
        let (proto, url) = match spec {
            TraceOutputSpec::Otlp(p, u) => (*p, u),
            TraceOutputSpec::Render(_, _) => continue,
        };

        let exporter = match create_exporter(proto, url) {
            Ok(exporter) => exporter,
            Err(e) => {
                eprintln!("error: failed to create OTLP exporter: {e}");
                std::process::exit(1);
            }
        };

        let provider = SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .with_resource(resource.clone())
            .build();
        let tracer = provider.tracer("devenv");
        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

        layers.push(Box::new(otel_layer));
        guards.push(Box::new(OtelGuard {
            provider,
            runtime_handle: runtime_handle.clone(),
        }));
    }

    let _ = Registry::default()
        .with(create_filter(level))
        .with(SpanIdLayer)
        .with(layers)
        .with(DevenvLayer::new())
        .try_init();

    // Register trace context propagator so subprocesses inherit TRACEPARENT/TRACESTATE.
    // HashMap<String, String> implements opentelemetry::propagation::Injector
    // (lowercases keys), so we uppercase them for env var convention.
    devenv_activity::register_trace_propagator({
        let propagator = TraceContextPropagator::new();
        move || {
            let context = tracing::Span::current().context();
            let mut headers: HashMap<String, String> = HashMap::new();
            propagator.inject_context(&context, &mut headers);
            headers
                .into_iter()
                .map(|(k, v)| (k.to_ascii_uppercase(), v))
                .collect()
        }
    });

    // Runtime must be dropped last — push it after all OtelGuards
    guards.push(Box::new(runtime));

    TracingGuard { _inner: guards }
}

fn create_exporter(
    protocol: OtlpProtocol,
    endpoint: &Url,
) -> Result<SpanExporter, ExporterBuildError> {
    match protocol {
        #[cfg(feature = "otlp-grpc")]
        OtlpProtocol::Grpc => SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint.as_str())
            .build(),
        #[cfg(not(feature = "otlp-grpc"))]
        OtlpProtocol::Grpc => {
            let _ = endpoint;
            eprintln!("error: otlp-grpc requires the 'otlp-grpc' cargo feature");
            std::process::exit(1);
        }
        #[cfg(feature = "otlp-http-protobuf")]
        OtlpProtocol::HttpProtobuf => SpanExporter::builder()
            .with_http()
            .with_endpoint(endpoint.as_str())
            .build(),
        #[cfg(not(feature = "otlp-http-protobuf"))]
        OtlpProtocol::HttpProtobuf => {
            let _ = endpoint;
            eprintln!("error: otlp-http-protobuf requires the 'otlp-http-protobuf' cargo feature");
            std::process::exit(1);
        }
        #[cfg(feature = "otlp-http-json")]
        OtlpProtocol::HttpJson => SpanExporter::builder()
            .with_http()
            .with_endpoint(endpoint.as_str())
            .build(),
        #[cfg(not(feature = "otlp-http-json"))]
        OtlpProtocol::HttpJson => {
            let _ = endpoint;
            eprintln!("error: otlp-http-json requires the 'otlp-http-json' cargo feature");
            std::process::exit(1);
        }
    }
}
