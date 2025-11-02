//! Arguments passed to the devenv flake template
//!
//! This module defines the structure for arguments passed to the flake template
//! when assembling the devenv environment. The struct is serialized to Nix syntax
//! using the `ser_nix` crate and inserted into the flake template.

use serde::Serialize;
use std::path::Path;

/// Arguments passed to Nix when assembling the environment
#[derive(Debug, Clone, Serialize)]
pub struct NixArgs<'a> {
    /// The devenv CLI version (e.g., "1.10.1")
    pub version: &'a str,

    /// The system string (e.g., "x86_64-linux", "aarch64-darwin")
    pub system: &'a str,

    /// Absolute path to the project root (location of devenv.nix)
    /// Serialized as a Nix path literal by ser_nix
    pub devenv_root: &'a Path,

    /// Absolute path to the devenv dotfile directory (.devenv)
    /// Serialized as a Nix path literal by ser_nix
    pub devenv_dotfile: &'a Path,

    /// Relative Nix path to the dotfile directory (e.g., ".devenv")
    /// Serialized as a Nix path literal by ser_nix
    pub devenv_dotfile_path: &'a Path,

    /// Absolute path to the system temporary directory
    /// Serialized as a Nix path literal by ser_nix
    /// TODO: remove in the next release
    pub devenv_tmpdir: &'a Path,

    /// Absolute path to the runtime directory for this shell session
    /// Serialized as a Nix path literal by ser_nix
    pub devenv_runtime: &'a Path,

    /// Whether the environment is being assembled for testing
    pub devenv_istesting: bool,

    /// Latest direnvrc version number available
    pub devenv_direnvrc_latest_version: u8,

    /// Container name if building/running a container, otherwise null
    pub container_name: Option<&'a str>,

    /// List of active profiles to enable
    pub active_profiles: &'a [String],

    /// Current system hostname
    pub hostname: Option<&'a str>,

    /// Current username
    pub username: Option<&'a str>,

    /// Git repository root path, if detected; otherwise null
    pub git_root: Option<&'a Path>,
}
