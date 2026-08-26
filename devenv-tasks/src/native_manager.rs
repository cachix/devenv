use crate::Tasks;
use devenv_mailbox::{FrontendEvent, ProcessCommand};
use devenv_processes::{
    ApiRequest, ApiResponse, AttachEvent, LogStream, ManagerResidence, OnIdle, ProcessInfo,
    ProcessPhase, ProcessRunner, RestartOutcome,
};
use miette::{IntoDiagnostic, Result, WrapErr};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, trace, warn};

const ATTACH_WRITE_STALL_TIMEOUT: Duration = Duration::from_secs(30);
const ATTACH_BACKLOG_LINES: usize = 50;
const ATTACH_EVENT_CHANNEL_CAPACITY: usize = 2048;

#[derive(Default)]
struct AttachTailers(Vec<JoinHandle<()>>);

impl AttachTailers {
    fn push(&mut self, task: JoinHandle<()>) {
        self.0.push(task);
    }
}

impl Drop for AttachTailers {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

fn same_process_command(left: &ProcessCommand, right: &ProcessCommand) -> bool {
    matches!(
        (left, right),
        (ProcessCommand::Restart(left), ProcessCommand::Restart(right))
            | (ProcessCommand::Stop(left), ProcessCommand::Stop(right))
            if left == right
    )
}

async fn recv_process_command(rx: &mut mpsc::Receiver<FrontendEvent>) -> Option<ProcessCommand> {
    while let Some(event) = rx.recv().await {
        if let FrontendEvent::Process(command) = event {
            return Some(command);
        }
    }
    None
}

fn latest_queued_process_command(rx: &mut mpsc::Receiver<FrontendEvent>) -> Option<ProcessCommand> {
    let mut latest = None;
    while let Ok(event) = rx.try_recv() {
        if let FrontendEvent::Process(command) = event {
            latest = Some(command);
        }
    }
    latest
}

/// Persistent native process service backed by one concrete task execution scope.
///
/// `Tasks` owns the process runner used by its process tasks. Keeping only the
/// task scope here makes it impossible to compose scheduling against one runner
/// while serving process state from another.
pub struct NativeProcessManager {
    tasks: Arc<Tasks>,
    residence: ManagerResidence,
}

impl NativeProcessManager {
    pub fn new(tasks: Arc<Tasks>, residence: ManagerResidence) -> Self {
        Self { tasks, residence }
    }

    pub fn tasks(&self) -> &Arc<Tasks> {
        &self.tasks
    }

    pub fn process_runner(&self) -> &Arc<ProcessRunner> {
        self.tasks.process_runner()
    }

    pub fn residence(&self) -> ManagerResidence {
        self.residence
    }

    pub fn manager_pid_file(&self) -> PathBuf {
        self.process_runner().manager_pid_file()
    }

    pub fn api_socket_path(&self) -> PathBuf {
        self.process_runner().api_socket_path()
    }

    pub async fn stop_all(&self) -> Result<()> {
        self.process_runner().stop_all().await
    }

    /// Whether every process is terminal, ready, or waiting only on work that
    /// requires an external action.
    pub async fn wait_settled(&self) -> bool {
        for process in self.process_runner().process_infos().await {
            match process.phase {
                ProcessPhase::Starting | ProcessPhase::Stopping => return false,
                ProcessPhase::Waiting => {
                    if !self.tasks.dependency_parked(&process.name).await {
                        return false;
                    }
                }
                ProcessPhase::NotStarted
                | ProcessPhase::Stopped
                | ProcessPhase::Ready
                | ProcessPhase::Exited
                | ProcessPhase::GaveUp => {}
            }
        }
        true
    }

    async fn handle_wait(&self) -> ApiResponse {
        loop {
            let notified = self.tasks.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.wait_settled().await {
                return ApiResponse::Ready;
            }
            notified.await;
        }
    }

    pub async fn handle_command(&self, command: ProcessCommand) {
        match command {
            ProcessCommand::Restart(name) => match self.process_runner().restart(&name).await {
                Ok(RestartOutcome::RestartedInPlace) => {}
                Ok(RestartOutcome::SchedulingRequired) => {
                    let outcome = self.tasks.start_with_deps([name.clone()]).await;
                    if !outcome.scheduled.contains(&name) && !outcome.skipped.contains(&name) {
                        warn!(process = %name, "failed to start process");
                    }
                }
                Err(error) => {
                    warn!(process = %name, ?error, "failed to restart process");
                }
            },
            ProcessCommand::Stop(name) => {
                if let Err(error) = self.process_runner().stop_and_keep(&name).await {
                    warn!(process = %name, ?error, "failed to stop process");
                }
            }
            ProcessCommand::StopManager => {
                debug!("ignoring StopManager on an in-process manager");
            }
        }
    }

