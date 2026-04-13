use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{ExporterBuildError, SpanExporter, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::{Layer, Registry, layer::SubscriberExt, util::SubscriberInitExt};

use super::devenv_layer::DevenvLayer;
use super::span_ids::SpanIdLayer;
use super::{
    Level, TraceFormat, TraceOutput, TraceOutputSpec, TracingGuard, build_cli_layer, create_filter,
    create_local_boxed_layer,
};

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
pub(super) fn init_tracing_unified(
    level: Level,
    specs: &[TraceOutputSpec],
    cli_output: bool,
) -> TracingGuard {
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

    // CLI layer
    if let Some(cli_layer) = build_cli_layer(level, cli_output) {
        layers.push(cli_layer);
    }

    // Local format layers
    for spec in specs.iter().filter(|s| !s.format.is_otlp()) {
        if let Some(layer) = create_local_boxed_layer(spec) {
            layers.push(layer);
        }
    }

    // OTLP layers — each gets its own provider but shares the runtime
    let resource = Resource::builder().with_service_name("devenv").build();
    for spec in specs.iter().filter(|s| s.format.is_otlp()) {
        let endpoint = match &spec.destination {
            TraceOutput::Url(url) => Some(url.as_str()),
            _ => None,
        };

        let exporter = match create_exporter(spec.format, endpoint) {
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

    // Runtime must be dropped last — push it after all OtelGuards
    guards.push(Box::new(runtime));

    TracingGuard { _inner: guards }
}

fn create_exporter(
    trace_format: TraceFormat,
    endpoint: Option<&str>,
) -> Result<SpanExporter, ExporterBuildError> {
    match trace_format {
        #[cfg(feature = "otlp-grpc")]
        TraceFormat::OtlpGrpc => {
            let mut builder = SpanExporter::builder().with_tonic();
            if let Some(url) = endpoint {
                builder = builder.with_endpoint(url);
            }
            builder.build()
        }
        #[cfg(not(feature = "otlp-grpc"))]
        TraceFormat::OtlpGrpc => {
            let _ = endpoint;
            eprintln!("error: otlp-grpc format requires the 'otlp-grpc' cargo feature");
            std::process::exit(1);
        }
        #[cfg(feature = "otlp-http-protobuf")]
        TraceFormat::OtlpHttpProtobuf => {
            let mut builder = SpanExporter::builder().with_http();
            if let Some(url) = endpoint {
                builder = builder.with_endpoint(url);
            }
            builder.build()
        }
        #[cfg(not(feature = "otlp-http-protobuf"))]
        TraceFormat::OtlpHttpProtobuf => {
            let _ = endpoint;
            eprintln!(
                "error: otlp-http-protobuf format requires the 'otlp-http-protobuf' cargo feature"
            );
            std::process::exit(1);
        }
        #[cfg(feature = "otlp-http-json")]
        TraceFormat::OtlpHttpJson => {
            let mut builder = SpanExporter::builder().with_http();
            if let Some(url) = endpoint {
                builder = builder.with_endpoint(url);
            }
            builder.build()
        }
        #[cfg(not(feature = "otlp-http-json"))]
        TraceFormat::OtlpHttpJson => {
            let _ = endpoint;
            eprintln!("error: otlp-http-json format requires the 'otlp-http-json' cargo feature");
            std::process::exit(1);
        }
        _ => unreachable!("non-OTLP format passed to create_exporter"),
    }
}
