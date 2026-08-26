use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use devenv_activity::ActivityRef;
use devenv_event_sources::{
    ExecProbe, FileWatcher, FileWatcherConfig, HttpGetProbe, NotifyMessage, NotifySocket, TcpProbe,
};
use futures::future::Either;
use tokio::sync::{Notify, mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace, warn};
use watchexec_supervisor::ProcessEnd;
use watchexec_supervisor::job::{CommandState, Job};

use crate::config::{ListenKind, ProcessConfig, SupervisionMode};
use crate::manager::ProcessResources;
use crate::process_guardian::ProcessScopeRegistry;
use crate::process_state::{DeadlineId, ListenerId, RunId, WatcherId};
use crate::supervisor_state::{Action, Event, ExitOutcome, JobStatus, SupervisorState};
use crate::{ChildState, ReadinessState};

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

struct StampedNotifications {
    run: RunId,
    listener: ListenerId,
    messages: Vec<NotifyMessage>,
}

struct NotifyReceiver {
    cancel: CancellationToken,
    task: JoinHandle<()>,
}

impl NotifyReceiver {
    async fn stop(self) {
        self.cancel.cancel();
        let _ = self.task.await;
    }
}

fn spawn_notify_receiver(
    socket: Option<Arc<NotifySocket>>,
    run: RunId,
    listener: ListenerId,
    tx: mpsc::Sender<StampedNotifications>,
) -> Option<NotifyReceiver> {
    let socket = socket?;
    let cancel = CancellationToken::new();
    let task_cancel = cancel.clone();
    let task = tokio::spawn(async move {
        loop {
            let messages = tokio::select! {
                _ = task_cancel.cancelled() => break,
                result = socket.recv() => match result {
                    Ok(messages) => messages,
                    Err(error) => {
                        warn!(%error, "notify socket receive failed");
                        break;
                    }
                },
            };
            if tx
                .send(StampedNotifications {
                    run,
                    listener,
                    messages,
                })
                .await
                .is_err()
            {
                break;
            }
        }
    });
    Some(NotifyReceiver { cancel, task })
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
    ProbeSucceeded {
        run: RunId,
        kind: ProbeKind,
    },
    FileChanged {
        watcher: WatcherId,
        drained: usize,
    },
    Notifications {
        run: RunId,
        listener: ListenerId,
        messages: Vec<NotifyMessage>,
    },
    Deadline {
        run: RunId,
        deadline: DeadlineId,
    },
    ProcessExit {
        run: RunId,
    },
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
    notify_socket: Option<Arc<NotifySocket>>,
    status_tx: watch::Sender<JobStatus>,
    shutdown: CancellationToken,
    stop_requested: CancellationToken,
    active_process: ActiveProcessGuard,
    state: SupervisorState,
    probes: ProbeSet,
    run: RunId,
    deadline: DeadlineId,
    watcher: WatcherId,
    listener: ListenerId,
    notification_tx: mpsc::Sender<StampedNotifications>,
    notification_rx: mpsc::Receiver<StampedNotifications>,
    notify_receiver: Option<NotifyReceiver>,
}

