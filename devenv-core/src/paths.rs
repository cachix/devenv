//! On-disk layout for a devenv project.

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
}
