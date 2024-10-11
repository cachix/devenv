use console::style;
use std::io::Write;
use std::time::Instant;

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

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone)]
pub enum Level {
    Silent,
    Error,
    Warn,
    Info,
    Debug,
}

#[derive(Clone)]
pub struct Logger {
    pub level: Level,
}

impl Logger {
    pub fn new(level: Level) -> Logger {
        Logger { level }
    }

    pub fn info(&self, message: &str) {
        self.log(message, Level::Info);
    }

    pub fn error(&self, message: &str) {
        self.log(message, Level::Error);
    }

    pub fn debug(&self, message: &str) {
        self.log(message, Level::Debug);
    }

    pub fn warn(&self, message: &str) {
        self.log(message, Level::Warn);
    }

    pub fn log(&self, message: &str, level: Level) {
        if level > self.level {
            return;
        }
        match level {
            Level::Info => {
                let prefix = style("•").blue();
                eprintln!("{} {}", prefix, message);
            }
            Level::Error => {
                let prefix = style("✖").red();
                eprintln!("{} {}", prefix, message);
            }
            Level::Warn => {
                let prefix = style("•").yellow();
                eprintln!("{} {}", prefix, message);
            }
            Level::Debug => {
                let prefix = style("•").italic();
                eprintln!("{} {}", prefix, message);
            }
            Level::Silent => {}
        }
    }
}
