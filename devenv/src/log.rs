use console::style;
use std::collections::HashSet;
use std::fmt;
use std::fs::File;
use std::io::{self, IsTerminal};
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tracing::level_filters::LevelFilter;
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
    span,
};
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::{
    EnvFilter, Layer,
    fmt::{
        FmtContext, FormatEvent, FormatFields,
        format::{FmtSpan, Writer},
    },
    layer,
    prelude::*,
    registry::LookupSpan,
};

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

#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LogFormat {
    /// The default human-readable log format used in the CLI.
    #[default]
    Cli,
    /// A verbose structured log format used for debugging.
    TracingFull,
    /// A pretty human-readable log format used for debugging.
    TracingPretty,
    /// A JSON log format used for machine consumption.
    TracingJson,
    /// Interactive Terminal User Interface with real-time progress.
    Tui,
}

macro_rules! json_export_layer {
    ($export_file:expr) => {
        $export_file.map(|file| {
            tracing_subscriber::fmt::layer()
                .with_writer(file)
                .json()
                .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
        })
    };
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
            let file_layer = json_export_layer!(export_file);
            tracing_subscriber::registry()
                .with(filter)
                .with(stderr_layer)
                .with(devenv_layer)
                .with(file_layer)
                .init();
        }
        LogFormat::TracingPretty => {
            let stderr_layer = tracing_subscriber::fmt::layer()
                .with_writer(stderr)
                .with_ansi(ansi)
                .pretty();
            let file_layer = json_export_layer!(export_file);
            tracing_subscriber::registry()
                .with(filter)
                .with(stderr_layer)
                .with(devenv_layer)
                .with(file_layer)
                .init();
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

            let file_layer = json_export_layer!(export_file);

            tracing_subscriber::registry()
                .with(filter)
                .with(stderr_layer)
                .with(devenv_layer)
                .with(filtered_layer)
                .with(file_layer)
                .init();
        }
        LogFormat::TracingJson => {
            let stderr_layer = tracing_subscriber::fmt::layer()
                .with_writer(stderr)
                .json()
                .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE);
            let file_layer = json_export_layer!(export_file);
            tracing_subscriber::registry()
                .with(filter)
                .with(stderr_layer)
                .with(file_layer)
                .init();
        }
        LogFormat::Tui => {
            // Initialize TUI with proper shutdown coordination
            let tui_handle = devenv_tui::init_tui();
            let model = tui_handle.model.clone();

            // Spawn TUI app in background
            let shutdown_clone = shutdown.clone();
            tokio::spawn(async move {
                let _ = devenv_tui::app::run_app(model, shutdown_clone).await;
            });

            // Register layers including TUI layer
            tracing_subscriber::registry()
                .with(filter)
                .with(tui_handle.layer)
                .with(devenv_layer)
                .init();
        }
    }
}

/// A structure to capture span timings, similar to what is available internally in tracing_subscriber.
#[derive(Debug, Clone)]
struct SpanTimings {
    idle: Duration,
    busy: Duration,
    last: Instant,
}

impl SpanTimings {
    fn new() -> Self {
        Self {
            idle: Duration::ZERO,
            busy: Duration::ZERO,
            last: Instant::now(),
        }
    }

    fn enter(&mut self) {
        let now = Instant::now();
        self.idle += now - self.last;
        self.last = now;
    }

    fn exit(&mut self) {
        let now = Instant::now();
        self.busy += now - self.last;
        self.last = now;
    }

    /// Returns the total duration of the span, combining the idle and busy times.
    fn total_duration(&self) -> HumanReadableDuration {
        HumanReadableDuration(self.idle + self.busy)
    }
}

pub struct HumanReadableDuration(pub Duration);

impl std::fmt::Display for HumanReadableDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut t = self.0.as_nanos() as f64;
        for unit in ["ns", "µs", "ms", "s"].iter() {
            if t < 10.0 {
                return write!(f, "{t:.2}{unit}");
            } else if t < 100.0 {
                return write!(f, "{t:.1}{unit}");
            } else if t < 1000.0 {
                return write!(f, "{t:.0}{unit}");
            }
            t /= 1000.0;
        }
        write!(f, "{:.0}s", t * 1000.0)
    }
}

