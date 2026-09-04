//! Typed messages for devenv's internal component mailboxes.
//!
//! This crate defines unstable, in-process plumbing between component owners.
//! Its types are not a serialized or externally compatible protocol. The
//! crate deliberately depends only on values that can cross the frontend and
//! backend ownership boundary, keeping messages independent of their senders
//! and receivers.

use portable_pty::CommandBuilder;
use std::path::PathBuf;

/// Commands that can be sent to control managed processes.
#[derive(Debug, Clone)]
pub enum ProcessCommand {
    /// Restart a running process, or start a stopped process.
    Restart(String),
    /// Stop a running process but keep it visible and restartable.
    Stop(String),
    /// Tear down the whole process manager and shut the daemon down.
    StopManager,
}

/// Commands sent from the backend to the terminal frontend.
#[derive(Debug)]
pub enum FrontendCommand {
    /// Stop the renderer and transition the frontend to shell ownership.
    ExitRenderer,
    /// Record whether the frontend is attached to an existing session.
    SetAttached(bool),
    /// Temporarily release terminal ownership for an interactive backend action.
    PauseForInteraction {
        /// Sent after the renderer has restored cooked terminal mode.
        ready: std::sync::mpsc::SyncSender<()>,
        /// The renderer resumes after this receiver observes completion.
        resume: std::sync::mpsc::Receiver<()>,
    },
    /// Command for the shell session after the frontend takes terminal ownership.
    Shell(ShellCommand),
}

/// Events sent from the terminal frontend to the backend.
#[derive(Debug)]
pub enum FrontendEvent {
    /// Command for the native process manager.
    Process(ProcessCommand),
    /// Event emitted by the shell session.
    Shell(ShellEvent),
}

/// Commands sent from coordinator to shell session.
#[derive(Debug)]
pub enum ShellCommand {
    /// Spawn the initial shell with this command.
    Spawn {
        command: CommandBuilder,
        watch_files: Vec<PathBuf>,
    },
    /// Update the list of watched files after the initial build.
    WatchedFiles { files: Vec<PathBuf> },
    /// File changed and a build started.
    Building { changed_files: Vec<PathBuf> },
    /// Environment rebuild completed successfully.
    ReloadReady { changed_files: Vec<PathBuf> },
    /// Build failed; retain the current shell.
    BuildFailed {
        changed_files: Vec<PathBuf>,
        error: String,
    },
    /// Reload was applied at the prompt.
    ReloadApplied,
    /// File watching was paused or resumed.
    WatchingPaused { paused: bool },
    /// Print the watched files.
    PrintWatchedFiles { files: Vec<PathBuf> },
    /// The coordinator is shutting down.
    Shutdown,
}

/// Events sent from shell session to coordinator.
#[derive(Debug)]
pub enum ShellEvent {
    /// The shell process exited.
    Exited { exit_code: Option<u32> },
    /// The terminal was resized.
    Resize { cols: u16, rows: u16 },
    /// User pressed Ctrl-Alt-D to toggle file watching.
    TogglePause,
    /// User pressed Ctrl-Alt-W to list watched files.
    ListWatchedFiles,
}