    pub fn start_command_listener(self: &Arc<Self>, mut rx: mpsc::Receiver<FrontendEvent>) {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            let mut queued_command = None;
            while let Some(command) = match queued_command.take() {
                Some(command) => Some(command),
                None => recv_process_command(&mut rx).await,
            } {
                manager.handle_command(command.clone()).await;
                queued_command = latest_queued_process_command(&mut rx)
                    .filter(|queued| !same_process_command(&command, queued));
            }
        });
    }

    pub async fn run_event_loop(
        self: &Arc<Self>,
        cancellation_token: CancellationToken,
        frontend_event_rx: Option<mpsc::Receiver<FrontendEvent>>,
        mode: OnIdle,
    ) -> Result<()> {
        trace!(
            mode = ?mode,
            token_cancelled = cancellation_token.is_cancelled(),
            "manager event loop started"
        );
        if let Some(rx) = frontend_event_rx {
            self.start_command_listener(rx);
        }
        let result = self
            .process_runner()
            .run_until(cancellation_token, mode)
            .await;
        info!("Manager event loop stopped");
        result
    }

    fn process_not_found(name: &str) -> ApiResponse {
        ApiResponse::Error {
            message: format!("process '{name}' not found"),
        }
    }

    async fn handle_api_client(stream: tokio::net::UnixStream, manager: Arc<Self>) {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        if reader.read_line(&mut line).await.is_err() {
            return;
        }

        let response = match serde_json::from_str::<ApiRequest>(&line) {
            Ok(ApiRequest::Wait) => manager.handle_wait().await,
            Ok(ApiRequest::List) => ApiResponse::ProcessList {
                processes: manager.process_runner().process_infos().await,
            },
            Ok(ApiRequest::Status { name }) => {
                match manager.process_runner().process_info_by_name(&name).await {
                    Some(info) => ApiResponse::ProcessDetail { info },
                    None => Self::process_not_found(&name),
                }
            }
            Ok(ApiRequest::Logs { name, lines }) => {
                let max_lines = lines.unwrap_or(100);
                if manager.process_runner().get_phase(&name).await.is_none() {
                    Self::process_not_found(&name)
                } else {
                    let (stdout_path, stderr_path) = devenv_processes::command::log_paths(
                        manager.process_runner().state_dir(),
                        &name,
                    );
                    let (stdout, stderr) = tokio::task::spawn_blocking(move || {
                        let stdout =
                            devenv_processes::log_tailer::read_tail(&stdout_path, max_lines);
                        let stderr =
                            devenv_processes::log_tailer::read_tail(&stderr_path, max_lines);
                        (stdout, stderr)
                    })
                    .await
                    .unwrap_or_default();
                    ApiResponse::ProcessLogs { stdout, stderr }
                }
            }
            Ok(ApiRequest::Restart { name }) => {
                if manager.process_runner().get_phase(&name).await.is_none() {
                    Self::process_not_found(&name)
                } else {
                    match manager.process_runner().restart(&name).await {
                        Ok(RestartOutcome::SchedulingRequired) => {
                            let outcome = manager.tasks.start_with_deps([name.clone()]).await;
                            if outcome.scheduled.contains(&name) || outcome.skipped.contains(&name)
                            {
                                ApiResponse::Ok
                            } else {
                                ApiResponse::Error {
                                    message: format!("failed to restart process '{name}'"),
                                }
                            }
                        }
                        Ok(RestartOutcome::RestartedInPlace) => ApiResponse::Ok,
                        Err(error) => ApiResponse::Error {
                            message: format!("failed to restart process '{name}': {error}"),
                        },
                    }
                }
            }
            Ok(ApiRequest::Start { names }) => ApiResponse::Start {
                outcome: manager.tasks.start_with_deps(names).await,
            },
            Ok(ApiRequest::Residence) => ApiResponse::Residence {
                residence: manager.residence(),
            },
            Ok(ApiRequest::Stop { name }) => match manager.process_runner().stop(&name).await {
                Ok(()) => ApiResponse::Ok,
                Err(error) => ApiResponse::Error {
                    message: format!("failed to stop process '{name}': {error}"),
                },
            },
            Ok(ApiRequest::Ports) => ApiResponse::PortAllocations {
                ports: manager.process_runner().port_allocations().await,
            },
            Ok(ApiRequest::Attach) => {
                Self::handle_attach_client(reader, writer, manager).await;
                return;
            }
            Err(error) => ApiResponse::Error {
                message: format!("invalid request: {error}"),
            },
        };

        if let Ok(mut json) = serde_json::to_vec(&response) {
            json.push(b'\n');
            let _ = writer.write_all(&json).await;
        }
    }

    async fn write_attach_event(
        writer: &mut tokio::net::unix::OwnedWriteHalf,
        event: &AttachEvent,
    ) -> std::io::Result<()> {
        use tokio::io::AsyncWriteExt;

        let mut json = serde_json::to_vec(event)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        json.push(b'\n');
        writer.write_all(&json).await
    }

    async fn write_attach_event_bounded(
        writer: &mut tokio::net::unix::OwnedWriteHalf,
        event: &AttachEvent,
        shutdown: &CancellationToken,
    ) -> bool {
        tokio::select! {
            result = tokio::time::timeout(
                ATTACH_WRITE_STALL_TIMEOUT,
                Self::write_attach_event(writer, event),
            ) => matches!(result, Ok(Ok(()))),
            _ = shutdown.cancelled() => false,
        }
    }

    async fn handle_attach_client(
        mut reader: tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>,
        mut writer: tokio::net::unix::OwnedWriteHalf,
        manager: Arc<Self>,
    ) {
        use tokio::io::AsyncReadExt;

        let connection = CancellationToken::new();
        let _guard = connection.clone().drop_guard();
        let snapshot = manager.process_runner().process_infos().await;

        if !Self::write_attach_event_bounded(
            &mut writer,
            &AttachEvent::InitialState {
                processes: snapshot.clone(),
            },
            manager.process_runner().shutdown_token(),
        )
        .await
        {
            return;
        }

        let (tx, mut rx) = mpsc::channel::<AttachEvent>(ATTACH_EVENT_CHANNEL_CAPACITY);
        tokio::spawn(Self::attach_feed(
            Arc::clone(&manager),
            snapshot,
            tx,
            connection.clone(),
        ));

        let mut probe = [0u8; 64];
        loop {
            tokio::select! {
                event = rx.recv() => match event {
                    Some(event) => {
                        if !Self::write_attach_event_bounded(
                            &mut writer,
                            &event,
                            manager.process_runner().shutdown_token(),
                        ).await {
                            break;
                        }
                    }
                    None => break,
                },
                read = reader.read(&mut probe) => {
                    if matches!(read, Ok(0) | Err(_)) {
                        break;
                    }
                }
                _ = manager.process_runner().shutdown_token().cancelled() => break,
            }
        }
    }

    async fn attach_feed(
        manager: Arc<Self>,
        snapshot: Vec<ProcessInfo>,
        tx: mpsc::Sender<AttachEvent>,
        connection: CancellationToken,
    ) {
        let mut tailers = AttachTailers::default();
        let mut previous: BTreeMap<String, ProcessInfo> = snapshot
            .into_iter()
            .map(|info| (info.name.clone(), info))
            .collect();
        for name in previous.keys() {
            Self::spawn_attach_tailers(
                manager.process_runner().state_dir(),
                name,
                &tx,
                &connection,
                &mut tailers,
            );
        }

        loop {
            let notified = manager.process_runner().entries_changed().notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            let current: BTreeMap<String, ProcessInfo> = manager
                .process_runner()
                .process_infos()
                .await
                .into_iter()
                .map(|info| (info.name.clone(), info))
                .collect();
            for (name, info) in &current {
                let is_new = !previous.contains_key(name);
                if previous.get(name) != Some(info)
                    && tx
                        .send(AttachEvent::Status { info: info.clone() })
                        .await
                        .is_err()
                {
                    return;
                }
                if is_new {
                    Self::spawn_attach_tailers(
                        manager.process_runner().state_dir(),
                        name,
                        &tx,
                        &connection,
                        &mut tailers,
                    );
                }
            }
            previous = current;

            tokio::select! {
                _ = notified => {}
                _ = connection.cancelled() => return,
            }
        }
    }

    fn try_send_attach_log(tx: &mpsc::Sender<AttachEvent>, event: AttachEvent) -> bool {
        match tx.try_send(event) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => true,
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        }
    }

    fn spawn_attach_tailers(
        state_dir: &Path,
        name: &str,
        tx: &mpsc::Sender<AttachEvent>,
        connection: &CancellationToken,
        tailers: &mut AttachTailers,
    ) {
        let (stdout_path, stderr_path) = devenv_processes::command::log_paths(state_dir, name);
        for (path, stream) in [
            (stdout_path, LogStream::Stdout),
            (stderr_path, LogStream::Stderr),
        ] {
            let (backlog, offset) =
                devenv_processes::log_tailer::read_backlog(&path, ATTACH_BACKLOG_LINES);
            for line in backlog {
                if !Self::try_send_attach_log(
                    tx,
                    AttachEvent::Log {
                        name: name.to_string(),
                        stream,
                        line,
                    },
                ) {
                    return;
                }
            }
            let tx = tx.clone();
            let name = name.to_string();
            tailers.push(devenv_processes::log_tailer::spawn_tail_to(
                path,
                offset,
                true,
                connection.clone(),
                move |line| {
                    Self::try_send_attach_log(
                        &tx,
                        AttachEvent::Log {
                            name: name.clone(),
                            stream,
                            line,
                        },
                    )
                },
            ));
        }
    }
}