/// Capture additional context during a span.
#[derive(Debug)]
struct SpanContext {
    /// The user message associated with the span.
    msg: String,
    /// Whether the span has an error event.
    has_error: bool,
    /// Whether the spinner should be disabled for this span.
    no_spinner: bool,
    /// Span timings
    timings: SpanTimings,
}

/// The kind of span event based on its lifecycle.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum SpanKind {
    /// Marks the start of a span.
    /// Equivalent to [tracing_subscriber::fmt::format::FmtSpan::NEW].
    Start = 0,
    /// Marks the end of a span.
    /// Equivalent to [tracing_subscriber::fmt::format::FmtSpan::CLOSE].
    End = 1,
}

impl TryFrom<u8> for SpanKind {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, ()> {
        match value {
            0 => Ok(SpanKind::Start),
            1 => Ok(SpanKind::End),
            _ => Err(()),
        }
    }
}

/// A helper to create child events from a span.
/// Borrowed from [tracing_subscriber].
macro_rules! with_event_from_span {
    ($id:ident, $span:ident, $($field:literal = $value:expr),*, |$event:ident| $code:block) => {
        let meta = $span.metadata();
        let cs = meta.callsite();
        let fs = tracing::field::FieldSet::new(&[$($field),*], cs);
        #[allow(unused)]
        let mut iter = fs.iter();
        let v = [$(
            (&iter.next().unwrap(), ::core::option::Option::Some(&$value as &dyn tracing::field::Value)),
        )*];
        let vs = fs.value_set(&v);
        let $event = Event::new_child_of($id, meta, &vs);
        $code
    };
}

use std::sync::atomic::{AtomicBool, Ordering};

/// Custom field formatter that extracts just the devenv.user_message value
struct DevenvFieldFormatter;

impl<'a> FormatFields<'a> for DevenvFieldFormatter {
    fn format_fields<R>(&self, mut writer: Writer<'a>, fields: R) -> fmt::Result
    where
        R: tracing_subscriber::field::RecordFields,
    {
        // Extract just the user message value, not the field name
        #[derive(Default)]
        struct UserMessageExtractor {
            user_message: Option<String>,
        }

        impl Visit for UserMessageExtractor {
            fn record_debug(&mut self, _field: &Field, _value: &dyn fmt::Debug) {}

            fn record_str(&mut self, field: &Field, value: &str) {
                if field.name() == "devenv.user_message" {
                    self.user_message = Some(value.to_string());
                }
            }
        }

        let mut extractor = UserMessageExtractor::default();
        fields.record(&mut extractor);

        if let Some(msg) = extractor.user_message {
            write!(writer, "{msg}")
        } else {
            // Fallback - show nothing for spans without user messages
            Ok(())
        }
    }
}

/// A filter layer that wraps IndicatifLayer and only shows progress bars for spans with `devenv.user_message`
pub struct DevenvIndicatifFilter<S, F> {
    inner: IndicatifLayer<S, F>,
    user_message_spans: RwLock<HashSet<span::Id>>,
}

impl<S, F> DevenvIndicatifFilter<S, F> {
    pub fn new(inner: IndicatifLayer<S, F>) -> Self {
        Self {
            inner,
            user_message_spans: RwLock::new(HashSet::new()),
        }
    }
}

impl<S, F> Layer<S> for DevenvIndicatifFilter<S, F>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    F: for<'writer> FormatFields<'writer> + 'static,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: layer::Context<'_, S>) {
        // Check if this span has devenv.user_message field and extract the message
        #[derive(Default)]
        struct UserMessageVisitor {
            user_message: Option<String>,
            no_spinner: bool,
        }

        impl Visit for UserMessageVisitor {
            fn record_debug(&mut self, _field: &Field, _value: &dyn fmt::Debug) {}

            fn record_str(&mut self, field: &Field, value: &str) {
                if field.name() == "devenv.user_message" {
                    self.user_message = Some(value.to_string());
                }
            }

            fn record_bool(&mut self, field: &Field, value: bool) {
                if field.name() == "devenv.no_spinner" {
                    self.no_spinner = value;
                }
            }
        }

        let mut visitor = UserMessageVisitor::default();
        attrs.record(&mut visitor);

