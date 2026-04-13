mod devenv_layer;
mod human_duration;
#[cfg(any(
    feature = "otlp-grpc",
    feature = "otlp-http-protobuf",
    feature = "otlp-http-json"
))]
mod otel;
mod span_ids;
mod span_timings;

use devenv_layer::{DevenvFormat, DevenvLayer};
use span_ids::{SpanContext, SpanIdLayer};

pub use crate::cli::{TraceFormat, TraceOutput, TraceOutputSpec};
pub use human_duration::HumanReadableDuration;

#[cfg(not(any(
    feature = "otlp-grpc",
    feature = "otlp-http-protobuf",
    feature = "otlp-http-json"
)))]
use clap::ValueEnum;
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

fn create_trace_writer(output: &TraceOutput) -> Option<Mutex<TraceWriter>> {
    match output {
        TraceOutput::Stdout => Some(Mutex::new(TraceWriter::Stdout(io::stdout()))),
        TraceOutput::Stderr => Some(Mutex::new(TraceWriter::Stderr(LineWriter::new(
            io::stderr(),
        )))),
        TraceOutput::File(path) => File::create(path)
            .ok()
            .map(|f| Mutex::new(TraceWriter::File(LineWriter::new(f)))),
        TraceOutput::Url(_) => None,
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

    if level >= Level::Warn {
        // In quiet mode the TUI is off and activity span events would just
        // leak to stderr, so suppress them entirely.
        filter.add_directive("devenv_activity=warn".parse().unwrap())
    } else {
        // Activity spans at trace level are needed so the TUI can render
        // all activity events.
        filter.add_directive("devenv::activity=trace".parse().unwrap())
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
    init_tracing(Level::default(), &[], true)
}

/// Initialize tracing with multiple output specs.
///
/// When `cli_output` is true, a human-readable stderr layer is added for
/// direct terminal output (used when no TUI is active).
///
/// Each `TraceOutputSpec` adds an export layer with its own format and destination.
/// Multiple outputs can be active simultaneously (e.g. pretty to stderr + JSON to file).
///
/// Returns a [`TracingGuard`] that must be held until program exit to ensure
/// proper flushing of trace data.
pub fn init_tracing(level: Level, specs: &[TraceOutputSpec], cli_output: bool) -> TracingGuard {
    let has_otlp = specs.iter().any(|s| s.format.is_otlp());

    if has_otlp {
        return init_tracing_with_otlp(level, specs, cli_output);
    }

    init_tracing_local(level, specs, cli_output)
}

/// Build a CLI output layer (human-readable stderr for direct terminal output).
pub(crate) fn build_cli_layer<S>(
    level: Level,
    cli_output: bool,
) -> Option<Box<dyn Layer<S> + Send + Sync>>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    if !cli_output {
        return None;
    }
    let ansi = io::stderr().is_terminal();
    let verbose = level >= Level::Debug;
    Some(Box::new(
        tracing_subscriber::fmt::layer()
            .event_format(DevenvFormat { verbose })
            .with_writer(io::stderr)
            .with_ansi(ansi),
    ))
}

/// Create a boxed local-format layer for a single spec.
pub(crate) fn create_local_boxed_layer<S>(
    spec: &TraceOutputSpec,
) -> Option<Box<dyn Layer<S> + Send + Sync>>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    let writer = create_trace_writer(&spec.destination)?;
    let ansi = match &spec.destination {
        TraceOutput::Stdout => io::stdout().is_terminal(),
        TraceOutput::Stderr => io::stderr().is_terminal(),
        _ => false,
    };
    match spec.format {
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
        _ => None, // OTLP handled elsewhere
    }
}

/// Init tracing with only local-format specs (no OTLP).
fn init_tracing_local(level: Level, specs: &[TraceOutputSpec], cli_output: bool) -> TracingGuard {
    let mut layers: Vec<Box<dyn Layer<_> + Send + Sync>> = Vec::new();

    if let Some(cli_layer) = build_cli_layer(level, cli_output) {
        layers.push(cli_layer);
    }

    for spec in specs {
        if let Some(layer) = create_local_boxed_layer(spec) {
            layers.push(layer);
        }
    }

    // DevenvLayer must be outermost: its on_new_span/on_close emit events via
    // ctx.event(), which only dispatches to layers *below* it. Placing it last
    // ensures all export layers receive those events.
    let _ = Registry::default()
        .with(create_filter(level))
        .with(SpanIdLayer)
        .with(layers)
        .with(DevenvLayer::new())
        .try_init();

    TracingGuard::empty()
}

fn init_tracing_with_otlp(
    level: Level,
    specs: &[TraceOutputSpec],
    cli_output: bool,
) -> TracingGuard {
    #[cfg(any(
        feature = "otlp-grpc",
        feature = "otlp-http-protobuf",
        feature = "otlp-http-json"
    ))]
    {
        otel::init_tracing_unified(level, specs, cli_output)
    }

    #[cfg(not(any(
        feature = "otlp-grpc",
        feature = "otlp-http-protobuf",
        feature = "otlp-http-json"
    )))]
    {
        let _ = (level, cli_output);
        use clap::ValueEnum;
        let otlp_formats: Vec<_> = specs
            .iter()
            .filter(|s| s.format.is_otlp())
            .map(|s| {
                s.format
                    .to_possible_value()
                    .map(|v| v.get_name().to_string())
                    .unwrap_or_else(|| format!("{:?}", s.format))
            })
            .collect();
        eprintln!(
            "error: trace format(s) '{}' require the corresponding cargo feature \
             (otlp-grpc, otlp-http-protobuf, or otlp-http-json)",
            otlp_formats.join(", ")
        );
        std::process::exit(1);
    }
}
