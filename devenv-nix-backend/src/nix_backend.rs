//! FFI-based NixBackend implementation using direct Rust bindings to Nix C++.
//!
//! This module implements the devenv NixBackend trait using the nixops4 Rust crates
//! which provide FFI bindings to the Nix C++ libraries. This replaces the shell-based
//! approach of spawning `nix` command processes with direct library calls.
//!
//! This version evaluates a plain default.nix file while using resolve-lock.nix to
//! resolve inputs from devenv.lock

use crate::anyhow_ext::AnyhowToMiette;

use async_trait::async_trait;
use include_dir::{Dir, include_dir};
use tokio_shutdown::Shutdown;

use devenv_core::GlobalOptions;
use devenv_core::cachix::{Cachix, CachixCacheInfo, CachixManager};
use devenv_core::config::Config;
use devenv_core::nix_args::NixArgs;
use devenv_core::nix_backend::{DevenvPaths, NixBackend, Options};
use devenv_eval_cache::Output;
use miette::{IntoDiagnostic, Result, WrapErr, miette};
use nix_bindings_expr::eval_state::{EvalState, EvalStateBuilder, gc_register_my_thread};
use nix_bindings_expr::to_json::value_to_json;
use nix_bindings_expr::{EvalCache, SearchParams, SearchResult, search};
use nix_bindings_fetchers::FetchersSettings;
use nix_bindings_flake::{
    EvalStateBuilderExt, FlakeReference, FlakeReferenceParseFlags, FlakeSettings, InputsLocker,
    LockMode,
};
use nix_bindings_store::build_env::BuildEnvironment;
use nix_bindings_store::path::StorePath;
use nix_bindings_store::store::{GcAction, Store, TrustedFlag};
use nix_bindings_util::settings;
use nix_cmd::ReplExitStatus;
use once_cell::sync::OnceCell;
use ser_nix;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Embedded bootstrap directory containing default.nix and resolve-lock.nix
static BOOTSTRAP_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/bootstrap");

/// Specifies where the project root is located
///
/// The project root is used as the base for loading devenv.nix and other configuration.
/// It can be either a local filesystem path or a reference to a flake input.
#[derive(Debug, Clone)]
pub enum ProjectRoot {
    /// A filesystem path (local or absolute)
    /// Creates a "project_root" input in the flake that gets locked and fetched
    /// Examples: PathBuf::from("."), PathBuf::from("/home/user/project")
    Path(PathBuf),

    /// A reference to a flake input, optionally with a subpath
    /// Examples: "nixpkgs", "myinput/subdir"
    /// The referenced input must already exist in devenv.yaml inputs
    InputRef(String),
}

impl Default for ProjectRoot {
    fn default() -> Self {
        ProjectRoot::Path(PathBuf::from("."))
    }
}

/// Package information extracted from nixpkgs attribute set
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PackageInfo {
    pname: String,
    version: String,
    description: String,
}

/// FFI-based Nix backend implementation using direct Rust bindings
pub struct NixRustBackend {
    pub global_options: GlobalOptions,
    pub paths: DevenvPaths,
    pub config: Config,

    // Core Nix FFI components
    #[allow(dead_code)] // May be needed to keep store connection alive
    store: Arc<Store>,
    // EvalState is wrapped in Mutex because Nix evaluation state is not thread-safe.
    // The C++ nix::EvalState is designed for single-threaded use.
    eval_state: Arc<Mutex<EvalState>>,

    // Flake and fetchers settings - created once and reused
    // These must live for the entire duration of the backend
    #[allow(dead_code)]
    flake_settings: FlakeSettings,
    #[allow(dead_code)]
    fetchers_settings: FetchersSettings,

    // Path to extracted bootstrap directory (lives for duration of backend)
    bootstrap_path: PathBuf,

    // Activity logger that forwards Nix events to tracing
    // Must be kept alive for the duration of the backend
    #[allow(dead_code)]
    _activity_logger: nix_bindings_expr::logger::ActivityLogger,

    // Cachix manager for handling binary cache configuration
    cachix_manager: Arc<CachixManager>,

    // Cached Cachix configuration (lazy-loaded on first use)
    cachix_caches: Arc<Mutex<Option<CachixCacheInfo>>>,

    // Cachix daemon for pushing store paths
    // Optional as it's only used when cachix.push is configured
    cachix_daemon: Arc<tokio::sync::Mutex<Option<crate::cachix_daemon::StreamingCachixDaemon>>>,

    // Cached import expression: (import /path/to/default.nix { ... args ... })
    // Set once in assemble() and used directly in evaluations
    cached_import_expr: Arc<OnceCell<String>>,

    // Temp files that must live as long as the backend (e.g., NIXPKGS_CONFIG)
    #[allow(dead_code)]
    _temp_files: Vec<tempfile::NamedTempFile>,

    // GC thread registration guard - must live as long as the backend
    // The Boehm GC requires threads to be registered before using GC-allocated memory.
    #[allow(dead_code)]
    _gc_registration: nix_bindings_expr::eval_state::ThreadRegistrationGuard,

    // Shutdown coordinator - stored so we can trigger shutdown in Drop
    // This ensures cleanup tasks are signaled to exit when the backend is dropped
    shutdown: Arc<Shutdown>,
}

// SAFETY: This is unsafe and relies on several assumptions about the Nix C++ library:
//
// CRITICAL ASSUMPTIONS:
// 1. Store: The C++ nix::Store object and its internal nix::Context must be safe to
//    send between threads and access from multiple threads. We assume the Nix C++ library
//    uses appropriate locking internally for store operations.
//
// 2. EvalState: The C++ nix::EvalState is NOT thread-safe internally. We protect it with
//    a Mutex to ensure only one thread can access it at a time. This relies on the Mutex
//    providing proper synchronization.
//
// KNOWN RISKS:
// - The Nix C++ library is primarily designed for single-threaded CLI usage
// - If the C++ store implementation has thread-local state, this could break
// - If multiple threads trigger evaluation concurrently, the Mutex will serialize access
//   but there's no guarantee the C++ side doesn't have issues with this pattern
//
// RECOMMENDATION:
// - DO NOT use this backend from multiple threads concurrently if you can avoid it
// - Consider creating separate NixRustBackend instances per thread instead
// - This implementation exists to satisfy the NixBackend: Send + Sync trait bound
//
// TODO: Verify these assumptions by:
// 1. Reading Nix C++ source code for Store and EvalState thread safety
// 2. Adding integration tests that exercise concurrent access
// 3. Consider upstreaming Send/Sync impls to nix-store and nix-expr crates with proper safety analysis
unsafe impl Send for NixRustBackend {}
unsafe impl Sync for NixRustBackend {}