        if let Some(_user_message) = visitor.user_message {
            // This span has a user message, check if spinner should be disabled
            if !visitor.no_spinner {
                // Show progress bar only if spinner is not disabled
                if let Ok(mut spans) = self.user_message_spans.write() {
                    spans.insert(id.clone());
                }

                // Forward the span to IndicatifLayer - it will show devenv.user_message in {span_fields}
                self.inner.on_new_span(attrs, id, ctx);
            }
        }
    }

    fn on_enter(&self, id: &span::Id, ctx: layer::Context<'_, S>) {
        // Only forward if this is a user message span
        if let Ok(spans) = self.user_message_spans.read()
            && spans.contains(id)
        {
            self.inner.on_enter(id, ctx);
        }
    }

    fn on_exit(&self, id: &span::Id, ctx: layer::Context<'_, S>) {
        // Only forward if this is a user message span
        if let Ok(spans) = self.user_message_spans.read()
            && spans.contains(id)
        {
            self.inner.on_exit(id, ctx);
        }
    }

    fn on_close(&self, id: span::Id, ctx: layer::Context<'_, S>) {
        // Only forward if this is a user message span
        let should_forward = if let Ok(mut spans) = self.user_message_spans.write() {
            let contained = spans.contains(&id);
            spans.remove(&id); // Clean up
            contained
        } else {
            false
        };

        if should_forward {
            self.inner.on_close(id, ctx);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: layer::Context<'_, S>) {
        // Forward all events to IndicatifLayer so they appear above progress bars without interruption
        self.inner.on_event(event, ctx);
    }
}

#[derive(Default)]
pub struct DevenvLayer {
    /// Whether the span has an error event.
    has_error: AtomicBool,
}

impl DevenvLayer {
    pub fn new() -> Self {
        Self {
            has_error: AtomicBool::new(false),
        }
    }
}

impl<S> layer::Layer<S> for DevenvLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: layer::Context<'_, S>) {
        let span = ctx.span(id).expect("Span not found in context");

        #[derive(Default)]
        struct UserMessageVisitor {
            user_message: Option<String>,
            no_spinner: bool,
        }

        impl Visit for UserMessageVisitor {
            fn record_debug(&mut self, _field: &Field, _value: &dyn fmt::Debug) {}

            fn record_str(&mut self, field: &Field, value: &str) {
                if field.name() == "devenv.user_message" {
                    self.user_message = Some(value.to_string());
                }
            }

            fn record_bool(&mut self, field: &Field, value: bool) {
                if field.name() == "devenv.no_spinner" {
                    self.no_spinner = value;
                }
            }
        }

        let mut visitor = UserMessageVisitor::default();
        attrs.record(&mut visitor);

        let mut ext = span.extensions_mut();

        if let Some(msg) = visitor.user_message {
            ext.insert(SpanContext {
                msg: msg.clone(),
                has_error: false,
                no_spinner: visitor.no_spinner,
                timings: SpanTimings::new(),
            });

            // If spinner is disabled, emit a start event to show the initial message
            if visitor.no_spinner {
                let msg = msg.clone();

                with_event_from_span!(
                    id,
                    span,
                    "message" = msg,
                    "devenv.ui.message" = true,
                    "devenv.span_event_kind" = SpanKind::Start as u8,
                    "devenv.span_has_error" = false,
                    |event| {
                        drop(ext);
                        drop(span);
                        ctx.event(&event);
                    }
                );
            }
        }
    }

    fn on_enter(&self, id: &span::Id, ctx: layer::Context<'_, S>) {
        let span = ctx.span(id).expect("Span not found in context");
        let mut extensions = span.extensions_mut();
        if let Some(span_ctx) = extensions.get_mut::<SpanContext>() {
            span_ctx.timings.enter();
        }
    }

    fn on_exit(&self, id: &span::Id, ctx: layer::Context<'_, S>) {
        let span = ctx.span(id).expect("Span not found in context");
        let mut extensions = span.extensions_mut();
        if let Some(span_ctx) = extensions.get_mut::<SpanContext>() {
            span_ctx.timings.exit();
        }
    }

    fn on_close(&self, id: span::Id, ctx: layer::Context<'_, S>) {
        let span = ctx.span(&id).expect("Span not found in context");
        let mut extensions = span.extensions_mut();

        if let Some(span_ctx) = extensions.get_mut::<SpanContext>() {
            span_ctx.timings.enter();

            let has_error = self.has_error.load(Ordering::SeqCst);
            if has_error {
                span_ctx.has_error = true;
            }

            let msg = span_ctx.msg.clone();
            let time_total = format!("{}", span_ctx.timings.total_duration());

            // Emit the final message event
            with_event_from_span!(
                id,
                span,
                "message" = msg,
                "devenv.ui.message" = true,
                "devenv.span_event_kind" = SpanKind::End as u8,
                "devenv.span_has_error" = has_error,
                "devenv.time_total" = time_total,
                |event| {
                    drop(extensions);
                    drop(span);
                    ctx.event(&event);
                }
            );
        }
    }

    // Track if any error events are emitted.
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: layer::Context<'_, S>) {
        if event.metadata().level() == &tracing::Level::ERROR {
            self.has_error.store(true, Ordering::SeqCst);
        }
    }
}

