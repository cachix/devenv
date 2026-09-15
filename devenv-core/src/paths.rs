//! On-disk layout for a devenv project.

use miette::{Result, bail, miette};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

pub const DEFAULT_LOCK_FILE: &str = "devenv.lock";

#[derive(Debug, Clone)]
pub struct DevenvPaths {
    pub root: PathBuf,
    pub lock_file: PathBuf,
    pub dotfile: PathBuf,
    pub dot_gc: PathBuf,
    pub home_gc: PathBuf,
    pub tmp: PathBuf,
    pub runtime: PathBuf,
    pub state: Option<PathBuf>,
    pub git_root: Option<PathBuf>,
}

/// Walk up from `start` looking for a directory containing `devenv.nix`.
/// Returns the first ancestor (including `start` itself) that contains it,
/// or `None` if none is found before reaching the filesystem root.
pub fn find_project_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|d| d.join("devenv.nix").exists())
        .map(PathBuf::from)
}

/// Resolve devenv's per-user data directory.
///
/// Used to store GC roots, the trust database, and cached public keys.
///
/// Honors the `DEVENV_HOME` environment variable, otherwise falls back to `$XDG_DATA_HOME`.
pub fn resolve_home() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("DEVENV_HOME").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(home));
    }
    xdg::BaseDirectories::with_prefix("devenv")
        .get_data_home()
        .ok_or_else(|| {
            miette!("Could not determine devenv data directory. Set DEVENV_HOME or HOME.")
        })
}

pub fn resolve_user_config_file() -> Option<PathBuf> {
    xdg::BaseDirectories::with_prefix("devenv").get_config_file("config.yaml")
}

/// Resolve the per-project runtime directory that holds sockets and other
/// short-lived runtime files.
///
/// The default is kept short to stay within the unix-domain-socket path length
/// limit. macOS gives `sun_path` 104 bytes including the terminator, which is
/// the tighter of the two platforms devenv supports.
///
/// Derives a short, deterministic `devenv-<hash>` path
/// under `$XDG_RUNTIME_DIR` (falling back to `/tmp`) that is unique to
/// `devenv_dotfile`. `$TMPDIR` is deliberately ignored because it may differ
/// between invocations that need to rendezvous on the same runtime directory.
///
/// # What belongs here, and what does not
///
/// This directory lasts as long as the login session, and no longer. The XDG
/// Base Directory specification is explicit: "if the user fully logs out the
/// directory MUST be removed", and "files in the directory MUST not survive
/// reboot or a full logout/login cycle". systemd implements exactly that, so
/// `/run/user/$UID` disappears when the last session ends. The fallback is no
/// safer over time: `/tmp` is aged by `systemd-tmpfiles` on Linux and by
/// `com.apple.tmp_cleaner` on macOS.
///
/// So this directory is right for anything scoped to the session, and for
/// secrets that should not outlive it — the managed netrc holds a Cachix
/// token, and 0700 under a per-user base is where it belongs.
///
/// It is wrong for anything that identifies work meant to outlive the
/// terminal. A detached process manager is defined by outliving it, so the
/// state that names one lives in `.devenv` instead. Losing this directory then
/// costs the client its socket, not the manager itself: devenv can still find
/// the manager and stop it. See `devenv_processes::ExternalManager`.
pub fn resolve_runtime_dir(devenv_dotfile: &Path) -> PathBuf {
    resolve_runtime_dir_with(devenv_dotfile, |name| std::env::var_os(name))
}

fn resolve_runtime_dir_with(
    devenv_dotfile: &Path,
    get_env: impl Fn(&str) -> Option<OsString>,
) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(devenv_dotfile.to_string_lossy().as_bytes());
    let hex = hex::encode(hasher.finalize());

    let runtime_base = get_env("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    runtime_base.join(format!("devenv-{}", &hex[..7]))
}

