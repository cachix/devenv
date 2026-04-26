//! Core types and traits for devenv
//!
//! This crate contains the core abstractions and types that are shared between
//! different parts of devenv, including backend implementations.

pub mod backend;
pub mod bootstrap_args;
pub mod cachix;
pub mod config;
pub mod eval_op;
pub mod evaluator;
pub mod internal_log;
pub mod nix_args;
pub mod nix_backend;
pub mod nix_config;
pub mod nix_log_bridge;
pub mod ports;
pub mod realized;
pub mod resource;
pub mod settings;
pub mod store;
pub mod store_settings;

pub use backend::Backend;
pub use bootstrap_args::BootstrapArgs;
pub use cachix::{CachixCacheInfo, CachixManager, CachixPaths};
pub use config::Config;
pub use eval_op::{EvalOp, OpObserver};
pub use evaluator::{
    BuildOptions, DevEnvOutput, Evaluator, NixMetadata, PackageSearchResult, SearchResults,
};
pub use internal_log::{ActivityType, Field, InternalLog, ResultType, Verbosity};
pub use nix_args::{CliOptionsConfig, NixArgs, SecretspecData};
pub use nix_backend::DevenvPaths;
pub use nix_config::NixConfig;
pub use ports::{PortAllocation, PortAllocator, PortSpec};
pub use realized::RealizedPathsObserver;
pub use resource::{ReplayError, ReplayableResource};
pub use settings::{
    CacheOptions, CacheSettings, InputOverrides, NixOptions, NixSettings, SecretOptions,
    SecretSettings, ShellOptions, ShellSettings, default_system, flag,
};
pub use store::{GcOptions, GcStats, PathInfo, Store, StorePath};
pub use store_settings::StoreSettings;
