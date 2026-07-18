//! Process management for devenv
//!
//! This crate provides process management with two backends:
//! - Native: Using watchexec-supervisor with socket activation, PTY support, restart policies
//! - ProcessCompose: Using the external process-compose tool
//!
//! Both backends implement the `ProcessManager` trait for a unified interface.

use async_trait::async_trait;
use miette::Result;
use sha2::Digest;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Subdirectory name for process manager state
pub const PROCESSES_DIR: &str = "processes";

/// Socket filename for the native process manager API.
pub const NATIVE_SOCKET_NAME: &str = "native.sock";

/// Compute the devenv runtime directory for a given dotfile path.
///
/// The path must be stable across processes of the same user regardless of
/// their environment (notably `$TMPDIR`), so that every invocation finds the
/// same daemon socket. Resolution order:
///
/// 1. An inherited `$DEVENV_RUNTIME` whose basename matches this project's
///    hash and which passes ownership validation. This bridges environment
///    boundaries (sandboxes, `nix develop`) and devenv upgrades. A value
///    inherited from a different project (nested devenvs) has a different
///    hash and is rejected.
/// 2. `$XDG_RUNTIME_DIR/devenv-<hash>` when the base is owned by the user.
/// 3. The legacy `$TMPDIR/devenv-<hash>` when a pre-upgrade native manager
///    is still listening there.
/// 4. `/run/user/<uid>/devenv-<hash>` when the directory exists, for setups
///    where pam created it but the variable was stripped from the env.
/// 5. `/tmp/devenv-<uid>-<hash>`. The uid keeps users on a shared machine
///    from colliding; [`ensure_runtime_dir`] guards against squatting.
///
/// `$TMPDIR` is deliberately not used as a fallback: it is the only base that
/// legitimately differs between two processes of the same user, which made
/// process managers mutually invisible (#1153, #1578, #2923). It is consulted
/// only to reconnect to a native manager started by an older devenv.
///
/// The flakes-integration default in `src/modules/top-level.nix` mirrors this
/// logic; keep the two in sync.
pub fn compute_runtime_dir(devenv_dotfile: &Path) -> PathBuf {
    compute_runtime_dir_impl(
        devenv_dotfile,
        |name| std::env::var(name).ok(),
        nix::unistd::geteuid().as_raw(),
    )
}

/// The short project hash used in runtime directory names: the first 7 hex
/// chars (same length as git's abbreviated commit hashes) of the SHA-256 of
/// the dotfile path.
fn project_hash(devenv_dotfile: &Path) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(devenv_dotfile.to_string_lossy().as_bytes());
    hex::encode(hasher.finalize())[..7].to_string()
}

fn compute_runtime_dir_impl(
    devenv_dotfile: &Path,
    get_env: impl Fn(&str) -> Option<String>,
    euid: u32,
) -> PathBuf {
    let hash = project_hash(devenv_dotfile);
    let plain = format!("devenv-{hash}");
    let with_uid = format!("devenv-{euid}-{hash}");

    if let Some(path) = get_env("DEVENV_RUNTIME")
        .filter(|v| !v.is_empty())
        .and_then(|inherited| inherited_runtime_dir(inherited, &plain, &with_uid, euid))
    {
        return path;
    }

    if let Some(xdg) = get_env("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        let base = PathBuf::from(&xdg);
        if base_is_usable(&base, euid) {
            return base.join(plain);
        }
        tracing::warn!(
            base = %base.display(),
            "XDG_RUNTIME_DIR is not a directory owned by the current user, falling back"
        );
    }

    if let Some(tmpdir) = get_env("TMPDIR").filter(|v| !v.is_empty()) {
        let legacy = PathBuf::from(tmpdir).join(&plain);
        if legacy_native_manager_exists(&legacy, euid) {
            return legacy;
        }
    }

    let run_user = PathBuf::from(format!("/run/user/{euid}"));
    if base_is_usable(&run_user, euid) {
        return run_user.join(plain);
    }

    PathBuf::from("/tmp").join(with_uid)
}

/// Check for a pre-upgrade native manager in the old `$TMPDIR` location.
///
/// Merely finding the directory is not enough: old build artifacts must not
/// make `$TMPDIR` influence new invocations. The manager's Unix socket is the
/// rendezvous marker, and the containing directory must pass the same trust
/// check as an inherited runtime directory.
fn legacy_native_manager_exists(path: &Path, euid: u32) -> bool {
    use std::os::unix::fs::FileTypeExt;

    if validate_owned_dir(path, euid) != Ok(true) {
        return false;
    }

    std::fs::symlink_metadata(native_socket_path_in(path))
        .is_ok_and(|meta| meta.file_type().is_socket())
}