impl NixRustBackend {
    /// Create a new NixRustBackend with initialized Nix FFI components with GlobalOptions
    ///
    /// # Arguments
    /// * `paths` - DevenvPaths containing root, dotfile, and cache directories
    /// * `config` - Devenv configuration
    /// * `global_options` - Global Nix options (offline mode, max jobs, etc.)
    /// * `cachix_manager` - CachixManager for handling binary cache configuration
    /// * `shutdown` - Shutdown coordinator for graceful cleanup of cachix daemon
    /// * `store` - Optional custom Nix store path (for testing with restricted permissions)
    pub fn new(
        paths: DevenvPaths,
        config: Config,
        global_options: GlobalOptions,
        cachix_manager: Arc<CachixManager>,
        shutdown: Arc<Shutdown>,
        store: Option<std::path::PathBuf>,
    ) -> Result<Self> {
        // Initialize Nix libexpr FIRST - this initializes the Nix C++ library
        nix_bindings_expr::eval_state::init()
            .to_miette()
            .wrap_err("Failed to initialize Nix expression library")?;

        // Register thread with garbage collector IMMEDIATELY after init()
        // This MUST happen before any other Nix FFI calls (including settings::set)
        // because the Boehm GC requires thread registration before allocations.
        let gc_registration = gc_register_my_thread()
            .to_miette()
            .wrap_err("Failed to register thread with Nix garbage collector")?;

        // Set experimental features after init() to ensure they're properly configured
        settings::set("experimental-features", "flakes nix-command")
            .to_miette()
            .wrap_err("Failed to enable experimental features")?;

        // Apply other global settings
        Self::apply_global_options(&global_options)?;

        // Apply cachix global settings BEFORE store creation (e.g., netrc-file path)
        let cachix_global_settings = cachix_manager
            .get_global_settings()
            .wrap_err("Failed to get cachix global settings")?;

        for (key, value) in &cachix_global_settings {
            settings::set(key, value).to_miette().wrap_err(format!(
                "Failed to set cachix global setting: {} = {}",
                key, value
            ))?;
            tracing::debug!("Applied cachix global setting: {} = {}", key, value);
        }

        // Create flake and fetchers settings - kept alive for the entire backend lifetime
        // These are needed for builtins.fetchTree, builtins.getFlake, and flake operations
        let flake_settings = FlakeSettings::new()
            .to_miette()
            .wrap_err("Failed to create flake settings")?;
        let fetchers_settings = FetchersSettings::new()
            .to_miette()
            .wrap_err("Failed to create fetchers settings")?;

        // Load fetchers settings from nix.conf (access-tokens for GitHub, etc.)
        // Note: load_config() internally calls initLibStore() to ensure global settings are initialized
        fetchers_settings
            .load_config()
            .to_miette()
            .wrap_err("Failed to load fetchers settings from nix.conf")?;

        // Extract bootstrap directory to dotfile location
        let bootstrap_path = Self::extract_bootstrap_files(&paths.dotfile)?;

        // Open store connection (with netrc-file setting now in place)
        let store_uri = store
            .as_ref()
            .map(|p| format!("local?root={}", p.display()));
        let store = Store::open(store_uri.as_deref(), [])
            .to_miette()
            .wrap_err("Failed to open Nix store")?;

        // Generate merged nixpkgs config and write to temp file for NIXPKGS_CONFIG env var
        // Wrap in a let expression that adds allowUnfreePredicate (a Nix function)
        // Note: NIXPKGS_CONFIG expects a file path, not inline Nix content
        let nixpkgs_config = config.nixpkgs_config(&global_options.system);
        let nixpkgs_config_base = ser_nix::to_string(&nixpkgs_config)
            .map_err(|e| miette::miette!("Failed to serialize nixpkgs config: {}", e))?;
        let nixpkgs_config_nix = format!(
            r#"let cfg = {base}; in cfg // {{
  allowUnfreePredicate =
    if cfg.allowUnfree or false then
      (_: true)
    else if (cfg.permittedUnfreePackages or []) != [] then
      (pkg: builtins.elem ((builtins.parseDrvName (pkg.name or pkg.pname or pkg)).name) (cfg.permittedUnfreePackages or []))
    else
      (_: false);
}}"#,
            base = nixpkgs_config_base
        );

        // Write nixpkgs config to a temp file (NIXPKGS_CONFIG expects a file path)
        let mut temp_files = Vec::new();
        let nixpkgs_config_file = tempfile::Builder::new()
            .prefix("devenv-nixpkgs-config-")
            .suffix(".nix")
            .tempfile()
            .map_err(|e| miette::miette!("Failed to create temp file for nixpkgs config: {}", e))?;
        std::fs::write(nixpkgs_config_file.path(), &nixpkgs_config_nix)
            .map_err(|e| miette::miette!("Failed to write nixpkgs config to temp file: {}", e))?;
        let nixpkgs_config_path = nixpkgs_config_file
            .path()
            .to_str()
            .ok_or_else(|| miette::miette!("Nixpkgs config path contains invalid UTF-8"))?
            .to_string();
        temp_files.push(nixpkgs_config_file);
        eprintln!("DEBUG: Creating EvalStateBuilder");

        // Create eval state with flake support and NIXPKGS_CONFIG
        // load_config() loads settings from nix.conf files including access-tokens
        let mut eval_state = EvalStateBuilder::new(store.clone())
            .to_miette()
            .wrap_err("Failed to create eval state builder")?
            .load_config()
            .base_directory(
                paths
                    .root
                    .to_str()
                    .ok_or_else(|| miette::miette!("Root path contains invalid UTF-8"))?,
            )
            .to_miette()
            .wrap_err("Failed to set base directory")?
            .env_override("NIXPKGS_CONFIG", &nixpkgs_config_path)
            .to_miette()
            .wrap_err("Failed to set NIXPKGS_CONFIG environment override")?
            .flakes(&flake_settings)
            .to_miette()
            .wrap_err("Failed to configure flakes in eval state")?;
        eprintln!("DEBUG: Building eval state (this calls loadConfFile)");
        let mut eval_state = eval_state
            .build()
            .to_miette()
            .wrap_err("Failed to build eval state")?;
        eprintln!("DEBUG: Eval state built");

        // Enable Nix debugger if requested
        if global_options.nix_debugger {
            eval_state
                .enable_debugger()
                .to_miette()
                .wrap_err("Failed to enable Nix debugger")?;
        }

        // Set up activity logger integration with tracing
        // MUST be initialized after EvalState is created to ensure Nix is initialized
        let activity_logger =
            crate::logger::setup_nix_logger().wrap_err("Failed to set up activity logger")?;

        let cachix_daemon: Arc<
            tokio::sync::Mutex<Option<crate::cachix_daemon::StreamingCachixDaemon>>,
        > = Arc::new(tokio::sync::Mutex::new(None));

        // Create oneshot channel for cleanup completion signaling
        let (cleanup_tx, cleanup_rx) = tokio::sync::oneshot::channel::<()>();
        shutdown.set_cleanup_receiver(cleanup_rx);

        // Spawn cleanup task that waits for shutdown signal via cancellation token.
        let daemon_for_cleanup = cachix_daemon.clone();
        let shutdown_for_task = shutdown.clone();
        tokio::spawn(async move {
            // Wait for shutdown signal
            shutdown_for_task.cancellation_token().cancelled().await;

            // Cleanup: finalize any queued cachix pushes
            let daemon = {
                let mut guard = daemon_for_cleanup.lock().await;
                guard.take()
            };

            if let Some(daemon) = daemon {
                tracing::info!("Finalizing cachix pushes on shutdown...");
                match daemon.wait_for_completion(Duration::from_secs(300)).await {
                    Ok(metrics) => {
                        tracing::info!("{}", metrics.summary());
                    }
                    Err(e) => {
                        tracing::warn!("Timeout waiting for cachix push completion: {}", e);
                    }
                }
            }

            // Signal cleanup complete (ignore error if receiver dropped)
            let _ = cleanup_tx.send(());
        });

        let backend = Self {
            global_options,
            paths,
            config,
            store: Arc::new(store),
            eval_state: Arc::new(Mutex::new(eval_state)),
            flake_settings,
            fetchers_settings,
            bootstrap_path,
            _activity_logger: activity_logger,
            cachix_manager,
            cachix_caches: Arc::new(Mutex::new(None)),
            cachix_daemon: cachix_daemon.clone(),
            cached_import_expr: Arc::new(OnceCell::new()),
            _temp_files: temp_files,
            _gc_registration: gc_registration,
            shutdown: shutdown.clone(),
        };

        Ok(backend)
    }

    /// Extract embedded bootstrap files to filesystem for Nix to access
    fn extract_bootstrap_files(dotfile_dir: &Path) -> Result<PathBuf> {
        use std::io::Write;

        let bootstrap_path = dotfile_dir.join("bootstrap");
        std::fs::create_dir_all(&bootstrap_path)
            .into_diagnostic()
            .wrap_err("Failed to create bootstrap directory")?;

        // Extract all files from embedded bootstrap directory
        for file in BOOTSTRAP_DIR.files() {
            let target_path = bootstrap_path.join(file.path());

            // Create parent directories if needed
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)
                    .into_diagnostic()
                    .wrap_err("Failed to create parent directories")?;
            }

            let mut output_file = std::fs::File::create(&target_path)
                .into_diagnostic()
                .wrap_err(format!("Failed to create file: {}", target_path.display()))?;

            output_file
                .write_all(file.contents())
                .into_diagnostic()
                .wrap_err(format!("Failed to write file: {}", target_path.display()))?;
        }

        Ok(bootstrap_path)
    }

    /// Get absolute path to a file in the bootstrap directory
    pub fn bootstrap_file(&self, relative_path: &str) -> PathBuf {
        self.bootstrap_path.join(relative_path)
    }

    /// Apply GlobalOptions settings to the Nix environment
    ///
    /// This must be called BEFORE opening the Store or creating EvalState,
    /// as these settings affect how Nix initializes.
    ///
    /// THREAD SAFETY: The underlying Nix settings use global mutable state.
    /// This should only be called during single-threaded initialization.
    fn apply_global_options(global_options: &GlobalOptions) -> Result<()> {
        // Disable flake evaluation cache to avoid stale cache issues
        settings::set("eval-cache", "false")
            .to_miette()
            .wrap_err("Failed to disable eval-cache")?;

        // Always allow substitutes even if they could be built locally
        settings::set("always-allow-substitutes", "true")
            .to_miette()
            .wrap_err("Failed to set always-allow-substitutes")?;

        // Improve parallelism during downloads
        settings::set("http-connections", "100")
            .to_miette()
            .wrap_err("Failed to set http-connections")?;

        // offline mode: disable substituters and set file transfer timeouts
        if global_options.offline {
            settings::set("substituters", "")
                .to_miette()
                .wrap_err("Failed to set offline mode (substituters)")?;

            // Set connection timeout for offline mode (fail fast)
            settings::set("connect-timeout", "1")
                .to_miette()
                .wrap_err("Failed to set connect-timeout for offline mode")?;
        }

        // max_jobs: set maximum concurrent builds
        if global_options.max_jobs > 0 {
            settings::set("max-jobs", &global_options.max_jobs.to_string())
                .to_miette()
                .wrap_err("Failed to set max-jobs")?;
        }

        // cores: set CPU cores available per build
        if global_options.cores > 0 {
            settings::set("cores", &global_options.cores.to_string())
                .to_miette()
                .wrap_err("Failed to set cores")?;
        }

        // system: override the build system architecture
        // Skip default/placeholder values to avoid overriding with nonsense
        if !global_options.system.is_empty()
            && global_options.system != "unknown architecture-unknown OS"
        {
            settings::set("system", &global_options.system)
                .to_miette()
                .wrap_err("Failed to set system")?;
        }

        // impure: allow impure evaluation (relaxes hermeticity)
        if global_options.impure {
            settings::set("impure", "1")
                .to_miette()
                .wrap_err("Failed to set impure mode")?;
        } else {
            // Enable pure evaluation by default when not in impure mode
            // This restricts file system and network access during evaluation
            settings::set("pure-eval", "true")
                .to_miette()
                .wrap_err("Failed to set pure-eval mode")?;
            // Allow local filesystem paths while maintaining other purity guarantees
            // This enables evaluating local devenv.nix files without copying entire repo to store
            settings::set("pure-eval-allow-local-paths", "true")
                .to_miette()
                .wrap_err("Failed to set pure-eval-allow-local-paths")?;
        }

        // nix_option: apply custom Nix configuration pairs
        // These are passed as pairs: ["key1", "value1", "key2", "value2", ...]
        for pair in global_options.nix_option.chunks_exact(2) {
            let key = &pair[0];
            let value = &pair[1];
            settings::set(key, value)
                .to_miette()
                .wrap_err(format!("Failed to set nix option: {key} = {value}"))?;
        }

        Ok(())
    }

    /// Apply Cachix settings from CachixManager to Nix configuration
    ///
    /// This method retrieves cachix configuration and applies substituters and trusted keys
    /// to the Nix environment using the settings API.
    ///
    /// THREAD SAFETY: Like apply_global_options, this uses global Nix settings which are
    /// not thread-safe. Should only be called during initialization.
    async fn apply_cachix_settings(&self) -> Result<()> {
        // Skip cachix settings if offline mode is enabled
        if self.global_options.offline {
            return Ok(());
        }

        let cachix_manager = &self.cachix_manager;

        // Try to get cached cachix config
        let mut cached = self
            .cachix_caches
            .lock()
            .map_err(|e| miette::miette!("Failed to lock cachix cache: {}", e))?;

        let cachix_caches: CachixCacheInfo = if let Some(ref caches) = *cached {
            caches.clone()
        } else {
            // Attempt to load cachix configuration from devenv config
            // Evaluate individual attributes to avoid circular references in the attrset

            // Check if cachix is enabled
            let enable = match self.eval(&["config.cachix.enable"]).await {
                Ok(json) => serde_json::from_str::<bool>(&json).unwrap_or(true),
                Err(e) => {
                    tracing::warn!("Failed to evaluate cachix.enable: {}", e);
                    return Ok(());
                }
            };

            if !enable {
                return Ok(());
            }

            // Get pull caches
            let pull = match self.eval(&["config.cachix.pull"]).await {
                Ok(json) => serde_json::from_str::<Vec<String>>(&json).unwrap_or_default(),
                Err(e) => {
                    tracing::warn!("Failed to evaluate cachix.pull: {}", e);
                    Vec::new()
                }
            };

            // Get push cache
            let push = match self.eval(&["config.cachix.push"]).await {
                Ok(json) => serde_json::from_str::<Option<String>>(&json).unwrap_or(None),
                Err(e) => {
                    tracing::warn!("Failed to evaluate cachix.push: {}", e);
                    None
                }
            };

            // Load known keys from trusted keys file if it exists
            let trusted_keys_path = &cachix_manager.paths.trusted_keys;
            let known_keys = if trusted_keys_path.exists() {
                match std::fs::read_to_string(trusted_keys_path) {
                    Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                    Err(_) => Default::default(),
                }
            } else {
                Default::default()
            };

            let cachix_caches = CachixCacheInfo {
                caches: Cachix { pull, push },
                known_keys,
            };

            *cached = Some(cachix_caches.clone());
            cachix_caches
        };

        // Apply cachix settings via CachixManager
        // Note: netrc-file path was already set in new() before store creation
        match cachix_manager.get_nix_settings(&cachix_caches).await {
            Ok(settings) => {
                // Get mutable store reference for applying settings
                let mut store = (*self.store).clone();

                // Apply substituters via store FFI
                if let Some(extra_substituters) = settings.get("extra-substituters") {
                    let substituters: Vec<&str> = extra_substituters.split_whitespace().collect();
                    for substituter in substituters {
                        if let Err(e) = store.add_substituter(substituter).to_miette() {
                            tracing::warn!("Failed to add substituter {}: {}", substituter, e);
                        } else {
                            tracing::debug!("Added substituter: {}", substituter);
                        }
                    }
                }

                // Apply trusted keys via store FFI
                if let Some(extra_trusted_keys) = settings.get("extra-trusted-public-keys") {
                    let keys: Vec<&str> = extra_trusted_keys.split_whitespace().collect();
                    if !keys.is_empty() {
                        tracing::debug!("Adding {} trusted public keys via store FFI", keys.len());
                        if let Err(e) = store.add_trusted_public_keys(&keys).to_miette() {
                            tracing::warn!("Failed to add trusted public keys: {}", e);
                        } else {
                            tracing::debug!("Added {} trusted public keys", keys.len());
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to get cachix nix settings: {}", e);
            }
        }

        Ok(())
    }

    /// Validate and ensure lock file is up-to-date with the current devenv configuration
    ///
    /// This method:
    /// 1. If lock file doesn't exist, creates it by calling update()
    /// 2. If lock file exists, verifies it's in sync with current configuration
    /// 3. If out of sync, automatically refreshes the locks by calling update()
    ///
    /// This matches the behavior of flakes - locks are automatically created/updated as needed.
    async fn validate_lock_file(&self) -> Result<()> {
        use crate::{create_flake_inputs, load_lock_file};

        let fetch_settings = &self.fetchers_settings;
        let flake_settings = &self.flake_settings;
        let lock_file_path = self.paths.root.join("devenv.lock");

        // If lock file doesn't exist, create it
        if !lock_file_path.exists() {
            return self.update(&None).await;
        }

        // Load existing lock file
        let old_lock = load_lock_file(fetch_settings, &lock_file_path)
            .to_miette()
            .wrap_err("Failed to load lock file")?;

        let old_lock = match old_lock {
            Some(lock) => lock,
            None => {
                // Lock file is invalid/empty - regenerate it
                return self.update(&None).await;
            }
        };

        // Convert devenv inputs to flake inputs
        let flake_inputs = create_flake_inputs(fetch_settings, flake_settings, &self.config)
            .to_miette()
            .wrap_err("Failed to create flake inputs")?;

        let base_dir_str = self
            .paths
            .root
            .to_str()
            .ok_or_else(|| miette!("Root path contains invalid UTF-8"))?;

        // Create a locker in Check mode to validate without modifying
        let locker = InputsLocker::new(flake_settings)
            .with_inputs(flake_inputs)
            .source_path(base_dir_str)
            .old_lock_file(&old_lock)
            .mode(LockMode::Check);

        // Get eval state for locking operation
        let lock_result = {
            let eval_state = self
                .eval_state
                .lock()
                .map_err(|e| miette!("Failed to lock eval state: {}", e))?;

            // Check if locks are in sync
            locker.lock(fetch_settings, &eval_state)
        };

        // Now check the result after the guard is dropped
        if lock_result.is_err() {
            // Locks are out of sync - refresh locks
            return self.update(&None).await;
        }

        Ok(())
    }

    /// Initialize cachix daemon if push is configured
    async fn init_cachix_daemon(&self) -> Result<()> {
        // Check if cachix push is configured in devenv.nix
        match self.eval(&["config.cachix.push"]).await {
            Ok(push_cache_json) => {
                // Parse the push cache name
                if let Ok(push_cache) = serde_json::from_str::<Option<String>>(&push_cache_json) {
                    if push_cache.is_some() {
                        // Start the daemon with config (using custom socket path if provided)
                        tracing::debug!("Starting cachix daemon for push operations");
                        let daemon_config = crate::cachix_daemon::DaemonConfig {
                            socket_path: self.cachix_manager.paths.daemon_socket.clone(),
                            ..Default::default()
                        };
                        match crate::cachix_daemon::StreamingCachixDaemon::start(daemon_config)
                            .await
                        {
                            Ok(daemon) => {
                                let mut handle = self.cachix_daemon.lock().await;
                                *handle = Some(daemon);
                                tracing::info!("Cachix daemon started successfully");
                            }
                            Err(e) => {
                                // Graceful degradation: daemon not available
                                tracing::warn!("Failed to start cachix daemon: {}", e);
                            }
                        }
                    }
                }
            }
            Err(_) => {
                // Cachix push not configured, this is fine
            }
        }

        Ok(())
    }

    /// Queue store paths for real-time pushing to cachix (fire-and-forget)
    ///
    /// This queues paths immediately as they're realized without blocking.
    /// The background daemon will process them asynchronously in parallel with the build.
    ///
    /// Push completion is automatically handled when the backend is dropped.
    ///
    /// This is a fire-and-forget operation - it does not wait for push completion.
    async fn queue_realized_paths(&self, store_paths: &[PathBuf]) -> Result<()> {
        if store_paths.is_empty() {
            return Ok(());
        }

        let path_strings: Vec<String> = store_paths
            .iter()
            .filter_map(|p| p.to_str().map(|s| s.to_string()))
            .collect();

        if path_strings.is_empty() {
            return Ok(());
        }

        // Acquire lock once and queue all paths in a single operation
        let daemon_guard = self.cachix_daemon.lock().await;
        if let Some(daemon) = daemon_guard.as_ref() {
            if let Err(e) = daemon.queue_paths(path_strings).await {
                tracing::warn!("Failed to queue paths to cachix: {}", e);
            }
        }

        Ok(())
    }

    /// Wait for all queued cachix pushes to complete and report results
    /// This does NOT queue any new paths - it only waits for previously queued paths.
    /// Called at the end of operations to ensure all pushes complete before returning.
    ///
    /// This is especially important in async contexts where the Drop handler cannot block_on.
    pub async fn finalize_cachix_push(&self) -> Result<()> {
        // Check if daemon is active
        let has_daemon = self.cachix_daemon.lock().await.as_ref().is_some();

        if !has_daemon {
            return Ok(());
        }

        tracing::debug!("Waiting for cachix push completion");

        // Wait for completion
        let daemon = self.cachix_daemon.lock().await;

        if let Some(daemon_ref) = daemon.as_ref() {
            match daemon_ref
                .wait_for_completion(Duration::from_secs(300))
                .await
            {
                Ok(metrics) => {
                    tracing::info!("{}", metrics.summary());

                    // Report any failures to the user
                    if metrics.failed > 0 {
                        let failed_reasons = metrics.failed_with_reasons.lock().await;
                        if !failed_reasons.is_empty() {
                            tracing::warn!(
                                failed_count = metrics.failed,
                                "Some paths failed to push to cachix:"
                            );
                            for (path, reason) in failed_reasons.iter() {
                                tracing::warn!("  {} - {}", path, reason);
                            }
                        } else {
                            tracing::warn!(
                                failed_count = metrics.failed,
                                "Some paths failed to push to cachix (no details available)"
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Timeout waiting for cachix push completion: {}", e);
                }
            }
        }

        Ok(())
    }
}

#[async_trait(?Send)]
impl NixBackend for NixRustBackend {
    async fn assemble(&self, args: &NixArgs<'_>) -> Result<()> {
        // Cache the import expression if not already set
        if self.cached_import_expr.get().is_none() {
            let default_nix_path = self.bootstrap_file("default.nix");

            let mut args_nix = ser_nix::to_string(args).unwrap_or_else(|_| "{}".to_string());

            // Unquote special Nix expressions that should be evaluated
            args_nix = args_nix.replace("\"builtins.currentSystem\"", "builtins.currentSystem");

            let import_expr = format!(
                "(import {import_path} {args_nix})",
                import_path = default_nix_path.display(),
            );

            self.cached_import_expr.set(import_expr).ok();
        }

        // Validate lock file once during assembly
        // This ensures all subsequent evaluations have a valid lock to work with
        self.validate_lock_file().await?;

        // Apply Cachix settings if a CachixManager is available
        self.apply_cachix_settings().await?;

        // Start cachix daemon if push is configured (skip in offline mode)
        if !self.global_options.offline {
            self.init_cachix_daemon().await?;
        }

        Ok(())
    }

    async fn dev_env(&self, json: bool, gc_root: &Path) -> Result<Output> {
        // Evaluate the devenv shell environment from default.nix
        // This replaces: nix print-dev-env --profile gc_root [--json]

        // Validate lock file before evaluation
        self.validate_lock_file().await?;

        let mut eval_state = self
            .eval_state
            .lock()
            .map_err(|e| miette!("Failed to lock eval state: {}", e))?;

        // Import default.nix with parameters and evaluate to get the devenv structure
        let import_expr = self
            .cached_import_expr
            .get()
            .expect("assemble() must be called first")
            .to_string();

        let devenv = eval_state
            .eval_from_string(&import_expr, self.paths.root.to_str().unwrap())
            .to_miette()
            .wrap_err("Failed to import default.nix")?;

        // Get the shell derivation from devenv.shell
        let shell_drv = eval_state
            .require_attrs_select(&devenv, "shell")
            .to_miette()
            .wrap_err("Failed to get shell attribute from devenv")?;

        // Force evaluation to ensure the derivation is fully evaluated
        eval_state
            .force(&shell_drv)
            .to_miette()
            .wrap_err("Failed to force evaluation of shell derivation")?;

        // Get the .drvPath to find the derivation file
        let drv_path_value = eval_state
            .require_attrs_select(&shell_drv, "drvPath")
            .to_miette()
            .wrap_err("Failed to get drvPath from shell derivation")?;

        let drv_path_str = eval_state
            .require_string(&drv_path_value)
            .to_miette()
            .wrap_err("Failed to extract drvPath as string")?;

        // Build the derivation to ensure it's realized and get the output path
        let realized = eval_state
            .realise_string(&drv_path_value, false)
            .to_miette()
            .wrap_err("Failed to realize shell derivation")?;

        // Create GC root profile with generation tracking
        // This matches the behavior of nix print-dev-env --profile
        if !realized.paths.is_empty() {
            let mut gc_root_paths = Vec::new();
            let mut store = (*self.store).clone();

            for store_path in &realized.paths {
                let path_str = store
                    .real_path(store_path)
                    .to_miette()
                    .wrap_err("Failed to get store path")?;

                gc_root_paths.push(PathBuf::from(&path_str));

                // Create a profile with generation tracking using FFI
                // This creates profile-N-link symlinks like nix-env does
                store
                    .create_generation(gc_root, store_path)
                    .to_miette()
                    .wrap_err("Failed to create profile generation")?;
            }

            // Delete old generations of this profile using FFI
            store
                .delete_old_generations(gc_root, false)
                .to_miette()
                .wrap_err("Failed to delete old generations")?;

            // Queue realized paths immediately for real-time pushing
            self.queue_realized_paths(&gc_root_paths).await?;
        }

        // Release eval_state lock before using FFI
        drop(eval_state);

        // Extract build environment from the derivation store path using FFI
        // Parse the derivation path to get a StorePath
        let mut store = (*self.store).clone();
        let drv_store_path = store
            .parse_store_path(&drv_path_str)
            .to_miette()
            .wrap_err("Failed to parse derivation store path")?;

        // Use the FFI function to get the fully-expanded dev environment
        // This builds a modified derivation that runs setup hooks and captures the result
        let mut build_env = BuildEnvironment::get_dev_environment(&self.store, &drv_store_path)
            .to_miette()
            .wrap_err("Failed to get dev environment from derivation")?;

        // Serialize to the requested format
        let output_str = if json {
            build_env
                .to_json()
                .to_miette()
                .wrap_err("Failed to serialize environment to JSON")?
        } else {
            build_env
                .to_bash()
                .to_miette()
                .wrap_err("Failed to serialize environment to bash")?
        };

        // Return as Output
        use std::os::unix::process::ExitStatusExt;
        let status = ExitStatus::from_raw(0);

        Ok(Output {
            status,
            stdout: output_str.as_bytes().to_vec(),
            stderr: Vec::new(),
            inputs: Vec::new(),
            cache_hit: false,
        })
    }

    async fn repl(&self) -> Result<()> {
        // Initialize the Nix command library (REPL support)
        nix_cmd::init()
            .to_miette()
            .wrap_err("Failed to initialize Nix command library")?;

        // Lock the eval_state for REPL access
        let mut eval_state = self
            .eval_state
            .lock()
            .map_err(|e| miette!("Failed to lock eval state: {}", e))?;

        // Load default.nix into the REPL scope
        let import_expr = self
            .cached_import_expr
            .get()
            .expect("assemble() must be called first")
            .to_string();

        let devenv_attrs = eval_state
            .eval_from_string(&import_expr, self.paths.root.to_str().unwrap())
            .to_miette()
            .wrap_err("Failed to import default.nix for REPL")?;

        // Create a ValMap to inject variables into the REPL scope
        let mut env = nix_cmd::ValMap::new()
            .to_miette()
            .wrap_err("Failed to create REPL environment")?;

        // Inject the devenv configuration as "devenv" variable in REPL
        env.insert("devenv", &devenv_attrs)
            .to_miette()
            .wrap_err("Failed to inject devenv into REPL scope")?;

        // Extract and inject pkgs for convenience
        let pkgs = eval_state
            .require_attrs_select(&devenv_attrs, "pkgs")
            .to_miette()
            .wrap_err("Failed to get pkgs attribute from devenv")?;
        env.insert("pkgs", &pkgs)
            .to_miette()
            .wrap_err("Failed to inject pkgs into REPL scope")?;

        // Run the interactive REPL with pre-populated environment
        // Note: This will block until the user exits with :quit or :continue
        let status = nix_cmd::run_repl_simple(&mut eval_state, Some(&mut env))
            .to_miette()
            .wrap_err("REPL failed")?;

        // Handle the exit status
        match status {
            ReplExitStatus::QuitAll => {
                // User exited with :quit - exit the program
                std::process::exit(0);
            }
            ReplExitStatus::Continue => {
                // User exited with :continue - return normally
                Ok(())
            }
        }
    }

    async fn build(
        &self,
        attributes: &[&str],
        _options: Option<Options>,
        gc_root: Option<&Path>,
    ) -> Result<Vec<PathBuf>> {
        // Build derivations and return output paths
        // Strategy: Evaluate, then build (using the same eval_state for caching)
        //
        // TODO: Use eval() underneath to evaluate first, then build
        // Currently we can't do this because eval() returns JSON string but we need
        // the actual Value to pass to realise_string(). Would need to change the API
        // to either return both Value and JSON, or add an internal eval method that
        // returns Values.

        // Validate lock file before building
        self.validate_lock_file().await?;

        if attributes.is_empty() {
            return Ok(Vec::new());
        }

        // Lock eval_state for the entire operation
        let mut eval_state = self
            .eval_state
            .lock()
            .map_err(|e| miette!("Failed to lock eval state: {}", e))?;

        // Import default.nix to get the attribute set
        let import_expr = self
            .cached_import_expr
            .get()
            .expect("assemble() must be called first")
            .to_string();

        let root_attrs = eval_state
            .eval_from_string(&import_expr, self.paths.root.to_str().unwrap())
            .to_miette()
            .wrap_err("Failed to import default.nix")?;

        let mut output_paths = Vec::new();

        for attr_path in attributes {
            // PHASE 1: Evaluate - Navigate to the attribute and force evaluation
            let value = eval_state
                .require_attrs_select(&root_attrs, attr_path)
                .to_miette()
                .wrap_err(format!(
                    "Failed to get attribute '{attr_path}' from default.nix",
                ))?;

            // Force full evaluation before building
            eval_state
                .force(&value)
                .to_miette()
                .wrap_err(format!("Failed to evaluate attribute: {attr_path}"))?;

            // If it's a derivation (attrs with .outPath), get the outPath
            // Otherwise use the value as-is (might be a string already)
            let build_value = eval_state
                .require_attrs_select_opt(&value, "outPath")
                .to_miette()
                .wrap_err(format!(
                    "Failed to check for outPath in attribute: {attr_path}",
                ))?
                .unwrap_or_else(|| value.clone());

            // PHASE 2: Build - Realize the value, which triggers the actual build
            // realise_string uses the cached evaluation from above
            let realized = eval_state
                .realise_string(&build_value, false)
                .to_miette()
                .wrap_err(format!("Failed to build attribute: {attr_path}"))?;

            // The realized.paths contains the built store paths
            let mut batch_paths = Vec::new();
            for store_path in realized.paths {
                // Get the full store path (not just the name)
                // We need a mutable reference to Store for real_path()
                let mut store = (*self.store).clone();
                let path_str = store
                    .real_path(&store_path)
                    .to_miette()
                    .wrap_err("Failed to get store path")?;

                let path = PathBuf::from(&path_str);

                // Add GC root if requested
                if let Some(gc_root) = gc_root {
                    // Parse the store path for the FFI call
                    let store_path = store
                        .parse_store_path(&path_str)
                        .to_miette()
                        .wrap_err("Failed to parse store path for GC root")?;

                    // Use FFI to register GC root properly with Nix
                    store
                        .add_perm_root(&store_path, gc_root)
                        .to_miette()
                        .wrap_err("Failed to add GC root")?;
                }

                batch_paths.push(path.clone());
                output_paths.push(path);
            }

            // Queue realized paths immediately for real-time pushing
            if !batch_paths.is_empty() {
                self.queue_realized_paths(&batch_paths).await?;
            }

            // If no paths were returned, try using the realized string directly
            if output_paths.is_empty() && !realized.s.is_empty() {
                let path = PathBuf::from(realized.s);
                self.queue_realized_paths(&[path.clone()]).await?;
                output_paths.push(path);
            }
        }

        Ok(output_paths)
    }

    async fn eval(&self, attributes: &[&str]) -> Result<String> {
        // Evaluate Nix expressions and return JSON
        // Evaluates attributes from default.nix
        // Lock file validation is done once in assemble(), not here

        // Lock the eval_state for the duration of evaluation
        let mut eval_state = self
            .eval_state
            .lock()
            .map_err(|e| miette!("Failed to lock eval state: {}", e))?;

        // Import default.nix once to get the attribute set
        let import_expr = self
            .cached_import_expr
            .get()
            .expect("assemble() must be called first")
            .to_string();

        let root_attrs = eval_state
            .eval_from_string(&import_expr, self.paths.root.to_str().unwrap())
            .to_miette()
            .wrap_err("Failed to import default.nix")?;

        let mut results = Vec::new();

        for attr_path in attributes {
            // Parse attribute path - remove leading ".#" if present
            let clean_path = attr_path.trim_start_matches(".#");

            // Navigate to the attribute using the Nix API
            let value = eval_state
                .require_attrs_select(&root_attrs, clean_path)
                .to_miette()
                .wrap_err(format!(
                    "Failed to get attribute '{attr_path}' from default.nix",
                ))?;

            // Force evaluation
            eval_state
                .force(&value)
                .to_miette()
                .wrap_err("Failed to force evaluation")?;

            // Convert to JSON string
            let json_value = value_to_json(&mut eval_state, &value)
                .to_miette()
                .wrap_err(format!("Failed to convert {attr_path} to JSON"))?;
            let json_str = serde_json::to_string(&json_value)
                .into_diagnostic()
                .wrap_err(format!("Failed to serialize {attr_path} to JSON"))?;

            results.push(json_str);
        }

        // If multiple attributes, wrap in array; if single, return as-is
        if results.len() == 1 {
            Ok(results.into_iter().next().unwrap())
        } else {
            Ok(format!("[{results_str}]", results_str = results.join(",")))
        }
    }

    async fn update(&self, input_name: &Option<String>) -> Result<()> {
        use crate::{create_flake_inputs, load_lock_file, write_lock_file};

        // Use settings created during backend initialization - ensures consistency
        let fetch_settings = &self.fetchers_settings;
        let flake_settings = &self.flake_settings;

        // Convert devenv inputs to flake inputs using Config and base_dir directly
        let flake_inputs = create_flake_inputs(fetch_settings, flake_settings, &self.config)
            .to_miette()
            .wrap_err("Failed to create flake inputs")?;

        // Determine lock file path
        let lock_file_path = self.paths.root.join("devenv.lock");

        // Load existing lock file
        let old_lock = load_lock_file(fetch_settings, &lock_file_path)
            .to_miette()
            .wrap_err("Failed to load lock file: {}")?;

        // Lock the inputs using InputsLocker directly
        let base_dir_str = self
            .paths
            .root
            .to_str()
            .ok_or_else(|| miette!("Root path contains invalid UTF-8"))?;

        let mut locker = InputsLocker::new(flake_settings)
            .with_inputs(flake_inputs)
            .source_path(base_dir_str)
            .mode(LockMode::Virtual);

        // Set the old lock file if provided
        if let Some(lock) = &old_lock {
            locker = locker.old_lock_file(lock);
        }

        // Mark specific input for update if provided
        if let Some(name) = input_name {
            locker = locker.update_input(name);
        }

        // Apply input overrides from global_options
        // Note: overrides must live until after lock() is called
        let overrides: Vec<(String, FlakeReference)> =
            if !self.global_options.override_input.is_empty() {
                let mut parse_flags = FlakeReferenceParseFlags::new(flake_settings).to_miette()?;
                parse_flags.set_base_directory(base_dir_str).to_miette()?;

                self.global_options
                    .override_input
                    .chunks_exact(2)
                    .map(|pair| {
                        let (override_ref, _) = FlakeReference::parse_with_fragment(
                            fetch_settings,
                            flake_settings,
                            &parse_flags,
                            &pair[1],
                        )?;
                        Ok((pair[0].clone(), override_ref))
                    })
                    .collect::<anyhow::Result<Vec<_>>>()
                    .to_miette()
                    .wrap_err("Failed to parse input overrides")?
            } else {
                Vec::new()
            };

        if !overrides.is_empty() {
            locker = locker.overrides(overrides.iter().map(|(name, ref_)| (name.clone(), ref_)));
        }

        // Get eval state from mutex only when needed for locking
        // This ensures we reuse the same eval state across calls
        let eval_state = self
            .eval_state
            .lock()
            .map_err(|e| miette!("Failed to lock eval state: {}", e))?;

        // Lock the inputs - pass eval_state by reference to avoid cloning
        let lock_file = locker
            .lock(fetch_settings, &eval_state)
            .to_miette()
            .wrap_err("Failed to lock inputs")?;

        // Write the updated lock file
        write_lock_file(&lock_file, &lock_file_path)
            .to_miette()
            .wrap_err("Failed to write lock file")?;

        Ok(())
    }

    async fn metadata(&self) -> Result<String> {
        // Get metadata: list inputs from lock file and evaluate config.info
        use crate::load_lock_file;

        // Validate lock file before reading metadata
        self.validate_lock_file().await?;

        let mut eval_state = self
            .eval_state
            .lock()
            .map_err(|e| miette!("Failed to lock eval state: {}", e))?;

        // PART 1: Format inputs from lock file using new FFI iterator
        let lock_file_path = self.paths.root.join("devenv.lock");
        let inputs_section = if lock_file_path.exists() {
            let lock = load_lock_file(&self.fetchers_settings, &lock_file_path)
                .to_miette()
                .wrap_err("Failed to load lock file")?;

            if let Some(lock_file) = lock {
                Self::format_lock_inputs(&lock_file)?
            } else {
                "Inputs:\n  (no lock file)".to_string()
            }
        } else {
            "Inputs:\n  (no lock file)".to_string()
        };

        // PART 2: Evaluate config.info from default.nix
        let import_expr = self
            .cached_import_expr
            .get()
            .expect("assemble() must be called first")
            .to_string();

        let root_attrs = eval_state
            .eval_from_string(&import_expr, self.paths.root.to_str().unwrap())
            .to_miette()
            .wrap_err("Failed to import default.nix")?;

        let info_json = if let Ok(Some(info_val)) = eval_state
            .require_attrs_select_opt(&root_attrs, "config.info")
            .to_miette()
        {
            // Force evaluation
            eval_state
                .force(&info_val)
                .to_miette()
                .wrap_err("Failed to force evaluation of info attribute")?;

            // Convert to JSON (handles any Nix value type, not just strings)
            match value_to_json(&mut eval_state, &info_val)
                .to_miette()
                .wrap_err("Failed to convert info attribute to JSON")
            {
                Ok(json_value) => serde_json::to_string(&json_value)
                    .into_diagnostic()
                    .wrap_err("Failed to serialize info to JSON")?,
                Err(_) => String::new(),
            }
        } else {
            String::new()
        };

        // Combine outputs to match original devenv format
        if info_json.is_empty() {
            Ok(inputs_section)
        } else {
            Ok(format!("{inputs_section}\n\n{info_json}"))
        }
    }

    async fn search(&self, name: &str, _options: Option<Options>) -> Result<Output> {
        // Search through pkgs from bootstrap/default.nix for packages matching the query
        // Uses the nix search C API which handles recurseForDerivations logic
        // Respects overlays, locked versions, and devenv configuration

        // Validate lock file before searching
        self.validate_lock_file().await?;

        let mut eval_state = self
            .eval_state
            .lock()
            .map_err(|e| miette!("Failed to lock eval state: {}", e))?;

        // Import default.nix to get the configured pkgs with overlays and settings
        let import_expr = self
            .cached_import_expr
            .get()
            .expect("assemble() must be called first")
            .to_string();

        let devenv = eval_state
            .eval_from_string(&import_expr, self.paths.root.to_str().unwrap())
            .to_miette()
            .wrap_err("Failed to import default.nix")?;

        // Extract the pkgs attribute from the devenv output
        let pkgs = eval_state
            .require_attrs_select(&devenv, "pkgs")
            .to_miette()
            .wrap_err("Failed to get pkgs attribute from devenv")?;

        // Create EvalCache for efficient lazy traversal with optional SQLite caching
        let cache = EvalCache::new(&mut eval_state, &pkgs, None)
            .to_miette()
            .wrap_err("Failed to create eval cache for pkgs")?;

        let cursor = cache
            .root()
            .to_miette()
            .wrap_err("Failed to get root cursor from eval cache")?;

        // Configure search with the query pattern (case-insensitive)
        let mut params = SearchParams::new()
            .to_miette()
            .wrap_err("Failed to create search params")?;

        params
            .add_regex(name)
            .to_miette()
            .wrap_err("Failed to add search regex")?;

        // Collect results using the search API
        let mut results: BTreeMap<String, PackageInfo> = BTreeMap::new();
        let max_results = 100;

        search(&cursor, Some(&params), |result: SearchResult| {
            if results.len() >= max_results {
                return false; // Stop searching
            }

            results.insert(
                result.attr_path,
                PackageInfo {
                    pname: result.name,
                    version: result.version,
                    description: result.description,
                },
            );
            true // Continue searching
        })
        .to_miette()
        .wrap_err("Search failed")?;

        // Convert results to JSON
        let json_output = serde_json::to_string(&results)
            .into_diagnostic()
            .wrap_err("Failed to serialize search results")?;

        // Return as Output struct
        use std::os::unix::process::ExitStatusExt;
        let status = ExitStatus::from_raw(0);

        Ok(Output {
            status,
            stdout: json_output.as_bytes().to_vec(),
            stderr: Vec::new(),
            inputs: Vec::new(),
            cache_hit: false,
        })
    }

    async fn gc(&self, paths: Vec<PathBuf>) -> Result<()> {
        // Delete store paths using FFI with closure computation
        // Strategy:
        // 1. Parse filesystem paths to StorePath objects
        // 2. Compute full closure for each path (includes all dependencies)
        // 3. Use collect_garbage with DeleteSpecific to delete the closure

        if paths.is_empty() {
            return Ok(());
        }

        let mut store = (*self.store).clone();

        // Convert filesystem paths to StorePath objects
        let mut store_paths = Vec::new();
        for path in &paths {
            let path_str = path
                .to_str()
                .ok_or_else(|| miette!("Path contains invalid UTF-8: {}", path.display()))?;

            match store.parse_store_path(path_str).to_miette() {
                Ok(store_path) => store_paths.push(store_path),
                Err(_) => {
                    // Not a valid store path, try to remove as regular file/directory
                    let _ = std::fs::remove_file(path).or_else(|_| std::fs::remove_dir_all(path));
                }
            }
        }

        if store_paths.is_empty() {
            return Ok(());
        }

        // Compute full closure for all paths (includes dependencies)
        // flip_direction=false: get dependencies of these paths
        // include_outputs=true: include outputs
        // include_derivers=false: don't include the derivations that created these
        let mut closure = Vec::new();
        for store_path in &store_paths {
            let path_closure = store
                .get_fs_closure(store_path, false, true, false)
                .to_miette()
                .wrap_err("Failed to compute closure for path")?;
            closure.extend(path_closure);
        }

        if closure.is_empty() {
            return Ok(());
        }

        // Delete the entire closure using collect_garbage
        let closure_refs: Vec<&StorePath> = closure.iter().collect();
        let (deleted_paths, bytes_freed) = store
            .collect_garbage(GcAction::DeleteSpecific, Some(&closure_refs), false, 0)
            .to_miette()
            .wrap_err("Failed to collect garbage for closure")?;

        if !deleted_paths.is_empty() {
            let paths_str = deleted_paths
                .iter()
                .filter_map(|p| store.real_path(p).to_miette().ok())
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!(
                "Deleted {num_paths} paths: {paths_str}",
                num_paths = deleted_paths.len()
            );
        }

        if bytes_freed > 0 {
            let mb = bytes_freed / (1024 * 1024);
            if mb > 0 {
                eprintln!("Freed {mb} MB");
            }
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "nix"
    }

    async fn get_bash(&self, refresh_cached_output: bool) -> Result<String> {
        // Get the bash shell executable path for this system
        //
        // Evaluates and builds the bash attribute from default.nix,
        // which comes from locked nixpkgs and respects the system architecture.
        // Caches the result with a GC root at .devenv/bash to avoid repeated builds.

        let gc_root = self.paths.dotfile.join("bash");

        // Try cache first
        if !refresh_cached_output
            && gc_root.exists()
            && let Ok(cached_path) = std::fs::read_link(&gc_root)
        {
            // Verify the path still exists in the store
            if cached_path.exists() {
                let path_str = cached_path.to_string_lossy().to_string();
                return Ok(format!("{path_str}/bin/bash"));
            }
        }

        // Cache miss or refresh requested - use build() which handles everything
        let paths = self
            .build(&["bash"], None, Some(&gc_root))
            .await
            .wrap_err("Failed to build bash attribute from default.nix")?;

        if paths.is_empty() {
            return Err(miette!("No output paths from bash build"));
        }

        // Return the path to the bash executable
        Ok(format!(
            "{store_path}/bin/bash",
            store_path = paths[0].to_string_lossy()
        ))
    }

    async fn is_trusted_user(&self) -> Result<bool> {
        // Check if the current user is trusted by the Nix daemon/store
        // This is used to determine if we can safely add substituters
        let mut store = (*self.store).clone();
        let trust_status = store.is_trusted_client();

        match trust_status {
            TrustedFlag::Trusted => Ok(true),
            TrustedFlag::NotTrusted => Ok(false),
            TrustedFlag::Unknown => Err(miette!(
                "Unable to determine trust status for Nix store (store type may not support trust queries)"
            )),
        }
    }
}

// Helper methods for NixRustBackend
impl NixRustBackend {
    /// Format lock file inputs as a tree structure
    fn format_lock_inputs(lock_file: &nix_bindings_flake::LockFile) -> Result<String> {
        let mut iter = lock_file
            .inputs_iterator()
            .to_miette()
            .wrap_err("Failed to create inputs iterator: {}")?;

        let mut inputs = Vec::new();

        // Collect all inputs
        while iter.next() {
            let attr_path = iter
                .attr_path()
                .to_miette()
                .wrap_err("Failed to get attr path")?;
            let locked_ref = iter
                .locked_ref()
                .to_miette()
                .wrap_err("Failed to get locked ref")?;

            // Only include top-level inputs (no "/" in path)
            if !attr_path.contains('/') {
                inputs.push((attr_path, locked_ref));
            }
        }

        if inputs.is_empty() {
            return Ok("Inputs:\n  (no inputs)".to_string());
        }

        // Sort inputs by name
        inputs.sort_by(|a, b| a.0.cmp(&b.0));

        let mut lines = vec!["Inputs:".to_string()];
        let inputs_len = inputs.len();

        for (idx, (path, ref_str)) in inputs.into_iter().enumerate() {
            let is_last = idx == inputs_len - 1;
            let prefix = if is_last {
                "└───"
            } else {
                "├───"
            };

            // Format brief reference info
            let brief_ref = Self::format_brief_ref(&ref_str);
            lines.push(format!("{prefix}{path}: {brief_ref}"));
        }

        Ok(lines.join("\n"))
    }

    /// Format a flake reference string into a brief display format
    fn format_brief_ref(ref_str: &str) -> String {
        // Parse reference like "github:NixOS/nixpkgs/6a08e6bb..." or "path:/nix/store/..."
        // Return in format "github:NixOS/nixpkgs/6a08e6b"

        if ref_str.is_empty() {
            return String::from("(follows)");
        }

        // Try to shorten the revision if it's a long hash
        if let Some(last_slash_idx) = ref_str.rfind('/') {
            let before_slash = &ref_str[..last_slash_idx];
            let after_slash = &ref_str[last_slash_idx + 1..];

            // If after slash looks like a hash (40+ chars, hex), truncate it
            if after_slash.len() >= 40 && after_slash.chars().all(|c| c.is_ascii_hexdigit()) {
                return format!("{}/{}", before_slash, &after_slash[..7]);
            }
        }

        ref_str.to_string()
    }
}

impl Drop for NixRustBackend {
    fn drop(&mut self) {
        // Trigger shutdown to signal the cleanup task to run and exit.
        // This is sync (doesn't wait), but ensures the task doesn't stay
        // orphaned waiting for a shutdown that never comes.
        // The cleanup task will run its cleanup code and then exit.
        // Callers who want to wait for cleanup should call
        // shutdown.wait_for_shutdown_complete().await before dropping.
        self.shutdown.shutdown();
    }
}
