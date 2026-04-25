//! Runtime configuration for a Nix backend.

use std::collections::BTreeMap;

use crate::config::{Input, NixpkgsConfig};
use crate::nix_backend::DevenvPaths;
use crate::settings::{CacheSettings, InputOverrides, NixSettings};

/// Runtime configuration the backend reads to evaluate Nix:
/// filesystem layout, flake inputs and overrides, FFI store and eval
/// settings, and the nixpkgs config to write at startup.
#[derive(Clone, Debug)]
pub struct NixConfig {
    pub paths: DevenvPaths,
    pub inputs: BTreeMap<String, Input>,
    pub input_overrides: InputOverrides,
    pub nix: NixSettings,
    pub cache: CacheSettings,
    pub nixpkgs: NixpkgsConfig,
}
