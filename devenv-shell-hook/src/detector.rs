use miette::Result;
use std::path::{Path, PathBuf};

pub fn find_devenv_root(start_dir: &Path) -> Result<Option<PathBuf>> {
    let mut current = start_dir;

    loop {
        if current.join("devenv.nix").exists() || current.join("devenv.yaml").exists() {
            return Ok(Some(current.to_path_buf()));
        }

        match current.parent() {
            Some(parent) => current = parent,
            None => return Ok(None),
        }
    }
}
