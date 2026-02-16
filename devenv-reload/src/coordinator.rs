//! Shell coordinator for TUI integration.
//!
//! Unlike ShellManager which owns the PTY and terminal, ShellCoordinator
//! only handles build coordination. The TUI owns the PTY and terminal.

use crate::builder::{BuildContext, BuildTrigger, ShellBuilder};
use crate::config::Config;
use devenv_activity::Activity;
use devenv_event_sources::{FileWatcher, FileWatcherConfig};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;

// Re-export protocol types from devenv-shell
pub use devenv_shell::{PtyTaskRequest, PtyTaskResult, ShellCommand, ShellEvent};

/// Compute blake3 hash of file contents.
/// Returns None if the file cannot be read.
fn hash_file(path: &Path) -> Option<blake3::Hash> {
    let content = std::fs::read(path).ok()?;
    Some(blake3::hash(&content))
}

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
        // Track file content hashes to detect actual changes
        let mut file_hashes: HashMap<PathBuf, blake3::Hash> = HashMap::new();
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
                    // No longer in ready state when new file changes come in
                    reload_ready = false;
                    // Check if file content actually changed by comparing hashes
                    let new_hash = match hash_file(&path) {
                        Some(h) => h,
                        None => {
                            tracing::debug!("Could not read file: {:?}", path);
                            continue;
                        }
                    };

                    if let Some(old_hash) = file_hashes.get(&path) {
                        if *old_hash == new_hash {
                            tracing::debug!("File unchanged (same hash): {:?}", path);
                            continue;
                        }
                    }

                    // Update stored hash
                    file_hashes.insert(path.clone(), new_hash);

                    tracing::debug!("File content changed: {:?}", path);

                    // Track the file that triggered this rebuild
                    pending_changes.push(path.clone());

                    // Cancel any running build
                    if let Some(handle) = current_build.take() {
                        handle.abort();
                    }

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
                    let activity = Activity::operation("Reloading shell")
                        .detail(files_display.join(", "))
                        .start();

                    let ctx = BuildContext {
                        cwd: cwd.clone(),
                        env: std::env::vars().collect(),
                        trigger: BuildTrigger::FileChanged(path),
                        watcher: watcher_handle.clone(),
                        reload_file: Some(reload_file.clone()),
                    };

                    // Spawn build in background task - use build_reload_env for hot-reload
                    let builder = builder.clone();
                    let build_tx = event_tx.clone();
                    let handle = tokio::spawn(async move {
                        let result =
                            tokio::task::spawn_blocking(move || builder.build_reload_env(&ctx))
                                .await
                                .unwrap_or_else(|e| {
                                    Err(crate::builder::BuildError::new(format!(
                                        "build task panicked: {}",
                                        e
                                    )))
                                });
                        let _ = build_tx
                            .send(Event::ReloadBuildComplete { result, activity })
                            .await;
                    });
                    current_build = Some(handle.abort_handle());
                }

                Event::ReloadBuildComplete { result, activity } => {
                    current_build = None;

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

    #[test]
    fn test_coordinator_error_display() {
        let err = CoordinatorError::ChannelClosed;
        assert_eq!(format!("{}", err), "channel closed");
    }
}
