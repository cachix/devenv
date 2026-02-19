use std::future::Future;
use std::io;
use std::os::unix::process::ExitStatusExt;
use std::pin::Pin;
use std::process::ExitStatus;
use std::sync::Mutex;

use process_wrap::tokio::ChildWrapper;
use tokio::sync::oneshot;

use crate::protocol::ProcessExit;

fn process_exit_to_status(exit: ProcessExit) -> ExitStatus {
    // Reconstruct the raw waitpid status word that ExitStatus::from_raw expects.
    // Exited: exit code in bits 8-15. Signaled: signal number in bits 0-6.
    let raw = match exit {
        ProcessExit::Exited(code) => code << 8,
        ProcessExit::Signaled(sig) => sig,
    };
    ExitStatus::from_raw(raw)
}

/// A child process launched via the cap-server.
///
/// Implements [`ChildWrapper`] so it can be used directly with watchexec-supervisor's
/// `set_spawn_fn` hook. Signal delivery uses direct `kill(2)` syscalls — valid because
/// the cap-server drops the launched process to the caller's uid/gid before exec.
/// Exit notification is delivered via a oneshot channel driven by the background
/// polling task in the process manager.
pub struct CapServerChild {
    pid: u32,
    /// Receives the exit info when the process exits.
    /// Wrapped in a Mutex so the struct is Sync (required by ChildWrapper).
    exit_rx: Mutex<Option<oneshot::Receiver<ProcessExit>>>,
    /// Cached exit status after the first successful `wait()`.
    cached_status: Option<ExitStatus>,
}

impl CapServerChild {
    /// Create a new `CapServerChild`.
    ///
    /// `exit_rx` must be the receiving half of a oneshot channel whose sending
    /// half is held by the polling task; the polling task sends the exit info
    /// when it detects that the process has exited.
    pub fn new(pid: u32, exit_rx: oneshot::Receiver<ProcessExit>) -> Self {
        Self {
            pid,
            exit_rx: Mutex::new(Some(exit_rx)),
            cached_status: None,
        }
    }
}

impl std::fmt::Debug for CapServerChild {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CapServerChild")
            .field("pid", &self.pid)
            .finish_non_exhaustive()
    }
}

impl ChildWrapper for CapServerChild {
    fn inner(&self) -> &dyn ChildWrapper {
        unreachable!("CapServerChild does not wrap another ChildWrapper")
    }

    fn inner_mut(&mut self) -> &mut dyn ChildWrapper {
        unreachable!("CapServerChild does not wrap another ChildWrapper")
    }

    fn into_inner(self: Box<Self>) -> Box<dyn ChildWrapper> {
        unreachable!("CapServerChild does not wrap another ChildWrapper")
    }

    fn id(&self) -> Option<u32> {
        Some(self.pid)
    }

    fn wait(&mut self) -> Pin<Box<dyn Future<Output = io::Result<ExitStatus>> + Send + '_>> {
        Box::pin(async move {
            if let Some(status) = self.cached_status {
                return Ok(status);
            }
            // Take the receiver out of the Mutex before awaiting it.
            let rx = self.exit_rx.lock().unwrap().take();
            if let Some(rx) = rx {
                let exit = rx.await.unwrap_or(ProcessExit::Signaled(libc::SIGKILL));
                let status = process_exit_to_status(exit);
                self.cached_status = Some(status);
                Ok(status)
            } else {
                Ok(self
                    .cached_status
                    .unwrap_or_else(|| ExitStatus::from_raw(0)))
            }
        })
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if let Some(status) = self.cached_status {
            return Ok(Some(status));
        }
        // Non-blocking check of the oneshot channel.
        let maybe_exit = self
            .exit_rx
            .lock()
            .unwrap()
            .as_mut()
            .and_then(|rx| rx.try_recv().ok());
        if let Some(exit) = maybe_exit {
            let status = process_exit_to_status(exit);
            self.cached_status = Some(status);
            *self.exit_rx.lock().unwrap() = None;
            Ok(Some(status))
        } else {
            Ok(None)
        }
    }

    /// Kill the process by sending SIGKILL directly.
    fn start_kill(&mut self) -> io::Result<()> {
        // SAFETY: libc::kill is always safe to call with a valid pid and signal number.
        let ret = unsafe { libc::kill(self.pid as libc::pid_t, libc::SIGKILL) };
        if ret == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Send a signal directly to the process.
    #[cfg(unix)]
    fn signal(&self, sig: i32) -> io::Result<()> {
        // SAFETY: libc::kill is always safe to call with a valid pid and signal number.
        let ret = unsafe { libc::kill(self.pid as libc::pid_t, sig) };
        if ret == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}
