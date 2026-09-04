use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use devenv_activity::{ActivityRef, ProcessStatus};
use devenv_event_sources::{
    ExecProbe, FileWatcher, FileWatcherConfig, HttpGetProbe, NotifyMessage, NotifySocket, TcpProbe,
};
use futures::future::Either;
use tokio::sync::{Notify, mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, trace, warn};
use watchexec_supervisor::ProcessEnd;
use watchexec_supervisor::job::{CommandState, Job};

use crate::config::{ListenKind, ProcessConfig, SupervisionMode};
use crate::manager::ProcessResources;
use crate::process_guardian::ProcessScopeRegistry;
use crate::supervisor_state::{
    Action, Event, ExitStatus, JobStatus, SupervisorPhase, SupervisorState,
};

/// Lifecycle requests serialized with automatic policy in the supervisor loop.
pub enum SupervisorCommand {
    /// Restart the job with a fresh restart budget.
    Restart { ack: oneshot::Sender<()> },
    /// Stop the job and end the supervisor task.
    Stop { ack: oneshot::Sender<()> },
}

/// Counts a started process until it reaches a terminal phase.
struct ActiveProcessGuard {
    live: Arc<AtomicUsize>,
    completion: Arc<Notify>,
    active: AtomicBool,
}

impl ActiveProcessGuard {
    fn new(live: Arc<AtomicUsize>, completion: Arc<Notify>) -> Self {
        live.fetch_add(1, Ordering::SeqCst);
        Self {
            live,
            completion,
            active: AtomicBool::new(true),
        }
    }

