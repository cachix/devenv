use crate::builder::{BuildContext, BuildTrigger, ShellBuilder};
use crate::config::Config;
use devenv_event_sources::{FileWatcher, FileWatcherConfig};
use devenv_shell::vt_utils::DEFAULT_MAX_SCROLLBACK;
use devenv_shell::{Pty, PtyError, RawModeGuard, get_terminal_size};
use libghostty_vt::fmt::{Format, Formatter, FormatterOptions};
use libghostty_vt::terminal::{Options as TerminalOptions, Terminal};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use thiserror::Error;
use tokio::sync::mpsc as tokio_mpsc;
use tracing::warn;

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
    #[error("terminal error: {0}")]
    Terminal(#[from] libghostty_vt::Error),
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
        messages: tokio_mpsc::Sender<ManagerMessage>,
    ) -> Result<(), ManagerError> {
        let builder = Arc::new(builder);
        let cwd = std::env::current_dir()?;
        let env: HashMap<String, String> = std::env::vars().collect();

        // Set up file watcher early so we can pass handle to initial build
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

        // Set up raw mode for stdin
        let _raw_guard = RawModeGuard::new()?;

        let (event_tx, event_rx) = std::sync::mpsc::channel::<Event>();

        // Spawn stdin reader thread
        let stdin_tx = event_tx.clone();
        std::thread::Builder::new()
            .name("reload-stdin".into())
            .spawn(move || {
                let mut stdin = io::stdin();
                let mut buf = [0u8; 1024];
                loop {
                    match stdin.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if stdin_tx.send(Event::Stdin(buf[..n].to_vec())).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            })
            .expect("failed to spawn reload-stdin thread");

        // Spawn PTY reader thread
        let spawn_pty_reader =
            |pty: Arc<Pty>, generation: u64, pty_tx: std::sync::mpsc::Sender<Event>| {
                std::thread::Builder::new()
                    .name("reload-pty".into())
                    .spawn(move || {
                        let mut buf = [0u8; 4096];
                        loop {
                            let result = pty.read(&mut buf);
                            match result {
                                Ok(0) => {
                                    let _ = pty_tx.send(Event::PtyExit(generation));
                                    break;
                                }
                                Ok(n) => {
                                    if pty_tx
                                        .send(Event::PtyOutput(generation, buf[..n].to_vec()))
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                                Err(_) => {
                                    let _ = pty_tx.send(Event::PtyExit(generation));
                                    break;
                                }
                            }
                        }
                    })
                    .expect("failed to spawn reload-pty thread");
            };
        spawn_pty_reader(initial_pty.clone(), 0, event_tx.clone());

        // Spawn file watcher forwarder
        let watch_tx = event_tx.clone();
        tokio::spawn(async move {
            while let Some(event) = watcher.recv().await {
                if watch_tx.send(Event::FileChange(event.path)).is_err() {
                    break;
                }
            }
        });

        // Move VT processing to a dedicated thread.
        // Terminal is !Send, so all VT access must stay on one thread.
        let vt_event_tx = event_tx.clone();
        let vt_handle = std::thread::spawn(move || -> Result<(), ManagerError> {
            let mut vt = Terminal::new(TerminalOptions {
                cols: size.cols,
                rows: size.rows,
                max_scrollback: DEFAULT_MAX_SCROLLBACK,
            })?;
            let mut stdout = io::stdout();
            let mut pty_generation: u64 = 0;
            let mut building = false;
            let mut pending_changes: Vec<std::path::PathBuf> = Vec::new();

            while let Ok(event) = event_rx.recv() {
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
                        vt.vt_write(&data);
                        stdout.write_all(&data)?;
                        stdout.flush()?;
                    }

                    Event::PtyExit(generation) => {
                        if generation == pty_generation {
                            break;
                        }
                    }

                    Event::FileChange(path) => {
                        if building {
                            tracing::debug!("Build in progress, ignoring file change: {:?}", path);
                            continue;
                        }

                        pending_changes.push(path.clone());

                        let ctx = BuildContext {
                            cwd: cwd.clone(),
                            env: std::env::vars().collect(),
                            trigger: BuildTrigger::FileChanged(path),
                            watcher: watcher_handle.clone(),
                            reload_file: Some(reload_file.clone()),
                        };

                        // Spawn build in background thread
                        building = true;
                        let builder = builder.clone();
                        let build_tx = vt_event_tx.clone();
                        std::thread::spawn(move || {
                            let result = builder.build(&ctx);
                            let _ = build_tx.send(Event::BuildComplete(result));
                        });
                    }

                    Event::BuildComplete(result) => {
                        building = false;

                        let files: Vec<PathBuf> = pending_changes
                            .drain(..)
                            .map(|p| p.strip_prefix(&cwd).map(|p| p.to_path_buf()).unwrap_or(p))
                            .collect();

                        match result {
                            Ok(cmd) => {
                                let state = match Formatter::new(
                                    &vt,
                                    FormatterOptions {
                                        format: Format::Vt,
                                        trim: false,
                                        unwrap: false,
                                    },
                                )
                                .and_then(|mut f| f.format_alloc::<()>(None))
                                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                                {
                                    Ok(s) => s,
                                    Err(e) => {
                                        warn!("failed to dump terminal state: {e}");
                                        String::new()
                                    }
                                };
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
                                            vt_event_tx.clone(),
                                        );

                                        vt = Terminal::new(TerminalOptions {
                                            cols: new_size.cols,
                                            rows: new_size.rows,
                                            max_scrollback: DEFAULT_MAX_SCROLLBACK,
                                        })?;

                                        let _ = new_pty.write_all(state.as_bytes());
                                        let _ = new_pty.flush();

                                        if let Err(e) =
                                            messages.try_send(ManagerMessage::Reloaded { files })
                                        {
                                            tracing::debug!("failed to send Reloaded message: {e}");
                                        }
                                    }
                                    Err(e) => {
                                        if let Err(send_err) =
                                            messages.try_send(ManagerMessage::ReloadFailed {
                                                files,
                                                error: e.to_string(),
                                            })
                                        {
                                            tracing::debug!(
                                                "failed to send ReloadFailed message: {send_err}"
                                            );
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                if let Err(send_err) =
                                    messages.try_send(ManagerMessage::BuildFailed {
                                        files,
                                        error: e.to_string(),
                                    })
                                {
                                    tracing::debug!(
                                        "failed to send BuildFailed message: {send_err}"
                                    );
                                }
                            }
                        }
                    }
                }
            }

            Ok(())
        });

        // Wait for VT thread without blocking the tokio runtime
        tokio::task::spawn_blocking(move || {
            vt_handle
                .join()
                .unwrap_or(Err(ManagerError::Io(io::Error::other(
                    "VT thread panicked",
                ))))
        })
        .await
        .map_err(|_| ManagerError::Io(io::Error::other("join task failed")))??;

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
