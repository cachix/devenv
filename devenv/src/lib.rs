pub mod cli;
pub mod config;
mod devenv;
pub mod log;
pub mod mcp;
pub(crate) mod nix;
pub mod nix_backend;
#[cfg(feature = "snix")]
pub(crate) mod snix_backend;
mod util;

pub use cli::{GlobalOptions, default_system};
pub use devenv::{DIRENVRC, DIRENVRC_VERSION, Devenv, DevenvOptions, ProcessOptions};
pub use devenv_tasks as tasks;
