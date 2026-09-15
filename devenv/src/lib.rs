// SecretsNeedPrompting fields trigger unused_assignments due to cross-crate usage (rustc 1.93)
#![allow(unused_assignments)]

pub mod backend;
pub mod changelog;
pub mod cli;
pub mod console;
mod devenv;
pub mod lsp;
pub mod mcp;
pub mod metadata;
pub mod nix_log_bridge;
mod proxy;
pub mod reload;
pub(crate) mod shell_env;
pub mod terminal;
pub mod tracing;
pub mod user_config;
pub use devenv_processes as processes;
mod util;

pub use devenv::{
    ClientRunMode, DIRENVRC, DIRENVRC_VERSION, Devenv, DevenvOptions, ProcessOptions,
    ProcessStartOutcome, SecretsNeedPrompting, SecretsPromptSource, ShellCommand,
    format_shell_exports, is_ai_agent, load_cachix_secretspec,
};
pub use devenv_tasks as tasks;
pub use metadata::{InfoSections, InputAttribute, InputMetadata, InputSource, Metadata};

// Re-export common subsystem crates for convenience.
pub use devenv_activity as activity;
pub use devenv_tui as tui;
pub use tokio_shutdown;

// Re-export core types from devenv-core for convenience
pub use devenv_core::{
    Backend, BuildOptions, CacheSettings, CachixCacheInfo, CachixManager, CachixPaths, Config,
    DevenvPaths, Evaluator, InputOverrides, NixArgs, NixSettings, SecretOptions, SecretSettings,
    SecretspecData, ShellSettings, VerbosityLevel, config, default_system,
};

/// Returns true if this binary was NOT built from a release.
///
/// DEVENV_IS_RELEASE is set by build.rs: either from the DEVENV_IS_RELEASE
/// env var (flake/CI builds) or auto-detected via git tag (local builds).
pub fn is_development_version() -> bool {
    !matches!(env!("DEVENV_IS_RELEASE"), "true" | "1")
}
