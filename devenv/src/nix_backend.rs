//! Abstraction layer for different Nix evaluation backends.
//!
//! This module defines a trait that allows devenv to use different Nix implementations,
//! such as the traditional C++ Nix binary or alternative implementations like Snix.

use async_trait::async_trait;
use devenv_eval_cache::Output;
use miette::Result;
use std::path::{Path, PathBuf};

/// Common paths used by devenv backends
#[derive(Debug, Clone)]
pub struct DevenvPaths {
    pub root: PathBuf,
    pub dotfile: PathBuf,
    pub dot_gc: PathBuf,
    pub home_gc: PathBuf,
    pub cachix_trusted_keys: PathBuf,
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
    /// Enable logging.
    pub logging: bool,
    /// Log the stdout of the command.
    pub logging_stdout: bool,
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
            logging: true,
            logging_stdout: false,
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
pub trait NixBackend {
    /// Initialize and assemble the backend (e.g., set up database connections)
    async fn assemble(&mut self) -> Result<()>;

    /// Get the development environment
    async fn dev_env(&self, json: bool, gc_root: &Path) -> Result<Output>;

    /// Add a garbage collection root
    async fn add_gc(&self, name: &str, path: &Path) -> Result<()>;

    /// Open a Nix REPL
    fn repl(&self) -> Result<()>;

    /// Build the specified attributes
    async fn build(&self, attributes: &[&str], options: Option<Options>) -> Result<Vec<PathBuf>>;

    /// Evaluate a Nix expression
    async fn eval(&self, attributes: &[&str]) -> Result<String>;

    /// Update flake inputs
    async fn update(&self, input_name: &Option<String>) -> Result<()>;

    /// Get flake metadata
    async fn metadata(&self) -> Result<String>;

    /// Search for packages
    async fn search(&self, name: &str, options: Option<Options>) -> Result<Output>;

    /// Garbage collect the specified paths
    fn gc(&self, paths: Vec<PathBuf>) -> Result<()>;

    /// Get the backend name (for debugging/logging)
    fn name(&self) -> &'static str;

    /// Run a nix command
    async fn run_nix(&self, command: &str, args: &[&str], options: &Options) -> Result<Output>;
}
