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

    // Killing this wrapper reparents devenv-tasks without signaling it.
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
