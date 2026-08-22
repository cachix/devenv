//! Cachix binary cache integration for devenv.
//!
//! This module handles fetching and configuring Cachix substituters and trusted keys
//! for Nix operations, including authentication token management and API integration.

use miette::{IntoDiagnostic, Result, WrapErr, miette};
use nix_conf_parser::NixConf;
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;
use std::env;
use std::io::ErrorKind;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::OnceCell;
use tracing::{debug, warn};

/// Name of the environment variable holding the Cachix auth token, and
/// the built-in SecretSpec secret name when that requirement is enabled.
///
/// The env-read and the child-process env passed to the cachix push
/// daemon both always use this exact name (that's what the cachix CLI
/// reads). The SecretSpec requirement is enabled, disabled, or renamed via
/// `secretspec.cachix_auth_token` in `devenv.yaml`.
pub const CACHIX_AUTH_TOKEN_ENV: &str = "CACHIX_AUTH_TOKEN";

/// Paths specific to Cachix operations
#[derive(Debug, Clone)]
pub struct CachixPaths {
    pub trusted_keys: PathBuf,
    /// Where the managed netrc is written. It merges the project's Cachix
    /// credentials with whatever Nix already had configured, so callers
    /// should pick a path that is private to this process and outside the
    /// project tree.
    pub netrc: PathBuf,
    /// Optional custom daemon socket path (for testing)
    pub daemon_socket: Option<PathBuf>,
}

/// Manages Cachix binary cache configuration and integration
pub struct CachixManager {
    pub paths: CachixPaths,
    netrc_path: Arc<OnceCell<String>>,
    /// Set after the generated Cachix entries have been written. The netrc
    /// path is initialized earlier, before the Nix store opens, so the
    /// backend can preserve credentials from Nix's existing netrc first.
    netrc_populated: Arc<OnceCell<()>>,
    /// Auth token supplied out of band (e.g. resolved from secretspec), plus
    /// its memoized resolution. Resolution can Dhall-evaluate cachix config,
    /// while a deferred machine-install override must invalidate the result.
    auth_token: Mutex<AuthTokenState>,
    deferred_auth: AtomicBool,
}

