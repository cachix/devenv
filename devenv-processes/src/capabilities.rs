//! Persistent privileged broker for Linux capability-bearing processes.

use miette::{IntoDiagnostic, Result, WrapErr, bail};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::ffi::{CString, OsString};
use std::future::Future;
use std::io::{IsTerminal, Read, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tokio::sync::oneshot::error::TryRecvError;

const BROKER_ARG: &str = "--devenv-capability-broker";
const MAX_MESSAGE: usize = 16 * 1024 * 1024;
static BROKER_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityRequest {
    pub process: String,
    pub capabilities: Vec<String>,
}

#[cfg(target_os = "linux")]
const ALLOWED: &[(caps::Capability, &str)] = &[
    (
        caps::Capability::CAP_NET_BIND_SERVICE,
        "bind to TCP/UDP ports below 1024",
    ),
    (caps::Capability::CAP_NET_RAW, "use raw and packet sockets"),
    (
        caps::Capability::CAP_NET_ADMIN,
        "configure network interfaces and routing",
    ),
    (caps::Capability::CAP_IPC_LOCK, "lock memory"),
    (caps::Capability::CAP_SYS_NICE, "change scheduling priority"),
    (
        caps::Capability::CAP_SYS_RESOURCE,
        "override selected resource limits",
    ),
    (
        caps::Capability::CAP_SYS_ADMIN,
        "perform system administration operations",
    ),
    (caps::Capability::CAP_CHOWN, "change file ownership"),
    (
        caps::Capability::CAP_DAC_OVERRIDE,
        "bypass file permission checks",
    ),
    (
        caps::Capability::CAP_FOWNER,
        "bypass checks requiring file ownership",
    ),
];

#[cfg(target_os = "linux")]
fn parse_capability(name: &str) -> Result<caps::Capability> {
    let normalized = name.trim().to_ascii_uppercase().replace('-', "_");
    let normalized = if normalized.starts_with("CAP_") {
        normalized
    } else {
        format!("CAP_{normalized}")
    };
    let capability = normalized
        .parse::<caps::Capability>()
        .map_err(|_| miette::miette!("unknown Linux capability '{name}'"))?;
    if !ALLOWED.iter().any(|(allowed, _)| *allowed == capability) {
        bail!(
            "Linux capability '{}' is not allowed for devenv processes",
            normalized.to_ascii_lowercase()
        );
    }
    Ok(capability)
}

#[cfg(target_os = "linux")]
pub(crate) fn validate_capabilities(names: &[String]) -> Result<()> {
    for name in names {
        parse_capability(name)?;
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn validate_capabilities(names: &[String]) -> Result<()> {
    if !names.is_empty() {
        bail!("process Linux capabilities are only supported on Linux");
    }
    Ok(())
}

fn process_list(requests: &[CapabilityRequest]) -> String {
    requests
        .iter()
        .map(|request| format!("'{}'", request.process))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Authenticate once and start the root broker before a manager detaches.
///
/// `required_now` says whether a capability-bearing process starts in this
/// invocation. When none does and sudo cannot prompt, the broker is skipped
/// with a warning instead of failing the whole start. `stderr` receives the
/// broker's diagnostics; a detached manager should point it at a log file so
/// the broker never touches a terminal that may go away.
pub async fn start_capability_broker(
    requests: &[CapabilityRequest],
    required_now: bool,
    runtime_dir: &Path,
    frontend: Option<&tokio::sync::mpsc::Sender<devenv_mailbox::FrontendCommand>>,
    stderr: Stdio,
) -> Result<Option<PathBuf>> {
    if requests.is_empty() {
        return Ok(None);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (required_now, runtime_dir, frontend, stderr);
        devenv_activity::message(
            devenv_activity::ActivityLevel::Warn,
            format!(
                "Linux capabilities are only supported on Linux; starting {} without them",
                process_list(requests)
            ),
        );
        return Ok(None);
    }
    #[cfg(target_os = "linux")]
    {
        for request in requests {
            validate_capabilities(&request.capabilities)?;
        }
        if !ensure_sudo_authentication(requests, required_now, frontend).await? {
            return Ok(None);
        }
        std::fs::create_dir_all(runtime_dir).into_diagnostic()?;
        let id = BROKER_ID.fetch_add(1, Ordering::Relaxed);
        let path = runtime_dir.join(format!(
            "capability-broker-{}-{id}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let executable = std::env::current_exe().into_diagnostic()?;
        let allowed = serde_json::to_string(requests).into_diagnostic()?;
        let mut command = Command::new("sudo");
        let mut child = command
            .arg("-n")
            .arg("--")
            .arg(executable)
            .arg(BROKER_ARG)
            .arg("--socket")
            .arg(&path)
            .arg("--allow-json")
            .arg(allowed)
            // The broker never reads input. With sudo's `use_pty` an inherited
            // terminal would be relayed to it, competing with the TUI for keys.
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            // Keep terminal signals (Ctrl-C, hangup) away from the broker. Its
            // lifetime is tied to the manager's socket connection instead, so a
            // detached manager keeps its broker after the terminal closes. A
            // new session is deliberately not created: sudo keys its cached
            // credentials on the controlling terminal.
            .process_group(0)
            .spawn()
            .into_diagnostic()
            .wrap_err("failed to start Linux capability broker through sudo")?;
        let uid = unsafe { libc::getuid() };
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            // The broker binds as root and hands the socket over with chown as
            // its last step before accepting, so ownership marks readiness.
            // Connecting earlier fails with EACCES.
            if std::fs::metadata(&path).is_ok_and(|meta| meta.uid() == uid) {
                return Ok(Some(path));
            }
            if let Some(status) = child.try_wait().into_diagnostic()? {
                bail!("Linux capability broker exited during startup: {status}");
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                bail!("timed out waiting for Linux capability broker");
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

/// Returns whether the broker should be started. `false` means sudo could not
/// prompt and no process needs capabilities right now.
#[cfg(target_os = "linux")]
async fn ensure_sudo_authentication(
    requests: &[CapabilityRequest],
    required_now: bool,
    frontend: Option<&tokio::sync::mpsc::Sender<devenv_mailbox::FrontendCommand>>,
) -> Result<bool> {
    let cached = Command::new("sudo")
        .args(["-n", "true"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !cached && !std::io::stderr().is_terminal() {
        if required_now {
            bail!(
                "sudo authentication is required for Linux capabilities; run `sudo -v` before devenv in non-interactive environments"
            );
        }
        devenv_activity::message(
            devenv_activity::ActivityLevel::Warn,
            format!(
                "sudo cannot prompt here, so {} cannot be started with Linux capabilities until devenv runs from a terminal or after `sudo -v`",
                process_list(requests)
            ),
        );
        return Ok(false);
    }

    let mut resume = None;
    if let Some(frontend) = frontend {
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(0);
        let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(0);
        frontend
            .send(devenv_mailbox::FrontendCommand::PauseForInteraction {
                ready: ready_tx,
                resume: resume_rx,
            })
            .await
            .map_err(|_| miette::miette!("terminal frontend stopped before sudo authentication"))?;
        tokio::task::spawn_blocking(move || ready_rx.recv())
            .await
            .into_diagnostic()?
            .map_err(|_| miette::miette!("terminal frontend stopped before sudo authentication"))?;
        resume = Some(resume_tx);
    }

    // The frontend has restored cooked terminal mode at this point. Keeping the
    // disclosure and sudo prompt in the same pause window ensures the TUI cannot
    // redraw over the explanation before the user has a chance to read it.
    let result = (|| {
        display_capability_requests(requests)?;
        if cached {
            return Ok(true);
        }
        let status = Command::new("sudo").arg("-v").status().into_diagnostic()?;
        if !status.success() {
            bail!("sudo authentication failed for the Linux capability broker");
        }
        Ok(true)
    })();
    if let Some(resume) = resume {
        let _ = resume.send(());
    }
    result
}

#[cfg(target_os = "linux")]
fn display_capability_requests(requests: &[CapabilityRequest]) -> Result<()> {
    eprintln!("\nThe following processes request Linux capabilities:\n");
    for request in requests {
        for name in &request.capabilities {
            let cap = parse_capability(name)?;
            let description = ALLOWED
                .iter()
                .find(|(allowed, _)| *allowed == cap)
                .expect("validated capability must have a description")
                .1;
            let display = format!("{cap:?}").trim_start_matches("CAP_").to_string();
            eprintln!("  {:<20} {:<24} {}", request.process, display, description);
        }
    }
    eprintln!();
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
enum Request {
    Launch(Box<LaunchRequest>),
    Poll,
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize)]
struct LaunchRequest {
    process: String,
    capabilities: Vec<String>,
    program: String,
    args: Vec<String>,
    env: HashMap<String, String>,
    cwd: PathBuf,
    stdout: PathBuf,
    stderr: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
enum Response {
    Launched { pid: u32 },
    Exited { processes: Vec<Exited> },
    Ok,
    Error { message: String },
}

#[derive(Debug, Serialize, Deserialize)]
struct Exited {
    pid: u32,
    exit: ProcessExit,
}

#[derive(Debug, Serialize, Deserialize)]
enum ProcessExit {
    Exited(i32),
    Signaled(i32),
}

fn write_message<T: Serialize>(stream: &mut UnixStream, value: &T) -> std::io::Result<()> {
    let payload = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    if payload.len() > MAX_MESSAGE {
        return Err(std::io::Error::other("capability broker message too large"));
    }
    stream.write_all(&(payload.len() as u32).to_be_bytes())?;
    stream.write_all(&payload)?;
    stream.flush()
}

fn read_message<T: for<'de> Deserialize<'de>>(stream: &mut UnixStream) -> std::io::Result<T> {
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length > MAX_MESSAGE {
        return Err(std::io::Error::other("capability broker message too large"));
    }
    let mut payload = vec![0; length];
    stream.read_exact(&mut payload)?;
    serde_json::from_slice(&payload).map_err(std::io::Error::other)
}

#[derive(Clone)]
pub(crate) struct CapabilityBrokerClient {
    stream: Arc<Mutex<UnixStream>>,
    exits: Arc<Mutex<HashMap<u32, oneshot::Sender<ProcessExit>>>>,
    shutdown: Arc<AtomicBool>,
    owners: Arc<()>,
}

impl CapabilityBrokerClient {
    pub(crate) fn connect(path: &Path) -> Result<Self> {
        let stream = UnixStream::connect(path)
            .into_diagnostic()
            .wrap_err_with(|| {
                format!("failed to connect to capability broker {}", path.display())
            })?;
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .into_diagnostic()?;
        let client = Self {
            stream: Arc::new(Mutex::new(stream)),
            exits: Arc::new(Mutex::new(HashMap::new())),
            shutdown: Arc::new(AtomicBool::new(false)),
            owners: Arc::new(()),
        };
        client.start_polling();
        Ok(client)
    }

    fn start_polling(&self) {
        let stream = Arc::clone(&self.stream);
        let exits = Arc::clone(&self.exits);
        let shutdown = Arc::clone(&self.shutdown);
        tokio::spawn(async move {
            while !shutdown.load(Ordering::Relaxed) {
                tokio::time::sleep(Duration::from_millis(100)).await;
                let response = tokio::task::block_in_place(|| {
                    let mut stream = stream.lock().unwrap_or_else(|e| e.into_inner());
                    write_message(&mut stream, &Request::Poll)?;
                    read_message::<Response>(&mut stream)
                });
                match response {
                    Ok(Response::Exited { processes }) => {
                        let mut senders = exits.lock().unwrap_or_else(|e| e.into_inner());
                        for process in processes {
                            if let Some(sender) = senders.remove(&process.pid) {
                                let _ = sender.send(process.exit);
                            }
                        }
                    }
                    Ok(Response::Error { message }) => {
                        tracing::warn!(%message, "capability broker poll failed")
                    }
                    Ok(_) => tracing::warn!("unexpected capability broker poll response"),
                    Err(error) => {
                        if !shutdown.load(Ordering::Relaxed) {
                            tracing::warn!(%error, "lost capability broker");
                        }
                        break;
                    }
                }
            }
            // Cancel every outstanding child wait if the broker connection is
            // gone. Receivers translate cancellation into a killed child, so
            // the supervisor cannot hang forever waiting on a lost broker.
            exits.lock().unwrap_or_else(|e| e.into_inner()).clear();
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn launch(
        &self,
        process: &str,
        capabilities: &[String],
        program: &str,
        args: &[String],
        env: &HashMap<String, String>,
        cwd: &Path,
        stdout: &Path,
        stderr: &Path,
    ) -> std::io::Result<Box<dyn process_wrap::tokio::ChildWrapper>> {
        let request = Request::Launch(Box::new(LaunchRequest {
            process: process.to_string(),
            capabilities: capabilities.to_vec(),
            program: program.to_string(),
            args: args.to_vec(),
            env: env.clone(),
            cwd: cwd.to_path_buf(),
            stdout: stdout.to_path_buf(),
            stderr: stderr.to_path_buf(),
        }));
        let (sender, receiver) = oneshot::channel();
        let mut stream = self.stream.lock().unwrap_or_else(|e| e.into_inner());
        write_message(&mut stream, &request)?;
        match read_message::<Response>(&mut stream)? {
            Response::Launched { pid } => {
                self.exits
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(pid, sender);
                Ok(Box::new(BrokerChild {
                    pid,
                    exit: Some(receiver),
                    status: None,
                }))
            }
            Response::Error { message } => Err(std::io::Error::other(message)),
            _ => Err(std::io::Error::other(
                "unexpected capability broker response",
            )),
        }
    }
}

impl Drop for CapabilityBrokerClient {
    fn drop(&mut self) {
        if Arc::strong_count(&self.owners) == 1 {
            self.shutdown.store(true, Ordering::Relaxed);
            if let Ok(mut stream) = self.stream.lock() {
                let _ = write_message(&mut stream, &Request::Shutdown);
            }
        }
    }
}

struct BrokerChild {
    pid: u32,
    /// Exit notification from the broker poll loop. Cleared once consumed so
    /// the completed oneshot is never polled again.
    exit: Option<oneshot::Receiver<ProcessExit>>,
    status: Option<ExitStatus>,
}
impl std::fmt::Debug for BrokerChild {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrokerChild")
            .field("pid", &self.pid)
            .finish()
    }
}
fn exit_status(exit: ProcessExit) -> ExitStatus {
    ExitStatus::from_raw(match exit {
        ProcessExit::Exited(code) => code << 8,
        ProcessExit::Signaled(signal) => signal,
    })
}
impl process_wrap::tokio::ChildWrapper for BrokerChild {
    fn inner(&self) -> &dyn process_wrap::tokio::ChildWrapper {
        unreachable!()
    }
    fn inner_mut(&mut self) -> &mut dyn process_wrap::tokio::ChildWrapper {
        unreachable!()
    }
    fn into_inner(self: Box<Self>) -> Box<dyn process_wrap::tokio::ChildWrapper> {
        unreachable!()
    }
    fn id(&self) -> Option<u32> {
        Some(self.pid)
    }
    fn wait(&mut self) -> Pin<Box<dyn Future<Output = std::io::Result<ExitStatus>> + Send + '_>> {
        Box::pin(async move {
            if let Some(status) = self.status {
                return Ok(status);
            }
            let Some(receiver) = self.exit.as_mut() else {
                return Err(std::io::Error::other(
                    "capability child exit status already consumed",
                ));
            };
            // Poll the receiver in place. The supervisor drops and recreates
            // this future whenever a control message wins its select loop, and
            // a cancelled wait must not lose the exit notification. A dropped
            // sender means the broker is gone and the child was killed with it.
            let exit = receiver
                .await
                .unwrap_or(ProcessExit::Signaled(libc::SIGKILL));
            self.exit = None;
            let status = exit_status(exit);
            self.status = Some(status);
            Ok(status)
        })
    }
    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        if let Some(status) = self.status {
            return Ok(Some(status));
        }
        let exit = match self.exit.as_mut().map(|receiver| receiver.try_recv()) {
            Some(Ok(exit)) => exit,
            Some(Err(TryRecvError::Closed)) => ProcessExit::Signaled(libc::SIGKILL),
            Some(Err(TryRecvError::Empty)) | None => return Ok(None),
        };
        self.exit = None;
        let status = exit_status(exit);
        self.status = Some(status);
        Ok(Some(status))
    }
    fn start_kill(&mut self) -> std::io::Result<()> {
        self.signal(libc::SIGKILL)
    }
    fn signal(&self, signal: i32) -> std::io::Result<()> {
        let pid = i32::try_from(self.pid)
            .map_err(|_| std::io::Error::other("invalid broker child pid"))?;
        if unsafe { libc::kill(pid, signal) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

pub fn maybe_run_capability_helper() -> Option<i32> {
    if std::env::args_os().nth(1).as_deref() != Some(std::ffi::OsStr::new(BROKER_ARG)) {
        return None;
    }
    Some(match run_broker(std::env::args_os().skip(2).collect()) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("devenv capability broker: {error:?}");
            126
        }
    })
}

#[cfg(not(target_os = "linux"))]
fn run_broker(_args: Vec<OsString>) -> Result<()> {
    bail!("Linux capabilities are unsupported on this platform")
}

#[cfg(target_os = "linux")]
fn run_broker(args: Vec<OsString>) -> Result<()> {
    // The broker outlives the terminal that started it: a detached manager
    // keeps using it after a hangup, and Ctrl-C is handled by the manager,
    // which stops its children through the socket. Only the socket connection
    // ends the broker.
    unsafe {
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
        libc::signal(libc::SIGINT, libc::SIG_IGN);
    }
    let (path, allowed_json) = match args.as_slice() {
        [socket_flag, path, allow_flag, allowed]
            if socket_flag == "--socket" && allow_flag == "--allow-json" =>
        {
            (PathBuf::from(path), allowed)
        }
        _ => bail!("expected --socket <path> --allow-json <requests>"),
    };
    if unsafe { libc::geteuid() } != 0 {
        bail!("broker must run through sudo");
    }
    let uid = sudo_id("SUDO_UID")?;
    let gid = sudo_id("SUDO_GID")?;
    if uid == 0 || gid == 0 {
        bail!("refusing to launch processes as root");
    }
    let user = CString::new(std::env::var("SUDO_USER").into_diagnostic()?).into_diagnostic()?;
    let allowed_requests: Vec<CapabilityRequest> =
        serde_json::from_str(&allowed_json.to_string_lossy()).into_diagnostic()?;
    let mut allowed = HashMap::<String, HashSet<caps::Capability>>::new();
    for request in allowed_requests {
        let entry = allowed.entry(request.process).or_default();
        for name in request.capabilities {
            entry.insert(parse_capability(&name)?);
        }
    }
    validate_socket_path(&path, uid)?;
    let listener = UnixListener::bind(&path).into_diagnostic()?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).into_diagnostic()?;
    if unsafe { libc::chown(path_cstring(&path)?.as_ptr(), uid, gid) } != 0 {
        return Err(std::io::Error::last_os_error()).into_diagnostic();
    }
    listener.set_nonblocking(true).into_diagnostic()?;
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    bail!("timed out waiting for process manager");
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(error).into_diagnostic(),
        }
    };
    if peer_uid(&stream)? != uid {
        bail!("capability broker rejected client uid");
    }
    let mut known = HashSet::new();
    let result = serve_manager(&mut stream, &allowed, uid, gid, &user, &mut known);
    // Every exit path, including a failed write to the manager, tears down the
    // children the broker launched and removes the socket.
    kill_all(&known);
    let _ = std::fs::remove_file(&path);
    result
}

/// Serve one manager connection until it shuts down or disconnects. Children
/// launched meanwhile are recorded in `known` for the caller to clean up.
#[cfg(target_os = "linux")]
fn serve_manager(
    stream: &mut UnixStream,
    allowed: &HashMap<String, HashSet<caps::Capability>>,
    uid: libc::uid_t,
    gid: libc::gid_t,
    user: &CString,
    known: &mut HashSet<u32>,
) -> Result<()> {
    let mut exited = HashMap::new();
    while let Ok(request) = read_message::<Request>(stream) {
        match request {
            Request::Launch(request) => {
                let LaunchRequest {
                    process,
                    capabilities,
                    program,
                    args,
                    env,
                    cwd,
                    stdout,
                    stderr,
                } = *request;
                let requested = capabilities
                    .iter()
                    .map(|name| parse_capability(name))
                    .collect::<Result<HashSet<_>>>();
                let authorized = requested.as_ref().is_ok_and(|requested| {
                    allowed
                        .get(&process)
                        .is_some_and(|granted| requested.is_subset(granted))
                });
                // An unauthorized request is answered like any other launch
                // failure. Ending the broker here would orphan the children it
                // already launched.
                let response = if !authorized {
                    Response::Error {
                        message: format!(
                            "process '{process}' requested undeclared Linux capabilities"
                        ),
                    }
                } else {
                    match launch_child(
                        uid,
                        gid,
                        user,
                        &capabilities,
                        &program,
                        &args,
                        &env,
                        &cwd,
                        &stdout,
                        &stderr,
                    ) {
                        Ok(pid) => {
                            known.insert(pid);
                            Response::Launched { pid }
                        }
                        Err(error) => Response::Error {
                            message: format!("failed to launch '{process}': {error:?}"),
                        },
                    }
                };
                write_message(stream, &response).into_diagnostic()?;
            }
            Request::Poll => {
                reap(known, &mut exited);
                let processes = exited
                    .drain()
                    .map(|(pid, exit)| Exited { pid, exit })
                    .collect();
                write_message(stream, &Response::Exited { processes }).into_diagnostic()?;
            }
            Request::Shutdown => {
                kill_all(known);
                known.clear();
                let _ = write_message(stream, &Response::Ok);
                return Ok(());
            }
        }
        reap(known, &mut exited);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn validate_socket_path(path: &Path, uid: libc::uid_t) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| miette::miette!("broker path has no parent"))?;
    let metadata = std::fs::symlink_metadata(parent).into_diagnostic()?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if !metadata.is_dir()
        || metadata.uid() != uid
        || !name.starts_with("capability-broker-")
        || !name.ends_with(".sock")
        || path.exists()
    {
        bail!("invalid capability broker socket path");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> Result<libc::uid_t> {
    use std::os::fd::AsRawFd;
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error()).into_diagnostic();
    }
    Ok(cred.uid)
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn launch_child(
    uid: libc::uid_t,
    gid: libc::gid_t,
    user: &CString,
    names: &[String],
    program: &str,
    args: &[String],
    env: &HashMap<String, String>,
    cwd: &Path,
    stdout: &Path,
    stderr: &Path,
) -> Result<u32> {
    let requested = names
        .iter()
        .map(|n| parse_capability(n))
        .collect::<Result<HashSet<_>>>()?;
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(std::io::Error::last_os_error()).into_diagnostic();
    }
    if pid == 0 {
        let Err(error) = child_exec(
            uid, gid, user, &requested, program, args, env, cwd, stdout, stderr,
        );
        // Never panic in the forked child: stderr may be a closed terminal.
        let _ = writeln!(std::io::stderr(), "capability child: {error:?}");
        unsafe { libc::_exit(126) };
    }
    u32::try_from(pid).into_diagnostic()
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn child_exec(
    uid: libc::uid_t,
    gid: libc::gid_t,
    user: &CString,
    requested: &HashSet<caps::Capability>,
    program: &str,
    args: &[String],
    env: &HashMap<String, String>,
    cwd: &Path,
    stdout: &Path,
    stderr: &Path,
) -> Result<std::convert::Infallible> {
    // The broker ignores terminal signals so it can survive a detached
    // manager. Ignored dispositions survive exec, so restore them before the
    // service inherits the broker's signal state.
    for signal in [libc::SIGHUP, libc::SIGINT] {
        if unsafe { libc::signal(signal, libc::SIG_DFL) } == libc::SIG_ERR {
            return Err(std::io::Error::last_os_error()).into_diagnostic();
        }
    }
    if unsafe { libc::setsid() } < 0 {
        return Err(std::io::Error::last_os_error()).into_diagnostic();
    }
    let mut identity = requested.clone();
    identity.extend([
        caps::Capability::CAP_SETUID,
        caps::Capability::CAP_SETGID,
        caps::Capability::CAP_SETPCAP,
    ]);
    set_caps(&identity)?;
    // `caps::all()` is a static list; dropping a capability this kernel does
    // not know fails with EINVAL. Probe the bounding set instead.
    for cap in caps::runtime::thread_all_supported() {
        if !requested.contains(&cap) && cap != caps::Capability::CAP_SETPCAP {
            caps::drop(None, caps::CapSet::Bounding, cap).into_diagnostic()?;
        }
    }
    let bits = libc::SECBIT_KEEP_CAPS | libc::SECBIT_NO_SETUID_FIXUP;
    if unsafe { libc::prctl(libc::PR_SET_SECUREBITS, bits, 0, 0, 0) } != 0 {
        return Err(std::io::Error::last_os_error()).into_diagnostic();
    }
    if unsafe { libc::initgroups(user.as_ptr(), gid) } != 0
        || unsafe { libc::setresgid(gid, gid, gid) } != 0
        || unsafe { libc::setresuid(uid, uid, uid) } != 0
    {
        return Err(std::io::Error::last_os_error()).into_diagnostic();
    }
    let mut transition = requested.clone();
    transition.insert(caps::Capability::CAP_SETPCAP);
    set_caps(&transition)?;
    for cap in requested {
        caps::raise(None, caps::CapSet::Ambient, *cap).into_diagnostic()?;
    }
    caps::drop(None, caps::CapSet::Bounding, caps::Capability::CAP_SETPCAP).into_diagnostic()?;
    if unsafe { libc::prctl(libc::PR_SET_SECUREBITS, 0, 0, 0, 0) } != 0 {
        return Err(std::io::Error::last_os_error()).into_diagnostic();
    }
    set_caps(requested)?;
    let out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(stdout)
        .into_diagnostic()?;
    let err = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(stderr)
        .into_diagnostic()?;
    let error = Command::new(program)
        .args(args)
        .env_clear()
        .envs(env)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .exec();
    Err(error)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to exec {program}"))
}

#[cfg(target_os = "linux")]
fn set_caps(value: &HashSet<caps::Capability>) -> Result<()> {
    for set in [
        caps::CapSet::Effective,
        caps::CapSet::Permitted,
        caps::CapSet::Inheritable,
    ] {
        caps::set(None, set, value).into_diagnostic()?;
    }
    Ok(())
}
#[cfg(target_os = "linux")]
fn sudo_id(name: &str) -> Result<libc::id_t> {
    std::env::var(name)
        .into_diagnostic()?
        .parse()
        .into_diagnostic()
}
#[cfg(target_os = "linux")]
fn path_cstring(path: &Path) -> Result<CString> {
    use std::os::unix::ffi::OsStrExt;
    CString::new(path.as_os_str().as_bytes()).into_diagnostic()
}
#[cfg(target_os = "linux")]
fn reap(known: &mut HashSet<u32>, exited: &mut HashMap<u32, ProcessExit>) {
    loop {
        let mut status = 0;
        let pid = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
        if pid <= 0 {
            break;
        }
        let pid = pid as u32;
        known.remove(&pid);
        let exit = if libc::WIFEXITED(status) {
            ProcessExit::Exited(libc::WEXITSTATUS(status))
        } else if libc::WIFSIGNALED(status) {
            ProcessExit::Signaled(libc::WTERMSIG(status))
        } else {
            continue;
        };
        exited.insert(pid, exit);
    }
}
#[cfg(target_os = "linux")]
fn kill_all(known: &HashSet<u32>) {
    if known.is_empty() {
        return;
    }
    for pid in known {
        let _ = unsafe { libc::kill(-(*pid as i32), libc::SIGTERM) };
    }
    std::thread::sleep(Duration::from_millis(200));
    for pid in known {
        let _ = unsafe { libc::kill(-(*pid as i32), libc::SIGKILL) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(target_os = "linux")]
    fn capability_names() {
        assert_eq!(
            parse_capability("net_bind_service").unwrap(),
            caps::Capability::CAP_NET_BIND_SERVICE
        );
        assert_eq!(
            parse_capability("CAP_NET_BIND_SERVICE").unwrap(),
            caps::Capability::CAP_NET_BIND_SERVICE
        );
        assert_eq!(
            parse_capability("sys_admin").unwrap(),
            caps::Capability::CAP_SYS_ADMIN
        );
        assert!(parse_capability("sys_ptrace").is_err());
    }

    fn broker_child() -> (oneshot::Sender<ProcessExit>, BrokerChild) {
        let (sender, receiver) = oneshot::channel();
        let child = BrokerChild {
            pid: 1,
            exit: Some(receiver),
            status: None,
        };
        (sender, child)
    }

    /// The supervisor drops and recreates the wait future on every control
    /// message; a cancelled wait must not turn a live child into a finished one.
    #[tokio::test]
    async fn wait_keeps_exit_channel_across_cancellation() {
        use process_wrap::tokio::ChildWrapper;
        use std::task::{Context, Poll, Waker};

        let (sender, mut child) = broker_child();
        {
            let mut wait = child.wait();
            let mut cx = Context::from_waker(Waker::noop());
            assert!(matches!(wait.as_mut().poll(&mut cx), Poll::Pending));
        }
        assert!(child.try_wait().unwrap().is_none());

        sender.send(ProcessExit::Exited(3)).unwrap();
        let status = child.wait().await.unwrap();
        assert_eq!(status.code(), Some(3));
        assert_eq!(child.try_wait().unwrap(), Some(status));
        assert_eq!(child.wait().await.unwrap(), status);
    }

    #[test]
    fn try_wait_reports_lost_broker_as_killed() {
        use process_wrap::tokio::ChildWrapper;

        let (sender, mut child) = broker_child();
        drop(sender);
        let status = child
            .try_wait()
            .unwrap()
            .expect("a closed channel ends the child");
        assert_eq!(status.signal(), Some(libc::SIGKILL));
    }
}
