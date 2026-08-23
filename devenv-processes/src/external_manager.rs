//! Launcher and lifecycle adapter for external process managers.
//!
//! Runs the Nix-built launcher for the configured manager and tracks the
//! resulting process scope without depending on a manager-specific protocol.

use async_trait::async_trait;
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::fs;
use tokio::process::Command;
use tracing::info;

use crate::pid::{self, PidStatus};
use crate::{
    BackgroundStartRequest, ManagerCapabilities, ManagerDescriptor, PreparedProcessScope,
    ProcessManagerControl, ProcessScope, StopPolicy, stop_process_scopes,
};

const PID_FILE_NAME: &str = "processes.pid";
const STATE_FILE_NAME: &str = "external-manager.json";
const RUNTIME_PID_FILE_NAME: &str = "external-manager.pid";
const LIFECYCLE_LOCK_FILE_NAME: &str = "external-manager.lock";
const STATE_VERSION: u32 = 1;
const BACKGROUND_ENV: &str = "DEVENV_PROCESS_MANAGER_BACKGROUND";

#[derive(Clone, Debug)]
struct LaunchConfiguration {
    manager: ManagerDescriptor,
    launcher_script: PathBuf,
    shutdown_script: Option<PathBuf>,
}

/// Durable state needed to stop an external manager without re-evaluating the
/// project's current Nix configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct ExternalManagerState {
    version: u32,
    manager_id: String,
    capabilities: ManagerCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shutdown_script: Option<PathBuf>,
    scope: ProcessScope,
}

/// Launches and tracks a process manager implemented by an external program.
pub struct ExternalManager {
    launch: Option<LaunchConfiguration>,
    /// Persistent directory for the wrapper, logs, and legacy PID state
    state_dir: PathBuf,
    /// Short-lived directory shared by process-manager invocations
    runtime_dir: PathBuf,
}

impl ExternalManager {
    /// Create an external manager that can launch a configured backend.
    ///
    /// # Arguments
    /// * `manager` - Selected backend identity and negotiated capabilities
    /// * `launcher_script` - Path to the Nix-built manager launcher
    /// * `shutdown_script` - Optional Nix-built manager-aware stop adapter
    /// * `state_dir` - Persistent directory for the wrapper, logs, and legacy PID file
    /// * `runtime_dir` - Runtime directory for current manager identity
    pub fn new(
        manager: ManagerDescriptor,
        launcher_script: PathBuf,
        shutdown_script: Option<PathBuf>,
        state_dir: PathBuf,
        runtime_dir: PathBuf,
    ) -> Self {
        Self {
            launch: Some(LaunchConfiguration {
                manager,
                launcher_script,
                shutdown_script,
            }),
            state_dir,
            runtime_dir,
        }
    }

    /// Open persisted detached state for status and stop operations.
    pub fn control(state_dir: PathBuf, runtime_dir: PathBuf) -> Self {
        Self {
            launch: None,
            state_dir,
            runtime_dir,
        }
    }

    /// Path to the legacy compatibility PID marker.
    pub fn pid_file(&self) -> PathBuf {
        self.state_dir.join(PID_FILE_NAME)
    }

    fn runtime_pid_file(&self) -> PathBuf {
        self.runtime_dir.join(RUNTIME_PID_FILE_NAME)
    }

    #[cfg(unix)]
    fn lock_lifecycle(&self) -> Result<nix::fcntl::Flock<std::fs::File>> {
        use nix::fcntl::{Flock, FlockArg};

        std::fs::create_dir_all(&self.runtime_dir).into_diagnostic()?;
        let path = self.runtime_dir.join(LIFECYCLE_LOCK_FILE_NAME);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to open lifecycle lock {}", path.display()))?;
        Flock::lock(file, FlockArg::LockExclusive).map_err(|(_, error)| {
            miette::miette!(
                "Failed to acquire external-manager lifecycle lock {}: {}",
                path.display(),
                error
            )
        })
    }

