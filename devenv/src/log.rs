use console::style;
use std::fmt;
use std::io::Write;
use std::time::Instant;
use tracing::level_filters::LevelFilter;
use tracing::{Event, Subscriber};
use tracing_subscriber::{
    fmt::{format::Writer, FmtContext, FormatEvent, FormatFields},
    registry::LookupSpan,
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

impl Into<LevelFilter> for Level {
    fn into(self) -> LevelFilter {
        match self {
            Level::Silent => LevelFilter::OFF,
            Level::Error => LevelFilter::ERROR,
            Level::Warn => LevelFilter::WARN,
            Level::Info => LevelFilter::INFO,
            Level::Debug => LevelFilter::DEBUG,
        }
    }
}

pub struct DevenvFormat {
    pub verbose: bool,
}

impl<S, N> FormatEvent<S, N> for DevenvFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();
        let ansi = writer.has_ansi_escapes();

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

        ctx.field_format().format_fields(writer.by_ref(), event)?;

        writeln!(writer)
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
        eprint!("{} {} ...", prefix, message);
        if newline {
            eprintln!();
        }
        std::io::stderr().flush().unwrap();
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
        let prefix = if self.failed {
            style("✖").red()
        } else {
            style("✔").green()
        };
        eprintln!(
            "\r{} {} in {:.1}s.",
            prefix,
            self.message,
            duration.as_secs_f32()
        );
    }
}
