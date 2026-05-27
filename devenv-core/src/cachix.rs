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
use std::sync::{Arc, OnceLock};
use tokio::sync::OnceCell;
use tracing::{debug, warn};

/// Name of the environment variable holding the Cachix auth token, and
/// the default secretspec secret name when no override is provided.
///
/// The env-read and the child-process env passed to the cachix push
/// daemon both always use this exact name (that's what the cachix CLI
/// reads). Only the secretspec lookup key is overridable, via the
/// `secretspec.cachix_auth_token` option in `devenv.yaml`.
pub const CACHIX_AUTH_TOKEN_ENV: &str = "CACHIX_AUTH_TOKEN";

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
    /// Auth token supplied out of band (e.g. resolved from secretspec),
    /// used when `CACHIX_AUTH_TOKEN` is absent from the environment.
    auth_token_override: Option<String>,
    /// Memoized result of [`CachixManager::resolve_auth_token`]. Resolution
    /// can read and Dhall-evaluate the cachix config from disk, and runs
    /// from several call sites per invocation; cache it once.
    resolved_token: OnceLock<Option<String>>,
}

impl CachixManager {
    /// Create a new CachixManager.
    ///
    /// `auth_token_override` is an optional token from an external secret
    /// store (secretspec); see [`CachixManager::resolve_auth_token`] for
    /// how it slots into the resolution precedence.
    pub fn new(paths: CachixPaths, auth_token_override: Option<String>) -> Self {
        Self {
            paths,
            netrc_path: Arc::new(OnceCell::new()),
            auth_token_override,
            resolved_token: OnceLock::new(),
        }
    }

    /// Resolve the Cachix auth token used for authenticating pulls
    /// (netrc) and pushes (the daemon subprocess env).
    ///
    /// Precedence:
    /// 1. `CACHIX_AUTH_TOKEN` environment variable (non-empty).
    /// 2. A token supplied out of band (secretspec) via [`CachixManager::new`].
    /// 3. `authToken` from the cachix CLI config (`cachix.dhall`), as
    ///    written by `cachix authtoken`.
    ///
    /// Returns `None` when no source yields a token, in which case
    /// access falls back to unauthenticated (public caches still work).
    ///
    /// The result is memoized: the precedence sources are stable for the
    /// lifetime of an invocation, so we resolve once and reuse it across
    /// the (several) call sites.
    pub fn resolve_auth_token(&self) -> Option<String> {
        self.resolved_token
            .get_or_init(|| self.resolve_auth_token_uncached())
            .clone()
    }

    fn resolve_auth_token_uncached(&self) -> Option<String> {
        if let Ok(token) = env::var(CACHIX_AUTH_TOKEN_ENV)
            && !token.is_empty()
        {
            return Some(token);
        }
        if let Some(token) = self.auth_token_override.as_ref().filter(|t| !t.is_empty()) {
            debug!("cachix: CACHIX_AUTH_TOKEN unset, using token from secretspec");
            return Some(token.clone());
        }
        let token = read_dhall_auth_token();
        if token.is_some() {
            debug!("cachix: CACHIX_AUTH_TOKEN unset, using authToken from cachix config");
        }
        token
    }

