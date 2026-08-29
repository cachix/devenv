use std::cell::RefCell;

use opentelemetry::global;
use opentelemetry::propagation::{Injector, TextMapPropagator};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{ExporterBuildError, MetricExporter, SpanExporter, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::{Layer, Registry, layer::SubscriberExt, util::SubscriberInitExt};

use super::devenv_layer::DevenvLayer;
use super::span_ids::SpanIdLayer;
use super::{
    Level, OtlpProtocol, TraceOutputSpec, TracingGuard, create_filter, create_local_boxed_layers,
};
use url::Url;

thread_local! {
    /// Prevent the dedicated exporter runtime from feeding its own transport
    /// instrumentation back into the global OpenTelemetry layer.
    static EXPORTER_DISPATCH_GUARD: RefCell<Option<tracing::dispatcher::DefaultGuard>> =
        const { RefCell::new(None) };
}

fn isolate_exporter_thread() {
    EXPORTER_DISPATCH_GUARD.with(|slot| {
        *slot.borrow_mut() = Some(tracing::dispatcher::set_default(&tracing::Dispatch::new(
            tracing::subscriber::NoSubscriber::default(),
        )));
    });
}

fn clear_exporter_thread_isolation() {
    EXPORTER_DISPATCH_GUARD.with(|slot| {
        slot.borrow_mut().take();
    });
}

/// Guard that shuts down an OTEL tracer provider on drop.
///
/// Uses a runtime `Handle` to enter the runtime context for async flush.
/// The runtime itself is stored separately in `TracingGuard` and must
/// outlive all `OtelGuard` instances.
struct OtelGuard {
    provider: SdkTracerProvider,
    runtime_handle: tokio::runtime::Handle,
}

/// Guard that flushes and shuts down the OTEL meter provider on drop.
struct OtelMetricsGuard {
    provider: SdkMeterProvider,
    runtime_handle: tokio::runtime::Handle,
}

impl Drop for OtelMetricsGuard {
    fn drop(&mut self) {
        let _guard = self.runtime_handle.enter();
        if let Err(e) = self.provider.shutdown() {
            eprintln!("warning: failed to shut down OpenTelemetry meter provider: {e}");
        }
    }
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
    has_trace_export: bool,
) -> TracingGuard {
    let has_activity_replay = specs
        .iter()
        .any(|spec| matches!(spec, TraceOutputSpec::Render(super::TraceFormat::Json, _)));
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
        .on_thread_start(isolate_exporter_thread)
        .on_thread_stop(clear_exporter_thread_isolation)
        .build()
        .expect("Failed to create OpenTelemetry runtime");

    let _rt_guard = runtime.enter();
    let runtime_handle = runtime.handle().clone();

    // Providers must be dropped (flushed) before the runtime.
    // Vec drops front-to-back, so we push OtelGuards first, runtime last.
    let mut guards: Vec<Box<dyn Send>> = Vec::new();

    let mut layers: Vec<Box<dyn Layer<_> + Send + Sync>> = Vec::new();

    // Render layers
    for spec in specs
        .iter()
        .filter(|s| matches!(s, TraceOutputSpec::Render(_, _)))
    {
        layers.extend(create_local_boxed_layers(spec));
    }

    // OTLP trace layers each get a provider. Metrics share one provider with
    // one exporter per OTLP destination.
    let resource = Resource::builder().with_service_name("devenv").build();
    let mut meter_provider = SdkMeterProvider::builder().with_resource(resource.clone());
    let mut has_metric_exporter = false;
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
        let metric_exporter = match create_metric_exporter(proto, url) {
            Ok(exporter) => exporter,
            Err(e) => {
                eprintln!("error: failed to create OTLP metrics exporter: {e}");
                std::process::exit(1);
            }
        };
        meter_provider = meter_provider.with_periodic_exporter(metric_exporter);
        has_metric_exporter = true;

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

    if has_metric_exporter {
        let meter_provider = meter_provider.build();
        global::set_meter_provider(meter_provider.clone());
        guards.push(Box::new(OtelMetricsGuard {
            provider: meter_provider,
            runtime_handle: runtime_handle.clone(),
        }));
    }

    let _ = Registry::default()
        .with(create_filter(level, has_trace_export, has_activity_replay))
        .with(SpanIdLayer)
        .with(layers)
        .with(DevenvLayer::new())
        .try_init();

    // Register trace context propagation without an intermediate HashMap/Vec.
    // TraceContextPropagator produces only the two W3C keys, so map them to
    // static environment names instead of allocating an uppercased copy.
    devenv_activity::register_trace_propagator({
        let propagator = TraceContextPropagator::new();
        move |inject| {
            struct EnvInjector<'a>(&'a mut dyn FnMut(&str, &str));

            impl Injector for EnvInjector<'_> {
                fn set(&mut self, key: &str, value: String) {
                    let env_key = match key {
                        "traceparent" => "TRACEPARENT",
                        "tracestate" => "TRACESTATE",
                        _ => key,
                    };
                    (self.0)(env_key, &value);
                }
            }

            let context = tracing::Span::current().context();
            propagator.inject_context(&context, &mut EnvInjector(inject));
        }
    });

    // Runtime must be dropped last — push it after all OtelGuards
    guards.push(Box::new(runtime));

    TracingGuard { _inner: guards }
}