    /// Whether current or legacy detached external-manager state exists.
    pub fn state_exists(state_dir: &Path, runtime_dir: &Path) -> bool {
        runtime_dir.join(STATE_FILE_NAME).exists() || state_dir.join(PID_FILE_NAME).exists()
    }

    fn state_file(&self) -> PathBuf {
        self.runtime_dir.join(STATE_FILE_NAME)
    }

    /// Load current manager state, falling back to the legacy PID format.
    async fn load_state(&self) -> Result<Option<ExternalManagerState>> {
        let state = match fs::read(self.state_file()).await {
            Ok(bytes) => {
                let state = serde_json::from_slice::<ExternalManagerState>(&bytes)
                    .into_diagnostic()
                    .wrap_err("Failed to read external-manager state")?;
                if state.version != STATE_VERSION {
                    bail!(
                        "Unsupported external-manager state version {} (expected {})",
                        state.version,
                        STATE_VERSION
                    );
                }
                Some(state)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).into_diagnostic(),
        };

        if let Some(state) = state {
            self.validate_legacy_pid(&state.scope).await?;
            return Ok(Some(state));
        }

        if !self.pid_file().exists() {
            return Ok(None);
        }

        let pid = pid::read_pid(&self.pid_file()).await?;
        // Compatibility with state written before durable scopes were persisted.
        // It cannot protect against PID reuse, but retains the old
        // process-group cleanup behavior.
        ProcessScope::legacy_unix_process_group(pid.as_raw())
            .map(Self::legacy_state)
            .map(Some)
            .into_diagnostic()
    }

    fn legacy_state(scope: ProcessScope) -> ExternalManagerState {
        ExternalManagerState {
            version: STATE_VERSION,
            manager_id: "unknown".to_string(),
            capabilities: ManagerCapabilities::default(),
            shutdown_script: None,
            scope,
        }
    }

    async fn validate_legacy_pid(&self, scope: &ProcessScope) -> Result<()> {
        if self.pid_file().exists() {
            let pid = pid::read_pid(&self.pid_file()).await?;
            if pid.as_raw() != scope.leader_pid() {
                bail!("Process-scope identity does not match PID file {}", pid);
            }
        }
        Ok(())
    }

