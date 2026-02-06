mod common;

use common::{TestShellBuilder, create_temp_dir_with_files, modify_file};
use devenv_reload::{
    BuildContext, BuildError, BuildTrigger, CommandBuilder, Config, ManagerError, ManagerMessage,
    ShellBuilder, ShellManager,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Builder that tracks build count and signals on each build
struct SignalingBuilder {
    count: Arc<AtomicUsize>,
    build_tx: mpsc::Sender<usize>,
}

impl ShellBuilder for SignalingBuilder {
    fn build(&self, _ctx: &BuildContext) -> Result<CommandBuilder, BuildError> {
        let n = self.count.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.build_tx.try_send(n);
        let mut cmd = CommandBuilder::new("sh");
        cmd.arg("-c");
        cmd.arg("sleep 0.5; exit 0");
        Ok(cmd)
    }

    fn build_reload_env(&self, _ctx: &BuildContext) -> Result<(), BuildError> {
        let n = self.count.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.build_tx.try_send(n);
        Ok(())
    }
}

/// Builder that fails
struct FailingBuilder;

impl ShellBuilder for FailingBuilder {
    fn build(&self, _ctx: &BuildContext) -> Result<CommandBuilder, BuildError> {
        Err(BuildError::new("intentional test failure"))
    }

    fn build_reload_env(&self, _ctx: &BuildContext) -> Result<(), BuildError> {
        Err(BuildError::new("intentional test failure"))
    }
}

#[tokio::test]
async fn test_manager_starts_and_exits_cleanly() {
    let builder = TestShellBuilder::new("sh").with_args(&["-c", "echo 'started'; exit 0"]);
    let config = Config::new(vec![]);
    let (msg_tx, _msg_rx) = mpsc::channel::<ManagerMessage>(10);

    let result =
        tokio::time::timeout(TEST_TIMEOUT, ShellManager::run(config, builder, msg_tx)).await;

    assert!(result.is_ok(), "test timed out");
    assert!(result.unwrap().is_ok());
}

#[tokio::test]
async fn test_manager_build_failure_on_start() {
    let config = Config::new(vec![]);
    let (msg_tx, _msg_rx) = mpsc::channel::<ManagerMessage>(10);

    let result = tokio::time::timeout(
        TEST_TIMEOUT,
        ShellManager::run(config, FailingBuilder, msg_tx),
    )
    .await;

    assert!(result.is_ok(), "test timed out");
    let result = result.unwrap();
    assert!(result.is_err());
    match result {
        Err(ManagerError::Build(e)) => {
            assert!(e.message.contains("intentional"));
        }
        _ => panic!("expected Build error"),
    }
}

#[tokio::test]
async fn test_manager_triggers_reload_on_file_change() {
    let temp_dir = create_temp_dir_with_files(&[("devenv.nix", "initial")]);
    let watch_file = temp_dir.path().join("devenv.nix");

    let build_count = Arc::new(AtomicUsize::new(0));
    let (build_tx, mut build_rx) = mpsc::channel::<usize>(10);

    let builder = SignalingBuilder {
        count: build_count.clone(),
        build_tx,
    };

    let config = Config::new(vec![watch_file.clone()]);
    let (msg_tx, mut msg_rx) = mpsc::channel::<ManagerMessage>(10);

    // Spawn manager in background
    let handle = tokio::spawn(async move { ShellManager::run(config, builder, msg_tx).await });

    // Wait for initial build signal
    let first_build = tokio::time::timeout(TEST_TIMEOUT, build_rx.recv()).await;
    assert!(first_build.is_ok(), "timeout waiting for initial build");
    assert_eq!(first_build.unwrap(), Some(1));

    // Modify file to trigger reload
    modify_file(&watch_file, "modified");

    // Wait for reload build signal
    let second_build = tokio::time::timeout(TEST_TIMEOUT, build_rx.recv()).await;
    assert!(second_build.is_ok(), "timeout waiting for reload build");
    assert!(
        second_build.unwrap().unwrap() >= 2,
        "expected at least build #2"
    );

    // Verify we received a Reloaded message
    let msg = tokio::time::timeout(TEST_TIMEOUT, msg_rx.recv()).await;
    assert!(msg.is_ok(), "timeout waiting for message");
    match msg.unwrap() {
        Some(ManagerMessage::Reloaded { files }) => {
            assert!(!files.is_empty(), "expected at least one file in message");
        }
        other => panic!("expected Reloaded message, got {:?}", other),
    }

    handle.abort();
}

#[tokio::test]
async fn test_manager_provides_correct_context() {
    use std::sync::Mutex;

    let temp_dir = create_temp_dir_with_files(&[("test.nix", "x")]);
    let watch_file = temp_dir.path().join("test.nix");

    let triggers: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let triggers_clone = triggers.clone();
    let (build_tx, mut build_rx) = mpsc::channel::<usize>(10);

    struct TriggerTracker {
        triggers: Arc<Mutex<Vec<String>>>,
        build_tx: mpsc::Sender<usize>,
        count: AtomicUsize,
    }

    impl ShellBuilder for TriggerTracker {
        fn build(&self, ctx: &BuildContext) -> Result<CommandBuilder, BuildError> {
            let trigger_str = match &ctx.trigger {
                BuildTrigger::Initial => "initial".to_string(),
                BuildTrigger::FileChanged(p) => format!("file:{}", p.display()),
            };
            self.triggers.lock().unwrap().push(trigger_str);

            let n = self.count.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = self.build_tx.try_send(n);

            let mut cmd = CommandBuilder::new("sh");
            cmd.arg("-c");
            cmd.arg("sleep 0.5; exit 0");
            Ok(cmd)
        }

        fn build_reload_env(&self, ctx: &BuildContext) -> Result<(), BuildError> {
            let trigger_str = match &ctx.trigger {
                BuildTrigger::Initial => "initial".to_string(),
                BuildTrigger::FileChanged(p) => format!("file:{}", p.display()),
            };
            self.triggers.lock().unwrap().push(trigger_str);

            let n = self.count.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = self.build_tx.try_send(n);

            Ok(())
        }
    }

    let builder = TriggerTracker {
        triggers: triggers_clone,
        build_tx,
        count: AtomicUsize::new(0),
    };

    let config = Config::new(vec![watch_file.clone()]);
    let (msg_tx, _msg_rx) = mpsc::channel::<ManagerMessage>(10);

    let handle = tokio::spawn(async move { ShellManager::run(config, builder, msg_tx).await });

    // Wait for initial build
    let first = tokio::time::timeout(TEST_TIMEOUT, build_rx.recv()).await;
    assert!(first.is_ok(), "timeout waiting for initial build");

    {
        let t = triggers.lock().unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0], "initial");
    }

    // Trigger reload
    modify_file(&watch_file, "y");

    // Wait for reload build
    let second = tokio::time::timeout(TEST_TIMEOUT, build_rx.recv()).await;
    assert!(second.is_ok(), "timeout waiting for reload build");

    {
        let t = triggers.lock().unwrap();
        assert!(t.len() >= 2, "expected at least 2 triggers, got {:?}", *t);
        assert!(
            t[1].starts_with("file:"),
            "expected file trigger, got {}",
            t[1]
        );
    }

    handle.abort();
}