    fn reactivate(&self) {
        if !self.active.swap(true, Ordering::SeqCst) {
            self.live.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn settle(&self) {
        if self.active.swap(false, Ordering::SeqCst) {
            self.live.fetch_sub(1, Ordering::SeqCst);
            self.completion.notify_one();
        }
    }
}

impl Drop for ActiveProcessGuard {
    fn drop(&mut self) {
        self.settle();
    }
}

#[derive(Clone, Copy)]
enum ProbeKind {
    Tcp,
    Exec,
    Http,
}

/// Readiness probe definitions and their currently running tasks.
struct ProbeSet {
    name: String,
    tcp_addresses: Option<Vec<String>>,
    exec_command: Option<String>,
    http_url: Option<String>,
    bash: String,
    env: HashMap<String, String>,
    initial_delay: Duration,
    period: Duration,
    timeout: Duration,
    tcp: Option<TcpProbe>,
    exec: Option<ExecProbe>,
    http: Option<HttpGetProbe>,
}

impl ProbeSet {
    fn new(config: &ProcessConfig) -> Self {
        let enabled = config.supervisor == SupervisionMode::Native;
        let uses_notifications = config.ready.as_ref().is_some_and(|ready| ready.notify);
        let explicit_probes_enabled = enabled && !uses_notifications;
        let exec_command = if explicit_probes_enabled {
            config.ready.as_ref().and_then(|ready| ready.exec.clone())
        } else {
            None
        };
        let http_url = if explicit_probes_enabled {
            config.ready.as_ref().and_then(|ready| {
                ready.http.as_ref().and_then(|http| {
                    http.get.as_ref().map(|get| {
                        format!("{}://{}:{}{}", get.scheme, get.host, get.port, get.path)
                    })
                })
            })
        } else {
            None
        };

        // TCP is the fallback when no explicit readiness probe is configured.
        let tcp_addresses =
            if explicit_probes_enabled && exec_command.is_none() && http_url.is_none() {
                // Allocated ports may bind either loopback address; declared sockets
                // have an exact address.
                config
                    .listen
                    .iter()
                    .find_map(|spec| {
                        if spec.kind == ListenKind::Tcp {
                            spec.address.clone().map(|address| vec![address])
                        } else {
                            None
                        }
                    })
                    .or_else(|| {
                        config
                            .ports
                            .values()
                            .next()
                            .map(|port| vec![format!("127.0.0.1:{port}"), format!("[::1]:{port}")])
                    })
            } else {
                None
            };

        let mut probes = Self {
            name: config.name.clone(),
            tcp_addresses,
            exec_command,
            http_url,
            bash: config.bash.clone(),
            env: config.env.clone(),
            initial_delay: Duration::from_secs(
                config.ready.as_ref().map_or(0, |ready| ready.initial_delay),
            ),
            period: Duration::from_secs(config.ready.as_ref().map_or(1, |ready| ready.period)),
            timeout: Duration::from_secs(
                config.ready.as_ref().map_or(5, |ready| ready.probe_timeout),
            ),
            tcp: None,
            exec: None,
            http: None,
        };
        probes.respawn();
        probes
    }

    fn respawn(&mut self) {
        self.respawn_tcp();
        self.exec = self.exec_command.as_ref().map(|command| {
            ExecProbe::spawn(
                command.clone(),
                self.name.clone(),
                self.bash.clone(),
                self.env.clone(),
                self.initial_delay,
                self.period,
                self.timeout,
            )
        });
        self.http = self.http_url.as_ref().map(|url| {
            HttpGetProbe::spawn(
                url.clone(),
                self.name.clone(),
                self.initial_delay,
                self.period,
                self.timeout,
            )
        });
    }

    fn respawn_tcp(&mut self) {
        self.tcp = self
            .tcp_addresses
            .as_ref()
            .map(|addresses| TcpProbe::spawn(addresses.clone(), self.name.clone()));
    }

    async fn recv(&mut self) -> ProbeKind {
        tokio::select! {
            biased;
            Some(()) = recv_tcp_probe(&mut self.tcp) => ProbeKind::Tcp,
            Some(()) = recv_exec_probe(&mut self.exec) => ProbeKind::Exec,
            Some(()) = recv_http_probe(&mut self.http) => ProbeKind::Http,
        }
    }

    fn complete(&mut self, kind: ProbeKind) {
        match kind {
            ProbeKind::Tcp => self.tcp = None,
            ProbeKind::Exec => self.exec = None,
            ProbeKind::Http => self.http = None,
        }
    }
}

async fn recv_tcp_probe(probe: &mut Option<TcpProbe>) -> Option<()> {
    match probe {
        Some(probe) => probe.recv().await,
        None => std::future::pending().await,
    }
}

async fn recv_exec_probe(probe: &mut Option<ExecProbe>) -> Option<()> {
    match probe {
        Some(probe) => probe.recv().await,
        None => std::future::pending().await,
    }
}

async fn recv_http_probe(probe: &mut Option<HttpGetProbe>) -> Option<()> {
    match probe {
        Some(probe) => probe.recv().await,
        None => std::future::pending().await,
    }
}

enum SupervisorEvent {
    Shutdown,
    StopRequested,
    Command(SupervisorCommand),
    ProbeSucceeded(ProbeKind),
    FileChanged { drained: usize },
    Notifications(Vec<NotifyMessage>),
    Deadline,
    ProcessExit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Continuation {
    Continue,
    Exit,
}

/// Mutable state and side effects for a single supervisor task.
struct SupervisorRuntime {
    config: ProcessConfig,
    job: Arc<Job>,
    scopes: Arc<ProcessScopeRegistry>,
    activity: ActivityRef,
    status_tx: watch::Sender<JobStatus>,
    shutdown: CancellationToken,
    stop_requested: CancellationToken,
    active_process: ActiveProcessGuard,
    state: SupervisorState,
    probes: ProbeSet,
}

impl SupervisorRuntime {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.state.next_deadline()
    }

    fn monitors_exit(&self) -> bool {
        self.state.phase() != SupervisorPhase::Exited
    }

    fn refresh_deadline(
        &self,
        current_deadline: &mut Option<Instant>,
        mut deadline_fut: Pin<&mut Either<tokio::time::Sleep, std::future::Pending<()>>>,
    ) {
        let new_deadline = self.state.next_deadline();
        if new_deadline != *current_deadline {
            *current_deadline = new_deadline;
            deadline_fut.set(make_deadline_future(new_deadline));
        }
    }

    fn publish_status(&self) {
        let status = self.state.status();
        let phase = match status.phase {
            SupervisorPhase::Starting => "starting",
            SupervisorPhase::Ready => "ready",
            SupervisorPhase::Stopping => "stopping",
            SupervisorPhase::Exited => "exited",
            SupervisorPhase::GaveUp => "gave_up",
        };
        self.activity
            .record("devenv.process.supervisor_phase", phase);
        self.activity
            .record("devenv.process.restart_count", status.restart_count as u64);
        if let Some(exit_status) = status.exit_status {
            self.activity.record(
                "devenv.process.exit_status",
                match exit_status {
                    ExitStatus::Success => "success",
                    ExitStatus::Failure => "failure",
                },
            );
        }
        let _ = self.status_tx.send(status);
    }

    fn give_up(&self, reason: &'static str) {
        warn!("{}: {}", self.name(), reason);
        self.activity.error(reason);
        self.activity.fail();
        self.publish_status();
    }

    async fn restart_job(&self) -> bool {
        crate::process_guardian::restart_job(
            &self.job,
            &self.scopes,
            &self.config.shutdown,
            &self.shutdown,
            &self.stop_requested,
        )
        .await
    }

    async fn handle_event(&mut self, event: SupervisorEvent) -> Continuation {
        match event {
            SupervisorEvent::Shutdown => {
                debug!("Shutdown requested for {}", self.name());
                Continuation::Exit
            }
            SupervisorEvent::StopRequested => self.handle_stop_requested(),
            SupervisorEvent::Command(command) => self.handle_command(command).await,
            SupervisorEvent::ProbeSucceeded(kind) => {
                self.handle_probe_success(kind);
                Continuation::Continue
            }
            SupervisorEvent::FileChanged { drained } => self.handle_file_change(drained).await,
            SupervisorEvent::Notifications(messages) => {
                for message in messages {
                    if self.handle_notification(message).await == Continuation::Exit {
                        return Continuation::Exit;
                    }
                }
                Continuation::Continue
            }
            SupervisorEvent::Deadline => self.handle_deadline().await,
            SupervisorEvent::ProcessExit => self.handle_exit().await,
        }
    }

    fn handle_stop_requested(&mut self) -> Continuation {
        debug!("Stop requested for {}", self.name());
        let _ = self.state.on_event(Event::StopRequested, Instant::now());
        self.publish_status();
        Continuation::Exit
    }

    async fn handle_command(&mut self, command: SupervisorCommand) -> Continuation {
        if self.shutdown.is_cancelled() {
            return Continuation::Exit;
        }

        match command {
            SupervisorCommand::Restart { ack } => {
                self.activity.log("Restart requested");
                self.active_process.reactivate();
                if !self.restart_job().await {
                    return Continuation::Exit;
                }
                self.state.reset_for_explicit_restart(Instant::now());
                if !self.config.has_readiness_probe() {
                    let _ = self.state.on_event(Event::Ready, Instant::now());
                }
                self.probes.respawn();
                self.publish_status();
                let _ = ack.send(());
                Continuation::Continue
            }
            SupervisorCommand::Stop { ack } => {
                self.activity.log("Stop requested");
                // Publish Stopping before the tail-stop grace period.
                let _ = self.state.on_event(Event::StopRequested, Instant::now());
                self.publish_status();
                let _ = ack.send(());
                Continuation::Exit
            }
        }
    }

    fn handle_probe_success(&mut self, kind: ProbeKind) {
        let probe_name = match kind {
            ProbeKind::Tcp => "TCP",
            ProbeKind::Exec => "Exec",
            ProbeKind::Http => "HTTP",
        };
        self.activity
            .log(format!("{probe_name} probe succeeded - process ready"));
        self.activity.set_status(ProcessStatus::Ready);
        let _ = self.state.on_event(Event::Ready, Instant::now());
        self.publish_status();
        self.probes.complete(kind);
    }

    async fn handle_file_change(&mut self, drained: usize) -> Continuation {
        if self.shutdown.is_cancelled() {
            return Continuation::Exit;
        }

        info!("File change detected for {}, restarting", self.name());
        if drained == 0 {
            self.activity.log("File change detected, restarting");
        } else {
            self.activity.log(format!(
                "File change detected, drained {drained} queued watch event(s), restarting"
            ));
        }

        match self.state.on_event(Event::FileChange, Instant::now()) {
            Action::Restart => {
                self.active_process.reactivate();
                if !self.restart_job().await {
                    return Continuation::Exit;
                }
                self.state.on_restart_complete(Instant::now());
                let count = self.state.restart_count();
                self.activity.log(format!("Restarted (attempt {count})"));
                self.probes.respawn();
            }
            Action::GiveUp { reason } => {
                self.give_up(reason);
                return Continuation::Exit;
            }
            Action::None => {}
        }
        self.publish_status();
        Continuation::Continue
    }

    async fn handle_notification(&mut self, message: NotifyMessage) -> Continuation {
        match message {
            NotifyMessage::Ready => {
                info!("Process {} signaled ready", self.name());
                self.activity.log("Process signaled ready");
                self.activity.set_status(ProcessStatus::Ready);
                let _ = self.state.on_event(Event::Ready, Instant::now());
                self.publish_status();
            }
            NotifyMessage::Watchdog => {
                trace!("Watchdog ping from {}", self.name());
                let _ = self.state.on_event(Event::WatchdogPing, Instant::now());
                self.publish_status();
            }
            NotifyMessage::WatchdogTrigger => {
                if self.shutdown.is_cancelled() {
                    return Continuation::Exit;
                }
                debug!("Watchdog trigger from {}", self.name());
                match self.state.on_event(Event::WatchdogTrigger, Instant::now()) {
                    Action::Restart => {
                        self.activity
                            .error("Watchdog trigger - process signaled failure");
                        if !self.restart_job().await {
                            return Continuation::Exit;
                        }
                        self.state.on_restart_complete(Instant::now());
                        let count = self.state.restart_count();
                        warn!(
                            "Process {} watchdog trigger, restarted (attempt {})",
                            self.name(),
                            count
                        );
                        self.activity.log(format!("Restarted (attempt {count})"));
                        self.probes.respawn_tcp();
                    }
                    Action::GiveUp { reason } => {
                        self.give_up(reason);
                        return Continuation::Exit;
                    }
                    Action::None => {}
                }
                self.publish_status();
            }
            NotifyMessage::ExtendTimeout { usec } => {
                trace!("Extend timeout from {}: {} usec", self.name(), usec);
                let _ = self
                    .state
                    .on_event(Event::ExtendTimeout { usec }, Instant::now());
                self.publish_status();
            }
            NotifyMessage::Status(status) => {
                trace!("Status from {}: {}", self.name(), status);
                self.activity.log(format!("Status: {status}"));
            }
            NotifyMessage::Stopping => {
                debug!("Process {} signaled stopping", self.name());
                self.activity.log("Process signaled stopping");
            }
            NotifyMessage::Reloading => {
                debug!("Process {} signaled reloading", self.name());
                self.activity.log("Process reloading configuration");
            }
            NotifyMessage::Unknown(message) => {
                debug!("Unknown notify message from {}: {}", self.name(), message);
            }
        }
        Continuation::Continue
    }

    async fn handle_deadline(&mut self) -> Continuation {
        if self.shutdown.is_cancelled() {
            return Continuation::Exit;
        }

        let now = Instant::now();
        let is_startup = self.state.phase() == SupervisorPhase::Starting;
        if is_startup {
            warn!("Startup timeout for process {}", self.name());
            self.activity
                .error("Startup timeout - process did not become ready");
        } else {
            warn!("Watchdog timeout for process {}", self.name());
            self.activity
                .error("Watchdog timeout - no heartbeat received");
        }

        match self.state.on_event(
            if is_startup {
                Event::StartupTimeout
            } else {
                Event::WatchdogTimeout
            },
            now,
        ) {
            Action::Restart => {
                if !self.restart_job().await {
                    return Continuation::Exit;
                }
                self.state.on_restart_complete(Instant::now());
                let count = self.state.restart_count();
                info!("Restarted process {} (attempt {})", self.name(), count);
                self.activity.log(format!("Restarted (attempt {count})"));
                self.probes.respawn();
            }
            Action::GiveUp { reason } => {
                self.give_up(reason);
                return Continuation::Exit;
            }
            Action::None => {}
        }
        self.publish_status();
        Continuation::Continue
    }

    async fn handle_exit(&mut self) -> Continuation {
        if self.shutdown.is_cancelled() {
            return Continuation::Exit;
        }

        // Ignore an exit notification queued by the replaced run. Query through
        // the job queue so this works with delegated children as well.
        let (running_tx, running_rx) = oneshot::channel();
        self.job.run(move |ctx| {
            let _ = running_tx.send(ctx.current.is_running());
        });
        if running_rx.await.unwrap_or(false) {
            trace!("Ignoring stale exit notification for {}", self.name());
            return Continuation::Continue;
        }

        let (tx, rx) = oneshot::channel();
        self.job
            .run_async(move |ctx| {
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
            })
            .await;

        let exit_status = match rx.await {
            Ok(Some(status)) => status,
            _ => {
                debug!("Process {} exited (unknown status)", self.name());
                return Continuation::Exit;
            }
        };

        self.scopes.cleanup(&self.config.shutdown).await;

        // A stop can arrive while cleanup waits for stubborn descendants. It
        // must win over an automatic restart.
        if self.shutdown.is_cancelled() || self.stop_requested.is_cancelled() {
            return Continuation::Exit;
        }

        match self.state.on_event(
            Event::ProcessExit {
                status: exit_status,
            },
            Instant::now(),
        ) {
            Action::Restart => {
                self.activity
                    .log(format!("Process exited ({exit_status:?}), restarting"));
                self.job.start().await;
                self.state.on_restart_complete(Instant::now());
                let count = self.state.restart_count();
                info!("Restarted process {} (attempt {})", self.name(), count);
                self.activity.log(format!("Restarted (attempt {count})"));
                self.probes.respawn();
            }
            Action::GiveUp { reason } => {
                self.give_up(reason);
                return Continuation::Exit;
            }
            Action::None => {
                self.active_process.settle();
                debug!("Process {} exited, not restarting", self.name());
                self.publish_status();
                if self.config.supervisor != SupervisionMode::Native
                    || self.config.watch.paths.is_empty()
                {
                    return Continuation::Exit;
                }
                debug!(
                    "Process {} parked after exit; watching {} path(s) for changes",
                    self.name(),
                    self.config.watch.paths.len()
                );
            }
        }
        self.publish_status();
        Continuation::Continue
    }

    async fn finish(&mut self) {
        // Shutdown uses the manager's Stopping/Stopped activity transition.
        if !self.shutdown.is_cancelled()
            && matches!(
                self.state.phase(),
                SupervisorPhase::Exited | SupervisorPhase::GaveUp
            )
        {
            self.activity.set_status(match self.state.phase() {
                SupervisorPhase::Exited => ProcessStatus::Exited,
                SupervisorPhase::GaveUp => ProcessStatus::GaveUp,
                _ => unreachable!("terminal phase checked above"),
            });
        }

        crate::process_guardian::stop_job(&self.job, &self.scopes, &self.config.shutdown).await;
        trace!("Supervision task for {} exiting", self.name());
    }
}

/// Spawn a task that monitors a job and applies `SupervisorState` decisions.
pub fn spawn_supervisor(
    resources: &ProcessResources,
    shutdown: CancellationToken,
    mut cmd_rx: mpsc::Receiver<SupervisorCommand>,
) -> JoinHandle<()> {
    let config = resources.config.clone();
    let job = resources.job.clone();
    let scopes = resources.scopes.clone();
    let activity = resources.activity.ref_handle();
    let notify_socket = resources.notify_socket.clone();
    let status_tx = resources.status_tx.clone();
    let stop_requested = resources.stop_requested.clone();
    let active_process =
        ActiveProcessGuard::new(resources.live.clone(), resources.completion.clone());

    tokio::spawn(async move {
        let state = SupervisorState::new(&config, Instant::now());
        let probes = ProbeSet::new(&config);
        let mut runtime = SupervisorRuntime {
            config,
            job: job.clone(),
            scopes,
            activity,
            status_tx,
            shutdown: shutdown.clone(),
            stop_requested: stop_requested.clone(),
            active_process,
            state,
            probes,
        };
        let mut file_watcher = spawn_file_watcher(&runtime.config).await;

        let mut current_deadline = runtime.next_deadline();
        let deadline_fut = make_deadline_future(current_deadline);
        tokio::pin!(deadline_fut);

        loop {
            let monitor_exit = runtime.monitors_exit();
            let event = tokio::select! {
                biased;

                _ = shutdown.cancelled() => SupervisorEvent::Shutdown,
                _ = stop_requested.cancelled() => SupervisorEvent::StopRequested,
                Some(command) = cmd_rx.recv() => SupervisorEvent::Command(command),
                kind = runtime.probes.recv() => SupervisorEvent::ProbeSucceeded(kind),
                drained = next_file_change(&mut file_watcher) => {
                    SupervisorEvent::FileChanged { drained }
                }
                messages = recv_notifications(&notify_socket) => {
                    SupervisorEvent::Notifications(messages)
                }
                _ = &mut deadline_fut => SupervisorEvent::Deadline,
                // A parked job keeps `to_wait()` ready and would spin this loop.
                _ = job.to_wait(), if monitor_exit => SupervisorEvent::ProcessExit,
            };

            if runtime.handle_event(event).await == Continuation::Exit {
                break;
            }
            runtime.refresh_deadline(&mut current_deadline, deadline_fut.as_mut());
        }

        runtime.finish().await;
    })
}

async fn spawn_file_watcher(config: &ProcessConfig) -> FileWatcher {
    let empty_paths: Vec<PathBuf> = Vec::new();
    let empty_strings: Vec<String> = Vec::new();
    let supervise_locally = config.supervisor == SupervisionMode::Native;
    FileWatcher::new(
        FileWatcherConfig {
            paths: if supervise_locally {
                &config.watch.paths
            } else {
                &empty_paths
            },
            extensions: if supervise_locally {
                &config.watch.extensions
            } else {
                &empty_strings
            },
            ignore: if supervise_locally {
                &config.watch.ignore
            } else {
                &empty_strings
            },
            recursive: true,
            ..Default::default()
        },
        &config.name,
    )
    .await
}

async fn next_file_change(file_watcher: &mut FileWatcher) -> usize {
    file_watcher.recv_batch().await;
    let mut drained = 0;
    while file_watcher.try_recv_batch().is_ok() {
        drained += 1;
    }
    drained
}

async fn recv_notifications(notify_socket: &Option<Arc<NotifySocket>>) -> Vec<NotifyMessage> {
    match notify_socket {
        Some(socket) => socket.recv().await.unwrap_or_default(),
        None => std::future::pending().await,
    }
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
