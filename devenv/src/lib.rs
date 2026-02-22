pub mod changelog;
pub mod cli;
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
    DIRENVRC, DIRENVRC_VERSION, Devenv, DevenvConfig, DevenvOptions, ProcessOptions, RunMode,
    SecretsNeedPrompting, ShellCommand, init,
};
pub use devenv_tasks as tasks;

// Re-export core types from devenv-core for convenience
pub use devenv_core::{
    CachixCacheInfo, CachixManager, CachixPaths, Config, DevenvPaths, NixArgs, NixBackend,
    NixSettings, Options, SecretspecData, default_system,
};

/// Returns true if this binary was NOT built from a release tag.
///
/// Uses build-time info from build.rs:
/// - `DEVENV_ON_RELEASE_TAG`: "true" when HEAD is on an exact tag (cargo builds with .git)
/// - `DEVENV_IS_RELEASE`: "1" for Nix release builds (set in package.nix, .git unavailable)
pub fn is_development_version() -> bool {
    if env!("DEVENV_ON_RELEASE_TAG") == "true" {
        return false;
    }
    if option_env!("DEVENV_IS_RELEASE") == Some("1") {
        return false;
    }
    true
}