    /// Get global Nix settings that must be applied BEFORE store creation
    ///
    /// Returns settings like netrc-file path that need to be in place before
    /// the Nix store makes any HTTP requests.
    pub fn get_global_settings(&self) -> Result<HashMap<String, String>> {
        let mut settings = HashMap::new();

        // If an auth token is available (env, secretspec, or cachix
        // config), set the netrc-file path. The actual file is created
        // later when we know which caches to configure.
        if self.resolve_auth_token().is_some() {
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
        if let Some(auth_token) = self.resolve_auth_token() {
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

    /// Produce the resolved `StoreSettings` derived from this manager's
    /// cachix configuration plus the netrc state already established by
    /// `ensure_netrc_file`.
    ///
    /// `CachixManager` is one possible producer of `StoreSettings`; the
    /// type itself is generic over any source (nix.conf parser, env
    /// overrides, hand-built in tests). The backend consumes a
    /// `StoreSettings`, never a `CachixManager` reference.
    pub async fn store_settings(
        &self,
        cachix_caches: Option<&CachixCacheInfo>,
    ) -> Result<crate::store_settings::StoreSettings> {
        let mut settings = crate::store_settings::StoreSettings::default();

        if let Some(info) = cachix_caches
            && !info.caches.pull.is_empty()
        {
            let nix_settings = self.get_nix_settings(info).await?;
            if let Some(s) = nix_settings.get("extra-substituters") {
                settings.extra_substituters = s.split_whitespace().map(str::to_owned).collect();
            }
            if let Some(k) = nix_settings.get("extra-trusted-public-keys") {
                settings.extra_trusted_public_keys =
                    k.split_whitespace().map(str::to_owned).collect();
            }
        }

        if let Some(path) = self.netrc_path.get() {
            settings.netrc_path = Some(PathBuf::from(path));
        }

        Ok(settings)
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

/// Path to the cachix CLI config, mirroring cachix's own XDG resolution
/// (`$XDG_CONFIG_HOME/cachix/cachix.dhall`, else `$HOME/.config/...`).
fn cachix_config_path() -> Option<PathBuf> {
    xdg::BaseDirectories::new().get_config_file("cachix/cachix.dhall")
}

/// Read and extract `authToken` from the cachix dhall config, if present.
fn read_dhall_auth_token() -> Option<String> {
    let path = cachix_config_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    parse_dhall_auth_token(&content).filter(|t| !t.is_empty())
}

/// Extract `authToken` from the contents of a cachix dhall config.
///
/// Deserializes the record with the Dhall library, reading only the
/// `authToken` field (the `binaryCaches` field and any others are
/// ignored). Returns `None` if the config can't be evaluated or has no
/// string `authToken`, so the caller degrades to unauthenticated access
/// rather than guessing.
fn parse_dhall_auth_token(content: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct CachixDhallConfig {
        #[serde(rename = "authToken")]
        auth_token: String,
    }

    match serde_dhall::from_str(content).parse::<CachixDhallConfig>() {
        Ok(config) => Some(config.auth_token),
        Err(e) => {
            debug!("cachix: could not read authToken from cachix config: {e}");
            None
        }
    }
}

/// Cachix module configuration (from devenv.config.cachix)
#[derive(Deserialize, Default, Clone)]
pub struct CachixConfig {
    pub enable: bool,
    #[serde(flatten)]
    pub caches: Cachix,
    /// Path to the cachix binary
    #[serde(default)]
    pub binary: PathBuf,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact record-literal shape `cachix authtoken` writes: the
    /// value sits on its own line and `binaryCaches` follows.
    #[test]
    fn parses_real_cachix_config_format() {
        let config = "\
{ authToken =
    \"eyJhbGciOiJIUzI1NiJ9.eyJkYXQiOjF9.In3NX31SdYBx3F6b6npo0pvjE3nlMbqn5E8xVGL9M_s\"
, binaryCaches =
  [ { name = \"mycache\"
    , secretKey = \"abc123==\"
    }
  ]
}
";
        assert_eq!(
            parse_dhall_auth_token(config).as_deref(),
            Some("eyJhbGciOiJIUzI1NiJ9.eyJkYXQiOjF9.In3NX31SdYBx3F6b6npo0pvjE3nlMbqn5E8xVGL9M_s")
        );
    }

    #[test]
    fn ignores_other_fields() {
        // Only `authToken` is read; `binaryCaches` (and anything else) is
        // ignored.
        let config = r#"{ authToken = "tok", binaryCaches = [] : List Text }"#;
        assert_eq!(parse_dhall_auth_token(config).as_deref(), Some("tok"));
    }

    #[test]
    fn handles_escaped_quotes_and_backslashes() {
        let config = r#"{ authToken = "a\"b\\c" }"#;
        assert_eq!(parse_dhall_auth_token(config).as_deref(), Some("a\"b\\c"));
    }

    #[test]
    fn evaluates_comments_and_concatenation() {
        // The Dhall library evaluates the expression, so comments and
        // text concatenation are handled, not just literals.
        let config = "{ authToken = {- prefix -} \"to\" ++ \"ken\" -- trailing\n }";
        assert_eq!(parse_dhall_auth_token(config).as_deref(), Some("token"));
    }

    #[test]
    fn rejects_non_string_value() {
        // A non-Text value can't deserialize into the token; degrade to None.
        let config = r#"{ authToken = 42 }"#;
        assert_eq!(parse_dhall_auth_token(config), None);
    }

    #[test]
    fn returns_none_when_field_absent() {
        let config = r#"{ binaryCaches = [] : List Text }"#;
        assert_eq!(parse_dhall_auth_token(config), None);
    }

    #[test]
    fn returns_none_on_invalid_dhall() {
        let config = r#"{ authToken = "unterminated"#;
        assert_eq!(parse_dhall_auth_token(config), None);
    }
}
