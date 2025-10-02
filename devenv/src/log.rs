use console::style;
use std::collections::HashSet;
use std::fmt;
use std::io::{self, IsTerminal};
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
    fmt::{FmtContext, FormatEvent, FormatFields, format::Writer},
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
    Cli,
    /// Enhanced TUI interface with operations and logs.
    #[default]
    Tui,
    /// A verbose structured log format used for debugging.
    TracingFull,
    /// A pretty human-readable log format used for debugging.
    TracingPretty,
}

/// Initialize tracing with SubsystemHandle for proper shutdown coordination
pub fn init_tracing(
    level: Level,
    log_format: LogFormat,
    shutdown: Arc<tokio_shutdown::Shutdown>,
) -> Option<()> {
    let devenv_layer = DevenvLayer::new();

    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::from(level).into())
        .from_env_lossy();

    match log_format {
        LogFormat::TracingFull => {
            let fmt_layer = tracing_subscriber::fmt::layer()
                .with_thread_names(true)
                .with_file(true)
                .with_line_number(true);

            tracing_subscriber::registry()
                .with(filter)
                .with(fmt_layer)
                .with(devenv_layer)
                .init();
        }
        LogFormat::TracingPretty => {
            let stderr_layer = tracing_subscriber::fmt::layer()
                .pretty()
                .with_writer(std::io::stderr);

            tracing_subscriber::registry()
                .with(filter)
                .with(stderr_layer)
                .with(devenv_layer)
                .init();
        }
        LogFormat::Tui => {
            // Initialize TUI with graceful shutdown support
            let tui_handle = devenv_tui::init_tui();

            // Start the TUI with proper cancellation support
            let shutdown_clone = shutdown.clone();
            let model = tui_handle.model();
            tokio::spawn(async move {
                tokio::select! {
                    result = devenv_tui::app::run_app(model, shutdown_clone.clone()) => {
                        if let Err(e) = result {
                            eprintln!("TUI error: {}", e);
                        }
                        // TUI completed - trigger shutdown
                        shutdown_clone.shutdown().await;
                    }
                    _ = shutdown_clone.wait_for_shutdown() => {
                        // Shutdown was requested externally
                    }
                }
            });

            tracing_subscriber::registry()
                .with(filter)
                .with(devenv_layer)
                .with(tui_handle.layer)
                .init();
            return None; // No longer returning sender since we use direct model updates
        }
        LogFormat::Cli => {
            // For CLI format, use indicatif layer if available, otherwise basic fmt
            use indicatif::ProgressStyle;
            let indicatif_layer = tracing_indicatif::IndicatifLayer::new().with_progress_style(
                ProgressStyle::with_template("{spinner:.green} {wide_msg}")
                    .unwrap()
                    .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
            );

            tracing_subscriber::registry()
                .with(filter)
                .with(indicatif_layer)
                .with(devenv_layer)
                .init();
        }
    }

    None
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
    /// The ui message associated with the span.
    message: String,
    /// The ui type (activity type like "build", "download", "eval", "task", "user", "command").
    ui_type: Option<String>,
    /// Additional detail information like phase, status, etc.
    detail: Option<String>,
    /// Unique operation identifier.
    operation_id: Option<String>,
    /// Whether the span has an error event.
    has_error: bool,
    /// Span timings
    timings: SpanTimings,
    /// Progress tracking
    progress_current: Option<u64>,
    progress_total: Option<u64>,
    progress_unit: Option<String>,
    progress_percent: Option<f32>,
    /// Download progress
    download_size_current: Option<u64>,
    download_size_total: Option<u64>,
    download_speed: Option<u64>,
    /// Build info
    build_phase: Option<String>,
    /// Log collection
    log_stdout_lines: Vec<String>,
    log_stderr_lines: Vec<String>,
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

/// Custom field formatter that extracts devenv.ui.* values for display
struct DevenvFieldFormatter;

