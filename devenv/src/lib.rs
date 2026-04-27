// SecretsNeedPrompting fields trigger unused_assignments due to cross-crate usage (rustc 1.93)
#![allow(unused_assignments)]

pub mod backend;
pub mod changelog;
pub mod cli;
pub mod commands;
pub mod console;
mod devenv;
pub mod lsp;
pub mod mcp;
pub mod nix_log_bridge;
pub mod reload;
pub(crate) mod shell_env;
pub mod tracing;
pub use devenv_processes as processes;
mod util;

#[cfg(feature = "snix")]
pub use devenv_snix_backend;

pub use devenv::{
    DIRENVRC, DIRENVRC_VERSION, Devenv, DevenvOptions, ProcessOptions, RunMode,
    SecretsNeedPrompting, ShellCommand, format_shell_exports,
};
pub use devenv_tasks as tasks;

// Re-export core types from devenv-core for convenience
pub use devenv_core::{
    Backend, BuildOptions, CachixCacheInfo, CachixManager, CachixPaths, Config, DevenvPaths,
    Evaluator, NixArgs, NixSettings, SecretOptions, SecretSettings, SecretspecData, default_system,
};

/// Returns true if this binary was NOT built from a release.
///
/// DEVENV_IS_RELEASE is set by build.rs: either from the DEVENV_IS_RELEASE
/// env var (flake/CI builds) or auto-detected via git tag (local builds).
pub fn is_development_version() -> bool {
    !matches!(env!("DEVENV_IS_RELEASE"), "true" | "1")
}
