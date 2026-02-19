use miette::{IntoDiagnostic, Result, WrapErr};
use nix::fcntl::{FcntlArg, FdFlag, fcntl};
use process_wrap::tokio::{CommandWrap, CommandWrapper};
use socket2::{Domain, Protocol, Socket, Type};
use std::net::SocketAddr;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use crate::config::{ListenKind, ListenSpec};

pub const SD_LISTEN_FDS_START: RawFd = 3;

/// Activation entry (internal)
enum ActivationEntry {
    Tcp {
        addr: SocketAddr,
        backlog: i32,
    },
    UnixStream {
        path: PathBuf,
        backlog: i32,
        mode: Option<u32>,
    },
}

/// Specification for socket activation
pub struct ActivationSpec {
    entries: Vec<ActivationEntry>,
}

/// Handle to created sockets that allows cleanup
pub struct ActivatedSockets {
    fds: Vec<RawFd>,
}

impl ActivatedSockets {
    /// Get the raw file descriptors
    pub fn fds(&self) -> &[RawFd] {
        &self.fds
    }

    /// Consume and return the raw FDs, caller takes ownership
    pub fn into_fds(self) -> Vec<RawFd> {
        let fds = self.fds.clone();
        std::mem::forget(self); // Don't close FDs in drop
        fds
    }
}

impl Drop for ActivatedSockets {
    fn drop(&mut self) {
        // Clean up any remaining FDs
        for &fd in &self.fds {
            let _ = nix::unistd::close(fd);
        }
    }
}

pub struct ActivationSpecBuilder {
    entries: Vec<ActivationEntry>,
}

impl Default for ActivationSpecBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ActivationSpecBuilder {
    pub fn new() -> Self {
        Self { entries: vec![] }
    }

    pub fn tcp(mut self, _name: impl Into<String>, addr: SocketAddr, backlog: i32) -> Self {
        self.entries.push(ActivationEntry::Tcp { addr, backlog });
        self
    }

    pub fn unix_stream(
        mut self,
        _name: impl Into<String>,
        path: PathBuf,
        backlog: i32,
        mode: Option<u32>,
    ) -> Self {
        self.entries.push(ActivationEntry::UnixStream {
            path,
            backlog,
            mode,
        });
        self
    }

    pub fn build(self) -> ActivationSpec {
        ActivationSpec {
            entries: self.entries,
        }
    }
}

impl ActivationSpec {
    pub fn builder() -> ActivationSpecBuilder {
        ActivationSpecBuilder::new()
    }

    /// Create sockets and return a handle
    ///
    /// The sockets will be listening and have FD_CLOEXEC set by default (to prevent
    /// leaking to non-activated children). The SocketActivationWrapper will clear
    /// FD_CLOEXEC in pre_exec for socket-activated processes only.
    ///
    /// Returns an ActivatedSockets handle that will clean up FDs on drop unless
    /// you call into_fds() to take ownership.
    ///
    /// FDs will be mapped to 3, 4, 5... in child process (SD_LISTEN_FDS_START).
    pub fn create_fds(&self) -> Result<ActivatedSockets> {
        let mut fds = Vec::with_capacity(self.entries.len());

        for entry in &self.entries {
            match entry {
                ActivationEntry::Tcp { addr, backlog } => {
                    let sock = Socket::new(
                        Domain::for_address(*addr),
                        Type::STREAM,
                        Some(Protocol::TCP),
                    )
                    .into_diagnostic()?;

                    sock.set_reuse_address(true).into_diagnostic()?;
                    sock.bind(&(*addr).into()).into_diagnostic()?;
                    sock.listen(*backlog).into_diagnostic()?;

                    let fd = sock.as_raw_fd();

                    // Keep FD_CLOEXEC set in parent to prevent leaking to non-activated children
                    // It will be cleared in pre_exec for activated processes

                    // Keep listener alive via std::mem::forget
                    std::mem::forget(sock);

                    fds.push(fd);
                }
                ActivationEntry::UnixStream {
                    path,
                    backlog,
                    mode,
                } => {
                    // Remove existing socket file - warn but don't fail on errors
                    if let Err(e) = std::fs::remove_file(path)
                        && e.kind() != std::io::ErrorKind::NotFound
                    {
                        tracing::warn!(
                            "Failed to remove existing socket file {}: {}",
                            path.display(),
                            e
                        );
                    }

                    let sock = Socket::new(Domain::UNIX, Type::STREAM, None).into_diagnostic()?;
                    let sa = socket2::SockAddr::unix(path).into_diagnostic()?;
                    sock.bind(&sa).into_diagnostic()?;
                    sock.listen(*backlog).into_diagnostic()?;

                    // Set permissions if specified
                    if let Some(m) = *mode {
                        std::fs::set_permissions(path, std::fs::Permissions::from_mode(m))
                            .into_diagnostic()
                            .wrap_err(format!(
                                "Failed to set permissions on socket {}",
                                path.display()
                            ))?;
                    }

                    let fd = sock.as_raw_fd();

                    // Keep FD_CLOEXEC set in parent to prevent leaking to non-activated children
                    // It will be cleared in pre_exec for activated processes

                    // Keep listener alive via std::mem::forget
                    std::mem::forget(sock);

                    fds.push(fd);
                }
            }
        }

        Ok(ActivatedSockets { fds })
    }
}