/// Resolve an inherited `$DEVENV_RUNTIME` that belongs to this project (its
/// basename is one of the two forms compute_runtime_dir produces) and already
/// exists, or `None` to fall through to recomputing the path.
fn inherited_runtime_dir(
    inherited: String,
    plain: &str,
    with_uid: &str,
    euid: u32,
) -> Option<PathBuf> {
    let path = PathBuf::from(&inherited);
    let basename_matches = path
        .file_name()
        .is_some_and(|n| n == plain || n == with_uid);
    if !basename_matches {
        tracing::debug!(
            inherited,
            "ignoring DEVENV_RUNTIME inherited from a different project"
        );
        return None;
    }
    match validate_owned_dir(&path, euid) {
        // An existing directory means a shell or daemon already rendezvoused
        // there; join it.
        Ok(true) => Some(path),
        // No directory, so no daemon to find; recompute instead of trusting a
        // path whose base may no longer exist.
        Ok(false) => None,
        Err(reason) => {
            tracing::warn!(
                path = %path.display(),
                reason,
                "ignoring inherited DEVENV_RUNTIME"
            );
            None
        }
    }
}

/// Check that `path` exists, is a directory (not a symlink), and is owned by
/// `euid`. This decides only identity and trust; permissions are enforced
/// separately by [`ensure_runtime_dir`], so directories created by older
/// devenv versions with a loose umask are accepted and tightened rather than
/// rejected (which would split the rendezvous).
///
/// Returns `Ok(false)` if the path does not exist, and `Err` with a reason if
/// it exists but is unsafe to use.
fn validate_owned_dir(path: &Path, euid: u32) -> std::result::Result<bool, String> {
    use std::os::unix::fs::MetadataExt;

    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("failed to stat: {e}")),
    };
    if !meta.is_dir() {
        return Err("not a directory".to_string());
    }
    if meta.uid() != euid {
        return Err(format!("owned by uid {}, not {}", meta.uid(), euid));
    }
    Ok(true)
}

/// Check that a base directory (e.g. `$XDG_RUNTIME_DIR`) is a directory owned
/// by `euid`. Follows symlinks, as the base is system-managed.
fn base_is_usable(base: &Path, euid: u32) -> bool {
    use std::os::unix::fs::MetadataExt;

    std::fs::metadata(base).is_ok_and(|meta| meta.is_dir() && meta.uid() == euid)
}

/// Create the runtime directory with owner-only permissions and verify it is
/// safe to use.
///
/// Tightens the permissions of pre-existing directories (older devenv versions
/// created them with the default umask) and refuses paths owned by other users
/// or replaced with symlinks, since `/tmp` is a shared namespace.
pub fn ensure_runtime_dir(path: &Path) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    let mut builder = std::fs::DirBuilder::new();
    builder.mode(0o700);
    match builder.create(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => {
            return Err(miette::miette!(
                "Failed to create {}: {}",
                path.display(),
                e
            ));
        }
    }

    let euid = nix::unistd::geteuid().as_raw();
    match validate_owned_dir(path, euid) {
        Ok(true) => {}
        Ok(false) => {
            return Err(miette::miette!(
                "Runtime directory {} disappeared while preparing it",
                path.display()
            ));
        }
        Err(reason) => {
            return Err(miette::miette!(
                "Refusing to use runtime directory {}: {}",
                path.display(),
                reason
            ));
        }
    }

    // DirBuilder::mode is masked by the umask, and pre-existing directories
    // (e.g. from older devenv versions) may be looser; always enforce
    // owner-only access.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|e| {
        miette::miette!(
            "Failed to restrict permissions of {}: {}",
            path.display(),
            e
        )
    })?;
    Ok(())
}

/// Compute the full path to the native process manager API socket for a given dotfile path.
pub fn native_socket_path(devenv_dotfile: &Path) -> PathBuf {
    native_socket_path_in(&compute_runtime_dir(devenv_dotfile))
}

/// The native process manager API socket path inside an already-resolved
/// runtime directory.
pub fn native_socket_path_in(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join(PROCESSES_DIR).join(NATIVE_SOCKET_NAME)
}

/// Get the runtime directory for processes given a base runtime directory.
/// Creates the directory if it doesn't exist.
pub fn get_process_runtime_dir(runtime_dir: &Path) -> Result<PathBuf> {
    ensure_runtime_dir(runtime_dir)?;
    let dir = runtime_dir.join(PROCESSES_DIR);
    ensure_runtime_dir(&dir)?;
    Ok(dir)
}