struct AuthTokenState {
    override_token: Option<String>,
    resolved: Option<Option<String>>,
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
            netrc_populated: Arc::new(OnceCell::new()),
            auth_token: Mutex::new(AuthTokenState {
                override_token: auth_token_override,
                resolved: None,
            }),
            deferred_auth: AtomicBool::new(false),
        }
    }

    /// Keep the managed netrc available for a token supplied after the Nix
    /// store opens. This is intentionally opt-in for deferred machine installs.
    pub fn enable_deferred_auth(&self) {
        self.deferred_auth.store(true, Ordering::Release);
    }

    /// Replace the out-of-band token and invalidate token resolution.
    ///
    /// Machine installs use this after their execution mode is known: local
    /// installs can then contribute an opportunistic project SecretSpec token
    /// without making target-only installs contact a workstation provider.
    pub fn set_auth_token_override(&self, auth_token_override: Option<String>) {
        let mut state = self.auth_token.lock().unwrap_or_else(|e| e.into_inner());
        state.override_token = auth_token_override;
        state.resolved = None;
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
        let mut state = self.auth_token.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(resolved) = &state.resolved {
            return resolved.clone();
        }
        let resolved = Self::resolve_auth_token_uncached(state.override_token.as_deref());
        state.resolved = Some(resolved.clone());
        resolved
    }

    fn resolve_auth_token_uncached(auth_token_override: Option<&str>) -> Option<String> {
        if let Ok(token) = env::var(CACHIX_AUTH_TOKEN_ENV)
            && !token.is_empty()
        {
            return Some(token);
        }
        if let Some(token) = auth_token_override.filter(|token| !token.is_empty()) {
            debug!("cachix: CACHIX_AUTH_TOKEN unset, using token from secretspec");
            return Some(token.to_string());
        }
        let token = read_dhall_auth_token();
        if token.is_some() {
            debug!("cachix: CACHIX_AUTH_TOKEN unset, using authToken from cachix config");
        }
        token
    }

    /// Ensure the managed netrc file exists and return its path.
    ///
    /// This happens before the Nix store opens. The backend can then copy
    /// credentials from Nix's existing `netrc-file` into this file before
    /// switching the process-global setting to it.
    async fn ensure_netrc_path(&self) -> Result<&String> {
        self.netrc_path
            .get_or_try_init(|| async {
                let netrc_path = self.paths.netrc.clone();
                write_netrc_file(&netrc_path, &[])?;
                Ok(netrc_path.to_string_lossy().to_string())
            })
            .await
    }

    /// Ensure netrc file is created and populated with cache credentials.
    ///
    /// The backend may already have seeded the file with credentials from
    /// Nix's previously configured netrc. Generated entries are written
    /// first so the explicitly resolved Cachix token wins for project caches,
    /// while credentials for unrelated global substituters remain available.
    pub async fn ensure_netrc_file(&self, pull_caches: &[String]) -> Result<()> {
        if let Some(auth_token) = self.resolve_auth_token() {
            let netrc_path = PathBuf::from(self.ensure_netrc_path().await?);
            if !pull_caches.is_empty() {
                self.netrc_populated
                    .get_or_try_init(|| async {
                        self.create_netrc_file(&netrc_path, pull_caches, &auth_token)
                            .await
                    })
                    .await?;
            }
        }
        Ok(())
    }

    /// Get Nix settings (--option flags) needed for Cachix substituters
    ///
    /// Returns a HashMap where keys are Nix option names and values are the option values.
    /// For example: `"extra-substituters" => "https://cache1.cachix.org https://cache2.cachix.org"`
    ///
    /// Note: This returns substituters and keys but NOT netrc-file. The
    /// netrc-file path is carried on [`crate::StoreSettings`] (see
    /// [`CachixManager::store_settings`]) and applied to the Nix settings
    /// registry before the store opens.
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
            // (the netrc-file path is applied before the store opens; see
            // `store_settings`)
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
        let existing_content = match std::fs::read(netrc_path) {
            Ok(content) => content,
            Err(e) if e.kind() == ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                return Err(e).into_diagnostic().wrap_err_with(|| {
                    format!("Failed to read netrc file at {}", netrc_path.display())
                });
            }
        };
        let mut netrc_content = Vec::new();

        for cache in pull_caches {
            netrc_content.extend_from_slice(
                format!("machine {cache}.cachix.org\nlogin token\npassword {auth_token}\n\n")
                    .as_bytes(),
            );
        }
        append_netrc_content(&mut netrc_content, &existing_content);

        write_netrc_file(netrc_path, &netrc_content)
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

        // Initialize and advertise the managed netrc whenever a token is
        // resolvable, or when a machine install opted into deferred auth. In
        // the latter case the already-open store must point at the file that
        // will receive credentials after execution-mode evaluation.
        if let Some(path) = self.netrc_path.get() {
            settings.netrc_path = Some(PathBuf::from(path));
        } else if self.resolve_auth_token().is_some() || self.deferred_auth.load(Ordering::Acquire)
        {
            // A netrc we cannot write costs authentication, not the cache
            // itself: leaving `netrc_path` unset degrades to unauthenticated
            // pulls, whereas failing here would take the substituters
            // resolved above down with it.
            match self.ensure_netrc_path().await {
                Ok(path) => settings.netrc_path = Some(PathBuf::from(path)),
                Err(e) => warn!(
                    error = %e,
                    "cachix: failed to prepare the netrc, private caches may fail to authenticate"
                ),
            }
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

/// Outcome of seeding devenv's managed netrc from Nix's existing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetrcPreservation {
    /// There was nothing to preserve: the source is the managed netrc
    /// itself, or it is missing or empty.
    NothingToPreserve,
    /// The source's credentials are now in the managed netrc.
    Preserved,
    /// The source exists but could not be read, so its credentials are
    /// missing from the managed netrc.
    SourceUnreadable,
}