    async fn remove_state(&self) -> Result<()> {
        for path in [self.pid_file(), self.state_file(), self.runtime_pid_file()] {
            match fs::remove_file(path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error).into_diagnostic(),
            }
        }
        Ok(())
    }

    /// Validate both the legacy PID marker and, when present, the durable
    /// process-scope identity written by current versions.
    async fn check_status(&self) -> Result<PidStatus> {
        let Some(state) = self.load_state().await? else {
            return Ok(PidStatus::NotFound);
        };
        if state.scope.is_alive() {
            return Ok(PidStatus::Running(Pid::from_raw(state.scope.leader_pid())));
        }

        self.remove_state().await?;
        Ok(PidStatus::StaleRemoved)
    }

    /// Path to the log file
    pub fn log_file(&self) -> PathBuf {
        self.state_dir.join("processes.log")
    }

    /// Path to the wrapper script
    fn wrapper_script(&self) -> PathBuf {
        self.state_dir.join("processes")
    }

    /// Prepare the command used to follow an external manager in this client.
    ///
    /// Returns a `std::process::Command` ready to be exec'd by the caller
    /// after terminal cleanup.
    pub async fn prepare_follow_command(
        &self,
        processes: &[String],
        env: &HashMap<String, String>,
    ) -> Result<std::process::Command> {
        match self.check_status().await? {
            PidStatus::Running(pid) => {
                bail!(
                    "Processes already running with PID {}. Stop them first with: devenv processes down",
                    pid
                );
            }
            PidStatus::NotFound | PidStatus::StaleRemoved => {}
        }

        let launch = self
            .launch
            .as_ref()
            .ok_or_else(|| miette::miette!("external manager has no launch configuration"))?;
        self.write_wrapper_script().await?;

        let wrapper = self.wrapper_script();
        let mut cmd = std::process::Command::new("bash");
        cmd.arg(&wrapper)
            .arg(&launch.launcher_script)
            .args(processes);

        if !env.is_empty() {
            cmd.env_clear().envs(env);
        }

        Ok(cmd)
    }

    /// Write the wrapper script that invokes the configured external manager.
    async fn write_wrapper_script(&self) -> Result<()> {
        // Pass both the store path and process names as argv. Embedding them in
        // shell text would reinterpret whitespace and shell metacharacters.
        let script = "#!/usr/bin/env bash\nexec \"$@\"\n";

        let wrapper = self.wrapper_script();
        fs::write(&wrapper, script).await.into_diagnostic()?;
        fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))
            .await
            .into_diagnostic()?;

        Ok(())
    }
    /// Start the configured external manager in an independent OS process
    /// scope and return after its detached state has been published.
    pub async fn start_background(&self, request: BackgroundStartRequest) -> Result<()> {
        // Serialize the check, launch, and state publication with `down` and
        // other cold starts from concurrent CLI processes.
        #[cfg(unix)]
        let _lifecycle_lock = self.lock_lifecycle()?;

        let launch = self
            .launch
            .as_ref()
            .ok_or_else(|| miette::miette!("external manager has no launch configuration"))?;

        // Check if already running
        match self.check_status().await? {
            PidStatus::Running(pid) => {
                bail!(
                    "Processes already running with PID {}. Stop them first with: devenv processes down",
                    pid
                );
            }
            PidStatus::NotFound | PidStatus::StaleRemoved => {}
        }

        // Write the manager-neutral argv-preserving wrapper.
        self.write_wrapper_script().await?;

        let wrapper = self.wrapper_script();
        let mut cmd = Command::new("bash");
        cmd.arg(&wrapper)
            .arg(&launch.launcher_script)
            .args(&request.processes);

        // Set up environment
        if !request.env.is_empty() {
            cmd.env_clear().envs(&request.env);
        }

        // Manager-specific Nix launchers may translate this generic residence
        // hint into their own flags or environment variables.
        cmd.env(BACKGROUND_ENV, "1");
        cmd.stdin(Stdio::null());
        let prepared_scope = PreparedProcessScope::prepare_tokio(&mut cmd).into_diagnostic()?;

        let mut process = if request.log_to_file {
            let log_file = std::fs::File::create(self.log_file()).into_diagnostic()?;
            cmd.stdout(log_file.try_clone().into_diagnostic()?)
                .stderr(log_file)
                .spawn()
                .into_diagnostic()?
        } else {
            cmd.stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .into_diagnostic()?
        };

        let pid = process
            .id()
            .ok_or_else(|| miette::miette!("Failed to get process ID"))?;
        let scope = prepared_scope.capture(pid).into_diagnostic()?;
        let state = ExternalManagerState {
            version: STATE_VERSION,
            manager_id: launch.manager.id.clone(),
            capabilities: launch.manager.capabilities,
            shutdown_script: launch.shutdown_script.clone(),
            scope: scope.clone(),
        };
        let state_json = serde_json::to_vec(&state).into_diagnostic()?;
        let persist_result = async {
            fs::create_dir_all(&self.runtime_dir)
                .await
                .into_diagnostic()?;
            let temporary_state = self
                .runtime_dir
                .join(format!(".{STATE_FILE_NAME}.{pid}.tmp"));
            fs::write(&temporary_state, state_json)
                .await
                .into_diagnostic()?;
            fs::rename(&temporary_state, self.state_file())
                .await
                .into_diagnostic()?;
            pid::write_pid(&self.runtime_pid_file(), pid).await?;

            // Older devenv clients only know `.devenv/processes.pid`.
            // Point that legacy marker at runtime state so it becomes
            // harmlessly dangling when the runtime directory is cleared.
            let runtime_pid_file = fs::canonicalize(self.runtime_pid_file())
                .await
                .into_diagnostic()?;
            match fs::remove_file(self.pid_file()).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error).into_diagnostic(),
            }
            std::os::unix::fs::symlink(runtime_pid_file, self.pid_file()).into_diagnostic()
        }
        .await;
        if let Err(error) = persist_result {
            let cleanup_scope = scope.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let _ = cleanup_scope.force_kill();
            })
            .await;
            let _ = process.wait().await;
            let _ = self.remove_state().await;
            return Err(error);
        }

        // A launcher can fail immediately after spawn (for example because
        // it requires a TTY). Do not publish a successful detached start
        // once the complete scope has already disappeared.
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        if let Some(status) = process.try_wait().into_diagnostic()? {
            if !scope.is_alive() {
                self.remove_state().await?;
                bail!(
                    "process manager '{}' exited during background start with status {}{}",
                    launch.manager.id,
                    status,
                    if request.log_to_file {
                        format!("; see {}", self.log_file().display())
                    } else {
                        String::new()
                    }
                );
            }
        }

        info!("PID is {}", pid);
        if request.log_to_file {
            info!("See logs:  $ tail -f {}", self.log_file().display());
        }
        info!("Stop:      $ devenv processes stop");
        Ok(())
    }
}

