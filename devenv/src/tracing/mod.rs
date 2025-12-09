mod devenv_layer;
mod human_duration;
mod indicatif_layer;
mod span_ids;
mod span_timings;

use devenv_layer::{DevenvFieldFormatter, DevenvFormat, DevenvLayer};
use indicatif_layer::{DevenvIndicatifFilter, IndicatifLayer};
use span_ids::{SpanIdLayer, SpanIds};

pub use human_duration::HumanReadableDuration;

use json_subscriber::JsonLayer;
use std::fs::File;
use std::io::{self, IsTerminal};
use std::path::Path;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{EnvFilter, prelude::*};

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

pub use devenv_core::cli::TraceFormat;

fn create_json_export_layer<S>(file: File) -> JsonLayer<S, File>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    let mut layer = JsonLayer::new(file);
    layer.with_timer("timestamp", tracing_subscriber::fmt::time::SystemTime);
    layer.with_level("level");
    layer.with_target("target");
    layer.serialize_extension::<SpanIds>("span_ids");
    layer.with_event("fields");
    layer
}

pub fn init_tracing_default() {
    init_tracing(Level::default(), TraceFormat::default(), None);
}

pub fn init_tracing(level: Level, trace_format: TraceFormat, trace_export_file: Option<&Path>) {
    let devenv_layer = DevenvLayer::new();
    let span_id_layer = SpanIdLayer;

    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::from(level).into())
        .from_env_lossy()
        // Always include activity events for trace export
        .add_directive("devenv::activity=trace".parse().unwrap());

    let stderr = io::stderr;
    let ansi = stderr().is_terminal();

    let export_file = trace_export_file.and_then(|path| File::create(path).ok());

    match trace_format {
        TraceFormat::TracingFull => {
            let stderr_layer = tracing_subscriber::fmt::layer()
                .with_writer(stderr)
                .with_ansi(ansi);

            match export_file {
                Some(file) => {
                    let file_layer = create_json_export_layer(file);
                    tracing_subscriber::registry()
                        .with(span_id_layer)
                        .with(filter)
                        .with(devenv_layer)
                        .with(stderr_layer)
                        .with(file_layer)
                        .init();
                }
                None => {
                    tracing_subscriber::registry()
                        .with(span_id_layer)
                        .with(filter)
                        .with(devenv_layer)
                        .with(stderr_layer)
                        .init();
                }
            }
        }
        TraceFormat::TracingPretty => {
            let stderr_layer = tracing_subscriber::fmt::layer()
                .with_writer(stderr)
                .with_ansi(ansi)
                .pretty();

            match export_file {
                Some(file) => {
                    let file_layer = create_json_export_layer(file);
                    tracing_subscriber::registry()
                        .with(span_id_layer)
                        .with(filter)
                        .with(devenv_layer)
                        .with(stderr_layer)
                        .with(file_layer)
                        .init();
                }
                None => {
                    tracing_subscriber::registry()
                        .with(span_id_layer)
                        .with(filter)
                        .with(devenv_layer)
                        .with(stderr_layer)
                        .init();
                }
            }
        }
        TraceFormat::LegacyCli => {
            // For CLI mode, use IndicatifLayer to coordinate ALL output with progress bars
            let style = tracing_indicatif::style::ProgressStyle::with_template(
                "{spinner:.blue} {span_fields}",
            )
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]);
            let indicatif_layer = IndicatifLayer::new()
                .with_progress_style(style)
                .with_span_field_formatter(DevenvFieldFormatter);

            // Get the managed writer before moving indicatif_layer into filter
            let indicatif_writer = indicatif_layer.get_stderr_writer();
            let filtered_layer = DevenvIndicatifFilter::new(indicatif_layer);

            // Use indicatif's managed writer for the fmt layer so all output is coordinated
            let stderr_layer = tracing_subscriber::fmt::layer()
                .event_format(DevenvFormat::default())
                .with_writer(indicatif_writer)
                .with_ansi(ansi);

            match export_file {
                Some(file) => {
                    let file_layer = create_json_export_layer(file);
                    tracing_subscriber::registry()
                        .with(span_id_layer)
                        .with(filter)
                        .with(devenv_layer)
                        .with(stderr_layer)
                        .with(filtered_layer)
                        .with(file_layer)
                        .init();
                }
                None => {
                    tracing_subscriber::registry()
                        .with(span_id_layer)
                        .with(filter)
                        .with(devenv_layer)
                        .with(stderr_layer)
                        .with(filtered_layer)
                        .init();
                }
            }
        }
        TraceFormat::TracingJson => {
            fn create_stderr_layer<S>() -> JsonLayer<S>
            where
                S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
            {
                let mut layer = JsonLayer::stdout();
                layer.with_timer("timestamp", tracing_subscriber::fmt::time::SystemTime);
                layer.with_level("level");
                layer.with_target("target");
                layer.serialize_extension::<SpanIds>("span_ids");
                layer.with_event("fields");
                layer
            }

            match export_file {
                Some(file) => {
                    let file_layer = create_json_export_layer(file);
                    let stderr_layer = create_stderr_layer();
                    tracing_subscriber::registry()
                        .with(span_id_layer)
                        .with(filter)
                        .with(stderr_layer)
                        .with(file_layer)
                        .init();
                }
                None => {
                    tracing_subscriber::registry()
                        .with(span_id_layer)
                        .with(filter)
                        .with(create_stderr_layer())
                        .init();
                }
            }
        }
        TraceFormat::Tui => {
            // TUI displays activities via channel, not tracing output.
            // Only set up file export if requested.
            match export_file {
                Some(file) => {
                    let file_layer = create_json_export_layer(file);
                    tracing_subscriber::registry()
                        .with(span_id_layer)
                        .with(filter)
                        .with(file_layer)
                        .init();
                }
                None => {
                    // No tracing output needed - TUI handles display
                }
            }
        }
    }
}
