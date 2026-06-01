//! `devenv daemon-processes`: re-exec target that runs the native process
//! manager as a detached daemon.
//!
//! Invoked by `devenv up -d` via re-exec to avoid fork-safety issues in
//! multithreaded programs. The parent serializes the task config to a JSON
//! file and spawns this process with `setsid` for full detachment.

use std::path::Path;
use std::sync::Arc;

use crate::processes::UpRequest;
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

        // Service `devenv up` attaches: the control socket forwards `Up`
        // requests to the task scheduler — which owns the dependency graph — so
        // it brings the requested processes up in dependency order, rather than
        // the client re-deriving the order and force-starting each one.
        //
        // Register the handler BEFORE the run below. The API socket starts
        // accepting connections as soon as processes are pre-registered (well
        // before `run_with_parent_activity` returns), and in `--background` mode
        // the PID file an attaching `devenv up` waits on is written first too.
        // Without the handler set, an `Up` arriving mid-startup is rejected
        // outright; buffered on this channel it instead waits until the
        // foreground loop drains it once startup completes.
        let (up_tx, up_rx) = tokio::sync::mpsc::channel::<UpRequest>(8);
        tasks_runner.process_manager().set_up_handler(up_tx);

        // The API server comes up at the start of the run, so the manager is
        // reachable while processes are still starting. In `--background` mode
        // (`devenv shell`) we write the PID file first so the caller stops
        // waiting and gets its shell immediately while processes come up here;
        // otherwise (`devenv up -d`) we wait for them to start before signaling.
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

        let result = run_foreground_with_up(&tasks_runner, &shutdown, up_rx).await;

        let _ = tokio::fs::remove_file(&pid_file).await;
        result
    })
}

/// Run the native manager in the foreground while answering `devenv up` attach
/// requests forwarded over the control socket, until the foreground loop exits
/// (shutdown signal or all processes gone).
///
/// The caller registers the up-handler (`set_up_handler`) and owns the receiver,
/// so it decides when the handler goes live: the daemon path sets it before the
/// cold start so requests that arrive during startup buffer here rather than
/// being rejected, while a foreground `devenv up` sets it just before calling.
pub(crate) async fn run_foreground_with_up(
    tasks_runner: &tasks::Tasks,
    shutdown: &Shutdown,
    mut up_rx: tokio::sync::mpsc::Receiver<UpRequest>,
) -> Result<()> {
    let foreground = tasks_runner
        .process_manager()
        .run_foreground(shutdown.cancellation_token(), None);
    tokio::pin!(foreground);

    loop {
        tokio::select! {
            res = &mut foreground => {
                break res.map_err(|e| miette::miette!("Process manager error: {}", e));
            }
            Some(req) = up_rx.recv() => {
                let started = tasks_runner.start_with_deps(&req.names).await;
                let _ = req.reply.send(started);
            }
        }
    }
}
