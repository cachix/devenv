mod activity_json_layer;
mod devenv_layer;
mod human_duration;
#[cfg(feature = "otlp")]
mod otel;
mod span_ids;
mod span_timings;

use activity_json_layer::ActivityJsonLayer;
use devenv_layer::DevenvLayer;
use span_ids::{SpanContext, SpanIdLayer};

pub use crate::cli::{OtlpProtocol, TraceFormat, TraceOutputSpec, TraceSink};
pub use human_duration::HumanReadableDuration;

use json_subscriber::JsonLayer;
use std::fs::File;
use std::io::{self, IsTerminal, LineWriter, Write};
use std::sync::{Arc, Mutex, MutexGuard};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Layer, Registry, util::SubscriberInitExt};

#[derive(Default, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub enum Level {
    Silent,
    Error,
    Warn,
    #[default]
    Info,
    Debug,
}

impl From<Level> for LevelFilter {
    fn from(level: Level) -> LevelFilter {
        match level {
            Level::Silent => LevelFilter::OFF,
            Level::Error => LevelFilter::ERROR,
            Level::Warn => LevelFilter::WARN,
            Level::Info => LevelFilter::INFO,
            Level::Debug => LevelFilter::DEBUG,
        }
    }
}

/// A writer for trace output.
enum TraceWriter {
    // Stdout is already line-buffered in the standard library.
    Stdout(io::Stdout),
    Stderr(LineWriter<io::Stderr>),
    File(LineWriter<File>),
}

impl Write for TraceWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            TraceWriter::Stdout(w) => w.write(buf),
            TraceWriter::Stderr(w) => w.write(buf),
            TraceWriter::File(w) => w.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            TraceWriter::Stdout(w) => w.flush(),
            TraceWriter::Stderr(w) => w.flush(),
            TraceWriter::File(w) => w.flush(),
        }
    }
}

#[derive(Clone)]
struct SharedTraceWriter(Arc<Mutex<TraceWriter>>);

struct SharedTraceWriterGuard<'a>(MutexGuard<'a, TraceWriter>);

impl Write for SharedTraceWriterGuard<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for SharedTraceWriter {
    type Writer = SharedTraceWriterGuard<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        SharedTraceWriterGuard(self.0.lock().expect("trace writer lock poisoned"))
    }
}

fn create_trace_writer(sink: &TraceSink) -> Option<SharedTraceWriter> {
    match sink {
        TraceSink::Stdout => Some(SharedTraceWriter(Arc::new(Mutex::new(
            TraceWriter::Stdout(io::stdout()),
        )))),
        TraceSink::Stderr => Some(SharedTraceWriter(Arc::new(Mutex::new(
            TraceWriter::Stderr(LineWriter::new(io::stderr())),
        )))),
        TraceSink::File(path) => match File::create(path) {
            Ok(f) => Some(SharedTraceWriter(Arc::new(Mutex::new(TraceWriter::File(
                LineWriter::new(f),
            ))))),
            Err(e) => {
                eprintln!(
                    "warning: failed to create trace output file '{}': {e}",
                    path.display()
                );
                None
            }
        },
    }
}

fn create_json_layer<S, W: for<'a> tracing_subscriber::fmt::MakeWriter<'a> + 'static>(
    writer: W,
) -> JsonLayer<S, W>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    let mut layer = JsonLayer::new(writer);
    layer.with_timer("timestamp", tracing_subscriber::fmt::time::SystemTime);
    layer.with_level("level");
    layer.with_target("target");
    layer.serialize_extension::<SpanContext>("span_context");
    layer.with_event("fields");
    layer.with_current_span("span");
    layer
}

fn create_filter(level: Level, has_trace_export: bool, has_activity_replay: bool) -> EnvFilter {
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::from(level).into())
        .from_env_lossy()
        .add_directive("watchexec=warn".parse().unwrap());

    // Activity spans retain call-site file/line/module metadata, but use a
    // stable target so their lifecycle is independent of ordinary log level.
    // Both activity targets are only useful to an explicit trace export.
    if has_trace_export {
        let filter = filter
            .add_directive("devenv_activity::spans=trace".parse().unwrap())
            .add_directive("devenv_activity::events=debug".parse().unwrap());
        if has_activity_replay {
            filter.add_directive("devenv_activity::replay=trace".parse().unwrap())
        } else {
            filter.add_directive("devenv_activity::replay=off".parse().unwrap())
        }
    } else {
        // The activity channel, not tracing, drives the TUI and console.
        // Disable tracing mirrors so their values are never serialized.
        filter
            .add_directive("devenv_activity::spans=off".parse().unwrap())
            .add_directive("devenv_activity::events=off".parse().unwrap())
            .add_directive("devenv_activity::replay=off".parse().unwrap())
    }
}