#[tokio::test]
async fn test_manager_keeps_shell_on_build_failure_during_reload() {
    let temp_dir = create_temp_dir_with_files(&[("test.nix", "x")]);
    let watch_file = temp_dir.path().join("test.nix");

    let build_count = Arc::new(AtomicUsize::new(0));
    let build_count_clone = build_count.clone();
    let (build_tx, mut build_rx) = mpsc::channel::<usize>(10);

    /// Builder that fails on reload
    struct FailOnReloadBuilder {
        count: Arc<AtomicUsize>,
        build_tx: mpsc::Sender<usize>,
    }

    impl ShellBuilder for FailOnReloadBuilder {
        fn build(&self, ctx: &BuildContext) -> Result<CommandBuilder, BuildError> {
            let n = self.count.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = self.build_tx.try_send(n);

            match &ctx.trigger {
                BuildTrigger::Initial => {
                    let mut cmd = CommandBuilder::new("sh");
                    cmd.arg("-c");
                    cmd.arg("sleep 10");
                    Ok(cmd)
                }
                BuildTrigger::FileChanged(_) => {
                    Err(BuildError::new("simulated build failure during reload"))
                }
            }
        }

        fn build_reload_env(&self, ctx: &BuildContext) -> Result<(), BuildError> {
            let n = self.count.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = self.build_tx.try_send(n);

            match &ctx.trigger {
                BuildTrigger::Initial => Ok(()),
                BuildTrigger::FileChanged(_) => {
                    Err(BuildError::new("simulated build failure during reload"))
                }
            }
        }
    }

    let builder = FailOnReloadBuilder {
        count: build_count_clone,
        build_tx,
    };

    let config = Config::new(vec![watch_file.clone()]);
    let (msg_tx, mut msg_rx) = mpsc::channel::<ManagerMessage>(10);

    let handle = tokio::spawn(async move { ShellManager::run(config, builder, msg_tx).await });

    // Wait for initial build
    let first = tokio::time::timeout(TEST_TIMEOUT, build_rx.recv()).await;
    assert!(first.is_ok(), "timeout waiting for initial build");
    assert_eq!(build_count.load(Ordering::SeqCst), 1);

    // Trigger reload (will fail)
    modify_file(&watch_file, "trigger failure");

    // Wait for reload attempt
    let second = tokio::time::timeout(TEST_TIMEOUT, build_rx.recv()).await;
    assert!(second.is_ok(), "timeout waiting for reload attempt");
    assert!(
        build_count.load(Ordering::SeqCst) >= 2,
        "build should have been attempted at least twice"
    );

    // Verify we received a BuildFailed message
    let msg = tokio::time::timeout(TEST_TIMEOUT, msg_rx.recv()).await;
    assert!(msg.is_ok(), "timeout waiting for message");
    match msg.unwrap() {
        Some(ManagerMessage::BuildFailed { files, error }) => {
            assert!(!files.is_empty(), "expected at least one file in message");
            assert!(
                error.contains("simulated"),
                "expected error to contain 'simulated', got: {}",
                error
            );
        }
        other => panic!("expected BuildFailed message, got {:?}", other),
    }

    // Manager should still be running (shell kept alive)
    assert!(
        !handle.is_finished(),
        "manager should still be running after failed reload"
    );

    handle.abort();
}

