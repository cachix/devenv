//! Processes supervised by an existing native manager.
//!
//! The task runner orders dependencies; the manager only launches processes
//! whose prerequisites this run has already satisfied. Status is queried while
//! waiting, so there is no replica of the manager's lifecycle state to maintain.

use devenv_processes::{ApiRequest, ApiResponse, AttachEvent, DependencyInfo, NativeManagerClient};
use miette::{Result, bail, miette};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, RwLock, watch};
use tokio::task::JoinHandle;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// A manager that owns process lifetimes, while this task run owns ordering.
#[derive(Debug)]
pub struct ProcessOwner {
    socket_path: PathBuf,
    /// Until a node's prerequisites are satisfied in this run, dependents must
    /// wait even if its process is already running in the manager.
    handed_over: RwLock<HashSet<String>>,
    /// Events are invalidations only. Each waiter has its own watch cursor, so
    /// a change during a status query remains pending until that waiter sees it.
    /// False means the subscription was lost, even if status queries still work.
    changes: watch::Receiver<bool>,
    watcher: JoinHandle<()>,
}

impl Drop for ProcessOwner {
    fn drop(&mut self) {
        self.watcher.abort();
    }
}

impl ProcessOwner {
    /// Establish the subscription before reading status. Receiving its initial
    /// snapshot confirms that subsequent transitions will be delivered.
    pub async fn connect(socket_path: &Path) -> Result<Arc<Self>> {
        let mut stream = tokio::time::timeout(
            REQUEST_TIMEOUT,
            NativeManagerClient::attach_stream(socket_path),
        )
        .await
        .map_err(|_| miette!("process manager did not respond in time"))??;
        match tokio::time::timeout(REQUEST_TIMEOUT, stream.next())
            .await
            .map_err(|_| miette!("process manager did not respond in time"))?
        {
            Some(Ok(AttachEvent::Snapshot { .. })) => {}
            Some(Err(error)) => return Err(error),
            other => bail!("Expected a process manager subscription snapshot, received {other:?}"),
        }
        let (events, changes) = watch::channel(true);
        let watcher = tokio::spawn(async move {
            while let Some(event) = stream.next().await {
                match event {
                    Ok(AttachEvent::Snapshot { .. } | AttachEvent::Status { .. }) => {
                        events.send_replace(true);
                    }
                    Ok(AttachEvent::Log { .. }) => {}
                    Err(error) => {
                        tracing::debug!(%error, "process manager subscription failed");
                        break;
                    }
                }
            }
            events.send_replace(false);
        });
        let owner = Arc::new(Self {
            socket_path: socket_path.to_path_buf(),
            handed_over: RwLock::new(HashSet::new()),
            changes,
            watcher,
        });
        owner.query(&[]).await?;
        Ok(owner)
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<bool> {
        let mut changes = self.changes.clone();
        changes.borrow_and_update();
        changes
    }

    fn check_subscription(&self) -> Result<()> {
        if !*self.changes.borrow() || self.changes.has_changed().is_err() {
            bail!("Lost the running process manager subscription; retry the task run");
        }
        Ok(())
    }

    pub(crate) async fn query(&self, names: &[String]) -> Result<Vec<DependencyInfo>> {
        self.check_subscription()?;
        let request = ApiRequest::DependencyStatus {
            names: names.to_vec(),
        };
        let mut subscription = self.changes.clone();
        let response = tokio::select! {
            biased;
            _ = subscription.wait_for(|connected| !*connected) => {
                bail!("Lost the running process manager subscription; retry the task run");
            }
            response = tokio::time::timeout(
                REQUEST_TIMEOUT,
                NativeManagerClient::api_request(&self.socket_path, &request),
            ) => response.map_err(|_| miette!("process manager did not respond in time"))??,
        };
        self.check_subscription()?;
        match response {
            ApiResponse::DependencyStatus { processes } => Ok(processes),
            ApiResponse::Error { message } => bail!("{message}"),
            other => bail!("Unexpected response to dependency status request: {other:?}"),
        }
    }

    /// Launch without resolving dependencies a second time. The daemon decides
    /// atomically whether the process is already live or has a terminal result.
    pub(crate) async fn start(&self, name: &str, restart: bool) -> Result<()> {
        self.check_subscription()?;
        let request = ApiRequest::EnsureStarted {
            name: name.to_string(),
            restart,
        };
        let mut subscription = self.changes.clone();
        let response = tokio::select! {
            biased;
            _ = subscription.wait_for(|connected| !*connected) => {
                bail!("Lost the running process manager subscription; retry the task run");
            }
            response = NativeManagerClient::api_request_bounded_connect(
                &self.socket_path, &request, REQUEST_TIMEOUT,
            ) => response?,
        };
        match response {
            ApiResponse::Ok => Ok(()),
            ApiResponse::Error { message } => bail!("{message}"),
            other => bail!("Unexpected response to ensure-started request: {other:?}"),
        }
    }

    pub(crate) async fn arm(&self, name: &str, notify: &Notify) {
        self.handed_over.write().await.insert(name.to_string());
        notify.notify_waiters();
    }

    pub(crate) async fn is_handed_over(&self, name: &str) -> bool {
        self.handed_over.read().await.contains(name)
    }

    /// Read authoritative state; lifecycle events only wake its readers.
    pub(crate) async fn state(&self, name: &str) -> Result<Option<DependencyInfo>> {
        self.check_subscription()?;
        if !self.is_handed_over(name).await {
            return Ok(None);
        }
        self.query(&[name.to_string()])
            .await?
            .into_iter()
            .next()
            .map(Some)
            .ok_or_else(|| miette!("process manager returned no status for '{name}'"))
    }
}

/// A `devenv tasks run` attached to a running daemon: the daemon is a real
/// [`crate::NativeProcessManager`] behind its API socket, and the attached
/// run is a separate [`crate::Tasks`] scheduler, so task selection and typed
/// edges are the scheduler's own.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TaskCompleted;
    use crate::{
        Config, NativeApiServer, NativeProcessManager, PROCESS_TASK_PREFIX, RunMode, TaskConfig,
        TaskStatus, TaskType, Tasks,
    };
    use devenv_core::VerbosityLevel;
    use devenv_processes::{
        ManagerResidence, ProcessConfig, ProcessPhase, ReadyConfig, RestartConfig, RestartPolicy,
        SupervisionMode,
    };
    use std::collections::HashMap;

