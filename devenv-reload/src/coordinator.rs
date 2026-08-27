//! Shell coordinator for TUI integration.
//!
//! ShellCoordinator handles build coordination only. The TUI owns the PTY
//! and terminal.

use crate::builder::{BuildContext, BuildTrigger, ShellBuilder};
use crate::config::Config;
use devenv_activity::Activity;
use devenv_event_sources::{FileWatcher, FileWatcherConfig};
use devenv_mailbox::{FrontendCommand, FrontendEvent};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;

// Re-export protocol types from devenv-shell
pub use devenv_shell::{ShellCommand, ShellEvent};

#[derive(Debug, Error)]
pub enum CoordinatorError {
    #[error("build failed: {0}")]
    Build(#[source] crate::builder::BuildError),
    #[error("channel closed")]
    ChannelClosed,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

enum Event {
    FileChange(PathBuf),
    /// Reload build completed (env written to file)
    ReloadBuildComplete {
        result: Result<(), crate::builder::BuildError>,
        /// The activity tracking this reload (dropped to complete it)
        activity: Activity,
    },
    /// Reload file was deleted (user applied the reload)
    ReloadFileDeleted,
    Shell(ShellEvent),
}

/// Shell coordinator for TUI mode.
///
/// Coordinates shell builds and file watching, but does not own the PTY.
/// The TUI is responsible for PTY management and terminal I/O.
pub struct ShellCoordinator;

#[derive(Debug, Clone, PartialEq, Eq)]
enum WatchedPathState {
    File(String),
    Directory(String),
    Missing,
    Unreadable,
}

fn hash_directory_listing(path: &Path) -> std::io::Result<String> {
    devenv_cache_core::file::compute_directory_hash(path)
        .map(|hash| hash.unwrap_or_default())
        .map_err(std::io::Error::other)
}

/// Reads and hashes the path, walking the whole tree for a directory —
/// event-loop callers go through [`capture_watched_path_states`].
fn capture_watched_path_state(path: &Path) -> WatchedPathState {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return WatchedPathState::Missing,
        Err(_) => return WatchedPathState::Unreadable,
    };

    if metadata.is_dir() {
        match hash_directory_listing(path) {
            Ok(hash) => WatchedPathState::Directory(hash),
            Err(_) => WatchedPathState::Unreadable,
        }
    } else {
        match devenv_cache_core::compute_file_hash(path) {
            Ok(hash) => WatchedPathState::File(hash),
            Err(_) => WatchedPathState::Unreadable,
        }
    }
}

async fn capture_watched_path_states(paths: Vec<PathBuf>) -> HashMap<PathBuf, WatchedPathState> {
    tokio::task::spawn_blocking(move || {
        paths
            .into_iter()
            .map(|path| {
                let state = capture_watched_path_state(&path);
                (path, state)
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

async fn snapshot_watched_path_states(
    watcher_handle: &devenv_event_sources::WatcherHandle,
) -> HashMap<PathBuf, WatchedPathState> {
    capture_watched_path_states(watcher_handle.watched_paths()).await
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: &Path) {
    if !paths.iter().any(|p| p == path) {
        paths.push(path.to_path_buf());
    }
}

fn launch_reload_build<B: ShellBuilder + 'static>(
    builder: Arc<B>,
    event_tx: mpsc::Sender<Event>,
    ctx: BuildContext,
    activity: Activity,
) -> tokio::task::AbortHandle {
    let handle = tokio::spawn(async move {
        let result = tokio::task::spawn_blocking(move || builder.build_reload_env(&ctx))
            .await
            .unwrap_or_else(|e| {
                Err(crate::builder::BuildError::new(format!(
                    "build task panicked: {}",
                    e
                )))
            });
        let _ = event_tx
            .send(Event::ReloadBuildComplete { result, activity })
            .await;
    });

    handle.abort_handle()
}

impl ShellCoordinator {
    /// Run the shell coordinator.
    ///
    /// Sends commands to the session for PTY spawning/swapping and receives
    /// its events (exit, resize). Returns when the session exits or
    /// disconnects, with the shell's exit code if it reported one.
    pub async fn run<B: ShellBuilder + 'static>(
        config: Config,
        builder: B,
        command_tx: mpsc::Sender<FrontendCommand>,
        mut event_rx: mpsc::Receiver<FrontendEvent>,
    ) -> Result<Option<u32>, CoordinatorError> {
        let builder = Arc::new(builder);
        let cwd = std::env::current_dir()?;

        // Set up file watcher
        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: &config.watch_files,
                recursive: false,
                ..Default::default()
            },
            "devenv-reload",
        )
        .await;
        let watcher_handle = watcher.handle();

        // Collect watch files for reporting
        let watch_files: Vec<PathBuf> = config.watch_files.clone();
        let reload_file = config.reload_file.clone();

        // Initial build - run in spawn_blocking since builder may block
        let ctx = BuildContext {
            cwd: cwd.clone(),
            env: std::env::vars().collect(),
            trigger: BuildTrigger::Initial,
            watcher: watcher_handle.clone(),
            reload_file: Some(reload_file.clone()),
        };

        let builder_clone = builder.clone();
        let cmd = tokio::task::spawn_blocking(move || builder_clone.build(&ctx))
            .await
            .map_err(|e| {
                CoordinatorError::Build(crate::builder::BuildError::new(format!(
                    "build task panicked: {}",
                    e
                )))
            })?
            .map_err(CoordinatorError::Build)?;

        // The build phase is over: release the terminal renderer before
        // queueing the shell command. The frontend consumes ExitRenderer and
        // hands this same receiver to ShellSession, leaving Spawn queued.
        command_tx
            .send(FrontendCommand::ExitRenderer)
            .await
            .map_err(|_| CoordinatorError::ChannelClosed)?;

        command_tx
            .send(FrontendCommand::Shell(ShellCommand::Spawn {
                command: cmd,
                watch_files,
            }))
            .await
            .map_err(|_| CoordinatorError::ChannelClosed)?;

        // Send the actual watched files (populated by builder during build)
        let watched = watcher_handle.watched_paths();
        if !watched.is_empty() {
            let _ = command_tx
                .send(FrontendCommand::Shell(ShellCommand::WatchedFiles {
                    files: watched,
                }))
                .await;
        }

        // Carries build completions from the tasks spawned by
        // `launch_reload_build`; the other sources are selected on directly.
        let (event_tx, mut internal_rx) = mpsc::channel::<Event>(100);

        // Track the currently running build task for cancellation
        let mut current_build: Option<tokio::task::AbortHandle> = None;
        // The shell's exit code, once the session reports it
        let mut shell_exit_code: Option<u32> = None;
        // Track files that changed and triggered rebuilds
        let mut pending_changes: Vec<PathBuf> = Vec::new();
        // Track watched path state (kind + content hash) to detect real changes.
        let mut path_states = snapshot_watched_path_states(&watcher_handle).await;
        // Track changes that arrive while a build is running.
        let mut deferred_changes: Vec<PathBuf> = Vec::new();
        // Track if reload is ready (waiting for user to apply)
        let mut reload_ready = false;
        // Track if file watching is paused
        let mut paused = false;
        // A dead watcher stops file-change handling but not the session.
        let mut watcher_alive = true;
        // Interval for checking if reload file was deleted (user applied reload)
        let mut reload_check_interval =
            tokio::time::interval(std::time::Duration::from_millis(100));
        reload_check_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            // All sources are cancel-safe, so a losing arm resumes untouched
            // on the next iteration.
            let event = tokio::select! {
                event = internal_rx.recv() => {
                    match event {
                        Some(e) => e,
                        None => break,
                    }
                }
                event = event_rx.recv() => {
                    match event {
                        Some(FrontendEvent::Shell(e)) => Event::Shell(e),
                        // Process controls belong to the renderer phase. A
                        // queued input racing the renderer handoff is stale.
                        Some(FrontendEvent::Process(_)) => continue,
                        // The session dropped its sender without reporting an
                        // exit, so this loop must not outlive it.
                        None => break,
                    }
                }
                change = watcher.recv(), if watcher_alive => {
                    match change {
                        Some(change) => Event::FileChange(change.path),
                        None => {
                            watcher_alive = false;
                            continue;
                        }
                    }
                }
                _ = reload_check_interval.tick(), if reload_ready => {
                    // Check if reload file was deleted (user applied reload)
                    if !reload_file.exists() {
                        Event::ReloadFileDeleted
                    } else {
                        continue;
                    }
                }
            };

            match event {
                Event::FileChange(path) => {
                    // A logical target may be subscribed through an existing
                    // ancestor while it is missing. Advance that OS anchor
                    // even when paused or when the target's content state is
                    // still Missing (for example, after an intermediate
                    // directory appears).
                    watcher_handle.refresh(&path).await;

                    // Ignore file changes when paused
                    if paused {
                        tracing::trace!("File watching paused, ignoring change: {:?}", path);
                        continue;
                    }
                    let new_state = capture_watched_path_states(vec![path.clone()])
                        .await
                        .remove(&path)
                        .unwrap_or(WatchedPathState::Unreadable);
                    if let Some(old_state) = path_states.get(&path)
                        && *old_state == new_state
                    {
                        tracing::trace!("Watched path unchanged: {:?}", path);
                        continue;
                    }

                    if matches!(
                        new_state,
                        WatchedPathState::Missing | WatchedPathState::Unreadable
                    ) {
                        tracing::warn!(
                            "Watched path became unavailable, forcing reload: {:?}",
                            path
                        );
                    }

                    // Content actually changed: no longer in ready state.
                    // Must be after the hash check: spurious watcher events
                    // (unchanged content) must not disable reload_file polling,
                    // otherwise the status line gets stuck on "Reload ready".
                    reload_ready = false;

                    // Update stored path state
                    path_states.insert(path.clone(), new_state);

                    tracing::debug!("File content changed: {:?}", path);

                    // If a build is already running, drop the event.
                    // spawn_blocking tasks cannot actually be cancelled, so
                    // aborting and restarting would accumulate zombie builds
                    // that can cascade into more file changes (fork bomb).
                    if current_build.is_some() {
                        push_unique_path(&mut deferred_changes, &path);
                        tracing::debug!("Build in progress, deferring file change: {:?}", path);
                        continue;
                    }

                    // Track the file that triggered this rebuild
                    pending_changes.push(path.clone());

                    // Notify TUI that build has started
                    let relative_files: Vec<PathBuf> = pending_changes
                        .iter()
                        .map(|p| {
                            p.strip_prefix(&cwd)
                                .map(|p| p.to_path_buf())
                                .unwrap_or(p.clone())
                        })
                        .collect();
                    let _ = command_tx
                        .send(FrontendCommand::Shell(ShellCommand::Building {
                            changed_files: relative_files.clone(),
                        }))
                        .await;

                    // Create activity for tracking the reload in the TUI
                    let files_display: Vec<String> = relative_files
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect();
                    let activity = devenv_activity::start!(
                        Activity::operation("Reloading shell").detail(files_display.join(", "))
                    );

                    let ctx = BuildContext {
                        cwd: cwd.clone(),
                        env: std::env::vars().collect(),
                        trigger: BuildTrigger::FileChanged(path),
                        watcher: watcher_handle.clone(),
                        reload_file: Some(reload_file.clone()),
                    };

                    current_build = Some(launch_reload_build(
                        builder.clone(),
                        event_tx.clone(),
                        ctx,
                        activity,
                    ));
                }

                Event::ReloadBuildComplete { result, activity } => {
                    current_build = None;

                    // The build may discover new dependencies or change a
                    // missing target into a file. Snapshot logical targets
                    // once; OS subscriptions are maintained independently.
                    path_states = snapshot_watched_path_states(&watcher_handle).await;

                    let watched_set: HashSet<PathBuf> = path_states.keys().cloned().collect();
                    deferred_changes.retain(|p| watched_set.contains(p));

                    // Collect changed files as relative paths
                    let files: Vec<PathBuf> = pending_changes
                        .drain(..)
                        .map(|p| p.strip_prefix(&cwd).map(|p| p.to_path_buf()).unwrap_or(p))
                        .collect();

                    let cmd = match &result {
                        Ok(()) => {
                            reload_ready = true;
                            ShellCommand::ReloadReady {
                                changed_files: files,
                            }
                        }
                        Err(e) => {
                            activity.fail();
                            ShellCommand::BuildFailed {
                                changed_files: files,
                                error: e.to_string(),
                            }
                        }
                    };
                    // Activity completes on drop (success by default, or failed if marked)
                    drop(activity);

                    if command_tx.send(FrontendCommand::Shell(cmd)).await.is_err() {
                        // TUI disconnected
                        break;
                    }

                    if paused || deferred_changes.is_empty() {
                        continue;
                    }

                    let mut changed_files = Vec::new();
                    std::mem::swap(&mut changed_files, &mut deferred_changes);

                    // Use first deferred path as trigger; include all deferred
                    // paths in UI reporting for this catch-up rebuild.
                    let trigger = changed_files[0].clone();
                    pending_changes.extend(changed_files);

                    let relative_files: Vec<PathBuf> = pending_changes
                        .iter()
                        .map(|p| {
                            p.strip_prefix(&cwd)
                                .map(|p| p.to_path_buf())
                                .unwrap_or(p.clone())
                        })
                        .collect();
                    let _ = command_tx
                        .send(FrontendCommand::Shell(ShellCommand::Building {
                            changed_files: relative_files.clone(),
                        }))
                        .await;

                    let files_display: Vec<String> = relative_files
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect();
                    let activity = devenv_activity::start!(
                        Activity::operation("Reloading shell").detail(files_display.join(", "))
                    );

                    let ctx = BuildContext {
                        cwd: cwd.clone(),
                        env: std::env::vars().collect(),
                        trigger: BuildTrigger::FileChanged(trigger),
                        watcher: watcher_handle.clone(),
                        reload_file: Some(reload_file.clone()),
                    };

                    current_build = Some(launch_reload_build(
                        builder.clone(),
                        event_tx.clone(),
                        ctx,
                        activity,
                    ));
                }

                Event::ReloadFileDeleted => {
                    // User applied the reload (pressed keybind), clear status line
                    reload_ready = false;
                    if command_tx
                        .send(FrontendCommand::Shell(ShellCommand::ReloadApplied))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }

                Event::Shell(ShellEvent::Exited { exit_code }) => {
                    // Shell exited, we're done
                    shell_exit_code = exit_code;
                    break;
                }

                Event::Shell(ShellEvent::Resize { .. }) => {
                    // Resize is handled by TUI directly on the PTY
                    // We might use this for future features
                }

                Event::Shell(ShellEvent::TogglePause) => {
                    paused = !paused;
                    tracing::debug!(
                        "File watching {}",
                        if paused { "paused" } else { "resumed" }
                    );
                    let _ = command_tx
                        .send(FrontendCommand::Shell(ShellCommand::WatchingPaused {
                            paused,
                        }))
                        .await;
                }

                Event::Shell(ShellEvent::ListWatchedFiles) => {
                    let files = watcher_handle.watched_paths();
                    let _ = command_tx
                        .send(FrontendCommand::Shell(ShellCommand::PrintWatchedFiles {
                            files,
                        }))
                        .await;
                }
            }
        }

        // Abort any running build task
        if let Some(handle) = current_build.take() {
            handle.abort();
            builder.interrupt();
        }

        // Send shutdown command
        let _ = command_tx
            .send(FrontendCommand::Shell(ShellCommand::Shutdown))
            .await;

        Ok(shell_exit_code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_coordinator_error_display() {
        let err = CoordinatorError::ChannelClosed;
        assert_eq!(format!("{}", err), "channel closed");
    }

    #[test]
    fn test_capture_watched_path_state_detects_dir_to_file_transition() {
        let temp = TempDir::new().expect("create temp dir");
        let path = temp.path().join("watched");

        std::fs::create_dir(&path).expect("create dir");
        let before = capture_watched_path_state(&path);
        assert!(matches!(before, WatchedPathState::Directory(_)));

        std::fs::remove_dir(&path).expect("remove dir");
        std::fs::write(&path, "now a file").expect("write file");
        let after = capture_watched_path_state(&path);
        assert!(matches!(after, WatchedPathState::File(_)));
        assert_ne!(before, after);
    }

    #[test]
    fn test_capture_watched_path_state_detects_directory_removal() {
        let temp = TempDir::new().expect("create temp dir");
        let path = temp.path().join("watched-dir");

        std::fs::create_dir(&path).expect("create dir");
        assert!(matches!(
            capture_watched_path_state(&path),
            WatchedPathState::Directory(_)
        ));

        std::fs::remove_dir(&path).expect("remove dir");
        assert!(matches!(
            capture_watched_path_state(&path),
            WatchedPathState::Missing
        ));
    }

    #[test]
    fn test_capture_watched_path_state_detects_directory_child_content_change() {
        let temp = TempDir::new().expect("create temp dir");
        let path = temp.path().join("watched-dir");
        std::fs::create_dir(&path).expect("create dir");

        let child = path.join("child.nix");
        std::fs::write(&child, "before").expect("write initial child content");

        let before = capture_watched_path_state(&path);
        assert!(matches!(before, WatchedPathState::Directory(_)));

        std::fs::write(&child, "after").expect("overwrite child content");

        let after = capture_watched_path_state(&path);
        assert!(matches!(after, WatchedPathState::Directory(_)));
        assert_ne!(before, after);
    }
}