#[tokio::test]
async fn test_manager_keeps_shell_on_spawn_failure_during_reload() {
    let temp_dir = create_temp_dir_with_files(&[("test.nix", "x")]);
    let watch_file = temp_dir.path().join("test.nix");

    let build_count = Arc::new(AtomicUsize::new(0));
    let build_count_clone = build_count.clone();
    let (build_tx, mut build_rx) = mpsc::channel::<usize>(10);

    /// Builder that returns invalid command on reload (PTY spawn will fail)
    struct BadCommandOnReloadBuilder {
        count: Arc<AtomicUsize>,
        build_tx: mpsc::Sender<usize>,
    }

    impl ShellBuilder for BadCommandOnReloadBuilder {
        fn build(&self, ctx: &BuildContext) -> Result<CommandBuilder, BuildError> {
            let n = self.count.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = self.build_tx.try_send(n);

            match &ctx.trigger {
                BuildTrigger::Initial => {
                    let mut cmd = CommandBuilder::new("sh");
                    cmd.arg("-c");
                    cmd.arg("sleep 10");
                    Ok(cmd)
                }
                BuildTrigger::FileChanged(_) => {
                    // Return a command that will fail to spawn
                    let cmd = CommandBuilder::new("/nonexistent/command/that/does/not/exist");
                    Ok(cmd)
                }
            }
        }

        fn build_reload_env(&self, ctx: &BuildContext) -> Result<(), BuildError> {
            let n = self.count.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = self.build_tx.try_send(n);

            match &ctx.trigger {
                BuildTrigger::Initial => Ok(()),
                BuildTrigger::FileChanged(_) => {
                    // Simulate a failed reload by returning an error
                    Err(BuildError::new("reload failed"))
                }
            }
        }
    }

    let builder = BadCommandOnReloadBuilder {
        count: build_count_clone,
        build_tx,
    };

    let config = Config::new(vec![watch_file.clone()]);
    let (msg_tx, mut msg_rx) = mpsc::channel::<ManagerMessage>(10);

    let handle = tokio::spawn(async move { ShellManager::run(config, builder, msg_tx).await });

    // Wait for initial build
    let first = tokio::time::timeout(TEST_TIMEOUT, build_rx.recv()).await;
    assert!(first.is_ok(), "timeout waiting for initial build");
    assert_eq!(build_count.load(Ordering::SeqCst), 1);

    // Trigger reload (spawn will fail)
    modify_file(&watch_file, "trigger spawn failure");

    // Wait for reload attempt
    let second = tokio::time::timeout(TEST_TIMEOUT, build_rx.recv()).await;
    assert!(second.is_ok(), "timeout waiting for reload attempt");
    assert!(
        build_count.load(Ordering::SeqCst) >= 2,
        "build should have been attempted at least twice"
    );

    // Verify we received a ReloadFailed message
    let msg = tokio::time::timeout(TEST_TIMEOUT, msg_rx.recv()).await;
    assert!(msg.is_ok(), "timeout waiting for message");
    match msg.unwrap() {
        Some(ManagerMessage::ReloadFailed { files, error }) => {
            assert!(!files.is_empty(), "expected at least one file in message");
            assert!(!error.is_empty(), "expected non-empty error message");
        }
        other => panic!("expected ReloadFailed message, got {:?}", other),
    }

    // Manager should still be running (original shell kept alive)
    assert!(
        !handle.is_finished(),
        "manager should still be running after failed spawn"
    );

    handle.abort();
}

