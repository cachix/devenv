//! Project paths and the eval-cache key helper used across backends.

use std::path::PathBuf;

pub use crate::evaluator::{DevEnvOutput, PackageSearchResult, SearchResults};

pub fn eval_cache_key_args(
    nix_args_str: &str,
    port_allocation_enabled: bool,
    strict_ports: bool,
) -> String {
    format!("{nix_args_str}:port_allocation={port_allocation_enabled}:strict_ports={strict_ports}")
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_cache_key_args_includes_port_flags() {
        let key = eval_cache_key_args("{ foo = 1; }", true, false);
        assert!(key.contains("port_allocation=true"));
        assert!(key.contains("strict_ports=false"));
    }
}
