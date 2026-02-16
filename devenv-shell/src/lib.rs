//! Shell and PTY management for devenv.
//!
//! This crate provides shell session management with hot-reload support,
//! including PTY spawning, terminal handling, status line rendering,
//! and task execution within the shell environment.

pub mod dialect;
mod protocol;
mod pty;
mod session;
mod status_line;
mod task_runner;
mod terminal;

// Protocol types
pub use protocol::{PtyTaskRequest, PtyTaskResult, ShellCommand, ShellEvent};

// PTY management
pub use pty::{Pty, PtyError, get_terminal_size};

// Terminal utilities
pub use terminal::{RawModeGuard, is_tty};

// Status line
pub use status_line::{StatusLine, StatusState};

// Shared UI constants (used by devenv-tui)
pub use status_line::{
    CHECKMARK, COLOR_ACTIVE, COLOR_ACTIVE_NESTED, COLOR_COMPLETED, COLOR_FAILED, COLOR_HIERARCHY,
    COLOR_INFO, COLOR_INTERACTIVE, COLOR_SECONDARY, SPINNER_FRAMES, SPINNER_INTERVAL_MS, XMARK,
};

// Task execution
pub use task_runner::{PtyTaskRunner, TaskRunnerError};

// Main session
pub use session::{SessionConfig, SessionError, SessionIo, ShellSession, TuiHandoff};

// Re-export for convenience
pub use portable_pty::{CommandBuilder, PtySize};