/// Opaque guard that flushes tracing resources on drop.
///
/// Hold this in `main` until the program exits.
pub struct TracingGuard {
    _inner: Vec<Box<dyn Send>>,
}

impl TracingGuard {
    fn empty() -> Self {
        Self { _inner: vec![] }
    }
}

pub fn init_tracing_default() -> TracingGuard {
    init_tracing(Level::default(), &[])
}

/// Initialize tracing with multiple output specs.
///
/// `tracing` events (`info!`/`warn!`/`error!`/`debug!`/`trace!`) are routed
/// only to the explicit `TraceOutputSpec` sinks — they never write to stderr
/// directly. The TUI and console consume the first-party activity channel;
/// trace exporters independently observe activity spans and update events.
///
/// Each `TraceOutputSpec` adds an export layer with its own format and destination.
/// Multiple outputs can be active simultaneously (e.g. pretty to stderr + JSON to file).
///
/// Returns a [`TracingGuard`] that must be held until program exit to ensure
/// proper flushing of trace data.
pub fn init_tracing(level: Level, specs: &[TraceOutputSpec]) -> TracingGuard {
    // The activity channel independently drives the TUI and console. Without
    // an explicit trace sink there is no tracing consumer, so avoid installing
    // a registry/lifecycle layer and let every tracing callsite stay disabled.
    if specs.is_empty() {
        return TracingGuard::empty();
    }

    let has_trace_export = !specs.is_empty();
    let has_otlp = specs
        .iter()
        .any(|s| matches!(s, TraceOutputSpec::Otlp(_, _)));

    if has_otlp {
        return init_tracing_with_otlp(level, specs, has_trace_export);
    }

    init_tracing_local(level, specs, has_trace_export)
}

/// Create boxed render layers for a `Render` spec.
pub(crate) fn create_local_boxed_layers<S>(
    spec: &TraceOutputSpec,
) -> Vec<Box<dyn Layer<S> + Send + Sync>>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    let (format, sink) = match spec {
        TraceOutputSpec::Render(format, sink) => (format, sink),
        TraceOutputSpec::Otlp(_, _) => return Vec::new(),
    };
    let Some(writer) = create_trace_writer(sink) else {
        return Vec::new();
    };
    let ansi = match sink {
        TraceSink::Stdout => io::stdout().is_terminal(),
        TraceSink::Stderr => io::stderr().is_terminal(),
        TraceSink::File(_) => false,
    };
    match format {
        TraceFormat::Full => vec![Box::new(
            tracing_subscriber::fmt::layer()
                .with_ansi(ansi)
                .with_writer(writer),
        )],
        TraceFormat::Pretty => vec![Box::new(
            tracing_subscriber::fmt::layer()
                .with_ansi(ansi)
                .with_writer(writer)
                .pretty(),
        )],
        TraceFormat::Json => vec![
            Box::new(
                create_json_layer(writer.clone()).with_filter(filter_fn(|metadata| {
                    metadata.target() != "devenv_activity::events"
                })),
            ),
            // Both JSON layers live inside a `Vec<Layer>`. Give this adapter
            // its own per-layer filter so tracing-subscriber's registry can
            // see that activity events rejected by the generic JSON layer are
            // still enabled for this layer.
            Box::new(
                ActivityJsonLayer::new(writer).with_filter(filter_fn(|metadata| {
                    matches!(
                        metadata.target(),
                        "devenv_activity::spans" | "devenv_activity::events"
                    )
                })),
            ),
        ],
    }
}

/// Init tracing with only local-format specs (no OTLP).
fn init_tracing_local(
    level: Level,
    specs: &[TraceOutputSpec],
    has_trace_export: bool,
) -> TracingGuard {
    let has_activity_replay = specs
        .iter()
        .any(|spec| matches!(spec, TraceOutputSpec::Render(TraceFormat::Json, _)));
    let mut layers: Vec<Box<dyn Layer<_> + Send + Sync>> = Vec::new();

    for spec in specs {
        layers.extend(create_local_boxed_layers(spec));
    }

    // DevenvLayer must be outermost: its on_new_span/on_close emit synthetic
    // events via ctx.event(), which only dispatch to layers *below* it. Placing
    // it last ensures all export layers receive those events.
    let _ = Registry::default()
        .with(create_filter(level, has_trace_export, has_activity_replay))
        .with(SpanIdLayer)
        .with(layers)
        .with(DevenvLayer::new())
        .try_init();

    TracingGuard::empty()
}

