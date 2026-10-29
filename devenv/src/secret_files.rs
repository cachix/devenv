//! Anonymous SecretSpec files whose lifetime is bounded by an explicit process.

use miette::{IntoDiagnostic, Result};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(crate) struct SecretFiles {
    owner: Option<u32>,
    keeper: Arc<Mutex<Option<Arc<platform::Keeper>>>>,
}

impl SecretFiles {
    pub(crate) fn new(owner: Option<u32>) -> Self {
        Self {
            owner,
            keeper: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn configure(&self, secrets: &mut secretspec::Secrets) {
        let files = self.clone();
        secrets.set_as_path_materializer(move |bytes, suffix| {
            if suffix.is_some_and(|suffix| !suffix.is_empty()) {
                return Err(std::io::Error::other(
                    "SecretSpec file suffixes are unsupported for anonymous files"));
            }
            let owner = files.owner.ok_or_else(|| std::io::Error::other(
                "SecretSpec as_path exports require --secret-owner-pid or DEVENV_SECRETS_OWNER_PID; configure a shell-lifetime lease"))?;
            let mut keeper = files.keeper.lock().map_err(|_| std::io::Error::other("Secret keeper lock poisoned"))?;
            if keeper.is_none() {
                *keeper = Some(Arc::new(platform::Keeper::spawn(owner)?));
            }
            keeper.as_ref().unwrap().materialize(bytes)
        });
    }

    /// Transfer the lease to the declared process before exporting paths or exec.
    pub(crate) fn handoff(&self) -> Result<()> {
        self.handoff_to(None)
    }

    pub(crate) fn retain_for_current_process(&self) -> Result<()> {
        if self.owner == Some(std::process::id()) {
            self.handoff()?;
        }
        Ok(())
    }

    pub(crate) fn handoff_to(&self, owner: Option<u32>) -> Result<()> {
        if let Some(keeper) = self
            .keeper
            .lock()
            .map_err(|_| miette::miette!("Secret keeper lock poisoned"))?
            .as_ref()
        {
            keeper.handoff_to(owner).into_diagnostic()?;
        }
        Ok(())
    }
}

/// Handle the private keeper protocol before initializing any application runtime.
#[doc(hidden)]
pub fn maybe_run_keeper() -> Option<i32> {
    platform::maybe_run_keeper()
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use nix::libc;
    use std::fs::File;
    use std::io::{self, Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::net::UnixStream;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicBool, Ordering};

    const SOCKET_ENV: &str = "_DEVENV_SECRET_KEEPER_SOCKET";
    const OWNER_ENV: &str = "_DEVENV_SECRET_KEEPER_OWNER";
    const ALLOCATE: u8 = 1;
    const HANDOFF: u8 = 2;
    const MAX_SECRET_BYTES: u64 = 64 * 1024 * 1024;

    pub(crate) struct Keeper {
        stream: Mutex<UnixStream>,
        child: Mutex<Option<Child>>,
        handed_off: AtomicBool,
    }

    fn syscall_fd(result: libc::c_long) -> io::Result<OwnedFd> {
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        // The syscall returned a new descriptor owned by this caller.
        Ok(unsafe { OwnedFd::from_raw_fd(result as i32) })
    }

    impl Keeper {
        pub(crate) fn spawn(owner: u32) -> io::Result<Self> {
            if owner == 0 || owner > i32::MAX as u32 {
                return Err(io::Error::other("Invalid secret owner PID"));
            }
            let owner = syscall_fd(unsafe { libc::syscall(libc::SYS_pidfd_open, owner, 0) })?;
            let (mut stream, remote) = UnixStream::pair()?;
            let socket_fd = remote.as_raw_fd();
            let owner_fd = owner.as_raw_fd();
            let mut command = Command::new(std::env::current_exe()?);
            command
                .env(SOCKET_ENV, socket_fd.to_string())
                .env(OWNER_ENV, owner_fd.to_string())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0);
            // Clear CLOEXEC only in the child, avoiding descriptor leaks into
            // other commands spawned by concurrent threads in the parent.
            unsafe {
                command.pre_exec(move || {
                    for fd in [socket_fd, owner_fd] {
                        if libc::fcntl(fd, libc::F_SETFD, 0) < 0 {
                            return Err(io::Error::last_os_error());
                        }
                    }
                    Ok(())
                });
            }
            #[cfg(test)]
            command
                .args([
                    "--exact",
                    "secret_files::tests::keeper_entry",
                    "--nocapture",
                ])
                .stderr(Stdio::inherit());
            let mut child = command.spawn()?;
            drop(remote);
            drop(owner);
            let mut ready = [0];
            if let Err(error) = stream.read_exact(&mut ready) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
            if ready != [1] {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::other("Secret keeper failed to initialize"));
            }
            Ok(Self {
                stream: Mutex::new(stream),
                child: Mutex::new(Some(child)),
                handed_off: AtomicBool::new(false),
            })
        }

        pub(crate) fn materialize(&self, bytes: &[u8]) -> io::Result<PathBuf> {
            if self.handed_off.load(Ordering::Acquire) {
                return Err(io::Error::other(
                    "Secret files have already been handed to their owner",
                ));
            }
            if bytes.len() as u64 > MAX_SECRET_BYTES {
                return Err(io::Error::other("Secret exceeds anonymous-file size limit"));
            }
            let mut stream = self
                .stream
                .lock()
                .map_err(|_| io::Error::other("Secret keeper lock poisoned"))?;
            stream.write_all(&[ALLOCATE])?;
            stream.write_all(&(bytes.len() as u64).to_le_bytes())?;
            stream.write_all(bytes)?;
            let mut reply = [0; 4];
            stream.read_exact(&mut reply)?;
            let fd = i32::from_le_bytes(reply);
            if fd < 0 {
                return Err(io::Error::other("Failed to allocate anonymous secret file"));
            }
            let pid = self
                .child
                .lock()
                .map_err(|_| io::Error::other("Secret keeper lock poisoned"))?
                .as_ref()
                .ok_or_else(|| io::Error::other("Secret keeper is closed"))?
                .id();
            Ok(PathBuf::from(format!("/proc/{pid}/fd/{fd}")))
        }

        #[cfg(test)]
        pub(crate) fn handoff(&self) -> io::Result<()> {
            self.handoff_to(None)
        }

        pub(crate) fn handoff_to(&self, owner: Option<u32>) -> io::Result<()> {
            if owner.is_some_and(|pid| pid == 0 || pid > i32::MAX as u32) {
                return Err(io::Error::other("Invalid secret owner PID"));
            }
            let mut stream = self
                .stream
                .lock()
                .map_err(|_| io::Error::other("Secret keeper lock poisoned"))?;
            if !self.handed_off.load(Ordering::Acquire) || owner.is_some() {
                stream.write_all(&[HANDOFF])?;
                stream.write_all(&owner.unwrap_or(0).to_le_bytes())?;
                let mut ack = [0];
                stream.read_exact(&mut ack)?;
                if ack != [1] {
                    return Err(io::Error::other("Secret keeper handoff failed"));
                }
                self.handed_off.store(true, Ordering::Release);
            }
            Ok(())
        }
    }