/// Owns the native manager's published Unix socket and accept loop.
pub struct NativeApiServer {
    manager: Arc<NativeProcessManager>,
    socket_path: PathBuf,
    task: JoinHandle<()>,
}

impl NativeApiServer {
    pub fn start(manager: Arc<NativeProcessManager>) -> Result<Self> {
        let socket_path = manager.api_socket_path();
        let _ = std::fs::remove_file(&socket_path);

        let listener = std::os::unix::net::UnixListener::bind(&socket_path)
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to bind API socket at {}", socket_path.display()))?;
        listener.set_nonblocking(true).into_diagnostic()?;
        let listener = tokio::net::UnixListener::from_std(listener).into_diagnostic()?;
        info!(path = %socket_path.display(), "API server listening");

        let accept_manager = Arc::clone(&manager);
        let task = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let manager = Arc::clone(&accept_manager);
                        tokio::spawn(NativeProcessManager::handle_api_client(stream, manager));
                    }
                    Err(error) => {
                        warn!(%error, "API accept error");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        });

        Ok(Self {
            manager,
            socket_path,
            task,
        })
    }

    pub fn manager(&self) -> &Arc<NativeProcessManager> {
        &self.manager
    }
}

impl std::fmt::Debug for NativeApiServer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeApiServer")
            .field("socket_path", &self.socket_path)
            .finish_non_exhaustive()
    }
}

impl Drop for NativeApiServer {
    fn drop(&mut self) {
        self.task.abort();
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, RunMode, TaskConfig, TaskType};
    use devenv_core::VerbosityLevel;
    use devenv_processes::config::StartConfig;
    use devenv_processes::{
        ApiRequest, ApiResponse, AttachStream, NativeManagerClient, ProcessConfig, ReadyConfig,
        RestartConfig, RestartPolicy, SupervisionMode,
    };
    use std::collections::{BTreeMap, HashMap};

    async fn test_manager(
        temp_dir: &tempfile::TempDir,
        task_configs: Vec<TaskConfig>,
        residence: ManagerResidence,
    ) -> Arc<NativeProcessManager> {
        let runtime_dir = temp_dir.path().join("runtime");
        let cache_dir = temp_dir.path().join("cache");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        let config = Config {
            tasks: task_configs,
            roots: Vec::new(),
            run_mode: RunMode::All,
            runtime_dir,
            cache_dir,
            sudo_context: None,
            env: HashMap::new(),
            bash: String::new(),
            ignore_process_deps: false,
            exit_on_idle: Some(false),
            supervisor: SupervisionMode::Native,
        };
        let shutdown = tokio_shutdown::Shutdown::new();
        let tasks = Arc::new(
            Tasks::builder(config, VerbosityLevel::Normal, shutdown)
                .build()
                .await
                .unwrap(),
        );
        Arc::new(NativeProcessManager::new(tasks, residence))
    }

