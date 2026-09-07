//! Process management for devenv.
//!
//! The crate separates three concerns:
//!
//! - A **manager** supervises configured processes. The native manager does so
//!   directly; an external manager is launched from a script built by Nix.
//! - [`ManagerCapabilities`] describes which CLI operations devenv may use with
//!   the selected manager. Current Nix modules declare these capabilities, with
//!   embedded compatibility data for older modules.
//! - [`ProcessScope`] identifies the operating-system process set used for
//!   liveness and final cleanup. It is independent of manager implementation.
//!
//! Running managers implement the minimum [`ProcessManagerControl`] boundary.
//! Sharing that trait does not imply support for every presentation or control
//! operation; callers must validate the selected manager's capabilities first.

use async_trait::async_trait;
use miette::Result;
use std::path::{Path, PathBuf};

/// Subdirectory name for process manager state
pub const PROCESSES_DIR: &str = "processes";

/// Socket filename for the native process manager API.
pub const NATIVE_SOCKET_NAME: &str = "native.sock";

/// Compute the full path to the native process manager API socket for a given dotfile path.
pub fn native_socket_path(devenv_dotfile: &Path) -> PathBuf {
    devenv_core::paths::resolve_runtime_dir(devenv_dotfile)
        .join(PROCESSES_DIR)
        .join(NATIVE_SOCKET_NAME)
}

/// Get the runtime directory for processes given a base runtime directory.
/// Creates the directory if it doesn't exist.
pub fn get_process_runtime_dir(runtime_dir: &Path) -> Result<PathBuf> {
    let dir = runtime_dir.join(PROCESSES_DIR);
    std::fs::create_dir_all(&dir)
        .map_err(|e| miette::miette!("Failed to create processes runtime directory: {}", e))?;
    Ok(dir)
}

pub mod capabilities;
pub mod command;
pub mod config;
pub mod external_manager;
pub mod force_exit_registry;
pub mod log_tailer;
pub mod manager;
pub mod manager_capabilities;
pub mod pid;
mod process_guardian;
pub mod process_scope;
pub mod pty;
pub mod socket_activation;
pub mod supervisor;
pub mod supervisor_state;

// Re-export config types at crate root
#[cfg(target_os = "linux")]
pub use capabilities::start_capability_daemon;
pub use capabilities::{CapabilityRequest, maybe_run_capability_helper, start_capability_broker};
pub use config::{
    HttpGetProbe, HttpProbe, ListenKind, ListenSpec, ProcessConfig, ProcessProxyConfig,
    ProcessProxyHttpsConfig, ProcessType, ReadyConfig, RestartConfig, RestartPolicy,
    ShutdownConfig, SocketActivationConfig, SupervisionMode, WatchConfig, WatchdogConfig,
};
pub use devenv_event_sources::{NotifyMessage, NotifySocket};
pub use devenv_mailbox::ProcessCommand;
pub use external_manager::ExternalManager;
pub use force_exit_registry::{kill_process_scopes, track_process_scope, tracked_process_scopes};
pub use manager::{
    ApiRequest, ApiResponse, AttachEvent, AttachStream, JobHandle, LogStream, ManagerResidence,
    NativeManagerClient, OnIdle, PortInfo, ProcessInfo, ProcessPhase, ProcessResources,
    ProcessRunner, ProcessState, StartOutcome,
};
pub use manager_capabilities::{
    DeclarationSource, ManagerAdapter, ManagerCapabilities, ManagerClient, ManagerDescriptor,
    ManagerOperation, ManagerStopMethod, ManagerTerminal, fallback_adapter, fallback_capabilities,
};
pub use pid::{PidStatus, check_pid_file, read_pid, remove_pid, write_pid};
pub use process_guardian::maybe_run_process_guardian;
pub use process_scope::{PreparedProcessScope, ProcessScope, StopPolicy, stop_process_scopes};
pub use pty::PtyProcess;
pub use socket_activation::{
    ActivatedSockets, ActivationSpec, ActivationSpecBuilder, SD_LISTEN_FDS_START,
    SocketActivationWrapper, activation_from_listen,
};
pub use supervisor_state::{ExitStatus, JobStatus, SupervisorPhase};

/// Request to start an external manager in the background.
#[derive(Debug, Clone, Default)]
pub struct BackgroundStartRequest {
    /// Specific processes to start (empty = all)
    pub processes: Vec<String>,
    /// Log output to file instead of inheriting the terminal streams.
    pub log_to_file: bool,
    /// Environment variables to pass to processes
    pub env: std::collections::HashMap<String, String>,
}

/// Runtime control shared by already-started native and external managers.
///
/// Startup deliberately is not part of this trait: the native manager is
/// assembled from the task scheduler, while an external manager is launched
/// from a Nix-built adapter. [`ManagerCapabilities`] describes which optional
/// operations those adapters support.
#[async_trait]
pub trait ProcessManagerControl: Send + Sync {
    /// Stop all running processes
    async fn stop(&self) -> Result<()>;

    /// Check if the process manager is currently running
    async fn is_running(&self) -> bool;
}
