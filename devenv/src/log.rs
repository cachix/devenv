use json_subscriber::JsonLayer;
use std::fs::File;
use std::io::{self, IsTerminal};
use std::path::Path;
use std::sync::{Arc, };
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{
    EnvFilter,
    prelude::*,
};

use crate::tracing::{SpanIdLayer, SpanAttributesLayer, SpanAttributes, SpanIds, DevenvLayer, DevenvFormat, DevenvFieldFormatter, IndicatifLayer, DevenvIndicatifFilter};

pub(crate) use crate::tracing::HumanReadableDuration;

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

// Re-export LogFormat from devenv_core
pub use devenv_core::cli::LogFormat;

fn create_json_export_layer<S>(file: File) -> JsonLayer<S, File>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    let mut layer = JsonLayer::new(file);
    layer.with_timer("timestamp", tracing_subscriber::fmt::time::SystemTime);
    layer.with_level("level");
    layer.with_target("target");
    layer.serialize_extension::<SpanIds>("span_ids");
    layer.serialize_extension::<SpanAttributes>("span_attrs");
    layer.with_event("fields");
    layer
}

pub fn init_tracing_default() {
    let shutdown = tokio_shutdown::Shutdown::new();
    init_tracing(Level::default(), LogFormat::default(), None, shutdown);
}

pub fn init_tracing(
    level: Level,
    log_format: LogFormat,
    trace_export_file: Option<&Path>,
    shutdown: Arc<tokio_shutdown::Shutdown>,
) {
    let devenv_layer = DevenvLayer::new();
    let span_id_layer = SpanIdLayer;
    let span_attrs_layer = SpanAttributesLayer;

    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::from(level).into())
        .from_env_lossy();

    let stderr = io::stderr;
    let ansi = stderr().is_terminal();

    let export_file = trace_export_file.and_then(|path| File::create(path).ok());

    match log_format {
        LogFormat::TracingFull => {
            let stderr_layer = tracing_subscriber::fmt::layer()
                .with_writer(stderr)
                .with_ansi(ansi);

            match export_file {
                Some(file) => {
                    let file_layer = create_json_export_layer(file);
                    tracing_subscriber::registry()
                        .with(span_id_layer)
                        .with(span_attrs_layer)
                        .with(filter)
                        .with(devenv_layer)
                        .with(stderr_layer)
                        .with(file_layer)
                        .init();
                }
                None => {
                    tracing_subscriber::registry()
                        .with(span_id_layer)
                        .with(span_attrs_layer)
                        .with(filter)
                        .with(devenv_layer)
                        .with(stderr_layer)
                        .init();
                }
            }
        }
        LogFormat::TracingPretty => {
            let stderr_layer = tracing_subscriber::fmt::layer()
                .with_writer(stderr)
                .with_ansi(ansi)
                .pretty();

            match export_file {
                Some(file) => {
                    let file_layer = create_json_export_layer(file);
                    tracing_subscriber::registry()
                        .with(span_id_layer)
                        .with(span_attrs_layer)
                        .with(filter)
                        .with(devenv_layer)
                        .with(stderr_layer)
                        .with(file_layer)
                        .init();
                }
                None => {
                    tracing_subscriber::registry()
                        .with(span_id_layer)
                        .with(span_attrs_layer)
                        .with(filter)
                        .with(devenv_layer)
                        .with(stderr_layer)
                        .init();
                }
            }
        }
        LogFormat::Cli => {
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
                        .with(span_attrs_layer)
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
                        .with(span_attrs_layer)
                        .with(filter)
                        .with(devenv_layer)
                        .with(stderr_layer)
                        .with(filtered_layer)
                        .init();
                }
            }
        }
        LogFormat::TracingJson => {
            fn create_stderr_layer<S>() -> JsonLayer<S>
            where
                S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
            {
                let mut layer = JsonLayer::stdout();
                layer.with_timer("timestamp", tracing_subscriber::fmt::time::SystemTime);
                layer.with_level("level");
                layer.with_target("target");
                layer.serialize_extension::<SpanIds>("span_ids");
                layer.serialize_extension::<SpanAttributes>("span_attrs");
                layer.with_event("fields");
                layer
            }

            match export_file {
                Some(file) => {
                    let file_layer = create_json_export_layer(file);
                    let stderr_layer = create_stderr_layer();
                    tracing_subscriber::registry()
                        .with(span_id_layer)
                        .with(span_attrs_layer)
                        .with(filter)
                        .with(stderr_layer)
                        .with(file_layer)
                        .init();
                }
                None => {
                    tracing_subscriber::registry()
                        .with(span_id_layer)
                        .with(span_attrs_layer)
                        .with(filter)
                        .with(create_stderr_layer())
                        .init();
                }
            }
        }
        LogFormat::Tui => {
            // Initialize TUI with proper shutdown coordination
            let tui_handle = devenv_tui::TuiHandle::init();

            // Create activity layers that forward to TUI
            let activity_tx = tui_handle.activity_tx();

            // Spawn TUI app in background
            let shutdown_clone = shutdown.clone();
            let tui_handle_clone = tui_handle.clone();
            tokio::spawn(async move {
                let _ = devenv_tui::app::run_app(tui_handle_clone, shutdown_clone).await;
            });

            // Register layers including activity layers
            tracing_subscriber::registry()
                .with(filter)
                .with(devenv_layer)
                .with(span_id_layer)
                .with(span_attrs_layer)
                .init();
        }
    }
}


