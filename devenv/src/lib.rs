pub mod changelog;
pub mod cli;
mod devenv;
pub mod log;
pub mod mcp;
pub(crate) mod nix;
pub mod nix_log_bridge;
mod tracing;
mod util;

#[cfg(feature = "snix")]
pub use devenv_snix_backend;

pub use devenv::{DIRENVRC, DIRENVRC_VERSION, Devenv, DevenvOptions, ProcessOptions};
pub use devenv_tasks as tasks;

// Re-export core types from devenv-core for convenience
pub use devenv_core::{
    CachixCacheInfo, CachixManager, CachixPaths, Config, DevenvPaths, GlobalOptions, NixArgs,
    NixBackend, Options, SecretspecData, default_system,
};

/// Result of a devenv command execution.
/// Some commands need to exec into a new process after cleanup.
#[derive(Debug)]
pub enum CommandResult<T = ()> {
    /// Command completed normally with a value
    Done(T),
    /// Exec into this command after cleanup (TUI shutdown, terminal restore)
    Exec(std::process::Command),
}

impl<T> CommandResult<T> {
    /// Transform the inner value, preserving Exec.
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> CommandResult<U> {
        match self {
            CommandResult::Done(v) => CommandResult::Done(f(v)),
            CommandResult::Exec(cmd) => CommandResult::Exec(cmd),
        }
    }

    /// Discard the value, converting to CommandResult<()>.
    pub fn discard(self) -> CommandResult<()> {
        self.map(|_| ())
    }

    /// Unwrap the Done value. Panics if this is an Exec variant.
    pub fn unwrap(self) -> T {
        match self {
            CommandResult::Done(v) => v,
            CommandResult::Exec(_) => panic!("called unwrap on CommandResult::Exec"),
        }
    }
}

impl CommandResult<()> {
    /// Create a Done result with unit value.
    pub fn done() -> CommandResult<()> {
        CommandResult::Done(())
    }

    /// Execute the pending command if any, replacing the current process.
    /// Returns Ok(()) if there's nothing to exec (Done variant).
    /// Never returns on successful exec.
    pub fn exec(self) -> miette::Result<()> {
        match self {
            CommandResult::Done(()) => Ok(()),
            CommandResult::Exec(mut cmd) => {
                use std::os::unix::process::CommandExt;

                let err = cmd.exec();
                miette::bail!("Failed to exec: {}", err);
            }
        }
    }
}
