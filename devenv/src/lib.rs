pub mod cli;
pub mod config;
mod devenv;
pub mod log;
pub mod mcp;
pub(crate) mod nix;
pub mod nix_backend;
pub mod nix_log_bridge;

#[cfg(feature = "snix")]
pub(crate) mod snix_backend;
mod util;

pub use cli::{GlobalOptions, default_system};
pub use devenv::{DIRENVRC, DIRENVRC_VERSION, Devenv, DevenvOptions, ProcessOptions};
pub use devenv_tasks as tasks;
// Note: op_info and op_debug macros are exported at crate root via #[macro_export]