#[tokio::test]
async fn test_manager_cancels_old_build_on_new_change() {
    let temp_dir = create_temp_dir_with_files(&[("test.nix", "x")]);
    let watch_file = temp_dir.path().join("test.nix");

    // Channel for builds to signal they've started
    let (build_started_tx, mut build_started_rx) = mpsc::channel::<usize>(10);
    // Channel to release builds (with timeout built into receiver)
    let (release_tx, release_rx) = mpsc::channel::<()>(10);

    /// Builder that waits for release signal before completing
    struct SyncBuilder {
        build_started_tx: mpsc::Sender<usize>,
        release_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<()>>>,
        build_counter: Arc<AtomicUsize>,
    }

    impl ShellBuilder for SyncBuilder {
        fn build(&self, ctx: &BuildContext) -> Result<CommandBuilder, BuildError> {
            match &ctx.trigger {
                BuildTrigger::Initial => {
                    let mut cmd = CommandBuilder::new("sh");
                    cmd.arg("-c");
                    cmd.arg("read x");
                    Ok(cmd)
                }
                BuildTrigger::FileChanged(_) => {
                    let build_num = self.build_counter.fetch_add(1, Ordering::SeqCst) + 1;

                    // Signal that this build has started
                    let _ = self.build_started_tx.try_send(build_num);

                    // Wait for release signal with timeout (prevents hang if aborted)
                    // This runs inside spawn_blocking so we can use blocking operations
                    let release_rx = self.release_rx.clone();
                    std::thread::sleep(Duration::from_millis(100)); // Small delay to allow abort to happen
                    let rt = tokio::runtime::Handle::current();
                    let _ = rt.block_on(async {
                        let mut rx = release_rx.lock().await;
                        tokio::time::timeout(Duration::from_millis(500), rx.recv()).await
                    });

                    let mut cmd = CommandBuilder::new("sh");
                    cmd.arg("-c");
                    cmd.arg("read x");
                    Ok(cmd)
                }
            }
        }

        fn build_reload_env(&self, ctx: &BuildContext) -> Result<(), BuildError> {
            match &ctx.trigger {
                BuildTrigger::Initial => Ok(()),
                BuildTrigger::FileChanged(_) => {
                    let build_num = self.build_counter.fetch_add(1, Ordering::SeqCst) + 1;

                    // Signal that this build has started
                    let _ = self.build_started_tx.try_send(build_num);

                    // Wait for release signal with timeout (prevents hang if aborted)
                    let release_rx = self.release_rx.clone();
                    std::thread::sleep(Duration::from_millis(100));
                    let rt = tokio::runtime::Handle::current();
                    let _ = rt.block_on(async {
                        let mut rx = release_rx.lock().await;
                        tokio::time::timeout(Duration::from_millis(500), rx.recv()).await
                    });

                    Ok(())
                }
            }
        }
    }

    let builder = SyncBuilder {
        build_started_tx,
        release_rx: Arc::new(tokio::sync::Mutex::new(release_rx)),
        build_counter: Arc::new(AtomicUsize::new(0)),
    };

    let config = Config::new(vec![watch_file.clone()]);
    let (msg_tx, _msg_rx) = mpsc::channel::<ManagerMessage>(10);
    let handle = tokio::spawn(async move { ShellManager::run(config, builder, msg_tx).await });

    tokio::task::yield_now().await;

    // Trigger first file change
    modify_file(&watch_file, "change 1");

    // Wait for first build to start
    let first_build = tokio::time::timeout(TEST_TIMEOUT, build_started_rx.recv()).await;
    assert!(first_build.is_ok(), "timeout waiting for first build");
    assert_eq!(first_build.unwrap(), Some(1), "first build should be #1");

    // Trigger second file change while first build is still in progress
    modify_file(&watch_file, "change 2");

    // Wait for second build to start
    let second_build = tokio::time::timeout(TEST_TIMEOUT, build_started_rx.recv()).await;
    assert!(second_build.is_ok(), "timeout waiting for second build");
    assert_eq!(second_build.unwrap(), Some(2), "second build should be #2");

    // Release the builds (only second one matters, first was aborted)
    let _ = release_tx.send(()).await;

    tokio::task::yield_now().await;

    // Verify manager is still running
    assert!(
        !handle.is_finished(),
        "manager should still be running after reload"
    );

    handle.abort();
}
