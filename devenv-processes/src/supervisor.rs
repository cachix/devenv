use std::time::{Duration, Instant};

use devenv_activity::ProcessStatus;
use devenv_event_sources::{
    ExecProbe, FileWatcher, FileWatcherConfig, HttpGetProbe, NotifyMessage, TcpProbe,
};
use futures::future::Either;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use watchexec_supervisor::job::CommandState;
use watchexec_supervisor::{ProcessEnd, Signal};

use crate::config::ListenKind;
use crate::manager::{ProcessBackend, ProcessResources};
use crate::pty::PtyProcess;
use crate::supervisor_state::{
    Action, Event, ExitStatus, JobStatus, SupervisorPhase, SupervisorState,
};

/// Handle a successful probe by marking the process as ready.
fn handle_probe_success(
    activity: &devenv_activity::ActivityRef,
    state: &mut SupervisorState,
    status_tx: &tokio::sync::watch::Sender<JobStatus>,
    probe_name: &str,
) {
    activity.log(format!("{} probe succeeded - process ready", probe_name));
    activity.set_status(ProcessStatus::Ready);
    let _ = state.on_event(Event::Ready, Instant::now());
    let _ = status_tx.send(state.status());
}

fn pty_exit_status(code: i32) -> ExitStatus {
    if code == 0 {
        ExitStatus::Success
    } else {
        ExitStatus::Failure
    }
}

async fn restart_pty(
    pty: &tokio::sync::RwLock<Option<PtyProcess>>,
    config: &crate::config::ProcessConfig,
    spawn_env: &std::collections::HashMap<String, String>,
    cwd: Option<&std::path::PathBuf>,
    stdout_log: &std::path::Path,
    activity: &devenv_activity::ActivityRef,
) -> miette::Result<()> {
    let old = {
        let mut guard = pty.write().await;
        guard.take()
    };

    if let Some(mut old) = old {
        old.kill().await?;
    }

    let new_pty = PtyProcess::spawn(
        &config.bash,
        &config.exec,
        &config.args,
        cwd,
        spawn_env,
        stdout_log,
        Some(activity.clone()),
    )?;

    let mut guard = pty.write().await;
    *guard = Some(new_pty);
    Ok(())
}

async fn wait_for_pty_exit(pty: &tokio::sync::RwLock<Option<PtyProcess>>) -> Option<ExitStatus> {
    let mut exit_rx = {
        let guard = pty.read().await;
        guard.as_ref()?.exit_status()
    };

    loop {
        if let Some(code) = *exit_rx.borrow() {
            return Some(pty_exit_status(code));
        }

        if exit_rx.changed().await.is_err() {
            return None;
        }
    }
}