/// Preserve credentials from Nix's existing netrc in devenv's managed
/// netrc. Existing managed entries come first and therefore retain
/// precedence when both files contain credentials for the same machine.
///
/// A missing source file is treated like an empty netrc, matching Nix's
/// behavior for its default `netrc-file` path. An unreadable one is
/// reported back rather than failing the run, so the caller can decide
/// whether switching Nix over to the managed netrc is still a good trade.
pub fn preserve_netrc_file(source_path: &Path, netrc_path: &Path) -> Result<NetrcPreservation> {
    let same_file = source_path == netrc_path
        || match (source_path.canonicalize(), netrc_path.canonicalize()) {
            (Ok(source), Ok(netrc)) => source == netrc,
            _ => false,
        };
    if same_file {
        return Ok(NetrcPreservation::NothingToPreserve);
    }

    let source_content = match std::fs::read(source_path) {
        Ok(content) => content,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Ok(NetrcPreservation::NothingToPreserve);
        }
        Err(e) => {
            // The source is whatever Nix currently has configured as
            // `netrc-file`, commonly a root-owned 0600 /etc/nix/netrc on a
            // multi-user install. Not being able to read it costs us those
            // credentials; it is not a reason to abort the run.
            warn!(
                path = %source_path.display(),
                error = %e,
                "failed to read the existing netrc, its credentials will not be preserved"
            );
            return Ok(NetrcPreservation::SourceUnreadable);
        }
    };
    if source_content.is_empty() {
        return Ok(NetrcPreservation::NothingToPreserve);
    }

    let mut netrc_content = match std::fs::read(netrc_path) {
        Ok(content) => content,
        Err(e) if e.kind() == ErrorKind::NotFound => Vec::new(),
        Err(e) => {
            return Err(e).into_diagnostic().wrap_err_with(|| {
                format!("Failed to read managed netrc at {}", netrc_path.display())
            });
        }
    };
    append_netrc_content(&mut netrc_content, &source_content);
    write_netrc_file(netrc_path, &netrc_content)?;

    Ok(NetrcPreservation::Preserved)
}

fn append_netrc_content(content: &mut Vec<u8>, appended: &[u8]) {
    if appended.is_empty() {
        return;
    }
    if !content.is_empty() && !content.ends_with(b"\n") {
        content.push(b'\n');
    }
    content.extend_from_slice(appended);
}

