use std::path::{Path, PathBuf};

use miette::{IntoDiagnostic, Result, WrapErr};
use tracing::debug;

use crate::config::ProcessConfig;

/// The output of building a process command: the command itself and its log paths.
pub struct BuiltCommand {
    pub script: String,
    pub stdout_log: PathBuf,
    pub stderr_log: PathBuf,
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

    let script = build_wrapper_script(
        config,
        &stdout_log,
        &stderr_log,
        notify_socket_path,
        watchdog_usec,
    );

    Ok(BuiltCommand {
        script,
        stdout_log,
        stderr_log,
    })
}

/// Build a shell wrapper script that handles env vars, cwd, logging, and sudo.
fn build_wrapper_script(
    config: &ProcessConfig,
    _stdout_log: &Path,
    _stderr_log: &Path,
    notify_socket_path: Option<&Path>,
    watchdog_usec: Option<u64>,
) -> String {
    use std::fmt::Write;

    let mut script = String::new();
    writeln!(script, "#!/bin/bash").unwrap();
    writeln!(script, "set -e").unwrap();

    if let Some(ref cwd) = config.cwd {
        writeln!(script, "cd {}", shell_escape::escape(cwd.to_string_lossy())).unwrap();
    }

    if !config.env.is_empty() {
        for (key, value) in &config.env {
            writeln!(
                script,
                "export {}={}",
                shell_escape::escape(key.into()),
                shell_escape::escape(value.into())
            )
            .unwrap();
        }
    }

    if let Some(path) = notify_socket_path {
        writeln!(
            script,
            "export NOTIFY_SOCKET={}",
            shell_escape::escape(path.to_string_lossy())
        )
        .unwrap();
    }

    if let Some(usec) = watchdog_usec {
        writeln!(script, "export WATCHDOG_USEC={}", usec).unwrap();
    }

    let mut cmd = String::new();

    write!(cmd, "{}", config.exec).unwrap();

    for arg in &config.args {
        write!(cmd, " {}", shell_escape::escape(arg.into())).unwrap();
    }

    writeln!(script, "exec {}", cmd).unwrap();

    debug!("Generated wrapper script for {}: {}", config.name, script);
    script
}
