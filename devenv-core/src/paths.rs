//! On-disk layout for a devenv project.

use std::path::PathBuf;

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
