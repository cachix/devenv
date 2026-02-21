//! Core types and traits for devenv
//!
//! This crate contains the core abstractions and types that are shared between
//! different parts of devenv, including backend implementations.

pub mod cachix;
pub mod cli;
pub mod config;
pub mod eval_op;
pub mod internal_log;
pub mod nix_args;
pub mod nix_backend;
pub mod nix_log_bridge;
pub mod ports;
pub mod resource;

pub use cachix::{CachixCacheInfo, CachixManager, CachixPaths};
pub use cli::{GlobalOptions, NixBuildDefaults, TraceFormat, default_system};
pub use config::{Config, SecretSettings, ShellSettings};
pub use eval_op::{EvalOp, OpObserver};
pub use internal_log::{ActivityType, Field, InternalLog, ResultType, Verbosity};
pub use nix_args::{CliOptionsConfig, NixArgs, SecretspecData};
pub use nix_backend::{
    DevEnvOutput, DevenvPaths, NixBackend, Options, PackageSearchResult, SearchResults,
};
pub use ports::{PortAllocation, PortAllocator, PortSpec};
pub use resource::{ReplayError, ReplayableResource};
