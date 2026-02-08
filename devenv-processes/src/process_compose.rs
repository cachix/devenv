//! Process-compose backend for process management.
//!
//! Uses the external process-compose tool to manage processes.

use async_trait::async_trait;
use miette::{IntoDiagnostic, Result, bail};
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::fs;
use tokio::process::Command;
use tracing::{debug, info, warn};

use crate::pid::{self, PidStatus};
use crate::{ProcessManager, StartOptions};

/// Process manager using external process-compose tool
pub struct ProcessComposeManager {
    /// Path to the procfile script built by Nix
    procfile_script: PathBuf,
    /// Directory for state files
    state_dir: PathBuf,
}

impl ProcessComposeManager {
    /// Create a new ProcessComposeManager
    ///
    /// # Arguments
    /// * `procfile_script` - Path to the Nix-built procfile script
    /// * `state_dir` - Directory for state files (wrapper script, PID file, logs)
    pub fn new(procfile_script: PathBuf, state_dir: PathBuf) -> Self {
        Self {
            procfile_script,
            state_dir,
        }
    }

    /// Path to the PID file
    pub fn pid_file(&self) -> PathBuf {
        self.state_dir.join("processes.pid")
    }

    /// Path to the log file
    pub fn log_file(&self) -> PathBuf {
        self.state_dir.join("processes.log")
    }

    /// Path to the wrapper script
    fn wrapper_script(&self) -> PathBuf {
        self.state_dir.join("processes")
    }

    /// Write the wrapper script that invokes process-compose
    async fn write_wrapper_script(&self, processes: &[String], disable_tui: bool) -> Result<()> {
        let tui_export = if disable_tui {
            "export PC_TUI_ENABLED=0\n"
        } else {
            ""
        };

        let processes_arg = processes.join(" ");
        let script = format!(
            "#!/usr/bin/env bash\n{tui_export}exec {} {processes_arg}\n",
            self.procfile_script.display()
        );

        let wrapper = self.wrapper_script();
        fs::write(&wrapper, script).await.into_diagnostic()?;
        fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))
            .await
            .into_diagnostic()?;

        Ok(())
    }
}

#[async_trait]
impl ProcessManager for ProcessComposeManager {
    async fn start(&self, options: StartOptions) -> Result<()> {
        // Check if already running
        match pid::check_pid_file(&self.pid_file()).await? {
            PidStatus::Running(pid) => {
                bail!(
                    "Processes already running with PID {}. Stop them first with: devenv processes down",
                    pid
                );
            }
            PidStatus::NotFound | PidStatus::StaleRemoved => {}
        }

        // Write wrapper script (disable TUI in detached mode)
        self.write_wrapper_script(&options.processes, options.detach)
            .await?;

        let wrapper = self.wrapper_script();
        let mut cmd = Command::new("bash");
        cmd.arg(&wrapper);

        // Set up environment
        if !options.env.is_empty() {
            cmd.env_clear().envs(&options.env);
        }

        if options.detach {
            // Detached mode: spawn and save PID
            let process = if options.log_to_file {
                let log_file = std::fs::File::create(self.log_file()).into_diagnostic()?;
                cmd.stdout(log_file.try_clone().into_diagnostic()?)
                    .stderr(log_file)
                    .spawn()
                    .into_diagnostic()?
            } else {
                cmd.stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .into_diagnostic()?
            };

            let pid = process
                .id()
                .ok_or_else(|| miette::miette!("Failed to get process ID"))?;
            pid::write_pid(&self.pid_file(), pid).await?;

            info!("PID is {}", pid);
            if options.log_to_file {
                info!("See logs:  $ tail -f {}", self.log_file().display());
            }
            info!("Stop:      $ devenv processes stop");
        } else {
            // Foreground mode: exec into the process
            use std::os::unix::process::CommandExt;
            let err = cmd.into_std().exec();
            bail!("Failed to exec process-compose: {}", err);
        }

        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        let pid_file = self.pid_file();

        if !pid_file.exists() {
            bail!("No processes running (PID file not found)");
        }

        let pid = pid::read_pid(&pid_file).await?;
        info!("Stopping process with PID {}", pid);

        // Send SIGTERM
        match signal::kill(pid, Signal::SIGTERM) {
            Ok(_) => {}
            Err(nix::errno::Errno::ESRCH) => {
                warn!("Process with PID {} not found, cleaning up PID file", pid);
                pid::remove_pid(&pid_file).await?;
                return Ok(());
            }
            Err(e) => {
                bail!("Failed to send SIGTERM to PID {}: {}", pid, e);
            }
        }

        // Wait for process to exit with exponential backoff
        let start = std::time::Instant::now();
        let max_wait = std::time::Duration::from_secs(30);
        let mut interval = std::time::Duration::from_millis(10);
        let max_interval = std::time::Duration::from_secs(1);

        loop {
            match signal::kill(pid, None) {
                Ok(_) => {
                    // Still running
                    if start.elapsed() >= max_wait {
                        warn!(
                            "Process {} did not shut down within {} seconds, sending SIGKILL to process group",
                            pid,
                            max_wait.as_secs()
                        );

                        // Send SIGKILL to process group
                        let pgid = Pid::from_raw(-pid.as_raw());
                        match signal::kill(pgid, Signal::SIGKILL) {
                            Ok(_) => info!("Sent SIGKILL to process group {}", pid.as_raw()),
                            Err(e) => warn!("Failed to send SIGKILL to process group: {}", e),
                        }

                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        break;
                    }

                    tokio::time::sleep(interval).await;
                    interval = std::time::Duration::from_secs_f64(
                        (interval.as_secs_f64() * 1.5).min(max_interval.as_secs_f64()),
                    );
                }
                Err(nix::errno::Errno::ESRCH) => {
                    debug!(
                        "Process {} has shut down after {:.2}s",
                        pid,
                        start.elapsed().as_secs_f32()
                    );
                    break;
                }
                Err(e) => {
                    warn!("Error checking process {}: {}", pid, e);
                    break;
                }
            }
        }

        pid::remove_pid(&pid_file).await?;
        Ok(())
    }

    async fn is_running(&self) -> bool {
        matches!(
            pid::check_pid_file(&self.pid_file()).await,
            Ok(PidStatus::Running(_))
        )
    }
}
