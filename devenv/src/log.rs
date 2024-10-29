use console::style;
use schematic::color::owo::OwoColorize;
use std::fmt;
use std::marker::PhantomData;
use std::time::Instant;
use tracing::level_filters::LevelFilter;
use tracing::{info, Event};
use tracing_subscriber::fmt::format::DefaultFields;
use tracing_subscriber::layer::{self};
use tracing_subscriber::{
    field::RecordFields,
    fmt::{format::Writer, FmtContext, FormatEvent, FormatFields, FormattedFields},
    registry::LookupSpan,
};

use core::time::Duration;
use tracing_core::{span, Subscriber};

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

pub struct DevenvLayer<S, L, F = DefaultFields>
where
    L: layer::Layer<S> + Sized,
    S: Subscriber,
{
    pub verbose: bool,
    formatter: F,
    inner: L,
    _subscriber: PhantomData<S>,
}

impl<S, L> DevenvLayer<S, L>
where
    L: layer::Layer<S> + Sized,
    S: Subscriber,
{
    pub fn new(inner: L) -> Self {
        Self {
            verbose: false,
            inner,
            formatter: DefaultFields::new(),
            _subscriber: PhantomData,
        }
    }
}

#[derive(Debug)]
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

    fn idle_duration(&self) -> HumanReadableDuration {
        HumanReadableDuration(self.idle)
    }

    fn busy_duration(&self) -> HumanReadableDuration {
        HumanReadableDuration(self.busy)
    }
}

struct HumanReadableDuration(Duration);

impl fmt::Display for HumanReadableDuration {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let secs = self.0.as_secs();
        let millis = self.0.subsec_millis();
        write!(f, "{}.{:03}s", secs, millis)
    }
}

struct SpanMessage(String);

impl<S, L, F> layer::Layer<S> for DevenvLayer<S, L, F>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    F: for<'writer> FormatFields<'writer> + 'static,
    L: layer::Layer<S>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: layer::Context<'_, S>) {
        let span = ctx
            .span(id)
            .expect("Span not found in context, this is a bug");

        let mut visitor = MessageVisitor(None);
        attrs.record(&mut visitor);

        let mut ext = span.extensions_mut();

        if let Some(msg) = visitor.0 {
            ext.insert(SpanMessage(msg));
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
}

pub struct DevenvFormat {
    pub verbose: bool,
}

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
                // eprintln!("record_debug event: {} {:?}", field.name(), value);
                // if field.name() == "message" {
                //     match format!("{:?}", value).as_str() {
                //         "\"new\"" | "\"exit\"" | "\"close\"" => {}
                //         _ => {}
                //     }
                // }
                match field.name() {
                    "time.idle" => self.idle_time = Some(format!("{:?}", value)),
                    "time.busy" => self.busy_time = Some(format!("{:?}", value)),
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

        let log_entry = visitor.finalize();

        if let Some(log_entry) = log_entry {
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
                    let mut user_message: Option<String> = None;
                    for span in ctx
                        .event_scope()
                        .into_iter()
                        .flat_map(tracing_subscriber::registry::Scope::from_root)
                    {
                        let ext = span.extensions();

                        if let Some(timings) = ext.get::<SpanTimings>() {
                            eprintln!("Timings: {:?}", timings);
                        }
                        if let Some(msg) = ext.get::<SpanMessage>() {
                            user_message = Some(msg.0.clone());
                        }
                        // if let Some(fields) = &ext.get::<FormattedFields<F>>() {
                        //     // eprintln!("Fields: {}", fields);
                        //     // Skip formatting the fields if the span had no fields.
                        //     if !fields.is_empty() {
                        //         writeln!(writer, "{}", fields)?;
                        //     }
                        // }
                    }

                    match progress {
                        Progress::New => {
                            let prefix = style("•").blue();
                            writeln!(
                                writer,
                                "{} {} ...",
                                prefix,
                                user_message.unwrap_or_default()
                            )?;
                        }
                        Progress::Close => {
                            writeln!(
                                writer,
                                "{} {} in {}",
                                style("✔").green(),
                                user_message.unwrap_or_default(),
                                idle_time,
                                // busy_time
                            )?;
                        }
                    }
                }
            }
        }

        // ctx.field_format().format_fields(writer.by_ref(), event)?;

        // writeln!(writer)
        Ok(())
    }
}

