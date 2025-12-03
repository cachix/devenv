//! Cachix binary cache integration for devenv.
//!
//! This module handles fetching and configuring Cachix substituters and trusted keys
//! for Nix operations, including authentication token management and API integration.

use miette::{IntoDiagnostic, Result, WrapErr};
use nix_conf_parser::NixConf;
use serde::{Deserialize, Deserializer};
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::OnceCell;
use tracing::{debug, warn};

/// Paths specific to Cachix operations
#[derive(Debug, Clone)]
pub struct CachixPaths {
    pub trusted_keys: PathBuf,
    pub netrc: PathBuf,
    /// Optional custom daemon socket path (for testing)
    pub daemon_socket: Option<PathBuf>,
}

/// Manages Cachix binary cache configuration and integration
pub struct CachixManager {
    pub paths: CachixPaths,
    netrc_path: Arc<OnceCell<String>>,
}

impl CachixManager {
    /// Create a new CachixManager
    pub fn new(paths: CachixPaths) -> Self {
        Self {
            paths,
            netrc_path: Arc::new(OnceCell::new()),
        }
    }

    /// Get global Nix settings that must be applied BEFORE store creation
    ///
    /// Returns settings like netrc-file path that need to be in place before
    /// the Nix store makes any HTTP requests.
    pub fn get_global_settings(&self) -> Result<HashMap<String, String>> {
        let mut settings = HashMap::new();

        // If CACHIX_AUTH_TOKEN exists, set netrc-file path
        // The actual file will be created later when we know which caches to configure
        if env::var("CACHIX_AUTH_TOKEN").is_ok() {
            let netrc_path_str = self.paths.netrc.to_string_lossy().to_string();
            settings.insert("netrc-file".to_string(), netrc_path_str);
        }

        Ok(settings)
    }

    /// Ensure netrc file is created and populated with cache credentials
    ///
    /// This should be called after we know which caches to configure.
    /// It creates the netrc file with authentication for the given caches.
    pub async fn ensure_netrc_file(&self, pull_caches: &[String]) -> Result<()> {
        if let Ok(auth_token) = env::var("CACHIX_AUTH_TOKEN") {
            // Only create if we haven't already
            if self.netrc_path.get().is_none() {
                let netrc_path = self.paths.netrc.clone();
                self.create_netrc_file(&netrc_path, pull_caches, &auth_token)
                    .await?;

                // Cache that we've created it
                let netrc_path_str = netrc_path.to_string_lossy().to_string();
                let _ = self.netrc_path.set(netrc_path_str);
            }
        }
        Ok(())
    }

    /// Get Nix settings (--option flags) needed for Cachix substituters
    ///
    /// Returns a HashMap where keys are Nix option names and values are the option values.
    /// For example: "extra-substituters" => "https://cache1.cachix.org https://cache2.cachix.org"
    ///
    /// Note: This returns substituters and keys but NOT netrc-file.
    /// Use get_global_settings() to get netrc-file path before store creation.
    pub async fn get_nix_settings(
        &self,
        cachix_caches: &CachixCacheInfo,
    ) -> Result<BTreeMap<String, String>> {
        let mut settings = BTreeMap::new();

        // Configure pull caches (substituters and trusted keys)
        if !cachix_caches.caches.pull.is_empty() {
            let mut pull_caches = cachix_caches
                .caches
                .pull
                .iter()
                .map(|cache| format!("https://{cache}.cachix.org"))
                .collect::<Vec<String>>();
            pull_caches.sort();
            settings.insert("extra-substituters".to_string(), pull_caches.join(" "));

            let mut keys = cachix_caches
                .known_keys
                .values()
                .cloned()
                .collect::<Vec<String>>();
            keys.sort();
            settings.insert("extra-trusted-public-keys".to_string(), keys.join(" "));

            // Ensure netrc file is created with cache credentials
            // (netrc-file path should already be set via get_global_settings())
            if let Err(e) = self.ensure_netrc_file(&cachix_caches.caches.pull).await {
                warn!("Failed to create netrc file: {}", e);
            }
        }

        Ok(settings)
    }