fn init_tracing_with_otlp(
    level: Level,
    specs: &[TraceOutputSpec],
    has_trace_export: bool,
) -> TracingGuard {
    #[cfg(feature = "otlp")]
    {
        otel::init_tracing_unified(level, specs, has_trace_export)
    }

    #[cfg(not(feature = "otlp"))]
    {
        let _ = level;
        let _ = has_trace_export;
        let otlp_protocols: Vec<String> = specs
            .iter()
            .filter_map(|s| match s {
                TraceOutputSpec::Otlp(proto, _) => Some(proto.to_string()),
                _ => None,
            })
            .collect();
        eprintln!(
            "error: trace protocol(s) '{}' require the corresponding cargo feature \
             (otlp-grpc, otlp-http-protobuf, or otlp-http-json)",
            otlp_protocols.join(", ")
        );
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use devenv_activity::{Activity, ActivityEvent, Build, start};
    use tracing::{Event, Subscriber, span};
    use tracing_subscriber::layer::{Context, SubscriberExt};
    use tracing_subscriber::registry::LookupSpan;

    use super::{
        DevenvLayer, Level, SpanIdLayer, TraceFormat, TraceOutputSpec, TraceSink, create_filter,
        create_local_boxed_layers,
    };

    #[derive(Clone, Default)]
    struct ActivityCapture {
        spans: Arc<AtomicUsize>,
        events: Arc<AtomicUsize>,
    }

    impl<S> tracing_subscriber::Layer<S> for ActivityCapture
    where
        S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    {
        fn on_new_span(&self, attrs: &span::Attributes<'_>, _id: &span::Id, _ctx: Context<'_, S>) {
            if attrs.metadata().target() == "devenv_activity::spans" {
                self.spans.fetch_add(1, Ordering::Relaxed);
            }
        }

        fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
            if event.metadata().target() == "devenv_activity::events" {
                self.events.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn emit_activity() {
        let activity = start!(Activity::build("filter test"));
        activity.progress(1, 2, None);
    }

    #[test]
    fn trace_export_captures_activities_even_at_silent_log_level() {
        let capture = ActivityCapture::default();
        let subscriber = tracing_subscriber::Registry::default()
            .with(create_filter(Level::Silent, true, false))
            .with(capture.clone());

        tracing::subscriber::with_default(subscriber, || {
            assert!(!tracing::enabled!(
                target: "devenv_activity::replay",
                tracing::Level::DEBUG
            ));
            emit_activity();
        });

        assert_eq!(capture.spans.load(Ordering::Relaxed), 1);
        assert_eq!(capture.events.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn no_trace_export_disables_activity_tracing_mirror() {
        let capture = ActivityCapture::default();
        let subscriber = tracing_subscriber::Registry::default()
            .with(create_filter(Level::Info, false, false))
            .with(capture.clone());

        tracing::subscriber::with_default(subscriber, emit_activity);

        assert_eq!(capture.spans.load(Ordering::Relaxed), 0);
        assert_eq!(capture.events.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn production_json_layers_export_replayable_activity_events() {
        let tempdir = tempfile::tempdir().unwrap();
        let output = tempdir.path().join("trace.jsonl");
        let spec = TraceOutputSpec::Render(TraceFormat::Json, TraceSink::File(output.clone()));
        let layers = create_local_boxed_layers(&spec);
        let subscriber = tracing_subscriber::Registry::default()
            .with(create_filter(Level::Silent, true, true))
            .with(SpanIdLayer)
            .with(layers)
            .with(DevenvLayer::new());

        tracing::subscriber::with_default(subscriber, || {
            let activity = start!(Activity::build("production JSON test").id(73));
            activity.progress(1, 2, None);
            drop(activity);
        });

        let records = std::fs::read_to_string(output).unwrap();
        let events = records
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .filter(|record| record["target"] == "devenv_activity::events")
            .map(|mut record| {
                serde_json::from_value::<ActivityEvent>(record["fields"]["event"].take()).unwrap()
            })
            .collect::<Vec<_>>();

        assert_eq!(events.len(), 3);
        assert!(matches!(
            events[0],
            ActivityEvent::Build(Build::Start { id: 73, .. })
        ));
        assert!(matches!(
            events[1],
            ActivityEvent::Build(Build::Progress {
                id: 73,
                done: 1,
                expected: 2,
                ..
            })
        ));
        assert!(matches!(
            events[2],
            ActivityEvent::Build(Build::Complete { id: 73, .. })
        ));
    }
}