fn create_metric_exporter(
    protocol: OtlpProtocol,
    endpoint: &Url,
) -> Result<MetricExporter, ExporterBuildError> {
    match protocol {
        #[cfg(feature = "otlp-grpc")]
        OtlpProtocol::Grpc => MetricExporter::builder()
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
        OtlpProtocol::HttpProtobuf => MetricExporter::builder()
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
        OtlpProtocol::HttpJson => MetricExporter::builder()
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use opentelemetry::trace::{Status, TracerProvider as _};
    use opentelemetry_sdk::{
        error::OTelSdkResult,
        trace::{SdkTracerProvider, SpanData, SpanExporter},
    };
    use tracing_subscriber::{Registry, layer::SubscriberExt};

    use super::{clear_exporter_thread_isolation, isolate_exporter_thread};

    #[derive(Clone, Debug, Default)]
    struct RecordingExporter(Arc<Mutex<Vec<SpanData>>>);

    impl SpanExporter for RecordingExporter {
        async fn export(&self, mut batch: Vec<SpanData>) -> OTelSdkResult {
            self.0.lock().unwrap().append(&mut batch);
            Ok(())
        }
    }

    #[test]
    fn exporter_thread_isolation_overrides_and_restores_dispatch() {
        std::thread::spawn(|| {
            tracing::subscriber::with_default(Registry::default(), || {
                assert!(tracing::enabled!(target: "h2::proto", tracing::Level::TRACE));
                isolate_exporter_thread();
                assert!(!tracing::enabled!(target: "h2::proto", tracing::Level::TRACE));
                clear_exporter_thread_isolation();
                assert!(tracing::enabled!(target: "h2::proto", tracing::Level::TRACE));
            });
        })
        .join()
        .unwrap();
    }

    #[test]
    fn failed_activity_exports_otel_status_description() {
        let exporter = RecordingExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let tracer = provider.tracer("devenv-test");
        let subscriber =
            Registry::default().with(tracing_opentelemetry::layer().with_tracer(tracer));

        tracing::subscriber::with_default(subscriber, || {
            let activity = devenv_activity::start!(devenv_activity::Activity::task("failing"));
            activity.fail_with_description("native task builtin failed: invalid input");
        });

        let spans = exporter.0.lock().unwrap();
        let span = spans
            .iter()
            .find(|span| span.name == "failing")
            .expect("failed activity span must be exported");
        assert_eq!(
            span.status,
            Status::error("native task builtin failed: invalid input")
        );
    }
}
