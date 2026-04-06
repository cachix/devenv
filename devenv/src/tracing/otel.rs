use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{ExporterBuildError, SpanExporter, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::{Registry, prelude::*};

use super::devenv_layer::{DevenvFormat, DevenvLayer};
use super::span_ids::SpanIdLayer;
use super::{Level, TraceFormat, TraceOutput, TracingGuard, create_filter};
use std::io::{self, IsTerminal};

/// Guard that shuts down the OTEL tracer provider and runtime on drop.
///
/// The OTLP batch exporter needs a tokio runtime for its background flush task.
/// Since `init_tracing` is called before any application runtime exists, we
/// create a dedicated single-thread runtime here. Dropping this guard shuts
/// down the provider (flushing pending spans) and then the runtime.
struct OtelGuard {
    provider: SdkTracerProvider,
    _runtime: tokio::runtime::Runtime,
}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        // Enter the runtime context so the provider can drive async flush tasks
        let _guard = self._runtime.enter();
        if let Err(e) = self.provider.shutdown() {
            eprintln!("warning: failed to shut down OpenTelemetry tracer provider: {e}");
        }
    }
}

pub(super) fn init_tracing_otlp(
    level: Level,
    trace_format: TraceFormat,
    trace_output: Option<&TraceOutput>,
    cli_output: bool,
) -> TracingGuard {
    // The OTLP exporter and batch processor need a tokio runtime.
    // This is called before the application's main runtime exists, so we
    // create a lightweight dedicated runtime that lives in the guard.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .thread_name("otel")
        .build()
        .expect("Failed to create OpenTelemetry runtime");

    let _guard = runtime.enter();

    // Resolve the endpoint: explicit URL > OTEL env var (handled by crate) > format default
    let endpoint = match trace_output {
        Some(TraceOutput::Url(url)) => Some(url.as_str()),
        _ => None,
    };

    let exporter = match create_exporter(trace_format, endpoint) {
        Ok(exporter) => exporter,
        Err(e) => {
            eprintln!("error: failed to create OTLP exporter: {e}");
            std::process::exit(1);
        }
    };

    let resource = Resource::builder().with_service_name("devenv").build();

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();

    let tracer = provider.tracer("devenv");

    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    let base = Registry::default()
        .with(create_filter(level))
        .with(SpanIdLayer);

    // CLI output layer (same as non-OTLP path)
    let cli_layer = if cli_output {
        let ansi = io::stderr().is_terminal();
        let verbose = level >= Level::Debug;
        Some(
            tracing_subscriber::fmt::layer()
                .event_format(DevenvFormat { verbose })
                .with_writer(io::stderr)
                .with_ansi(ansi),
        )
    } else {
        None
    };

    let _ = base
        .with(cli_layer)
        .with(otel_layer)
        .with(DevenvLayer::new())
        .try_init();

    TracingGuard {
        _inner: vec![Box::new(OtelGuard {
            provider,
            _runtime: runtime,
        })],
    }
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