impl<'a> FormatFields<'a> for DevenvFieldFormatter {
    fn format_fields<R>(&self, mut writer: Writer<'a>, fields: R) -> fmt::Result
    where
        R: tracing_subscriber::field::RecordFields,
    {
        // Extract UI fields for display
        #[derive(Default)]
        struct UiFieldExtractor {
            message: Option<String>,
            ui_type: Option<String>,
            detail: Option<String>,
        }

        impl Visit for UiFieldExtractor {
            fn record_debug(&mut self, _field: &Field, _value: &dyn fmt::Debug) {}

            fn record_str(&mut self, field: &Field, value: &str) {
                match field.name() {
                    "devenv.ui.message" => self.message = Some(value.to_string()),
                    "devenv.ui.type" => self.ui_type = Some(value.to_string()),
                    "devenv.ui.detail" => self.detail = Some(value.to_string()),
                    _ => {}
                }
            }
        }

        let mut extractor = UiFieldExtractor::default();
        fields.record(&mut extractor);

        if let Some(message) = extractor.message {
            let formatted = format_ui_message(&extractor.ui_type, &message, &extractor.detail);
            write!(writer, "{formatted}")
        } else {
            // Fallback - show nothing for spans without ui messages
            Ok(())
        }
    }
}

/// Format a UI message based on its type and details
fn format_ui_message(ui_type: &Option<String>, message: &str, detail: &Option<String>) -> String {
    match ui_type.as_deref() {
        Some("user") => message.to_string(),
        Some("build") => {
            let detail_str = detail.as_deref().unwrap_or("");
            if detail_str.is_empty() {
                format!("building {}", message)
            } else {
                format!("building {} {}", message, detail_str)
            }
        }
        Some("download") => {
            let detail_str = detail.as_deref().unwrap_or("");
            if detail_str.is_empty() {
                format!("downloading {}", message)
            } else {
                format!("downloading {} {}", message, detail_str)
            }
        }
        Some("eval") => format!("evaluating {}", message),
        Some("task") => {
            if let Some(detail_str) = detail {
                format!("{} ({})", message, detail_str)
            } else {
                message.to_string()
            }
        }
        Some("command") => {
            if let Some(detail_str) = detail {
                format!("{} [{}]", message, detail_str)
            } else {
                message.to_string()
            }
        }
        _ => message.to_string(), // Default case
    }
}

/// A filter layer that wraps IndicatifLayer and only shows progress bars for spans with UI messages
pub struct DevenvIndicatifFilter<S, F> {
    inner: IndicatifLayer<S, F>,
    ui_message_spans: RwLock<HashSet<span::Id>>,
}

impl<S, F> DevenvIndicatifFilter<S, F> {
    pub fn new(inner: IndicatifLayer<S, F>) -> Self {
        Self {
            inner,
            ui_message_spans: RwLock::new(HashSet::new()),
        }
    }
}

impl<S, F> Layer<S> for DevenvIndicatifFilter<S, F>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    F: for<'writer> FormatFields<'writer> + 'static,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: layer::Context<'_, S>) {
        // Check if this span has devenv.ui.message field and extract the message
        #[derive(Default)]
        struct UiMessageVisitor(Option<String>);

        impl Visit for UiMessageVisitor {
            fn record_debug(&mut self, _field: &Field, _value: &dyn fmt::Debug) {}

            fn record_str(&mut self, field: &Field, value: &str) {
                if field.name() == "devenv.ui.message" {
                    self.0 = Some(value.to_string());
                }
            }
        }

        let mut visitor = UiMessageVisitor::default();
        attrs.record(&mut visitor);