/// Create the runtime directory resolved by [`resolve_runtime_dir`], with
/// access limited to its owner.
///
/// The directory holds sockets and the managed netrc, which carries the
/// Cachix auth token plus a copy of Nix's global netrc. Its path is a hash
/// of the project path under a base that is often shared between users
/// (`/tmp`, whenever `$XDG_RUNTIME_DIR` is unset), so on a multi-user host
/// another local user can predict it and get there first. Refusing a
/// directory somebody else owns, and keeping the mode at 0700, stops them
/// from planting symlinks for devenv to write secrets through.
pub fn create_runtime_dir(runtime_dir: &Path) -> Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(runtime_dir)
        .map_err(|e| miette!("Failed to create {}: {}", runtime_dir.display(), e))?;

    // Creating a directory that already exists succeeds without touching
    // it, so the mode above says nothing about the one we ended up with.
    let metadata = std::fs::symlink_metadata(runtime_dir)
        .map_err(|e| miette!("Failed to stat {}: {}", runtime_dir.display(), e))?;
    if !metadata.is_dir() {
        bail!(
            "Runtime directory {} is a symlink or not a directory. \
             Remove it, or point $XDG_RUNTIME_DIR elsewhere.",
            runtime_dir.display()
        );
    }
    let uid = nix::unistd::getuid().as_raw();
    if metadata.uid() != uid {
        bail!(
            "Runtime directory {} is owned by uid {}, not by uid {}. \
             Remove it, or point $XDG_RUNTIME_DIR elsewhere.",
            runtime_dir.display(),
            metadata.uid(),
            uid
        );
    }
    // Tighten directories left behind by older devenv versions, which
    // created this one with the default mode.
    if metadata.mode() & 0o077 != 0 {
        std::fs::set_permissions(runtime_dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| miette!("Failed to secure {}: {}", runtime_dir.display(), e))?;
    }
    Ok(())
}

