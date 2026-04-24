//! Resolved Nix store settings handed to a backend at init time.
//!
//! `StoreSettings` is a value type: anything that can produce substituter
//! URLs, trusted public keys, and an optional netrc path can build one.
//! Today the producer is `CachixManager`; future producers might parse
//! `nix.conf`, read environment overrides, or be hand-built in tests.
//!
//! Lists are typed (`Vec<String>`), not pre-joined whitespace strings,
//! so consumers that want to inspect, dedupe, or merge entries don't
//! have to re-parse. The FFI backend joins on whitespace at the
//! `nix.conf` boundary; that's a one-line conversion.

use std::path::PathBuf;

/// Owned, `Send` snapshot of resolved Nix store settings.
///
/// Held briefly at backend `init()` time, applied to the store, then
/// dropped — the backend does not retain it.
#[derive(Clone, Debug, Default)]
pub struct StoreSettings {
    /// Substituter URLs to register in addition to whatever is already
    /// configured globally (corresponds to Nix's `extra-substituters`).
    pub extra_substituters: Vec<String>,
    /// Trusted public signing keys to register in addition to globally
    /// configured ones (corresponds to `extra-trusted-public-keys`).
    pub extra_trusted_public_keys: Vec<String>,
    /// Path to a netrc file for substituter authentication, if any.
    pub netrc_path: Option<PathBuf>,
}

impl StoreSettings {
    /// Returns true when no settings would change the store's behavior.
    pub fn is_empty(&self) -> bool {
        self.extra_substituters.is_empty()
            && self.extra_trusted_public_keys.is_empty()
            && self.netrc_path.is_none()
    }
}