    const BOUND: Duration = Duration::from_secs(30);

    fn config(dir: &Path, side: &str, tasks: Vec<TaskConfig>, roots: Vec<String>) -> Config {
        let runtime_dir = dir.join(format!("{side}-runtime"));
        std::fs::create_dir_all(&runtime_dir).unwrap();
        Config {
            tasks,
            roots,
            run_mode: RunMode::All,
            runtime_dir,
            cache_dir: dir.join(format!("{side}-cache")),
            sudo_context: None,
            env: HashMap::new(),
            bash: String::new(),
            ignore_process_deps: false,
            exit_on_idle: Some(false),
            supervisor: SupervisionMode::Native,
            capability_broker: None,
        }
    }

    /// A process that appends a line to `starts` every time it is launched.
    /// With `ready`, dependents may use `@ready` (the default suffix).
    fn process(name: &str, starts: &Path, body: &str, ready: bool) -> TaskConfig {
        TaskConfig {
            name: format!("{PROCESS_TASK_PREFIX}{name}"),
            r#type: TaskType::Process,
            command: Some(format!("echo started >> '{}'\n{body}", starts.display())),
            process: Some(ProcessConfig {
                ready: ready.then(|| ReadyConfig {
                    exec: Some("true".to_string()),
                    period: 1,
                    ..Default::default()
                }),
                restart: RestartConfig {
                    on: RestartPolicy::Never,
                    max: Some(0),
                    window: None,
                },
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// A one-shot that touches `marker`. One-shot commands are executables,
    /// so the body is written to a script next to the marker.
    fn oneshot(name: &str, after: &[String], marker: &Path) -> TaskConfig {
        use std::os::unix::fs::PermissionsExt;
        let script = marker.with_extension("sh");
        std::fs::write(
            &script,
            format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        TaskConfig {
            name: name.to_string(),
            r#type: TaskType::Oneshot,
            after: after.to_vec(),
            command: Some(script.display().to_string()),
            ..Default::default()
        }
    }

    fn dep(name: &str, suffix: &str) -> String {
        format!("{PROCESS_TASK_PREFIX}{name}{suffix}")
    }

    fn launches(starts: &Path) -> usize {
        std::fs::read_to_string(starts)
            .map(|contents| contents.lines().count())
            .unwrap_or(0)
    }

    struct Daemon {
        manager: Arc<NativeProcessManager>,
        _server: NativeApiServer,
    }

    impl Daemon {
        /// `devenv up -d <roots>`: only `roots` are launched; every other
        /// process is registered `NotStarted`.
        async fn start(dir: &Path, tasks: Vec<TaskConfig>, roots: &[&str]) -> Self {
            let roots = roots.iter().map(|name| dep(name, "")).collect();
            let tasks = Tasks::builder(
                config(dir, "daemon", tasks, roots),
                VerbosityLevel::Normal,
                tokio_shutdown::Shutdown::new(),
            )
            .build()
            .await
            .unwrap();
            let manager = Arc::new(NativeProcessManager::new(
                Arc::new(tasks),
                ManagerResidence::Daemon,
            ));
            let server = NativeApiServer::start(Arc::clone(&manager)).unwrap();
            tokio::time::timeout(BOUND, manager.tasks().run(true))
                .await
                .expect("daemon did not finish its initial schedule");
            Self {
                manager,
                _server: server,
            }
        }

        async fn phase(&self, name: &str) -> Option<ProcessPhase> {
            self.manager.process_runner().get_phase(name).await
        }

        async fn wait_for(&self, name: &str, phase: ProcessPhase) {
            tokio::time::timeout(BOUND, async {
                while self.phase(name).await != Some(phase) {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap_or_else(|_| panic!("daemon {name} never reached {phase}"));
        }

        async fn restarts(&self, name: &str) -> usize {
            self.manager
                .process_runner()
                .process_info_by_name(name)
                .await
                .unwrap()
                .restart_count
        }

        /// `devenv tasks run <roots>` attached to this daemon.
        async fn attached(
            &self,
            dir: &Path,
            tasks: Vec<TaskConfig>,
            roots: &[&str],
            shutdown: Arc<tokio_shutdown::Shutdown>,
        ) -> Arc<Tasks> {
            let owner = ProcessOwner::connect(&self.manager.api_socket_path())
                .await
                .unwrap();
            let roots = roots.iter().map(|root| root.to_string()).collect();
            Arc::new(
                Tasks::builder(
                    config(dir, "client", tasks, roots),
                    VerbosityLevel::Normal,
                    shutdown,
                )
                .with_process_owner(owner)
                .build()
                .await
                .unwrap(),
            )
        }
    }

    /// Run an attached task graph to completion, then do what `devenv tasks
    /// run` does on exit: stop everything its own runner started.
    async fn run_attached(tasks: &Tasks) {
        tokio::time::timeout(BOUND, tasks.run(false))
            .await
            .expect("attached task run did not finish");
        tasks.process_runner().stop_all().await.unwrap();
    }

    async fn status(tasks: &Tasks, name: &str) -> TaskStatus {
        let index = tasks.task_index_by_name[name];
        tasks.graph[index].read().await.status.clone()
    }

    fn succeeded(status: &TaskStatus) -> bool {
        matches!(status, TaskStatus::Completed(TaskCompleted::Success(..)))
    }

    #[tokio::test]
    async fn healthy_owned_process_is_reused_and_left_running() {
        let dir = tempfile::tempdir().unwrap();
        let starts = dir.path().join("web-starts");
        let ran = dir.path().join("repro-ran");
        let web = || process("web", &starts, "exec tail -f /dev/null", true);

        let daemon = Daemon::start(dir.path(), vec![web()], &["web"]).await;
        daemon.wait_for("web", ProcessPhase::Ready).await;

        let attached = daemon
            .attached(
                dir.path(),
                vec![web(), oneshot("test:repro", &[dep("web", "")], &ran)],
                &["test:repro"],
                tokio_shutdown::Shutdown::new(),
            )
            .await;
        run_attached(&attached).await;

        let s = status(&attached, "test:repro").await;
        assert!(succeeded(&s), "{} => {s:?}", "test:repro");
        assert!(ran.exists());
        assert_eq!(launches(&starts), 1, "the attached run launched web again");
        assert_eq!(daemon.restarts("web").await, 0);
        assert_eq!(daemon.phase("web").await, Some(ProcessPhase::Ready));
        assert_eq!(
            attached.process_runner().get_phase("web").await,
            None,
            "an owned process must never be registered with the local runner"
        );
        assert!(!attached.get_completion_status().await.has_failures());
        daemon.manager.stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn wanted_by_without_ordering_starts_the_owned_process() {
        let dir = tempfile::tempdir().unwrap();
        let starts = dir.path().join("web-starts");
        let ran = dir.path().join("root-ran");
        let web = || TaskConfig {
            wanted_by: Some(vec!["test:root".to_string()]),
            ..process("web", &starts, "exec tail -f /dev/null", true)
        };
        let root = || oneshot("test:root", &[], &ran);

        let daemon = Daemon::start(dir.path(), vec![web(), root()], &[]).await;
        assert_eq!(daemon.phase("web").await, Some(ProcessPhase::NotStarted));

        let attached = daemon
            .attached(
                dir.path(),
                vec![web(), root()],
                &["test:root"],
                tokio_shutdown::Shutdown::new(),
            )
            .await;
        run_attached(&attached).await;

        let s = status(&attached, "test:root").await;
        assert!(succeeded(&s), "{} => {s:?}", "test:root");
        assert_eq!(daemon.phase("web").await, Some(ProcessPhase::Ready));
        assert_eq!(launches(&starts), 1);
        daemon.manager.stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn empty_wanted_by_opts_an_ordered_process_out() {
        let dir = tempfile::tempdir().unwrap();
        let starts = dir.path().join("web-starts");
        let ran = dir.path().join("root-ran");
        let web = || TaskConfig {
            after: vec!["test:root".to_string()],
            wanted_by: Some(Vec::new()),
            ..process("web", &starts, "exec tail -f /dev/null", true)
        };
        let root = || oneshot("test:root", &[], &ran);

        let daemon = Daemon::start(dir.path(), vec![web(), root()], &[]).await;
        let attached = daemon
            .attached(
                dir.path(),
                vec![web(), root()],
                &["test:root"],
                tokio_shutdown::Shutdown::new(),
            )
            .await;
        run_attached(&attached).await;

        let s = status(&attached, "test:root").await;
        assert!(succeeded(&s), "{} => {s:?}", "test:root");
        assert_eq!(daemon.phase("web").await, Some(ProcessPhase::NotStarted));
        assert_eq!(launches(&starts), 0, "an opted-out process was started");
        daemon.manager.stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn completed_waits_for_a_running_owned_worker_and_started_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let starts = dir.path().join("worker-starts");
        let gate = dir.path().join("worker-gate");
        let started_ran = dir.path().join("started-ran");
        let completed_ran = dir.path().join("completed-ran");
        let body = format!(
            "while [ ! -e '{}' ]; do sleep 0.05; done\nexit 0",
            gate.display()
        );
        let worker = || process("worker", &starts, &body, false);

        let daemon = Daemon::start(dir.path(), vec![worker()], &["worker"]).await;
        daemon.wait_for("worker", ProcessPhase::Ready).await;

        let attached = daemon
            .attached(
                dir.path(),
                vec![
                    worker(),
                    oneshot("test:started", &[dep("worker", "@started")], &started_ran),
                    oneshot(
                        "test:completed",
                        &[dep("worker", "@completed")],
                        &completed_ran,
                    ),
                ],
                &["test:started", "test:completed"],
                tokio_shutdown::Shutdown::new(),
            )
            .await;
        let run = tokio::spawn({
            let attached = Arc::clone(&attached);
            async move { run_attached(&attached).await }
        });

        tokio::time::timeout(BOUND, async {
            while !started_ran.exists() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("@started did not run while the worker was running");
        // Give a wrong `@completed` evaluation ample time to show itself.
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(
            !completed_ran.exists(),
            "@completed ran while the worker was still running"
        );
        assert!(matches!(
            status(&attached, "test:completed").await,
            TaskStatus::Pending
        ));

        std::fs::write(&gate, "").unwrap();
        run.await.unwrap();

        let s = status(&attached, "test:completed").await;
        assert!(succeeded(&s), "{} => {s:?}", "test:completed");
        assert_eq!(daemon.phase("worker").await, Some(ProcessPhase::Exited));
        assert_eq!(launches(&starts), 1);
        daemon.manager.stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn completed_is_satisfied_by_an_owned_worker_that_already_exited() {
        let dir = tempfile::tempdir().unwrap();
        let starts = dir.path().join("worker-starts");
        let ran = dir.path().join("completed-ran");
        let worker = || process("worker", &starts, "exit 0", false);

        let daemon = Daemon::start(dir.path(), vec![worker()], &["worker"]).await;
        daemon.wait_for("worker", ProcessPhase::Exited).await;

        let attached = daemon
            .attached(
                dir.path(),
                vec![
                    worker(),
                    oneshot("test:completed", &[dep("worker", "@completed")], &ran),
                ],
                &["test:completed"],
                tokio_shutdown::Shutdown::new(),
            )
            .await;
        run_attached(&attached).await;

        let s = status(&attached, "test:completed").await;
        assert!(succeeded(&s), "{} => {s:?}", "test:completed");
        assert_eq!(launches(&starts), 1, "the exited worker was run again");
        assert_eq!(daemon.phase("worker").await, Some(ProcessPhase::Exited));
        let completion = attached.get_completion_status().await;
        assert!(!completion.has_failures());
        daemon.manager.stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn cancelling_an_attached_run_leaves_owned_processes_running() {
        let dir = tempfile::tempdir().unwrap();
        let starts = dir.path().join("web-starts");
        let ran = dir.path().join("never-ran");
        let web = || process("web", &starts, "exec tail -f /dev/null", true);

        let daemon = Daemon::start(dir.path(), vec![web()], &["web"]).await;
        daemon.wait_for("web", ProcessPhase::Ready).await;

        let shutdown = tokio_shutdown::Shutdown::new();
        let attached = daemon
            .attached(
                dir.path(),
                vec![
                    web(),
                    oneshot("test:wait", &[dep("web", "@completed")], &ran),
                ],
                &["test:wait"],
                Arc::clone(&shutdown),
            )
            .await;
        let run = tokio::spawn({
            let attached = Arc::clone(&attached);
            async move { run_attached(&attached).await }
        });
        tokio::time::sleep(Duration::from_millis(300)).await;
        shutdown.shutdown();
        run.await.unwrap();

        assert!(matches!(
            status(&attached, "test:wait").await,
            TaskStatus::Completed(TaskCompleted::Cancelled(_))
        ));
        assert!(!ran.exists());
        assert_eq!(daemon.phase("web").await, Some(ProcessPhase::Ready));
        assert_eq!(daemon.restarts("web").await, 0);
        assert_eq!(launches(&starts), 1);
        daemon.manager.stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn process_unknown_to_the_owner_fails_its_dependents() {
        let dir = tempfile::tempdir().unwrap();
        let starts = dir.path().join("ghost-starts");
        let ran = dir.path().join("never-ran");

        let daemon = Daemon::start(dir.path(), Vec::new(), &[]).await;
        let attached = daemon
            .attached(
                dir.path(),
                vec![
                    process("ghost", &starts, "exec tail -f /dev/null", true),
                    oneshot("test:x", &[dep("ghost", "")], &ran),
                ],
                &["test:x"],
                tokio_shutdown::Shutdown::new(),
            )
            .await;
        run_attached(&attached).await;

        assert!(matches!(
            status(&attached, &dep("ghost", "")).await,
            TaskStatus::Completed(TaskCompleted::Failed(_, ref failure))
                if failure.error.contains("not known to the running process manager")
        ));
        assert!(matches!(
            status(&attached, "test:x").await,
            TaskStatus::Completed(TaskCompleted::DependencyFailed)
        ));
        assert!(!ran.exists());
        assert_eq!(launches(&starts), 0);
        assert!(attached.get_completion_status().await.has_failures());
    }
    #[tokio::test]
    async fn owned_start_runs_prerequisites_only_in_the_requesting_scheduler() {
        let dir = tempfile::tempdir().unwrap();
        let starts = dir.path().join("web-starts");
        let prep_marker = dir.path().join("prep-count");
        let ran = dir.path().join("root-ran");
        let prep = || {
            let t = oneshot("test:prepare", &[], &prep_marker);
            std::fs::write(
                t.command.as_ref().unwrap(),
                format!("#!/bin/sh\necho prepared >> '{}'\n", prep_marker.display()),
            )
            .unwrap();
            t
        };
        let web = || TaskConfig {
            after: vec!["test:prepare".to_string()],
            ..process("web", &starts, "exec tail -f /dev/null", true)
        };
        let daemon = Daemon::start(dir.path(), vec![prep(), web()], &[]).await;
        let attached = daemon
            .attached(
                dir.path(),
                vec![prep(), web(), oneshot("test:root", &[dep("web", "")], &ran)],
                &["test:root"],
                tokio_shutdown::Shutdown::new(),
            )
            .await;
        run_attached(&attached).await;
        daemon.manager.stop_all().await.unwrap();
        assert!(ran.exists());
        assert_eq!(launches(&prep_marker), 1, "prerequisite ran twice");
    }

    /// A small API peer that keeps status authoritative and sends only wakeups
    /// over attach. `race` publishes Ready before replying with an old Starting
    /// sample, reproducing the original lost-event window.
    struct NotificationServer {
        socket: PathBuf,
        sampled: Arc<Notify>,
        disconnect: Option<tokio::sync::oneshot::Sender<()>>,
        task: JoinHandle<()>,
    }

    impl NotificationServer {
        async fn start(dir: &Path, race: bool, settled: ProcessPhase, stall: bool) -> Self {
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
            let socket = dir.join("api.sock");
            let listener = tokio::net::UnixListener::bind(&socket).unwrap();
            let (disconnect, mut disconnect_rx) = tokio::sync::oneshot::channel();
            let sampled = Arc::new(Notify::new());
            let signal = Arc::clone(&sampled);
            let task = tokio::spawn(async move {
                let mut attach: Option<tokio::net::unix::OwnedWriteHalf> = None;
                let mut named_queries = 0;
                loop {
                    let (stream, _) = tokio::select! {
                        _ = &mut disconnect_rx, if attach.is_some() => {
                            attach.take();
                            continue;
                        }
                        accepted = listener.accept() => accepted.unwrap(),
                    };
                    let (read, mut write) = stream.into_split();
                    let mut line = String::new();
                    BufReader::new(read).read_line(&mut line).await.unwrap();
                    let request = serde_json::from_str::<ApiRequest>(&line).unwrap();
                    if stall
                        && (matches!(&request, ApiRequest::EnsureStarted { .. })
                            || matches!(&request, ApiRequest::DependencyStatus { names } if !names.is_empty()))
                    {
                        signal.notify_one();
                        let _ = (&mut disconnect_rx).await;
                        attach.take();
                        // Keep the API reply open after the stream closes.
                        std::future::pending::<()>().await;
                    }
                    let mut has_sample = false;
                    let response = match request {
                        ApiRequest::Attach => {
                            let snapshot = AttachEvent::Snapshot {
                                processes: Vec::new(),
                            };
                            write
                                .write_all(
                                    (serde_json::to_string(&snapshot).unwrap() + "\n").as_bytes(),
                                )
                                .await
                                .unwrap();
                            attach = Some(write);
                            continue;
                        }
                        ApiRequest::EnsureStarted { .. } => ApiResponse::Ok,
                        ApiRequest::DependencyStatus { names } => {
                            let processes = if names.is_empty() {
                                Vec::new()
                            } else {
                                has_sample = true;
                                named_queries += 1;
                                let phase = if race && named_queries == 1 {
                                    let event = AttachEvent::Status {
                                        info: devenv_processes::ProcessInfo {
                                            name: "web".to_string(),
                                            phase: settled,
                                            restart_count: 0,
                                            ports: Vec::new(),
                                        },
                                    };
                                    attach
                                        .as_mut()
                                        .unwrap()
                                        .write_all(
                                            (serde_json::to_string(&event).unwrap() + "\n")
                                                .as_bytes(),
                                        )
                                        .await
                                        .unwrap();
                                    tokio::time::sleep(Duration::from_millis(50)).await;
                                    ProcessPhase::Starting
                                } else if race {
                                    settled
                                } else {
                                    ProcessPhase::Starting
                                };
                                names
                                    .into_iter()
                                    .map(|name| DependencyInfo {
                                        name,
                                        phase: Some(phase),
                                        exit_status: None,
                                    })
                                    .collect()
                            };
                            ApiResponse::DependencyStatus { processes }
                        }
                        request => panic!("unexpected request: {request:?}"),
                    };
                    write
                        .write_all((serde_json::to_string(&response).unwrap() + "\n").as_bytes())
                        .await
                        .unwrap();
                    if has_sample {
                        signal.notify_one();
                    }
                }
            });
            Self {
                socket,
                sampled,
                disconnect: Some(disconnect),
                task,
            }
        }
    }

    impl Drop for NotificationServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    #[tokio::test]
    async fn events_during_status_queries_are_retained_by_readiness_and_dependency_waiters() {
        // Exercise both the process's own readiness wait and its dependent's
        // typed-edge wait. Neither may miss an event while querying status.
        for ready_config in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let settled = if ready_config {
                ProcessPhase::Ready
            } else {
                ProcessPhase::Exited
            };
            let suffix = if ready_config { "" } else { "@completed" };
            let server = NotificationServer::start(dir.path(), true, settled, false).await;
            let ran = dir.path().join("root-ran");
            let owner = ProcessOwner::connect(&server.socket).await.unwrap();
            let tasks = Tasks::builder(
                config(
                    dir.path(),
                    "client",
                    vec![
                        process("web", &dir.path().join("starts"), "unused", ready_config),
                        oneshot("test:root", &[dep("web", suffix)], &ran),
                    ],
                    vec!["test:root".to_string()],
                ),
                VerbosityLevel::Normal,
                tokio_shutdown::Shutdown::new(),
            )
            .with_process_owner(owner)
            .build()
            .await
            .unwrap();
            run_attached(&tasks).await;
            assert!(ran.exists(), "the stale Starting sample stranded a waiter");
            assert!(!tasks.get_completion_status().await.has_failures());
        }
    }

    #[tokio::test]
    async fn stream_closure_fails_waiters_even_when_status_requests_still_work() {
        for ready_config in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let suffix = if ready_config { "" } else { "@completed" };
            let mut server =
                NotificationServer::start(dir.path(), false, ProcessPhase::Ready, false).await;
            let ran = dir.path().join("never-ran");
            let owner = ProcessOwner::connect(&server.socket).await.unwrap();
            let tasks = Arc::new(
                Tasks::builder(
                    config(
                        dir.path(),
                        "client",
                        vec![
                            process("web", &dir.path().join("starts"), "unused", ready_config),
                            oneshot("test:root", &[dep("web", suffix)], &ran),
                        ],
                        vec!["test:root".to_string()],
                    ),
                    VerbosityLevel::Normal,
                    tokio_shutdown::Shutdown::new(),
                )
                .with_process_owner(owner)
                .build()
                .await
                .unwrap(),
            );
            let run = tokio::spawn({
                let tasks = Arc::clone(&tasks);
                async move { run_attached(&tasks).await }
            });
            tokio::time::timeout(BOUND, server.sampled.notified())
                .await
                .unwrap();
            server.disconnect.take().unwrap().send(()).unwrap();
            run.await.unwrap();
            assert!(!ran.exists());
            assert!(tasks.get_completion_status().await.has_failures());
            // Only the subscription closed; the peer still answers status.
            assert!(matches!(
                NativeManagerClient::api_request(
                    &server.socket,
                    &ApiRequest::DependencyStatus {
                        names: vec!["web".to_string()]
                    }
                )
                .await
                .unwrap(),
                ApiResponse::DependencyStatus { .. }
            ));
        }
    }

    #[tokio::test]
    async fn stream_closure_interrupts_in_flight_status_and_start_requests() {
        for start_request in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let mut server =
                NotificationServer::start(dir.path(), false, ProcessPhase::Ready, true).await;
            let owner = ProcessOwner::connect(&server.socket).await.unwrap();
            let request = tokio::spawn(async move {
                if start_request {
                    owner.start("web", false).await
                } else {
                    owner.query(&["web".to_string()]).await.map(|_| ())
                }
            });
            tokio::time::timeout(BOUND, server.sampled.notified())
                .await
                .unwrap();
            server.disconnect.take().unwrap().send(()).unwrap();
            let error = tokio::time::timeout(Duration::from_secs(2), request)
                .await
                .expect("stream closure did not interrupt the in-flight request")
                .unwrap()
                .unwrap_err();
            assert!(error.to_string().contains("subscription"));
        }
    }

    #[tokio::test]
    async fn concurrent_ensure_requests_launch_once() {
        let dir = tempfile::tempdir().unwrap();
        let starts = dir.path().join("web-starts");
        let web = process("web", &starts, "exec tail -f /dev/null", false);
        let daemon = Daemon::start(dir.path(), vec![web], &[]).await;
        let owner = ProcessOwner::connect(&daemon.manager.api_socket_path())
            .await
            .unwrap();
        let (first, second) = tokio::join!(owner.start("web", false), owner.start("web", false));
        first.unwrap();
        second.unwrap();
        daemon.wait_for("web", ProcessPhase::Ready).await;
        // The shell can publish its phase before executing the first command.
        tokio::time::timeout(BOUND, async {
            while launches(&starts) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        daemon.manager.stop_all().await.unwrap();
        assert_eq!(launches(&starts), 1);
    }

    #[tokio::test]
    async fn explicitly_requested_exited_process_is_restarted() {
        let dir = tempfile::tempdir().unwrap();
        let starts = dir.path().join("worker-starts");
        let worker = || process("worker", &starts, "exit 0", false);
        let daemon = Daemon::start(dir.path(), vec![worker()], &["worker"]).await;
        daemon.wait_for("worker", ProcessPhase::Exited).await;
        let attached = daemon
            .attached(
                dir.path(),
                vec![worker()],
                &["devenv:processes:worker"],
                tokio_shutdown::Shutdown::new(),
            )
            .await;
        run_attached(&attached).await;
        daemon.wait_for("worker", ProcessPhase::Exited).await;
        assert_eq!(launches(&starts), 2);
        daemon.manager.stop_all().await.unwrap();
    }
    #[tokio::test]
    async fn ensure_preserves_an_existing_dependency_waiter() {
        let dir = tempfile::tempdir().unwrap();
        let starts = dir.path().join("web-starts");
        let gate = dir.path().join("prepare-gate");
        let marker = dir.path().join("prepare-ran");
        let prepare = oneshot("test:prepare", &[], &marker);
        std::fs::write(
            prepare.command.as_ref().unwrap(),
            format!(
                "#!/bin/sh\nwhile [ ! -e '{}' ]; do sleep 0.05; done\ntouch '{}'\n",
                gate.display(),
                marker.display(),
            ),
        )
        .unwrap();
        let web = TaskConfig {
            after: vec!["test:prepare".to_string()],
            ..process("web", &starts, "exec tail -f /dev/null", true)
        };
        let daemon = Daemon::start(dir.path(), vec![prepare, web], &[]).await;
        daemon.manager.tasks().start_with_deps(["web"]).await;
        let owner = ProcessOwner::connect(&daemon.manager.api_socket_path())
            .await
            .unwrap();
        owner.start("web", false).await.unwrap();
        assert_eq!(daemon.phase("web").await, Some(ProcessPhase::Waiting));
        assert_eq!(launches(&starts), 0, "ensure bypassed the existing waiter");
        std::fs::write(gate, "").unwrap();
        daemon.wait_for("web", ProcessPhase::Ready).await;
        daemon.manager.stop_all().await.unwrap();
        assert!(marker.exists());
        assert_eq!(launches(&starts), 1);
    }

    #[tokio::test]
    async fn ensure_preserves_terminal_results_after_explicit_stop() {
        let dir = tempfile::tempdir().unwrap();
        let starts = dir.path().join("worker-starts");
        let daemon = Daemon::start(
            dir.path(),
            vec![process("worker", &starts, "exit 0", false)],
            &["worker"],
        )
        .await;
        daemon.wait_for("worker", ProcessPhase::Exited).await;
        daemon
            .manager
            .process_runner()
            .stop_and_keep("worker")
            .await
            .unwrap();
        let owner = ProcessOwner::connect(&daemon.manager.api_socket_path())
            .await
            .unwrap();
        owner.start("worker", false).await.unwrap();
        assert_eq!(daemon.phase("worker").await, Some(ProcessPhase::Stopped));
        assert_eq!(
            owner.query(&["worker".to_string()]).await.unwrap()[0].phase,
            Some(ProcessPhase::Exited)
        );
        assert_eq!(launches(&starts), 1);
        daemon.manager.stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn losing_the_manager_fails_waiting_dependents() {
        let dir = tempfile::tempdir().unwrap();
        let starts = dir.path().join("web-starts");
        let marker = dir.path().join("never-ran");
        let web = || process("web", &starts, "exec tail -f /dev/null", false);
        let daemon = Daemon::start(dir.path(), vec![web()], &["web"]).await;
        let attached = daemon
            .attached(
                dir.path(),
                vec![
                    web(),
                    oneshot("test:root", &[dep("web", "@completed")], &marker),
                ],
                &["test:root"],
                tokio_shutdown::Shutdown::new(),
            )
            .await;
        let run = tokio::spawn({
            let attached = Arc::clone(&attached);
            async move { run_attached(&attached).await }
        });
        tokio::time::timeout(BOUND, async {
            while !attached
                .process_owner
                .as_ref()
                .unwrap()
                .is_handed_over("web")
                .await
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let Daemon { manager, _server } = daemon;
        drop(_server);
        manager.stop_all().await.unwrap();
        run.await.unwrap();
        assert!(matches!(
            status(&attached, "test:root").await,
            TaskStatus::Completed(TaskCompleted::DependencyFailed)
        ));
        assert!(!marker.exists());
        manager.stop_all().await.unwrap();
    }
}
