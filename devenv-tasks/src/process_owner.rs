//! Process dependencies owned by an already-running native manager.
//!
//! `devenv tasks run` keeps process tasks in its own schedule, so task
//! selection (`after`, `before`, `wantedBy`, run modes) and the typed
//! dependency edges are exactly those of the scheduler. When a manager started
//! by `devenv up` is running, though, that manager owns the process lifetimes:
//! launching a second copy crash-loops services such as PostgreSQL (#3137),
//! and stopping them when the task run ends would tear down the user's
//! environment. [`ProcessOwner`] lets the task runner hand process nodes to the
//! owner and judge edges on them against the owner's live dependency state.

use devenv_processes::{
    ApiRequest, ApiResponse, AttachEvent, DependencyInfo, NativeManagerClient, StartOutcome,
};
use miette::{Result, bail, miette};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::{Notify, RwLock};
use tokio::task::JoinHandle;

/// Bound on connecting to the manager and on replies that do no user work.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Refresh interval while the manager offers no attach stream to wake on.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// A running native process manager that owns process lifetimes for a task
/// run. Construct it with [`ProcessOwner::connect`] and hand it to
/// [`crate::TasksBuilder::with_process_owner`].
pub struct ProcessOwner {
    socket_path: PathBuf,
    /// Last observed dependency state, keyed by short process name. A name is
    /// present once the local runner has handed its node to the owner
    /// (started it, or found it already running or settled); until then
    /// dependents keep waiting, as they would on a locally `Waiting` process.
    armed: RwLock<HashMap<String, DependencyInfo>>,
    /// The manager went away mid-run; nothing it owned can change any more.
    lost: AtomicBool,
    watcher: std::sync::Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for ProcessOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessOwner")
            .field("socket_path", &self.socket_path)
            .finish_non_exhaustive()
    }
}

impl Drop for ProcessOwner {
    fn drop(&mut self) {
        if let Ok(mut watcher) = self.watcher.lock()
            && let Some(handle) = watcher.take()
        {
            handle.abort();
        }
    }
}

impl ProcessOwner {
    /// Connect to the manager listening on `socket_path` and check that it
    /// understands dependency status requests. An older daemon answers with
    /// an "unknown variant" error, which is returned as is so the caller can
    /// turn it into restart guidance.
    pub async fn connect(socket_path: &Path) -> Result<Arc<Self>> {
        let owner = Arc::new(Self {
            socket_path: socket_path.to_path_buf(),
            armed: RwLock::new(HashMap::new()),
            lost: AtomicBool::new(false),
            watcher: std::sync::Mutex::new(None),
        });
        owner.query(&[]).await?;
        Ok(owner)
    }

    /// The owner's dependency state for every name, in request order.
    pub(crate) async fn query(&self, names: &[String]) -> Result<Vec<DependencyInfo>> {
        let request = ApiRequest::DependencyStatus {
            names: names.to_vec(),
        };
        let response = tokio::time::timeout(
            REQUEST_TIMEOUT,
            NativeManagerClient::api_request(&self.socket_path, &request),
        )
        .await
        .map_err(|_| miette!("process manager did not respond in time"))??;
        match response {
            ApiResponse::DependencyStatus { processes } => Ok(processes),
            ApiResponse::Error { message } => bail!("{message}"),
            other => bail!("Unexpected response to dependency status request: {other:?}"),
        }
    }

    /// Ask the owner to start one process through its own scheduler, which
    /// resolves the process's prerequisites against the owner's live graph
    /// and skips a process that is already running or pending.
    pub(crate) async fn start(&self, name: &str) -> Result<StartOutcome> {
        let request = ApiRequest::Start {
            names: vec![name.to_string()],
        };
        match NativeManagerClient::api_request_bounded_connect(
            &self.socket_path,
            &request,
            REQUEST_TIMEOUT,
        )
        .await?
        {
            ApiResponse::Start { outcome } => Ok(outcome),
            ApiResponse::Error { message } => bail!("{message}"),
            other => bail!("Unexpected response to start request: {other:?}"),
        }
    }

    /// Hand a process node to the owner: from now on dependents are judged
    /// against the owner's state for `name`.
    pub(crate) async fn arm(&self, name: &str, notify: &Notify) -> Result<()> {
        let info = self
            .query(&[name.to_string()])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| miette!("process manager returned no status for '{name}'"))?;
        self.armed.write().await.insert(name.to_string(), info);
        notify.notify_waiters();
        Ok(())
    }

    /// Last observed state for an armed process; `None` while the local
    /// runner has not handed the process to the owner yet.
    pub(crate) async fn state(&self, name: &str) -> Option<DependencyInfo> {
        self.armed.read().await.get(name).cloned()
    }

    /// Whether the manager disappeared after the run attached to it.
    pub(crate) fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }

    /// Re-read every armed process and wake dependents when anything changed.
    async fn refresh(&self, notify: &Notify) -> Result<()> {
        let names: Vec<String> = self.armed.read().await.keys().cloned().collect();
        if names.is_empty() {
            return Ok(());
        }
        let infos = self.query(&names).await?;
        let mut changed = false;
        {
            let mut armed = self.armed.write().await;
            for info in infos {
                if let Some(previous) = armed.get_mut(&info.name)
                    && *previous != info
                {
                    *previous = info;
                    changed = true;
                }
            }
        }
        if changed {
            notify.notify_waiters();
        }
        Ok(())
    }

    /// Follow the owner's lifecycle transitions and keep the armed state
    /// current, waking `notify` (the task runner's dependency loop) on every
    /// change. Transitions arrive over the attach stream; without one the
    /// state is polled.
    pub(crate) fn watch(self: &Arc<Self>, notify: Arc<Notify>) {
        let weak = Arc::downgrade(self);
        let socket_path = self.socket_path.clone();
        let handle = tokio::spawn(async move {
            match NativeManagerClient::attach_stream(&socket_path).await {
                Ok(mut stream) => loop {
                    match stream.next().await {
                        Some(Ok(AttachEvent::Snapshot { .. } | AttachEvent::Status { .. })) => {
                            if !Self::refresh_or_mark_lost(&weak, &notify).await {
                                return;
                            }
                        }
                        Some(Ok(AttachEvent::Log { .. })) => {}
                        // The stream closes when the manager shuts down, and
                        // errors when it predates attach; tell the two apart
                        // by asking it directly.
                        Some(Err(_)) | None => break,
                    }
                },
                Err(error) => {
                    tracing::debug!(%error, "process manager attach stream unavailable");
                }
            }
            loop {
                if !Self::refresh_or_mark_lost(&weak, &notify).await {
                    return;
                }
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        });
        if let Ok(mut watcher) = self.watcher.lock() {
            *watcher = Some(handle);
        }
    }

    /// Returns whether watching should continue.
    async fn refresh_or_mark_lost(weak: &Weak<Self>, notify: &Notify) -> bool {
        let Some(owner) = weak.upgrade() else {
            return false;
        };
        match owner.refresh(notify).await {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(%error, "lost the running process manager");
                owner.lost.store(true, Ordering::Release);
                notify.notify_waiters();
                false
            }
        }
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
}
