mod devenv_layer;
mod human_duration;
#[cfg(feature = "otlp")]
mod otel;
mod span_ids;
mod span_timings;

use devenv_layer::{DevenvFormat, DevenvLayer};
use span_ids::{SpanContext, SpanIdLayer};

pub use crate::cli::{OtlpProtocol, TraceFormat, TraceOutputSpec, TraceSink};
pub use human_duration::HumanReadableDuration;

use json_subscriber::JsonLayer;
use std::fs::File;
use std::io::{self, IsTerminal, LineWriter, Write};
use std::sync::Mutex;
use tracing::level_filters::LevelFilter;
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

fn create_trace_writer(sink: &TraceSink) -> Option<Mutex<TraceWriter>> {
    match sink {
        TraceSink::Stdout => Some(Mutex::new(TraceWriter::Stdout(io::stdout()))),
        TraceSink::Stderr => Some(Mutex::new(TraceWriter::Stderr(LineWriter::new(
            io::stderr(),
        )))),
        TraceSink::File(path) => match File::create(path) {
            Ok(f) => Some(Mutex::new(TraceWriter::File(LineWriter::new(f)))),
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
    layer
}

fn create_filter(level: Level) -> EnvFilter {
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::from(level).into())
        .from_env_lossy()
        .add_directive("watchexec=warn".parse().unwrap());

    if level <= Level::Warn {
        // In quiet mode the TUI is off and activity span events would just
        // leak to stderr, so suppress them entirely.
        filter.add_directive("devenv_activity=warn".parse().unwrap())
    } else {
        // Activity spans at trace level are needed so the TUI can render
        // all activity events.
        filter.add_directive("devenv_activity=trace".parse().unwrap())
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
/// directly. Activity start/complete output is produced separately by the
/// activity channel consumers ([`crate::console::ConsoleOutput`] or the TUI).
///
/// Each `TraceOutputSpec` adds an export layer with its own format and destination.
/// Multiple outputs can be active simultaneously (e.g. pretty to stderr + JSON to file).
///
/// Returns a [`TracingGuard`] that must be held until program exit to ensure
/// proper flushing of trace data.
pub fn init_tracing(level: Level, specs: &[TraceOutputSpec]) -> TracingGuard {
    let has_otlp = specs
        .iter()
        .any(|s| matches!(s, TraceOutputSpec::Otlp(_, _)));

    if has_otlp {
        return init_tracing_with_otlp(level, specs);
    }

    init_tracing_local(level, specs)
}

/// Create a boxed render layer for a `Render` spec. Returns `None` for OTLP specs.
pub(crate) fn create_local_boxed_layer<S>(
    spec: &TraceOutputSpec,
) -> Option<Box<dyn Layer<S> + Send + Sync>>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    let (format, sink) = match spec {
        TraceOutputSpec::Render(format, sink) => (format, sink),
        TraceOutputSpec::Otlp(_, _) => return None,
    };
    let writer = create_trace_writer(sink)?;
    let ansi = match sink {
        TraceSink::Stdout => io::stdout().is_terminal(),
        TraceSink::Stderr => io::stderr().is_terminal(),
        TraceSink::File(_) => false,
    };
    match format {
        TraceFormat::Full => Some(Box::new(
            tracing_subscriber::fmt::layer()
                .with_ansi(ansi)
                .with_writer(writer),
        )),
        TraceFormat::Pretty => Some(Box::new(
            tracing_subscriber::fmt::layer()
                .with_ansi(ansi)
                .with_writer(writer)
                .pretty(),
        )),
        TraceFormat::Json => Some(Box::new(create_json_layer(writer))),
    }
}

/// Renders WARN/ERROR `tracing` events to stderr alongside the activity
/// channel. Lower levels never reach the terminal — `--trace-to` is the
/// escape hatch for debug/trace output.
pub(crate) fn build_cli_layer<S>() -> Box<dyn Layer<S> + Send + Sync>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    let ansi = io::stderr().is_terminal();
    Box::new(
        tracing_subscriber::fmt::layer()
            .event_format(DevenvFormat)
            .with_writer(io::stderr)
            .with_ansi(ansi),
    )
}

/// Init tracing with only local-format specs (no OTLP).
fn init_tracing_local(level: Level, specs: &[TraceOutputSpec]) -> TracingGuard {
    let mut layers: Vec<Box<dyn Layer<_> + Send + Sync>> = Vec::new();

    layers.push(build_cli_layer());

    for spec in specs {
        if let Some(layer) = create_local_boxed_layer(spec) {
            layers.push(layer);
        }
    }

    // DevenvLayer must be outermost: its on_new_span/on_close emit synthetic
    // events via ctx.event(), which only dispatch to layers *below* it. Placing
    // it last ensures all export layers receive those events.
    let _ = Registry::default()
        .with(create_filter(level))
        .with(SpanIdLayer)
        .with(layers)
        .with(DevenvLayer::new())
        .try_init();

    TracingGuard::empty()
}

fn init_tracing_with_otlp(level: Level, specs: &[TraceOutputSpec]) -> TracingGuard {
    #[cfg(feature = "otlp")]
    {
        otel::init_tracing_unified(level, specs)
    }

    #[cfg(not(feature = "otlp"))]
    {
        let _ = level;
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
