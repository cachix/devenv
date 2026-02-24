//! Abstraction layer for different Nix evaluation backends.
//!
//! This module defines a trait that allows devenv to use different Nix implementations,
//! such as the traditional C++ Nix binary or alternative implementations like Snix.

use async_trait::async_trait;
use miette::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::Input;
use crate::nix_args::NixArgs;

/// Output of dev_env evaluation.
#[derive(Debug, Clone, Default)]
pub struct DevEnvOutput {
    /// The bash environment script.
    pub bash_env: Vec<u8>,
    /// File paths that the evaluation depends on (for direnv to watch).
    pub inputs: Vec<PathBuf>,
}

/// Package search result from nixpkgs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageSearchResult {
    pub pname: String,
    pub version: String,
    pub description: String,
}

/// Result type for package search operations.
pub type SearchResults = BTreeMap<String, PackageSearchResult>;

/// Common paths used by devenv backends
#[derive(Debug, Clone)]
pub struct DevenvPaths {
    pub root: PathBuf,
    pub dotfile: PathBuf,
    pub dot_gc: PathBuf,
    pub home_gc: PathBuf,
}

/// Options for Nix operations
#[derive(Debug, Clone)]
pub struct Options {
    /// Run `exec` to replace the shell with the command.
    pub replace_shell: bool,
    /// Error out if the command returns a non-zero status code.
    pub bail_on_error: bool,
    /// Cache the output of the command.
    pub cache_output: bool,
    /// Force a refresh of the cached output.
    pub refresh_cached_output: bool,
    /// Extra flags to pass to nix commands.
    pub nix_flags: &'static [&'static str],
}

impl Default for Options {
    fn default() -> Self {
        Self {
            replace_shell: false,
            bail_on_error: true,
            cache_output: false,
            refresh_cached_output: false,
            nix_flags: &[
                "--show-trace",
                "--extra-experimental-features",
                "nix-command",
                "--extra-experimental-features",
                "flakes",
                "--option",
                "lazy-trees",
                "true",
                "--option",
                "warn-dirty",
                "false",
                "--keep-going",
            ],
        }
    }
}

/// Trait defining the interface for Nix evaluation backends
#[async_trait(?Send)]
pub trait NixBackend: Send + Sync {
    /// Compute a content fingerprint from the lock file's inputs' narHashes.
    ///
    /// This is used for eval-cache invalidation when local inputs change.
    /// Unlike the serialized lock file, this includes narHashes for path inputs
    /// which are normally stripped when writing to disk.
    ///
    /// Returns a hex-encoded BLAKE3 hash of the combined narHashes.
    /// Must be called before `assemble()` to include in NixArgs.
    async fn lock_fingerprint(&self) -> Result<String>;

    /// Initialize and assemble the backend
    ///
    /// The args parameter contains all context needed for backend-specific file generation
    async fn assemble(&self, args: &NixArgs<'_>) -> Result<()>;

    /// Get the development environment
    async fn dev_env(&self, json: bool, gc_root: &Path) -> Result<DevEnvOutput>;

    /// Open a Nix REPL
    async fn repl(&self) -> Result<()>;

    /// Build the specified attributes
    async fn build(
        &self,
        attributes: &[&str],
        options: Option<Options>,
        gc_root: Option<&Path>,
    ) -> Result<Vec<PathBuf>>;

    /// Evaluate a Nix expression
    async fn eval(&self, attributes: &[&str]) -> Result<String>;

    /// Update flake inputs
    ///
    /// `override_inputs` contains name/URL pairs (alternating elements) that override
    /// specific inputs during locking, even if the lock file is otherwise up-to-date.
    async fn update(
        &self,
        input_name: &Option<String>,
        inputs: &BTreeMap<String, Input>,
        override_inputs: &[String],
    ) -> Result<()>;

    /// Get flake metadata
    async fn metadata(&self) -> Result<String>;

    /// Search for packages
    async fn search(&self, name: &str, options: Option<Options>) -> Result<SearchResults>;

    /// Garbage collect the specified paths
    /// Returns (paths_deleted, bytes_freed)
    async fn gc(&self, paths: Vec<PathBuf>) -> Result<(u64, u64)>;

    /// Get the backend name (for debugging/logging)
    fn name(&self) -> &'static str;

    /// Get the bash shell executable path
    async fn get_bash(&self, refresh_cached_output: bool) -> Result<String>;

    /// Check if the current user is a trusted user of the Nix store
    async fn is_trusted_user(&self) -> Result<bool>;

    /// Invalidate cached state for hot-reload.
    ///
    /// This clears any cached evaluation state to force re-evaluation on the next operation.
    /// Used by hot-reload to ensure file changes are picked up.
    fn invalidate(&self);
}
