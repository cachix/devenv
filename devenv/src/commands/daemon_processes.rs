//! `devenv daemon-processes`: re-exec target that runs the native process
//! manager as a detached daemon.
//!
//! Invoked by `devenv up -d` via re-exec to avoid fork-safety issues in
//! multithreaded programs. The parent serializes the task config to a JSON
//! file and spawns this process with `setsid` for full detachment.

use std::path::Path;
use std::sync::Arc;

use crate::tasks;
use miette::{IntoDiagnostic, Result, WrapErr};
use tokio_shutdown::Shutdown;

pub fn run(config_file: &Path, background: bool) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()?;

    runtime.block_on(async {
        let shutdown = Shutdown::new();
        shutdown.install_signals().await;

        let config_json = tokio::fs::read_to_string(config_file)
            .await
            .into_diagnostic()
            .wrap_err("Failed to read daemon config")?;
        let config: tasks::Config = serde_json::from_str(&config_json).into_diagnostic()?;

        let _ = tokio::fs::remove_file(config_file).await;

        let tasks_runner = tasks::Tasks::builder(
            config,
            devenv_core::VerbosityLevel::Normal,
            shutdown.clone(),
        )
        .build()
        .await
        .map_err(|e| miette::miette!("Failed to build task runner: {}", e))?;

        let phase = devenv_activity::start!(
            devenv_activity::Activity::operation("Running processes").parent(None)
        );
        let pid_file = tasks_runner.process_manager().manager_pid_file();
        let parent_activity = Arc::new(phase);

        // In `--background` mode (`devenv shell`) we write the PID file first so
        // the caller stops waiting and gets its shell immediately while processes
        // come up here; otherwise (`devenv up -d`) we wait for them to start
        // before signaling.
        if background {
            crate::processes::write_pid(&pid_file, std::process::id())
                .await
                .map_err(|e| miette::miette!("Failed to write PID: {}", e))?;
            let _outputs = tasks_runner.run_with_parent_activity(parent_activity).await;
        } else {
            let _outputs = tasks_runner.run_with_parent_activity(parent_activity).await;
            crate::processes::write_pid(&pid_file, std::process::id())
                .await
                .map_err(|e| miette::miette!("Failed to write PID: {}", e))?;
        }

        let result = tasks_runner
            .process_manager()
            .run_foreground(shutdown.cancellation_token(), None)
            .await
            .map_err(|e| miette::miette!("Process manager error: {}", e));

        let _ = tokio::fs::remove_file(&pid_file).await;
        result
    })
}