impl SupervisorRuntime {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.state.next_deadline()
    }

    fn monitors_exit(&self) -> bool {
        self.state.status().child == ChildState::Running
    }

    fn refresh_deadline(
        &mut self,
        current_deadline: &mut Option<Instant>,
        mut deadline_fut: Pin<&mut Either<tokio::time::Sleep, std::future::Pending<()>>>,
    ) {
        let new_deadline = self.state.next_deadline();
        if new_deadline != *current_deadline {
            if self.deadline.next().is_none() {
                error!(
                    process = self.name(),
                    "deadline generation exhausted; stopping supervisor"
                );
                self.stop_requested.cancel();
                return;
            }
            *current_deadline = new_deadline;
            deadline_fut.set(make_deadline_future(new_deadline));
        }
    }

    fn advance_run(&mut self) -> bool {
        let (Some(run), Some(listener), Some(deadline)) = (
            self.run.successor(),
            self.listener.successor(),
            self.deadline.successor(),
        ) else {
            error!(
                process = self.name(),
                "process observation generation exhausted; stopping supervisor"
            );
            self.stop_requested.cancel();
            return false;
        };
        self.run = run;
        self.listener = listener;
        self.deadline = deadline;
        true
    }

    fn current_run(&self, observed: RunId, source: &'static str) -> bool {
        if observed == self.run {
            true
        } else {
            trace!(
                process = self.name(),
                ?observed,
                current = ?self.run,
                source,
                "ignoring stale process observation"
            );
            false
        }
    }

    fn publish_status(&self) {
        let next = self.state.status();
        if !next.is_valid() {
            error!(
                process = self.name(),
                current = ?*self.status_tx.borrow(),
                rejected = ?next,
                "rejecting invalid supervisor status publication"
            );
            return;
        }
        let _ = self.status_tx.send(next);
        self.activity.set_status(next.activity_status());
    }

    fn give_up(&self, reason: &'static str) {
        warn!("{}: {}", self.name(), reason);
        self.activity.error(reason);
        self.activity.fail();
        self.publish_status();
    }

    fn watches_for_changes(&self) -> bool {
        self.config.supervisor == SupervisionMode::Native && !self.config.watch.paths.is_empty()
    }

    /// Exhaustion is terminal for the current automatic-restart attempt, but
    /// a current file watcher may revive it. A live child first publishes
    /// Stopping and is reaped; only the settled state publishes GaveUp.
    async fn settle_give_up(&mut self, reason: &'static str) -> Continuation {
        self.give_up(reason);
        if self.state.status().child == ChildState::Running {
            crate::process_guardian::stop_job(&self.job, &self.scopes, &self.config.shutdown).await;
            self.state.on_termination_complete();
            self.publish_status();
        }
        self.active_process.settle();

        if self.watches_for_changes() {
            debug!(
                "Process {} parked after exhaustion; watching {} path(s) for changes",
                self.name(),
                self.config.watch.paths.len()
            );
            Continuation::Continue
        } else {
            Continuation::Exit
        }
    }

    async fn restart_job(&mut self) -> bool {
        crate::process_guardian::stop_job(&self.job, &self.scopes, &self.config.shutdown).await;
        if let Some(receiver) = self.notify_receiver.take() {
            receiver.stop().await;
        }
        if self.shutdown.is_cancelled() || self.stop_requested.is_cancelled() {
            return false;
        }
        if let Some(socket) = &self.notify_socket
            && let Err(error) = socket.drain()
        {
            warn!(process = self.name(), %error, "failed to reset notify socket");
            self.activity
                .error(format!("Failed to reset notify socket: {error}"));
            return false;
        }

        if !self.advance_run() {
            self.activity
                .error("Process observation generation exhausted");
            return false;
        }
        self.notify_receiver = spawn_notify_receiver(
            self.notify_socket.clone(),
            self.run,
            self.listener,
            self.notification_tx.clone(),
        );
        self.job.start().await;
        true
    }

    async fn handle_event(&mut self, event: SupervisorEvent) -> Continuation {
        match event {
            SupervisorEvent::Shutdown => {
                debug!("Shutdown requested for {}", self.name());
                Continuation::Exit
            }
            SupervisorEvent::StopRequested => self.handle_stop_requested(),
            SupervisorEvent::Command(command) => self.handle_command(command).await,
            SupervisorEvent::ProbeSucceeded { run, kind } => {
                if !self.current_run(run, "probe") {
                    return Continuation::Continue;
                }
                self.handle_probe_success(kind);
                Continuation::Continue
            }
            SupervisorEvent::FileChanged { watcher, drained } => {
                if watcher != self.watcher {
                    trace!(process = self.name(), ?watcher, current = ?self.watcher, "ignoring stale file observation");
                    return Continuation::Continue;
                }
                self.handle_file_change(drained).await
            }
            SupervisorEvent::Notifications {
                run,
                listener,
                messages,
            } => {
                if !self.current_run(run, "notify") || listener != self.listener {
                    trace!(process = self.name(), ?listener, current = ?self.listener, "ignoring stale notify listener");
                    return Continuation::Continue;
                }
                for message in messages {
                    if self.handle_notification(message).await == Continuation::Exit {
                        return Continuation::Exit;
                    }
                }
                Continuation::Continue
            }
            SupervisorEvent::Deadline { run, deadline } => {
                if !self.current_run(run, "deadline") || deadline != self.deadline {
                    trace!(process = self.name(), ?deadline, current = ?self.deadline, "ignoring stale deadline");
                    return Continuation::Continue;
                }
                self.handle_deadline().await
            }
            SupervisorEvent::ProcessExit { run } => {
                if !self.current_run(run, "exit") {
                    return Continuation::Continue;
                }
                self.handle_exit().await
            }
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
                return self.settle_give_up(reason).await;
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
                        return self.settle_give_up(reason).await;
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
        let is_startup = self.state.status().readiness == ReadinessState::Pending;
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
                return self.settle_give_up(reason).await;
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

        // Ignore an exit notification queued by the replaced run.
        if self.job.is_running() {
            trace!("Ignoring stale exit notification for {}", self.name());
            return Continuation::Continue;
        }

        let (tx, rx) = oneshot::channel();
        self.job
            .run_async(move |ctx| {
                let status = if let CommandState::Finished { status, .. } = ctx.current {
                    Some(if matches!(status, ProcessEnd::Success) {
                        ExitOutcome::Success
                    } else {
                        ExitOutcome::Failure
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
                return self.settle_give_up(reason).await;
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
        if let Some(receiver) = self.notify_receiver.take() {
            receiver.stop().await;
        }

        let controlled_teardown = self.state.status().child == ChildState::Running;
        crate::process_guardian::stop_job(&self.job, &self.scopes, &self.config.shutdown).await;
        if controlled_teardown {
            self.state.on_termination_complete();
            self.publish_status();
        }

        // Shutdown uses the manager's Stopping/Stopped activity transition.
        if !self.shutdown.is_cancelled() {
            self.activity
                .set_status(self.state.status().activity_status());
        }
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
    let activity = resources.activity.clone();
    let notify_socket = resources.notify_socket.clone();
    let status_tx = resources.status_tx.clone();
    let stop_requested = resources.stop_requested.clone();
    let active_process =
        ActiveProcessGuard::new(resources.live.clone(), resources.completion.clone());

    tokio::spawn(async move {
        let state = SupervisorState::new(&config, Instant::now());
        let probes = ProbeSet::new(&config);
        let mut run = RunId::initial();
        let _ = run.next();
        let mut deadline = DeadlineId::initial();
        let _ = deadline.next();
        let mut watcher = WatcherId::initial();
        let _ = watcher.next();
        let mut listener = ListenerId::initial();
        let _ = listener.next();
        let (notification_tx, notification_rx) = mpsc::channel(16);
        let notify_receiver = spawn_notify_receiver(
            notify_socket.clone(),
            run,
            listener,
            notification_tx.clone(),
        );
        let mut runtime = SupervisorRuntime {
            config,
            job: job.clone(),
            scopes,
            activity,
            notify_socket: notify_socket.clone(),
            status_tx,
            shutdown: shutdown.clone(),
            stop_requested: stop_requested.clone(),
            active_process,
            state,
            probes,
            run,
            deadline,
            watcher,
            listener,
            notification_tx,
            notification_rx,
            notify_receiver,
        };
        let mut file_watcher = spawn_file_watcher(&runtime.config).await;

        let mut current_deadline = runtime.next_deadline();
        let deadline_fut = make_deadline_future(current_deadline);
        tokio::pin!(deadline_fut);

        loop {
            let monitor_exit = runtime.monitors_exit();
            let run = runtime.run;
            let deadline = runtime.deadline;
            let watcher = runtime.watcher;
            let event = tokio::select! {
                biased;

                _ = shutdown.cancelled() => SupervisorEvent::Shutdown,
                _ = stop_requested.cancelled() => SupervisorEvent::StopRequested,
                Some(command) = cmd_rx.recv() => SupervisorEvent::Command(command),
                kind = runtime.probes.recv() => SupervisorEvent::ProbeSucceeded { run, kind },
                drained = next_file_change(&mut file_watcher) => {
                    SupervisorEvent::FileChanged { watcher, drained }
                }
                Some(batch) = runtime.notification_rx.recv() => {
                    SupervisorEvent::Notifications {
                        run: batch.run,
                        listener: batch.listener,
                        messages: batch.messages,
                    }
                }
                _ = &mut deadline_fut => SupervisorEvent::Deadline { run, deadline },
                // A parked job keeps `to_wait()` ready and would spin this loop.
                _ = job.to_wait(), if monitor_exit => SupervisorEvent::ProcessExit { run },
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

/// Returns a future that completes at `deadline`, or pends forever if `None`.
fn make_deadline_future(
    deadline: Option<Instant>,
) -> Either<tokio::time::Sleep, std::future::Pending<()>> {
    match deadline {
        Some(d) => Either::Left(tokio::time::sleep_until(d.into())),
        None => Either::Right(std::future::pending()),
    }
}
