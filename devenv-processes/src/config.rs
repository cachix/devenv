//! Configuration types for native process manager.
//!
//! All process-related types with serde support for Nix/JSON deserialization.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// Process type.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessType {
    /// Standard foreground process.
    #[default]
    Foreground,
}

/// Process restart policy.
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

/// Listen socket type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListenKind {
    Tcp,
    UnixStream,
}

/// Listen socket specification.
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

/// Socket activation configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SocketActivationConfig {
    #[serde(default)]
    pub listens: Vec<ListenSpec>,
}

/// File-watch configuration.
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

/// Watchdog configuration.
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

/// Readiness probe configuration.
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

/// HTTP readiness probe configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpProbe {
    pub get: Option<HttpGetProbe>,
}

/// HTTP GET probe parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpGetProbe {
    pub host: String,
    pub port: u16,
    pub path: String,
    pub scheme: String,
}

/// Restart configuration.
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

fn default_bash() -> String {
    "bash".to_string()
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

/// Linux-specific process configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LinuxConfig {
    /// Linux capabilities to add as ambient (e.g., "net_bind_service", "sys_ptrace")
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// Graceful process shutdown configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShutdownConfig {
    /// Unix signal number sent for graceful shutdown.
    #[serde(default = "default_shutdown_signal")]
    pub signal: i32,
    /// Seconds to wait before escalating to SIGKILL.
    #[serde(default = "default_shutdown_grace")]
    pub grace: u64,
}

fn default_shutdown_signal() -> i32 {
    15
}

fn default_shutdown_grace() -> u64 {
    5
}

impl Default for ShutdownConfig {
    fn default() -> Self {
        Self {
            signal: default_shutdown_signal(),
            grace: default_shutdown_grace(),
        }
    }
}

impl ShutdownConfig {
    pub fn grace_duration(&self) -> Duration {
        Duration::from_secs(self.grace)
    }
}

/// Process auto-start configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StartConfig {
    #[serde(default = "default_true")]
    pub enable: bool,
}

impl Default for StartConfig {
    fn default() -> Self {
        Self { enable: true }
    }
}

/// Opt-in HTTPS configuration for a process's proxy routes.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ProcessProxyHttpsConfig {
    #[serde(default)]
    pub enable: bool,
}

/// Shared HTTP proxy configuration.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ProcessProxyConfig {
    /// Full `.localhost` hostname used as the base hostname for this process.
    #[serde(default)]
    pub hostname: Option<String>,
    /// Full `.localhost` hostname overrides keyed by port name.
    #[serde(default)]
    pub port_hostnames: HashMap<String, String>,
    /// Opt in to HTTPS URLs for this process.
    #[serde(default)]
    pub https: ProcessProxyHttpsConfig,
    /// Resolved proxy URLs, populated by the CLI for process display.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub urls: Vec<String>,
}

/// Who owns automatic restart, readiness, watchdog, and file-watch policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SupervisionMode {
    /// Run local restart, readiness, watchdog, and file-watch policy.
    #[default]
    Native,
    /// Report lifecycle state while the host owns supervision policy.
    External,
}

/// Process configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProcessConfig {
    #[serde(default)]
    pub name: String,
    /// Path to the bash binary to use for exec probes
    #[serde(default = "default_bash")]
    pub bash: String,
    #[serde(default)]
    pub start: StartConfig,
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
    /// Shared HTTP proxy configuration.
    #[serde(default)]
    pub proxy: ProcessProxyConfig,
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
    /// Signal and grace period used when stopping the process.
    #[serde(default)]
    pub shutdown: ShutdownConfig,
    /// Linux-specific configuration
    #[serde(default)]
    pub linux: LinuxConfig,
    /// Who runs the supervision loop for this process.
    #[serde(default)]
    pub supervisor: SupervisionMode,
}

impl ProcessConfig {
    /// Whether the native supervisor should run a readiness probe.
    pub fn has_readiness_probe(&self) -> bool {
        if self.supervisor == SupervisionMode::External {
            return false;
        }
        self.ready.is_some()
            || self.listen.iter().any(|spec| spec.kind == ListenKind::Tcp)
            || !self.ports.is_empty()
    }
}

impl Default for ProcessConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            bash: "bash".to_string(),
            start: StartConfig::default(),
            process_type: ProcessType::default(),
            exec: String::new(),
            args: Vec::new(),
            cwd: None,
            env: HashMap::new(),
            listen: Vec::new(),
            ports: HashMap::new(),
            proxy: ProcessProxyConfig::default(),
            ready: None,
            restart: RestartConfig::default(),
            watch: WatchConfig::default(),
            watchdog: None,
            shutdown: ShutdownConfig::default(),
            linux: LinuxConfig::default(),
            supervisor: SupervisionMode::default(),
        }
    }
}
