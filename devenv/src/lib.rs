pub mod changelog;
pub mod cli;
mod devenv;
pub mod mcp;
pub mod nix_log_bridge;
pub mod tracing;
mod util;

#[cfg(feature = "snix")]
pub use devenv_snix_backend;

pub use devenv::{
    DIRENVRC, DIRENVRC_VERSION, Devenv, DevenvOptions, ProcessOptions, RunMode,
    SecretsNeedPrompting, ShellCommand,
};
pub use devenv_tasks as tasks;

// Re-export core types from devenv-core for convenience
pub use devenv_core::{
    CachixCacheInfo, CachixManager, CachixPaths, Config, DevenvPaths, GlobalOptions, NixArgs,
    NixBackend, Options, SecretspecData, default_system,
};