/// Spawn a supervision task that monitors a job and handles restarts.
///
/// Uses `SupervisorState` for all restart/watchdog decisions.
/// The select loop maps I/O events to `Event`s and dispatches `Action`s.
pub fn spawn_supervisor(
    resources: &ProcessResources,
    shutdown: CancellationToken,
) -> JoinHandle<()> {
    let config = resources.config.clone();
    let backend = resources.backend.clone();
    let activity = resources.activity.ref_handle();
    let notify_socket = resources.notify_socket.clone();
    let status_tx = resources.status_tx.clone();
    let spawn_env = resources.spawn_env.clone();
    let cwd = resources.cwd.clone();
    let stdout_log = resources.stdout_log.clone();
    let name = config.name.clone();

    // Probe timing from ready config
    let initial_delay = Duration::from_secs(config.ready.as_ref().map_or(0, |r| r.initial_delay));
    let probe_period = Duration::from_secs(config.ready.as_ref().map_or(1, |r| r.period));
    let probe_timeout = Duration::from_secs(config.ready.as_ref().map_or(5, |r| r.probe_timeout));

    // Exec probe command (from ready.exec, only when not using notify)
    let exec_probe_cmd = if !config.ready.as_ref().is_some_and(|r| r.notify) {
        config.ready.as_ref().and_then(|r| r.exec.clone())
    } else {
        None
    };

    // HTTP probe URL (from ready.http.get, only when not using notify)
    let http_probe_url = if !config.ready.as_ref().is_some_and(|r| r.notify) {
        config.ready.as_ref().and_then(|r| {
            r.http.as_ref().and_then(|h| {
                h.get
                    .as_ref()
                    .map(|get| format!("{}://{}:{}{}", get.scheme, get.host, get.port, get.path))
            })
        })
    } else {
        None
    };

    // TCP probe for readiness (listen sockets or allocated ports, without notify or exec/http)
    // Only use TCP probe as fallback when no explicit exec or http probe is configured.
    let tcp_probe_addresses: Option<Vec<String>> =
        if !config.ready.as_ref().is_some_and(|r| r.notify)
            && exec_probe_cmd.is_none()
            && http_probe_url.is_none()
        {
            // Explicit listen socket: probe only the configured address.
            // Allocated port: probe both IPv4 and IPv6 loopback since we don't
            // know which interface the process will bind to (e.g. vite uses tcp6).
            config
                .listen
                .iter()
                .find_map(|spec| {
                    if spec.kind == ListenKind::Tcp {
                        spec.address.clone().map(|addr| vec![addr])
                    } else {
                        None
                    }
                })
                .or_else(|| {
                    config
                        .ports
                        .values()
                        .next()
                        .map(|port| vec![format!("127.0.0.1:{}", port), format!("[::1]:{}", port)])
                })
        } else {
            None
        };

    tokio::spawn(async move {
        let mut state = SupervisorState::new(&config, Instant::now());

        // TCP probe: signals the supervisor loop when the port becomes reachable.
        // The supervisor handles the Ready event so status is updated consistently.
        let mut tcp_probe = tcp_probe_addresses
            .as_ref()
            .map(|addrs| TcpProbe::spawn(addrs.clone(), name.clone()));

        // Exec probe: runs a shell command periodically until it exits 0
        let process_env = config.env.clone();
        let process_bash = config.bash.clone();
        let mut exec_probe = exec_probe_cmd.as_ref().map(|cmd| {
            ExecProbe::spawn(
                cmd.clone(),
                name.clone(),
                process_bash.clone(),
                process_env.clone(),
                initial_delay,
                probe_period,
                probe_timeout,
            )
        });

        // HTTP probe: sends GET requests until a 2xx response
        let mut http_probe = http_probe_url.as_ref().map(|url| {
            HttpGetProbe::spawn(
                url.clone(),
                name.clone(),
                initial_delay,
                probe_period,
                probe_timeout,
            )
        });

        let mut file_watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: &config.watch.paths,
                extensions: &config.watch.extensions,
                ignore: &config.watch.ignore,
                recursive: true,
                ..Default::default()
            },
            &name,
        )
        .await;

        // Pin the deadline future outside the loop so it survives across iterations.
        // Recreate only when the deadline actually changes (after state transitions).
        let mut current_deadline = state.next_deadline();
        let deadline_fut = make_deadline_future(current_deadline);
        tokio::pin!(deadline_fut);

        /// Refresh the pinned deadline future if the state machine's deadline changed.
        macro_rules! refresh_deadline {
            ($state:expr, $current_deadline:expr, $deadline_fut:expr) => {
                let new_deadline = $state.next_deadline();
                if new_deadline != $current_deadline {
                    $current_deadline = new_deadline;
                    $deadline_fut.set(make_deadline_future(new_deadline));
                }
            };
        }

        /// Respawn all readiness probes after a process restart.
        macro_rules! respawn_probes {
            () => {
                if let Some(ref addrs) = tcp_probe_addresses {
                    tcp_probe = Some(TcpProbe::spawn(addrs.clone(), name.clone()));
                }
                if let Some(ref cmd) = exec_probe_cmd {
                    exec_probe = Some(ExecProbe::spawn(
                        cmd.clone(),
                        name.clone(),
                        process_bash.clone(),
                        process_env.clone(),
                        initial_delay,
                        probe_period,
                        probe_timeout,
                    ));
                }
                if let Some(ref url) = http_probe_url {
                    http_probe = Some(HttpGetProbe::spawn(
                        url.clone(),
                        name.clone(),
                        initial_delay,
                        probe_period,
                        probe_timeout,
                    ));
                }
            };
        }

        'supervisor: loop {
            tokio::select! {
                biased;

                _ = shutdown.cancelled() => {
                    debug!("Shutdown requested for {}", name);
                    break;
                }

                Some(()) = async {
                    match &mut tcp_probe {
                        Some(probe) => probe.recv().await,
                        None => std::future::pending::<Option<()>>().await,
                    }
                } => {
                    handle_probe_success(&activity, &mut state, &status_tx, "TCP");
                    tcp_probe = None;
                }

                Some(()) = async {
                    match &mut exec_probe {
                        Some(probe) => probe.recv().await,
                        None => std::future::pending::<Option<()>>().await,
                    }
                } => {
                    handle_probe_success(&activity, &mut state, &status_tx, "Exec");
                    exec_probe = None;
                }

                Some(()) = async {
                    match &mut http_probe {
                        Some(probe) => probe.recv().await,
                        None => std::future::pending::<Option<()>>().await,
                    }
                } => {
                    handle_probe_success(&activity, &mut state, &status_tx, "HTTP");
                    http_probe = None;
                }

                _ = file_watcher.recv() => {
                    // Bail if shutdown fired while another select arm was executing.
                    // The biased select catches it next iteration, but we skip
                    // expensive restart work below.
                    if shutdown.is_cancelled() { break 'supervisor; }
                        info!("File change detected for {}, restarting", name);
                        activity.log("File change detected, restarting");
                        match state.on_event(Event::FileChange, Instant::now()) {
                            Action::Restart => {
                            let restart_result = match &backend {
                                ProcessBackend::Job(job) => {
                                    job.stop_with_signal(Signal::Terminate, Duration::from_secs(2)).await;
                                    tokio::time::sleep(Duration::from_millis(100)).await;
                                    job.start().await;
                                    Ok(())
                                }
                                ProcessBackend::Pty(pty) => {
                                    restart_pty(pty, &config, &spawn_env, cwd.as_ref(), &stdout_log, &activity).await
                                }
                                ,
                            };
                            if let Err(err) = restart_result {
                                let reason = format!("Failed to restart process {}: {}", name, err);
                                warn!("{}", reason);
                                activity.error(&reason);
                                activity.fail();
                                let _ = status_tx.send(state.status());
                                break;
                            }
                            state.on_restart_complete(Instant::now());
                            let count = state.restart_count();
                            activity.log(format!("Restarted (attempt {})", count));
                            respawn_probes!();
                        }
                        Action::GiveUp { reason } => {
                            warn!("{}: {}", name, reason);
                            activity.error(reason);
                            activity.fail();
                            let _ = status_tx.send(state.status());
                            break;
                        }
                        Action::None => {}
                    }
                    let _ = status_tx.send(state.status());
                }

                result = async {
                    match &notify_socket {
                        Some(socket) => socket.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    if let Ok(messages) = result {
                        for msg in messages {
                            match msg {
                                NotifyMessage::Ready => {
                                    info!("Process {} signaled ready", name);
                                    activity.log("Process signaled ready");
                                    activity.set_status(ProcessStatus::Ready);
                                    let _ = state.on_event(Event::Ready, Instant::now());
                                    let _ = status_tx.send(state.status());
                                }
                                NotifyMessage::Watchdog => {
                                    debug!("Watchdog ping from {}", name);
                                    let _ = state.on_event(Event::WatchdogPing, Instant::now());
                                    let _ = status_tx.send(state.status());
                                }
                                NotifyMessage::WatchdogTrigger => {
                                    if shutdown.is_cancelled() { break 'supervisor; }
                                    debug!("Watchdog trigger from {}", name);
                                    match state.on_event(Event::WatchdogTrigger, Instant::now()) {
                                        Action::Restart => {
                                            activity.error("Watchdog trigger - process signaled failure");
                                            let restart_result = match &backend {
                                                ProcessBackend::Job(job) => {
                                                    job.restart_with_signal(Signal::Terminate, Duration::from_secs(2)).await;
                                                    Ok(())
                                                }
                                                ProcessBackend::Pty(pty) => {
                                                    restart_pty(pty, &config, &spawn_env, cwd.as_ref(), &stdout_log, &activity).await
                                                }
                                                ,
                                            };
                                            if let Err(err) = restart_result {
                                                let reason = format!("Failed to restart process {}: {}", name, err);
                                                warn!("{}", reason);
                                                activity.error(&reason);
                                                activity.fail();
                                                let _ = status_tx.send(state.status());
                                                break 'supervisor;
                                            }
                                            state.on_restart_complete(Instant::now());
                                            let count = state.restart_count();
                                            let msg = format!("Restarted (attempt {count})");
                                            warn!("Process {} watchdog trigger, restarted (attempt {})", name, count);
                                            activity.log(&msg);
                                            if let Some(ref addrs) = tcp_probe_addresses {
                                                tcp_probe = Some(TcpProbe::spawn(addrs.clone(), name.clone()));
                                            }
                                        }
                                        Action::GiveUp { reason } => {
                                            warn!("{}: {}", name, reason);
                                            activity.error(reason);
                                            activity.fail();
                                            let _ = status_tx.send(state.status());
                                            break 'supervisor;
                                        }
                                        Action::None => {}
                                    }
                                    let _ = status_tx.send(state.status());
                                }
                                NotifyMessage::ExtendTimeout { usec } => {
                                    debug!("Extend timeout from {}: {} usec", name, usec);
                                    let _ = state.on_event(Event::ExtendTimeout { usec }, Instant::now());
                                    let _ = status_tx.send(state.status());
                                    // Eagerly refresh deadline since ExtendTimeout changes it
                                    refresh_deadline!(state, current_deadline, deadline_fut);
                                }
                                NotifyMessage::Status(status) => {
                                    debug!("Status from {}: {}", name, status);
                                    activity.log(format!("Status: {}", status));
                                }
                                NotifyMessage::Stopping => {
                                    debug!("Process {} signaled stopping", name);
                                    activity.log("Process signaled stopping");
                                }
                                NotifyMessage::Reloading => {
                                    debug!("Process {} signaled reloading", name);
                                    activity.log("Process reloading configuration");
                                }
                                NotifyMessage::Unknown(s) => {
                                    debug!("Unknown notify message from {}: {}", name, s);
                                }
                            }
                        }
                    }
                }

                _ = &mut deadline_fut => {
                    if shutdown.is_cancelled() { break 'supervisor; }
                    let now = Instant::now();
                    let is_startup = state.phase() == SupervisorPhase::Starting;
                    if is_startup {
                        warn!("Startup timeout for process {}", name);
                        activity.error("Startup timeout - process did not become ready");
                    } else {
                        warn!("Watchdog timeout for process {}", name);
                        activity.error("Watchdog timeout - no heartbeat received");
                    }
                    match state.on_event(
                        if is_startup { Event::StartupTimeout } else { Event::WatchdogTimeout },
                        now,
                    ) {
                        Action::Restart => {
                            let restart_result = match &backend {
                                ProcessBackend::Job(job) => {
                                    job.restart_with_signal(Signal::Terminate, Duration::from_secs(2)).await;
                                    Ok(())
                                }
                                ProcessBackend::Pty(pty) => {
                                    restart_pty(pty, &config, &spawn_env, cwd.as_ref(), &stdout_log, &activity).await
                                }
                                ,
                            };
                            if let Err(err) = restart_result {
                                let reason = format!("Failed to restart process {}: {}", name, err);
                                warn!("{}", reason);
                                activity.error(&reason);
                                activity.fail();
                                let _ = status_tx.send(state.status());
                                break;
                            }
                            state.on_restart_complete(Instant::now());
                            let count = state.restart_count();
                            let msg = format!("Restarted (attempt {count})");
                            info!("Restarted process {} (attempt {})", name, count);
                            activity.log(&msg);
                            respawn_probes!();
                        }
                        Action::GiveUp { reason } => {
                            warn!("{}: {}", name, reason);
                            activity.error(reason);
                            activity.fail();
                            let _ = status_tx.send(state.status());
                            break;
                        }
                        Action::None => {}
                    }
                    let _ = status_tx.send(state.status());
                }

                result = async {
                    match &backend {
                        ProcessBackend::Job(job) => {
                            job.to_wait().await;

                            let (tx, rx) = tokio::sync::oneshot::channel();
                            job.run_async(move |ctx| {
                                let status = if let CommandState::Finished { status, .. } = ctx.current {
                                    Some(if matches!(status, ProcessEnd::Success) {
                                        ExitStatus::Success
                                    } else {
                                        ExitStatus::Failure
                                    })
                                } else {
                                    None
                                };
                                Box::new(async move {
                                    let _ = tx.send(status);
                                })
                            }).await;

                            match rx.await {
                                Ok(Some(status)) => Some(status),
                                _ => None,
                            }
                        }
                        ProcessBackend::Pty(pty) => wait_for_pty_exit(pty).await,
                    }
                } => {
                    if shutdown.is_cancelled() { break 'supervisor; }
                    let exit_status = match result {
                        Some(status) => status,
                        None => {
                            debug!("Process {} exited (unknown status)", name);
                            break;
                        }
                    };

                    match state.on_event(Event::ProcessExit { status: exit_status }, Instant::now()) {
                        Action::Restart => {
                            activity.log(format!("Process exited ({exit_status:?}), restarting"));
                            let restart_result = match &backend {
                                ProcessBackend::Job(job) => {
                                    job.start().await;
                                    Ok(())
                                }
                                ProcessBackend::Pty(pty) => {
                                    restart_pty(pty, &config, &spawn_env, cwd.as_ref(), &stdout_log, &activity).await
                                }
                                ,
                            };
                            if let Err(err) = restart_result {
                                let reason = format!("Failed to restart process {}: {}", name, err);
                                warn!("{}", reason);
                                activity.error(&reason);
                                activity.fail();
                                let _ = status_tx.send(state.status());
                                break;
                            }
                            state.on_restart_complete(Instant::now());
                            let count = state.restart_count();
                            let msg = format!("Restarted (attempt {count})");
                            info!("Restarted process {} (attempt {})", name, count);
                            activity.log(&msg);
                            respawn_probes!();
                        }
                        Action::GiveUp { reason } => {
                            warn!("{}: {}", name, reason);
                            activity.error(reason);
                            activity.fail();
                            let _ = status_tx.send(state.status());
                            break;
                        }
                        Action::None => {
                            debug!("Process {} exited, not restarting", name);
                            let _ = status_tx.send(state.status());
                            break;
                        }
                    }
                    let _ = status_tx.send(state.status());
                }
            }

            // Refresh the pinned deadline future only when the deadline changed
            refresh_deadline!(state, current_deadline, deadline_fut);
        }

        debug!("Supervision task for {} exiting", name);
    })
}

/// Returns a future that completes at `deadline`, or pends forever if `None`.
fn make_deadline_future(
    deadline: Option<Instant>,
) -> Either<tokio::time::Sleep, std::future::Pending<()>> {
    match deadline {
        Some(d) => Either::Left(tokio::time::sleep_until(d.into())),
        None => Either::Right(std::future::pending()),
    }
}
