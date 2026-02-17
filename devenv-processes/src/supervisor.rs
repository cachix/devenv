use std::sync::Arc;
use std::time::{Duration, Instant};

use devenv_activity::ProcessStatus;
use devenv_event_sources::{FileWatcher, FileWatcherConfig, NotifyMessage, TcpProbe};
use futures::future::Either;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use watchexec_supervisor::job::CommandState;
use watchexec_supervisor::{ProcessEnd, Signal};

use crate::config::ListenKind;
use crate::manager::ProcessResources;
use crate::supervisor_state::{Action, Event, ExitStatus, SupervisorPhase, SupervisorState};

/// Spawn a supervision task that monitors a job and handles restarts.
///
/// Uses `SupervisorState` for all restart/watchdog decisions.
/// The select loop maps I/O events to `Event`s and dispatches `Action`s.
pub fn spawn_supervisor(
    resources: &ProcessResources,
    shutdown: Arc<tokio::sync::Notify>,
) -> JoinHandle<()> {
    let config = resources.config.clone();
    let job = resources.job.clone();
    let activity = resources.activity.clone();
    let notify_socket = resources.notify_socket.clone();
    let status_tx = resources.status_tx.clone();
    let name = config.name.clone();

    // TCP probe for readiness (listen sockets or allocated ports, without notify)
    let tcp_probe_address = if !config.ready.as_ref().is_some_and(|r| r.notify) {
        // First try listen sockets
        config
            .listen
            .iter()
            .find_map(|spec| {
                if spec.kind == ListenKind::Tcp {
                    spec.address.clone()
                } else {
                    None
                }
            })
            // Fall back to the first allocated port
            .or_else(|| {
                config
                    .ports
                    .values()
                    .next()
                    .map(|port| format!("127.0.0.1:{}", port))
            })
    } else {
        None
    };

    tokio::spawn(async move {
        let mut state = SupervisorState::new(&config, Instant::now());

        // TCP probe: signals the supervisor loop when the port becomes reachable.
        // The supervisor handles the Ready event so status is updated consistently.
        let mut tcp_probe = tcp_probe_address
            .as_ref()
            .map(|addr| TcpProbe::spawn(addr.clone(), name.clone()));

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

        'supervisor: loop {
            tokio::select! {
                biased;

                _ = shutdown.notified() => {
                    debug!("Shutdown requested for {}", name);
                    break;
                }

                Some(()) = async {
                    match &mut tcp_probe {
                        Some(probe) => probe.recv().await,
                        None => std::future::pending::<Option<()>>().await,
                    }
                } => {
                    activity.log("TCP probe succeeded - process ready");
                    activity.set_status(ProcessStatus::Ready);
                    let _ = state.on_event(Event::Ready, Instant::now());
                    let _ = status_tx.send(state.status());
                    tcp_probe = None;
                }

                _ = file_watcher.recv() => {
                    info!("File change detected for {}, restarting", name);
                    activity.log("File change detected, restarting");
                    match state.on_event(Event::FileChange, Instant::now()) {
                        Action::Restart => {
                            job.stop_with_signal(Signal::Terminate, Duration::from_secs(2)).await;
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            job.start().await;
                            state.on_restart_complete(Instant::now());
                            let count = state.restart_count();
                            activity.log(format!("Restarted (attempt {})", count));
                            if let Some(ref address) = tcp_probe_address {
                                tcp_probe = Some(TcpProbe::spawn(address.clone(), name.clone()));
                            }
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
                                    debug!("Watchdog trigger from {}", name);
                                    match state.on_event(Event::WatchdogTrigger, Instant::now()) {
                                        Action::Restart => {
                                            activity.error("Watchdog trigger - process signaled failure");
                                            job.restart_with_signal(Signal::Terminate, Duration::from_secs(2)).await;
                                            state.on_restart_complete(Instant::now());
                                            let count = state.restart_count();
                                            warn!("Process {} watchdog trigger, restarted (attempt {})", name, count);
                                            activity.log(format!("Restarted (attempt {})", count));
                                            if let Some(ref address) = tcp_probe_address {
                                                tcp_probe = Some(TcpProbe::spawn(address.clone(), name.clone()));
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
                            state.on_restart_complete(Instant::now());
                            let count = state.restart_count();
                            info!("Restarted process {} (attempt {})", name, count);
                            activity.log(format!("Restarted (attempt {})", count));
                            if let Some(ref address) = tcp_probe_address {
                                tcp_probe = Some(TcpProbe::spawn(address.clone(), name.clone()));
                            }
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

                _ = job.to_wait() => {
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
                            job.start().await;
                            state.on_restart_complete(Instant::now());
                            let count = state.restart_count();
                            info!("Restarted process {} (attempt {})", name, count);
                            activity.log(format!("Restarted (attempt {})", count));
                            if let Some(ref address) = tcp_probe_address {
                                tcp_probe = Some(TcpProbe::spawn(address.clone(), name.clone()));
                            }
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
