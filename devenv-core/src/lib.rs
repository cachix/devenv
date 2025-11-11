//! Core types and traits for devenv
//!
//! This crate contains the core abstractions and types that are shared between
//! different parts of devenv, including backend implementations.

pub mod cachix;
pub mod cli;
pub mod config;
pub mod nix_args;
pub mod nix_backend;

pub use cachix::{CachixCacheInfo, CachixManager, CachixPaths};
pub use cli::{GlobalOptions, LogFormat, NixBuildDefaults, default_system};
pub use config::Config;
pub use nix_args::{NixArgs, SecretspecData};
pub use nix_backend::{DevenvPaths, NixBackend, Options};
