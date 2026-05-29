use std::time::{Duration, Instant};

use devenv_activity::ProcessStatus;
use devenv_event_sources::{
    ExecProbe, FileWatcher, FileWatcherConfig, HttpGetProbe, NotifyMessage, TcpProbe,
};
use futures::future::Either;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, trace, warn};

use watchexec_supervisor::job::CommandState;
use watchexec_supervisor::{ProcessEnd, Signal};

use crate::config::ListenKind;
use crate::manager::ProcessResources;
use crate::supervisor_state::{
    Action, Event, ExitStatus, JobStatus, SupervisorPhase, SupervisorState,
};

/// Lifecycle requests delivered into the supervisor's loop.
///
/// The supervisor is the sole driver of its job, so a `Restart` or `Stop` here
/// can't race its restart policy. Each command carries an ack the manager awaits.
pub enum SupervisorCommand {
    /// Restart the job with a fresh restart budget.
    Restart { ack: oneshot::Sender<()> },
    /// Stop the job and end the supervisor task.
    Stop { ack: oneshot::Sender<()> },
}

/// Grace period before SIGKILL when stopping a process via a `Stop` command.
const STOP_GRACE: Duration = Duration::from_secs(5);

/// RAII accounting for a live supervisor. Incremented when a supervisor is
/// spawned, decremented (and `completion` notified) when its task ends for any
/// reason — a terminal phase, give-up, or an external `abort()`.
struct LiveGuard {
    live: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    completion: std::sync::Arc<tokio::sync::Notify>,
}

impl LiveGuard {
    fn new(
        live: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        completion: std::sync::Arc<tokio::sync::Notify>,
    ) -> Self {
        live.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self { live, completion }
    }
}

impl Drop for LiveGuard {
    fn drop(&mut self) {
        self.live.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        self.completion.notify_one();
    }
}

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

/// Processes without a readiness probe are ready as soon as their child is
/// launched. Keep the state machine aligned with the manager's initial status
/// after every kind of restart.
fn mark_ready_without_probe(config: &crate::config::ProcessConfig, state: &mut SupervisorState) {
    if !config.has_readiness_probe() {
        let _ = state.on_event(Event::Ready, Instant::now());
    }
}