#[async_trait]
impl ProcessManagerControl for ExternalManager {
    async fn stop(&self) -> Result<()> {
        #[cfg(unix)]
        let _lifecycle_lock = self.lock_lifecycle()?;

        let Some(state) = self.load_state().await? else {
            bail!("No processes running (process state not found)");
        };
        info!(
            manager = %state.manager_id,
            "Stopping process with PID {}",
            state.scope.leader_pid()
        );

        if state.capabilities.manager_aware_stop {
            if let Some(shutdown_script) = &state.shutdown_script {
                let status = tokio::time::timeout(
                    std::time::Duration::from_secs(10),
                    Command::new(shutdown_script)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status(),
                )
                .await;
                match status {
                    Ok(Ok(status)) if status.success() => {
                        info!(manager = %state.manager_id, "manager-aware shutdown completed");
                    }
                    Ok(Ok(status)) => tracing::warn!(
                        manager = %state.manager_id,
                        %status,
                        "manager-aware shutdown failed; falling back to process-scope cleanup"
                    ),
                    Ok(Err(error)) => tracing::warn!(
                        manager = %state.manager_id,
                        %error,
                        "manager-aware shutdown could not start; falling back to process-scope cleanup"
                    ),
                    Err(_) => tracing::warn!(
                        manager = %state.manager_id,
                        "manager-aware shutdown timed out; falling back to process-scope cleanup"
                    ),
                }
            } else {
                tracing::warn!(
                    manager = %state.manager_id,
                    "manager-aware stop was advertised without an adapter; falling back to process-scope cleanup"
                );
            }
        }

        let scope = state.scope;
        tokio::task::spawn_blocking(move || {
            stop_process_scopes(
                [scope],
                StopPolicy {
                    signal: Signal::SIGTERM as i32,
                    grace: std::time::Duration::from_secs(30),
                },
            )
        })
        .await
        .into_diagnostic()?
        .into_diagnostic()?;

        self.remove_state().await
    }