/// Distinguishes the scratch files of concurrent writers within a process.
/// Across processes the netrc file name already carries the pid.
static NETRC_TMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Replace the netrc at `netrc_path` with `content`, readable only by its
/// owner.
///
/// Nix reads the netrc lazily, once per HTTP request, and this path is
/// already installed as `netrc-file` while fetches are in flight, so
/// rewriting it in place would let a request observe an empty or truncated
/// file and fall back to unauthenticated. Writing a scratch file and
/// renaming it over the target keeps every read seeing one whole netrc or
/// the other. The scratch file is created with `O_EXCL`, which refuses to
/// follow a symlink planted at its name, and `rename` replaces a symlink at
/// the target instead of writing through it -- both matter because the
/// runtime directory sits on a predictable path.
fn write_netrc_file(netrc_path: &Path, content: &[u8]) -> Result<()> {
    let file_name = netrc_path
        .file_name()
        .ok_or_else(|| miette!("netrc path {} has no file name", netrc_path.display()))?;
    let tmp_path = netrc_path.with_file_name(format!(
        "{}.{}.{}.tmp",
        file_name.to_string_lossy(),
        std::process::id(),
        NETRC_TMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));

    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&tmp_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to create netrc file at {}", tmp_path.display()))?;

    let written = std::io::Write::write_all(&mut file, content)
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to write netrc content to {}", tmp_path.display()));
    drop(file);

    let result = written.and_then(|()| {
        std::fs::rename(&tmp_path, netrc_path)
            .into_diagnostic()
            .wrap_err_with(|| {
                format!(
                    "Failed to move the netrc into place at {}",
                    netrc_path.display()
                )
            })
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    result
}

/// Prefix of the per-process managed netrc file names.
const NETRC_PREFIX: &str = "netrc.";

/// Name of the managed netrc for `process_id` inside `runtime_dir`.
///
/// The file is per-process because concurrent devenv invocations in one
/// project would otherwise overwrite and delete each other's netrc while
/// the other's Nix is still reading it.
pub fn managed_netrc_path(runtime_dir: &Path, process_id: u32) -> PathBuf {
    runtime_dir.join(format!("{NETRC_PREFIX}{process_id}"))
}

/// Remove managed netrc files in `runtime_dir` belonging to devenv
/// processes that have since exited.
///
/// [`CachixManager`] removes its own on drop, but a SIGKILL or an aborting
/// panic leaves behind a file holding the Cachix auth token and a copy of
/// Nix's global netrc, and nothing else ever revisits the runtime directory.
pub fn reap_stale_netrc_files(runtime_dir: &Path) {
    reap_stale_netrc_files_with(runtime_dir, |pid| {
        // ESRCH means no such process; EPERM means it exists but belongs to
        // someone else. Anything other than "definitely gone" keeps the file.
        !matches!(
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None),
            Err(nix::errno::Errno::ESRCH)
        )
    });
}

fn reap_stale_netrc_files_with(runtime_dir: &Path, is_alive: impl Fn(i32) -> bool) {
    let entries = match std::fs::read_dir(runtime_dir) {
        Ok(entries) => entries,
        Err(e) => {
            debug!(
                path = %runtime_dir.display(),
                error = %e,
                "could not list the runtime directory to reap stale netrc files"
            );
            return;
        }
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name
            .to_str()
            .and_then(|name| name.strip_prefix(NETRC_PREFIX))
            .and_then(|pid| pid.parse::<i32>().ok())
        else {
            continue;
        };
        if is_alive(pid) {
            continue;
        }
        match std::fs::remove_file(entry.path()) {
            Ok(()) => debug!(pid, "removed a netrc left behind by an exited devenv"),
            Err(e) => debug!(pid, error = %e, "failed to remove a stale netrc"),
        }
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
    use std::os::unix::fs::PermissionsExt as _;

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

    #[test]
    fn preserves_existing_netrc_after_managed_entries() {
        let root = tempfile::tempdir().unwrap();
        let existing_path = root.path().join("existing-netrc");
        let managed_path = root.path().join("managed-netrc");
        std::fs::write(
            &existing_path,
            "machine global-private.cachix.org\nlogin token\npassword global-token\n",
        )
        .unwrap();
        std::fs::write(
            &managed_path,
            "machine project.cachix.org\nlogin token\npassword project-token\n",
        )
        .unwrap();
        std::fs::set_permissions(&managed_path, std::fs::Permissions::from_mode(0o644)).unwrap();

        assert_eq!(
            preserve_netrc_file(&existing_path, &managed_path).unwrap(),
            NetrcPreservation::Preserved
        );

        assert_eq!(
            std::fs::read_to_string(&managed_path).unwrap(),
            "\
machine project.cachix.org
login token
password project-token
machine global-private.cachix.org
login token
password global-token
"
        );
        assert_eq!(
            std::fs::metadata(&managed_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn preserving_missing_or_same_netrc_is_a_noop() {
        let root = tempfile::tempdir().unwrap();
        let managed_path = root.path().join("managed-netrc");
        let content = "machine project.cachix.org\nlogin token\npassword project-token\n";
        std::fs::write(&managed_path, content).unwrap();

        for source in [root.path().join("missing-netrc"), managed_path.clone()] {
            assert_eq!(
                preserve_netrc_file(&source, &managed_path).unwrap(),
                NetrcPreservation::NothingToPreserve
            );
        }

        assert_eq!(std::fs::read_to_string(&managed_path).unwrap(), content);
    }

    /// Nix commonly points `netrc-file` at a root-owned 0600 `/etc/nix/netrc`
    /// that an unprivileged devenv cannot read. That must degrade to "no
    /// credentials preserved", never fail the run, and it has to be
    /// distinguishable from having nothing to preserve: the caller keeps Nix
    /// on its own netrc rather than trading those credentials away. A
    /// directory stands in for the unreadable source so the test does not
    /// depend on the uid it runs as.
    #[test]
    fn preserving_an_unreadable_netrc_is_reported_not_fatal() {
        let root = tempfile::tempdir().unwrap();
        let managed_path = root.path().join("managed-netrc");
        let content = "machine project.cachix.org\nlogin token\npassword project-token\n";
        std::fs::write(&managed_path, content).unwrap();
        let unreadable_path = root.path().join("unreadable-netrc");
        std::fs::create_dir(&unreadable_path).unwrap();

        assert_eq!(
            preserve_netrc_file(&unreadable_path, &managed_path).unwrap(),
            NetrcPreservation::SourceUnreadable
        );

        assert_eq!(std::fs::read_to_string(&managed_path).unwrap(), content);
    }

    /// The runtime directory holding the managed netrc sits on a path other
    /// local users can predict, so a symlink can be waiting at the netrc's
    /// name. Writing through it would hand them the Cachix auth token and
    /// every credential copied out of Nix's global netrc.
    #[test]
    fn writing_the_netrc_replaces_a_planted_symlink() {
        let root = tempfile::tempdir().unwrap();
        let attacker_path = root.path().join("attacker-owned");
        std::fs::write(&attacker_path, "").unwrap();
        let managed_path = root.path().join("netrc.1");
        std::os::unix::fs::symlink(&attacker_path, &managed_path).unwrap();

        write_netrc_file(&managed_path, b"machine project.cachix.org\n").unwrap();

        assert_eq!(std::fs::read_to_string(&attacker_path).unwrap(), "");
        assert!(
            !std::fs::symlink_metadata(&managed_path)
                .unwrap()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_to_string(&managed_path).unwrap(),
            "machine project.cachix.org\n"
        );
    }

    /// Nix rereads the netrc per request while devenv is rewriting it, so
    /// the file is replaced whole rather than truncated and refilled, and
    /// the scratch file it goes through does not outlive the write.
    #[test]
    fn writing_the_netrc_leaves_no_scratch_file_behind() {
        let root = tempfile::tempdir().unwrap();
        let managed_path = root.path().join("netrc.1");

        write_netrc_file(&managed_path, b"first\n").unwrap();
        write_netrc_file(&managed_path, b"second\n").unwrap();

        assert_eq!(std::fs::read_to_string(&managed_path).unwrap(), "second\n");
        let leftovers: Vec<_> = std::fs::read_dir(root.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name())
            .filter(|name| name != "netrc.1")
            .collect();
        assert!(leftovers.is_empty(), "left behind {leftovers:?}");
    }

    #[test]
    fn reaps_only_netrc_files_of_exited_processes() {
        let root = tempfile::tempdir().unwrap();
        for name in ["netrc.11", "netrc.22", "netrc.notapid", "pc.sock"] {
            std::fs::write(root.path().join(name), "").unwrap();
        }

        reap_stale_netrc_files_with(root.path(), |pid| pid == 11);

        let mut remaining: Vec<_> = std::fs::read_dir(root.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        remaining.sort();
        assert_eq!(remaining, ["netrc.11", "netrc.notapid", "pc.sock"]);
    }

    /// A netrc devenv cannot write costs authentication, not the cache: the
    /// substituters and trusted keys still have to reach the caller, which
    /// applies them as a unit with the netrc path.
    #[tokio::test]
    async fn store_settings_keeps_substituters_when_the_netrc_cannot_be_written() {
        let root = tempfile::tempdir().unwrap();
        let manager = CachixManager::new(
            CachixPaths {
                trusted_keys: root.path().join("trusted-keys.json"),
                // A netrc inside a directory that does not exist.
                netrc: root.path().join("missing-dir/netrc.1"),
                daemon_socket: None,
            },
            Some("project-token".to_string()),
        );
        let info = CachixCacheInfo {
            caches: Cachix {
                pull: vec!["project".to_string()],
                push: None,
            },
            known_keys: BTreeMap::from([("project".to_string(), "project-key".to_string())]),
        };

        let settings = manager.store_settings(Some(&info)).await.unwrap();

        assert_eq!(settings.netrc_path, None);
        assert_eq!(
            settings.extra_substituters,
            ["https://project.cachix.org".to_string()]
        );
        assert_eq!(
            settings.extra_trusted_public_keys,
            ["project-key".to_string()]
        );
    }

    #[tokio::test]
    async fn managed_netrc_keeps_global_credentials_when_project_caches_are_added() {
        let root = tempfile::tempdir().unwrap();
        let existing_path = root.path().join("existing-netrc");
        let managed_path = root.path().join("managed-netrc");
        std::fs::write(
            &existing_path,
            "machine global-private.cachix.org\nlogin token\npassword global-token\n",
        )
        .unwrap();

        let manager = CachixManager::new(
            CachixPaths {
                trusted_keys: root.path().join("trusted-keys.json"),
                netrc: managed_path.clone(),
                daemon_socket: None,
            },
            Some("project-token".to_string()),
        );
        let resolved_token = manager.resolve_auth_token().unwrap();

        let initial = manager.store_settings(None).await.unwrap();
        assert_eq!(initial.netrc_path.as_deref(), Some(managed_path.as_path()));
        assert_eq!(std::fs::read(&managed_path).unwrap(), b"");

        preserve_netrc_file(&existing_path, &managed_path).unwrap();

        let info = CachixCacheInfo {
            caches: Cachix {
                pull: vec!["project".to_string()],
                push: None,
            },
            known_keys: BTreeMap::new(),
        };
        manager.store_settings(Some(&info)).await.unwrap();

        assert_eq!(
            std::fs::read_to_string(&managed_path).unwrap(),
            format!(
                "\
machine project.cachix.org
login token
password {resolved_token}

machine global-private.cachix.org
login token
password global-token
"
            )
        );
        assert_eq!(
            std::fs::metadata(&managed_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        drop(manager);
        assert!(!managed_path.exists());
    }

    #[tokio::test]
    async fn deferred_auth_token_populates_the_netrc_opened_with_the_store() {
        let root = tempfile::tempdir().unwrap();
        let managed_path = root.path().join("managed-netrc");
        let manager = CachixManager::new(
            CachixPaths {
                trusted_keys: root.path().join("trusted-keys.json"),
                netrc: managed_path.clone(),
                daemon_socket: None,
            },
            None,
        );
        manager.enable_deferred_auth();

        let initial = manager.store_settings(None).await.unwrap();
        assert_eq!(initial.netrc_path.as_deref(), Some(managed_path.as_path()));
        assert_eq!(std::fs::read(&managed_path).unwrap(), b"");

        manager.set_auth_token_override(Some("late-project-token".to_string()));
        let info = CachixCacheInfo {
            caches: Cachix {
                pull: vec!["project".to_string()],
                push: None,
            },
            known_keys: BTreeMap::new(),
        };
        manager.store_settings(Some(&info)).await.unwrap();

        assert_eq!(
            std::fs::read_to_string(&managed_path).unwrap(),
            "machine project.cachix.org\nlogin token\npassword late-project-token\n\n"
        );
    }
}