#[derive(Default)]
pub struct DevenvFormat {
    pub verbose: bool,
}

impl<S, F> FormatEvent<S, F> for DevenvFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    F: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, F>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        #[derive(Debug, Default)]
        struct EventVisitor {
            message: Option<String>,
            is_user_message: bool,
            span_event_kind: Option<SpanKind>,
        }

        impl Visit for EventVisitor {
            fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
                if field.name() == "message" {
                    self.message = Some(format!("{value:?}"));
                }
            }

            fn record_str(&mut self, field: &Field, value: &str) {
                if field.name() == "message" {
                    self.message = Some(value.to_string());
                }
            }

            fn record_bool(&mut self, field: &Field, value: bool) {
                if field.name() == "devenv.ui.message" {
                    self.is_user_message = value;
                }
            }

            fn record_u64(&mut self, field: &Field, value: u64) {
                if field.name() == "devenv.span_event_kind" {
                    self.span_event_kind = SpanKind::try_from(value as u8).ok()
                }
            }
        }

        let mut visitor = EventVisitor::default();
        event.record(&mut visitor);

        if let Some(span_kind) = visitor.span_event_kind
            && let Some(span) = ctx.parent_span()
        {
            let ext = span.extensions();

            if let Some(span_ctx) = ext.get::<SpanContext>()
                && visitor.is_user_message
            {
                let ansi = writer.has_ansi_escapes();
                let time_total = format!("{}", span_ctx.timings.total_duration());
                let has_error = span_ctx.has_error;
                let msg = &span_ctx.msg;
                match span_kind {
                    SpanKind::Start => {
                        // If spinner is disabled, show the start message with a dot
                        if span_ctx.no_spinner {
                            if ansi {
                                write!(writer, "{prefix} ", prefix = style("•").blue())?;
                            }
                            return writeln!(writer, "{msg}");
                        }
                        // Otherwise, IndicatifLayer will handle the spinner, but we still need to
                        // return early to avoid duplicate output in our format layer
                        return Ok(());
                    }

                    SpanKind::End => {
                        if ansi {
                            let prefix = if has_error {
                                style("✖").red()
                            } else {
                                style("✓").green()
                            };
                            write!(writer, "{prefix} ")?;
                        }
                        return writeln!(writer, "{msg} in {time_total}");
                    }
                }
            }
        }
        if let Some(msg) = visitor.message {
            if visitor.is_user_message {
                let meta = event.metadata();
                let ansi = writer.has_ansi_escapes();

                if ansi && !self.verbose {
                    let level = meta.level();
                    match *level {
                        tracing::Level::ERROR => {
                            write!(writer, "{} ", style("✖").red())?;
                        }
                        tracing::Level::WARN => {
                            write!(writer, "{} ", style("•").yellow())?;
                        }
                        tracing::Level::INFO => {
                            write!(writer, "{} ", style("•").blue())?;
                        }
                        tracing::Level::DEBUG => {
                            write!(writer, "{} ", style("•").italic())?;
                        }
                        _ => {}
                    }
                }
            }

            writeln!(writer, "{msg}")?;
        };

        Ok(())
    }
}