    fn process_task(name: &str, command: &str) -> TaskConfig {
        TaskConfig {
            name: format!("{}{}", crate::PROCESS_TASK_PREFIX, name),
            r#type: TaskType::Process,
            command: Some(command.to_string()),
            process: Some(ProcessConfig {
                start: StartConfig { enable: false },
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

    fn long_running_config(name: &str) -> ProcessConfig {
        ProcessConfig {
            name: name.to_string(),
            exec: "exec tail -f /dev/null".to_string(),
            restart: RestartConfig {
                on: RestartPolicy::Never,
                max: Some(5),
                window: None,
            },
            ..Default::default()
        }
    }

    fn auto_start_off_config(name: &str) -> ProcessConfig {
        ProcessConfig {
            start: StartConfig { enable: false },
            ..long_running_config(name)
        }
    }

    fn readiness_gated_config(name: &str, gate: &Path) -> ProcessConfig {
        ProcessConfig {
            ready: Some(ReadyConfig {
                exec: Some(format!("test -e '{}'", gate.display())),
                period: 1,
                ..Default::default()
            }),
            ..long_running_config(name)
        }
    }

    fn exit_gated_config(
        name: &str,
        gate: &Path,
        exit_code: i32,
        restart_on: RestartPolicy,
        restart_max: usize,
    ) -> ProcessConfig {
        ProcessConfig {
            name: name.to_string(),
            exec: format!(
                "while [ ! -e '{}' ]; do sleep 0.05; done; exit {exit_code}",
                gate.display()
            ),
            restart: RestartConfig {
                on: restart_on,
                max: Some(restart_max),
                window: None,
            },
            ..Default::default()
        }
    }

    async fn wait_for_phase(manager: &NativeProcessManager, name: &str, phase: ProcessPhase) {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let notified = manager.process_runner().entries_changed().notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if manager.process_runner().get_phase(name).await == Some(phase) {
                    return;
                }
                notified.await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {name} to reach {phase}"));
    }

    async fn wait_for_attach_phase(
        stream: &mut AttachStream,
        name: &str,
        expected: ProcessPhase,
    ) -> ProcessInfo {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let event = stream
                    .next()
                    .await
                    .expect("attach stream closed before status transition")
                    .expect("attach stream failed before status transition");
                if let AttachEvent::Status { info } = event
                    && info.name == name
                    && info.phase == expected
                {
                    return info;
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for attached {name} to reach {expected}"))
    }

    #[tokio::test]
    async fn process_command_queue_keeps_only_the_latest_intent() {
        let (tx, mut rx) = mpsc::channel(4);
        tx.send(FrontendEvent::Process(ProcessCommand::Restart(
            "alpha".to_string(),
        )))
        .await
        .unwrap();
        tx.send(FrontendEvent::Process(ProcessCommand::Stop(
            "alpha".to_string(),
        )))
        .await
        .unwrap();
        tx.send(FrontendEvent::Process(ProcessCommand::Restart(
            "beta".to_string(),
        )))
        .await
        .unwrap();

        let first = recv_process_command(&mut rx).await.unwrap();
        let latest = latest_queued_process_command(&mut rx).unwrap();
        assert!(matches!(first, ProcessCommand::Restart(name) if name == "alpha"));
        assert!(matches!(latest, ProcessCommand::Restart(name) if name == "beta"));
    }

    #[tokio::test]
    async fn slow_attach_log_queue_is_strictly_bounded() {
        const CAPACITY: usize = 4;
        let (tx, mut rx) = mpsc::channel(CAPACITY);
        let event = || AttachEvent::Log {
            name: "noisy".to_string(),
            stream: LogStream::Stdout,
            line: "line".to_string(),
        };

        for _ in 0..CAPACITY {
            assert!(NativeProcessManager::try_send_attach_log(&tx, event()));
        }
        assert_eq!(rx.len(), CAPACITY);
        assert_eq!(tx.capacity(), 0);

        for _ in 0..100_000 {
            assert!(
                NativeProcessManager::try_send_attach_log(&tx, event()),
                "a full queue drops logs without disconnecting a slow client"
            );
        }
        assert_eq!(rx.len(), CAPACITY);
        assert_eq!(tx.capacity(), 0);

        while rx.try_recv().is_ok() {}
        drop(rx);
        assert!(
            !NativeProcessManager::try_send_attach_log(&tx, event()),
            "a closed queue stops its tailers"
        );
    }

    #[test]
    fn repeated_process_command_is_the_same_intent() {
        assert!(same_process_command(
            &ProcessCommand::Restart("alpha".to_string()),
            &ProcessCommand::Restart("alpha".to_string()),
        ));
        assert!(!same_process_command(
            &ProcessCommand::Restart("alpha".to_string()),
            &ProcessCommand::Stop("alpha".to_string()),
        ));
    }

    #[tokio::test]
    async fn start_request_uses_the_owned_task_graph_and_runner() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = test_manager(
            &temp_dir,
            vec![process_task("web", "exec tail -f /dev/null")],
            ManagerResidence::Daemon,
        )
        .await;
        let _server = NativeApiServer::start(Arc::clone(&manager)).unwrap();

        let response = NativeManagerClient::api_request(
            &manager.api_socket_path(),
            &ApiRequest::Start {
                names: vec!["web".to_string()],
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            response,
            ApiResponse::Start { outcome }
                if outcome.scheduled == ["web"]
                    && outcome.skipped.is_empty()
                    && outcome.unknown.is_empty()
                    && outcome.failed.is_empty()
        ));
        wait_for_phase(&manager, "web", ProcessPhase::Ready).await;
        manager.stop_all().await.unwrap();
    }

    /// Closing the requesting socket must not cancel work owned by the daemon.
    #[tokio::test]
    async fn interrupted_start_client_does_not_cancel_daemon_work() {
        use tokio::io::AsyncWriteExt;

        const BOUND: Duration = Duration::from_secs(30);

        let temp_dir = tempfile::tempdir().unwrap();
        let manager = test_manager(
            &temp_dir,
            vec![process_task("slow", "exec tail -f /dev/null")],
            ManagerResidence::Daemon,
        )
        .await;
        let _server = NativeApiServer::start(Arc::clone(&manager)).unwrap();

        // Keep the real scheduler inside start_with_deps so the client can
        // disconnect while daemon-owned work is observably still pending.
        let start_lock = manager.tasks.start_with_deps_lock.lock().await;
        let mut client = tokio::net::UnixStream::connect(manager.api_socket_path())
            .await
            .unwrap();
        let mut request = serde_json::to_vec(&ApiRequest::Start {
            names: vec!["slow".to_string()],
        })
        .unwrap();
        request.push(b'\n');
        client.write_all(&request).await.unwrap();
        drop(client);

        // A blocked Start handler is per-connection and must not monopolize
        // the manager API after its client goes away.
        let response = tokio::time::timeout(
            BOUND,
            NativeManagerClient::api_request(&manager.api_socket_path(), &ApiRequest::List),
        )
        .await
        .expect("manager stopped responding after Start client interruption")
        .unwrap();
        assert!(matches!(response, ApiResponse::ProcessList { .. }));

        drop(start_lock);
        wait_for_phase(&manager, "slow", ProcessPhase::Ready).await;
        manager.stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn residence_round_trips_over_the_owned_server() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = test_manager(&temp_dir, Vec::new(), ManagerResidence::Daemon).await;
        let _server = NativeApiServer::start(Arc::clone(&manager)).unwrap();

        assert_eq!(
            NativeManagerClient::query_manager_residence(&manager.api_socket_path()).await,
            Some(ManagerResidence::Daemon)
        );
    }

    #[tokio::test]
    async fn attach_snapshot_is_sorted_and_preserves_all_manager_phases() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = test_manager(&temp_dir, Vec::new(), ManagerResidence::InProcess).await;
        let readiness_gate = temp_dir.path().join("snapshot-starting-ready");
        let runner = manager.process_runner();

        runner
            .start_command(&long_running_config("active"), None)
            .await
            .unwrap();

        let mut idle = auto_start_off_config("idle");
        idle.ports.insert("http".to_string(), 48_123);
        runner.register_waiting(idle, None).await;
        runner.launch_waiting("idle").await.unwrap();

        runner
            .register_waiting(long_running_config("waiting"), None)
            .await;
        runner
            .start_command(&long_running_config("stopped"), None)
            .await
            .unwrap();
        runner.stop_and_keep("stopped").await.unwrap();
        runner
            .start_command(&readiness_gated_config("starting", &readiness_gate), None)
            .await
            .unwrap();
        runner
            .start_command(
                &ProcessConfig {
                    name: "exited".to_string(),
                    exec: "true".to_string(),
                    restart: RestartConfig {
                        on: RestartPolicy::Never,
                        max: Some(0),
                        window: None,
                    },
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();
        wait_for_phase(&manager, "exited", ProcessPhase::Exited).await;

        runner
            .start_command(
                &ProcessConfig {
                    name: "gave-up".to_string(),
                    exec: "false".to_string(),
                    restart: RestartConfig {
                        on: RestartPolicy::OnFailure,
                        max: Some(1),
                        window: None,
                    },
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();
        wait_for_phase(&manager, "gave-up", ProcessPhase::GaveUp).await;

        let _server = NativeApiServer::start(Arc::clone(&manager)).unwrap();
        let mut stream = NativeManagerClient::attach_stream(&manager.api_socket_path())
            .await
            .unwrap();
        let event = tokio::time::timeout(Duration::from_secs(30), stream.next())
            .await
            .expect("timed out waiting for attach snapshot")
            .expect("attach stream closed")
            .expect("attach snapshot failed");
        let AttachEvent::InitialState { processes } = event else {
            panic!("snapshot must be the first attach event");
        };

        assert_eq!(
            processes
                .iter()
                .map(|info| info.name.as_str())
                .collect::<Vec<_>>(),
            [
                "active", "exited", "gave-up", "idle", "starting", "stopped", "waiting"
            ]
        );
        let by_name: BTreeMap<_, _> = processes
            .into_iter()
            .map(|info| (info.name.clone(), info))
            .collect();
        assert_eq!(by_name["active"].phase, ProcessPhase::Ready);
        assert_eq!(by_name["exited"].phase, ProcessPhase::Exited);
        assert_eq!(by_name["gave-up"].phase, ProcessPhase::GaveUp);
        assert_eq!(by_name["gave-up"].restart_count, 1);
        assert_eq!(by_name["idle"].phase, ProcessPhase::NotStarted);
        assert_eq!(by_name["idle"].ports, ["http:48123"]);
        assert_eq!(by_name["starting"].phase, ProcessPhase::Starting);
        assert_eq!(by_name["stopped"].phase, ProcessPhase::Stopped);
        assert_eq!(by_name["waiting"].phase, ProcessPhase::Waiting);

        drop(stream);
        for (name, phase) in [
            ("active", ProcessPhase::Ready),
            ("exited", ProcessPhase::Exited),
            ("gave-up", ProcessPhase::GaveUp),
            ("idle", ProcessPhase::NotStarted),
            ("starting", ProcessPhase::Starting),
            ("stopped", ProcessPhase::Stopped),
            ("waiting", ProcessPhase::Waiting),
        ] {
            assert_eq!(
                runner.get_phase(name).await,
                Some(phase),
                "disconnecting an observer must not mutate {name}"
            );
        }

        std::fs::write(&readiness_gate, "").unwrap();
        wait_for_phase(&manager, "starting", ProcessPhase::Ready).await;
        runner.launch_waiting("waiting").await.unwrap();
        wait_for_phase(&manager, "waiting", ProcessPhase::Ready).await;
        runner.start_not_started("stopped").await.unwrap();
        wait_for_phase(&manager, "stopped", ProcessPhase::Ready).await;
        assert_eq!(runner.get_phase("exited").await, Some(ProcessPhase::Exited));
        assert_eq!(
            runner.get_phase("gave-up").await,
            Some(ProcessPhase::GaveUp)
        );

        manager.stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn attach_stream_reports_live_nonterminal_and_terminal_transitions() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = test_manager(&temp_dir, Vec::new(), ManagerResidence::InProcess).await;
        let ready_gate = temp_dir.path().join("live-ready");
        let exit_gate = temp_dir.path().join("live-exit");
        let gave_up_gate = temp_dir.path().join("live-gave-up");
        let runner = manager.process_runner();

        runner
            .register_waiting(long_running_config("waiting-live"), None)
            .await;
        runner
            .start_command(&readiness_gated_config("starting-live", &ready_gate), None)
            .await
            .unwrap();
        runner
            .start_command(
                &exit_gated_config("exit-live", &exit_gate, 0, RestartPolicy::Never, 0),
                None,
            )
            .await
            .unwrap();
        runner
            .start_command(
                &exit_gated_config(
                    "gave-up-live",
                    &gave_up_gate,
                    1,
                    RestartPolicy::OnFailure,
                    0,
                ),
                None,
            )
            .await
            .unwrap();

        let _server = NativeApiServer::start(Arc::clone(&manager)).unwrap();
        let mut stream = NativeManagerClient::attach_stream(&manager.api_socket_path())
            .await
            .unwrap();
        let snapshot = tokio::time::timeout(Duration::from_secs(30), stream.next())
            .await
            .expect("timed out waiting for live-transition snapshot")
            .expect("live-transition attach stream closed")
            .expect("live-transition snapshot failed");
        let AttachEvent::InitialState { processes } = snapshot else {
            panic!("snapshot must be the first attach event");
        };
        let by_name: BTreeMap<_, _> = processes
            .into_iter()
            .map(|info| (info.name.clone(), info))
            .collect();
        assert_eq!(by_name["waiting-live"].phase, ProcessPhase::Waiting);
        assert_eq!(by_name["starting-live"].phase, ProcessPhase::Starting);
        assert_eq!(by_name["exit-live"].phase, ProcessPhase::Ready);
        assert_eq!(by_name["gave-up-live"].phase, ProcessPhase::Ready);

        runner.launch_waiting("waiting-live").await.unwrap();
        wait_for_attach_phase(&mut stream, "waiting-live", ProcessPhase::Ready).await;
        std::fs::write(&ready_gate, "").unwrap();
        wait_for_attach_phase(&mut stream, "starting-live", ProcessPhase::Ready).await;
        std::fs::write(&exit_gate, "").unwrap();
        wait_for_attach_phase(&mut stream, "exit-live", ProcessPhase::Exited).await;
        std::fs::write(&gave_up_gate, "").unwrap();
        wait_for_attach_phase(&mut stream, "gave-up-live", ProcessPhase::GaveUp).await;

        drop(stream);
        for (name, phase) in [
            ("waiting-live", ProcessPhase::Ready),
            ("starting-live", ProcessPhase::Ready),
            ("exit-live", ProcessPhase::Exited),
            ("gave-up-live", ProcessPhase::GaveUp),
        ] {
            assert_eq!(runner.get_phase(name).await, Some(phase));
        }
        manager.stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn disconnecting_one_attach_client_does_not_affect_another() {
        const BOUND: Duration = Duration::from_secs(30);

        async fn expect_snapshot(stream: &mut AttachStream) {
            let event = tokio::time::timeout(BOUND, stream.next())
                .await
                .expect("timed out waiting for snapshot")
                .expect("stream closed")
                .expect("snapshot failed");
            assert!(matches!(event, AttachEvent::InitialState { .. }));
        }

        let temp_dir = tempfile::tempdir().unwrap();
        let manager = test_manager(&temp_dir, Vec::new(), ManagerResidence::InProcess).await;
        manager
            .process_runner()
            .start_command(&long_running_config("shared"), None)
            .await
            .unwrap();
        let _server = NativeApiServer::start(Arc::clone(&manager)).unwrap();

        let mut first = NativeManagerClient::attach_stream(&manager.api_socket_path())
            .await
            .unwrap();
        let mut second = NativeManagerClient::attach_stream(&manager.api_socket_path())
            .await
            .unwrap();
        expect_snapshot(&mut first).await;
        expect_snapshot(&mut second).await;
        drop(first);

        let (stdout_path, _) =
            devenv_processes::command::log_paths(manager.process_runner().state_dir(), "shared");
        {
            use std::io::Write;
            let mut stdout = std::fs::OpenOptions::new()
                .append(true)
                .open(stdout_path)
                .unwrap();
            stdout.write_all(b"after-first-disconnect\n").unwrap();
        }

        loop {
            let event = tokio::time::timeout(BOUND, second.next())
                .await
                .expect("second client stopped receiving logs")
                .expect("second stream closed")
                .expect("second stream failed");
            if matches!(
                event,
                AttachEvent::Log {
                    ref name,
                    stream: LogStream::Stdout,
                    ref line,
                } if name == "shared" && line == "after-first-disconnect"
            ) {
                break;
            }
        }

        manager
            .process_runner()
            .stop_and_keep("shared")
            .await
            .unwrap();
        wait_for_attach_phase(&mut second, "shared", ProcessPhase::Stopped).await;
        assert_eq!(
            manager.process_runner().get_phase("shared").await,
            Some(ProcessPhase::Stopped)
        );
    }

    #[tokio::test]
    async fn repeated_attach_disconnect_has_no_duplicate_logs_or_process_mutation() {
        const BOUND: Duration = Duration::from_secs(30);
        const CYCLES: usize = 10;

        let temp_dir = tempfile::tempdir().unwrap();
        let manager = test_manager(&temp_dir, Vec::new(), ManagerResidence::InProcess).await;
        manager
            .process_runner()
            .start_command(&long_running_config("stable"), None)
            .await
            .unwrap();
        let _server = NativeApiServer::start(Arc::clone(&manager)).unwrap();
        let (stdout_path, _) =
            devenv_processes::command::log_paths(manager.process_runner().state_dir(), "stable");

        for cycle in 0..CYCLES {
            let marker = format!("attach-cycle-{cycle}");
            let mut stream = NativeManagerClient::attach_stream(&manager.api_socket_path())
                .await
                .unwrap();
            let snapshot = tokio::time::timeout(BOUND, stream.next())
                .await
                .expect("timed out waiting for repeated-attach snapshot")
                .expect("repeated attach stream closed")
                .expect("repeated attach snapshot failed");
            assert!(matches!(snapshot, AttachEvent::InitialState { .. }));

            {
                use std::io::Write;
                let mut stdout = std::fs::OpenOptions::new()
                    .append(true)
                    .open(&stdout_path)
                    .unwrap();
                writeln!(stdout, "{marker}").unwrap();
            }

            loop {
                let event = tokio::time::timeout(BOUND, stream.next())
                    .await
                    .expect("repeated attach stopped receiving logs")
                    .expect("repeated attach stream closed")
                    .expect("repeated attach stream failed");
                if matches!(
                    event,
                    AttachEvent::Log {
                        ref name,
                        stream: LogStream::Stdout,
                        ref line,
                    } if name == "stable" && line == &marker
                ) {
                    break;
                }
            }

            if let Ok(Some(Ok(AttachEvent::Log { name, line, .. }))) =
                tokio::time::timeout(Duration::from_millis(250), stream.next()).await
            {
                assert!(
                    name != "stable" || line != marker,
                    "live line was duplicated in attach cycle {cycle}"
                );
            }

            drop(stream);
            assert_eq!(
                manager.process_runner().get_phase("stable").await,
                Some(ProcessPhase::Ready),
                "observer cycle {cycle} mutated the manager-owned process"
            );
        }

        manager.stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn non_reading_attach_client_does_not_block_manager_or_peer() {
        use tokio::io::AsyncWriteExt;

        const BOUND: Duration = Duration::from_secs(30);
        const BULK_LINES: usize = ATTACH_EVENT_CHANNEL_CAPACITY * 4;

        let temp_dir = tempfile::tempdir().unwrap();
        let manager = test_manager(&temp_dir, Vec::new(), ManagerResidence::InProcess).await;
        manager
            .process_runner()
            .start_command(&long_running_config("noisy"), None)
            .await
            .unwrap();
        let _server = NativeApiServer::start(Arc::clone(&manager)).unwrap();

        let mut slow = tokio::net::UnixStream::connect(manager.api_socket_path())
            .await
            .unwrap();
        slow.write_all(b"{\"command\":\"attach\"}\n").await.unwrap();

        let (stdout_path, _) =
            devenv_processes::command::log_paths(manager.process_runner().state_dir(), "noisy");
        {
            use std::io::{BufWriter, Write};
            let stdout = std::fs::OpenOptions::new()
                .append(true)
                .open(&stdout_path)
                .unwrap();
            let mut stdout = BufWriter::new(stdout);
            let payload = "x".repeat(512);
            for index in 0..BULK_LINES {
                writeln!(stdout, "bulk-{index:05}-{payload}").unwrap();
            }
            stdout.flush().unwrap();
        }
        tokio::time::sleep(Duration::from_millis(500)).await;

        let response = tokio::time::timeout(
            BOUND,
            NativeManagerClient::api_request(&manager.api_socket_path(), &ApiRequest::List),
        )
        .await
        .expect("slow attach consumer blocked the manager API")
        .unwrap();
        assert!(matches!(response, ApiResponse::ProcessList { .. }));

        let mut peer = NativeManagerClient::attach_stream(&manager.api_socket_path())
            .await
            .unwrap();
        let snapshot = tokio::time::timeout(BOUND, peer.next())
            .await
            .expect("responsive peer did not receive a snapshot")
            .expect("responsive peer closed")
            .expect("responsive peer snapshot failed");
        assert!(matches!(snapshot, AttachEvent::InitialState { .. }));

        {
            use std::io::Write;
            let mut stdout = std::fs::OpenOptions::new()
                .append(true)
                .open(&stdout_path)
                .unwrap();
            writeln!(stdout, "peer-live-marker").unwrap();
        }
        loop {
            let event = tokio::time::timeout(BOUND, peer.next())
                .await
                .expect("responsive peer stopped receiving live logs")
                .expect("responsive peer closed")
                .expect("responsive peer failed");
            if matches!(
                event,
                AttachEvent::Log {
                    ref name,
                    stream: LogStream::Stdout,
                    ref line,
                } if name == "noisy" && line == "peer-live-marker"
            ) {
                break;
            }
        }

        manager
            .process_runner()
            .stop_and_keep("noisy")
            .await
            .unwrap();
        wait_for_attach_phase(&mut peer, "noisy", ProcessPhase::Stopped).await;

        drop(slow);
        drop(peer);
    }

    #[tokio::test]
    async fn attach_stream_end_to_end() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        const BOUND: Duration = Duration::from_secs(30);

        async fn next_event(
            lines: &mut tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
        ) -> AttachEvent {
            let line = tokio::time::timeout(BOUND, lines.next_line())
                .await
                .expect("timed out waiting for attach event")
                .expect("attach stream read failed")
                .expect("attach stream closed unexpectedly");
            serde_json::from_str(&line).expect("invalid attach event")
        }

        let temp_dir = tempfile::tempdir().unwrap();
        let manager = test_manager(&temp_dir, Vec::new(), ManagerResidence::InProcess).await;
        manager
            .process_runner()
            .start_command(
                &ProcessConfig {
                    name: "attach-proc".to_string(),
                    exec: "echo attach-line; exec tail -f /dev/null".to_string(),
                    restart: RestartConfig {
                        on: RestartPolicy::Never,
                        max: Some(0),
                        window: None,
                    },
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();
        let _server = NativeApiServer::start(Arc::clone(&manager)).unwrap();

        let mut stream = tokio::net::UnixStream::connect(manager.api_socket_path())
            .await
            .unwrap();
        stream
            .write_all(b"{\"command\":\"attach\"}\n")
            .await
            .unwrap();
        let (reader, _writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();

        match next_event(&mut lines).await {
            AttachEvent::InitialState { processes } => assert!(
                processes.iter().any(|info| info.name == "attach-proc"),
                "snapshot must contain attach-proc: {processes:?}"
            ),
            other => panic!("expected snapshot first, got {other:?}"),
        }

        loop {
            if matches!(
                next_event(&mut lines).await,
                AttachEvent::Log {
                    ref name,
                    stream: LogStream::Stdout,
                    ref line,
                } if name == "attach-proc" && line == "attach-line"
            ) {
                break;
            }
        }

        let (stdout_log, _) = devenv_processes::command::log_paths(
            manager.process_runner().state_dir(),
            "attach-proc",
        );
        {
            use std::io::Write;
            let mut stdout = std::fs::OpenOptions::new()
                .append(true)
                .open(&stdout_log)
                .unwrap();
            stdout.write_all(b"live-line\n").unwrap();
        }
        loop {
            if let AttachEvent::Log {
                name,
                stream: LogStream::Stdout,
                line,
            } = next_event(&mut lines).await
                && name == "attach-proc"
            {
                if line == "live-line" {
                    break;
                }
                assert_ne!(line, "attach-line", "backlog line must not be re-emitted");
            }
        }

        manager
            .process_runner()
            .stop_and_keep("attach-proc")
            .await
            .unwrap();
        loop {
            if matches!(
                next_event(&mut lines).await,
                AttachEvent::Status { ref info }
                    if info.name == "attach-proc" && info.phase == ProcessPhase::Stopped
            ) {
                break;
            }
        }

        manager.process_runner().shutdown_supervisors();
        loop {
            let line = tokio::time::timeout(BOUND, lines.next_line())
                .await
                .expect("timed out waiting for stream EOF")
                .expect("attach stream read failed");
            match line {
                None => break,
                Some(line) => {
                    let _: AttachEvent =
                        serde_json::from_str(&line).expect("invalid event before EOF");
                }
            }
        }
    }

    #[tokio::test]
    async fn list_logs_ports_and_attach_use_the_owned_runner() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = test_manager(&temp_dir, Vec::new(), ManagerResidence::InProcess).await;
        let config = ProcessConfig {
            name: "web".to_string(),
            exec: "echo attached; exec tail -f /dev/null".to_string(),
            ports: HashMap::from([("http".to_string(), 8080)]),
            restart: RestartConfig {
                on: RestartPolicy::Never,
                max: Some(0),
                window: None,
            },
            ..Default::default()
        };
        manager
            .process_runner()
            .start_command(&config, None)
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let (stdout_path, _) = devenv_processes::command::log_paths(
                    manager.process_runner().state_dir(),
                    "web",
                );
                if std::fs::read_to_string(stdout_path)
                    .is_ok_and(|stdout| stdout.contains("attached"))
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("timed out waiting for process output");
        let _server = NativeApiServer::start(Arc::clone(&manager)).unwrap();

        let list = NativeManagerClient::api_request(&manager.api_socket_path(), &ApiRequest::List)
            .await
            .unwrap();
        assert!(matches!(
            list,
            ApiResponse::ProcessList { processes }
                if processes.len() == 1 && processes[0].name == "web"
        ));

        let ports =
            NativeManagerClient::api_request(&manager.api_socket_path(), &ApiRequest::Ports)
                .await
                .unwrap();
        assert!(matches!(
            ports,
            ApiResponse::PortAllocations { ports }
                if ports.len() == 1
                    && ports[0].process_name == "web"
                    && ports[0].port_name == "http"
                    && ports[0].port == 8080
        ));

        let logs = NativeManagerClient::api_request(
            &manager.api_socket_path(),
            &ApiRequest::Logs {
                name: "web".to_string(),
                lines: Some(10),
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            logs,
            ApiResponse::ProcessLogs { stdout, .. } if stdout.contains("attached")
        ));

        let mut attach = NativeManagerClient::attach_stream(&manager.api_socket_path())
            .await
            .unwrap();
        let snapshot = tokio::time::timeout(Duration::from_secs(30), attach.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(
            snapshot,
            AttachEvent::InitialState { processes }
                if processes.len() == 1 && processes[0].name == "web"
        ));

        manager.stop_all().await.unwrap();
    }
}
