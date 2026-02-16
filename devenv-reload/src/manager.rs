use crate::builder::{BuildContext, BuildTrigger, ShellBuilder};
use crate::config::Config;
use avt::Vt;
use devenv_event_sources::{FileWatcher, FileWatcherConfig};
use devenv_shell::{Pty, PtyError, RawModeGuard, get_terminal_size};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use thiserror::Error;
use tokio::sync::mpsc;

/// Messages sent by the shell manager to notify consumers of events.
#[derive(Debug, Clone)]
pub enum ManagerMessage {
    /// Environment was successfully reloaded after file changes.
    Reloaded { files: Vec<PathBuf> },
    /// Reload failed due to PTY spawn error.
    ReloadFailed { files: Vec<PathBuf>, error: String },
    /// Build failed before PTY spawn could be attempted.
    BuildFailed { files: Vec<PathBuf>, error: String },
}

#[derive(Debug, Error)]
pub enum ManagerError {
    #[error("failed to spawn shell: {0}")]
    Spawn(#[source] PtyError),
    #[error("build failed: {0}")]
    Build(#[source] crate::builder::BuildError),
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

enum Event {
    Stdin(Vec<u8>),
    PtyOutput(u64, Vec<u8>),
    PtyExit(u64),
    FileChange(std::path::PathBuf),
    BuildComplete(Result<portable_pty::CommandBuilder, crate::builder::BuildError>),
}

pub struct ShellManager;

impl ShellManager {
    /// Run the shell session with hot-reload capability.
    /// Blocks until the shell exits or a fatal error occurs.
    ///
    /// Messages about reload events are sent to the provided channel.
    pub async fn run<B: ShellBuilder + 'static>(
        config: Config,
        builder: B,
        messages: mpsc::Sender<ManagerMessage>,
    ) -> Result<(), ManagerError> {
        let builder = Arc::new(builder);
        let cwd = std::env::current_dir()?;
        let env: HashMap<String, String> = std::env::vars().collect();

        // Set up file watcher early so we can pass handle to initial build
        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: config.watch_files.clone(),
                recursive: false,
                ..Default::default()
            },
            "devenv-reload",
        )
        .await;
        let watcher_handle = watcher.handle();

        // Initial build
        let reload_file = config.reload_file.clone();
        let ctx = BuildContext {
            cwd: cwd.clone(),
            env: env.clone(),
            trigger: BuildTrigger::Initial,
            watcher: watcher_handle.clone(),
            reload_file: Some(reload_file.clone()),
        };
        let cmd = builder.build(&ctx).map_err(ManagerError::Build)?;

        let size = get_terminal_size();
        let initial_pty = Arc::new(Pty::spawn(cmd, size).map_err(ManagerError::Spawn)?);
        let pty = Arc::new(RwLock::new(initial_pty.clone()));
        let mut pty_generation: u64 = 0;

        // Set up terminal state tracking
        let mut vt = Vt::new(size.cols as usize, size.rows as usize);

        // Set up raw mode for stdin
        let _raw_guard = RawModeGuard::new()?;

        let (event_tx, mut event_rx) = mpsc::channel::<Event>(100);

        // Spawn stdin reader thread
        let stdin_tx = event_tx.clone();
        std::thread::spawn(move || {
            let mut stdin = io::stdin();
            let mut buf = [0u8; 1024];
            loop {
                match stdin.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if stdin_tx
                            .blocking_send(Event::Stdin(buf[..n].to_vec()))
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        // Spawn PTY reader thread
        let pty_tx = event_tx.clone();
        let spawn_pty_reader = |pty: Arc<Pty>, generation: u64, pty_tx: mpsc::Sender<Event>| {
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    let result = pty.read(&mut buf);
                    match result {
                        Ok(0) => {
                            let _ = pty_tx.blocking_send(Event::PtyExit(generation));
                            break;
                        }
                        Ok(n) => {
                            if pty_tx
                                .blocking_send(Event::PtyOutput(generation, buf[..n].to_vec()))
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(_) => {
                            let _ = pty_tx.blocking_send(Event::PtyExit(generation));
                            break;
                        }
                    }
                }
            });
        };
        spawn_pty_reader(initial_pty.clone(), pty_generation, pty_tx);

        // Spawn file watcher forwarder
        let watch_tx = event_tx.clone();
        tokio::spawn(async move {
            while let Some(event) = watcher.recv().await {
                if watch_tx.send(Event::FileChange(event.path)).await.is_err() {
                    break;
                }
            }
        });

        let mut stdout = io::stdout();

        // Track the currently running build task for cancellation
        let mut current_build: Option<tokio::task::AbortHandle> = None;
        // Track files that changed and triggered rebuilds
        let mut pending_changes: Vec<std::path::PathBuf> = Vec::new();

        while let Some(event) = event_rx.recv().await {
            match event {
                Event::Stdin(data) => {
                    let current_pty = pty.read().unwrap().clone();
                    let _ = current_pty.write_all(&data);
                    let _ = current_pty.flush();
                }

                Event::PtyOutput(generation, data) => {
                    if generation != pty_generation {
                        continue;
                    }
                    vt.feed_str(&String::from_utf8_lossy(&data));
                    stdout.write_all(&data)?;
                    stdout.flush()?;
                }

                Event::PtyExit(generation) => {
                    if generation == pty_generation {
                        break;
                    }
                }

                Event::FileChange(path) => {
                    // Track the file that triggered this rebuild
                    pending_changes.push(path.clone());

                    // Cancel any running build
                    if let Some(handle) = current_build.take() {
                        handle.abort();
                    }

                    let ctx = BuildContext {
                        cwd: cwd.clone(),
                        env: std::env::vars().collect(),
                        trigger: BuildTrigger::FileChanged(path),
                        watcher: watcher_handle.clone(),
                        reload_file: Some(reload_file.clone()),
                    };

                    // Spawn build in background task
                    let builder = builder.clone();
                    let build_tx = event_tx.clone();
                    let handle = tokio::spawn(async move {
                        let result = tokio::task::spawn_blocking(move || builder.build(&ctx))
                            .await
                            .unwrap_or_else(|e| {
                                Err(crate::builder::BuildError::new(format!(
                                    "build task panicked: {}",
                                    e
                                )))
                            });
                        let _ = build_tx.send(Event::BuildComplete(result)).await;
                    });
                    current_build = Some(handle.abort_handle());
                }

                Event::BuildComplete(result) => {
                    current_build = None;

                    // Collect changed files as relative paths
                    let files: Vec<PathBuf> = pending_changes
                        .drain(..)
                        .map(|p| p.strip_prefix(&cwd).map(|p| p.to_path_buf()).unwrap_or(p))
                        .collect();

                    match result {
                        Ok(cmd) => {
                            let state = vt.dump();
                            let new_size = get_terminal_size();

                            match Pty::spawn(cmd, new_size) {
                                Ok(new_pty) => {
                                    let new_pty = Arc::new(new_pty);
                                    let old_pty = {
                                        let mut pty_guard = pty.write().unwrap();
                                        let old = pty_guard.clone();
                                        *pty_guard = new_pty.clone();
                                        old
                                    };
                                    let _ = old_pty.kill();

                                    pty_generation = pty_generation.wrapping_add(1);
                                    spawn_pty_reader(
                                        new_pty.clone(),
                                        pty_generation,
                                        event_tx.clone(),
                                    );

                                    vt = Vt::new(new_size.cols as usize, new_size.rows as usize);

                                    let _ = new_pty.write_all(state.as_bytes());
                                    let _ = new_pty.flush();

                                    let _ = messages.try_send(ManagerMessage::Reloaded { files });
                                }
                                Err(e) => {
                                    let _ = messages.try_send(ManagerMessage::ReloadFailed {
                                        files,
                                        error: e.to_string(),
                                    });
                                }
                            }
                        }
                        Err(e) => {
                            let _ = messages.try_send(ManagerMessage::BuildFailed {
                                files,
                                error: e.to_string(),
                            });
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manager_error_io_from() {
        let io_err = io::Error::new(io::ErrorKind::Other, "test");
        let mgr_err: ManagerError = io_err.into();
        assert!(matches!(mgr_err, ManagerError::Io(_)));
    }

    #[test]
    fn test_manager_error_display() {
        let io_err = io::Error::new(io::ErrorKind::Other, "test error");
        let mgr_err = ManagerError::Io(io_err);
        let display = format!("{}", mgr_err);
        assert!(display.contains("IO error"));
    }
}
