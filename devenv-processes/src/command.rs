use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use miette::{IntoDiagnostic, Result, WrapErr};
use tracing::warn;
use watchexec_supervisor::command::{Command, Program, Shell, SpawnOptions};

use crate::config::ProcessConfig;

/// The output of building a process command: the command itself and its log paths.
pub struct BuiltCommand {
    pub command: Arc<Command>,
    pub stdout_log: PathBuf,
    pub stderr_log: PathBuf,
    /// Environment variables to set on the spawned process via the spawn hook.
    pub env: HashMap<String, String>,
    /// Working directory for the spawned process, set via the spawn hook.
    pub cwd: Option<PathBuf>,
}

/// Open a log file for appending, creating it if needed.
/// Returns `None` and logs a warning on failure.
pub fn open_log_file(path: &Path) -> Option<std::fs::File> {
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        Ok(f) => Some(f),
        Err(e) => {
            warn!("Failed to open log file {}: {}", path.display(), e);
            None
        }
    }
}

/// Compute the stdout/stderr log paths for a process.
pub fn log_paths(state_dir: &Path, name: &str) -> (PathBuf, PathBuf) {
    let log_dir = state_dir.join("logs");
    (
        log_dir.join(format!("{}.stdout.log", name)),
        log_dir.join(format!("{}.stderr.log", name)),
    )
}

/// Build a command from configuration, returning the command and log file paths.
///
/// Environment variables, working directory, and stdio redirection are returned
/// separately in `BuiltCommand` so they can be applied via the spawn hook on the
/// `TokioCommand` directly. This avoids exceeding the kernel's ARG_MAX limit
/// with large environments.
pub fn build_command(
    state_dir: &Path,
    config: &ProcessConfig,
    notify_socket_path: Option<&Path>,
    watchdog_usec: Option<u64>,
) -> Result<BuiltCommand> {
    let log_dir = state_dir.join("logs");
    std::fs::create_dir_all(&log_dir)
        .into_diagnostic()
        .wrap_err("Failed to create logs directory")?;

    let (stdout_log, stderr_log) = log_paths(state_dir, &config.name);

    // Build the environment: start with config.env, add notify/watchdog vars
    let mut env = config.env.clone();
    if let Some(path) = notify_socket_path {
        env.insert(
            "NOTIFY_SOCKET".to_string(),
            path.to_string_lossy().into_owned(),
        );
    }
    if let Some(usec) = watchdog_usec {
        env.insert("WATCHDOG_USEC".to_string(), usec.to_string());
    }

    let program = Program::Shell {
        shell: Shell::new("bash"),
        command: config.exec.clone(),
        args: config.args.clone(),
    };

    let command = Arc::new(Command {
        program,
        options: SpawnOptions {
            session: true,
            ..Default::default()
        },
    });

    Ok(BuiltCommand {
        command,
        stdout_log,
        stderr_log,
        env,
        cwd: config.cwd.clone(),
    })
}
