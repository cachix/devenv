//! Process management for devenv
//!
//! This crate provides process management with two backends:
//! - Native: Using watchexec-supervisor with socket activation, PTY support, restart policies
//! - ProcessCompose: Using the external process-compose tool
//!
//! Both backends implement the `ProcessManager` trait for a unified interface.

use async_trait::async_trait;
use miette::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Subdirectory name for process manager state
pub const PROCESSES_DIR: &str = "processes";

/// Get the runtime directory for processes given a base runtime directory.
/// Creates the directory if it doesn't exist.
pub fn get_process_runtime_dir(runtime_dir: &Path) -> Result<PathBuf> {
    let dir = runtime_dir.join(PROCESSES_DIR);
    std::fs::create_dir_all(&dir)
        .map_err(|e| miette::miette!("Failed to create processes runtime directory: {}", e))?;
    Ok(dir)
}

pub mod command;
pub mod config;
pub mod log_tailer;
pub mod manager;
pub mod pid;
pub mod process_compose;
pub mod pty;
pub mod socket_activation;
pub mod supervisor;
pub mod supervisor_state;

// Re-export config types at crate root
pub use config::{
    HttpGetProbe, HttpProbe, ListenKind, ListenSpec, ProcessConfig, ProcessType, ReadyConfig,
    RestartConfig, RestartPolicy, SocketActivationConfig, WatchConfig, WatchdogConfig,
};
pub use devenv_event_sources::{NotifyMessage, NotifySocket};
pub use manager::{
    ApiRequest, ApiResponse, JobHandle, NativeProcessManager, ProcessCommand, ProcessResources,
    ProcessState,
};
pub use pid::{PidStatus, check_pid_file, read_pid, remove_pid, write_pid};
pub use process_compose::ProcessComposeManager;
pub use pty::PtyProcess;
pub use socket_activation::{
    ActivatedSockets, ActivationSpec, ActivationSpecBuilder, SD_LISTEN_FDS_START,
    SocketActivationWrapper, activation_from_listen,
};
pub use supervisor_state::{JobStatus, SupervisorPhase};

/// Options for starting processes
#[derive(Debug, Clone, Default)]
pub struct StartOptions {
    /// Process configurations to start
    pub process_configs: HashMap<String, ProcessConfig>,
    /// Specific processes to start (empty = all)
    pub processes: Vec<String>,
    /// Run in background (detached from terminal)
    pub detach: bool,
    /// Log output to file instead of terminal (only for detached mode)
    pub log_to_file: bool,
    /// Environment variables to pass to processes
    pub env: HashMap<String, String>,
    /// Cancellation token for graceful shutdown coordination
    pub cancellation_token: Option<tokio_util::sync::CancellationToken>,
    /// Path to the devenv-cap-server binary (Linux capability granting via sudo)
    pub cap_server_binary: Option<std::path::PathBuf>,
}

/// Common interface for process managers
#[async_trait]
pub trait ProcessManager: Send + Sync {
    /// Start processes with the given options
    async fn start(&self, options: StartOptions) -> Result<()>;

    /// Stop all running processes
    async fn stop(&self) -> Result<()>;

    /// Check if the process manager is currently running
    async fn is_running(&self) -> bool;
}
