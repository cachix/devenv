//! SIGKILL recovery coverage for the native `files-reconcile` builtin.
//!
//! Each failpoint writes a marker after its filesystem boundary and parks the
//! child. The parent observes that marker before SIGKILL, avoiding scheduling
//! races or wall-clock sleeps as crash triggers.

#![cfg(all(unix, feature = "test-all"))]

use nix::{
    sys::signal::{self, Signal},
    unistd::Pid,
};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::process::ExitStatusExt,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

const TASK_NAME: &str = "devenv:test:files-crash-recovery";
const MARKER_TIMEOUT: Duration = Duration::from_secs(10);

struct Fixture {
    _temp: TempDir,
    root: PathBuf,
    state: PathBuf,
    cache: PathBuf,
    runtime: PathBuf,
    first: PathBuf,
    second: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let sources = temp.path().join("sources");
        let first = sources.join("v1");
        let second = sources.join("v2");
        for (source, contents) in [(&first, "first"), (&second, "second")] {
            fs::create_dir_all(source.join("nested")).unwrap();
            fs::write(source.join("nested/value"), contents).unwrap();
        }
        Self {
            root: temp.path().join("project"),
            state: temp.path().join("state/files.json"),
            cache: temp.path().join("cache"),
            runtime: temp.path().join("runtime"),
            first,
            second,
            _temp: temp,
        }
    }

    fn task_file(&self, tag: &str, digest: &str, source: &Path) -> PathBuf {
        let task_file = self.cache.join(format!("tasks-{tag}.json"));
        fs::create_dir_all(&self.cache).unwrap();
        fs::create_dir_all(&self.runtime).unwrap();
        let tasks = json!([{
            "name": TASK_NAME,
            "command": "/bin/false",
            "builtin": {
                "name": "files-reconcile",
                "version": 1,
                "input": {
                    "root": self.root,
                    "state_file": self.state,
                    "desired_digest": digest,
                    "files": [{
                        "path": "target",
                        "source": source,
                        "mode": "copy",
                    }],
                }
            }
        }]);
        fs::write(&task_file, serde_json::to_vec(&tasks).unwrap()).unwrap();
        task_file
    }

    fn command(&self, tag: &str, digest: &str, source: &Path) -> Command {
        let task_file = self.task_file(tag, digest, source);
        let mut command = Command::new(env!("CARGO_BIN_EXE_devenv-tasks"));
        command
            .args([
                "run",
                TASK_NAME,
                "--mode",
                "all",
                "--task-file",
                task_file.to_str().unwrap(),
                "--cache-dir",
                self.cache.to_str().unwrap(),
                "--runtime-dir",
                self.runtime.to_str().unwrap(),
                "--on-idle",
                "exit",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    fn run(&self, tag: &str, digest: &str, source: &Path) {
        let status = self.command(tag, digest, source).status().unwrap();
        assert!(status.success(), "{tag} failed with {status}");
    }
}

struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for_marker(marker: &Path, child: &mut Child) {
    let deadline = Instant::now() + MARKER_TIMEOUT;
    while !marker.exists() {
        assert!(
            child.try_wait().unwrap().is_none(),
            "child exited before crash failpoint"
        );
        assert!(
            Instant::now() < deadline,
            "child did not reach crash failpoint {}",
            marker.display()
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn crash_at(fixture: &Fixture, boundary: &str) -> ExitStatus {
    let marker = fixture.cache.join(format!("crash-marker-{boundary}"));
    let mut child = fixture.command("crash", "v2", &fixture.second);
    child
        .env("DEVENV_FILES_TEST_CRASH_BOUNDARY", boundary)
        .env("DEVENV_FILES_TEST_CRASH_MARKER", &marker);
    let mut child = KillOnDrop(child.spawn().unwrap());
    wait_for_marker(&marker, &mut child.0);
    let pid = child.0.id() as i32;
    signal::kill(Pid::from_raw(pid), Signal::SIGKILL).unwrap();
    let status = child.0.wait().unwrap();
    assert_eq!(status.signal(), Some(Signal::SIGKILL as i32));
    status
}

fn assert_recovered(fixture: &Fixture) {
    assert_eq!(
        fs::read_to_string(fixture.root.join("target/nested/value")).unwrap(),
        "second"
    );
    assert!(
        !fixture.state.with_extension("recovery").exists(),
        "recovery journal remained after clean reconciliation"
    );
    let state: Value = serde_json::from_slice(&fs::read(&fixture.state).unwrap()).unwrap();
    assert_eq!(state["managedFiles"], json!(["target"]));
    assert_eq!(state["entries"]["target"]["source"], json!(fixture.second));
    assert!(
        state.get("garbage").is_none(),
        "recovery left deferred cleanup behind: {}",
        state
    );
    for entry in fs::read_dir(&fixture.root).unwrap() {
        let entry = entry.unwrap();
        assert!(
            !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".devenv-files-artifact-"),
            "recovery left private artifact {}",
            entry.path().display()
        );
    }
}

#[test]
fn sigkill_recovery_converges_across_mkdir_journal_rename_and_state_boundaries() {
    for boundary in ["mkdir", "journal", "rename", "state"] {
        let fixture = Fixture::new();
        fixture.run("initial", "v1", &fixture.first);

        crash_at(&fixture, boundary);

        fixture.run("recover", "v2", &fixture.second);
        assert_recovered(&fixture);
    }
}
