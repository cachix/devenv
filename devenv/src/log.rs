use ansiterm::Colour::{Blue, DarkGray, Green, Red, Yellow};
use std::io::Write;
use std::time::Instant;

pub struct LogProgress {
    message: String,
    start: Option<Instant>,
    pub failed: bool,
}

impl LogProgress {
    pub fn new(message: &str, newline: bool) -> LogProgress {
        let prefix = Blue.paint("•");
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
            Red.paint("✖")
        } else {
            Green.paint("✔")
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
    Error,
    Warn,
    Info,
    Debug,
}

#[derive(Clone)]
pub struct Logger {
    level: Level,
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

    fn log(&self, message: &str, level: Level) {
        if level > self.level {
            return;
        }
        match level {
            Level::Info => {
                let prefix = Blue.paint("•");
                eprintln!("{} {}", prefix, message);
            }
            Level::Error => {
                let prefix = Red.paint("✖");
                eprintln!("{} {}", prefix, message);
            }
            Level::Warn => {
                let prefix = Yellow.paint("•");
                eprintln!("{} {}", prefix, message);
            }
            Level::Debug => {
                let prefix = DarkGray.paint("•");
                eprintln!("{} {}", prefix, message);
            }
        }
    }
}