/// Resolve `path` against `base`: relative paths join onto `base`, absolute
/// paths pass through unchanged. Does not canonicalize; callers decide their
/// own canonicalization and error policy.
pub fn resolve_against(path: &Path, base: &Path) -> PathBuf {
    if path.is_relative() {
        base.join(path)
    } else {
        path.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_marker_in_start_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("devenv.nix"), "").unwrap();
        assert_eq!(find_project_root(tmp.path()).as_deref(), Some(tmp.path()));
    }

    #[test]
    fn walks_up_to_parent() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("devenv.nix"), "").unwrap();
        let nested = tmp.path().join("a/b/c");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(find_project_root(&nested).as_deref(), Some(tmp.path()));
    }

    #[test]
    fn returns_none_when_no_marker() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(find_project_root(tmp.path()).is_none());
    }

    /// Set `DEVENV_HOME` for a test. Safe because cargo nextest runs each test
    /// in its own process, so there is no concurrent env access.
    fn set_devenv_home(dir: &Path) {
        unsafe { std::env::set_var("DEVENV_HOME", dir) };
    }

    fn unset_devenv_home() {
        unsafe { std::env::remove_var("DEVENV_HOME") };
    }

    #[test]
    fn resolve_home_honors_env_override() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("custom-home");
        set_devenv_home(&home);

        assert_eq!(resolve_home().unwrap(), home);

        unset_devenv_home();
    }

    #[test]
    fn resolve_home_empty_env_falls_back_to_xdg() {
        // An empty DEVENV_HOME is treated as unset.
        set_devenv_home(Path::new(""));

        let expected = xdg::BaseDirectories::with_prefix("devenv").get_data_home();
        assert_eq!(resolve_home().ok(), expected);

        unset_devenv_home();
    }

    #[test]
    fn resolve_home_unset_falls_back_to_xdg() {
        unset_devenv_home();

        let expected = xdg::BaseDirectories::with_prefix("devenv").get_data_home();
        assert_eq!(resolve_home().ok(), expected);
    }

    fn resolve_runtime_dir_for_test(devenv_dotfile: &Path, vars: &[(&str, &str)]) -> PathBuf {
        let vars = vars
            .iter()
            .map(|(name, value)| ((*name).to_owned(), OsString::from(value)))
            .collect::<std::collections::HashMap<_, _>>();
        resolve_runtime_dir_with(devenv_dotfile, |name| vars.get(name).cloned())
    }

    #[test]
    fn resolve_runtime_dir_ignores_devenv_runtime() {
        let dir = resolve_runtime_dir_for_test(
            Path::new("/project/.devenv"),
            &[
                ("DEVENV_RUNTIME", "/tmp/custom-runtime"),
                ("XDG_RUNTIME_DIR", "/run/user/1234"),
            ],
        );
        assert_eq!(dir.parent(), Some(Path::new("/run/user/1234")));
    }

    #[test]
    fn resolve_runtime_dir_is_independent_of_tmpdir() {
        let dotfile = Path::new("/project/.devenv");
        let a = resolve_runtime_dir_for_test(dotfile, &[("TMPDIR", "/tmp/dvA")]);
        let b = resolve_runtime_dir_for_test(dotfile, &[("TMPDIR", "/tmp/claude-501")]);
        assert_eq!(a, b);
        assert_eq!(a.parent(), Some(Path::new("/tmp")));
    }

    #[test]
    fn resolve_runtime_dir_uses_xdg_runtime_dir() {
        let dir = resolve_runtime_dir_for_test(
            Path::new("/project/.devenv"),
            &[("XDG_RUNTIME_DIR", "/run/user/1234")],
        );
        assert_eq!(dir.parent(), Some(Path::new("/run/user/1234")));
    }

    #[test]
    fn resolve_runtime_dir_does_not_validate_xdg_runtime_dir() {
        let dir = resolve_runtime_dir_for_test(
            Path::new("/project/.devenv"),
            &[("XDG_RUNTIME_DIR", "relative/runtime")],
        );
        assert_eq!(dir.parent(), Some(Path::new("relative/runtime")));
    }

    #[test]
    fn resolve_runtime_dir_empty_env_falls_back_to_hash() {
        let dir =
            resolve_runtime_dir_for_test(Path::new("/project/.devenv"), &[("XDG_RUNTIME_DIR", "")]);
        let name = dir.file_name().unwrap().to_string_lossy();
        assert!(
            name.starts_with("devenv-"),
            "unexpected runtime dir: {dir:?}"
        );
        assert_eq!(name.len(), "devenv-".len() + 7);
    }

    #[test]
    fn resolve_runtime_dir_is_deterministic_per_dotfile() {
        let a = resolve_runtime_dir_for_test(Path::new("/project/.devenv"), &[]);
        let b = resolve_runtime_dir_for_test(Path::new("/project/.devenv"), &[]);
        let c = resolve_runtime_dir_for_test(Path::new("/other/.devenv"), &[]);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn resolve_runtime_dir_keeps_profiles_isolated() {
        let base = resolve_runtime_dir_for_test(Path::new("/project/.devenv"), &[]);
        let profile =
            resolve_runtime_dir_for_test(Path::new("/project/.devenv/profiles/prod"), &[]);
        assert_ne!(base, profile);
    }

    fn mode_of(path: &Path) -> u32 {
        std::fs::symlink_metadata(path).unwrap().mode() & 0o777
    }

    #[test]
    fn create_runtime_dir_is_owner_only() {
        let tmp = tempfile::tempdir().unwrap();
        let runtime = tmp.path().join("base/devenv-abc1234");

        create_runtime_dir(&runtime).unwrap();

        assert_eq!(mode_of(&runtime), 0o700);
    }

    /// Older devenv versions created this directory with the default mode,
    /// which lets another local user plant symlinks for the managed netrc
    /// to be written through.
    #[test]
    fn create_runtime_dir_tightens_a_world_readable_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let runtime = tmp.path().join("devenv-abc1234");
        std::fs::create_dir(&runtime).unwrap();
        std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o755)).unwrap();

        create_runtime_dir(&runtime).unwrap();

        assert_eq!(mode_of(&runtime), 0o700);
    }

    /// The path is predictable, so somebody else can get there first.
    #[test]
    fn create_runtime_dir_rejects_a_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let elsewhere = tmp.path().join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        let runtime = tmp.path().join("devenv-abc1234");
        std::os::unix::fs::symlink(&elsewhere, &runtime).unwrap();

        let error = create_runtime_dir(&runtime).unwrap_err().to_string();

        assert!(error.contains("symlink"), "{error}");
    }

    #[test]
    fn resolve_against_joins_relative_and_passes_absolute() {
        let base = Path::new("/base");
        assert_eq!(
            resolve_against(Path::new("sub/dir"), base),
            PathBuf::from("/base/sub/dir")
        );
        assert_eq!(
            resolve_against(Path::new("/abs/dir"), base),
            PathBuf::from("/abs/dir")
        );
    }
}
