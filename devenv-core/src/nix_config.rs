//! Consolidated configuration consumed by Nix backends.
//!
//! `NixConfig` is built once on `Devenv::new` and shared via `Arc<NixConfig>`
//! between the framework and the backend. It is immutable for the lifetime
//! of `Devenv`; the only "hot-reload" devenv supports today is
//! `NixBackend::reload_eval_state`, which clears the backend's eval caches
//! in place against the same config.

use std::collections::BTreeMap;

use crate::config::{Input, NixpkgsConfig};
use crate::nix_backend::DevenvPaths;
use crate::settings::{CacheSettings, InputOverrides, NixSettings};

/// Aggregated, owned configuration for a Nix backend.
///
/// Pub fields with `#[non_exhaustive]` follow the convention established by
/// `DevenvPaths`. External crates can read fields directly but cannot
/// construct the struct via a struct literal — use `NixConfig::new()`.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct NixConfig {
    pub paths: DevenvPaths,
    pub inputs: BTreeMap<String, Input>,
    pub imports: Box<[String]>,
    pub input_overrides: InputOverrides,
    pub nix: NixSettings,
    pub cache: CacheSettings,
    pub nixpkgs: NixpkgsConfig,
    pub active_profiles: Box<[String]>,
    pub container_name: Option<Box<str>>,
    pub from_external: bool,
    pub is_testing: bool,
    pub require_version_match: bool,
}

impl NixConfig {
    /// Construct a `NixConfig` from its parts.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        paths: DevenvPaths,
        inputs: BTreeMap<String, Input>,
        imports: Vec<String>,
        input_overrides: InputOverrides,
        nix: NixSettings,
        cache: CacheSettings,
        nixpkgs: NixpkgsConfig,
        active_profiles: Vec<String>,
        container_name: Option<String>,
        from_external: bool,
        is_testing: bool,
        require_version_match: bool,
    ) -> Self {
        Self {
            paths,
            inputs,
            imports: imports.into_boxed_slice(),
            input_overrides,
            nix,
            cache,
            nixpkgs,
            active_profiles: active_profiles.into_boxed_slice(),
            container_name: container_name.map(Into::into),
            from_external,
            is_testing,
            require_version_match,
        }
    }
}
