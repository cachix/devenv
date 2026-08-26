//! `devenv daemon-processes`: re-exec target that runs the native process
//! manager as a detached daemon.
//!
//! Invoked by `devenv up -d` via re-exec to avoid fork-safety issues in
//! multithreaded programs. The parent serializes the task config to a JSON
//! file, disconnects its standard streams, and spawns it in a separate process
//! group. The implementation does not create a new Unix session.

use std::path::Path;
use std::sync::Arc;

use devenv::tasks;
use miette::{IntoDiagnostic, Result, WrapErr};
use tokio_shutdown::Shutdown;

pub fn run(config_file: &Path) -> Result<()> {
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

        let tasks_runner = Arc::new(
            tasks::Tasks::builder(
                config,
                devenv_core::VerbosityLevel::Normal,
                shutdown.clone(),
            )
            .build()
            .await
            .map_err(|e| miette::miette!("Failed to build task runner: {}", e))?,
        );

        let phase = devenv_activity::start!(
            devenv_activity::Activity::operation("Running processes").parent(None)
        );

        let manager = Arc::new(tasks::NativeProcessManager::new(
            Arc::clone(&tasks_runner),
            devenv::processes::ManagerResidence::Daemon,
        ));

        let _outputs = tasks_runner.run_with_parent_activity(Arc::new(phase)).await;

        let api_server = tasks::NativeApiServer::start(manager)?;

        let pid_file = api_server.manager().manager_pid_file();
        devenv::processes::write_pid(&pid_file, std::process::id())
            .await
            .map_err(|e| miette::miette!("Failed to write PID: {}", e))?;

        let result = api_server
            .manager()
            .run_event_loop(
                shutdown.cancellation_token(),
                None,
                devenv::processes::OnIdle::Linger,
            )
            .await
            .map_err(|e| miette::miette!("Process manager error: {}", e));

        let _ = tokio::fs::remove_file(&pid_file).await;
        result
    })
}