use tracing::field::{Field, Visit};

#[derive(Debug)]
struct MessageVisitor(Option<String>);

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        eprintln!("record_debug span: {} {:?}", field.name(), value);
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        // eprint!("record_str: {}: {}", field.name(), value);
        if field.name() == "user_message" {
            self.0 = Some(value.to_string());
        }
    }
}

pub enum LogProgressCreator {
    Silent,
    Logging,
}

impl LogProgressCreator {
    pub fn with_newline(&self, message: &str) -> Option<LogProgress> {
        use LogProgressCreator::*;
        match self {
            Silent => None,
            Logging => Some(LogProgress::new(message, true)),
        }
    }

    pub fn without_newline(&self, message: &str) -> Option<LogProgress> {
        use LogProgressCreator::*;
        match self {
            Silent => None,
            Logging => Some(LogProgress::new(message, false)),
        }
    }
}

pub struct LogProgress {
    message: String,
    start: Option<Instant>,
    pub failed: bool,
}

impl LogProgress {
    pub fn new(message: &str, newline: bool) -> LogProgress {
        let prefix = style("•").blue();
        info!("{} {} ...", prefix, message);
        // if newline {
        //     eprintln!();
        // }
        LogProgress {
            message: message.to_string(),
            start: Some(Instant::now()),
            failed: false,
        }
    }
}

impl Drop for LogProgress {
    fn drop(&mut self) {
        let duration = self.start.unwrap_or_else(Instant::now).elapsed();
        // let prefix = if self.failed {
        //     style("✖").red()
        // } else {
        //     style("✔").green()
        // };
        let prefix = "";
        info!(
            "{} {} in {:.1}s.", // \r
            prefix,
            self.message,
            duration.as_secs_f32()
        );
    }
}

// REMOVE

pub struct DevenvFieldFormatter {}

impl<'a> FormatFields<'a> for DevenvFieldFormatter {
    fn format_fields<R: RecordFields>(&self, writer: Writer<'_>, fields: R) -> fmt::Result {
        let mut visitor = FieldFormatterVisitor {
            writer,
            result: Ok(()),
        };
        fields.record(&mut visitor);
        visitor.result
    }
}

struct FieldFormatterVisitor<'a> {
    writer: Writer<'a>,
    result: fmt::Result,
}

impl<'a> FieldFormatterVisitor<'a> {
    fn record_display(&mut self, field: &tracing::field::Field, value: &dyn fmt::Display) {
        if self.result.is_err() {
            return;
        }

        write!(self.writer, "{}", value);
    }
}

impl<'a> Visit for FieldFormatterVisitor<'a> {
    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.record_display(field, &value)
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.record_display(field, &value)
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.record_display(field, &format_args!("{:#x}", value))
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.record_display(field, &value)
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            match value.to_string().as_str() {
                "new" | "exit" | "close" => return,
                _ => {}
            }
        }

        self.record_display(field, &format_args!("{}", value));
    }

    fn record_error(
        &mut self,
        field: &tracing::field::Field,
        mut value: &(dyn std::error::Error + 'static),
    ) {
        self.record_debug(field, &format_args!("{}", value));
        while let Some(s) = value.source() {
            value = s;
            if self.result.is_err() {
                return;
            }
            self.result = write!(self.writer, ": {}", value);
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        eprintln!("debug: {} {:?}", field.name(), value);
        if field.name() == "message" {
            match format!("{:?}", value).as_str() {
                "new" | "exit" | "close" => {}
                _ => self.record_display(field, &format_args!("{:?}", value)),
            }
        }
        // self.record_display(field, &format_args!("{:x?}", value))
    }
}
