//! On-disk layout for a devenv project.

use miette::{miette, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DevenvPaths {
    pub root: PathBuf,
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

/// Resolve devenv's per-user data directory ("devenv home").
///
/// Honors the `DEVENV_HOME` environment variable (an empty value is treated as
/// unset), otherwise falls back to the XDG data home (`~/.local/share/devenv`,
/// respecting `$XDG_DATA_HOME`). This is the single source of truth for where
/// GC roots, the trust database, and cached Cachix keys live.
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
}