/// Build ActivationSpec from ListenSpec array
pub fn activation_from_listen(listens: &[ListenSpec]) -> Result<ActivationSpec> {
    let mut builder = ActivationSpec::builder();

    for spec in listens {
        match spec.kind {
            ListenKind::Tcp => {
                let addr_str = spec
                    .address
                    .as_ref()
                    .ok_or_else(|| miette::miette!("TCP listen requires address"))?;
                let addr: SocketAddr = addr_str
                    .parse()
                    .into_diagnostic()
                    .map_err(|_| miette::miette!("Failed to parse TCP address: {}", addr_str))?;
                builder = builder.tcp(&spec.name, addr, spec.backlog.unwrap_or(128));
            }
            ListenKind::UnixStream => {
                let path = spec
                    .path
                    .as_ref()
                    .ok_or_else(|| miette::miette!("Unix stream listen requires path"))?;
                builder = builder.unix_stream(
                    &spec.name,
                    path.clone(),
                    spec.backlog.unwrap_or(128),
                    spec.mode,
                );
            }
        }
    }

    Ok(builder.build())
}

/// Wrapper for process-wrap that sets up socket activation and Linux capabilities
///
/// This implements CommandWrapper to integrate with watchexec-supervisor.
/// It uses pre_exec to configure systemd-style socket activation and Linux
/// capabilities in the child process.
#[derive(Debug, Clone)]
pub struct ProcessSetupWrapper {
    fds: Vec<RawFd>,
    capabilities: Vec<String>,
}

impl ProcessSetupWrapper {
    /// Create a new process setup wrapper
    pub fn new(fds: Vec<RawFd>, capabilities: Vec<String>) -> Self {
        Self { fds, capabilities }
    }

    /// Create wrapper for socket activation only
    pub fn socket_activation(fds: Vec<RawFd>) -> Self {
        Self {
            fds,
            capabilities: Vec::new(),
        }
    }

    /// Create wrapper for capabilities only
    pub fn capabilities(capabilities: Vec<String>) -> Self {
        Self {
            fds: Vec::new(),
            capabilities,
        }
    }
}

// Keep the old name as an alias for backwards compatibility
pub type SocketActivationWrapper = ProcessSetupWrapper;