        if let Some(_ui_message) = visitor.0 {
            // This span has a ui message, so it should get a progress bar
            if let Ok(mut spans) = self.ui_message_spans.write() {
                spans.insert(id.clone());
            }

            // Forward the span to IndicatifLayer - it will show devenv.ui.* fields in {span_fields}
            self.inner.on_new_span(attrs, id, ctx);
        }
    }

    fn on_enter(&self, id: &span::Id, ctx: layer::Context<'_, S>) {
        // Only forward if this is a ui message span
        if let Ok(spans) = self.ui_message_spans.read() {
            if spans.contains(id) {
                self.inner.on_enter(id, ctx);
            }
        }
    }

    fn on_exit(&self, id: &span::Id, ctx: layer::Context<'_, S>) {
        // Only forward if this is a ui message span
        if let Ok(spans) = self.ui_message_spans.read() {
            if spans.contains(id) {
                self.inner.on_exit(id, ctx);
            }
        }
    }

    fn on_close(&self, id: span::Id, ctx: layer::Context<'_, S>) {
        // Only forward if this is a ui message span
        let should_forward = if let Ok(mut spans) = self.ui_message_spans.write() {
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
        struct UiFieldsVisitor {
            message: Option<String>,
            ui_type: Option<String>,
            detail: Option<String>,
            operation_id: Option<String>,
            progress_current: Option<u64>,
            progress_total: Option<u64>,
            progress_unit: Option<String>,
            progress_percent: Option<f32>,
            download_size_current: Option<u64>,
            download_size_total: Option<u64>,
            download_speed: Option<u64>,
            build_phase: Option<String>,
        }

        impl Visit for UiFieldsVisitor {
            fn record_debug(&mut self, _field: &Field, _value: &dyn fmt::Debug) {}

            fn record_str(&mut self, field: &Field, value: &str) {
                match field.name() {
                    "devenv.ui.message" => self.message = Some(value.to_string()),
                    "devenv.ui.type" => self.ui_type = Some(value.to_string()),
                    "devenv.ui.detail" => self.detail = Some(value.to_string()),
                    "devenv.ui.id" => self.operation_id = Some(value.to_string()),
                    "devenv.ui.progress.unit" => self.progress_unit = Some(value.to_string()),
                    "devenv.ui.build.phase" => self.build_phase = Some(value.to_string()),
                    _ => {}
                }
            }

            fn record_u64(&mut self, field: &Field, value: u64) {
                match field.name() {
                    "devenv.ui.progress.current" => self.progress_current = Some(value),
                    "devenv.ui.progress.total" => self.progress_total = Some(value),
                    "devenv.ui.download.size_current" => self.download_size_current = Some(value),
                    "devenv.ui.download.size_total" => self.download_size_total = Some(value),
                    "devenv.ui.download.speed" => self.download_speed = Some(value),
                    _ => {}
                }
            }

            fn record_f64(&mut self, field: &Field, value: f64) {
                if field.name() == "devenv.ui.progress.percent" {
                    self.progress_percent = Some(value as f32);
                }
            }
        }

        let mut visitor = UiFieldsVisitor::default();
        attrs.record(&mut visitor);

        let mut ext = span.extensions_mut();

        if let Some(message) = visitor.message {
            ext.insert(SpanContext {
                message: message.clone(),
                ui_type: visitor.ui_type,
                detail: visitor.detail,
                operation_id: visitor.operation_id,
                has_error: false,
                timings: SpanTimings::new(),
                progress_current: visitor.progress_current,
                progress_total: visitor.progress_total,
                progress_unit: visitor.progress_unit,
                progress_percent: visitor.progress_percent,
                download_size_current: visitor.download_size_current,
                download_size_total: visitor.download_size_total,
                download_speed: visitor.download_speed,
                build_phase: visitor.build_phase,
                log_stdout_lines: Vec::new(),
                log_stderr_lines: Vec::new(),
            });
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

            let message = span_ctx.message.clone();
            let time_total = format!("{}", span_ctx.timings.total_duration());

            // Emit the final message event
            with_event_from_span!(
                id,
                span,
                "message" = message,
                "devenv.is_ui_message" = true,
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

    // Track if any error events are emitted and handle progress/log events
    fn on_event(&self, event: &tracing::Event<'_>, ctx: layer::Context<'_, S>) {
        if event.metadata().level() == &tracing::Level::ERROR {
            self.has_error.store(true, Ordering::SeqCst);
        }

        // Handle progress and log events
        let target = event.metadata().target();
        if target == "devenv.ui.progress" || target == "devenv.ui.log" {
            if let Some(span) = ctx.current_span().id() {
                if let Some(span_ref) = ctx.span(span) {
                    let mut extensions = span_ref.extensions_mut();
                    if let Some(span_ctx) = extensions.get_mut::<SpanContext>() {
                        // Handle progress updates
                        if target == "devenv.ui.progress" {
                            #[derive(Default)]
                            struct ProgressVisitor {
                                current: Option<u64>,
                                total: Option<u64>,
                                unit: Option<String>,
                                percent: Option<f32>,
                            }

                            impl Visit for ProgressVisitor {
                                fn record_debug(
                                    &mut self,
                                    _field: &Field,
                                    _value: &dyn fmt::Debug,
                                ) {
                                }

                                fn record_str(&mut self, field: &Field, value: &str) {
                                    if field.name() == "devenv.ui.progress.unit" {
                                        self.unit = Some(value.to_string());
                                    }
                                }

                                fn record_u64(&mut self, field: &Field, value: u64) {
                                    match field.name() {
                                        "devenv.ui.progress.current" => self.current = Some(value),
                                        "devenv.ui.progress.total" => self.total = Some(value),
                                        _ => {}
                                    }
                                }

                                fn record_f64(&mut self, field: &Field, value: f64) {
                                    if field.name() == "devenv.ui.progress.percent" {
                                        self.percent = Some(value as f32);
                                    }
                                }
                            }

                            let mut visitor = ProgressVisitor::default();
                            event.record(&mut visitor);

                            if let Some(current) = visitor.current {
                                span_ctx.progress_current = Some(current);
                            }
                            if let Some(total) = visitor.total {
                                span_ctx.progress_total = Some(total);
                            }
                            if let Some(unit) = visitor.unit {
                                span_ctx.progress_unit = Some(unit);
                            }
                            if let Some(percent) = visitor.percent {
                                span_ctx.progress_percent = Some(percent);
                            }
                        }

                        // Handle log events
                        if target == "devenv.ui.log" {
                            #[derive(Default)]
                            struct LogVisitor {
                                stdout: Option<String>,
                                stderr: Option<String>,
                            }

                            impl Visit for LogVisitor {
                                fn record_debug(
                                    &mut self,
                                    _field: &Field,
                                    _value: &dyn fmt::Debug,
                                ) {
                                }

                                fn record_str(&mut self, field: &Field, value: &str) {
                                    match field.name() {
                                        "devenv.ui.log.stdout" => {
                                            self.stdout = Some(value.to_string())
                                        }
                                        "devenv.ui.log.stderr" => {
                                            self.stderr = Some(value.to_string())
                                        }
                                        _ => {}
                                    }
                                }
                            }

                            let mut visitor = LogVisitor::default();
                            event.record(&mut visitor);

                            if let Some(stdout_line) = visitor.stdout {
                                span_ctx.log_stdout_lines.push(stdout_line);
                            }
                            if let Some(stderr_line) = visitor.stderr {
                                span_ctx.log_stderr_lines.push(stderr_line);
                            }
                        }
                    }
                }
            }
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
            is_ui_message: bool,
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
                if field.name() == "devenv.is_ui_message" {
                    self.is_ui_message = value;
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
                && visitor.is_ui_message
            {
                let time_total = format!("{}", span_ctx.timings.total_duration());
                let has_error = span_ctx.has_error;
                let formatted_message =
                    format_ui_message(&span_ctx.ui_type, &span_ctx.message, &span_ctx.detail);

                match span_kind {
                    SpanKind::Start => {
                        // IndicatifLayer will handle the spinner, but we still need to
                        // return early to avoid duplicate output in our format layer
                        return Ok(());
                    }

                    SpanKind::End => {
                        let prefix = if has_error {
                            style("✖").red()
                        } else {
                            style("✓").green()
                        };
                        return writeln!(
                            writer,
                            "{} {} in {}",
                            prefix, formatted_message, time_total
                        );
                    }
                }
            }
        }

        if let Some(msg) = visitor.message {
            if visitor.is_ui_message {
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
