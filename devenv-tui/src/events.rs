use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Unique identifier for operations
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OperationId(pub String);

impl OperationId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for OperationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Events that can be sent to the TUI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TuiEvent {
    /// An operation has started
    OperationStart {
        id: OperationId,
        message: String,
        parent: Option<OperationId>,
        data: HashMap<String, String>,
    },
    /// An operation has ended
    OperationEnd {
        id: OperationId,
        result: OperationResult,
    },
    /// A log message
    LogMessage {
        level: LogLevel,
        message: String,
        source: LogSource,
        data: HashMap<String, String>,
    },
    /// Nix build started
    NixBuildStart {
        operation_id: OperationId,
        derivation: String,
    },
    /// Nix build progress update
    NixBuildProgress {
        operation_id: OperationId,
        phase: String,
    },
    /// Nix build ended
    NixBuildEnd {
        operation_id: OperationId,
        success: bool,
    },
}

/// Result of an operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationResult {
    Success,
    Failure {
        message: String,
        code: Option<i32>,
        output: Option<String>,
    },
}

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

/// Source of log messages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogSource {
    User,    // Regular user-facing messages
    Tracing, // From tracing framework
    Nix,     // From Nix build logs
    System,  // System messages
}

/// State of an operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationState {
    Active,
    Complete { duration: Duration, success: bool },
}

/// Information about an operation
#[derive(Debug, Clone)]
pub struct Operation {
    pub id: OperationId,
    pub message: String,
    pub state: OperationState,
    pub start_time: Instant,
    pub children: Vec<OperationId>,
    pub parent: Option<OperationId>,
    pub data: HashMap<String, String>,
}

impl Operation {
    pub fn new(
        id: OperationId,
        message: String,
        parent: Option<OperationId>,
        data: HashMap<String, String>,
    ) -> Self {
        Self {
            id,
            message,
            state: OperationState::Active,
            start_time: Instant::now(),
            children: Vec::new(),
            parent,
            data,
        }
    }

    pub fn complete(&mut self, success: bool) {
        let duration = self.start_time.elapsed();
        self.state = OperationState::Complete { duration, success };
    }
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

/// Information about a Nix build
#[derive(Debug, Clone)]
pub struct NixBuildInfo {
    pub operation_id: OperationId,
    pub derivation: String,
    pub current_phase: Option<String>,
    pub start_time: Instant,
}
