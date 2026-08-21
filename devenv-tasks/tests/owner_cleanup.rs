//! Process cleanup after `devenv-tasks` exits or is killed.

#![cfg(unix)]

use devenv_processes::{ProcessConfig, RestartConfig, RestartPolicy, ShutdownConfig};
use devenv_tasks::{TaskConfig, TaskType};
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

const READY_TIMEOUT: Duration = Duration::from_secs(10);

fn process_is_alive(pid: i32) -> bool {
    signal::kill(Pid::from_raw(pid), None).is_ok()
}

async fn wait_for_file(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    false
}

async fn wait_for_exit(pid: i32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !process_is_alive(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    false
}

fn read_pid(path: &Path) -> i32 {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        .trim()
        .parse()
        .unwrap_or_else(|e| panic!("parse pid in {}: {e}", path.display()))
}

fn kill_leftovers(pids: &[i32]) {
    for pid in pids {
        let _ = signal::killpg(Pid::from_raw(*pid), Signal::SIGKILL);
        let _ = signal::kill(Pid::from_raw(*pid), Signal::SIGKILL);
    }
}

struct Fixture {
    temp: tempfile::TempDir,
    task_name: String,
}

impl Fixture {
    fn new(task_name: &str) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(temp.path().join("cache")).unwrap();
        std::fs::create_dir_all(temp.path().join("runtime")).unwrap();
        Self {
            temp,
            task_name: format!("devenv:processes:{task_name}"),
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.temp.path().join(name)
    }

    /// Start a leader and a child in separate process groups within one session.
    fn leader_with_private_child(&self) -> String {
        format!(
            "bash -c 'set -m; echo $$ > {}; sleep 3600 & echo $! > {}; touch {}; wait'",
            self.path("parent.pid").display(),
            self.path("child.pid").display(),
            self.path("ready").display(),
        )
    }

    /// Ignore graceful signals in both process groups.
    fn stubborn_leader_with_private_child(&self) -> String {
        format!(
            "bash -c 'trap \"\" TERM INT; set -m; echo $$ > {}; (trap \"\" TERM INT; while :; do sleep 1; done) & echo $! > {}; touch {}; wait'",
            self.path("parent.pid").display(),
            self.path("child.pid").display(),
            self.path("ready").display(),
        )
    }

    fn write_task(&self, command: String, shutdown: ShutdownConfig) {
        let task = TaskConfig {
            name: self.task_name.clone(),
            r#type: TaskType::Process,
            command: Some(command),
            process: Some(ProcessConfig {
                restart: RestartConfig {
                    on: RestartPolicy::Never,
                    ..Default::default()
                },
                shutdown,
                ..Default::default()
            }),
            ..Default::default()
        };
        std::fs::write(
            self.path("tasks.json"),
            serde_json::to_vec(&vec![task]).unwrap(),
        )
        .unwrap();
    }

    fn owner_args(&self) -> Vec<String> {
        vec![
            "run".to_string(),
            self.task_name.clone(),
            "--mode".to_string(),
            "all".to_string(),
            "--task-file".to_string(),
            self.path("tasks.json").display().to_string(),
            "--cache-dir".to_string(),
            self.path("cache").display().to_string(),
            "--runtime-dir".to_string(),
            self.path("runtime").display().to_string(),
        ]
    }

    fn owner_command(&self) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_devenv-tasks"));
        command
            .args(self.owner_args())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    fn owner_command_with_supervisor(&self, supervisor: &str) -> tokio::process::Command {
        let mut command = self.owner_command();
        command.arg("--supervisor").arg(supervisor);
        command
    }

    async fn wait_ready(&self) -> (i32, i32) {
        assert!(
            wait_for_file(&self.path("ready"), READY_TIMEOUT).await,
            "service did not become ready"
        );
        (
            read_pid(&self.path("parent.pid")),
            read_pid(&self.path("child.pid")),
        )
    }
}

#[cfg(feature = "test-all")]
#[tokio::test(flavor = "multi_thread")]
async fn recoverable_owner_error_reclaims_service_session() {
    let fixture = Fixture::new("error-test");
    fixture.write_task(
        fixture.leader_with_private_child(),
        ShutdownConfig::default(),
    );

    let mut owner = fixture
        .owner_command()
        .env("DEVENV_TASKS_TEST_FAIL_UI_AFTER_PROCESS_START", "1")
        .spawn()
        .expect("spawn devenv-tasks");
    let (parent_pid, child_pid) = fixture.wait_ready().await;

    let status = tokio::time::timeout(Duration::from_secs(10), owner.wait())
        .await
        .expect("owner did not return after injected error")
        .expect("wait for owner");
    assert!(
        !status.success(),
        "injected owner failure unexpectedly succeeded"
    );

    let parent_exited = wait_for_exit(parent_pid, Duration::from_secs(3)).await;
    let child_exited = wait_for_exit(child_pid, Duration::from_secs(3)).await;
    kill_leftovers(&[parent_pid, child_pid]);
    assert!(parent_exited, "service survived recoverable owner error");
    assert!(
        child_exited,
        "private process group survived recoverable owner error"
    );
}

#[cfg(feature = "test-all")]
#[tokio::test(flavor = "multi_thread")]
async fn guardian_start_failure_stops_service_session() {
    let fixture = Fixture::new("guardian-start-failure");
    fixture.write_task(
        fixture.leader_with_private_child(),
        ShutdownConfig::default(),
    );

    let mut owner = fixture
        .owner_command()
        .env("DEVENV_TASKS_TEST_FAIL_SESSION_GUARDIAN_START", "1")
        .spawn()
        .expect("spawn devenv-tasks");
    let (parent_pid, child_pid) = fixture.wait_ready().await;

    let parent_exited = wait_for_exit(parent_pid, Duration::from_secs(3)).await;
    let child_exited = wait_for_exit(child_pid, Duration::from_secs(3)).await;
    let _ = owner.start_kill();
    let _ = owner.wait().await;
    kill_leftovers(&[parent_pid, child_pid]);

    assert!(parent_exited, "service survived guardian startup failure");
    assert!(
        child_exited,
        "private process group survived guardian startup failure"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn parent_death_reclaims_service_session() {
    let fixture = Fixture::new("parent-death-test");
    fixture.write_task(
        fixture.leader_with_private_child(),
        ShutdownConfig {
            signal: 15,
            grace: 1,
        },
    );

    // Killing only the wrapper forces parent-death detection.
    let owner_pid_file = fixture.path("owner.pid");
    let wrapper_command = format!(
        "{} {} >/dev/null 2>&1 & echo $! > {}; wait",
        env!("CARGO_BIN_EXE_devenv-tasks"),
        fixture.owner_args().join(" "),
        owner_pid_file.display(),
    );
    let mut wrapper = tokio::process::Command::new("bash")
        .arg("-c")
        .arg(wrapper_command)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn wrapper");
    let (parent_pid, child_pid) = fixture.wait_ready().await;
    let owner_pid = read_pid(&owner_pid_file);

    signal::kill(
        Pid::from_raw(wrapper.id().expect("wrapper pid") as i32),
        Signal::SIGKILL,
    )
    .unwrap();
    let _ = wrapper.wait().await;

    let owner_exited = wait_for_exit(owner_pid, Duration::from_secs(5)).await;
    let parent_exited = wait_for_exit(parent_pid, Duration::from_secs(5)).await;
    let child_exited = wait_for_exit(child_pid, Duration::from_secs(5)).await;
    kill_leftovers(&[owner_pid, parent_pid, child_pid]);
    assert!(owner_exited, "devenv-tasks survived parent death");
    assert!(parent_exited, "service survived parent death");
    assert!(child_exited, "private process group survived parent death");
}

async fn assert_sigkill_reclaims_service_session(supervisor: &str) {
    let fixture = Fixture::new(&format!("{supervisor}-orphan-test"));
    fixture.write_task(
        fixture.leader_with_private_child(),
        ShutdownConfig::default(),
    );

    let mut owner_command = fixture.owner_command_with_supervisor(supervisor);
    // Model the process group that process-compose gives its child.
    owner_command.process_group(0);
    let mut owner = owner_command.spawn().expect("spawn devenv-tasks");
    let (parent_pid, child_pid) = fixture.wait_ready().await;

    signal::killpg(
        Pid::from_raw(owner.id().expect("owner pid") as i32),
        Signal::SIGKILL,
    )
    .unwrap();
    let _ = owner.wait().await;

    let parent_exited = wait_for_exit(parent_pid, Duration::from_secs(8)).await;
    let child_exited = wait_for_exit(child_pid, Duration::from_secs(8)).await;
    kill_leftovers(&[parent_pid, child_pid]);
    assert!(parent_exited, "service leader survived owner SIGKILL");
    assert!(child_exited, "private process group survived owner SIGKILL");
}

#[tokio::test(flavor = "multi_thread")]
async fn sigkill_of_native_devenv_tasks_reclaims_service_session() {
    assert_sigkill_reclaims_service_session("native").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sigkill_of_external_devenv_tasks_reclaims_service_session() {
    assert_sigkill_reclaims_service_session("external").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn second_signal_reclaims_private_process_groups() {
    let fixture = Fixture::new("second-signal-test");
    fixture.write_task(
        fixture.stubborn_leader_with_private_child(),
        ShutdownConfig::default(),
    );

    let mut owner = fixture.owner_command().spawn().expect("spawn devenv-tasks");
    let (parent_pid, child_pid) = fixture.wait_ready().await;
    let owner_pid = Pid::from_raw(owner.id().expect("owner pid") as i32);

    signal::kill(owner_pid, Signal::SIGINT).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    signal::kill(owner_pid, Signal::SIGINT).unwrap();

    let status = tokio::time::timeout(Duration::from_secs(3), owner.wait())
        .await
        .expect("devenv-tasks did not force-exit after the second signal")
        .expect("wait for devenv-tasks");
    assert!(
        !status.success(),
        "force-exited owner unexpectedly succeeded"
    );

    let parent_exited = wait_for_exit(parent_pid, Duration::from_secs(8)).await;
    let child_exited = wait_for_exit(child_pid, Duration::from_secs(8)).await;
    kill_leftovers(&[parent_pid, child_pid]);
    assert!(parent_exited, "service leader survived the second signal");
    assert!(
        child_exited,
        "private process group survived force-exit guardian cleanup"
    );
}

async fn assert_native_api_socket_publishing(supervisor: &str, expected: bool) {
    let fixture = Fixture::new(&format!("{supervisor}-socket-ownership"));
    fixture.write_task(
        fixture.leader_with_private_child(),
        ShutdownConfig::default(),
    );

    let mut owner_command = fixture.owner_command_with_supervisor(supervisor);
    owner_command.process_group(0);
    let mut owner = owner_command.spawn().expect("spawn devenv-tasks");
    let (parent_pid, child_pid) = fixture.wait_ready().await;

    let socket = fixture.path("runtime/processes/native.sock");
    let socket_published = wait_for_file(&socket, Duration::from_secs(1)).await;

    signal::killpg(
        Pid::from_raw(owner.id().expect("owner pid") as i32),
        Signal::SIGKILL,
    )
    .unwrap();
    let _ = owner.wait().await;
    kill_leftovers(&[parent_pid, child_pid]);

    assert_eq!(
        socket_published,
        expected,
        "{supervisor} devenv-tasks unexpectedly {} the native manager API socket",
        if expected {
            "did not publish"
        } else {
            "published"
        },
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn native_devenv_tasks_publishes_native_api_socket() {
    assert_native_api_socket_publishing("native", true).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn external_devenv_tasks_does_not_publish_native_api_socket() {
    assert_native_api_socket_publishing("external", false).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn external_devenv_tasks_does_not_remove_native_api_socket() {
    let fixture = Fixture::new("external-socket-cleanup");
    fixture.write_task("true".to_string(), ShutdownConfig::default());

    // Simulate the native owner of the shared socket.
    let socket = fixture.path("runtime/processes/native.sock");
    std::fs::create_dir_all(socket.parent().expect("socket parent"))
        .expect("create process runtime directory");
    let _native_listener =
        std::os::unix::net::UnixListener::bind(&socket).expect("bind native manager API socket");

    let mut owner = fixture
        .owner_command_with_supervisor("external")
        .spawn()
        .expect("spawn external devenv-tasks");
    let status = tokio::time::timeout(Duration::from_secs(10), owner.wait())
        .await
        .expect("external devenv-tasks did not exit")
        .expect("wait for external devenv-tasks");
    assert!(status.success(), "external devenv-tasks failed: {status}");

    assert!(
        socket.exists(),
        "external devenv-tasks removed the native manager API socket"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn external_devenv_tasks_propagates_failed_child_exit() {
    let fixture = Fixture::new("external-failure");
    fixture.write_task("exit 7".to_string(), ShutdownConfig::default());

    let mut owner = fixture
        .owner_command_with_supervisor("external")
        .spawn()
        .expect("spawn external devenv-tasks");
    let status = tokio::time::timeout(Duration::from_secs(10), owner.wait())
        .await
        .expect("external devenv-tasks did not exit")
        .expect("wait for external devenv-tasks");

    assert_eq!(
        status.code(),
        Some(1),
        "failed externally supervised process must fail its wrapper"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn restart_reconciles_previous_session_before_launch() {
    let fixture = Fixture::new("reconcile-test");
    let shutdown = ShutdownConfig {
        signal: 15,
        grace: 1,
    };

    // Force reconciliation through guardian escalation.
    let old_pid_file = fixture.path("old.pid");
    let old_ready = fixture.path("old.ready");
    fixture.write_task(
        format!(
            "bash -c 'trap \"\" TERM; echo $$ > {}; touch {}; while :; do sleep 1; done'",
            old_pid_file.display(),
            old_ready.display(),
        ),
        shutdown.clone(),
    );
    let mut old_owner = fixture.owner_command().spawn().expect("spawn old owner");
    assert!(
        wait_for_file(&old_ready, READY_TIMEOUT).await,
        "old service did not become ready"
    );
    let old_pid = read_pid(&old_pid_file);

    signal::kill(
        Pid::from_raw(old_owner.id().expect("old owner pid") as i32),
        Signal::SIGKILL,
    )
    .unwrap();
    let _ = old_owner.wait().await;

    let new_pid_file = fixture.path("new.pid");
    let new_ready = fixture.path("new.ready");
    fixture.write_task(
        format!(
            "bash -c 'echo $$ > {}; touch {}; while :; do sleep 1; done'",
            new_pid_file.display(),
            new_ready.display(),
        ),
        shutdown,
    );
    let mut new_owner = fixture.owner_command().spawn().expect("spawn new owner");

    let new_started = wait_for_file(&new_ready, READY_TIMEOUT).await;
    let old_exited = wait_for_exit(old_pid, Duration::from_secs(1)).await;
    let new_pid = new_started.then(|| read_pid(&new_pid_file));

    let _ = new_owner.start_kill();
    let _ = new_owner.wait().await;
    let new_exited = match new_pid {
        Some(pid) => wait_for_exit(pid, Duration::from_secs(5)).await,
        None => true,
    };
    kill_leftovers(&[old_pid]);
    if let Some(pid) = new_pid {
        kill_leftovers(&[pid]);
    }

    assert!(
        new_started,
        "new service did not launch after lease reconciliation"
    );
    assert!(
        old_exited,
        "new service launched before the previous session was reclaimed"
    );
    assert!(new_exited, "new service survived owner shutdown");
}