/// Spawn a supervision task that monitors a job and handles restarts.
///
/// Uses `SupervisorState` for all restart/watchdog decisions.
/// The select loop maps I/O events to `Event`s and dispatches `Action`s.
pub fn spawn_supervisor(
    resources: &ProcessResources,
    shutdown: CancellationToken,
    mut cmd_rx: mpsc::Receiver<SupervisorCommand>,
    startup_result: Option<(
        mpsc::UnboundedReceiver<String>,
        oneshot::Sender<std::result::Result<(), String>>,
    )>,
) -> JoinHandle<()> {
    let config = resources.config.clone();
    let job = resources.job.clone();
    let activity = resources.activity.ref_handle();
    let notify_socket = resources.notify_socket.clone();
    let status_tx = resources.status_tx.clone();
    // Increment synchronously so the live count reflects this supervisor the
    // moment `launch`/`restart` returns, before any await point.
    let live_guard = LiveGuard::new(resources.live.clone(), resources.completion.clone());
    let name = config.name.clone();

    // Under an external supervisor, host owns probes/watcher entirely.
    let supervise_locally = config.supervisor == crate::config::Supervisor::Native;

    // Probe timing from ready config
    let initial_delay = Duration::from_secs(config.ready.as_ref().map_or(0, |r| r.initial_delay));
    let probe_period = Duration::from_secs(config.ready.as_ref().map_or(1, |r| r.period));
    let probe_timeout = Duration::from_secs(config.ready.as_ref().map_or(5, |r| r.probe_timeout));

    // Exec probe command (from ready.exec, only when not using notify)
    let exec_probe_cmd = if supervise_locally && !config.ready.as_ref().is_some_and(|r| r.notify) {
        config.ready.as_ref().and_then(|r| r.exec.clone())
    } else {
        None
    };

    // HTTP probe URL (from ready.http.get, only when not using notify)
    let http_probe_url = if supervise_locally && !config.ready.as_ref().is_some_and(|r| r.notify) {
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
    let tcp_probe_addresses: Option<Vec<String>> = if supervise_locally
        && !config.ready.as_ref().is_some_and(|r| r.notify)
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
        // Owns the live-count slot for the duration of this task; decrements on
        // drop (terminal break, give-up, or abort).
        let _live_guard = live_guard;

        // The supervisor is the sole driver of its job: start it here so there
        // is no window between launch and supervision where a fast process could
        // exit before the loop watches it. The spawn hook is already set on the job.
        job.start().await;
        if let Some((mut error_rx, result_tx)) = startup_result {
            if let Ok(error) = error_rx.try_recv() {
                job.delete().await;
                let _ = result_tx.send(Err(error));
                return;
            }
            let _ = result_tx.send(Ok(()));
        }

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

        // Under External the host owns file-watch reloads; pass empty paths
        // so FileWatcher pends forever in the select arm.
        let empty_paths: Vec<std::path::PathBuf> = Vec::new();
        let empty_strs: Vec<String> = Vec::new();
        let mut file_watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: if supervise_locally {
                    &config.watch.paths
                } else {
                    &empty_paths
                },
                extensions: if supervise_locally {
                    &config.watch.extensions
                } else {
                    &empty_strs
                },
                ignore: if supervise_locally {
                    &config.watch.ignore
                } else {
                    &empty_strs
                },
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
        // watchexec can deliver an exit notification for the process replaced
        // by an explicit stop/start after the new child is already running.
        // Discard exactly that notification; the following one belongs to the
        // replacement child and is handled normally.
        let mut discard_replaced_exit = false;

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

                Some(cmd) = cmd_rx.recv() => {
                    if shutdown.is_cancelled() { break 'supervisor; }
                    match cmd {
                        SupervisorCommand::Restart { ack } => {
                            activity.log("Restart requested");
                            // Restart atomically at the job layer. watchexec
                            // still publishes the replaced child's exit, so
                            // the job-wait arm discards that one notification.
                            job.restart_with_signal(Signal::Terminate, Duration::from_secs(2))
                                .await;
                            // The restart ticket can resolve after the graceful
                            // stop while the queued Start is still pending. A
                            // no-op job control is a barrier behind that Start.
                            job.run(|_| {}).await;
                            discard_replaced_exit = true;
                            state.reset_for_external_restart(Instant::now());
                            mark_ready_without_probe(&config, &mut state);
                            respawn_probes!();
                            let _ = status_tx.send(state.status());
                            let _ = ack.send(());
                        }
                        SupervisorCommand::Stop { ack } => {
                            activity.log("Stop requested");
                            // Surface Stopping phase to status_rx subscribers
                            // (TUI etc.) for the duration of the tail-stop
                            // grace period.
                            let _ = state.on_event(Event::StopRequested, Instant::now());
                            let _ = status_tx.send(state.status());
                            let _ = ack.send(());
                            break 'supervisor;
                        }
                    }
                    refresh_deadline!(state, current_deadline, deadline_fut);
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
                    let mut drained = 0usize;
                    while file_watcher.try_recv().is_ok() {
                        drained += 1;
                    }
                    info!("File change detected for {}, restarting", name);
                    if drained == 0 {
                        activity.log("File change detected, restarting");
                    } else {
                        activity.log(format!(
                            "File change detected, drained {} queued watch event(s), restarting",
                            drained
                        ));
                    }
                    match state.on_event(Event::FileChange, Instant::now()) {
                        Action::Restart => {
                            job.restart_with_signal(Signal::Terminate, Duration::from_secs(2))
                                .await;
                            job.run(|_| {}).await;
                            discard_replaced_exit = true;
                            state.on_restart_complete(Instant::now());
                            mark_ready_without_probe(&config, &mut state);
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
                                    trace!("Watchdog ping from {}", name);
                                    let _ = state.on_event(Event::WatchdogPing, Instant::now());
                                    let _ = status_tx.send(state.status());
                                }
                                NotifyMessage::WatchdogTrigger => {
                                    if shutdown.is_cancelled() { break 'supervisor; }
                                    debug!("Watchdog trigger from {}", name);
                                    match state.on_event(Event::WatchdogTrigger, Instant::now()) {
                                        Action::Restart => {
                                            activity.error("Watchdog trigger - process signaled failure");
                                            job.restart_with_signal(Signal::Terminate, Duration::from_secs(2)).await;
                                            job.run(|_| {}).await;
                                            discard_replaced_exit = true;
                                            state.on_restart_complete(Instant::now());
                                            mark_ready_without_probe(&config, &mut state);
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
                                    trace!("Extend timeout from {}: {} usec", name, usec);
                                    let _ = state.on_event(Event::ExtendTimeout { usec }, Instant::now());
                                    let _ = status_tx.send(state.status());
                                    // Eagerly refresh deadline since ExtendTimeout changes it
                                    refresh_deadline!(state, current_deadline, deadline_fut);
                                }
                                NotifyMessage::Status(status) => {
                                    trace!("Status from {}: {}", name, status);
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
                            job.restart_with_signal(Signal::Terminate, Duration::from_secs(2)).await;
                            job.run(|_| {}).await;
                            discard_replaced_exit = true;
                            state.on_restart_complete(Instant::now());
                            mark_ready_without_probe(&config, &mut state);
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

                // Skipped while parked in the Exited phase: `to_wait()` resolves
                // immediately for a command that isn't running, which would
                // busy-spin the loop. A parked process is only revived by a
                // file change (handled by the `file_watcher` arm above).
                _ = job.to_wait(), if state.phase() != SupervisorPhase::Exited => {
                    if shutdown.is_cancelled() { break 'supervisor; }
                    if discard_replaced_exit {
                        discard_replaced_exit = false;
                        trace!("Discarding replaced-process exit for {}", name);
                        continue 'supervisor;
                    }
                    // Extract exit status from the job
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

                    let exit_status = match rx.await {
                        Ok(Some(status)) => status,
                        _ => {
                            debug!("Process {} exited (unknown status)", name);
                            break;
                        }
                    };

                    match state.on_event(Event::ProcessExit { status: exit_status }, Instant::now()) {
                        Action::Restart => {
                            activity.log(format!("Process exited ({exit_status:?}), restarting"));
                            job.start().await;
                            state.on_restart_complete(Instant::now());
                            mark_ready_without_probe(&config, &mut state);
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
                            // When file watching is configured, keep the
                            // supervisor alive after a clean exit so subsequent
                            // file changes can re-run the process. This is what
                            // makes `watch` usable with one-shot commands that
                            // exit immediately, not just long-running ones.
                            // The process is reported as Exited, so `@completed`
                            // / `@started` dependencies are satisfied; the loop
                            // simply parks until a file change arrives.
                            if config.watch.paths.is_empty() {
                                break;
                            }
                            debug!(
                                "Process {} parked after exit; watching {} path(s) for changes",
                                name,
                                config.watch.paths.len()
                            );
                        }
                    }
                    let _ = status_tx.send(state.status());
                }
            }

            // Refresh the pinned deadline future only when the deadline changed
            refresh_deadline!(state, current_deadline, deadline_fut);
        }

        // The supervisor loop has ended. If the process reached a terminal phase
        // on its own — exited without a restart, or gave up after exhausting its
        // restart budget — reflect that in the activity. The `status_tx` channel
        // already carries the terminal phase to the manager/task system, but the
        // activity status drives the TUI, and nothing else updates it here: leave
        // it untouched and a self-exited process keeps showing as `running`.
        // A shutdown-driven exit is skipped: the manager's stop path owns the
        // `Stopping`/`Stopped` activity transition in that case.
        if !shutdown.is_cancelled()
            && matches!(
                state.phase(),
                SupervisorPhase::Exited | SupervisorPhase::GaveUp
            )
        {
            activity.set_status(match state.phase() {
                SupervisorPhase::Exited => ProcessStatus::Exited,
                SupervisorPhase::GaveUp => ProcessStatus::GaveUp,
                _ => unreachable!("terminal phase checked above"),
            });
        }

        // Tail-stop invariant: on every supervisor exit (shutdown / GaveUp /
        // Stop), the child gets SIGTERM-with-grace here. Without it, the child
        // only dies via watchexec KillOnDrop (SIGKILL) once the last Arc<Job>
        // drops — losing the graceful shutdown hook.
        job.stop_with_signal(Signal::Terminate, STOP_GRACE).await;

        trace!("Supervision task for {} exiting", name);
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
