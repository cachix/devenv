use miette::{Result, miette};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use tracing::{debug, error};

const EXPANDED_LOG_GUTTER_WIDTH: u16 = 8;

fn current_pty_size() -> PtySize {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    PtySize {
        rows,
        cols: cols.saturating_sub(EXPANDED_LOG_GUTTER_WIDTH).max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// PTY process wrapper
pub struct PtyProcess {
    pid: Option<u32>,
    reader_thread: Option<thread::JoinHandle<()>>,
    waiter_thread: Option<thread::JoinHandle<()>>,
    running: Arc<Mutex<bool>>,
    exit_rx: tokio::sync::watch::Receiver<Option<i32>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl std::fmt::Debug for PtyProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtyProcess")
            .field("pid", &self.pid)
            .finish()
    }
}

impl PtyProcess {
    /// Spawn a new process with a pseudo-terminal, optionally logging to a file.
    pub fn spawn(
        exec: &str,
        args: &[String],
        cwd: Option<&PathBuf>,
        env: &HashMap<String, String>,
        log_path: Option<PathBuf>,
        activity_ref: Option<devenv_activity::ActivityRef>,
    ) -> Result<Self> {
        debug!("Spawning PTY process: {} {:?}", exec, args);

        // Get the native PTY system
        let pty_system = native_pty_system();

        // Create a new PTY
        let pair = pty_system
            .openpty(current_pty_size())
            .map_err(|e| miette!("Failed to open PTY: {}", e))?;

        // Build the command
        let mut cmd = CommandBuilder::new(exec);
        cmd.args(args);

        // Set working directory - fall back to current dir if not specified
        // (portable_pty falls back to HOME which may not exist in sandboxes)
        if let Some(dir) = cwd {
            cmd.cwd(dir);
        } else if let Ok(current) = std::env::current_dir() {
            cmd.cwd(current);
        }

        for (key, value) in env {
            cmd.env(key, value);
        }

        // Spawn the child process
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| miette!("Failed to spawn command: {}", e))?;
        let pid = child.process_id();
        debug!("PTY process spawned successfully with PID {:?}", pid);

        // Set up a reader thread to forward PTY output to stdout and optionally log file
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| miette!("Failed to take PTY writer: {}", e))?;
        let master = Arc::new(Mutex::new(pair.master));
        let writer = Arc::new(Mutex::new(writer));
        let mut reader = master
            .lock()
            .unwrap()
            .try_clone_reader()
            .map_err(|e| miette!("Failed to clone PTY reader: {}", e))?;
        let running = Arc::new(Mutex::new(true));
        let running_clone = Arc::clone(&running);

        let reader_thread = thread::spawn(move || {
            let mut buffer = [0u8; 8192];

            // Open log file if provided
            let mut log_file = log_path.and_then(|path| {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .map_err(|e| {
                        error!("Failed to open log file {}: {}", path.display(), e);
                        e
                    })
                    .ok()
            });

            loop {
                // Check if we should stop
                {
                    let is_running = running_clone.lock().unwrap();
                    if !*is_running {
                        break;
                    }
                }

                match reader.read(&mut buffer) {
                    Ok(0) => {
                        // EOF reached
                        debug!("PTY reader: EOF");
                        break;
                    }
                    Ok(n) => {
                        // Write to log file if available
                        if let Some(ref mut file) = log_file {
                            let _ = file.write_all(&buffer[..n]);
                            let _ = file.flush();
                        }
                        if let Some(ref activity) = activity_ref {
                            activity.raw_log(buffer[..n].to_vec());
                        }
                    }
                    Err(e) => {
                        error!("PTY reader: Read error: {}", e);
                        break;
                    }
                }
            }
            debug!("PTY reader thread exiting");
        });

        let (exit_tx, exit_rx) = tokio::sync::watch::channel(None);
        let child: Box<dyn Child + Send> = child;
        let child = Arc::new(Mutex::new(child));
        let child_clone = Arc::clone(&child);

        let waiter_thread = thread::spawn(move || {
            let status = {
                let mut child = child_clone.lock().unwrap();
                child.wait()
            };
            let code = match status {
                Ok(s) => s.exit_code() as i32,
                Err(_) => 1,
            };
            let _ = exit_tx.send(Some(code));
        });

        Ok(Self {
            pid,
            reader_thread: Some(reader_thread),
            waiter_thread: Some(waiter_thread),
            running,
            exit_rx,
            master,
            writer,
        })
    }

    /// Get the process ID
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Subscribe to the exit status
    pub fn exit_status(&self) -> tokio::sync::watch::Receiver<Option<i32>> {
        self.exit_rx.clone()
    }

    /// Resize the PTY to match the visible terminal area.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        let size = PtySize {
            rows: rows.max(1),
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

    /// Write raw input bytes into the PTY.
    pub fn send_input(&self, data: &[u8]) -> Result<()> {
        let mut writer = self.writer.lock().unwrap();
        writer
            .write_all(data)
            .and_then(|_| writer.flush())
            .map_err(|e| miette!("Failed to write PTY input: {}", e))
    }

    /// Wait for the process to exit asynchronously
    pub async fn wait(&mut self) -> Result<i32> {
        let mut rx = self.exit_rx.clone();
        let code = loop {
            if let Some(code) = *rx.borrow() {
                break code;
            }
            rx.changed()
                .await
                .map_err(|_| miette!("Exit channel closed"))?;
        };

        // Signal the reader thread to stop
        {
            let mut running = self.running.lock().unwrap();
            *running = false;
        }

        // Wait for reader thread to finish asynchronously to avoid blocking the executor
        if let Some(handle) = self.reader_thread.take() {
            let _ = tokio::task::spawn_blocking(move || {
                let _ = handle.join();
            })
            .await;
        }

        // Wait for waiter thread to finish asynchronously
        if let Some(handle) = self.waiter_thread.take() {
            let _ = tokio::task::spawn_blocking(move || {
                let _ = handle.join();
            })
            .await;
        }

        debug!("PTY process exited with code {}", code);
        Ok(code)
    }

    /// Gracefully stop the wrapper process and wait for it to exit naturally.
    pub async fn kill(&mut self) -> Result<()> {
        if let Some(pid) = self.pid {
            #[cfg(unix)]
            {
                use nix::errno::Errno;
                use nix::sys::signal::{Signal, kill};
                use nix::unistd::Pid;

                let pid = Pid::from_raw(pid as i32);
                let target = nix::unistd::getpgid(Some(pid))
                    .ok()
                    .map(|pgid| Pid::from_raw(-pgid.as_raw()))
                    .unwrap_or(pid);

                match kill(target, Signal::SIGTERM) {
                    Ok(_) | Err(Errno::ESRCH) => {}
                    Err(e) => {
                        return Err(miette!("Failed to send SIGTERM to process group: {}", e));
                    }
                }

                if tokio::time::timeout(std::time::Duration::from_secs(5), self.wait())
                    .await
                    .is_ok()
                {
                    return Ok(());
                }

                match kill(target, Signal::SIGKILL) {
                    Ok(_) | Err(Errno::ESRCH) => {}
                    Err(e) => {
                        return Err(miette!("Failed to send SIGKILL to process group: {}", e));
                    }
                }

                let _ = self.wait().await?;
                return Ok(());
            }
        }

        let _ = self.wait().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pty_spawn_echo() {
        let mut pty = PtyProcess::spawn(
            "echo",
            &["hello".to_string()],
            None,
            &HashMap::new(),
            None,
            None,
        )
        .expect("Failed to spawn PTY process");

        let exit_code = pty.wait().await.expect("Failed to wait for process");
        assert_eq!(exit_code, 0);
    }

    #[tokio::test]
    async fn test_pty_get_pid() {
        let pty = PtyProcess::spawn(
            "sleep",
            &["0.1".to_string()],
            None,
            &HashMap::new(),
            None,
            None,
        )
        .expect("Failed to spawn PTY process");

        let pid = pty.pid();
        assert!(pid.is_some());
        assert!(pid.unwrap() > 0);
    }

    #[tokio::test]
    async fn test_pty_kill() {
        let mut pty = PtyProcess::spawn(
            "sleep",
            &["10".to_string()],
            None,
            &HashMap::new(),
            None,
            None,
        )
        .expect("Failed to spawn PTY process");

        pty.kill().await.expect("Failed to kill process");

        // Wait for it - should be fast and not hang
        let _ = pty.wait().await.expect("Failed to wait for process");
    }
}