    async fn is_running(&self) -> bool {
        matches!(self.check_status().await, Ok(PidStatus::Running(_)))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn write_executable_script(directory: &Path, name: &str, body: &str) -> PathBuf {
        let path = directory.join(name);
        std::fs::write(&path, format!("#!/bin/bash\nset -e\n{body}\n"))
            .expect("write executable script");
        let mut permissions = std::fs::metadata(&path)
            .expect("read script metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).expect("make script executable");
        path
    }

    fn state_for_scope(scope: ProcessScope) -> ExternalManagerState {
        ExternalManagerState {
            version: STATE_VERSION,
            manager_id: "test-manager".to_string(),
            capabilities: ManagerCapabilities::default(),
            shutdown_script: None,
            scope,
        }
    }

    #[tokio::test]
    async fn durable_state_round_trips_manager_contract() {
        let state_dir = tempfile::tempdir().expect("state directory");
        let runtime = tempfile::tempdir().expect("runtime directory");
        let manager = ExternalManager::control(state_dir.path().into(), runtime.path().into());
        let capabilities = ManagerCapabilities {
            background_start: true,
            devenv_attach: false,
            wait_ready: true,
            individual_control: false,
            subset_start: true,
            requires_tty: false,
            manager_aware_stop: true,
        };
        let shutdown_script = runtime.path().join("shutdown adapter with spaces");
        let scope = ProcessScope::unix_session(std::process::id() as i32)
            .expect("capture test process identity");
        let expected = ExternalManagerState {
            version: STATE_VERSION,
            manager_id: "external-test-manager".to_string(),
            capabilities,
            shutdown_script: Some(shutdown_script.clone()),
            scope: scope.clone(),
        };

        fs::write(
            manager.state_file(),
            serde_json::to_vec(&expected).expect("serialize state"),
        )
        .await
        .expect("write state");

        let loaded = manager
            .load_state()
            .await
            .expect("load state")
            .expect("state exists");
        assert_eq!(loaded.version, STATE_VERSION);
        assert_eq!(loaded.manager_id, "external-test-manager");
        assert_eq!(loaded.capabilities, capabilities);
        assert_eq!(loaded.shutdown_script, Some(shutdown_script));
        assert_eq!(loaded.scope, scope);
    }

    #[tokio::test]
    async fn legacy_pid_state_loads_as_conservative_state() {
        let state_dir = tempfile::tempdir().expect("state directory");
        let runtime = tempfile::tempdir().expect("runtime directory");
        let manager = ExternalManager::control(state_dir.path().into(), runtime.path().into());
        pid::write_pid(&manager.pid_file(), std::process::id())
            .await
            .expect("write legacy pid state");
        let loaded_pid = manager
            .load_state()
            .await
            .expect("load pid-only state")
            .expect("pid-only state exists");
        assert_eq!(loaded_pid.manager_id, "unknown");
        assert_eq!(loaded_pid.capabilities, ManagerCapabilities::default());
        assert_eq!(loaded_pid.shutdown_script, None);
        assert_eq!(loaded_pid.scope.leader_pid(), std::process::id() as i32);
    }

    #[tokio::test]
    async fn rejects_unknown_durable_state_version() {
        let state_dir = tempfile::tempdir().expect("state directory");
        let runtime = tempfile::tempdir().expect("runtime directory");
        let manager = ExternalManager::control(state_dir.path().into(), runtime.path().into());
        let future_state = ExternalManagerState {
            version: STATE_VERSION + 1,
            manager_id: "future-manager".to_string(),
            capabilities: ManagerCapabilities::default(),
            shutdown_script: None,
            scope: ProcessScope::unix_session(std::process::id() as i32)
                .expect("capture test process identity"),
        };
        fs::write(
            manager.state_file(),
            serde_json::to_vec(&future_state).expect("serialize future state"),
        )
        .await
        .expect("write future state");

        let error = manager
            .load_state()
            .await
            .expect_err("future version must be rejected");
        assert!(
            error
                .to_string()
                .contains("Unsupported external-manager state version 2 (expected 1)"),
            "unexpected error: {error:?}"
        );
    }

    #[tokio::test]
    async fn follow_command_preserves_process_names_as_argv() {
        let state_dir = tempfile::tempdir().expect("state directory");
        let runtime = tempfile::tempdir().expect("runtime directory");
        let argv_file = runtime.path().join("argv");
        let injected_file = runtime.path().join("shell-injection");
        let launcher = write_executable_script(
            runtime.path(),
            "record-argv",
            r#"printf '%s\0' "$@" > "$ARGV_FILE""#,
        );
        let manager = ExternalManager::new(
            ManagerDescriptor::resolve("argv-test", Some(ManagerCapabilities::default())),
            launcher,
            None,
            state_dir.path().into(),
            runtime.path().into(),
        );
        let names = vec![
            "name with spaces".to_string(),
            "semi;colon".to_string(),
            format!("$(touch {})", injected_file.display()),
            "quotes-'\"-and-$dollar".to_string(),
            "wildcards-*?[abc]".to_string(),
        ];
        let mut env = HashMap::new();
        env.insert(
            "ARGV_FILE".to_string(),
            argv_file.to_string_lossy().into_owned(),
        );
        if let Some(path) = std::env::var_os("PATH") {
            env.insert("PATH".to_string(), path.to_string_lossy().into_owned());
        }

        let status = manager
            .prepare_follow_command(&names, &env)
            .await
            .expect("prepare follow command")
            .status()
            .expect("run follow command");
        assert!(status.success());

        let bytes = std::fs::read(argv_file).expect("read recorded argv");
        let actual = bytes
            .split(|byte| *byte == 0)
            .filter(|argument| !argument.is_empty())
            .map(|argument| String::from_utf8(argument.to_vec()).expect("utf-8 argument"))
            .collect::<Vec<_>>();
        assert_eq!(actual, names);
        assert!(
            !injected_file.exists(),
            "process name was interpreted as shell text"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn manager_aware_shutdown_runs_before_scope_cleanup() {
        let state_dir = tempfile::tempdir().expect("state directory");
        let runtime = tempfile::tempdir().expect("runtime directory");
        let marker = runtime.path().join("shutdown-marker");
        let launcher =
            write_executable_script(runtime.path(), "long-running-manager", "exec sleep 300");
        let shutdown = write_executable_script(
            runtime.path(),
            "shutdown-manager",
            &format!(
                "if kill -0 \"$(cat '{}')\" 2>/dev/null; then printf alive; else printf dead; fi > '{}'",
                runtime.path().join(RUNTIME_PID_FILE_NAME).display(),
                marker.display()
            ),
        );
        let capabilities = ManagerCapabilities {
            background_start: true,
            manager_aware_stop: true,
            ..ManagerCapabilities::default()
        };
        let manager = ExternalManager::new(
            ManagerDescriptor::resolve("adapter-test", Some(capabilities)),
            launcher,
            Some(shutdown),
            state_dir.path().into(),
            runtime.path().into(),
        );

        manager
            .start_background(BackgroundStartRequest::default())
            .await
            .expect("start external manager");
        let persisted = manager
            .load_state()
            .await
            .expect("load persisted state")
            .expect("persisted state exists");
        let scope = persisted.scope.clone();

        let stop_result = manager.stop().await;
        if scope.is_alive() {
            scope
                .force_kill()
                .expect("force cleanup after failed test stop");
        }
        stop_result.expect("stop external manager");

        assert_eq!(
            std::fs::read_to_string(marker).expect("read shutdown marker"),
            "alive",
            "shutdown adapter must run while the manager scope is still alive"
        );
        assert!(!scope.is_alive(), "scope cleanup must follow the adapter");
        assert!(!ExternalManager::state_exists(
            state_dir.path(),
            runtime.path()
        ));
    }

    #[test]
    fn state_detection_is_backend_specific_and_legacy_compatible() {
        let state = tempfile::tempdir().expect("state directory");
        let runtime = tempfile::tempdir().expect("runtime directory");

        assert!(!ExternalManager::state_exists(state.path(), runtime.path()));

        std::fs::write(runtime.path().join("native-manager.pid"), "1")
            .expect("write native-manager state");
        assert!(
            !ExternalManager::state_exists(state.path(), runtime.path()),
            "native-manager state must not be mistaken for external-manager state"
        );

        std::os::unix::fs::symlink(
            runtime.path().join("missing-external-manager.pid"),
            state.path().join(PID_FILE_NAME),
        )
        .expect("write dangling legacy marker");
        assert!(
            !ExternalManager::state_exists(state.path(), runtime.path()),
            "a legacy marker whose runtime target vanished must not count as live state"
        );
        std::fs::remove_file(state.path().join(PID_FILE_NAME))
            .expect("remove dangling legacy marker");

        std::fs::write(state.path().join(PID_FILE_NAME), "1")
            .expect("write legacy external-manager state");
        assert!(ExternalManager::state_exists(state.path(), runtime.path()));
        std::fs::remove_file(state.path().join(PID_FILE_NAME))
            .expect("remove legacy external-manager state");

        std::fs::write(runtime.path().join(STATE_FILE_NAME), "{}")
            .expect("write current external-manager state");
        assert!(ExternalManager::state_exists(state.path(), runtime.path()));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn durable_state_survives_the_leader_and_can_be_stopped() {
        let state = tempfile::tempdir().expect("state directory");
        let runtime = tempfile::tempdir().expect("runtime directory");
        let manager = ExternalManager::control(state.path().into(), runtime.path().into());

        let mut command = Command::new("bash");
        command
            .arg("-c")
            .arg("sleep 300 </dev/null >/dev/null 2>&1 &")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let spawn =
            PreparedProcessScope::prepare_tokio(&mut command).expect("prepare process scope");
        let mut child = command.spawn().expect("spawn leader");
        let leader_pid = child.id().expect("leader pid");
        let scope = spawn.capture(leader_pid).expect("capture scope");
        child.wait().await.expect("reap leader");
        assert!(scope.is_alive(), "background child must keep scope alive");

        fs::write(
            manager.state_file(),
            serde_json::to_vec(&state_for_scope(scope.clone())).expect("serialize state"),
        )
        .await
        .expect("write state");
        assert!(ExternalManager::state_exists(state.path(), runtime.path()));

        assert!(matches!(
            manager.check_status().await.expect("check status"),
            PidStatus::Running(pid) if pid.as_raw() == leader_pid as i32
        ));

        manager.stop().await.expect("stop process scope");
        assert!(!scope.is_alive());
        assert!(!ExternalManager::state_exists(state.path(), runtime.path()));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn legacy_pid_state_still_stops_its_process_group() {
        let state = tempfile::tempdir().expect("state directory");
        let runtime = tempfile::tempdir().expect("runtime directory");
        let manager = ExternalManager::control(state.path().into(), runtime.path().into());

        let mut command = Command::new("sleep");
        command
            .arg("300")
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = command.spawn().expect("spawn legacy process group");
        let pid = child.id().expect("leader pid");
        pid::write_pid(&manager.pid_file(), pid)
            .await
            .expect("write pid");

        manager.stop().await.expect("stop legacy process group");
        let status = child.wait().await.expect("reap leader");
        assert!(!status.success());
        assert!(!ExternalManager::state_exists(state.path(), runtime.path()));
    }

    #[tokio::test]
    async fn rejects_mismatched_pid_and_durable_state() {
        let state = tempfile::tempdir().expect("state directory");
        let runtime = tempfile::tempdir().expect("runtime directory");
        let manager = ExternalManager::control(state.path().into(), runtime.path().into());
        let scope = ProcessScope::unix_session(std::process::id() as i32)
            .expect("capture test process identity");
        fs::write(
            manager.state_file(),
            serde_json::to_vec(&state_for_scope(scope)).expect("serialize state"),
        )
        .await
        .expect("write state");
        let mismatched_pid = if std::process::id() == 1 { 2 } else { 1 };
        pid::write_pid(&manager.pid_file(), mismatched_pid)
            .await
            .expect("write mismatched pid");

        let error = manager.load_state().await.expect_err("state must disagree");
        assert!(
            error
                .to_string()
                .contains("Process-scope identity does not match PID file")
        );
    }

    #[tokio::test]
    async fn stale_state_and_pid_marker_are_removed_together() {
        let state = tempfile::tempdir().expect("state directory");
        let runtime = tempfile::tempdir().expect("runtime directory");
        let manager = ExternalManager::control(state.path().into(), runtime.path().into());
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("spawn short-lived process");
        let pid = child.id();
        let scope = ProcessScope::unix_session(pid as i32).expect("capture process identity");
        child.wait().expect("reap process");
        fs::write(
            manager.state_file(),
            serde_json::to_vec(&state_for_scope(scope)).expect("serialize state"),
        )
        .await
        .expect("write state");
        pid::write_pid(&manager.pid_file(), pid)
            .await
            .expect("write pid");

        assert!(matches!(
            manager.check_status().await.expect("check stale status"),
            PidStatus::StaleRemoved
        ));
        assert!(!ExternalManager::state_exists(state.path(), runtime.path()));
    }
}
