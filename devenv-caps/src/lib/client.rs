use crate::protocol::{self, Request, Response};
use miette::{bail, Result};
use rustix::io::{fcntl_getfd, fcntl_setfd, FdFlags};
use rustix::process::{getgid, getgroups, getuid};
use std::collections::HashMap;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ClientError {
    #[error("failed to create socketpair: {0}")]
    SocketPair(std::io::Error),
    #[error("failed to spawn cap-server via sudo: {0}")]
    Spawn(std::io::Error),
    #[error("I/O error communicating with cap-server: {0}")]
    Io(#[from] std::io::Error),
    #[error("cap-server returned error: {0}")]
    Server(String),
    #[error("unexpected response from cap-server")]
    Protocol,
}

/// Handle to a running cap-server process.
///
/// The cap-server runs as root (via sudo) and communicates over a socketpair
/// inherited from this process. No filesystem socket is created — the fd is
/// the only way to reach the server.
pub struct CapServer {
    stream: UnixStream,
    child: Child,
}

/// Configuration for the cap-server.
pub struct CapServerConfig {
    /// Path to the devenv-cap-server binary.
    pub server_binary: PathBuf,
    /// UID to drop to when launching processes.
    pub uid: u32,
    /// GID to drop to when launching processes.
    pub gid: u32,
    /// Supplementary groups for launched processes.
    pub groups: Vec<u32>,
}

impl CapServerConfig {
    /// Build a config for the current user.
    pub fn current_user(server_binary: PathBuf) -> Self {
        let uid = getuid().as_raw();
        let gid = getgid().as_raw();
        let groups: Vec<u32> = getgroups()
            .unwrap_or_default()
            .into_iter()
            .map(|g| g.as_raw())
            .collect();

        Self {
            server_binary,
            uid,
            gid,
            groups,
        }
    }
}

impl CapServer {
    /// Spawn the cap-server via `sudo`.
    ///
    /// Creates a socketpair, passes one end to the child via `--fd`, and
    /// returns a `CapServer` holding the other end. No filesystem socket
    /// is ever created.
    pub fn start(config: &CapServerConfig) -> Result<Self, ClientError> {
        let (parent_sock, child_sock) = UnixStream::pair().map_err(ClientError::SocketPair)?;

        let child_fd = child_sock.as_raw_fd();

        // Clear FD_CLOEXEC so the cap-server inherits this fd across exec.
        let mut flags = fcntl_getfd(&child_sock).unwrap_or(FdFlags::empty());
        flags.remove(FdFlags::CLOEXEC);
        let _ = fcntl_setfd(&child_sock, flags);

        let groups_str: Vec<String> = config.groups.iter().map(|g| g.to_string()).collect();

        // Use --preserve-fd so that modern sudo (>= 1.9.14) does not close
        // the inherited socketpair fd. Without this, sudo's default closefrom
        // behavior drops all fds >= 3.
        let child_fd_str = child_fd.to_string();
        let child = Command::new("sudo")
            .args(["--preserve-fd", &child_fd_str])
            .arg("--")
            .arg(&config.server_binary)
            .args(["--fd", &child_fd_str])
            .args(["--uid", &config.uid.to_string()])
            .args(["--gid", &config.gid.to_string()])
            .args(["--groups", &groups_str.join(",")])
            .spawn()
            .map_err(ClientError::Spawn)?;

        // Close our copy of the child's end. The child inherited it via fork.
        drop(child_sock);

        // Guard against the server crashing between receiving a request and
        // sending its response, which would otherwise block the client forever.
        parent_sock
            .set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(ClientError::Io)?;

        Ok(Self {
            stream: parent_sock,
            child,
        })
    }

    /// Launch a process with capabilities through the cap-server.
    ///
    /// The cap-server forks, sets the requested ambient capabilities,
    /// drops to the target user, and execs the command.
    ///
    /// Returns the PID of the launched child.
    pub fn launch(
        &mut self,
        id: &str,
        caps: &[String],
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
        working_dir: &Path,
    ) -> Result<u32, ClientError> {
        let request = Request::Launch {
            id: id.to_string(),
            caps: caps.to_vec(),
            command: command.to_string(),
            args: args.to_vec(),
            env: env.clone(),
            working_dir: working_dir.to_path_buf(),
        };

        protocol::write_message(&mut self.stream, &request)?;
        let response: Response = protocol::read_message(&mut self.stream)?;

        match response {
            Response::Launched { pid } => Ok(pid),
            Response::Error { message } => Err(ClientError::Server(message)),
            _ => Err(ClientError::Protocol),
        }
    }

    /// Poll for child processes that have exited since the last call.
    ///
    /// Returns a list of exited processes with their raw Unix waitpid status.
    /// Callers should poll periodically to deliver exit notifications.
    pub fn poll(&mut self) -> Result<Vec<crate::protocol::ExitedProcess>, ClientError> {
        protocol::write_message(&mut self.stream, &Request::Poll)?;
        let response: Response = protocol::read_message(&mut self.stream)?;
        match response {
            Response::Exited { processes } => Ok(processes),
            Response::Error { message } => Err(ClientError::Server(message)),
            _ => Err(ClientError::Protocol),
        }
    }

    /// Send a signal to a process launched via the cap-server.
    pub fn signal(&mut self, pid: u32, signal: i32) -> Result<(), ClientError> {
        let request = Request::Signal { pid, signal };
        protocol::write_message(&mut self.stream, &request)?;
        let response: Response = protocol::read_message(&mut self.stream)?;

        match response {
            Response::Ok => Ok(()),
            Response::Error { message } => Err(ClientError::Server(message)),
            _ => Err(ClientError::Protocol),
        }
    }

    /// Shut down the cap-server. Sends SIGTERM to all children it launched,
    /// then the server process exits.
    pub fn shutdown(mut self) -> Result<(), ClientError> {
        protocol::write_message(&mut self.stream, &Request::Shutdown)?;
        // Best-effort read; server may close the connection immediately.
        let _ = protocol::read_message::<Response>(&mut self.stream);
        let _ = self.child.wait();
        Ok(())
    }

    /// Get the PID of the cap-server process itself.
    pub fn server_pid(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for CapServer {
    fn drop(&mut self) {
        // Best-effort shutdown if the user forgot to call shutdown().
        let _ = protocol::write_message(&mut self.stream, &Request::Shutdown);
        let _ = self.child.wait();
    }
}

/// Find the `devenv-cap-server` binary.
///
/// Looks first next to the running executable (installed layout where all
/// devenv binaries share a `bin/` directory), then falls back to `$PATH`.
/// Returns `None` if the binary cannot be located.
pub fn find_cap_server_binary() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("devenv-cap-server");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    which::which("devenv-cap-server").ok()
}

/// Check whether `sudo` can run the given binary without a password prompt.
///
/// Returns `true` when NOPASSWD is configured or a cached sudo session
/// exists. This is safe to call while the TUI is active (no terminal I/O).
pub fn can_sudo_noninteractive(binary: &Path) -> bool {
    Command::new("sudo")
        .args(["-n", "--"])
        .arg(binary)
        .arg("--check")
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Ensure `sudo` can run `devenv-cap-server` without interaction.
///
/// Tries a non-interactive check first (`sudo -n --check`).  If that fails
/// and a TTY is available, prompts via `sudo -v` to refresh the cached
/// session.  If neither works, returns an actionable error.
///
/// Call this once before starting the cap-server, ideally before the TUI
/// takes over the terminal so the password prompt is clean.
pub fn preflight_sudo_auth(binary: &Path) -> Result<()> {
    use std::io::IsTerminal;

    if can_sudo_noninteractive(binary) {
        return Ok(());
    }

    // Need a password — only prompt when we have a TTY.
    if std::io::stderr().is_terminal() {
        let ok = Command::new("sudo")
            .arg("-v") // refresh cached credentials
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if ok {
            return Ok(());
        }
    }

    let username = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "your_user".to_string());

    bail!(
        "sudo authentication failed for devenv-cap-server.\n\
         \n\
         Processes that require Linux capabilities (e.g. net_bind_service) need\n\
         to be launched by the cap-server, which runs as root via sudo.\n\
         \n\
         To allow passwordless access, add to /etc/sudoers.d/devenv:\n\
         \n\
         {username}  ALL=(root) NOPASSWD: {binary}\n\
         \n\
         Or run `sudo -v` before starting devenv to cache your credentials.",
        binary = binary.display(),
    )
}
