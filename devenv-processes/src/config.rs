//! Configuration types for native process manager.
//!
//! All process-related types with serde support for Nix/JSON deserialization.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Process type determining lifecycle behavior
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessType {
    /// Standard foreground process (default)
    /// Ready immediately after start, can restart based on RestartPolicy
    #[default]
    Foreground,
}

/// Process restart policy
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestartPolicy {
    /// Never restart the process
    Never,
    /// Always restart the process when it exits
    Always,
    /// Restart only on failure (non-zero exit code)
    #[default]
    OnFailure,
}

/// Type of listen socket
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListenKind {
    Tcp,
    UnixStream,
}

/// Specification for a listen socket
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListenSpec {
    pub name: String,
    pub kind: ListenKind,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub backlog: Option<i32>,
    #[serde(default)]
    pub mode: Option<u32>,
}

/// Socket activation configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SocketActivationConfig {
    #[serde(default)]
    pub listens: Vec<ListenSpec>,
}

/// Watch configuration for file-based process restarts
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WatchConfig {
    /// Paths to watch for changes (files or directories)
    #[serde(default)]
    pub paths: Vec<PathBuf>,
    /// File extensions to watch (e.g., "rs", "js", "py"). If empty, all extensions are watched.
    #[serde(default)]
    pub extensions: Vec<String>,
    /// Glob patterns to ignore (e.g., ".git", "target", "*.log")
    #[serde(default)]
    pub ignore: Vec<String>,
}

/// Watchdog configuration for health monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchdogConfig {
    /// Watchdog interval in microseconds
    pub usec: u64,
    /// Require READY=1 notification before enforcing watchdog (default: true)
    #[serde(default = "default_true")]
    pub require_ready: bool,
}

fn default_true() -> bool {
    true
}

/// Notify socket configuration for systemd-style notifications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyConfig {
    /// Enable systemd notify protocol
    #[serde(default = "default_true")]
    pub enable: bool,
}

/// Linux capability sets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySet {
    Ambient,
    Permitted,
    Effective,
    Inheritable,
    Bounding,
}

/// Linux capabilities configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LinuxCapabilities {
    /// Capabilities to add (e.g., "net_bind_service", "sys_ptrace")
    #[serde(default)]
    pub add: Vec<String>,
    /// Which capability sets to apply (default: ambient)
    #[serde(default)]
    pub sets: Vec<CapabilitySet>,
}

/// Linux-specific process configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LinuxConfig {
    /// Linux capabilities configuration
    #[serde(default)]
    pub capabilities: LinuxCapabilities,
}

/// Process configuration
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ProcessConfig {
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "type")]
    pub process_type: ProcessType,
    #[serde(default)]
    pub exec: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub use_sudo: bool,
    #[serde(default)]
    pub pseudo_terminal: bool,
    #[serde(default)]
    pub listen: Vec<ListenSpec>,
    #[serde(default)]
    pub restart: RestartPolicy,
    #[serde(default)]
    pub max_restarts: Option<usize>,
    /// Watch configuration for file-triggered restarts
    #[serde(default)]
    pub watch: WatchConfig,
    /// Watchdog configuration for health monitoring
    #[serde(default)]
    pub watchdog: Option<WatchdogConfig>,
    /// Notify socket configuration
    #[serde(default)]
    pub notify: Option<NotifyConfig>,
    /// Linux-specific configuration
    #[serde(default)]
    pub linux: LinuxConfig,
}
