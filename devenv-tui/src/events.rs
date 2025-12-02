use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

/// Log levels matching tracing levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<tracing::Level> for LogLevel {
    fn from(level: tracing::Level) -> Self {
        match level {
            tracing::Level::ERROR => LogLevel::Error,
            tracing::Level::WARN => LogLevel::Warn,
            tracing::Level::INFO => LogLevel::Info,
            tracing::Level::DEBUG => LogLevel::Debug,
            tracing::Level::TRACE => LogLevel::Trace,
        }
    }
}

impl From<devenv_activity::LogLevel> for LogLevel {
    fn from(level: devenv_activity::LogLevel) -> Self {
        match level {
            devenv_activity::LogLevel::Error => LogLevel::Error,
            devenv_activity::LogLevel::Warn => LogLevel::Warn,
            devenv_activity::LogLevel::Info => LogLevel::Info,
            devenv_activity::LogLevel::Debug => LogLevel::Debug,
            devenv_activity::LogLevel::Trace => LogLevel::Trace,
        }
    }
}

/// Source of log messages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogSource {
    User,
    Tracing,
    Nix,
    System,
}

/// State of a Nix activity
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NixActivityState {
    Active,
    Completed {
        success: bool,
        duration: std::time::Duration,
    },
}

/// A log message with context
#[derive(Debug, Clone)]
pub struct LogMessage {
    pub level: LogLevel,
    pub message: String,
    pub source: LogSource,
    pub timestamp: Instant,
    pub data: HashMap<String, String>,
}

impl LogMessage {
    pub fn new(
        level: LogLevel,
        message: String,
        source: LogSource,
        data: HashMap<String, String>,
    ) -> Self {
        Self {
            level,
            message,
            source,
            timestamp: Instant::now(),
            data,
        }
    }
}
