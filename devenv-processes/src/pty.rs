use crossterm::terminal;
use devenv_activity::ActivityRef;
use miette::{IntoDiagnostic, Result, WrapErr, miette};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tracing::{debug, error};

const EXPANDED_LOG_GUTTER_WIDTH: u16 = 8;

fn current_pty_size() -> PtySize {
    let (cols, rows) = terminal::size().unwrap_or((80, 24));
    PtySize {
        rows,
        cols: cols.saturating_sub(EXPANDED_LOG_GUTTER_WIDTH).max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

pub struct PtyProcess {
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    pid: Option<u32>,
    exit_rx: tokio::sync::watch::Receiver<Option<i32>>,
    reader_thread: Option<thread::JoinHandle<()>>,
    waiter_thread: Option<thread::JoinHandle<()>>,
}

impl PtyProcess {
    pub fn spawn(
        bash: &str,
        exec: &str,
        args: &[String],
        cwd: Option<&PathBuf>,
        env: &HashMap<String, String>,
        log_path: &Path,
        activity: Option<ActivityRef>,
    ) -> Result<Self> {
        debug!("Spawning PTY process via {} -c ...", bash);

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(current_pty_size())
            .map_err(|e| miette!("Failed to open PTY: {}", e))?;

        let mut cmd = CommandBuilder::new(bash);
        cmd.arg("-c");
        cmd.arg(exec);
        cmd.args(args);

        if let Some(dir) = cwd {
            cmd.cwd(dir);
        } else if let Ok(current) = std::env::current_dir() {
            cmd.cwd(current);
        }

        for (key, value) in env {
            cmd.env(key, value);
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| miette!("Failed to spawn PTY command: {}", e))?;
        let pid = child.process_id();

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| miette!("Failed to clone PTY reader: {}", e))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| miette!("Failed to acquire PTY writer: {}", e))?;

        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .into_diagnostic()
            .wrap_err(format!(
                "Failed to open PTY log file {}",
                log_path.display()
            ))?;

        let reader_thread = thread::Builder::new()
            .name("devenv-pty-reader".into())
            .spawn(move || {
                let mut reader = reader;
                let mut log_file = log_file;
                let mut buffer = [0u8; 8192];
                let activity = activity;

                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => {
                            if let Some(activity) = &activity {
                                activity.raw_log(buffer[..n].to_vec());
                            }
                            if let Err(err) = log_file.write_all(&buffer[..n]) {
                                error!("PTY reader failed to append to log file: {}", err);
                                break;
                            }
                            let _ = log_file.flush();
                        }
                        Err(err) => {
                            error!("PTY reader failed: {}", err);
                            break;
                        }
                    }
                }
            })
            .map_err(|e| miette!("Failed to spawn PTY reader thread: {}", e))?;

        let child = Arc::new(Mutex::new(child));
        let (exit_tx, exit_rx) = tokio::sync::watch::channel(None);
        let waiter_child = Arc::clone(&child);
        let waiter_thread = thread::Builder::new()
            .name("devenv-pty-waiter".into())
            .spawn(move || {
                loop {
                    let wait_result = {
                        let mut child = waiter_child.lock().unwrap();
                        child.try_wait()
                    };

                    match wait_result {
                        Ok(Some(status)) => {
                            let _ = exit_tx.send(Some(status.exit_code() as i32));
                            break;
                        }
                        Ok(None) => thread::sleep(Duration::from_millis(100)),
                        Err(err) => {
                            error!("PTY waiter failed: {}", err);
                            let _ = exit_tx.send(Some(1));
                            break;
                        }
                    }
                }
            })
            .map_err(|e| miette!("Failed to spawn PTY waiter thread: {}", e))?;

        Ok(Self {
            master: Arc::new(Mutex::new(pair.master)),
            child,
            writer: Arc::new(Mutex::new(writer)),
            pid,
            exit_rx,
            reader_thread: Some(reader_thread),
            waiter_thread: Some(waiter_thread),
        })
    }

    pub fn exit_status(&self) -> tokio::sync::watch::Receiver<Option<i32>> {
        self.exit_rx.clone()
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        let size = PtySize {
            rows,
            cols: cols.saturating_sub(EXPANDED_LOG_GUTTER_WIDTH).max(1),
            pixel_width: 0,
            pixel_height: 0,
        };
        self.master
            .lock()
            .unwrap()
            .resize(size)
            .map_err(|e| miette!("Failed to resize PTY: {}", e))
    }

    pub fn send_input(&self, data: &[u8]) -> Result<()> {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(data).into_diagnostic()?;
        writer.flush().into_diagnostic()?;
        Ok(())
    }

    pub async fn kill(&mut self) -> Result<()> {
        if let Some(pid) = self.pid {
            let target = Pid::from_raw(-(pid as i32));
            match kill(target, Signal::SIGTERM) {
                Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
                Err(e) => return Err(miette!("Failed to send SIGTERM to process group: {}", e)),
            }

            let mut exit_rx = self.exit_rx.clone();
            let exited = tokio::time::timeout(Duration::from_secs(5), async move {
                loop {
                    if exit_rx.borrow().is_some() {
                        return;
                    }
                    if exit_rx.changed().await.is_err() {
                        return;
                    }
                }
            })
            .await;

            if exited.is_err() {
                match kill(target, Signal::SIGKILL) {
                    Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
                    Err(e) => {
                        return Err(miette!("Failed to send SIGKILL to process group: {}", e));
                    }
                }
            }
        } else {
            let mut child = self.child.lock().unwrap();
            child.kill().into_diagnostic()?;
        }

        self.join_threads();
        Ok(())
    }

    fn join_threads(&mut self) {
        if let Some(handle) = self.reader_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.waiter_thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for PtyProcess {
    fn drop(&mut self) {
        self.join_threads();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pty_spawn_echo() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("echo.log");
        let pty = PtyProcess::spawn(
            "bash",
            "echo hello",
            &[],
            None,
            &HashMap::new(),
            &log_path,
            None,
        )
        .expect("Failed to spawn PTY process");

        let mut rx = pty.exit_status();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if rx.borrow().is_some() {
                        break;
                    }
                    rx.changed()
                        .await
                        .expect("PTY exit watcher closed unexpectedly");
                }
            })
            .await
            .expect("PTY process did not exit");
        });
    }
}
