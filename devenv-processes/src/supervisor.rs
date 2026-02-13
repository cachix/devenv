use std::sync::Arc;
use std::time::{Duration, Instant};

use devenv_activity::ProcessStatus;
use futures::future::Either;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use watchexec_supervisor::job::CommandState;
use watchexec_supervisor::{ProcessEnd, Signal};

use crate::config::ListenKind;
use crate::manager::ProcessResources;
use crate::notify_socket::NotifyMessage;
use crate::supervisor_state::{Action, Event, ExitStatus, SupervisorState};

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
    let ready_state = resources.ready_state.clone();
    let name = config.name.clone();

    // TCP probe for readiness (listen sockets or allocated ports, without notify)
    let tcp_probe_address = if config.notify.as_ref().is_none_or(|n| !n.enable) {
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
        let mut ready_signaled = false;

        // TCP probe task: continuously tries to connect until success
        let _tcp_probe_task = tcp_probe_address.map(|address| {
            let ready_state = ready_state.clone();
            let name = name.clone();
            let activity = activity.clone();
            tokio::spawn(async move {
                debug!("Starting TCP probe for {} at {}", name, address);
                loop {
                    match tokio::net::TcpStream::connect(&address).await {
                        Ok(_) => {
                            info!("TCP probe succeeded for {} at {}", name, address);
                            activity.log("TCP probe succeeded - process ready");
                            activity.set_status(ProcessStatus::Ready);
                            let _ = ready_state.send(true);
                            break;
                        }
                        Err(_) => {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    }
                }
            })
        });

        let mut file_watcher = crate::file_watcher::FileWatcher::new(&config.watch, &name);

        // Pin the deadline future outside the loop so it survives across iterations.
        // Recreate only when the deadline actually changes (after state transitions).
        let mut current_deadline = state.next_deadline();
        let deadline_fut = make_deadline_future(current_deadline);
        tokio::pin!(deadline_fut);

        loop {
            tokio::select! {
                biased;

                _ = shutdown.notified() => {
                    debug!("Shutdown requested for {}", name);
                    break;
                }

                _ = file_watcher.rx.recv() => {
                    info!("File change detected for {}, restarting", name);
                    activity.log("File change detected, restarting");
                    // FileChange always returns Restart (no rate limiting)
                    state.on_event(Event::FileChange, Instant::now());
                    job.stop_with_signal(Signal::Terminate, Duration::from_secs(2)).await;
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    job.start().await;
                    state.on_restart_complete(Instant::now());
                }

                result = async {
                    match &notify_socket {
                        Some(socket) => socket.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    if let Ok(messages) = result {
                        let mut gave_up = false;
                        for msg in messages {
                            match msg {
                                NotifyMessage::Ready => {
                                    info!("Process {} signaled ready", name);
                                    activity.log("Process signaled ready");
                                    activity.set_status(ProcessStatus::Ready);
                                    state.on_event(Event::Ready, Instant::now());
                                    if !ready_signaled {
                                        ready_signaled = true;
                                        let _ = ready_state.send(true);
                                    }
                                }
                                NotifyMessage::Watchdog => {
                                    debug!("Watchdog ping from {}", name);
                                    state.on_event(Event::WatchdogPing, Instant::now());
                                }
                                NotifyMessage::WatchdogTrigger => {
                                    debug!("Watchdog trigger from {}", name);
                                    match state.on_event(Event::WatchdogTrigger, Instant::now()) {
                                        Action::Restart => {
                                            let count = state.restart_count() + 1;
                                            warn!("Process {} watchdog trigger, restarting (attempt {})", name, count);
                                            activity.error("Watchdog trigger - process signaled failure");
                                            activity.log(format!("Restarting (attempt {})", count));
                                            job.restart_with_signal(Signal::Terminate, Duration::from_secs(2)).await;
                                            state.on_restart_complete(Instant::now());
                                        }
                                        Action::GiveUp { reason } => {
                                            warn!("{}: {}", name, reason);
                                            activity.error(reason);
                                            activity.fail();
                                            gave_up = true;
                                            break;
                                        }
                                        Action::None => {}
                                    }
                                }
                                NotifyMessage::ExtendTimeout { usec } => {
                                    debug!("Extend timeout from {}: {} usec", name, usec);
                                    state.on_event(Event::ExtendTimeout { usec }, Instant::now());
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
                        if gave_up { break; }
                    }
                }

                _ = &mut deadline_fut => {
                    let now = Instant::now();
                    let is_startup = current_deadline.is_some_and(|d| state.is_startup_deadline(d));
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
                            let count = state.restart_count() + 1;
                            info!("Restarting process {} (attempt {})", name, count);
                            activity.log(format!("Restarting (attempt {})", count));
                            job.restart_with_signal(Signal::Terminate, Duration::from_secs(2)).await;
                            state.on_restart_complete(Instant::now());
                        }
                        Action::GiveUp { reason } => {
                            warn!("{}: {}", name, reason);
                            activity.error(reason);
                            activity.fail();
                            break;
                        }
                        Action::None => {}
                    }
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
                            let count = state.restart_count() + 1;
                            info!("Restarting process {} (attempt {})", name, count);
                            activity.log(format!("Restarting (attempt {})", count));
                            job.start().await;
                            state.on_restart_complete(Instant::now());
                        }
                        Action::GiveUp { reason } => {
                            warn!("{}: {}", name, reason);
                            activity.error(reason);
                            activity.fail();
                            break;
                        }
                        Action::None => {
                            debug!("Process {} exited, not restarting", name);
                            break;
                        }
                    }
                }
            }

            // Refresh the pinned deadline future only when the deadline changed
            let new_deadline = state.next_deadline();
            if new_deadline != current_deadline {
                current_deadline = new_deadline;
                deadline_fut.set(make_deadline_future(new_deadline));
            }
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