    /// Create a netrc file with Cachix authentication
    async fn create_netrc_file(
        &self,
        netrc_path: &Path,
        pull_caches: &[String],
        auth_token: &str,
    ) -> Result<()> {
        let mut netrc_content = String::new();

        for cache in pull_caches {
            netrc_content.push_str(&format!(
                "machine {cache}.cachix.org\nlogin token\npassword {auth_token}\n\n",
            ));
        }

        // Create netrc file with restrictive permissions (600)
        {
            use tokio::io::AsyncWriteExt;

            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .mode(0o600)
                .open(netrc_path)
                .await
                .into_diagnostic()
                .wrap_err_with(|| {
                    format!("Failed to create netrc file at {}", netrc_path.display())
                })?;

            file.write_all(netrc_content.as_bytes())
                .await
                .into_diagnostic()
                .wrap_err_with(|| {
                    format!("Failed to write netrc content to {}", netrc_path.display())
                })?;
        }

        Ok(())
    }

    /// Clean up the netrc file if it was created during this session
    fn cleanup_netrc(&self) {
        if let Some(netrc_path_str) = self.netrc_path.get() {
            let netrc_path = Path::new(netrc_path_str);
            match std::fs::remove_file(netrc_path) {
                Ok(()) => debug!("Removed netrc file: {}", netrc_path_str),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => warn!("Failed to remove netrc file {}: {}", netrc_path_str, e),
            }
        }
    }
}

impl Drop for CachixManager {
    fn drop(&mut self) {
        self.cleanup_netrc();
    }
}

/// Cachix module configuration (from devenv.config.cachix)
#[derive(Deserialize, Default, Clone)]
pub struct CachixConfig {
    pub enable: bool,
    #[serde(flatten)]
    pub caches: Cachix,
}

/// Cachix cache configuration
#[derive(Deserialize, Default, Clone)]
pub struct Cachix {
    pub pull: Vec<String>,
    pub push: Option<String>,
}

/// Cachix cache information including configuration and public signing keys
#[derive(Deserialize, Default, Clone)]
pub struct CachixCacheInfo {
    pub caches: Cachix,
    pub known_keys: BTreeMap<String, String>,
}

/// Cachix API response containing cache metadata
#[derive(Deserialize, Clone)]
pub struct CacheMetadata {
    #[serde(rename = "publicSigningKeys")]
    pub public_signing_keys: Vec<String>,
}

/// Response from `nix store ping` command
#[derive(Debug, Deserialize, Clone)]
pub struct StorePing {
    /// Whether the current user is trusted by the Nix store (requires Nix 2.4+)
    #[serde(rename = "trusted", deserialize_with = "deserialize_trusted")]
    pub is_trusted: bool,
}

/// Custom deserializer for the `trusted` field that requires it to be present
fn deserialize_trusted<'de, D>(deserializer: D) -> std::result::Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    match Option::<u8>::deserialize(deserializer)? {
        Some(1) => Ok(true),
        Some(0) => Ok(false),
        Some(n) => Err(Error::custom(format!(
            "expected trusted to be 0 or 1, got {}",
            n
        ))),
        None => Err(Error::missing_field(
            "trusted field is missing - upgrade to Nix 2.4 or later",
        )),
    }
}

/// Detect which caches and public keys are missing from Nix configuration
pub fn detect_missing_caches(
    caches: &CachixCacheInfo,
    nix_conf: NixConf,
) -> (Vec<String>, Vec<String>) {
    let mut missing_caches = Vec::new();
    let mut missing_public_keys = Vec::new();

    let substituters = nix_conf
        .get("substituters")
        .map(|s| s.split_whitespace().collect::<Vec<_>>());
    let extra_substituters = nix_conf
        .get("extra-substituters")
        .map(|s| s.split_whitespace().collect::<Vec<_>>());
    let all_substituters = substituters
        .into_iter()
        .flatten()
        .chain(extra_substituters.into_iter().flatten())
        .collect::<Vec<_>>();

    for cache in caches.caches.pull.iter() {
        let cache_url = format!("https://{cache}.cachix.org");
        if !all_substituters.iter().any(|s| s == &cache_url) {
            missing_caches.push(cache_url);
        }
    }

    let trusted_public_keys = nix_conf
        .get("trusted-public-keys")
        .map(|s| s.split_whitespace().collect::<Vec<_>>());
    let extra_trusted_public_keys = nix_conf
        .get("extra-trusted-public-keys")
        .map(|s| s.split_whitespace().collect::<Vec<_>>());
    let all_trusted_public_keys = trusted_public_keys
        .into_iter()
        .flatten()
        .chain(extra_trusted_public_keys.into_iter().flatten())
        .collect::<Vec<_>>();

    for (_name, key) in caches.known_keys.iter() {
        if !all_trusted_public_keys.iter().any(|p| p == key) {
            missing_public_keys.push(key.clone());
        }
    }

    (missing_caches, missing_public_keys)
}
