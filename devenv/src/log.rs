use console::style;
use std::fmt;
use std::io::IsTerminal;
use std::marker::PhantomData;
use std::time::{Duration, Instant};
use tracing::level_filters::LevelFilter;
use tracing::{
    field::{Field, Visit},
    Event,
};
use tracing_core::{span, Subscriber};
use tracing_subscriber::{
    fmt::{format::Writer, FmtContext, FormatEvent, FormatFields},
    layer,
    prelude::*,
    registry::LookupSpan,
    EnvFilter, Layer,
};

#[derive(Default, Clone, Eq, PartialEq, Ord, PartialOrd)]
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

pub fn init_tracing(level: Level) {
    let devenv_layer = DevenvLayer::default();

    let filter = EnvFilter::from_default_env()
        .add_directive(tracing::level_filters::LevelFilter::from(level.clone()).into());

    let subscriber = tracing_subscriber::registry().with(devenv_layer);

    use tracing_subscriber::fmt::format::FmtSpan;

    if level == Level::Debug {
        subscriber
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(std::io::stderr)
                    .with_ansi(std::io::stderr().is_terminal())
                    .with_filter(filter),
            )
            .init();
    } else {
        subscriber
            .with(
                tracing_subscriber::fmt::layer()
                    .with_span_events(FmtSpan::CLOSE | FmtSpan::NEW)
                    .event_format(DevenvFormat::default())
                    .with_writer(std::io::stderr)
                    .with_ansi(std::io::stderr().is_terminal())
                    .with_filter(filter),
            )
            .init();
    };
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

    fn total_duration(&self) -> HumanReadableDuration {
        HumanReadableDuration(self.idle + self.busy)
    }
}

struct HumanReadableDuration(Duration);

impl std::fmt::Display for HumanReadableDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut t = self.0.as_nanos() as f64;
        for unit in ["ns", "µs", "ms", "s"].iter() {
            if t < 10.0 {
                return write!(f, "{:.2}{}", t, unit);
            } else if t < 100.0 {
                return write!(f, "{:.1}{}", t, unit);
            } else if t < 1000.0 {
                return write!(f, "{:.0}{}", t, unit);
            }
            t /= 1000.0;
        }
        write!(f, "{:.0}s", t * 1000.0)
    }
}

/// A newtype to capture and expose span `user_message`s in subsequent events.
struct SpanContext {
    msg: String,
    has_error: bool,
}

use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
pub struct DevenvLayer<S>
where
    S: Subscriber,
{
    has_error: AtomicBool,
    _subscriber: PhantomData<S>,
}

impl<S> layer::Layer<S> for DevenvLayer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: layer::Context<'_, S>) {
        let span = ctx
            .span(id)
            .expect("Span not found in context, this is a bug");

        #[derive(Default)]
        struct UserMessageVisitor(Option<String>);

        impl Visit for UserMessageVisitor {
            fn record_debug(&mut self, _field: &Field, _value: &dyn fmt::Debug) {}

            fn record_str(&mut self, field: &Field, value: &str) {
                if field.name() == "user_message" {
                    self.0 = Some(value.to_string());
                }
            }
        }

        let mut visitor = UserMessageVisitor::default();
        attrs.record(&mut visitor);

        let mut ext = span.extensions_mut();

        if let Some(msg) = visitor.0 {
            ext.insert(SpanContext {
                msg,
                has_error: false,
            });
        }

        if ext.get_mut::<SpanTimings>().is_none() {
            ext.insert(SpanTimings::new());
        }
    }

    fn on_enter(&self, id: &span::Id, ctx: layer::Context<'_, S>) {
        let span = ctx
            .span(id)
            .expect("Span not found in context, this is a bug");
        let mut extensions = span.extensions_mut();
        if let Some(timings) = extensions.get_mut::<SpanTimings>() {
            timings.enter();
        }
    }

    fn on_exit(&self, id: &span::Id, ctx: layer::Context<'_, S>) {
        let span = ctx
            .span(id)
            .expect("Span not found in context, this is a bug");
        let mut extensions = span.extensions_mut();
        if let Some(timings) = extensions.get_mut::<SpanTimings>() {
            timings.exit();
        }
    }

    fn on_close(&self, id: span::Id, ctx: layer::Context<'_, S>) {
        let span = ctx
            .span(&id)
            .expect("Span not found in context, this is a bug");
        let mut extensions = span.extensions_mut();

        if let Some(timings) = extensions.get_mut::<SpanTimings>() {
            timings.enter();
        }

        if let Some(span_ctx) = extensions.get_mut::<SpanContext>() {
            if self.has_error.load(Ordering::SeqCst) {
                span_ctx.has_error = true;
            }
        }
    }

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

