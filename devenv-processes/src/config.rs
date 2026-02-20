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

/// Readiness probe configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyConfig {
    /// Shell command to execute. Exit 0 = ready.
    #[serde(default)]
    pub exec: Option<String>,
    /// HTTP probe configuration
    #[serde(default)]
    pub http: Option<HttpProbe>,
    /// Enable systemd notify protocol for readiness signaling
    #[serde(default)]
    pub notify: bool,
    /// Seconds to wait before first probe
    #[serde(default)]
    pub initial_delay: u64,
    /// Seconds between probes
    #[serde(default = "default_period")]
    pub period: u64,
    /// Seconds before a single probe times out
    #[serde(default = "default_probe_timeout")]
    pub probe_timeout: u64,
    /// Overall deadline in seconds for the process to become ready. None = no deadline.
    #[serde(default)]
    pub timeout: Option<u64>,
    /// Consecutive successes needed to be considered ready
    #[serde(default = "default_success")]
    pub success_threshold: u32,
    /// Consecutive failures before marking unhealthy
    #[serde(default = "default_failure")]
    pub failure_threshold: u32,
}

impl Default for ReadyConfig {
    fn default() -> Self {
        Self {
            exec: None,
            http: None,
            notify: false,
            initial_delay: 0,
            period: default_period(),
            probe_timeout: default_probe_timeout(),
            timeout: None,
            success_threshold: default_success(),
            failure_threshold: default_failure(),
        }
    }
}

/// HTTP probe configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpProbe {
    pub get: Option<HttpGetProbe>,
}

/// HTTP GET probe parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpGetProbe {
    pub host: String,
    pub port: u16,
    pub path: String,
    pub scheme: String,
}

/// Structured restart configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestartConfig {
    /// When to restart
    #[serde(default)]
    pub on: RestartPolicy,
    /// Maximum restart attempts. None = unlimited.
    #[serde(default)]
    pub max: Option<usize>,
    /// Sliding window in seconds for restart rate limiting. None = lifetime limit.
    #[serde(default)]
    pub window: Option<u64>,
}

impl Default for RestartConfig {
    fn default() -> Self {
        Self {
            on: RestartPolicy::OnFailure,
            max: Some(5),
            window: None,
        }
    }
}

fn default_period() -> u64 {
    10
}

fn default_probe_timeout() -> u64 {
    1
}

fn default_success() -> u32 {
    1
}

fn default_failure() -> u32 {
    3
}

/// Linux-specific process configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LinuxConfig {
    /// Linux capabilities to add as ambient (e.g., "net_bind_service", "sys_ptrace")
    #[serde(default)]
    pub capabilities: Vec<String>,
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
    pub listen: Vec<ListenSpec>,
    /// Allocated ports for display (e.g., {"http": 8080, "admin": 9000})
    #[serde(default)]
    pub ports: HashMap<String, u16>,
    /// Readiness probe configuration
    #[serde(default)]
    pub ready: Option<ReadyConfig>,
    #[serde(default)]
    pub restart: RestartConfig,
    /// Watch configuration for file-triggered restarts
    #[serde(default)]
    pub watch: WatchConfig,
    /// Watchdog configuration for health monitoring
    #[serde(default)]
    pub watchdog: Option<WatchdogConfig>,
    /// Linux-specific configuration
    #[serde(default)]
    pub linux: LinuxConfig,
    /// Whether to run this process under sudo
    #[serde(default)]
    pub use_sudo: bool,
}
