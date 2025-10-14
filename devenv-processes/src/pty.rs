use miette::{IntoDiagnostic, Result, miette};
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use tracing::{debug, error};

/// PTY process wrapper
pub struct PtyProcess {
    child: Box<dyn Child + Send>,
    reader_thread: Option<thread::JoinHandle<()>>,
    running: Arc<Mutex<bool>>,
}

impl PtyProcess {
    /// Spawn a new process with a pseudo-terminal
    pub fn spawn(
        exec: &str,
        args: &[String],
        cwd: Option<&PathBuf>,
        env: &HashMap<String, String>,
    ) -> Result<Self> {
        debug!("Spawning PTY process: {} {:?}", exec, args);

        // Get the native PTY system
        let pty_system = native_pty_system();

        // Create a new PTY
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
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
        debug!("PTY process spawned successfully");

        // Set up a reader thread to forward PTY output to stdout
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| miette!("Failed to clone PTY reader: {}", e))?;
        let running = Arc::new(Mutex::new(true));
        let running_clone = Arc::clone(&running);

        let reader_thread = thread::spawn(move || {
            let mut buffer = [0u8; 8192];
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
                        // Forward to stdout
                        if let Err(e) = std::io::stdout().write_all(&buffer[..n]) {
                            error!("PTY reader: Failed to write to stdout: {}", e);
                            break;
                        }
                        let _ = std::io::stdout().flush();
                    }
                    Err(e) => {
                        error!("PTY reader: Read error: {}", e);
                        break;
                    }
                }
            }
            debug!("PTY reader thread exiting");
        });

        Ok(Self {
            child,
            reader_thread: Some(reader_thread),
            running,
        })
    }

    /// Get the process ID
    pub fn pid(&self) -> Option<u32> {
        self.child.process_id()
    }

    /// Wait for the process to exit
    pub fn wait(&mut self) -> Result<i32> {
        // Wait for child to exit
        let exit_status = self.child.wait().into_diagnostic()?;

        // Signal the reader thread to stop
        {
            let mut running = self.running.lock().unwrap();
            *running = false;
        }

        // Wait for reader thread to finish
        if let Some(handle) = self.reader_thread.take() {
            let _ = handle.join();
        }

        let code = exit_status.exit_code();
        debug!("PTY process exited with code {}", code);
        Ok(code as i32)
    }

    /// Kill the process
    pub fn kill(&mut self) -> Result<()> {
        self.child.kill().into_diagnostic()?;

        // Signal the reader thread to stop
        {
            let mut running = self.running.lock().unwrap();
            *running = false;
        }

        // Wait for reader thread to finish
        if let Some(handle) = self.reader_thread.take() {
            let _ = handle.join();
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pty_spawn_echo() {
        let mut pty = PtyProcess::spawn("echo", &["hello".to_string()], None, &HashMap::new())
            .expect("Failed to spawn PTY process");

        let exit_code = pty.wait().expect("Failed to wait for process");
        assert_eq!(exit_code, 0);
    }

    #[test]
    fn test_pty_get_pid() {
        let pty = PtyProcess::spawn("sleep", &["0.1".to_string()], None, &HashMap::new())
            .expect("Failed to spawn PTY process");

        let pid = pty.pid();
        assert!(pid.is_some());
        assert!(pid.unwrap() > 0);
    }
}