pub mod command;
pub mod config;
pub mod log_tailer;
pub mod manager;
pub mod pid;
pub mod process_compose;
pub mod pty;
pub mod socket_activation;
pub mod supervisor;
pub mod supervisor_state;

// Re-export config types at crate root
pub use config::{
    HttpGetProbe, HttpProbe, ListenKind, ListenSpec, ProcessConfig, ProcessType, ReadyConfig,
    RestartConfig, RestartPolicy, SocketActivationConfig, WatchConfig, WatchdogConfig,
};
pub use devenv_event_sources::{NotifyMessage, NotifySocket};
pub use manager::{
    ApiRequest, ApiResponse, JobHandle, NativeProcessManager, PortInfo, ProcessCommand,
    ProcessInfo, ProcessPhase, ProcessResources, ProcessState,
};
pub use pid::{PidStatus, check_pid_file, read_pid, remove_pid, write_pid};
pub use process_compose::ProcessComposeManager;
pub use pty::PtyProcess;
pub use socket_activation::{
    ActivatedSockets, ActivationSpec, ActivationSpecBuilder, SD_LISTEN_FDS_START,
    SocketActivationWrapper, activation_from_listen,
};
pub use supervisor_state::{JobStatus, SupervisorPhase};

/// Options for starting processes
#[derive(Debug, Clone, Default)]
pub struct StartOptions {
    /// Process configurations to start
    pub process_configs: HashMap<String, ProcessConfig>,
    /// Specific processes to start (empty = all)
    pub processes: Vec<String>,
    /// Run in background (detached from terminal)
    pub detach: bool,
    /// Log output to file instead of terminal (only for detached mode)
    pub log_to_file: bool,
    /// Environment variables to pass to processes
    pub env: HashMap<String, String>,
    /// Cancellation token for graceful shutdown coordination
    pub cancellation_token: Option<tokio_util::sync::CancellationToken>,
}

/// Common interface for process managers
#[async_trait]
pub trait ProcessManager: Send + Sync {
    /// Start processes with the given options
    async fn start(&self, options: StartOptions) -> Result<()>;

    /// Stop all running processes
    async fn stop(&self) -> Result<()>;

    /// Check if the process manager is currently running
    async fn is_running(&self) -> bool;
}

