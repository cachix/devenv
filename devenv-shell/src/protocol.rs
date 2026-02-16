//! Communication protocol types for shell coordination.
//!
//! These types define the interface between the shell coordinator (which handles
//! file watching and build triggering) and the shell session (which manages the PTY).

use portable_pty::CommandBuilder;
use std::collections::BTreeMap;
use std::path::PathBuf;
use tokio::sync::oneshot;

/// Commands sent from coordinator to shell session.
#[derive(Debug)]
pub enum ShellCommand {
    /// Spawn the initial shell with this command.
    Spawn {
        command: CommandBuilder,
        watch_files: Vec<PathBuf>,
    },
    /// Update the list of watched files (sent after initial build populates the watcher).
    WatchedFiles { files: Vec<PathBuf> },
    /// File changed, build started. Show "Building..." status.
    Building { changed_files: Vec<PathBuf> },
    /// Environment rebuild completed successfully.
    /// The new environment has been written to the reload file
    /// and will be picked up by the shell's PROMPT_COMMAND hook.
    ReloadReady { changed_files: Vec<PathBuf> },
    /// Build failed, keep current shell.
    BuildFailed {
        changed_files: Vec<PathBuf>,
        error: String,
    },
    /// User applied the reload (pressed keybind). Clear status line.
    ReloadApplied,
    /// File watching paused/resumed.
    WatchingPaused { paused: bool },
    /// Print list of watched files.
    PrintWatchedFiles { files: Vec<PathBuf> },
    /// Coordinator is shutting down.
    Shutdown,
}

/// Events sent from shell session to coordinator.
#[derive(Debug)]
pub enum ShellEvent {
    /// Shell process exited.
    Exited { exit_code: Option<u32> },
    /// Terminal was resized.
    Resize { cols: u16, rows: u16 },
    /// User pressed Ctrl-Alt-D to toggle file watching.
    TogglePause,
    /// User pressed Ctrl-Alt-W to list watched files.
    ListWatchedFiles,
}

/// Request to execute a task command in the PTY.
///
/// Used by PtyExecutor to run tasks inside the shell environment.
pub struct PtyTaskRequest {
    /// Unique ID for this task execution.
    pub id: u64,
    /// The command to execute (path to script).
    pub command: String,
    /// Environment variables to set before execution.
    pub env: BTreeMap<String, String>,
    /// Working directory (optional).
    pub cwd: Option<String>,
    /// Channel to send result back.
    pub response_tx: oneshot::Sender<PtyTaskResult>,
}

/// Result of executing a task in the PTY.
#[derive(Debug, Clone)]
pub struct PtyTaskResult {
    /// Whether the task succeeded (exit code 0).
    pub success: bool,
    /// Captured stdout lines with timestamps.
    pub stdout_lines: Vec<(std::time::Instant, String)>,
    /// Captured stderr lines with timestamps.
    pub stderr_lines: Vec<(std::time::Instant, String)>,
    /// Error message if the task failed.
    pub error: Option<String>,
}
