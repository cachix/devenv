//! Shell coordinator for TUI integration.
//!
//! ShellCoordinator handles build coordination only. The TUI owns the PTY
//! and terminal.

use crate::builder::{BuildContext, BuildTrigger, ShellBuilder};
use crate::config::Config;
use devenv_activity::Activity;
use devenv_event_sources::{FileWatcher, FileWatcherConfig};
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
    Tui(ShellEvent),
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

fn snapshot_watched_path_states(
    watcher_handle: &devenv_event_sources::WatcherHandle,
) -> HashMap<PathBuf, WatchedPathState> {
    watcher_handle
        .watched_paths()
        .into_iter()
        .map(|path| {
            let state = capture_watched_path_state(&path);
            (path, state)
        })
        .collect()
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: &Path) {
    if !paths.iter().any(|p| p == path) {
        paths.push(path.to_path_buf());
    }
}

fn reconcile_post_rewatch_drift(
    before_rewatch: &HashMap<PathBuf, WatchedPathState>,
    after_rewatch: &HashMap<PathBuf, WatchedPathState>,
    deferred_changes: &mut Vec<PathBuf>,
) -> usize {
    let mut drift_count = 0;

    for (path, before_state) in before_rewatch {
        if let Some(after_state) = after_rewatch.get(path)
            && before_state != after_state
        {
            drift_count += 1;
            push_unique_path(deferred_changes, path);
        }
    }

    drift_count
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
    /// Sends commands to TUI for PTY spawning/swapping.
    /// Receives events from TUI (exit, resize).
    pub async fn run<B: ShellBuilder + 'static>(
        config: Config,
        builder: B,
        command_tx: mpsc::Sender<ShellCommand>,
        mut event_rx: mpsc::Receiver<ShellEvent>,
    ) -> Result<(), CoordinatorError> {
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

        // Send initial spawn command to TUI
        command_tx
            .send(ShellCommand::Spawn {
                command: cmd,
                watch_files,
            })
            .await
            .map_err(|_| CoordinatorError::ChannelClosed)?;

        // Send the actual watched files (populated by builder during build)
        let watched = watcher_handle.watched_paths();
        if !watched.is_empty() {
            let _ = command_tx
                .send(ShellCommand::WatchedFiles { files: watched })
                .await;
        }

        let (event_tx, mut internal_rx) = mpsc::channel::<Event>(100);

        // Forward file watcher events
        let watch_tx = event_tx.clone();
        let watcher_task = tokio::spawn(async move {
            while let Some(event) = watcher.recv().await {
                if watch_tx.send(Event::FileChange(event.path)).await.is_err() {
                    break;
                }
            }
        });

        // Forward TUI events
        let tui_tx = event_tx.clone();
        let tui_forwarder_task = tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if tui_tx.send(Event::Tui(event)).await.is_err() {
                    break;
                }
            }
        });

        // Track the currently running build task for cancellation
        let mut current_build: Option<tokio::task::AbortHandle> = None;
        // Track files that changed and triggered rebuilds
        let mut pending_changes: Vec<PathBuf> = Vec::new();
        // Track watched path state (kind + content hash) to detect real changes.
        let mut path_states = snapshot_watched_path_states(&watcher_handle);
        // Track changes that arrive while a build is running.
        let mut deferred_changes: Vec<PathBuf> = Vec::new();
        // Track if reload is ready (waiting for user to apply)
        let mut reload_ready = false;
        // Track if file watching is paused
        let mut paused = false;
        // Interval for checking if reload file was deleted (user applied reload)
        let mut reload_check_interval =
            tokio::time::interval(std::time::Duration::from_millis(100));
        reload_check_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            // Use select! to handle both events and reload file checks
            let event = tokio::select! {
                event = internal_rx.recv() => {
                    match event {
                        Some(e) => e,
                        None => break,
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
                    // Ignore file changes when paused
                    if paused {
                        tracing::debug!("File watching paused, ignoring change: {:?}", path);
                        continue;
                    }
                    let new_state = capture_watched_path_state(&path);
                    if let Some(old_state) = path_states.get(&path)
                        && *old_state == new_state
                    {
                        tracing::debug!("Watched path unchanged: {:?}", path);
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
                        .send(ShellCommand::Building {
                            changed_files: relative_files.clone(),
                        })
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

                    let before_rewatch = snapshot_watched_path_states(&watcher_handle);

                    // Refresh all inotify watches. Editors using atomic save
                    // (write temp + rename) replace the file inode, which
                    // silently invalidates the kernel-level inotify watch.
                    // The watchexec diff logic won't re-watch paths it thinks
                    // are already watched, so we force a full refresh.
                    watcher_handle.rewatch_all().await;

                    let after_rewatch = snapshot_watched_path_states(&watcher_handle);
                    let rewatch_drift_count = reconcile_post_rewatch_drift(
                        &before_rewatch,
                        &after_rewatch,
                        &mut deferred_changes,
                    );
                    if rewatch_drift_count > 0 {
                        tracing::warn!(
                            "Detected {} watched-path changes during rewatch gap; scheduling catch-up rebuild",
                            rewatch_drift_count
                        );
                    }
                    path_states = after_rewatch;

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

                    if command_tx.send(cmd).await.is_err() {
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
                        .send(ShellCommand::Building {
                            changed_files: relative_files.clone(),
                        })
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
                    if command_tx.send(ShellCommand::ReloadApplied).await.is_err() {
                        break;
                    }
                }

                Event::Tui(ShellEvent::Exited { .. }) => {
                    // Shell exited, we're done
                    break;
                }

                Event::Tui(ShellEvent::Resize { .. }) => {
                    // Resize is handled by TUI directly on the PTY
                    // We might use this for future features
                }

                Event::Tui(ShellEvent::TogglePause) => {
                    paused = !paused;
                    tracing::debug!(
                        "File watching {}",
                        if paused { "paused" } else { "resumed" }
                    );
                    let _ = command_tx
                        .send(ShellCommand::WatchingPaused { paused })
                        .await;
                }

                Event::Tui(ShellEvent::ListWatchedFiles) => {
                    let files = watcher_handle.watched_paths();
                    let _ = command_tx
                        .send(ShellCommand::PrintWatchedFiles { files })
                        .await;
                }
            }
        }

        // Abort any running build task
        if let Some(handle) = current_build.take() {
            handle.abort();
            builder.interrupt();
        }

        // Abort forwarder tasks to prevent panics during runtime shutdown
        watcher_task.abort();
        tui_forwarder_task.abort();

        // Send shutdown command
        let _ = command_tx.send(ShellCommand::Shutdown).await;

        Ok(())
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

    #[test]
    fn test_reconcile_post_rewatch_drift_enqueues_changed_paths() {
        let mut before = HashMap::new();
        let mut after = HashMap::new();

        let a = PathBuf::from("/tmp/a");
        let b = PathBuf::from("/tmp/b");
        let c = PathBuf::from("/tmp/c");

        before.insert(a.clone(), WatchedPathState::File("h1".to_string()));
        before.insert(b.clone(), WatchedPathState::Directory("d1".to_string()));
        before.insert(c.clone(), WatchedPathState::Missing);

        after.insert(a.clone(), WatchedPathState::File("h2".to_string()));
        after.insert(b.clone(), WatchedPathState::Directory("d1".to_string()));
        after.insert(c.clone(), WatchedPathState::File("h3".to_string()));

        let mut deferred_changes = vec![a.clone()];
        let drift = reconcile_post_rewatch_drift(&before, &after, &mut deferred_changes);

        assert_eq!(drift, 2);
        assert_eq!(deferred_changes.len(), 2);
        assert!(deferred_changes.contains(&a));
        assert!(deferred_changes.contains(&c));
    }
}