#[cfg(test)]
mod runtime_dir_tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    const DOTFILE: &str = "/home/user/project/.devenv";

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name: &str| map.get(name).cloned()
    }

    fn euid() -> u32 {
        nix::unistd::geteuid().as_raw()
    }

    fn compute(pairs: &[(&str, &str)], euid: u32) -> PathBuf {
        compute_runtime_dir_impl(Path::new(DOTFILE), env(pairs), euid)
    }

    /// The short project hash for DOTFILE.
    fn dotfile_hash() -> String {
        project_hash(Path::new(DOTFILE))
    }

    /// Regression test for #2923: a different TMPDIR must not change the path.
    #[test]
    fn tmpdir_does_not_affect_runtime_dir() {
        let default = compute(&[], euid());
        let overridden = compute(&[("TMPDIR", "/tmp/claude-501")], euid());
        assert_eq!(default, overridden);
    }

    #[test]
    fn legacy_tmpdir_used_while_native_manager_socket_exists() {
        let tmpdir = tempfile::tempdir().unwrap();
        let legacy = tmpdir.path().join(format!("devenv-{}", dotfile_hash()));
        let processes = legacy.join(PROCESSES_DIR);
        fs::create_dir_all(&processes).unwrap();
        let _listener =
            std::os::unix::net::UnixListener::bind(processes.join(NATIVE_SOCKET_NAME)).unwrap();

        let dir = compute(&[("TMPDIR", tmpdir.path().to_str().unwrap())], euid());
        assert_eq!(dir, legacy);
    }

    #[test]
    fn legacy_tmpdir_ignored_without_native_manager_socket() {
        let tmpdir = tempfile::tempdir().unwrap();
        let legacy = tmpdir.path().join(format!("devenv-{}", dotfile_hash()));
        fs::create_dir(&legacy).unwrap();

        let dir = compute(&[("TMPDIR", tmpdir.path().to_str().unwrap())], euid());
        assert_ne!(dir, legacy);
    }

    #[test]
    fn falls_back_to_tmp_with_uid() {
        // A synthetic euid has no /run/user/<uid>, forcing the /tmp fallback.
        let dir = compute(&[], 99999);
        assert!(dir.to_string_lossy().starts_with("/tmp/devenv-99999-"));
    }

    #[test]
    fn xdg_runtime_dir_used_when_owned() {
        let base = tempfile::tempdir().unwrap();
        let dir = compute(
            &[("XDG_RUNTIME_DIR", base.path().to_str().unwrap())],
            euid(),
        );
        assert_eq!(dir, base.path().join(format!("devenv-{}", dotfile_hash())));
    }

    #[test]
    fn xdg_runtime_dir_ignored_when_not_owned() {
        // The temp dir is owned by the test user, not the synthetic euid.
        let base = tempfile::tempdir().unwrap();
        let dir = compute(&[("XDG_RUNTIME_DIR", base.path().to_str().unwrap())], 99999);
        assert!(dir.to_string_lossy().starts_with("/tmp/devenv-99999-"));
    }

    #[test]
    fn inherited_runtime_honored_for_both_basename_forms() {
        // The per-user bases use devenv-<hash> while the /tmp fallback uses
        // devenv-<uid>-<hash> (and the Nix-side default mirrors both); an
        // inherited value in either form must be accepted.
        let hash = dotfile_hash();

        for name in [
            format!("devenv-{hash}"),
            format!("devenv-{}-{hash}", euid()),
        ] {
            let base = tempfile::tempdir().unwrap();
            let inherited = base.path().join(&name);
            ensure_runtime_dir(&inherited).unwrap();
            let dir = compute(&[("DEVENV_RUNTIME", inherited.to_str().unwrap())], euid());
            assert_eq!(dir, inherited);
        }
    }

    #[test]
    fn inherited_runtime_ignored_for_different_project() {
        // A nested devenv inherits the outer project's DEVENV_RUNTIME; the
        // hash mismatch must reject it.
        let base = tempfile::tempdir().unwrap();
        let inherited = base.path().join("devenv-0000000");
        ensure_runtime_dir(&inherited).unwrap();
        let dir = compute(&[("DEVENV_RUNTIME", inherited.to_str().unwrap())], euid());
        assert_ne!(dir, inherited);
    }

    #[test]
    fn inherited_runtime_ignored_when_missing() {
        let base = tempfile::tempdir().unwrap();
        let inherited = base.path().join(format!("devenv-{}", dotfile_hash()));
        let dir = compute(&[("DEVENV_RUNTIME", inherited.to_str().unwrap())], euid());
        assert_ne!(dir, inherited);
    }

    #[test]
    fn inherited_runtime_honored_despite_loose_permissions() {
        // Directories created by older devenv versions can be group/other
        // accessible; rejecting them would split the rendezvous, so they are
        // accepted here and tightened by ensure_runtime_dir before use.
        let base = tempfile::tempdir().unwrap();
        let inherited = base.path().join(format!("devenv-{}", dotfile_hash()));
        fs::create_dir(&inherited).unwrap();
        fs::set_permissions(&inherited, fs::Permissions::from_mode(0o775)).unwrap();
        let dir = compute(&[("DEVENV_RUNTIME", inherited.to_str().unwrap())], euid());
        assert_eq!(dir, inherited);
    }

    #[test]
    fn inherited_runtime_ignored_when_symlink() {
        let base = tempfile::tempdir().unwrap();
        let target = base.path().join("target");
        fs::create_dir(&target).unwrap();
        let inherited = base.path().join(format!("devenv-{}", dotfile_hash()));
        std::os::unix::fs::symlink(&target, &inherited).unwrap();
        let dir = compute(&[("DEVENV_RUNTIME", inherited.to_str().unwrap())], euid());
        assert_ne!(dir, inherited);
    }

    #[test]
    fn ensure_runtime_dir_creates_private_dir() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("devenv-test");
        ensure_runtime_dir(&dir).unwrap();
        let mode = fs::symlink_metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }

    #[test]
    fn ensure_runtime_dir_tightens_permissions() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("devenv-test");
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        ensure_runtime_dir(&dir).unwrap();
        let mode = fs::symlink_metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }

    #[test]
    fn ensure_runtime_dir_rejects_symlink() {
        let base = tempfile::tempdir().unwrap();
        let target = base.path().join("target");
        fs::create_dir(&target).unwrap();
        let dir = base.path().join("devenv-test");
        std::os::unix::fs::symlink(&target, &dir).unwrap();
        assert!(ensure_runtime_dir(&dir).is_err());
    }

    #[test]
    fn ensure_runtime_dir_rejects_file() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("devenv-test");
        fs::write(&dir, "").unwrap();
        assert!(ensure_runtime_dir(&dir).is_err());
    }
}