#[derive(Debug)]
enum Progress {
    New,
    Close,
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
        #[derive(Debug)]
        enum LogEntry {
            Message(String),
            LogProgress {
                progress: Progress,
                idle_time: String,
                busy_time: String,
            },
        }

        #[derive(Default)]
        struct EventVisitor {
            message: Option<String>,
            idle_time: Option<String>,
            busy_time: Option<String>,
        }

        impl EventVisitor {
            fn finalize(mut self) -> Option<LogEntry> {
                if let Some(message) = self.message.take() {
                    match message.as_str() {
                        "new" | "close" => {
                            return Some(LogEntry::LogProgress {
                                progress: match message.as_str() {
                                    "new" => Progress::New,
                                    "close" => Progress::Close,
                                    _ => unreachable!(),
                                },
                                idle_time: self.idle_time.unwrap_or_default(),
                                busy_time: self.busy_time.unwrap_or_default(),
                            })
                        }
                        _ => return Some(LogEntry::Message(message)),
                    }
                }

                None
            }
        }

        impl Visit for EventVisitor {
            fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
                match field.name() {
                    "time.idle" => self.idle_time = Some(format!("{:?}", value)),
                    "time.busy" => self.busy_time = Some(format!("{:?}", value)),
                    "message" => self.message = Some(format!("{:?}", value)),
                    _ => {}
                }
            }

            fn record_str(&mut self, field: &Field, value: &str) {
                if field.name() == "message" {
                    self.message = Some(value.to_string());
                }
            }
        }

        let mut visitor = EventVisitor::default();
        event.record(&mut visitor);

        if let Some(log_entry) = visitor.finalize() {
            let meta = event.metadata();
            let ansi = writer.has_ansi_escapes();

            match log_entry {
                LogEntry::Message(message) => {
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

                    writeln!(writer, "{}", message)?;
                }
                LogEntry::LogProgress {
                    progress,
                    idle_time,
                    busy_time,
                } => {
                    let mut span_message: Option<String> = None;
                    let mut span_timings: Option<SpanTimings> = None;
                    let mut has_error = false;

                    for span in ctx
                        .event_scope()
                        .into_iter()
                        .flat_map(tracing_subscriber::registry::Scope::from_root)
                    {
                        let ext = span.extensions();
                        if let Some(timings) = ext.get::<SpanTimings>() {
                            span_timings = Some(timings.clone());
                        }
                        if let Some(span_ctx) = ext.get::<SpanContext>() {
                            span_message = Some(span_ctx.msg.clone());
                            has_error = span_ctx.has_error;
                        }
                    }

                    match progress {
                        Progress::New => {
                            let prefix = style("•").blue();
                            writeln!(
                                writer,
                                "{} {} ...",
                                prefix,
                                span_message.unwrap_or_default()
                            )?;
                        }
                        Progress::Close => {
                            let prefix = if has_error {
                                style("✖").red()
                            } else {
                                style("✔").green()
                            };
                            writeln!(
                                writer,
                                "{} {} in {}",
                                prefix,
                                span_message.unwrap_or_default(),
                                span_timings
                                    .map(|t| t.total_duration())
                                    .unwrap_or(HumanReadableDuration(Duration::ZERO))
                            )?;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
