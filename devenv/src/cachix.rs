//! Cachix binary cache integration for devenv.
//!
//! This module handles fetching and configuring Cachix substituters and trusted keys
//! for Nix operations, including authentication token management and API integration.

use miette::{IntoDiagnostic, Result, WrapErr};
use nix_conf_parser::NixConf;
use serde::Deserialize;
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

    /// Get Nix settings (--option flags) needed for Cachix substituters
    ///
    /// Returns a HashMap where keys are Nix option names and values are the option values.
    /// For example: "extra-substituters" => "https://cache1.cachix.org https://cache2.cachix.org"
    pub async fn get_nix_settings(
        &self,
        cachix_caches: &CachixCaches,
    ) -> Result<HashMap<String, String>> {
        let mut settings = HashMap::new();

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

            // Configure netrc file with auth token if available
            if let Ok(auth_token) = env::var("CACHIX_AUTH_TOKEN") {
                let netrc_path = self
                    .netrc_path
                    .get_or_try_init(|| async {
                        let netrc_path = self.paths.netrc.clone();
                        let netrc_path_str = netrc_path.to_string_lossy().to_string();

                        self.create_netrc_file(
                            &netrc_path,
                            &cachix_caches.caches.pull,
                            &auth_token,
                        )
                        .await?;

                        Ok::<String, miette::Report>(netrc_path_str)
                    })
                    .await;

                match netrc_path {
                    Ok(path) => {
                        settings.insert("netrc-file".to_string(), path.to_string());
                    }
                    Err(e) => {
                        warn!("{e}");
                    }
                }
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

/// Cached Cachix caches with their trusted keys
#[derive(Deserialize, Default, Clone)]
pub struct CachixCaches {
    pub caches: Cachix,
    pub known_keys: BTreeMap<String, String>,
}

/// Response from Cachix API for cache information
#[derive(Deserialize, Clone)]
pub struct CachixResponse {
    #[serde(rename = "publicSigningKeys")]
    pub public_signing_keys: Vec<String>,
}

/// Response from `nix store ping` command
#[derive(Deserialize, Clone)]
pub struct StorePing {
    pub trusted: Option<u8>,
}

/// Detect which caches and public keys are missing from Nix configuration
pub fn detect_missing_caches(
    caches: &CachixCaches,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trusted() {
        let store_ping = r#"{"trusted":1,"url":"daemon","version":"2.18.1"}"#;
        let store_ping: StorePing = serde_json::from_str(store_ping).unwrap();
        assert_eq!(store_ping.trusted, Some(1));
    }

    #[test]
    fn test_no_trusted() {
        let store_ping = r#"{"url":"daemon","version":"2.18.1"}"#;
        let store_ping: StorePing = serde_json::from_str(store_ping).unwrap();
        assert_eq!(store_ping.trusted, None);
    }

    #[test]
    fn test_not_trusted() {
        let store_ping = r#"{"trusted":0,"url":"daemon","version":"2.18.1"}"#;
        let store_ping: StorePing = serde_json::from_str(store_ping).unwrap();
        assert_eq!(store_ping.trusted, Some(0));
    }

    #[test]
    fn test_missing_substituters() {
        let mut cachix = CachixCaches::default();
        cachix.caches.pull = vec!["cache1".to_string(), "cache2".to_string()];
        cachix
            .known_keys
            .insert("cache1".to_string(), "key1".to_string());
        cachix
            .known_keys
            .insert("cache2".to_string(), "key2".to_string());
        let nix_conf = NixConf::parse_stdout(
            r#"
              substituters = https://cache1.cachix.org https://cache3.cachix.org
              trusted-public-keys = key1 key3
            "#
            .as_bytes(),
        )
        .expect("Failed to parse NixConf");
        assert_eq!(
            detect_missing_caches(&cachix, nix_conf),
            (
                vec!["https://cache2.cachix.org".to_string()],
                vec!["key2".to_string()]
            )
        );
    }

    #[test]
    fn test_extra_missing_substituters() {
        let mut cachix = CachixCaches::default();
        cachix.caches.pull = vec!["cache1".to_string(), "cache2".to_string()];
        cachix
            .known_keys
            .insert("cache1".to_string(), "key1".to_string());
        cachix
            .known_keys
            .insert("cache2".to_string(), "key2".to_string());
        let nix_conf = NixConf::parse_stdout(
            r#"
              extra-substituters = https://cache1.cachix.org https://cache3.cachix.org
              extra-trusted-public-keys = key1 key3
            "#
            .as_bytes(),
        )
        .expect("Failed to parse NixConf");
        assert_eq!(
            detect_missing_caches(&cachix, nix_conf),
            (
                vec!["https://cache2.cachix.org".to_string()],
                vec!["key2".to_string()]
            )
        );
    }
}
