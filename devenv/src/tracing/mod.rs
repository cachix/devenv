mod devenv_layer;
mod human_duration;
mod span_ids;
mod span_timings;

use devenv_layer::{DevenvFormat, DevenvLayer};
use span_ids::{SpanContext, SpanIdLayer};

pub use crate::cli::{TraceFormat, TraceOutput};
pub use human_duration::HumanReadableDuration;

use json_subscriber::JsonLayer;
use std::fs::File;
use std::io::{self, IsTerminal, LineWriter, Write};
use std::sync::Mutex;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{EnvFilter, Registry, prelude::*};

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
    EnvFilter::builder()
        .with_default_directive(LevelFilter::from(level).into())
        .from_env_lossy()
        .add_directive("devenv::activity=trace".parse().unwrap())
        .add_directive("watchexec=warn".parse().unwrap())
}

pub fn init_tracing_default() {
    init_tracing(Level::default(), TraceFormat::Json, None, true);
}

/// Initialize tracing.
///
/// When `cli_output` is true, a human-readable stderr layer is added for
/// direct terminal output (used when no TUI is active).
///
/// `trace_format` and `trace_output` control an optional export layer for
/// structured trace output to stdout, stderr, or a file.
pub fn init_tracing(
    level: Level,
    trace_format: TraceFormat,
    trace_output: Option<&TraceOutput>,
    cli_output: bool,
) {
    let base = Registry::default()
        .with(create_filter(level))
        .with(SpanIdLayer);

    // CLI output: human-readable format on stderr (when no TUI handles display)
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

    // Export layer: configurable format to chosen destination
    let ansi = match trace_output {
        Some(TraceOutput::Stdout) => io::stdout().is_terminal(),
        Some(TraceOutput::Stderr) => io::stderr().is_terminal(),
        Some(TraceOutput::File(_)) | None => false,
    };
    let writer = trace_output.and_then(create_trace_writer);

    // DevenvLayer must be outermost: its on_new_span/on_close emit events via
    // ctx.event(), which only dispatches to layers *below* it. Placing it last
    // ensures cli_layer and export_layer receive those events.
    let _ = match trace_format {
        TraceFormat::Full => {
            let export_layer = writer.map(|w| {
                tracing_subscriber::fmt::layer()
                    .with_ansi(ansi)
                    .with_writer(w)
            });
            base.with(cli_layer)
                .with(export_layer)
                .with(DevenvLayer::new())
                .try_init()
        }
        TraceFormat::Pretty => {
            let export_layer = writer.map(|w| {
                tracing_subscriber::fmt::layer()
                    .with_ansi(ansi)
                    .with_writer(w)
                    .pretty()
            });
            base.with(cli_layer)
                .with(export_layer)
                .with(DevenvLayer::new())
                .try_init()
        }
        TraceFormat::Json => {
            let export_layer = writer.map(create_json_layer);
            base.with(cli_layer)
                .with(export_layer)
                .with(DevenvLayer::new())
                .try_init()
        }
    };
}
