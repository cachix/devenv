//! Shell and PTY management for devenv.
//!
//! This crate provides shell session management with hot-reload support,
//! including PTY spawning, terminal handling, status line rendering,
//! and task execution within the shell environment.

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

// Task execution
pub use task_runner::{PtyTaskRunner, TaskRunnerError, strip_ansi_codes};

// Main session
pub use session::{SessionConfig, SessionError, ShellSession, TuiHandoff};

// Re-export for convenience
pub use portable_pty::{CommandBuilder, PtySize};