    impl Drop for Keeper {
        fn drop(&mut self) {
            if let Ok(child) = self.child.get_mut()
                && let Some(mut child) = child.take()
            {
                if self.handed_off.load(Ordering::Acquire) {
                    reap_when_exited(child);
                } else {
                    // Failed evaluation or an owned session ending: promptly
                    // release anonymous files and reap the helper.
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    }

    // Reloads can retire many keepers during one interactive session. Reap
    // them with one worker instead of keeping one waiting thread per reload.
    fn reap_when_exited(child: Child) {
        use std::sync::{OnceLock, mpsc};
        static REAPER: OnceLock<mpsc::Sender<Child>> = OnceLock::new();
        let sender = REAPER.get_or_init(|| {
            let (sender, receiver) = mpsc::channel::<Child>();
            std::thread::spawn(move || {
                let mut children = Vec::new();
                loop {
                    match receiver.recv_timeout(std::time::Duration::from_millis(100)) {
                        Ok(child) => children.push(child),
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                    children.retain_mut(|child| matches!(child.try_wait(), Ok(None)));
                }
            });
            sender
        });
        let _ = sender.send(child);
    }

    pub(crate) fn maybe_run_keeper() -> Option<i32> {
        let socket = std::env::var(SOCKET_ENV).ok()?;
        Some(
            (|| -> io::Result<()> {
                let socket: i32 = socket
                    .parse()
                    .map_err(|_| io::Error::other("Invalid keeper socket"))?;
                let owner: i32 = std::env::var(OWNER_ENV)
                    .map_err(io::Error::other)?
                    .parse()
                    .map_err(|_| io::Error::other("Invalid owner descriptor"))?;
                if socket < 3 || owner < 3 || socket == owner {
                    return Err(io::Error::other("Invalid keeper descriptors"));
                }
                // These descriptors are exclusively inherited from Keeper::spawn.
                let mut stream = unsafe { UnixStream::from_raw_fd(socket) };
                let owner = unsafe { OwnedFd::from_raw_fd(owner) };
                // Prevent core files without disabling access to /proc/<pid>/fd.
                let mut limit = libc::rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                };
                if unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut limit) } < 0 {
                    return Err(io::Error::other(format!(
                        "read core limit: {}",
                        io::Error::last_os_error()
                    )));
                }
                limit.rlim_cur = 0;
                if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) } < 0 {
                    return Err(io::Error::other(format!(
                        "disable core dumps: {}",
                        io::Error::last_os_error()
                    )));
                }
                let mut owner_watch = OwnerWatch::start(owner)?;
                stream.write_all(&[1])?;
                let mut files = Vec::new();
                let mut handed_off = false;
                loop {
                    let mut op = [0];
                    match stream.read_exact(&mut op) {
                        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                            if handed_off {
                                loop {
                                    let _ = &owner_watch;
                                    std::thread::park();
                                }
                            }
                            return Ok(());
                        }
                        result => result?,
                    }
                    match op[0] {
                        ALLOCATE => {
                            let mut length = [0; 8];
                            stream.read_exact(&mut length)?;
                            let length = u64::from_le_bytes(length);
                            if length > MAX_SECRET_BYTES {
                                return Err(io::Error::other("Oversized secret"));
                            }
                            let mut bytes =
                                secrecy::zeroize::Zeroizing::new(vec![0; length as usize]);
                            stream.read_exact(&mut bytes)?;
                            let result = anonymous_file(&bytes);
                            match result {
                                Ok(file) => {
                                    stream.write_all(&file.as_raw_fd().to_le_bytes())?;
                                    files.push(file);
                                }
                                Err(_) => stream.write_all(&(-1i32).to_le_bytes())?,
                            }
                        }
                        HANDOFF => {
                            let mut pid = [0; 4];
                            stream.read_exact(&mut pid)?;
                            let pid = u32::from_le_bytes(pid);
                            if pid != 0 {
                                let next_owner = syscall_fd(unsafe {
                                    libc::syscall(libc::SYS_pidfd_open, pid, 0)
                                })?;
                                let next_watch = OwnerWatch::start(next_owner)?;
                                // The launching process is still alive during
                                // this handshake. Stop watching it only after
                                // the daemon's watcher is installed.
                                drop(owner_watch);
                                owner_watch = next_watch;
                            }
                            handed_off = true;
                            stream.write_all(&[1])?;
                            // Keep the control channel available for a later
                            // transfer from a launching CLI to its daemon.
                            // EOF after handoff leaves only the process lease.
                        }
                        _ => return Err(io::Error::other("Invalid keeper operation")),
                    }
                }
            })()
            .map_or_else(
                |_error| {
                    #[cfg(test)]
                    eprintln!("keeper startup: {_error}");
                    1
                },
                |_| 0,
            ),
        )
    }

    struct OwnerWatch {
        cancel: File,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl OwnerWatch {
        fn start(owner: OwnedFd) -> io::Result<Self> {
            let cancel = File::from(syscall_fd(
                unsafe { libc::eventfd(0, libc::EFD_CLOEXEC) } as libc::c_long
            )?);
            let listener = cancel.try_clone()?;
            let thread = std::thread::spawn(move || {
                let mut descriptors = [
                    libc::pollfd {
                        fd: owner.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    },
                    libc::pollfd {
                        fd: listener.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    },
                ];
                loop {
                    let result =
                        unsafe { libc::poll(descriptors.as_mut_ptr(), descriptors.len() as _, -1) };
                    if result > 0 {
                        if descriptors[1].revents != 0 {
                            return;
                        }
                        if descriptors[0].revents != 0 {
                            std::process::exit(0);
                        }
                    }
                    if result < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted
                    {
                        std::process::exit(1);
                    }
                }
            });
            Ok(Self {
                cancel,
                thread: Some(thread),
            })
        }
    }

    impl Drop for OwnerWatch {
        fn drop(&mut self) {
            let _ = self.cancel.write_all(&1u64.to_ne_bytes());
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn anonymous_file(bytes: &[u8]) -> io::Result<File> {
        let fd = syscall_fd(unsafe {
            libc::syscall(
                libc::SYS_memfd_create,
                c"devenv-secret".as_ptr(),
                libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
            )
        })?;
        let mut file = File::from(fd);
        file.write_all(bytes)?;
        if unsafe { libc::fchmod(file.as_raw_fd(), 0o400) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let seals =
            libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_ADD_SEALS, seals) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(file)
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    use super::*;
    pub(crate) struct Keeper;
    impl Keeper {
        pub(crate) fn spawn(_: u32) -> std::io::Result<Self> {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Anonymous SecretSpec as_path files currently require Linux",
            ))
        }
        pub(crate) fn materialize(&self, _: &[u8]) -> std::io::Result<PathBuf> {
            unreachable!()
        }
        pub(crate) fn handoff_to(&self, _: Option<u32>) -> std::io::Result<()> {
            unreachable!()
        }
    }
    pub(crate) fn maybe_run_keeper() -> Option<i32> {
        None
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::{
        io::Write,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    // The test executable acts as the helper without requiring a separately
    // installed devenv binary. Keeper::spawn selects this test in test builds.
    #[test]
    fn keeper_entry() {
        if let Some(code) = maybe_run_keeper() {
            std::process::exit(code);
        }
    }

    fn wait_until_revoked(path: &std::path::Path) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!path.exists(), "anonymous secret survived its owner");
    }

    #[test]
    fn anonymous_files_are_read_only_and_removed_on_drop() {
        use std::os::unix::fs::PermissionsExt;
        let keeper = platform::Keeper::spawn(std::process::id()).unwrap();
        let path = keeper.materialize(b"key-contents").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"key-contents");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o400
        );
        let target = std::fs::read_link(&path).unwrap();
        assert!(target.to_string_lossy().starts_with("/memfd:devenv-secret"));
        if let Ok(mut file) = std::fs::OpenOptions::new().write(true).open(&path) {
            assert!(file.write_all(b"tampered").is_err());
        }
        drop(keeper);
        assert!(!path.exists());
    }

    #[test]
    fn handed_off_files_are_revoked_when_owner_is_killed() {
        let mut owner = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let keeper = platform::Keeper::spawn(owner.id()).unwrap();
        let path = keeper.materialize(b"leased-key").unwrap();
        keeper.handoff().unwrap();
        // Handoff can be called twice without sending a second message to a
        // keeper that has stopped accepting allocations.
        keeper.handoff().unwrap();
        assert!(keeper.materialize(b"too-late").is_err());
        drop(keeper);
        assert_eq!(std::fs::read(&path).unwrap(), b"leased-key");
        owner.kill().unwrap();
        owner.wait().unwrap();
        wait_until_revoked(&path);
    }

    #[test]
    fn daemon_handoff_survives_the_launching_process() {
        let mut launcher = Command::new("sleep").arg("60").spawn().unwrap();
        let mut daemon = Command::new("sleep").arg("60").spawn().unwrap();
        let keeper = platform::Keeper::spawn(launcher.id()).unwrap();
        let path = keeper.materialize(b"daemon-key").unwrap();
        keeper.handoff().unwrap();
        keeper.handoff_to(Some(daemon.id())).unwrap();
        drop(keeper);
        launcher.kill().unwrap();
        launcher.wait().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"daemon-key");
        daemon.kill().unwrap();
        daemon.wait().unwrap();
        wait_until_revoked(&path);
    }

    #[test]
    fn killing_keeper_revokes_files_without_cleanup_handlers() {
        let keeper = platform::Keeper::spawn(std::process::id()).unwrap();
        let path = keeper.materialize(b"key").unwrap();
        let pid: i32 = path
            .components()
            .nth(2)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGKILL,
        )
        .unwrap();
        wait_until_revoked(&path);
    }

    #[test]
    fn separate_resolutions_do_not_overwrite_or_revoke_each_other() {
        let first = platform::Keeper::spawn(std::process::id()).unwrap();
        let second = platform::Keeper::spawn(std::process::id()).unwrap();
        let first_path = first.materialize(b"first-key").unwrap();
        let second_path = second.materialize(b"second-key").unwrap();
        assert_ne!(first_path, second_path);
        drop(second);
        assert_eq!(std::fs::read(&first_path).unwrap(), b"first-key");
        assert!(!second_path.exists());
    }
}

#[cfg(test)]
mod export_tests {
    use super::*;

    fn resolve(owner: Option<u32>) -> String {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("secretspec.toml"),
            r#"
[project]
name = "anonymous-exports"
revision = "1.0"
require_reason = false
[profiles.default]
KEY = { description = "key", as_path = true }
"#,
        )
        .unwrap();
        std::fs::write(root.path().join(".env"), "KEY=test-key\n").unwrap();
        let mut spec =
            secretspec::Secrets::load_from(&root.path().join("secretspec.toml")).unwrap();
        spec.set_provider("dotenv:.env");
        SecretFiles::new(owner).configure(&mut spec);
        spec.validate()
            .err()
            .expect("unsupported export must fail closed")
            .to_string()
    }

    #[test]
    fn exports_without_an_owner_fail_closed() {
        assert!(resolve(None).contains("secret-owner-pid"));
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn unsupported_platforms_fail_without_creating_secret_files() {
        assert!(resolve(Some(std::process::id())).contains("require Linux"));
    }
}