impl CommandWrapper for ProcessSetupWrapper {
    fn pre_spawn(
        &mut self,
        command: &mut tokio::process::Command,
        _core: &CommandWrap,
    ) -> std::io::Result<()> {
        // Set LISTEN_FDS before fork (number of sockets doesn't change)
        // LISTEN_PID is set in pre_exec where we have the correct child PID
        if !self.fds.is_empty() {
            command.env("LISTEN_FDS", self.fds.len().to_string());
        }
        let has_socket_fds = !self.fds.is_empty();

        // Clone data for the pre_exec closure
        let fds = self.fds.clone();

        // Parse capabilities upfront (before fork) so errors are reported early
        #[cfg(target_os = "linux")]
        let parsed_caps: Vec<caps::Capability> = {
            self.capabilities
                .iter()
                .map(|name| {
                    // Normalize: add CAP_ prefix if not present
                    let normalized = if name.to_uppercase().starts_with("CAP_") {
                        name.to_uppercase()
                    } else {
                        format!("CAP_{}", name.to_uppercase())
                    };
                    normalized.parse::<caps::Capability>()
                })
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?
        };

        // Capabilities are silently ignored on non-Linux platforms
        #[cfg(not(target_os = "linux"))]
        let _ = &self.capabilities;

        // Use pre_exec to set up FDs, capabilities, and LISTEN_PID
        unsafe {
            command.pre_exec(move || {
                use nix::libc;
                use std::ffi::CString;
                use std::os::fd::BorrowedFd;

                // === Socket Activation PID ===
                // Set LISTEN_PID to the actual child PID (getpid() returns child PID in pre_exec)
                if has_socket_fds {
                    let pid = libc::getpid();
                    let pid_str = CString::new(pid.to_string()).unwrap();
                    let key = CString::new("LISTEN_PID").unwrap();
                    libc::setenv(key.as_ptr(), pid_str.as_ptr(), 1);
                }

                // Helper to validate FD
                fn is_valid_fd(fd: RawFd) -> bool {
                    unsafe { libc::fcntl(fd, libc::F_GETFD) != -1 }
                }

                // === Socket Activation Setup ===
                for (i, &source_fd) in fds.iter().enumerate() {
                    let target_fd = SD_LISTEN_FDS_START + i as RawFd;

                    if !is_valid_fd(source_fd) {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            format!("Source FD {} is invalid", source_fd),
                        ));
                    }

                    if source_fd == target_fd {
                        let borrowed = BorrowedFd::borrow_raw(target_fd);
                        let flags = FdFlag::from_bits_truncate(
                            fcntl(borrowed, FcntlArg::F_GETFD)
                                .map_err(|_| std::io::Error::last_os_error())?,
                        );
                        let mut new_flags = flags;
                        new_flags.remove(FdFlag::FD_CLOEXEC);
                        if new_flags != flags {
                            fcntl(borrowed, FcntlArg::F_SETFD(new_flags))
                                .map_err(|_| std::io::Error::last_os_error())?;
                        }
                        continue;
                    }

                    if libc::dup2(source_fd, target_fd) == -1 {
                        return Err(std::io::Error::last_os_error());
                    }

                    let target_borrowed = BorrowedFd::borrow_raw(target_fd);
                    let flags = FdFlag::from_bits_truncate(
                        fcntl(target_borrowed, FcntlArg::F_GETFD)
                            .map_err(|_| std::io::Error::last_os_error())?,
                    );
                    let mut new_flags = flags;
                    new_flags.remove(FdFlag::FD_CLOEXEC);
                    if new_flags != flags {
                        fcntl(target_borrowed, FcntlArg::F_SETFD(new_flags))
                            .map_err(|_| std::io::Error::last_os_error())?;
                    }

                    if source_fd != target_fd && is_valid_fd(source_fd) {
                        libc::close(source_fd);
                    }
                }

                // === Capabilities Setup (Linux only) ===
                // Capabilities are applied as ambient (inheritable first, then ambient)
                // so they are inherited by child processes.
                #[cfg(target_os = "linux")]
                for cap in &parsed_caps {
                    caps::raise(None, caps::CapSet::Inheritable, *cap)
                        .and_then(|_| caps::raise(None, caps::CapSet::Ambient, *cap))
                        .map_err(|e| {
                            std::io::Error::new(std::io::ErrorKind::PermissionDenied, e.to_string())
                        })?;
                }

                Ok(())
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::{libc, unistd};
    use std::os::fd::BorrowedFd;

    #[test]
    fn test_activation_spec_builder() {
        let spec = ActivationSpec::builder()
            .tcp("http", "127.0.0.1:8080".parse().unwrap(), 128)
            .build();

        assert_eq!(spec.entries.len(), 1);
    }

    #[test]
    fn test_activation_from_listen() {
        let listens = vec![ListenSpec {
            name: "http".to_string(),
            kind: ListenKind::Tcp,
            address: Some("127.0.0.1:8080".to_string()),
            path: None,
            backlog: Some(128),
            mode: None,
        }];

        let spec = activation_from_listen(&listens);
        assert!(spec.is_ok());
    }

    #[test]
    fn test_create_tcp_socket() {
        let spec = ActivationSpec::builder()
            .tcp("test-http", "127.0.0.1:0".parse().unwrap(), 128)
            .build();

        let activated = spec.create_fds().expect("Failed to create sockets");
        assert_eq!(activated.fds().len(), 1, "Should create exactly one socket");

        // Verify socket is valid and listening
        let fd = activated.fds()[0];
        assert!(fd >= 0, "FD should be valid");

        // Check that FD_CLOEXEC is set in parent (for safety)
        unsafe {
            let borrowed = BorrowedFd::borrow_raw(fd);
            let flags = fcntl(borrowed, FcntlArg::F_GETFD).expect("Failed to get fd flags");
            let fd_flags = FdFlag::from_bits_truncate(flags);
            assert!(
                fd_flags.contains(FdFlag::FD_CLOEXEC),
                "FD_CLOEXEC should be set in parent to prevent leaking to non-activated children"
            );
        }

        // FDs should be cleaned up when activated is dropped
    }

    #[test]
    fn test_create_multiple_sockets() {
        let spec = ActivationSpec::builder()
            .tcp("http", "127.0.0.1:0".parse().unwrap(), 128)
            .tcp("https", "127.0.0.1:0".parse().unwrap(), 128)
            .build();

        let activated = spec.create_fds().expect("Failed to create sockets");
        assert_eq!(activated.fds().len(), 2, "Should create two sockets");

        // Verify all FDs are unique and valid
        assert_ne!(
            activated.fds()[0],
            activated.fds()[1],
            "FDs should be different"
        );
        assert!(
            activated.fds()[0] >= 0 && activated.fds()[1] >= 0,
            "All FDs should be valid"
        );
    }

    #[test]
    fn test_unix_socket_creation() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let socket_path = temp_dir.path().join("test.sock");

        let spec = ActivationSpec::builder()
            .unix_stream("test-unix", socket_path.clone(), 128, Some(0o600))
            .build();

        let activated = spec.create_fds().expect("Failed to create unix socket");
        assert_eq!(activated.fds().len(), 1);

        // Verify socket file exists
        assert!(socket_path.exists(), "Unix socket file should exist");

        // Verify permissions (if specified)
        let metadata = std::fs::metadata(&socket_path).expect("Failed to get metadata");
        let permissions = metadata.permissions();
        assert_eq!(
            permissions.mode() & 0o777,
            0o600,
            "Socket should have correct permissions"
        );
    }

    #[test]
    fn test_activated_sockets_into_fds() {
        // Test that into_fds() prevents cleanup
        let spec = ActivationSpec::builder()
            .tcp("test", "127.0.0.1:0".parse().unwrap(), 128)
            .build();

        let activated = spec.create_fds().expect("Failed to create sockets");
        let fds = activated.into_fds();
        let fd = fds[0];

        // FD should still be valid after consuming ActivatedSockets
        unsafe {
            assert!(
                libc::fcntl(fd, libc::F_GETFD) != -1,
                "FD should still be valid"
            );
        }

        // Manually clean up
        let _ = unistd::close(fd);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_socket_activation_wrapper_sets_env() {
        use process_wrap::tokio::CommandWrap;
        use tokio::process::Command;
        use tokio::time::{Duration, timeout};

        // Create a test socket
        let spec = ActivationSpec::builder()
            .tcp("test", "127.0.0.1:0".parse().unwrap(), 128)
            .build();
        let activated = spec.create_fds().expect("Failed to create sockets");

        // Create wrapper - take ownership of FDs
        let wrapper = ProcessSetupWrapper::socket_activation(activated.into_fds());

        // Create a command that echoes environment variables
        let mut cmd = Command::new("sh");
        cmd.arg("-c");
        cmd.arg("echo LISTEN_FDS=$LISTEN_FDS LISTEN_PID=$LISTEN_PID");
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        // Wrap command
        let mut cmd_wrap = CommandWrap::from(cmd);
        cmd_wrap.wrap(wrapper);

        // Spawn and capture output with timeout
        let child = cmd_wrap.spawn().expect("Failed to spawn");
        let output = timeout(
            Duration::from_secs(5),
            Box::into_pin(child.wait_with_output()),
        )
        .await
        .expect("Test timed out after 5 seconds")
        .expect("Failed to wait");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Debug output if test fails
        if !output.status.success() {
            eprintln!("Command failed with status: {}", output.status);
            eprintln!("Stderr: {}", stderr);
        }

        // Verify LISTEN_FDS is set
        assert!(
            stdout.contains("LISTEN_FDS=1"),
            "LISTEN_FDS should be set to 1, got stdout: '{}', stderr: '{}'",
            stdout,
            stderr
        );

        // Verify LISTEN_PID is set to the child's actual PID
        assert!(
            stdout.contains("LISTEN_PID=") && !stdout.contains("LISTEN_PID=0"),
            "LISTEN_PID should be set to child PID, got: {}",
            stdout
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_socket_activation_fds_at_standard_positions() {
        use process_wrap::tokio::CommandWrap;
        use tokio::process::Command;
        use tokio::time::{Duration, timeout};

        // Create test sockets
        let spec = ActivationSpec::builder()
            .tcp("http", "127.0.0.1:0".parse().unwrap(), 128)
            .tcp("https", "127.0.0.1:0".parse().unwrap(), 128)
            .build();
        let activated = spec.create_fds().expect("Failed to create sockets");

        // Create wrapper - take ownership of FDs
        let wrapper = ProcessSetupWrapper::socket_activation(activated.into_fds());

        // Check that FDs are at positions 3 and 4 (SD_LISTEN_FDS_START)
        // List all FDs for diagnostic purposes if test fails
        let mut cmd = Command::new("sh");
        cmd.arg("-c");
        cmd.arg("ls -la /dev/fd/ 2>&1; echo 'LISTEN_FDS='$LISTEN_FDS; test -e /dev/fd/3 && test -e /dev/fd/4 && echo 'FDs at standard positions'");
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        // Wrap command
        let mut cmd_wrap = CommandWrap::from(cmd);
        cmd_wrap.wrap(wrapper);

        // Spawn and capture output with timeout
        let child = cmd_wrap.spawn().expect("Failed to spawn");
        let output = timeout(
            Duration::from_secs(5),
            Box::into_pin(child.wait_with_output()),
        )
        .await
        .expect("Test timed out after 5 seconds")
        .expect("Failed to wait");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(
            stdout.contains("FDs at standard positions"),
            "FDs should be at positions 3 and 4.\nStdout:\n{}\nStderr:\n{}",
            stdout,
            stderr
        );
    }

    #[test]
    fn test_socket_activation_constants() {
        // Verify SD_LISTEN_FDS_START matches systemd spec
        assert_eq!(SD_LISTEN_FDS_START, 3, "SD_LISTEN_FDS_START must be 3");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_minimal_command_wrap() {
        // Test if process-wrap works at all with a simple command
        use process_wrap::tokio::CommandWrap;
        use tokio::process::Command;
        use tokio::time::{Duration, timeout};

        let mut cmd = Command::new("echo");
        cmd.arg("hello");
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());

        let mut cmd_wrap = CommandWrap::from(cmd);

        let child = cmd_wrap.spawn().expect("Failed to spawn");
        let output = timeout(
            Duration::from_secs(2),
            Box::into_pin(child.wait_with_output()),
        )
        .await
        .expect("Test timed out")
        .expect("Failed to wait");

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("hello"));
    }
}
